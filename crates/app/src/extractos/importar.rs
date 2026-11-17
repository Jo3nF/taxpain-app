//! Importación de los extractos leídos ([`Leido`]) a la bóveda, de cualquier banco, formato y
//! divisa, sin duplicar movimientos.
//!
//! - **La cuenta** es su clave y su divisa juntas ([`Extracto::cuenta`] y
//!   [`Extracto::divisa`]): el mismo IBAN en euros y en dólares son dos cuentas, cada una con su
//!   subcuenta. Si el extracto sólo enseña unos dígitos de la cuenta, se casa con la conocida que
//!   encaja (si es una sola); si no, la cuenta queda como provisional y, cuando otro extracto
//!   trae el IBAN completo, se completa esa misma cuenta en vez de crear otra.
//! - **Sin duplicados** entre extractos que se solapan, de cualquier formato (mensuales,
//!   anuales, el PDF y el Excel de la misma cuenta, copias en varias carpetas): cada movimiento
//!   que llega se busca entre los de la cuenta por fecha e importe, y el saldo de después decide
//!   entre movimientos iguales del mismo día, que se cuentan y no se juntan (ver [`emparejar`]).
//! - **Sólo lo que cuadra**: un extracto que no cuadra con sus saldos no entra salvo que se
//!   fuerce; uno sin saldos (listados de tarjeta) entra con un aviso.
//! - **Qué es la cuenta**: las cuentas y los monederos (PayPal…) van a una subcuenta de la 572;
//!   las tarjetas de crédito, a una de la 5201, para conciliar sus compras con las facturas y el
//!   cargo mensual de la cuenta contra ella; las de débito sólo informan, porque sus compras ya
//!   salen una a una en la cuenta.
//! - **Divisas**: los importes se guardan en la divisa de la cuenta, nunca convertidos.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::Serialize;
use sha2::{Digest, Sha256};
use taxpain_core::accounting::AccountCode;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use super::iban::{self, Enmascarada};
use super::{Clase, Cuadre, Extracto, Leido, Movimiento};
use crate::accounting::posting_accounts;
use crate::error::{AppError, Result};

/// Días de margen para casar un movimiento con otro de fecha distinta: un formato da la fecha
/// valor y otro la de la operación, o un cargo se asienta unos días después.
const MARGEN_DIAS: i64 = 5;

/// Avisos de posibles duplicados por extracto, como mucho.
const MAX_AVISOS: usize = 10;

/// Subcuenta madre de las cuentas en moneda extranjera: 573 «Bancos e instituciones de
/// crédito c/c vista, moneda extranjera» (PGC); la 572 es la de las cuentas en euros.
const MONEDA_EXTRANJERA: &str = "573";

/// Subcuenta madre de las tarjetas de crédito: 5201 «Deudas a corto plazo por crédito
/// dispuesto» (PGC). Lo comprado con la tarjeta es una deuda con el banco hasta que se liquida
/// con el cargo mensual en la cuenta (5201 a 572). TODO(VERIFY): hay asesorías que usan la 5208
/// o una 555 (partidas pendientes de aplicación); confirmar con la de la empresa.
const TARJETAS_CREDITO: &str = "5201";

/// `kind` de la cuenta en la bóveda.
pub(crate) fn tipo(c: Clase) -> &'static str {
    match c {
        Clase::Cuenta => "cuenta",
        Clase::TarjetaCredito => "tarjeta_credito",
        Clase::TarjetaDebito => "tarjeta_debito",
        Clase::Monedero => "monedero",
    }
}

fn es_tarjeta(c: Clase) -> bool {
    matches!(c, Clase::TarjetaCredito | Clase::TarjetaDebito)
}

/// Lo que ha pasado con un extracto al importarlo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExtractoImportado {
    /// Para enseñarla: banco, últimos dígitos y divisa («BBVA …1332 EUR»).
    pub cuenta: String,
    /// Cuenta de la bóveda en la que han entrado los movimientos.
    pub cuenta_id: Option<i64>,
    /// Se ha creado la cuenta con este extracto.
    pub cuenta_nueva: bool,
    /// Subcuenta contable (572x, 5201x; vacía si sólo informa).
    pub subcuenta: Option<String>,
    pub formato: String,
    pub clase: Clase,
    pub divisa: String,
    pub desde: Option<Date>,
    pub hasta: Option<Date>,
    /// Movimientos del extracto.
    pub movimientos: usize,
    pub movimientos_nuevos: usize,
    /// Los que ya estaban (de este u otro extracto que se solapa).
    pub ya_estaban: usize,
    /// Movimientos que ya estaban sin saldo y se lo ha puesto este extracto.
    pub saldos_completados: usize,
    /// Cuenta en otra divisa: movimientos a los que el propio extracto ha dado sus euros.
    pub con_contravalor: usize,
    /// El extracto guardado en la lista de importados (para deshacerlo).
    pub extracto_id: Option<i64>,
    /// De los nuevos, los de días que ya contabilizó un diario importado o de un ejercicio
    /// cerrado: se guardan, pero no se concilian.
    pub ya_contabilizados: usize,
    pub cuadre: Cuadre,
    pub importado: bool,
    /// Por qué no se ha importado (vacío si se ha importado).
    pub motivo: String,
    /// Lo que conviene revisar (del lector y de la importación).
    pub avisos: Vec<String>,
}

/// Resultado de importar un fichero de extractos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Resumen {
    pub fichero: String,
    pub extractos: Vec<ExtractoImportado>,
    pub movimientos_nuevos: usize,
    pub ya_estaban: usize,
}

/// Un extracto que se va a importar y, si se saben, los códigos de concepto común de la Norma
/// 43 de sus movimientos (en el mismo orden).
pub(crate) struct Entrada<'a> {
    pub leido: &'a Leido,
    pub codigos: Option<Vec<String>>,
    /// La referencia de cada movimiento es su identificador en el banco (los que llegan por una
    /// conexión con el banco): se guarda y, con él, un movimiento que vuelve a llegar se
    /// reconoce aunque cambien su fecha o su saldo.
    pub ids_externos: bool,
}

/// Aviso de un extracto sin saldos con los que comprobarlo.
pub(crate) const AVISO_SIN_SALDOS: &str = "El extracto no trae saldos: no se ha podido comprobar \
                                           que estén todos los movimientos.";

/// Comprueba un extracto hecho o corregido a mano: lo ordena, completa sus saldos y mira si
/// cuadra, como los que salen de [`super::leer`].
pub fn comprobado(mut extracto: Extracto) -> Leido {
    let cuadre = extracto.cerrar();
    Leido { extracto, cuadre }
}

/// Importa un fichero de extractos de cualquier formato: los Norma 43 y camt con su lector de
/// siempre (que guarda el código de concepto común de cada movimiento) y los demás con
/// [`super::leer_con_lectura`] (con la lectura de IA, si la hay, para los que no reconoce
/// ningún lector). Es la entrada de la carpeta, de las lecturas y del botón de «Banco».
pub fn importar_fichero(
    v: &Vault,
    nombre: &str,
    bytes: &[u8],
    lectura: Option<&crate::lectura_ia::Reading>,
    forzar: bool,
    now: OffsetDateTime,
) -> Result<Resumen> {
    let texto = super::texto_de(&bytes[..bytes.len().min(64 * 1024)]);
    let t = texto.trim_start_matches('\u{feff}').trim_start();
    if crate::carpeta::is_norma43(t.as_bytes()) || (t.starts_with('<') && t.contains("BkToCstmr")) {
        return crate::bank::import_statement_named(v, nombre, bytes, forzar, now);
    }
    let leidos = super::leer_con_lectura(nombre, bytes, lectura)?;
    importar(v, nombre, bytes, &leidos, forzar, now)
}

impl Resumen {
    /// El resultado en una frase («12 movimientos nuevos (3 ya estaban)», y lo que no se
    /// importó y por qué).
    pub fn frase(&self) -> String {
        let mut t = format!(
            "{} {}",
            self.movimientos_nuevos,
            if self.movimientos_nuevos == 1 {
                "movimiento nuevo"
            } else {
                "movimientos nuevos"
            }
        );
        if self.ya_estaban > 0 {
            t.push_str(&format!(" ({} ya estaban)", self.ya_estaban));
        }
        for e in self.extractos.iter().filter(|e| !e.importado) {
            t.push_str(&format!("; {} no se ha importado: {}", e.cuenta, e.motivo));
        }
        t
    }
}

/// Lee un fichero de extractos de cualquier formato ([`super::leer`]) y lo importa
/// ([`importar`]).
pub fn leer_e_importar(
    v: &Vault,
    nombre: &str,
    bytes: &[u8],
    forzar: bool,
    now: OffsetDateTime,
) -> Result<Resumen> {
    let leidos = super::leer(nombre, bytes)?;
    importar(v, nombre, bytes, &leidos, forzar, now)
}

/// Importa los extractos leídos de un fichero, cada uno en su cuenta (por su clave y su
/// divisa), sin duplicar los movimientos que ya estaban. Un extracto que no cuadra no se
/// importa salvo con `forzar`. Todo o nada: si falla algo, no queda nada a medias.
pub fn importar(
    v: &Vault,
    nombre: &str,
    bytes: &[u8],
    leidos: &[Leido],
    forzar: bool,
    now: OffsetDateTime,
) -> Result<Resumen> {
    let entradas: Vec<Entrada<'_>> = leidos
        .iter()
        .map(|leido| Entrada {
            leido,
            codigos: None,
            ids_externos: false,
        })
        .collect();
    importar_entradas(v, nombre, bytes, &entradas, None, forzar, now)
}

/// Como [`importar`], pero en una cuenta que elige el usuario (extractos que no dicen de qué
/// cuenta son). La cuenta ha de ser de la misma divisa que los extractos.
pub fn importar_en_cuenta(
    v: &Vault,
    nombre: &str,
    bytes: &[u8],
    leidos: &[Leido],
    cuenta_id: i64,
    forzar: bool,
    now: OffsetDateTime,
) -> Result<Resumen> {
    let entradas: Vec<Entrada<'_>> = leidos
        .iter()
        .map(|leido| Entrada {
            leido,
            codigos: None,
            ids_externos: false,
        })
        .collect();
    importar_entradas(v, nombre, bytes, &entradas, Some(cuenta_id), forzar, now)
}

pub(crate) fn importar_entradas(
    v: &Vault,
    nombre: &str,
    bytes: &[u8],
    entradas: &[Entrada<'_>],
    fija: Option<i64>,
    forzar: bool,
    now: OffsetDateTime,
) -> Result<Resumen> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let mut resumen = Resumen {
        fichero: nombre.to_owned(),
        extractos: Vec::new(),
        movimientos_nuevos: 0,
        ya_estaban: 0,
    };
    for en in entradas {
        let r = importar_uno(&tx, nombre, en, fija, forzar, now)?;
        resumen.movimientos_nuevos += r.movimientos_nuevos;
        resumen.ya_estaban += r.ya_estaban;
        resumen.extractos.push(r);
    }
    let detalle: Vec<serde_json::Value> = resumen
        .extractos
        .iter()
        .map(|e| {
            serde_json::json!({
                "cuenta": e.cuenta_id,
                "divisa": e.divisa,
                "formato": e.formato,
                "nuevos": e.movimientos_nuevos,
                "ya_estaban": e.ya_estaban,
                "importado": e.importado,
                "cuadre": e.cuadre,
            })
        })
        .collect();
    audit::append(
        &tx,
        &ahora(now),
        Actor::User,
        "bank.import",
        "extracto",
        &serde_json::json!({
            "fichero": nombre,
            "sha256": hex::encode(Sha256::digest(bytes)),
            "new": resumen.movimientos_nuevos,
            "duplicates": resumen.ya_estaban,
            "forzado": forzar,
            "extractos": detalle,
        }),
    )?;
    tx.commit()?;
    Ok(resumen)
}

fn ahora(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn divisa_de(e: &Extracto) -> String {
    let d = e.divisa.trim().to_uppercase();
    if d.is_empty() { "EUR".to_owned() } else { d }
}

fn importar_uno(
    conn: &Connection,
    nombre: &str,
    en: &Entrada<'_>,
    fija: Option<i64>,
    forzar: bool,
    now: OffsetDateTime,
) -> Result<ExtractoImportado> {
    let e = &en.leido.extracto;
    let mut r = ExtractoImportado {
        cuenta: etiqueta(e),
        cuenta_id: None,
        cuenta_nueva: false,
        subcuenta: None,
        formato: e.formato.clone(),
        clase: e.clase,
        divisa: divisa_de(e),
        desde: e.desde,
        hasta: e.hasta,
        movimientos: e.movimientos.len(),
        movimientos_nuevos: 0,
        ya_estaban: 0,
        saldos_completados: 0,
        con_contravalor: 0,
        extracto_id: None,
        ya_contabilizados: 0,
        cuadre: en.leido.cuadre.clone(),
        importado: false,
        motivo: String::new(),
        avisos: e.avisos.clone(),
    };
    if e.movimientos.is_empty() {
        r.motivo = "no trae movimientos".into();
        return Ok(r);
    }
    match &en.leido.cuadre {
        Cuadre::Cuadra => {}
        Cuadre::SinSaldos => r.avisos.push(AVISO_SIN_SALDOS.into()),
        Cuadre::NoCuadra {
            diferencia,
            detalle,
            ..
        } => {
            if !forzar {
                r.motivo = format!(
                    "no cuadra con sus saldos ({detalle}; diferencia {diferencia}): no se \
                     importa sin confirmarlo"
                );
                return Ok(r);
            }
            r.avisos.push(format!(
                "Importado aunque no cuadra con sus saldos: {detalle}."
            ));
        }
    }
    let cuenta = match fija {
        Some(id) => cuenta_por_id(conn, id, &r.divisa)?,
        None => match resolver(conn, e, &r.divisa, &mut r.avisos)? {
            Some(c) => c,
            None => {
                r.motivo = "el extracto no dice de qué cuenta es: elige la cuenta".into();
                return Ok(r);
            }
        },
    };
    if cuenta.nueva {
        match e.clase {
            Clase::TarjetaCredito => r.avisos.push(format!(
                "Tarjeta de crédito: sus compras se concilian con las facturas contra la \
                 {}, y el cargo mensual en la cuenta del banco, contra esa misma subcuenta.",
                cuenta.account
            )),
            Clase::TarjetaDebito => r.avisos.push(
                "Tarjeta de débito: sus compras ya salen una a una en la cuenta del banco; \
                 se guardan sólo para consultarlas (no se concilian ni se contabilizan)."
                    .into(),
            ),
            Clase::Cuenta | Clase::Monedero => {}
        }
        if r.divisa != "EUR" && e.movimientos.iter().all(|m| m.importe_eur.is_none()) {
            r.avisos.push(format!(
                "Cuenta en {}: los movimientos se guardan en {}; para contabilizarlos hace \
                 falta su contravalor en euros, que todavía se pone a mano.",
                r.divisa, r.divisa
            ));
        }
    }
    let fuente = if nombre == e.formato {
        nombre.to_owned()
    } else {
        format!("{} · {nombre}", e.formato)
    };
    let volcado = volcar(
        conn,
        cuenta.id,
        e,
        en.codigos.as_deref(),
        en.ids_externos,
        &fuente,
        now,
    )?;
    r.cuenta_id = Some(cuenta.id);
    r.cuenta_nueva = cuenta.nueva;
    r.subcuenta = Some(cuenta.account);
    r.movimientos_nuevos = volcado.nuevos;
    r.ya_estaban = volcado.ya_estaban;
    r.saldos_completados = volcado.saldos;
    r.con_contravalor = volcado.con_euros;
    r.avisos.extend(volcado.avisos);
    if volcado.con_euros > 0 {
        r.avisos.push(format!(
            "{} con los euros que da el propio extracto como contravalor (se pueden cambiar en \
             cada movimiento).",
            if volcado.con_euros == 1 {
                "1 movimiento queda".to_owned()
            } else {
                format!("{} movimientos quedan", volcado.con_euros)
            }
        ));
    }
    r.extracto_id = Some(crate::extractos_importados::registrar(
        conn,
        cuenta.id,
        nombre,
        e,
        &volcado.tocados,
        volcado.nuevos,
        now,
    )?);
    // Una fecha futura suele ser un extracto mal leído (día y mes al revés, otro año…).
    let hoy = taxpain_core::spain_time::peninsular_date(now);
    let futuros = e.movimientos.iter().filter(|m| m.fecha > hoy).count();
    if futuros > 0 {
        r.avisos.push(format!(
            "{} con fecha posterior a hoy ({}): comprueba que las fechas del extracto se han \
             leído bien.",
            if futuros == 1 {
                "Hay 1 movimiento".to_owned()
            } else {
                format!("Hay {futuros} movimientos")
            },
            taxpain_core::calendar::reminders::format_es(hoy)
        ));
    }
    if let Some((desde, hasta)) = volcado.ids {
        let n: i64 = conn.query_row(
            &format!(
                "SELECT count(*) FROM bank_tx t WHERE t.bank_account = ?1 AND t.id BETWEEN ?2 AND ?3
                   AND {}",
                crate::bank::BOOKED_ELSEWHERE
            ),
            params![cuenta.id, desde, hasta],
            |x| x.get(0),
        )?;
        r.ya_contabilizados = usize::try_from(n).unwrap_or(0);
        if n > 0 {
            r.avisos.push(format!(
                "{n} de los movimientos nuevos son de días que ya contabilizó un diario importado \
                 o de un ejercicio cerrado: se guardan para consultarlos, sin conciliarlos."
            ));
        }
    }
    r.importado = true;
    Ok(r)
}

/// Nombre de la cuenta para enseñarla: banco (o titular), últimos dígitos y divisa.
fn etiqueta(e: &Extracto) -> String {
    let ult = ultimos(&e.cuenta);
    let quien = [e.banco.trim(), e.titular.trim()]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or("Cuenta");
    let pre = match e.clase {
        Clase::TarjetaCredito => "Tarjeta de crédito ",
        Clase::TarjetaDebito => "Tarjeta de débito ",
        Clase::Cuenta | Clase::Monedero => "",
    };
    let digitos = if ult.is_empty() {
        String::new()
    } else {
        format!(" …{ult}")
    };
    format!("{pre}{quien}{digitos} {}", divisa_de(e))
}

/// Los cuatro últimos dígitos o letras de una clave.
fn ultimos(clave: &str) -> String {
    let c: Vec<char> = clave.chars().filter(char::is_ascii_alphanumeric).collect();
    c[c.len().saturating_sub(4)..].iter().collect()
}

// ---------- Cuentas ----------

/// Cuenta de la bóveda.
#[derive(Clone, Debug)]
struct CuentaBd {
    id: i64,
    number: String,
    iban: Option<String>,
    account: String,
    name: String,
    kind: String,
    bank: String,
}

/// La cuenta en la que entran los movimientos.
struct Resuelta {
    id: i64,
    account: String,
    nueva: bool,
}

impl From<&CuentaBd> for Resuelta {
    fn from(c: &CuentaBd) -> Self {
        Self {
            id: c.id,
            account: c.account.clone(),
            nueva: false,
        }
    }
}

fn cuentas_de(conn: &Connection, divisa: &str) -> Result<Vec<CuentaBd>> {
    let mut stmt = conn.prepare(
        "SELECT id, number, iban, account, name, kind, bank FROM bank_account
         WHERE currency = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([divisa], |r| {
        Ok(CuentaBd {
            id: r.get(0)?,
            number: r.get(1)?,
            iban: r.get(2)?,
            account: r.get(3)?,
            name: r.get(4)?,
            kind: r.get(5)?,
            bank: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn cuenta_por_id(conn: &Connection, id: i64, divisa: &str) -> Result<Resuelta> {
    let (account, currency): (String, String) = conn
        .query_row(
            "SELECT account, currency FROM bank_account WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    if currency != divisa {
        return Err(AppError::Other(format!(
            "la cuenta es en {currency} y el extracto en {divisa}: los importes no se mezclan"
        )));
    }
    Ok(Resuelta {
        id,
        account,
        nueva: false,
    })
}

/// ¿Una cuenta de la bóveda de este tipo puede ser la de un extracto de esta clase? Las
/// tarjetas sólo con tarjetas y las cuentas y monederos, entre ellos.
fn mismo_genero(kind: &str, clase: Clase) -> bool {
    kind.starts_with("tarjeta") == es_tarjeta(clase)
}

/// La cuenta enmascarada de una clave provisional ([`Enmascarada::clave`]).
fn enmascarada_de(clave: &str) -> Option<Enmascarada> {
    let (antes, ultimos) = clave.split_once('*')?;
    if ultimos.is_empty() || !ultimos.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let b = antes.as_bytes();
    let (prefijo, resto) = if b.len() >= 4
        && b[..2].iter().all(u8::is_ascii_uppercase)
        && b[2..4].iter().all(u8::is_ascii_digit)
    {
        (Some(antes[..4].to_owned()), &antes[4..])
    } else {
        (None, antes)
    };
    let entidad =
        (resto.len() == 4 && resto.bytes().all(|b| b.is_ascii_digit())).then(|| resto.to_owned());
    Some(Enmascarada {
        prefijo,
        entidad,
        ultimos: ultimos.to_owned(),
    })
}

/// ¿Pueden ser la misma cuenta dos enmascaradas? Lo que se ve de las dos no se contradice.
fn compatibles(a: &Enmascarada, b: &Enmascarada) -> bool {
    let igual = |x: &Option<String>, y: &Option<String>| match (x, y) {
        (Some(x), Some(y)) => x == y,
        _ => true,
    };
    (a.ultimos.ends_with(&b.ultimos) || b.ultimos.ends_with(&a.ultimos))
        && igual(&a.prefijo, &b.prefijo)
        && igual(&a.entidad, &b.entidad)
}

/// ¿Es una clave que no dice el número de la cuenta («revolut:Main», «hoja:…», «ia:…»,
/// «QIF:…»)? La ponen los lectores cuando el extracto no trae ni el IBAN ni unos dígitos.
pub(crate) fn sin_numero(clave: &str) -> bool {
    let l = clave.to_lowercase();
    ["hoja:", "ia:", "qif:"].iter().any(|p| l.starts_with(p))
        || (!clave.contains('*') && !clave.bytes().any(|b| b.is_ascii_digit()))
}

/// La única cuenta conocida del mismo banco, divisa y tipo que el extracto, si sólo hay una
/// (con número de verdad o, con `sin_numero_tambien`, también las que no lo tienen).
fn unica_del_banco<'a>(
    todas: &'a [CuentaBd],
    e: &Extracto,
    sin_numero_tambien: bool,
) -> Option<&'a CuentaBd> {
    if e.banco.trim().is_empty() {
        return None;
    }
    let mismas: Vec<&CuentaBd> = todas
        .iter()
        .filter(|c| {
            c.bank.eq_ignore_ascii_case(e.banco.trim())
                && mismo_genero(&c.kind, e.clase)
                && (sin_numero_tambien || !sin_numero(&c.number))
        })
        .collect();
    match mismas.as_slice() {
        [c] => Some(c),
        _ => None,
    }
}

/// Entidad y últimos dígitos de una clave de tarjeta (`tarjeta:<entidad>:<últimos>`).
fn tarjeta_de(clave: &str) -> Option<(&str, &str)> {
    let resto = clave.strip_prefix("tarjeta:")?;
    resto.split_once(':')
}

/// IBAN de una clave, si es un IBAN o se deduce de ella (entidad + oficina + cuenta de la
/// Norma 43, con sus dígitos de control).
fn iban_de_clave(clave: &str) -> Option<String> {
    if iban::valido(clave) {
        return Some(iban::compactar(clave));
    }
    if clave.len() == 18 && clave.bytes().all(|b| b.is_ascii_digit()) {
        return taxpain_core::sepa::iban_from_n43(clave).ok();
    }
    None
}

/// Código de la entidad de la cuenta, si se sabe.
fn entidad_de(clave: &str, iban_: Option<&str>) -> Option<String> {
    if let Some((e, _, _)) = iban_.and_then(iban::ccc) {
        return Some(e);
    }
    if let Some((e, _)) = tarjeta_de(clave) {
        return (e != "?").then(|| e.to_owned());
    }
    (clave.len() == 18 && clave.bytes().all(|b| b.is_ascii_digit())).then(|| clave[..4].to_owned())
}

/// Clave de la cuenta del extracto, como la de [`Extracto::con_iban`] aunque el lector haya
/// puesto el IBAN o el código cuenta cliente tal cual (así no sale otra cuenta para la misma).
fn clave_de(e: &Extracto) -> String {
    let k = e.cuenta.trim();
    if k.is_empty() {
        return e
            .iban
            .as_deref()
            .filter(|i| iban::valido(i))
            .map(iban::clave)
            .unwrap_or_default();
    }
    if iban::valido(k) {
        return iban::clave(k);
    }
    match iban::desde_ccc(k) {
        Some(i) => iban::clave(&i),
        None => k.to_owned(),
    }
}

/// Busca (o crea) la cuenta del extracto por su clave y su divisa. `None` si el extracto no
/// trae clave.
fn resolver(
    conn: &Connection,
    e: &Extracto,
    divisa: &str,
    avisos: &mut Vec<String>,
) -> Result<Option<Resuelta>> {
    let normalizada = clave_de(e);
    let clave = normalizada.as_str();
    if clave.is_empty() {
        return Ok(None);
    }
    let todas = cuentas_de(conn, divisa)?;
    if let Some(c) = todas.iter().find(|c| c.number == clave) {
        return usar(conn, c, e, avisos).map(Some);
    }
    if let Some(m) = enmascarada_de(clave) {
        // Sólo se ven unos dígitos: la cuenta conocida que encaja, si es una sola; si no, la
        // provisional que ya haya con lo mismo.
        let enteras: Vec<&CuentaBd> = todas
            .iter()
            .filter(|c| {
                !c.number.contains('*')
                    && !c.number.starts_with("tarjeta:")
                    && mismo_genero(&c.kind, e.clase)
                    && m.casa_con(c.iban.as_deref().unwrap_or(&c.number))
            })
            .collect();
        match enteras.as_slice() {
            [c] => return usar(conn, c, e, avisos).map(Some),
            [] => {
                let provisionales: Vec<&CuentaBd> = todas
                    .iter()
                    .filter(|c| {
                        mismo_genero(&c.kind, e.clase)
                            && enmascarada_de(&c.number).is_some_and(|p| compatibles(&p, &m))
                    })
                    .collect();
                if let [c] = provisionales.as_slice() {
                    return usar(conn, c, e, avisos).map(Some);
                }
            }
            varias => avisos.push(format!(
                "La cuenta …{} puede ser {}: queda aparte, como provisional, hasta que un \
                 extracto traiga su IBAN completo.",
                m.ultimos,
                varias
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" o ")
            )),
        }
        avisos.push(format!(
            "Cuenta provisional …{}: el extracto sólo enseña unos dígitos; cuando importes uno \
             con el IBAN completo, se completará esta misma cuenta.",
            ultimos(clave)
        ));
        return crear(conn, e, clave, divisa, true).map(Some);
    }
    if let Some((entidad, ult)) = tarjeta_de(clave) {
        // Una tarjeta sin entidad conocida es la que haya con esos últimos dígitos (si es una);
        // con entidad, completa la que se importó sin ella.
        let candidatas: Vec<&CuentaBd> = todas
            .iter()
            .filter(|c| {
                tarjeta_de(&c.number).is_some_and(|(ent, u)| {
                    u == ult && (entidad == "?" || ent == "?") && mismo_genero(&c.kind, e.clase)
                })
            })
            .collect();
        if let [c] = candidatas.as_slice() {
            if entidad != "?" {
                mejorar(conn, c, clave, e)?;
            }
            return usar(conn, c, e, avisos).map(Some);
        }
        return crear(conn, e, clave, divisa, false).map(Some);
    }
    // Un extracto que no dice su cuenta va a la única de ese banco en esa divisa, si sólo hay
    // una (Revolut, en las búsquedas filtradas, no pone el IBAN).
    if sin_numero(clave) {
        if let Some(c) = unica_del_banco(&todas, e, false) {
            avisos.push(format!(
                "El extracto no dice de qué cuenta es: se ha importado en {}, la única de {} en \
                 {divisa}.",
                c.name, e.banco
            ));
            return usar(conn, c, e, avisos).map(Some);
        }
        return crear(conn, e, clave, divisa, false).map(Some);
    }
    // Clave entera: si hay una cuenta que entró sin número de ese mismo banco y divisa (y es la
    // única del banco), es esta: se completa.
    if let Some(c) = unica_del_banco(&todas, e, true)
        && sin_numero(&c.number)
    {
        mejorar(conn, c, clave, e)?;
        return usar(conn, c, e, avisos).map(Some);
    }
    // Clave entera: si hay una cuenta provisional que es esta, se completa esa.
    let objetivo = e.iban.as_deref().unwrap_or(clave);
    let provisionales: Vec<&CuentaBd> = todas
        .iter()
        .filter(|c| {
            mismo_genero(&c.kind, e.clase)
                && enmascarada_de(&c.number)
                    .is_some_and(|p| p.casa_con(objetivo) || p.casa_con(clave))
        })
        .collect();
    if let Some((c, resto)) = provisionales.split_first() {
        mejorar(conn, c, clave, e)?;
        if !resto.is_empty() {
            avisos.push(format!(
                "También pueden ser esta cuenta {}: revisa si son la misma.",
                resto
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        return usar(conn, c, e, avisos).map(Some);
    }
    crear(conn, e, clave, divisa, false).map(Some)
}

/// Usa una cuenta que ya estaba: completa su IBAN, banco y titular si no los tenía.
fn usar(
    conn: &Connection,
    c: &CuentaBd,
    e: &Extracto,
    avisos: &mut Vec<String>,
) -> Result<Resuelta> {
    let iban_ = e.iban.clone().or_else(|| iban_de_clave(&c.number));
    conn.execute(
        "UPDATE bank_account SET iban = COALESCE(iban, ?1),
           bank = CASE WHEN bank = '' THEN ?2 ELSE bank END,
           holder = CASE WHEN holder = '' THEN ?3 ELSE holder END
         WHERE id = ?4",
        params![iban_, e.banco.trim(), e.titular.trim(), c.id],
    )?;
    if c.kind != tipo(e.clase) {
        avisos.push(format!(
            "El extracto parece de {} y la cuenta {} está como {}: se deja como estaba.",
            match e.clase {
                Clase::Cuenta => "una cuenta",
                Clase::TarjetaCredito => "una tarjeta de crédito",
                Clase::TarjetaDebito => "una tarjeta de débito",
                Clase::Monedero => "un monedero",
            },
            c.name,
            match c.kind.as_str() {
                "tarjeta_credito" => "tarjeta de crédito",
                "tarjeta_debito" => "tarjeta de débito",
                otro => otro,
            }
        ));
    }
    Ok(Resuelta::from(c))
}

/// Pone la clave entera (y el IBAN) a una cuenta provisional o a una tarjeta sin entidad.
fn mejorar(conn: &Connection, c: &CuentaBd, clave: &str, e: &Extracto) -> Result<()> {
    let iban_ = e.iban.clone().or_else(|| iban_de_clave(clave));
    let nombre = if c.name.ends_with(" (provisional)") {
        etiqueta(e)
    } else {
        c.name.clone()
    };
    conn.execute(
        "UPDATE bank_account SET number = ?1, iban = COALESCE(?2, iban),
           bank_code = COALESCE(?3, bank_code), name = ?4 WHERE id = ?5",
        params![
            clave,
            iban_,
            entidad_de(clave, iban_.as_deref()),
            nombre,
            c.id
        ],
    )?;
    Ok(())
}

/// Primera subcuenta de `madre` que no tiene ya otra cuenta del banco.
fn subcuenta_libre(conn: &Connection, madre: &AccountCode, ancho: usize) -> Result<AccountCode> {
    let mut stmt = conn.prepare("SELECT account FROM bank_account")?;
    let usadas: BTreeSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    (1..=99_999u32)
        .filter_map(|n| madre.subaccount(ancho, n).ok())
        .find(|c| !usadas.contains(c.as_str()))
        .ok_or_else(|| AppError::Other(format!("no quedan subcuentas libres de la {madre}")))
}

fn crear(
    conn: &Connection,
    e: &Extracto,
    clave: &str,
    divisa: &str,
    provisional: bool,
) -> Result<Resuelta> {
    let a = posting_accounts(conn)?;
    let account = match e.clase {
        // Sus compras ya están en la cuenta: no tiene subcuenta, sólo informa.
        Clase::TarjetaDebito => String::new(),
        Clase::TarjetaCredito => {
            let madre =
                AccountCode::new(TARJETAS_CREDITO).map_err(|x| AppError::Other(x.to_string()))?;
            subcuenta_libre(conn, &madre, a.subaccount_width)?
                .as_str()
                .to_owned()
        }
        // TODO(VERIFY): los monederos (PayPal, Stripe…) van a la 572 como una cuenta más; hay
        // quien los lleva a una 5xx propia (cuenta corriente con la pasarela).
        Clase::Cuenta | Clase::Monedero if divisa == "EUR" => {
            // Siempre desde la cuenta de tres dígitos: si la configurada ya es una subcuenta
            // (5720001), derivar de ella daría una de ocho dígitos.
            let base = AccountCode::new(a.bank.as_str().get(..3).unwrap_or("572"))
                .map_err(|x| AppError::Other(x.to_string()))?;
            subcuenta_libre(conn, &base, a.subaccount_width)?
                .as_str()
                .to_owned()
        }
        Clase::Cuenta | Clase::Monedero => {
            let madre =
                AccountCode::new(MONEDA_EXTRANJERA).map_err(|x| AppError::Other(x.to_string()))?;
            subcuenta_libre(conn, &madre, a.subaccount_width)?
                .as_str()
                .to_owned()
        }
    };
    let iban_ = e.iban.clone().or_else(|| iban_de_clave(clave));
    let mut nombre = etiqueta(e);
    if provisional {
        nombre.push_str(" (provisional)");
    }
    conn.execute(
        "INSERT INTO bank_account(iban, bank_code, number, account, name, currency, kind, bank,
           holder) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            iban_,
            entidad_de(clave, iban_.as_deref()),
            clave,
            account,
            nombre,
            divisa,
            tipo(e.clase),
            e.banco.trim(),
            e.titular.trim()
        ],
    )?;
    Ok(Resuelta {
        id: conn.last_insert_rowid(),
        account,
        nueva: true,
    })
}

// ---------- Movimientos ----------

/// Movimiento que ya estaba en la cuenta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fila {
    pub id: i64,
    pub fecha: Date,
    pub valor: Date,
    pub importe: Money,
    pub saldo: Option<Money>,
    pub concepto: String,
    /// Llegó por una conexión con el banco, con su identificador.
    pub externo: bool,
}

fn fecha(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|x| AppError::Other(x.to_string()))
}

/// Movimientos de la cuenta entre dos fechas, en el orden en que se importaron.
fn filas(conn: &Connection, cuenta: i64, desde: Date, hasta: Date) -> Result<Vec<Fila>> {
    let mut stmt = conn.prepare(
        "SELECT id, op_date, value_date, amount, balance, description, external_id IS NOT NULL
         FROM bank_tx WHERE bank_account = ?1 AND op_date BETWEEN ?2 AND ?3 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![cuenta, desde.to_string(), hasta.to_string()], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, bool>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, op, valor, importe, saldo, concepto, externo) = row?;
        let op = fecha(&op)?;
        out.push(Fila {
            id,
            fecha: op,
            valor: fecha(&valor).unwrap_or(op),
            importe: Money::from_cents(importe),
            saldo: saldo.map(Money::from_cents),
            concepto,
            externo,
        });
    }
    Ok(out)
}

/// Las filas de cada día (sus posiciones, en el orden en que se importaron).
type PorFecha = BTreeMap<Date, Vec<usize>>;

fn por_fecha(filas: &[Fila]) -> PorFecha {
    let mut out = PorFecha::new();
    for (j, f) in filas.iter().enumerate() {
        out.entry(f.fecha).or_default().push(j);
    }
    out
}

/// Las filas de los días entre `fecha` − `margen` y `fecha` + `margen`.
fn cerca(indice: &PorFecha, fecha: Date, margen: i64) -> impl Iterator<Item = usize> + '_ {
    let d = time::Duration::days(margen);
    let desde = fecha.checked_sub(d).unwrap_or(fecha);
    let hasta = fecha.checked_add(d).unwrap_or(fecha);
    indice
        .range(desde..=hasta)
        .flat_map(|(_, js)| js.iter().copied())
}

/// La fila libre más cercana en fecha a `fecha` (hasta `margen` días) que cumple `pred`.
fn elegir(
    filas: &[Fila],
    indice: &PorFecha,
    usada: &[bool],
    fecha: Date,
    margen: i64,
    pred: impl Fn(&Fila) -> bool,
) -> Option<usize> {
    cerca(indice, fecha, margen)
        .filter(|j| !usada[*j] && pred(&filas[*j]))
        .min_by_key(|j| (dias(filas[*j].fecha, fecha), *j))
}

fn dias(a: Date, b: Date) -> i64 {
    (a - b).whole_days().abs()
}

/// Empareja los movimientos que llegan con los que ya estaban en la cuenta: para cada uno, la
/// fila que es él mismo (`None` si es nuevo). Cada fila se empareja una sola vez, así que los
/// movimientos iguales del mismo día se cuentan: dos que llegan contra uno que había dan uno
/// nuevo. Por orden de confianza:
///
/// 1. La misma fecha, importe y saldo.
/// 2. Con saldo, la misma fecha e importe de una fila sin saldo (de un extracto sin saldos,
///    como la Norma 43).
/// 3. Sin saldo, la misma fecha e importe.
/// 4. Un día que ya estaba entero pero con otro orden dentro del día (cada formato lo lista a
///    su manera, y los saldos de cada fila cambian): si el saldo al empezar y al acabar el día
///    es el mismo y los importes que faltan por casar están entre los libres de ese día.
/// 5. Con saldo, el mismo importe y saldo unos días antes o después (un formato da la fecha
///    valor y otro la de la operación).
/// 6. La fecha de uno es la fecha valor del otro, con el mismo importe y sin saldos que se
///    contradigan.
pub(crate) fn emparejar(movs: &[Movimiento], filas: &[Fila]) -> Vec<Option<usize>> {
    let mut par: Vec<Option<usize>> = vec![None; movs.len()];
    let mut usada = vec![false; filas.len()];
    let indice = por_fecha(filas);
    let pasada = |par: &mut Vec<Option<usize>>,
                  usada: &mut Vec<bool>,
                  margen: i64,
                  aplica: &dyn Fn(&Movimiento) -> bool,
                  casa: &dyn Fn(&Movimiento, &Fila) -> bool| {
        for (i, m) in movs.iter().enumerate() {
            if par[i].is_some() || !aplica(m) {
                continue;
            }
            if let Some(j) = elegir(filas, &indice, usada, m.fecha, margen, |f| casa(m, f)) {
                par[i] = Some(j);
                usada[j] = true;
            }
        }
    };
    // 1, 2 y 3.
    pasada(&mut par, &mut usada, 0, &|m| m.saldo.is_some(), &|m, f| {
        f.importe == m.importe && f.saldo == m.saldo
    });
    pasada(&mut par, &mut usada, 0, &|m| m.saldo.is_some(), &|m, f| {
        f.importe == m.importe && f.saldo.is_none()
    });
    pasada(&mut par, &mut usada, 0, &|m| m.saldo.is_none(), &|m, f| {
        f.importe == m.importe
    });
    // 4. Días con otro orden.
    let pendientes: BTreeSet<Date> = movs
        .iter()
        .zip(&par)
        .filter(|(m, p)| p.is_none() && m.saldo.is_some())
        .map(|(m, _)| m.fecha)
        .collect();
    for d in pendientes {
        let entra: Vec<usize> = (0..movs.len()).filter(|i| movs[*i].fecha == d).collect();
        let hay: Vec<usize> = indice.get(&d).cloned().unwrap_or_default();
        let extremos_entra = extremos(entra.iter().map(|i| (movs[*i].importe, movs[*i].saldo)));
        let extremos_hay = extremos(hay.iter().map(|j| (filas[*j].importe, filas[*j].saldo)));
        if extremos_entra.is_none() || extremos_entra != extremos_hay {
            continue;
        }
        let mut libres: Vec<usize> = hay.into_iter().filter(|j| !usada[*j]).collect();
        let mut asignadas: Vec<(usize, usize)> = Vec::new();
        let mut todas = true;
        for i in entra.into_iter().filter(|i| par[*i].is_none()) {
            match libres
                .iter()
                .position(|j| filas[*j].importe == movs[i].importe)
            {
                Some(k) => asignadas.push((i, libres.remove(k))),
                None => {
                    todas = false;
                    break;
                }
            }
        }
        if todas {
            for (i, j) in asignadas {
                par[i] = Some(j);
                usada[j] = true;
            }
        }
    }
    // 5 y 6. Con otra fecha.
    pasada(
        &mut par,
        &mut usada,
        MARGEN_DIAS,
        &|m| m.saldo.is_some(),
        &|m, f| f.importe == m.importe && f.saldo == m.saldo,
    );
    pasada(&mut par, &mut usada, MARGEN_DIAS, &|_| true, &|m, f| {
        f.importe == m.importe
            && (m.saldo.is_none() || f.saldo.is_none() || m.saldo == f.saldo)
            && (m.fecha == f.valor || m.fecha_valor == Some(f.fecha))
    });
    par
}

/// Saldo al empezar y al acabar un día, si todos sus movimientos lo traen.
fn extremos(dia: impl Iterator<Item = (Money, Option<Money>)>) -> Option<(Money, Money)> {
    let mut primero: Option<Money> = None;
    let mut ultimo: Option<Money> = None;
    for (importe, saldo) in dia {
        let s = saldo?;
        if primero.is_none() {
            primero = Some(s - importe);
        }
        ultimo = Some(s);
    }
    Some((primero?, ultimo?))
}

/// Lo que ha pasado al volcar los movimientos de un extracto.
struct Volcado {
    nuevos: usize,
    ya_estaban: usize,
    saldos: usize,
    avisos: Vec<String>,
    /// Primer y último `id` de los movimientos nuevos.
    ids: Option<(i64, i64)>,
    /// Los movimientos de la cuenta que trae el extracto: los nuevos y los que ya estaban.
    tocados: Vec<i64>,
    /// Movimientos que han entrado con su contravalor en euros (o se lo ha puesto este
    /// extracto a uno que ya estaba sin él).
    con_euros: usize,
}

/// Huella única del movimiento (`import_hash`): cuenta, fecha, importe, saldo y cuántos iguales
/// había ya.
fn huella(cuenta: i64, m: &Movimiento, n: usize) -> String {
    let saldo = m.saldo.map(|s| s.cents().to_string()).unwrap_or_default();
    hex::encode(Sha256::digest(
        format!(
            "extracto|{cuenta}|{}|{}|{saldo}|{n}",
            m.fecha,
            m.importe.cents()
        )
        .as_bytes(),
    ))
}

/// Un movimiento de la cuenta que llegó con su identificador del banco.
#[derive(Clone, Copy, Debug)]
struct Conocido {
    fila: i64,
    saldo: Option<i64>,
    importe: i64,
}

/// Movimientos de la cuenta con identificador del banco (por identificador; puede haber más de
/// uno si el banco lo reutilizó) y los identificadores de los que el usuario borró.
type Conocidos = (HashMap<String, Vec<Conocido>>, HashSet<String>);

fn conocidos(conn: &Connection, cuenta: i64) -> Result<Conocidos> {
    let mut stmt = conn.prepare(
        "SELECT external_id, id, balance, amount FROM bank_tx
         WHERE bank_account = ?1 AND external_id IS NOT NULL ORDER BY id",
    )?;
    let filas = stmt.query_map([cuenta], |r| {
        Ok((
            r.get::<_, String>(0)?,
            Conocido {
                fila: r.get(1)?,
                saldo: r.get(2)?,
                importe: r.get(3)?,
            },
        ))
    })?;
    let mut out: HashMap<String, Vec<Conocido>> = HashMap::new();
    for f in filas {
        let (k, c) = f?;
        out.entry(k).or_default().push(c);
    }
    let mut stmt =
        conn.prepare("SELECT external_id FROM bank_tx_ignored WHERE bank_account = ?1")?;
    let borrados = stmt
        .query_map([cuenta], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<HashSet<_>, _>>()?;
    Ok((out, borrados))
}

/// Mete los movimientos del extracto en la cuenta sin repetir los que ya estaban.
///
/// Con `ids_externos` (movimientos de una conexión con el banco, con su identificador en la
/// referencia), primero se quitan los repetidos de la misma respuesta y los que el usuario ya
/// borró, y los que ya estaban se reconocen por su identificador (con el mismo importe). Los
/// demás se emparejan ([`emparejar`]) sólo con los que no tienen identificador (de extractos en
/// fichero): dos movimientos iguales del mismo día con identificadores distintos son dos
/// movimientos. Al que casa con uno de fichero se le pone el identificador, para reconocerlo la
/// vez siguiente.
fn volcar(
    conn: &Connection,
    cuenta: i64,
    e: &Extracto,
    codigos: Option<&[String]>,
    ids_externos: bool,
    fuente: &str,
    now: OffsetDateTime,
) -> Result<Volcado> {
    // Cuenta en otra divisa: los euros que dé el extracto son su contravalor.
    let en_divisa: bool = conn.query_row(
        "SELECT currency <> 'EUR' FROM bank_account WHERE id = ?1",
        [cuenta],
        |r| r.get(0),
    )?;
    let mut out = Volcado {
        nuevos: 0,
        ya_estaban: 0,
        saldos: 0,
        avisos: Vec::new(),
        ids: None,
        tocados: Vec::new(),
        con_euros: 0,
    };
    let (Some(min), Some(max)) = (
        e.movimientos.iter().map(|m| m.fecha).min(),
        e.movimientos.iter().map(|m| m.fecha).max(),
    ) else {
        return Ok(out);
    };
    let margen = time::Duration::days(MARGEN_DIAS);
    let existentes = filas(
        conn,
        cuenta,
        min.checked_sub(margen).unwrap_or(min),
        max.checked_add(margen).unwrap_or(max),
    )?;
    let id_de = |m: &Movimiento| -> Option<String> {
        let r = m.referencia.trim();
        (ids_externos && !r.is_empty()).then(|| r.to_owned())
    };
    let (conocidos, borrados) = if ids_externos {
        conocidos(conn, cuenta)?
    } else {
        (HashMap::new(), HashSet::new())
    };
    let posicion: HashMap<i64, usize> = existentes
        .iter()
        .enumerate()
        .map(|(j, f)| (f.id, j))
        .collect();
    let mut usadas = vec![false; existentes.len()];
    // Los resueltos por su identificador: repetidos en la misma respuesta, borrados por el
    // usuario o que ya estaban con el mismo importe.
    let mut por_id = vec![false; e.movimientos.len()];
    let mut primeros: HashMap<String, (Date, Money)> = HashMap::new();
    let mut filas_por_id: HashSet<i64> = HashSet::new();
    for (i, m) in e.movimientos.iter().enumerate() {
        let Some(k) = id_de(m) else {
            continue;
        };
        match primeros.get(&k) {
            Some(x) if *x == (m.fecha, m.importe) => {
                por_id[i] = true;
                out.ya_estaban += 1;
                continue;
            }
            Some(_) => {}
            None => {
                primeros.insert(k.clone(), (m.fecha, m.importe));
            }
        }
        if borrados.contains(&k) {
            por_id[i] = true;
            out.ya_estaban += 1;
            continue;
        }
        let Some(c) = conocidos.get(&k).and_then(|v| {
            v.iter()
                .find(|c| c.importe == m.importe.cents() && !filas_por_id.contains(&c.fila))
                .copied()
        }) else {
            continue;
        };
        por_id[i] = true;
        out.ya_estaban += 1;
        out.tocados.push(c.fila);
        out.con_euros += completar_euros(conn, c.fila, m, en_divisa)?;
        filas_por_id.insert(c.fila);
        if let Some(j) = posicion.get(&c.fila) {
            usadas[*j] = true;
        }
        if let (Some(s), None) = (m.saldo, c.saldo) {
            out.saldos += conn.execute(
                "UPDATE bank_tx SET balance = ?1 WHERE id = ?2 AND balance IS NULL",
                params![s.cents(), c.fila],
            )?;
        }
    }
    // Los demás, por fecha, importe y saldo; con identificadores, sólo contra los que no lo
    // tienen.
    let resto: Vec<usize> = (0..e.movimientos.len()).filter(|i| !por_id[*i]).collect();
    let candidatas: Vec<usize> = (0..existentes.len())
        .filter(|j| !usadas[*j] && !(ids_externos && existentes[*j].externo))
        .collect();
    let pares_resto = emparejar(
        &resto
            .iter()
            .map(|i| e.movimientos[*i].clone())
            .collect::<Vec<_>>(),
        &candidatas
            .iter()
            .map(|j| existentes[*j].clone())
            .collect::<Vec<_>>(),
    );
    let mut pares: Vec<Option<usize>> = vec![None; e.movimientos.len()];
    for (k, i) in resto.iter().enumerate() {
        pares[*i] = pares_resto[k].map(|j| candidatas[j]);
    }
    let indice = por_fecha(&existentes);
    let codigos = codigos.filter(|c| c.len() == e.movimientos.len());
    for j in pares.iter().flatten() {
        usadas[*j] = true;
    }
    let mut parecidos: Vec<String> = Vec::new();
    for (i, (m, par)) in e.movimientos.iter().zip(&pares).enumerate() {
        if por_id[i] {
            continue;
        }
        let externo = id_de(m);
        if let Some(j) = par {
            out.ya_estaban += 1;
            let f = &existentes[*j];
            out.tocados.push(f.id);
            out.con_euros += completar_euros(conn, f.id, m, en_divisa)?;
            if let (Some(s), None) = (m.saldo, f.saldo) {
                out.saldos += conn.execute(
                    "UPDATE bank_tx SET balance = ?1 WHERE id = ?2 AND balance IS NULL",
                    params![s.cents(), f.id],
                )?;
            }
            if let Some(k) = &externo {
                conn.execute(
                    "UPDATE bank_tx SET external_id = ?1 WHERE id = ?2 AND external_id IS NULL",
                    params![k, f.id],
                )?;
            }
            continue;
        }
        let mut n = 0usize;
        let hash = loop {
            let h = huella(cuenta, m, n);
            let existe: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM bank_tx WHERE import_hash = ?1)",
                [&h],
                |r| r.get(0),
            )?;
            if !existe {
                break h;
            }
            n += 1;
        };
        let concepto = if m.concepto.trim().is_empty() {
            m.referencia.trim()
        } else {
            m.concepto.trim()
        };
        let codigo = codigos
            .and_then(|c| c.get(i))
            .map(|c| c.trim())
            .filter(|c| !c.is_empty());
        conn.execute(
            "INSERT INTO bank_tx(bank_account, op_date, value_date, amount, concept_code,
               description, import_hash, created_at, balance, source, external_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                cuenta,
                m.fecha.to_string(),
                m.fecha_valor.unwrap_or(m.fecha).to_string(),
                m.importe.cents(),
                codigo,
                concepto,
                hash,
                ahora(now),
                m.saldo.map(Money::cents),
                fuente,
                externo
            ],
        )?;
        out.nuevos += 1;
        let id = conn.last_insert_rowid();
        out.tocados.push(id);
        out.con_euros += completar_euros(conn, id, m, en_divisa)?;
        out.ids = Some(out.ids.map_or((id, id), |(a, _)| (a, id)));
        // ¿Hay uno parecido que este extracto no trae? Un cargo pendiente que luego cambió de
        // importe (el de una compra en otra divisa) o el mismo con otro saldo.
        if let Some(k) =
            cerca(&indice, m.fecha, 1).find(|k| !usadas[*k] && parecido(m, &existentes[*k]))
        {
            usadas[k] = true;
            let f = &existentes[k];
            parecidos.push(format!(
                "Posible duplicado: el {} entra «{concepto}» ({}) y ya había «{}» ({}) del {}; \
                 si son el mismo movimiento, sobra uno.",
                m.fecha,
                m.importe,
                f.concepto.trim(),
                f.importe,
                f.fecha
            ));
        }
    }
    let mas = parecidos.len().saturating_sub(MAX_AVISOS);
    out.avisos.extend(parecidos.into_iter().take(MAX_AVISOS));
    if mas > 0 {
        out.avisos
            .push(format!("Y {mas} posibles duplicados más como esos."));
    }
    Ok(out)
}

/// Pone a `tx_id` los euros que da el extracto para el movimiento `m`, si es de una cuenta en
/// otra divisa y aún no los tenía (como los tecleados: `manual`; no se tocan los de un movimiento
/// ya conciliado ni los que ya tenía). Devuelve 1 si los ha puesto.
fn completar_euros(
    conn: &Connection,
    tx_id: i64,
    m: &Movimiento,
    en_divisa: bool,
) -> Result<usize> {
    let Some(eur) = m.importe_eur.filter(|e| en_divisa && !e.is_zero()) else {
        return Ok(0);
    };
    let Some(rate) = crate::divisa::implied_rate(m.importe, eur) else {
        return Ok(0);
    };
    let n = conn.execute(
        "UPDATE bank_tx SET amount_eur = ?1, fx_rate = ?2, fx_date = op_date, fx_source = 'manual'
          WHERE id = ?3 AND amount_eur IS NULL AND entry_id IS NULL AND matched_kind IS NULL",
        params![eur.cents(), rate.micros(), tx_id],
    )?;
    Ok(n)
}

/// ¿Se parecen tanto que pueden ser el mismo movimiento? Del mismo día (o el de al lado), en el
/// mismo sentido y con importes que se separan como mucho un 10 %.
fn parecido(m: &Movimiento, f: &Fila) -> bool {
    let (a, b) = (m.importe.cents(), f.importe.cents());
    dias(m.fecha, f.fecha) <= 1
        && a != 0
        && (a < 0) == (b < 0)
        && (a - b).abs() * 10 <= a.abs().max(b.abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn mov(fecha: Date, importe: i64, saldo: Option<i64>) -> Movimiento {
        Movimiento {
            importe_eur: None,
            fecha,
            fecha_valor: None,
            concepto: format!("mov {importe}"),
            importe: Money::from_cents(importe),
            saldo: saldo.map(Money::from_cents),
            referencia: String::new(),
        }
    }

    fn fila(id: i64, fecha: Date, importe: i64, saldo: Option<i64>) -> Fila {
        Fila {
            id,
            fecha,
            valor: fecha,
            importe: Money::from_cents(importe),
            saldo: saldo.map(Money::from_cents),
            concepto: String::new(),
            externo: false,
        }
    }

    const D1: Date = date!(2025 - 05 - 05);
    const D2: Date = date!(2025 - 05 - 06);

    #[test]
    fn iguales_del_mismo_dia_se_cuentan() {
        // Dos retenciones iguales el mismo día, sólo distintas por el saldo.
        let filas = [fila(1, D1, -1_140, Some(98_860))];
        let movs = [mov(D1, -1_140, Some(98_860)), mov(D1, -1_140, Some(97_720))];
        assert_eq!(emparejar(&movs, &filas), [Some(0), None]);
        // Sin saldos, también se cuentan.
        let filas = [fila(1, D1, -500, None)];
        let movs = [mov(D1, -500, None), mov(D1, -500, None)];
        assert_eq!(emparejar(&movs, &filas), [Some(0), None]);
    }

    #[test]
    fn el_saldo_completa_una_fila_sin_saldo() {
        let filas = [fila(1, D1, -500, None), fila(2, D1, -500, None)];
        let movs = [mov(D1, -500, Some(9_500)), mov(D1, -500, Some(9_000))];
        assert_eq!(emparejar(&movs, &filas), [Some(0), Some(1)]);
        // Con otro saldo no es el mismo movimiento.
        let filas = [fila(1, D1, -500, Some(1))];
        assert_eq!(emparejar(&movs[..1], &filas), [None]);
    }

    #[test]
    fn un_dia_listado_en_otro_orden() {
        // Retención, retención, abono, abono en un formato; alternados en el otro.
        let filas = [
            fila(1, D1, -1_140, Some(98_860)),
            fila(2, D1, -1_140, Some(97_720)),
            fila(3, D1, 6_000, Some(103_720)),
            fila(4, D1, 6_000, Some(109_720)),
        ];
        let movs = [
            mov(D1, -1_140, Some(98_860)),
            mov(D1, 6_000, Some(104_860)),
            mov(D1, -1_140, Some(103_720)),
            mov(D1, 6_000, Some(109_720)),
        ];
        let p = emparejar(&movs, &filas);
        assert!(p.iter().all(Option::is_some), "{p:?}");
        // Si el día no acaba igual, lo que sobra es nuevo.
        let movs = [
            mov(D1, -1_140, Some(98_860)),
            mov(D1, 6_000, Some(104_860)),
            mov(D1, -1_140, Some(103_720)),
        ];
        let p = emparejar(&movs, &filas);
        assert_eq!(p.iter().filter(|x| x.is_none()).count(), 2, "{p:?}");
    }

    #[test]
    fn otra_fecha_con_el_mismo_saldo_o_la_fecha_valor() {
        let filas = [fila(1, D1, -2_000, Some(5_000))];
        assert_eq!(
            emparejar(&[mov(D2, -2_000, Some(5_000))], &filas),
            [Some(0)]
        );
        let mut f = fila(1, D1, -2_000, None);
        f.valor = D2;
        assert_eq!(emparejar(&[mov(D2, -2_000, None)], &[f]), [Some(0)]);
        // Otro día, sin saldo ni fecha valor que lo diga: es otro movimiento.
        assert_eq!(
            emparejar(&[mov(D2, -2_000, None)], &[fila(1, D1, -2_000, None)]),
            [None]
        );
    }

    #[test]
    fn claves_enmascaradas() {
        let m = enmascarada_de("ES912100*1332").unwrap();
        assert_eq!(
            (
                m.prefijo.as_deref(),
                m.entidad.as_deref(),
                m.ultimos.as_str()
            ),
            (Some("ES91"), Some("2100"), "1332")
        );
        let n = enmascarada_de("*1332").unwrap();
        assert_eq!((n.prefijo, n.entidad), (None, None));
        assert!(compatibles(&m, &enmascarada_de("ES91*1332").unwrap()));
        assert!(!compatibles(&m, &enmascarada_de("ES92*1332").unwrap()));
        assert_eq!(enmascarada_de("tarjeta:0182:1234"), None);
        assert_eq!(ultimos("tarjeta:0182:1234"), "1234");
    }
}

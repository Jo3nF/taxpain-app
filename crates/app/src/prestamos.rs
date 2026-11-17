//! Préstamos concedidos y recibidos (Contabilidad → Préstamos): la ficha del contrato, su cuadro
//! (`taxpain_core::accounting::prestamo`), el alta, cada cuota (conciliable con el movimiento del
//! banco), el devengo de los intereses a una fecha, la reclasificación del largo al corto plazo
//! al cierre, los avisos fiscales y el contrato en RTF para firmar.
//!
//! - Alta: concedido, crédito (252/542, 2423/5323…) al debe contra la tesorería; recibido,
//!   tesorería contra la deuda (170/520, 171/521, 1633/5133…). La parte que vence en doce meses
//!   va a la cuenta de corto plazo. Si el préstamo ya estaba en el diario (saldos iniciales o el
//!   del gestor), se marca «ya contabilizado» y no se hace asiento.
//! - Cuota: capital, intereses (762/662, menos lo ya devengado, que sale de la 547/528…) y la
//!   retención del 19 % sobre los intereses: la 473 si nos la practican (concedido a una entidad
//!   o empresario obligado a retener) o la 4751 si la practicamos (recibido de un prestamista
//!   residente que no es entidad de crédito: art. 101.4 LIRPF y art. 128.6 LIS). La de la 4751
//!   se ingresa con el 123 del trimestre del vencimiento y va en el 193 (clave B, naturaleza 03):
//!   `models::draft_123` y `models::draft_193` leen [`intereses_pagados`].
//! - Devengo a una fecha (normalmente el cierre): los intereses de la cuota en curso desde el
//!   vencimiento anterior, en proporción a los días (NRV 9.ª: se reconocen por el método del
//!   tipo de interés efectivo; aquí, el nominal). Al registrar la cuota, lo devengado se salda.
//! - Reclasificación al cierre: la cuenta de largo plazo queda con el capital que vence a más de
//!   doce meses según el cuadro (como el leasing, por saldos y agrupando los préstamos que
//!   comparten cuentas).
//!
//! La marca «vinculada» sólo sirve aquí para proponer las subcuentas y avisar del valor de
//! mercado (art. 18 LIS); el registro de operaciones vinculadas y el 232 son otra pantalla.

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::AccountCode;
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::entry::{EntryLine, JournalEntry, Origin};
use taxpain_core::accounting::prestamo::{
    self as calc, Contraparte, Cuentas, Cuota, OrigenContraparte, Prestamo, Sentido, Sistema,
    TipoCarencia, Vinculo,
};
use taxpain_core::money::{Money, Rate};
use taxpain_core::nif::Nif;
use taxpain_invoicing::pdf::Para;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, Row, params};
use time::{Date, OffsetDateTime};

use crate::accounting;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Retención general sobre los intereses (art. 101.4 LIRPF; art. 128.6 LIS).
pub const RETENCION_GENERAL: Rate = Rate::percent(19);

/// Interés legal del dinero, la referencia que se usa para avisar de un tipo muy bajo en un
/// préstamo vinculado (no es el valor de mercado del art. 18 LIS, que depende del caso).
/// Es el de `taxpain_core::parametros::INTERES_LEGAL` (3,25 % de 2023 a 2027: LPGE de 2023 y sus
/// prórrogas, que el Banco de España confirma hasta 2026; 2027 `TODO(VERIFY)`: sin ley de
/// presupuestos a 16-11-2026), que un test compara.
pub const INTERES_LEGAL: Rate = Rate::from_bp(325);

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

fn fecha(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).map_err(other)
}

fn fmt_fecha(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

fn texto<T: serde::de::DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).map_err(other)
}

// ---------- Ficha ----------

const COLS: &str = "id, nombre, sentido, contraparte_origen, contraparte_ref, contraparte_nif,
    contraparte_nombre, contraparte_provincia, persona_fisica, entidad_credito, vinculo, principal,
    fecha_inicio, tipo_bp, sistema, periodicidad_meses, plazo_meses, carencia_meses, carencia_tipo,
    cuenta_largo, cuenta_corto, cuenta_intereses, cuenta_devengo, retencion_bp, notas,
    alta_estado, alta_entry";

/// Estado del alta del préstamo (entrega o recepción del principal).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoAlta {
    /// Aún sin contabilizar.
    Pendiente,
    /// Contabilizada desde aquí.
    Contabilizada,
    /// Ya estaba en el diario (saldos iniciales o el del gestor).
    Fuera,
}

struct Fila {
    p: Prestamo,
    alta: EstadoAlta,
    alta_entry: Option<i64>,
}

fn fila(r: &Row<'_>) -> taxpain_vault::rusqlite::Result<[String; 27]> {
    let mut out: [String; 27] = Default::default();
    for (i, slot) in out.iter_mut().enumerate() {
        let v: taxpain_vault::rusqlite::types::Value = r.get(i)?;
        *slot = match v {
            taxpain_vault::rusqlite::types::Value::Null => String::new(),
            taxpain_vault::rusqlite::types::Value::Integer(n) => n.to_string(),
            taxpain_vault::rusqlite::types::Value::Text(t) => t,
            taxpain_vault::rusqlite::types::Value::Real(f) => f.to_string(),
            taxpain_vault::rusqlite::types::Value::Blob(_) => String::new(),
        };
    }
    Ok(out)
}

fn de_fila(c: &[String; 27]) -> Result<Fila> {
    let int = |i: usize| c[i].parse::<i64>().map_err(other);
    let cuenta = |i: usize| AccountCode::new(&c[i]).map_err(other);
    let p = Prestamo {
        id: int(0)?,
        nombre: c[1].clone(),
        sentido: texto(&c[2])?,
        contraparte: Contraparte {
            origen: texto(&c[3])?,
            ref_id: c[4].parse().ok(),
            nif: c[5].clone(),
            nombre: c[6].clone(),
            provincia: c[7].clone(),
            persona_fisica: int(8)? == 1,
            entidad_credito: int(9)? == 1,
            vinculo: texto(&c[10])?,
        },
        principal: Money::from_cents(int(11)?),
        inicio: fecha(&c[12])?,
        tipo: Rate::from_bp(i32::try_from(int(13)?).map_err(other)?),
        sistema: texto(&c[14])?,
        periodicidad: u8::try_from(int(15)?).map_err(other)?,
        plazo: u16::try_from(int(16)?).map_err(other)?,
        carencia: u16::try_from(int(17)?).map_err(other)?,
        carencia_tipo: texto(&c[18])?,
        cuentas: Cuentas {
            largo: cuenta(19)?,
            corto: cuenta(20)?,
            intereses: cuenta(21)?,
            devengo: cuenta(22)?,
        },
        retencion: Rate::from_bp(i32::try_from(int(23)?).map_err(other)?),
        notas: c[24].clone(),
    };
    Ok(Fila {
        p,
        alta: texto(&c[25])?,
        alta_entry: c[26].parse().ok(),
    })
}

fn cargar(conn: &Connection, id: i64) -> Result<Fila> {
    let c = conn
        .query_row(
            &format!("SELECT {COLS} FROM prestamo WHERE id = ?1"),
            [id],
            fila,
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    de_fila(&c)
}

fn todos(conn: &Connection) -> Result<Vec<Fila>> {
    let mut st = conn.prepare(&format!(
        "SELECT {COLS} FROM prestamo ORDER BY fecha_inicio, id"
    ))?;
    let rows: Vec<[String; 27]> = st
        .query_map([], fila)?
        .collect::<std::result::Result<_, _>>()?;
    rows.iter().map(de_fila).collect()
}

/// Cuentas y retención que se proponen para un préstamo con esa contraparte (el usuario las
/// puede cambiar).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Propuesta {
    pub cuentas: Cuentas,
    pub retencion: Rate,
    /// Por qué esa retención.
    pub motivo: String,
}

/// Subcuentas del PGC de PYMES por sentido y vínculo (…3 grupo, …4 asociadas, …5 otras partes
/// vinculadas) y retención propuesta.
///
/// `TODO(VERIFY)`: retención. Recibido de un prestamista residente que no es entidad de
/// crédito: 19 % (art. 101.4 LIRPF, art. 90 RIRPF; art. 128.6 LIS). Los intereses que cobran
/// las entidades de crédito por sus préstamos no llevan retención (art. 61 RIS; la letra sin
/// leer en el BOE). Concedido: nos retiene el prestatario si es una entidad o un empresario o
/// profesional que paga en su actividad (art. 76 RIRPF, art. 58 RIS); una persona física que no
/// actúa como empresario no retiene.
pub fn propuesta(sentido: Sentido, c: &Contraparte) -> Propuesta {
    let sub = |base: &str| acc(&format!("{base}001"));
    let (largo, corto, devengo, intereses) = match (sentido, c.vinculo) {
        (Sentido::Concedido, Vinculo::Grupo) => (sub("2423"), sub("5323"), acc("5343"), "762"),
        (Sentido::Concedido, Vinculo::Asociada) => (sub("2424"), sub("5324"), acc("5344"), "762"),
        (Sentido::Concedido, Vinculo::Otra) => (sub("2425"), sub("5325"), acc("5345"), "762"),
        (Sentido::Concedido, Vinculo::Ninguno) => (sub("2520"), sub("5420"), acc("547"), "762"),
        (Sentido::Recibido, _) if c.entidad_credito => {
            (sub("1700"), sub("5200"), acc("527"), "662")
        }
        (Sentido::Recibido, Vinculo::Grupo) => (sub("1633"), sub("5133"), acc("5143"), "662"),
        (Sentido::Recibido, Vinculo::Asociada) => (sub("1634"), sub("5134"), acc("5144"), "662"),
        (Sentido::Recibido, Vinculo::Otra) => (sub("1635"), sub("5135"), acc("5145"), "662"),
        (Sentido::Recibido, Vinculo::Ninguno) => (sub("1710"), sub("5210"), acc("528"), "662"),
    };
    let (retencion, motivo) = match sentido {
        Sentido::Recibido if c.entidad_credito => (
            Rate::ZERO,
            "Los intereses que se pagan a un banco no llevan retención.".to_owned(),
        ),
        Sentido::Recibido => (
            RETENCION_GENERAL,
            "La sociedad retiene el 19 % de los intereses que paga (4751): se ingresa con el 123 \
             y va en el 193."
                .to_owned(),
        ),
        Sentido::Concedido if c.persona_fisica => (
            Rate::ZERO,
            "Una persona física que no actúa como empresario no retiene: los intereses se cobran \
             íntegros."
                .to_owned(),
        ),
        Sentido::Concedido if c.entidad_credito => (Rate::ZERO, String::new()),
        Sentido::Concedido => (
            RETENCION_GENERAL,
            "La sociedad prestataria retiene el 19 % de los intereses que paga: va a la 473 \
             (pago a cuenta del Impuesto sobre Sociedades)."
                .to_owned(),
        ),
    };
    Propuesta {
        cuentas: Cuentas {
            largo,
            corto,
            intereses: acc(intereses),
            devengo,
        },
        retencion,
        motivo,
    }
}

fn prefijo(a: &AccountCode, prefijos: &[&str]) -> bool {
    prefijos.iter().any(|p| a.as_str().starts_with(p))
}

/// Comprueba la ficha antes de guardarla.
pub fn comprobar(p: &Prestamo) -> Result<()> {
    calc::comprobar(p).map_err(other)?;
    if p.nombre.trim().is_empty() {
        return Err(other("ponle un nombre al préstamo"));
    }
    let c = &p.contraparte;
    if c.nombre.trim().is_empty() {
        return Err(other("falta el nombre de la contraparte"));
    }
    if !c.nif.trim().is_empty() && Nif::parse(c.nif.trim()).is_err() {
        return Err(other("el NIF de la contraparte no es válido"));
    }
    let prov = c.provincia.trim();
    if !prov.is_empty()
        && !(prov.len() == 2 && prov.parse::<u8>().is_ok_and(|n| (1..=53).contains(&n)))
    {
        return Err(other("la provincia es un código de 01 a 53"));
    }
    if p.sentido == Sentido::Recibido && p.retencion.bp() != 0 && c.nif.trim().is_empty() {
        return Err(other(
            "con retención hace falta el NIF del prestamista (va en el 123 y el 193)",
        ));
    }
    let k = &p.cuentas;
    let (largo, corto, intereses, devengo): (&[&str], &[&str], &[&str], &[&str]) = match p.sentido {
        Sentido::Concedido => (
            &["24", "25", "55"],
            &["53", "54", "55"],
            &["76"],
            &["53", "54", "55"],
        ),
        Sentido::Recibido => (
            &["16", "17", "55"],
            &["51", "52", "55"],
            &["66"],
            &["51", "52", "55"],
        ),
    };
    let que = match p.sentido {
        Sentido::Concedido => {
            "un préstamo concedido va en el activo: largo plazo en la 24x/25x, corto en la \
             53x/54x (o una cuenta corriente 55x en las dos), intereses en la 76x y su devengo en \
             la 53x/54x"
        }
        Sentido::Recibido => {
            "un préstamo recibido va en el pasivo: largo plazo en la 16x/17x, corto en la 51x/52x \
             (o una cuenta corriente 55x en las dos), intereses en la 66x y su devengo en la \
             51x/52x"
        }
    };
    if !prefijo(&k.largo, largo)
        || !prefijo(&k.corto, corto)
        || !prefijo(&k.intereses, intereses)
        || !prefijo(&k.devengo, devengo)
    {
        return Err(other(que));
    }
    let cc = |a: &AccountCode| a.as_str().starts_with("55");
    if cc(&k.largo) != cc(&k.corto) || (cc(&k.largo) && k.largo != k.corto) {
        return Err(other(
            "una cuenta corriente (551, 552…) va igual en el largo y en el corto plazo",
        ));
    }
    if !cc(&k.largo) && k.largo == k.corto {
        return Err(other(
            "la cuenta de largo plazo y la de corto han de ser distintas",
        ));
    }
    Ok(())
}

/// Aviso de un préstamo con una parte vinculada sin intereses o con un tipo por debajo del
/// interés legal del dinero. Es el mismo texto en la ficha del préstamo y en el registro de
/// operaciones vinculadas (que lo da también si la contraparte es vinculada sin estar marcada).
pub fn aviso_tipo_vinculada(p: &Prestamo) -> Option<String> {
    if p.tipo.bp() == 0 {
        Some(
            "Préstamo sin intereses con una parte vinculada: las operaciones vinculadas se valoran \
             por su valor de mercado (art. 18.1 LIS) y Hacienda puede exigir el interés que se \
             pactaría con un tercero y ajustar el Impuesto sobre Sociedades. Pon el tipo de \
             mercado (y documenta de dónde sale)."
                .to_owned(),
        )
    } else if p.tipo < INTERES_LEGAL {
        Some(format!(
            "Tipo del {} con una parte vinculada, por debajo del interés legal del dinero ({}): \
             no es el valor de mercado del art. 18 LIS, pero es la referencia que suele mirarse. \
             Comprueba que es el que pactaríais con un tercero.",
            p.tipo, INTERES_LEGAL
        ))
    } else {
        None
    }
}

/// Avisos fiscales de la ficha.
pub fn avisos(p: &Prestamo) -> Vec<String> {
    let c = &p.contraparte;
    let mut out = Vec::new();
    let socio_pf = c.persona_fisica && (c.vinculada() || c.origen == OrigenContraparte::Socio);
    if c.vinculada()
        && let Some(a) = aviso_tipo_vinculada(p)
    {
        out.push(a);
    }
    if p.sentido == Sentido::Concedido && socio_pf && p.tipo < INTERES_LEGAL {
        out.push(
            "Préstamo al socio sin interés o con un interés bajo: además del ajuste en la \
             sociedad, la diferencia con el interés de mercado puede tributar en el IRPF del \
             socio como utilidad por su condición de socio (art. 18.11 LIS y art. 25.1.d LIRPF) \
             o, si es administrador o empleado, como retribución en especie (art. 42.2.f \
             LIRPF). TODO(VERIFY): consúltalo con tu gestoría."
                .to_owned(),
        );
    }
    if p.sentido == Sentido::Recibido && c.persona_fisica && p.tipo.bp() == 0 {
        out.push(
            "Préstamo de una persona física sin intereses: el IRPF presume retribuida la cesión \
             de capitales salvo prueba en contrario (art. 6.5 LIRPF) y, si es vinculada, por su \
             valor de mercado (art. 41 LIRPF): el prestamista tributaría por ese interés y la \
             sociedad tendría que retener. TODO(VERIFY)."
                .to_owned(),
        );
    }
    if p.sentido == Sentido::Recibido && !c.entidad_credito && p.retencion.bp() == 0 {
        out.push(
            "Los intereses que paga la sociedad a un prestamista residente que no es un banco \
             llevan retención del 19 % (art. 101.4 LIRPF; art. 128.6 LIS), que se ingresa con el \
             123 y va en el 193. Revisa la retención (salvo que el prestamista no sea residente \
             o esté exento)."
                .to_owned(),
        );
    }
    if p.sentido == Sentido::Recibido && c.entidad_credito && p.retencion.bp() > 0 {
        out.push(
            "Los intereses de los préstamos de bancos no llevan retención (art. 61 RIS): revisa \
             la retención. TODO(VERIFY)."
                .to_owned(),
        );
    }
    if p.sentido == Sentido::Concedido && c.persona_fisica && p.retencion.bp() > 0 {
        out.push(
            "Una persona física que no actúa como empresario no está obligada a retener (art. 76 \
             RIRPF): normalmente estos intereses se cobran íntegros."
                .to_owned(),
        );
    }
    if p.sentido == Sentido::Recibido && c.persona_fisica {
        out.push(
            "Préstamo de un particular: está sujeto al ITP y exento (art. 45.I.B.15 TRLITPAJD), \
             pero se presenta el modelo 600 (exento) en el plazo de 30 días hábiles. \
             TODO(VERIFY)."
                .to_owned(),
        );
    }
    if p.sentido == Sentido::Recibido && c.persona_fisica && c.vinculada() && p.retencion.bp() > 0 {
        out.push(
            "Si el socio te presta más de tres veces su participación en los fondos propios, los \
             intereses de lo que excede van a la base general de su IRPF (art. 46.a LIRPF) y en \
             el 193 con la clave D: TaxPain los declara todos con la clave B. TODO(VERIFY)."
                .to_owned(),
        );
    }
    out
}

// ---------- Vista ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoCuota {
    Pendiente,
    /// Contabilizada desde aquí.
    Registrada,
    /// Ya contabilizada fuera de TaxPain.
    Fuera,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Devengo {
    pub id: i64,
    pub n: u16,
    pub fecha: Date,
    pub importe: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CuotaVista {
    #[serde(flatten)]
    pub q: Cuota,
    pub estado: EstadoCuota,
    /// Retención (la registrada o, si está pendiente, la que llevaría con la ficha actual).
    pub retencion: Money,
    /// Lo que se mueve en el banco: el pago menos la retención.
    pub neto: Money,
    /// Intereses ya devengados de esta cuota.
    pub devengado: Money,
    /// Movimiento del banco conciliado con su asiento.
    pub bank_tx: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PrestamoVista {
    pub prestamo: Prestamo,
    pub alta: EstadoAlta,
    /// Movimiento del banco conciliado con el asiento del alta.
    pub alta_bank_tx: Option<i64>,
    pub cuotas: Vec<CuotaVista>,
    pub devengos: Vec<Devengo>,
    /// Capital pendiente hoy según el cuadro, y su parte a corto y a largo plazo.
    pub saldo_vivo: Money,
    pub corto_plazo: Money,
    pub largo_plazo: Money,
    /// Cuotas vencidas (hasta hoy) sin registrar.
    pub vencidas: usize,
    pub avisos: Vec<String>,
}

fn banco_de(conn: &Connection, entry: Option<i64>) -> Result<Option<i64>> {
    let Some(e) = entry else { return Ok(None) };
    Ok(conn
        .query_row(
            "SELECT id FROM bank_tx WHERE entry_id = ?1 ORDER BY id LIMIT 1",
            [e],
            |r| r.get(0),
        )
        .optional()?)
}

struct Registro {
    capital: Money,
    intereses: Money,
    capitalizados: Money,
    retencion: Money,
    entry: Option<i64>,
}

fn registradas(conn: &Connection, id: i64) -> Result<std::collections::BTreeMap<u16, Registro>> {
    let mut st = conn.prepare(
        "SELECT n, capital, intereses, capitalizados, retencion, entry_id FROM prestamo_cuota
         WHERE prestamo_id = ?1",
    )?;
    let rows = st.query_map([id], |r| {
        Ok((
            r.get::<_, u16>(0)?,
            Registro {
                capital: Money::from_cents(r.get(1)?),
                intereses: Money::from_cents(r.get(2)?),
                capitalizados: Money::from_cents(r.get(3)?),
                retencion: Money::from_cents(r.get(4)?),
                entry: r.get(5)?,
            },
        ))
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn devengos(conn: &Connection, id: i64) -> Result<Vec<Devengo>> {
    let mut st = conn.prepare(
        "SELECT id, n, fecha, importe FROM prestamo_devengo WHERE prestamo_id = ?1
         ORDER BY fecha, id",
    )?;
    let rows: Vec<(i64, u16, String, i64)> = st
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<std::result::Result<_, _>>()?;
    rows.into_iter()
        .map(|(id, n, f, imp)| {
            Ok(Devengo {
                id,
                n,
                fecha: fecha(&f)?,
                importe: Money::from_cents(imp),
            })
        })
        .collect()
}

fn vista(conn: &Connection, f: Fila, hoy: Date) -> Result<PrestamoVista> {
    let p = f.p;
    let cuadro = calc::cuadro(&p);
    let reg = registradas(conn, p.id)?;
    let dev = devengos(conn, p.id)?;
    let mut cuotas = Vec::with_capacity(cuadro.len());
    for q in cuadro.iter().cloned() {
        let devengado = dev.iter().filter(|d| d.n == q.n).map(|d| d.importe).sum();
        let (estado, retencion, bank_tx, q) = match reg.get(&q.n) {
            Some(r) => {
                // Con los importes con que se registró (por si la ficha cambió después).
                let q = Cuota {
                    capital: r.capital,
                    intereses: r.intereses,
                    capitalizados: r.capitalizados,
                    pago: r.capital + r.intereses - r.capitalizados,
                    ..q
                };
                let estado = if r.entry.is_some() {
                    EstadoCuota::Registrada
                } else {
                    EstadoCuota::Fuera
                };
                (estado, r.retencion, banco_de(conn, r.entry)?, q)
            }
            None => (EstadoCuota::Pendiente, calc::retencion_de(&p, &q), None, q),
        };
        cuotas.push(CuotaVista {
            neto: q.pago - retencion,
            q,
            estado,
            retencion,
            devengado,
            bank_tx,
        });
    }
    let vencidas = cuotas
        .iter()
        .filter(|c| c.estado == EstadoCuota::Pendiente && c.q.fecha <= hoy)
        .count();
    let saldo_vivo = calc::pendiente_a(&p, &cuadro, hoy);
    let largo_plazo = calc::largo_plazo_a(&p, &cuadro, hoy);
    Ok(PrestamoVista {
        alta: f.alta,
        alta_bank_tx: banco_de(conn, f.alta_entry)?,
        cuotas,
        devengos: dev,
        saldo_vivo,
        corto_plazo: saldo_vivo - largo_plazo,
        largo_plazo,
        vencidas,
        avisos: avisos(&p),
        prestamo: p,
    })
}

/// Los préstamos con su cuadro, lo registrado y el saldo vivo a `hoy`.
pub fn list(v: &Vault, hoy: Date) -> Result<Vec<PrestamoVista>> {
    let conn = v.connection();
    todos(conn)?
        .into_iter()
        .map(|f| vista(conn, f, hoy))
        .collect()
}

fn uno(v: &Vault, id: i64, hoy: Date) -> Result<PrestamoVista> {
    let conn = v.connection();
    vista(conn, cargar(conn, id)?, hoy)
}

// ---------- Alta y edición ----------

fn guardar(conn: &Connection, p: &Prestamo, id: Option<i64>, now: OffsetDateTime) -> Result<i64> {
    use taxpain_vault::rusqlite::params_from_iter;
    use taxpain_vault::rusqlite::types::Value;
    let c = &p.contraparte;
    let t = |s: &str| Value::Text(s.to_owned());
    let i = |n: i64| Value::Integer(n);
    let mut vals = vec![
        t(p.nombre.trim()),
        t(p.sentido.as_str()),
        t(c.origen.as_str()),
        c.ref_id.map_or(Value::Null, Value::Integer),
        t(&c.nif.trim().to_uppercase()),
        t(c.nombre.trim()),
        t(c.provincia.trim()),
        i(i64::from(c.persona_fisica)),
        i(i64::from(c.entidad_credito)),
        t(c.vinculo.as_str()),
        i(i64::from(c.vinculada())),
        i(p.principal.cents()),
        t(&p.inicio.to_string()),
        i(i64::from(p.tipo.bp())),
        t(p.sistema.as_str()),
        i(i64::from(p.periodicidad)),
        i(i64::from(p.plazo)),
        i(i64::from(p.carencia)),
        t(match p.carencia_tipo {
            TipoCarencia::Capital => "capital",
            TipoCarencia::Total => "total",
        }),
        t(p.cuentas.largo.as_str()),
        t(p.cuentas.corto.as_str()),
        t(p.cuentas.intereses.as_str()),
        t(p.cuentas.devengo.as_str()),
        i(i64::from(p.retencion.bp())),
        t(p.notas.trim()),
        t(&now_str(now)),
    ];
    match id {
        None => {
            conn.execute(
                "INSERT INTO prestamo(nombre, sentido, contraparte_origen, contraparte_ref,
                   contraparte_nif, contraparte_nombre, contraparte_provincia, persona_fisica,
                   entidad_credito, vinculo, vinculada, principal, fecha_inicio, tipo_bp, sistema,
                   periodicidad_meses, plazo_meses, carencia_meses, carencia_tipo, cuenta_largo,
                   cuenta_corto, cuenta_intereses, cuenta_devengo, retencion_bp, notas, created_at,
                   updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                   ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?26)",
                params_from_iter(vals.iter()),
            )?;
            Ok(conn.last_insert_rowid())
        }
        Some(id) => {
            vals.push(i(id));
            conn.execute(
                "UPDATE prestamo SET nombre = ?1, sentido = ?2, contraparte_origen = ?3,
                   contraparte_ref = ?4, contraparte_nif = ?5, contraparte_nombre = ?6,
                   contraparte_provincia = ?7, persona_fisica = ?8, entidad_credito = ?9,
                   vinculo = ?10, vinculada = ?11, principal = ?12, fecha_inicio = ?13,
                   tipo_bp = ?14, sistema = ?15, periodicidad_meses = ?16, plazo_meses = ?17,
                   carencia_meses = ?18, carencia_tipo = ?19, cuenta_largo = ?20,
                   cuenta_corto = ?21, cuenta_intereses = ?22, cuenta_devengo = ?23,
                   retencion_bp = ?24, notas = ?25, updated_at = ?26
                 WHERE id = ?27",
                params_from_iter(vals.iter()),
            )?;
            Ok(id)
        }
    }
}

/// Con «al vencimiento» la periodicidad no cuenta: se guarda 12 (la base exige 1, 3, 6 o 12).
fn normalizar(p: &Prestamo) -> Prestamo {
    let mut p = p.clone();
    if p.sistema == Sistema::AlVencimiento {
        p.periodicidad = 12;
        p.carencia = 0;
    }
    p
}

/// Da de alta la ficha de un préstamo (sin asiento: el alta se contabiliza después, contra la
/// tesorería o un movimiento del banco, o se marca como ya contabilizada). Devuelve su id.
pub fn create(v: &Vault, p: &Prestamo, now: OffsetDateTime) -> Result<i64> {
    let p = normalizar(p);
    comprobar(&p)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let id = guardar(&tx, &p, None, now)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "prestamo.create",
        &id.to_string(),
        &serde_json::json!({
            "nombre": p.nombre.trim(),
            "sentido": p.sentido.as_str(),
            "principal": p.principal.cents(),
            "vinculada": p.contraparte.vinculada(),
        }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// ¿Tiene ya algo contabilizado (o marcado como tal)?
fn con_movimientos(conn: &Connection, id: i64, alta: EstadoAlta) -> Result<bool> {
    let n: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM prestamo_cuota WHERE prestamo_id = ?1)
             OR EXISTS(SELECT 1 FROM prestamo_devengo WHERE prestamo_id = ?1)",
        [id],
        |r| r.get(0),
    )?;
    Ok(n || alta == EstadoAlta::Contabilizada)
}

/// Cambia la ficha. Con el alta contabilizada o alguna cuota o devengo registrados sólo se
/// pueden cambiar los datos que no mueven el cuadro ni las cuentas: el nombre, la contraparte
/// (y su vínculo), la retención de las cuotas que falten y las notas.
pub fn update(v: &Vault, id: i64, p: &Prestamo, now: OffsetDateTime) -> Result<()> {
    let p = Prestamo {
        id,
        ..normalizar(p)
    };
    comprobar(&p)?;
    let conn = v.connection();
    let antes = cargar(conn, id)?;
    if con_movimientos(conn, id, antes.alta)? {
        let a = &antes.p;
        let mismo = a.sentido == p.sentido
            && a.inicio == p.inicio
            && a.principal == p.principal
            && a.tipo == p.tipo
            && a.sistema == p.sistema
            && a.periodicidad == p.periodicidad
            && a.plazo == p.plazo
            && a.carencia == p.carencia
            && a.carencia_tipo == p.carencia_tipo
            && a.cuentas == p.cuentas;
        if !mismo {
            return Err(other(format!(
                "el préstamo «{}» ya tiene asientos: sus condiciones (importe, fecha, tipo, \
                 sistema, plazo, carencia y cuentas) no se cambian. Deshaz antes las cuotas, los \
                 devengos y el alta, o, si el contrato se ha novado, da de alta otro préstamo.",
                a.nombre
            )));
        }
    }
    let tx = conn.unchecked_transaction()?;
    guardar(&tx, &p, Some(id), now)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "prestamo.update",
        &id.to_string(),
        &serde_json::json!({
            "nombre": p.nombre.trim(),
            "principal": p.principal.cents(),
            "tipo": p.tipo.bp(),
            "retencion": p.retencion.bp(),
            "vinculada": p.contraparte.vinculada(),
        }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------- Tesorería y banco ----------

/// Con qué se paga o se cobra: una cuenta de tesorería (572…, o la cuenta corriente 551 si se
/// compensa) o un movimiento del banco aún sin conciliar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pago {
    #[serde(default)]
    pub tesoreria: Option<String>,
    #[serde(default)]
    pub bank_tx: Option<i64>,
}

/// Movimiento del banco que puede ser el de una cuota o el alta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Movimiento {
    pub id: i64,
    pub fecha: Date,
    pub importe: Money,
    pub concepto: String,
    pub cuenta: String,
    pub banco: String,
}

/// Días de margen entre el vencimiento y el movimiento del banco.
const DIAS_BANCO: i64 = 15;

/// Movimientos pendientes (sin conciliar, en euros, de un periodo que no está contabilizado en
/// otro sitio) de `importe` (con signo: + cobro, − pago) a ±`DIAS_BANCO` días de `fecha`.
fn candidatos(conn: &Connection, importe: Money, fecha: Date) -> Result<Vec<Movimiento>> {
    let mut st = conn.prepare(&format!(
        "SELECT t.id, t.op_date, t.amount, t.description, b.account, b.name FROM bank_tx t
         JOIN bank_account b ON b.id = t.bank_account
         WHERE t.matched_kind IS NULL AND t.entry_id IS NULL AND t.amount = ?1
           AND b.currency = 'EUR' AND b.kind <> ?2 AND b.account <> ''
           AND NOT {}
         ORDER BY t.op_date, t.id",
        crate::bank::BOOKED_ELSEWHERE
    ))?;
    let rows: Vec<(i64, String, i64, String, String, String)> = st
        .query_map(params![importe.cents(), crate::bank::INFORMATIVE], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, f, imp, concepto, cuenta, banco) in rows {
        let f = fecha_ok(&f)?;
        if (f - fecha).whole_days().abs() <= DIAS_BANCO {
            out.push(Movimiento {
                id,
                fecha: f,
                importe: Money::from_cents(imp),
                concepto,
                cuenta,
                banco,
            });
        }
    }
    out.sort_by_key(|m| (m.fecha - fecha).whole_days().abs());
    Ok(out)
}

fn fecha_ok(s: &str) -> Result<Date> {
    fecha(s.get(..10).unwrap_or(s))
}

/// Importe con signo que se mueve en el banco: + si cobramos, − si pagamos.
fn con_signo(cobro: bool, importe: Money) -> Money {
    if cobro { importe } else { -importe }
}

/// Movimientos del banco que pueden ser los de la cuota `n` (0: el alta) del préstamo `id`.
pub fn bank_candidates(v: &Vault, id: i64, n: u16, hoy: Date) -> Result<Vec<Movimiento>> {
    let view = uno(v, id, hoy)?;
    let p = &view.prestamo;
    let concedido = p.sentido == Sentido::Concedido;
    let (importe, fecha) = if n == 0 {
        (con_signo(!concedido, p.principal), p.inicio)
    } else {
        let c = view
            .cuotas
            .iter()
            .find(|c| c.q.n == n)
            .ok_or(AppError::NotFound)?;
        (con_signo(concedido, c.neto), c.q.fecha)
    };
    if importe.is_zero() {
        return Ok(Vec::new());
    }
    candidatos(v.connection(), importe, fecha)
}

/// La cuenta de tesorería y la fecha del asiento para `pago` de `importe` (con signo) que vence
/// el día `vence`; con un movimiento del banco, la de su cuenta y su fecha.
fn tesoreria(
    conn: &Connection,
    pago: &Pago,
    importe: Money,
    vence: Date,
) -> Result<(AccountCode, Date, Option<i64>)> {
    if let Some(tx) = pago.bank_tx {
        let m = candidatos(conn, importe, vence)?
            .into_iter()
            .find(|m| m.id == tx)
            .ok_or_else(|| {
                other(format!(
                    "ese movimiento del banco no es de {} a {DIAS_BANCO} días del {} o ya está \
                     conciliado",
                    importe,
                    fmt_fecha(vence)
                ))
            })?;
        return Ok((
            AccountCode::new(&m.cuenta).map_err(other)?,
            m.fecha,
            Some(tx),
        ));
    }
    let cuenta = pago
        .tesoreria
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .ok_or_else(|| other("elige la cuenta de tesorería o el movimiento del banco"))?;
    let cuenta = AccountCode::new(cuenta).map_err(other)?;
    if !prefijo(&cuenta, &["57", "55"]) {
        return Err(other(
            "se paga o se cobra por la tesorería (572…) o compensando la cuenta corriente \
             (551, 552…)",
        ));
    }
    Ok((cuenta, vence, None))
}

/// Concilia el movimiento `tx` con el asiento `entry` (dentro de la transacción).
fn conciliar(conn: &Connection, tx: Option<i64>, entry: i64) -> Result<()> {
    if let Some(tx) = tx {
        let n = conn.execute(
            "UPDATE bank_tx SET entry_id = ?1, matched_kind = 'cuenta'
             WHERE id = ?2 AND matched_kind IS NULL AND entry_id IS NULL",
            params![entry, tx],
        )?;
        if n != 1 {
            return Err(other("ese movimiento del banco ya está conciliado"));
        }
    }
    Ok(())
}

/// Deshace la conciliación con el asiento `entry` y lo borra (dentro de la transacción).
fn quitar_asiento(conn: &Connection, entry: i64) -> Result<()> {
    conn.execute(
        "UPDATE bank_tx SET entry_id = NULL, matched_kind = NULL, matched_id = NULL,
                matched_seq = NULL WHERE entry_id = ?1",
        [entry],
    )?;
    conn.execute(
        "DELETE FROM line_tick WHERE line_id IN (SELECT id FROM journal_line WHERE entry_id = ?1)",
        [entry],
    )?;
    accounting::delete_entry_rows(conn, entry)
}

/// Por qué no se puede borrar el asiento `entry` (`None` si se puede): de un ejercicio cerrado.
fn bloqueo_asiento(conn: &Connection, entry: i64) -> Result<Option<String>> {
    let (year, closed): (i32, bool) = conn.query_row(
        "SELECT e.fiscal_year, COALESCE(f.closed, 0) FROM journal_entry e
         LEFT JOIN fiscal_year f ON f.year = e.fiscal_year WHERE e.id = ?1",
        [entry],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(closed.then(|| {
        format!("su asiento es del ejercicio {year}, que está cerrado: reábrelo antes (Cierre)")
    }))
}

// ---------- Alta ----------

/// Contabiliza el alta (entrega o recepción del principal) contra la tesorería o un movimiento
/// del banco. Devuelve el asiento.
pub fn post_start(
    v: &Vault,
    id: i64,
    pago: &Pago,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let f = cargar(conn, id)?;
    if f.alta != EstadoAlta::Pendiente {
        return Err(other("el alta de este préstamo ya está contabilizada"));
    }
    let p = &f.p;
    let importe = con_signo(p.sentido == Sentido::Recibido, p.principal);
    let (cuenta, fecha, bank) = tesoreria(conn, pago, importe, p.inicio)?;
    let mut e = calc::asiento_alta(p, &cuenta);
    e.date = fecha;
    let tx = conn.unchecked_transaction()?;
    let entry = accounting::record_entry(&tx, &e, now)?;
    conciliar(&tx, bank, entry)?;
    tx.execute(
        "UPDATE prestamo SET alta_estado = 'contabilizada', alta_entry = ?1 WHERE id = ?2",
        params![entry, id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "prestamo.alta",
        &id.to_string(),
        &serde_json::json!({ "entry": entry, "bank_tx": bank, "cuenta": cuenta.as_str() }),
    )?;
    tx.commit()?;
    Ok(entry)
}

/// Marca el alta como ya contabilizada fuera de TaxPain (saldos iniciales o diario del gestor),
/// o lo deshace (`fuera = false`).
pub fn mark_start_outside(v: &Vault, id: i64, fuera: bool, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let f = cargar(conn, id)?;
    let (de, a) = if fuera {
        (EstadoAlta::Pendiente, "fuera")
    } else {
        (EstadoAlta::Fuera, "pendiente")
    };
    if f.alta != de {
        return Err(other(if fuera {
            "el alta de este préstamo no está pendiente"
        } else {
            "el alta de este préstamo no estaba marcada como contabilizada fuera"
        }));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE prestamo SET alta_estado = ?1 WHERE id = ?2",
        params![a, id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "prestamo.alta_fuera",
        &id.to_string(),
        &serde_json::json!({ "fuera": fuera }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Deshace el asiento del alta (con confirmación): sólo si no hay cuotas ni devengos y su
/// ejercicio está abierto. El movimiento del banco vuelve a estar pendiente.
pub fn undo_start(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let f = cargar(conn, id)?;
    let Some(entry) = f.alta_entry else {
        return Err(other("el alta de este préstamo no tiene asiento"));
    };
    if con_movimientos(conn, id, EstadoAlta::Pendiente)? {
        return Err(other(
            "el préstamo tiene cuotas o devengos registrados: deshazlos antes",
        ));
    }
    if let Some(b) = bloqueo_asiento(conn, entry)? {
        return Err(other(b));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE prestamo SET alta_estado = 'pendiente', alta_entry = NULL WHERE id = ?1",
        [id],
    )?;
    quitar_asiento(&tx, entry)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "prestamo.alta_undo",
        &id.to_string(),
        &serde_json::json!({ "entry": entry }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------- Cuotas ----------

/// Registra la cuota `n` (la primera sin registrar): asiento con capital, intereses, retención y
/// tesorería (o el movimiento del banco, que queda conciliado). Necesita el alta contabilizada o
/// marcada como ya contabilizada. Devuelve el asiento.
pub fn post_installment(
    v: &Vault,
    id: i64,
    n: u16,
    pago: &Pago,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let hoy = taxpain_core::spain_time::peninsular_date(now);
    let view = uno(v, id, hoy)?;
    let p = &view.prestamo;
    if view.alta == EstadoAlta::Pendiente {
        return Err(other(
            "contabiliza antes el alta del préstamo (o márcala como ya contabilizada)",
        ));
    }
    let next = view
        .cuotas
        .iter()
        .find(|c| c.estado == EstadoCuota::Pendiente)
        .ok_or_else(|| other("todas las cuotas están registradas"))?;
    if next.q.n != n {
        return Err(other(format!(
            "las cuotas se registran por orden: la siguiente es la {}",
            next.q.n
        )));
    }
    let concedido = p.sentido == Sentido::Concedido;
    let conn = v.connection();
    let (cuenta, fecha, bank) = if next.neto.is_zero() {
        // Carencia total: no se mueve dinero.
        (acc("572"), next.q.fecha, None)
    } else {
        tesoreria(conn, pago, con_signo(concedido, next.neto), next.q.fecha)?
    };
    let e = calc::asiento_cuota(p, &next.q, next.devengado, &cuenta, fecha);
    let tx = conn.unchecked_transaction()?;
    let entry = accounting::record_entry(&tx, &e, now)?;
    conciliar(&tx, bank, entry)?;
    tx.execute(
        "INSERT INTO prestamo_cuota(prestamo_id, n, fecha, capital, intereses, capitalizados,
           retencion, entry_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            n,
            next.q.fecha.to_string(),
            next.q.capital.cents(),
            next.q.intereses.cents(),
            next.q.capitalizados.cents(),
            next.retencion.cents(),
            entry,
            now_str(now)
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "prestamo.cuota",
        &id.to_string(),
        &serde_json::json!({
            "n": n,
            "entry": entry,
            "bank_tx": bank,
            "capital": next.q.capital.cents(),
            "intereses": next.q.intereses.cents(),
            "retencion": next.retencion.cents(),
        }),
    )?;
    tx.commit()?;
    Ok(entry)
}

/// Marca como ya contabilizadas fuera de TaxPain (en el diario del gestor o en los saldos
/// iniciales) las cuotas pendientes que vencen hasta `hasta`, sin asiento. Cuentan para el 123
/// y el 193 con su retención. Devuelve cuántas.
pub fn mark_outside(
    v: &Vault,
    id: i64,
    hasta: Date,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<usize> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let hoy = taxpain_core::spain_time::peninsular_date(now);
    let view = uno(v, id, hoy)?;
    let mut hechas = Vec::new();
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    for c in &view.cuotas {
        if c.q.fecha > hasta {
            break;
        }
        if c.estado != EstadoCuota::Pendiente {
            continue;
        }
        if c.devengado.cents() > 0 {
            return Err(other(format!(
                "la cuota {} tiene intereses devengados desde aquí: regístrala con su asiento o \
                 deshaz antes el devengo",
                c.q.n
            )));
        }
        tx.execute(
            "INSERT INTO prestamo_cuota(prestamo_id, n, fecha, capital, intereses, capitalizados,
               retencion, entry_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8)",
            params![
                id,
                c.q.n,
                c.q.fecha.to_string(),
                c.q.capital.cents(),
                c.q.intereses.cents(),
                c.q.capitalizados.cents(),
                c.retencion.cents(),
                now_str(now)
            ],
        )?;
        hechas.push(c.q.n);
    }
    if !hechas.is_empty() {
        audit::append(
            &tx,
            &now_str(now),
            Actor::User,
            "prestamo.cuotas_fuera",
            &id.to_string(),
            &serde_json::json!({ "cuotas": hechas, "hasta": hasta.to_string() }),
        )?;
    }
    tx.commit()?;
    Ok(hechas.len())
}

/// Deshace la última cuota registrada (o marcada como ya contabilizada): borra su asiento (si
/// su ejercicio está abierto) y el movimiento del banco vuelve a estar pendiente.
pub fn undo_installment(
    v: &Vault,
    id: i64,
    n: u16,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let reg = registradas(conn, id)?;
    let Some((ultima, r)) = reg.iter().next_back() else {
        return Err(other("no hay cuotas registradas"));
    };
    if *ultima != n {
        return Err(other(format!(
            "se deshace desde la última registrada: la {ultima}"
        )));
    }
    if let Some(e) = r.entry
        && let Some(b) = bloqueo_asiento(conn, e)?
    {
        return Err(other(b));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM prestamo_cuota WHERE prestamo_id = ?1 AND n = ?2",
        params![id, n],
    )?;
    if let Some(e) = r.entry {
        quitar_asiento(&tx, e)?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "prestamo.cuota_undo",
        &id.to_string(),
        &serde_json::json!({ "n": n, "entry": r.entry }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------- Devengo de intereses ----------

/// Resultado de un devengo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Devengado {
    pub prestamo: String,
    pub n: u16,
    pub importe: Money,
}

/// Devenga a `fecha` los intereses de la cuota en curso que falten (lo devengado hasta ese día
/// menos lo ya devengado antes). `None` si no hay nada que devengar.
fn devengar_uno(
    conn: &Connection,
    f: &Fila,
    fecha: Date,
    now: OffsetDateTime,
) -> Result<Option<Devengado>> {
    let p = &f.p;
    if f.alta == EstadoAlta::Pendiente {
        return Ok(None);
    }
    let cuadro = calc::cuadro(p);
    let Some((n, total)) = calc::devengado_a(p, &cuadro, fecha) else {
        return Ok(None);
    };
    let reg = registradas(conn, p.id)?;
    if reg.contains_key(&n) {
        return Ok(None);
    }
    let ya: Money = devengos(conn, p.id)?
        .iter()
        .filter(|d| d.n == n)
        .map(|d| d.importe)
        .sum();
    let importe = total - ya;
    if importe.cents() <= 0 {
        return Ok(None);
    }
    let entry = accounting::record_entry(conn, &calc::asiento_devengo(p, n, fecha, importe), now)?;
    conn.execute(
        "INSERT INTO prestamo_devengo(prestamo_id, n, fecha, importe, entry_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            p.id,
            n,
            fecha.to_string(),
            importe.cents(),
            entry,
            now_str(now)
        ],
    )?;
    Ok(Some(Devengado {
        prestamo: p.nombre.clone(),
        n,
        importe,
    }))
}

/// Devenga a `fecha` los intereses del préstamo `id` (o de todos, con `None`) que falten. No
/// toca los que tienen el alta pendiente ni las cuotas ya registradas. Devuelve lo devengado.
pub fn accrue(
    v: &Vault,
    id: Option<i64>,
    fecha: Date,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<Vec<Devengado>> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let filas = match id {
        Some(id) => vec![cargar(conn, id)?],
        None => todos(conn)?,
    };
    let tx = conn.unchecked_transaction()?;
    let mut out = Vec::new();
    for f in &filas {
        if let Some(d) = devengar_uno(&tx, f, fecha, now)? {
            out.push(d);
        }
    }
    if !out.is_empty() {
        audit::append(
            &tx,
            &now_str(now),
            Actor::User,
            "prestamo.devengo",
            &id.map_or_else(|| "todos".to_owned(), |i| i.to_string()),
            &serde_json::json!({
                "fecha": fecha.to_string(),
                "devengos": out.iter().map(|d| serde_json::json!({
                    "prestamo": d.prestamo, "n": d.n, "importe": d.importe.cents()
                })).collect::<Vec<_>>(),
            }),
        )?;
    }
    tx.commit()?;
    Ok(out)
}

/// Deshace un devengo (con confirmación): sólo si su cuota no está registrada y su ejercicio
/// está abierto.
pub fn undo_accrual(v: &Vault, devengo: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let (id, n, entry): (i64, u16, Option<i64>) = conn
        .query_row(
            "SELECT prestamo_id, n, entry_id FROM prestamo_devengo WHERE id = ?1",
            [devengo],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    if registradas(conn, id)?.contains_key(&n) {
        return Err(other(format!(
            "la cuota {n} ya está registrada y saldó este devengo: deshaz antes la cuota"
        )));
    }
    if let Some(e) = entry
        && let Some(b) = bloqueo_asiento(conn, e)?
    {
        return Err(other(b));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM prestamo_devengo WHERE id = ?1", [devengo])?;
    if let Some(e) = entry {
        quitar_asiento(&tx, e)?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "prestamo.devengo_undo",
        &id.to_string(),
        &serde_json::json!({ "devengo": devengo, "n": n, "entry": entry }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------- Reclasificación al cierre ----------

/// Al cierre del ejercicio `year`, deja en cada cuenta de largo plazo el capital que vence a más
/// de doce meses según el cuadro (y el resto en la de corto), con un asiento por la diferencia.
/// Los préstamos que comparten cuentas se reclasifican juntos (un asiento por par de cuentas),
/// como el leasing; las cuentas corrientes (largo = corto) no se tocan, ni los préstamos con el
/// alta pendiente. Se puede repetir: si ya está al día no hace nada. Devuelve cuántos asientos.
pub fn reclassify(v: &Vault, year: i32, now: OffsetDateTime, confirmed: bool) -> Result<usize> {
    use std::collections::BTreeMap;
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let fy = accounting::fiscal_year(v, year)?;
    let conn = v.connection();
    let filas = todos(conn)?;
    type Clave = (AccountCode, AccountCode, bool);
    let mut grupos: BTreeMap<Clave, (Vec<&Prestamo>, Money)> = BTreeMap::new();
    for f in &filas {
        let p = &f.p;
        if p.inicio > fy.end
            || f.alta == EstadoAlta::Pendiente
            || p.cuentas.largo == p.cuentas.corto
        {
            continue;
        }
        let g = grupos
            .entry((
                p.cuentas.largo.clone(),
                p.cuentas.corto.clone(),
                p.sentido == Sentido::Concedido,
            ))
            .or_insert_with(|| (Vec::new(), Money::ZERO));
        g.0.push(p);
        g.1 += calc::largo_plazo_a(p, &calc::cuadro(p), fy.end);
    }
    let tx = conn.unchecked_transaction()?;
    let mut n = 0;
    for ((largo, corto, activo), (prestamos, objetivo)) in grupos {
        let Some(primero) = prestamos.first() else {
            continue;
        };
        // Saldo de la cuenta de largo plazo en el ejercicio (con su apertura, sin el cierre):
        // deudor en un préstamo concedido, acreedor en uno recibido.
        let deudor: i64 = tx.query_row(
            "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
             JOIN journal_entry e ON e.id = l.entry_id
             WHERE l.account = ?1 AND e.fiscal_year = ?2 AND e.origin_kind <> 'Cierre'",
            params![largo.as_str(), year],
            |r| r.get(0),
        )?;
        let actual = Money::from_cents(if activo { deudor } else { -deudor });
        let dif = objetivo - actual;
        if dif.is_zero() {
            continue;
        }
        // dif > 0: pasa capital del corto al largo plazo; dif < 0, del largo al corto.
        let al_largo = dif.cents() > 0;
        let importe = dif.abs();
        let nombres: Vec<&str> = prestamos.iter().map(|p| p.nombre.as_str()).collect();
        let e = JournalEntry {
            date: fy.end,
            concept: format!(
                "Préstamo {}: capital a más de un año {} la {}",
                nombres.join(", "),
                if al_largo { "a" } else { "desde" },
                largo.as_str()
            ),
            origin: Origin::Prestamo(primero.id),
            lines: [
                // Activo: el largo plazo sube al debe. Pasivo: sube al haber.
                EntryLine::signed(largo.clone(), importe, al_largo == activo),
                EntryLine::signed(corto.clone(), importe, al_largo != activo),
            ]
            .into_iter()
            .flatten()
            .collect(),
        };
        accounting::record_entry(&tx, &e, now)?;
        n += 1;
    }
    if n > 0 {
        audit::append(
            &tx,
            &now_str(now),
            Actor::User,
            "prestamo.reclasificacion",
            &year.to_string(),
            &serde_json::json!({ "entries": n }),
        )?;
    }
    tx.commit()?;
    Ok(n)
}

// ---------- Borrar ----------

/// Por qué no se puede borrar el préstamo (`None` si se puede).
pub fn delete_blocker(v: &Vault, id: i64) -> Result<Option<String>> {
    let conn = v.connection();
    let f = cargar(conn, id)?;
    let con_asiento: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM prestamo_cuota WHERE prestamo_id = ?1 AND entry_id IS NOT NULL)
             OR EXISTS(SELECT 1 FROM prestamo_devengo WHERE prestamo_id = ?1)",
        [id],
        |r| r.get(0),
    )?;
    if con_asiento || f.alta == EstadoAlta::Contabilizada {
        return Ok(Some(format!(
            "el préstamo «{}» tiene asientos (alta, cuotas o devengos): deshazlos antes desde \
             su ficha",
            f.p.nombre
        )));
    }
    Ok(None)
}

/// Borra un préstamo dado de alta por error (con confirmación): sólo sin asientos (las cuotas
/// marcadas como ya contabilizadas fuera se van con él). Queda en la auditoría.
pub fn delete(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    if let Some(b) = delete_blocker(v, id)? {
        return Err(other(b));
    }
    let conn = v.connection();
    let f = cargar(conn, id)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM prestamo_cuota WHERE prestamo_id = ?1", [id])?;
    tx.execute("DELETE FROM prestamo WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "prestamo.delete",
        &id.to_string(),
        &serde_json::json!({
            "nombre": f.p.nombre,
            "principal": f.p.principal.cents(),
            "sentido": f.p.sentido.as_str(),
        }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Nombre del préstamo (para el aviso de Touch ID).
pub fn name(v: &Vault, id: i64) -> Result<Option<String>> {
    Ok(v.connection()
        .query_row("SELECT nombre FROM prestamo WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?)
}

// ---------- Para otras pantallas ----------

/// NIF de la contraparte: el de la ficha o, si falta, el del socio o el contacto elegido.
pub fn nif_contraparte(conn: &Connection, c: &Contraparte) -> Result<String> {
    let nif = c.nif.trim().to_uppercase();
    if !nif.is_empty() {
        return Ok(nif);
    }
    let sql = match c.origen {
        OrigenContraparte::Socio => "SELECT nif FROM shareholder WHERE id = ?1",
        OrigenContraparte::Contacto => "SELECT tax_id FROM contact WHERE id = ?1",
        _ => return Ok(nif),
    };
    let Some(id) = c.ref_id else { return Ok(nif) };
    Ok(conn
        .query_row(sql, [id], |r| r.get::<_, String>(0))
        .optional()?
        .map(|n| n.trim().to_uppercase())
        .unwrap_or_default())
}

/// Un préstamo visto en un ejercicio: lo dispuesto, los intereses devengados y el saldo vivo
/// al cierre (registro de vinculadas y 232, memoria).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DelEjercicio {
    pub prestamo: Prestamo,
    /// NIF de la contraparte ([`nif_contraparte`]).
    pub nif: String,
    /// Principal entregado (concedido) o recibido en el ejercicio: el de la ficha si el
    /// préstamo empieza dentro del ejercicio.
    pub dispuesto: Money,
    /// Intereses del ejercicio por devengo: los de las cuotas que vencen en él más lo devengado
    /// al cierre de la cuota en curso menos lo devengado al cierre anterior.
    pub intereses: Money,
    /// Capital pendiente al cierre según el cuadro.
    pub saldo_vivo: Money,
}

/// Los préstamos que existen en el ejercicio de `start` a `end` (empezados antes de `end` y con
/// capital pendiente al empezar el ejercicio o dispuestos en él).
pub fn del_ejercicio(conn: &Connection, start: Date, end: Date) -> Result<Vec<DelEjercicio>> {
    let antes = start.previous_day().unwrap_or(start);
    let mut out = Vec::new();
    for f in todos(conn)? {
        if f.p.inicio > end {
            continue;
        }
        let vista = vista(conn, f, end)?;
        let p = vista.prestamo;
        let cuadro = calc::cuadro(&p);
        let dispuesto = if p.inicio >= start {
            p.principal
        } else {
            Money::ZERO
        };
        if dispuesto.is_zero() && calc::pendiente_a(&p, &cuadro, antes).is_zero() {
            continue;
        }
        let vencidos: Money = vista
            .cuotas
            .iter()
            .filter(|q| q.q.fecha >= start && q.q.fecha <= end)
            .map(|q| q.q.intereses)
            .sum();
        let al = |d: Date| calc::devengado_a(&p, &cuadro, d).map_or(Money::ZERO, |(_, m)| m);
        let intereses = vencidos + al(end) - al(antes);
        out.push(DelEjercicio {
            nif: nif_contraparte(conn, &p.contraparte)?,
            dispuesto,
            intereses,
            saldo_vivo: vista.saldo_vivo,
            prestamo: p,
        });
    }
    Ok(out)
}

/// Condiciones del préstamo en una línea (libro de contratos del socio único, memoria).
pub fn condiciones(p: &Prestamo) -> String {
    let cuadro = calc::cuadro(p);
    let ultima = cuadro.last().map_or(p.inicio, |q| q.fecha);
    let interes = if p.tipo.bp() == 0 {
        "sin intereses".to_owned()
    } else {
        format!("interés nominal anual del {}", p.tipo)
    };
    let devolucion = match p.sistema {
        Sistema::AlVencimiento => {
            format!(
                "capital e intereses en un solo pago el {}",
                fmt_fecha(ultima)
            )
        }
        s => {
            let per = match p.periodicidad {
                1 => "mensuales",
                3 => "trimestrales",
                6 => "semestrales",
                _ => "anuales",
            };
            let mut t = format!(
                "{} cuotas {per} por el sistema {} hasta el {}",
                cuadro.len(),
                s.nombre(),
                fmt_fecha(ultima)
            );
            if p.carencia > 0 {
                t.push_str(&format!(
                    ", con {} meses de carencia {}",
                    p.carencia,
                    match p.carencia_tipo {
                        TipoCarencia::Capital => "de capital",
                        TipoCarencia::Total => "total",
                    }
                ));
            }
            t
        }
    };
    let retencion = if p.retencion.bp() > 0 {
        format!(", retención del {} sobre los intereses", p.retencion)
    } else {
        String::new()
    };
    format!(
        "Plazo de {} meses, {interes}, {devolucion}{retencion}. Garantías: [ninguna o las que \
         haya].",
        p.plazo
    )
}

// ---------- 123 y 193 ----------

/// Intereses pagados en una cuota de un préstamo recibido (registrada o ya contabilizada fuera),
/// con la retención practicada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteresPagado {
    /// Vencimiento de la cuota (exigibilidad: decide el 123 en que se ingresa la retención).
    pub fecha: Date,
    pub prestamo: String,
    pub nif: String,
    pub nombre: String,
    pub provincia: String,
    pub entidad_credito: bool,
    pub intereses: Money,
    pub retencion: Money,
    pub tipo: Rate,
}

/// Intereses de los préstamos recibidos con vencimiento entre `start` y `end` (los
/// capitalizados no se pagan ni se retienen). Para el 123 y el 193.
///
/// `TODO(VERIFY)`: la retención se toma en el vencimiento de la cuota (exigibilidad de los
/// intereses, art. 78.1 RIRPF por remisión del 94 y art. 65.1 RIS); si se paga más tarde, el
/// 123 es el del vencimiento.
pub fn intereses_pagados(conn: &Connection, start: Date, end: Date) -> Result<Vec<InteresPagado>> {
    let mut st = conn.prepare(
        "SELECT c.fecha, p.nombre, p.contraparte_nif, p.contraparte_nombre,
                p.contraparte_provincia, p.entidad_credito, c.intereses - c.capitalizados,
                c.retencion, p.retencion_bp
         FROM prestamo_cuota c JOIN prestamo p ON p.id = c.prestamo_id
         WHERE p.sentido = 'recibido' AND c.fecha BETWEEN ?1 AND ?2
           AND c.intereses - c.capitalizados > 0
         ORDER BY c.fecha, p.id, c.n",
    )?;
    type F = (String, String, String, String, String, bool, i64, i64, i32);
    let rows: Vec<F> = st
        .query_map(params![start.to_string(), end.to_string()], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        })?
        .collect::<std::result::Result<_, _>>()?;
    rows.into_iter()
        .map(
            |(f, prestamo, nif, nombre, provincia, banco, int, ret, bp)| {
                Ok(InteresPagado {
                    fecha: fecha(&f)?,
                    prestamo,
                    nif,
                    nombre,
                    provincia,
                    entidad_credito: banco,
                    intereses: Money::from_cents(int),
                    retencion: Money::from_cents(ret),
                    tipo: if ret > 0 {
                        Rate::from_bp(bp)
                    } else {
                        Rate::ZERO
                    },
                })
            },
        )
        .collect()
}

// ---------- Contrato ----------

/// Borrador del contrato de préstamo en RTF con los datos de la ficha y su cuadro, para
/// revisar y firmar (lo que no sale de la ficha va entre «[…]»).
///
/// `TODO(VERIFY)`: es un modelo general; un préstamo con garantías, con un socio único (libro
/// de contratos, art. 16 LSC) o con un administrador (art. 230 LSC: dispensa de la junta) puede
/// necesitar cláusulas o acuerdos que no están aquí.
pub fn contract_rtf(v: &Vault, id: i64) -> Result<String> {
    Ok(crate::memoria::to_rtf(&contract_paragraphs(v, id)?))
}

fn contract_paragraphs(v: &Vault, id: i64) -> Result<Vec<Para>> {
    let conn = v.connection();
    let p = cargar(conn, id)?.p;
    let info = crate::companies::info(v)?;
    let empresa = info.name.clone();
    let nif_empresa = info.nif.clone().unwrap_or_else(|| "[NIF]".into());
    let domicilio = crate::invoicing::settings(v)?
        .and_then(|s| s.issuer.address.map(|a| a.lines().join(", ")))
        .unwrap_or_else(|| "[domicilio social]".into());
    let c = &p.contraparte;
    let nif_c = if c.nif.trim().is_empty() {
        "[NIF]".to_owned()
    } else {
        c.nif.trim().to_uppercase()
    };
    let sociedad = format!(
        "{empresa}, con NIF {nif_empresa} y domicilio en {domicilio}, representada por [nombre \
         y cargo del representante]"
    );
    let otra = if c.persona_fisica {
        format!(
            "D./D.ª {}, mayor de edad, con NIF {nif_c} y domicilio en [domicilio]",
            c.nombre.trim()
        )
    } else {
        format!(
            "{}, con NIF {nif_c} y domicilio en [domicilio], representada por [nombre y cargo \
             del representante]",
            c.nombre.trim()
        )
    };
    let (prestamista, prestatario) = match p.sentido {
        Sentido::Concedido => (sociedad, otra),
        Sentido::Recibido => (otra, sociedad),
    };
    let cuadro = calc::cuadro(&p);
    let ultima = cuadro.last().map_or(p.inicio, |q| q.fecha);
    let periodicidad = match p.periodicidad {
        1 => "mensuales",
        3 => "trimestrales",
        6 => "semestrales",
        _ => "anuales",
    };
    let mut out = vec![
        Para::Title("CONTRATO DE PRÉSTAMO".into()),
        Para::Text(format!("En [lugar], a {}.", fmt_fecha(p.inicio))),
        Para::Text(
            "BORRADOR preparado con TaxPain a partir de la ficha del préstamo: revísalo y \
             completa lo que va entre corchetes antes de firmarlo."
                .into(),
        ),
        Para::Bold("REUNIDOS".into()),
        Para::Text(format!("De una parte, como PRESTAMISTA, {prestamista}.")),
        Para::Text(format!("De otra parte, como PRESTATARIO, {prestatario}.")),
        Para::Text(
            "Ambas partes se reconocen capacidad suficiente para otorgar este contrato y".into(),
        ),
        Para::Bold("ACUERDAN".into()),
        Para::Text(format!(
            "PRIMERA. Objeto. El prestamista presta al prestatario la cantidad de {} \
             ({}), que el prestatario declara recibir {}.",
            p.principal,
            "[importe en letra]",
            if p.inicio <= taxpain_core::spain_time::peninsular_date(OffsetDateTime::now_utc()) {
                "en este acto mediante transferencia bancaria [o la forma de entrega]"
            } else {
                "el día de la fecha mediante transferencia bancaria [o la forma de entrega]"
            }
        )),
    ];
    out.push(Para::Text(if p.tipo.bp() == 0 {
        "SEGUNDA. Intereses. El préstamo no devenga intereses [motivo; si las partes son \
         vinculadas, téngase en cuenta el art. 18 LIS]."
            .to_owned()
    } else {
        format!(
            "SEGUNDA. Intereses. El capital pendiente devenga un interés nominal anual del {}, \
             que se liquida con cada cuota en proporción al periodo (tipo anual × meses del \
             periodo / 12).{}",
            p.tipo,
            if c.vinculada() {
                " Las partes declaran que es el tipo que acordarían partes independientes en \
                 condiciones comparables (art. 18 LIS)."
            } else {
                ""
            }
        )
    }));
    let devolucion = match p.sistema {
        Sistema::AlVencimiento => format!(
            "TERCERA. Devolución. El capital y los intereses se devuelven en un solo pago el {}.",
            fmt_fecha(ultima)
        ),
        s => {
            let mut t = format!(
                "TERCERA. Devolución. El préstamo se devuelve en {} cuotas {periodicidad} por el \
                 sistema {}, la primera el {} y la última el {}",
                cuadro.len(),
                s.nombre(),
                cuadro.first().map_or(String::new(), |q| fmt_fecha(q.fecha)),
                fmt_fecha(ultima)
            );
            if p.carencia > 0 {
                t.push_str(&format!(
                    ", con {} meses de carencia {}",
                    p.carencia,
                    match p.carencia_tipo {
                        TipoCarencia::Capital => "de capital (sólo se pagan los intereses)",
                        TipoCarencia::Total =>
                            "total (los intereses se suman al capital pendiente)",
                    }
                ));
            }
            t.push_str(", según el cuadro que se adjunta.");
            t
        }
    };
    out.push(Para::Text(devolucion));
    out.push(Para::Text(
        "CUARTA. Amortización anticipada. El prestatario puede devolver anticipadamente todo o \
         parte del capital pendiente [sin comisión / con una comisión del …], con los intereses \
         devengados hasta ese día."
            .into(),
    ));
    out.push(Para::Text(if p.retencion.bp() > 0 {
        format!(
            "QUINTA. Retenciones. El pagador de los intereses practicará sobre ellos la \
             retención a cuenta que corresponda ({} a la fecha de este contrato) y la ingresará \
             en la Agencia Tributaria.",
            p.retencion
        )
    } else {
        "QUINTA. Retenciones. Sobre los intereses se practicarán las retenciones que exija la \
         normativa en cada momento."
            .to_owned()
    }));
    out.push(Para::Text(
        "SEXTA. Gastos y tributos. Los gastos de este contrato son a cargo del prestatario. El \
         préstamo está exento del Impuesto sobre Transmisiones Patrimoniales (art. 45.I.B.15 \
         TRLITPAJD) [cuando proceda, se presentará el modelo 600]."
            .into(),
    ));
    out.push(Para::Text(
        "SÉPTIMA. Incumplimiento. La falta de pago de [dos] cuotas permite al prestamista dar \
         por vencido el préstamo y reclamar el capital pendiente y los intereses."
            .into(),
    ));
    out.push(Para::Text(
        "OCTAVA. Ley y jurisdicción. Este contrato se rige por la ley española (arts. 1740 y \
         siguientes del Código Civil y, siendo mercantil, arts. 311 y siguientes del Código de \
         Comercio). Las partes se someten a los juzgados y tribunales de [ciudad]."
            .into(),
    ));
    out.push(Para::Text(
        "Y en prueba de conformidad, firman este contrato por duplicado en el lugar y la fecha \
         indicados."
            .into(),
    ));
    out.push(Para::Gap);
    out.push(Para::Row("EL PRESTAMISTA".into(), "EL PRESTATARIO".into()));
    out.push(Para::Gap);
    out.push(Para::Gap);
    out.push(Para::Bold("ANEXO. Cuadro de amortización".into()));
    out.push(Para::Text(format!(
        "Principal {} · interés nominal anual {} · inicio {}",
        p.principal,
        p.tipo,
        fmt_fecha(p.inicio)
    )));
    for q in &cuadro {
        out.push(Para::Row(
            format!(
                "{}. {} · capital {} · intereses {}{}",
                q.n,
                fmt_fecha(q.fecha),
                q.capital,
                q.intereses,
                if q.capitalizados.is_zero() {
                    String::new()
                } else {
                    " (capitalizados)".to_owned()
                }
            ),
            format!("{} · pendiente {}", q.pago, q.pendiente),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn el_interes_legal_es_el_de_la_tabla_hasta_2027() {
        for y in 2023..=taxpain_core::parametros::ULTIMO_EJERCICIO_PREVISTO {
            // Millonésimas a centésimas de punto: 32.500 → 325.
            assert_eq!(
                i64::from(INTERES_LEGAL.bp()),
                taxpain_core::parametros::interes_legal(y) / 100,
                "{y}"
            );
        }
    }

    pub(crate) fn ficha(sentido: Sentido) -> Prestamo {
        let contraparte = Contraparte {
            origen: OrigenContraparte::Participada,
            nif: "B12345674".into(),
            nombre: "Participada Demo SL".into(),
            provincia: "46".into(),
            vinculo: Vinculo::Grupo,
            ..Default::default()
        };
        let pr = propuesta(sentido, &contraparte);
        Prestamo {
            id: 0,
            nombre: "Préstamo demo".into(),
            sentido,
            contraparte,
            inicio: date!(2026 - 03 - 01),
            principal: Money::from_euros(24_000, 0),
            tipo: Rate::from_bp(400),
            sistema: Sistema::Lineal,
            periodicidad: 1,
            plazo: 24,
            carencia: 0,
            carencia_tipo: TipoCarencia::Capital,
            cuentas: pr.cuentas,
            retencion: pr.retencion,
            notas: String::new(),
        }
    }

    #[test]
    fn propone_cuentas_y_retencion() {
        let c = Contraparte {
            nombre: "Banco".into(),
            entidad_credito: true,
            ..Default::default()
        };
        let pr = propuesta(Sentido::Recibido, &c);
        assert_eq!(pr.cuentas.largo.as_str(), "1700001");
        assert_eq!(pr.cuentas.devengo.as_str(), "527");
        assert_eq!(pr.retencion, Rate::ZERO);
        let socio = Contraparte {
            nombre: "Carmen".into(),
            persona_fisica: true,
            vinculo: Vinculo::Otra,
            ..Default::default()
        };
        let pr = propuesta(Sentido::Recibido, &socio);
        assert_eq!(pr.cuentas.largo.as_str(), "1635001");
        assert_eq!(pr.retencion, RETENCION_GENERAL);
        let pr = propuesta(Sentido::Concedido, &socio);
        assert_eq!(pr.cuentas.corto.as_str(), "5325001");
        assert_eq!(pr.retencion, Rate::ZERO);
        let f = ficha(Sentido::Concedido);
        assert_eq!(f.cuentas.largo.as_str(), "2423001");
        assert_eq!(f.retencion, RETENCION_GENERAL);
        comprobar(&f).unwrap();
        comprobar(&ficha(Sentido::Recibido)).unwrap();
    }

    #[test]
    fn comprueba_cuentas_y_datos() {
        let mut f = ficha(Sentido::Concedido);
        f.cuentas.largo = acc("1700001");
        assert!(comprobar(&f).is_err());
        let mut f = ficha(Sentido::Recibido);
        f.cuentas.largo = acc("551");
        assert!(comprobar(&f).is_err(), "cuenta corriente sólo en un lado");
        f.cuentas.corto = acc("551");
        comprobar(&f).unwrap();
        f.contraparte.nif = "X".into();
        assert!(comprobar(&f).is_err());
        f.contraparte.nif = String::new();
        assert!(comprobar(&f).is_err(), "con retención hace falta el NIF");
        f.retencion = Rate::ZERO;
        comprobar(&f).unwrap();
        f.contraparte.provincia = "60".into();
        assert!(comprobar(&f).is_err());
    }

    #[test]
    fn avisa_del_valor_de_mercado() {
        let mut f = ficha(Sentido::Concedido);
        f.tipo = Rate::ZERO;
        let a = avisos(&f);
        assert!(a.iter().any(|x| x.contains("art. 18.1 LIS")), "{a:?}");
        f.tipo = Rate::from_bp(100);
        assert!(avisos(&f).iter().any(|x| x.contains("interés legal")));
        f.tipo = Rate::from_bp(500);
        assert!(avisos(&f).is_empty(), "{:?}", avisos(&f));
        // Socio persona física sin interés: IRPF del socio.
        f.contraparte.persona_fisica = true;
        f.contraparte.vinculo = Vinculo::Otra;
        f.retencion = Rate::ZERO;
        f.tipo = Rate::ZERO;
        assert!(avisos(&f).iter().any(|x| x.contains("IRPF del socio")));
        // Recibido de un tercero sin retención.
        let mut r = ficha(Sentido::Recibido);
        r.retencion = Rate::ZERO;
        assert!(
            avisos(&r)
                .iter()
                .any(|x| x.contains("llevan retención del 19 %"))
        );
    }
}

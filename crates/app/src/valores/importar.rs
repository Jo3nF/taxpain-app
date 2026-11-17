//! Importación de lo que dan los brókeres: el PDF de XTB, el Flex de Interactive Brokers (XML o
//! CSV, descargado a mano o por el Flex Web Service). Primero se lee ([`leer`], sin guardar
//! nada) y se enseña; con la conformidad se aplica ([`aplicar`]) sin duplicar:
//! - cada operación lleva una huella con el identificador del bróker (el ID de la operación en
//!   efectivo de XTB, el `transactionID` de IBKR): la misma operación otra vez (el mismo PDF dos
//!   veces, PDF o extractos que se solapan) se salta;
//! - en XTB las compras y ventas salen de las operaciones en efectivo; si un PDF sólo trae las
//!   posiciones cerradas, se leen de ahí, y las que luego llegan con las operaciones en efectivo
//!   las sustituyen (mismo valor, día y precio) en vez de sumarse.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use taxpain_core::cripto::{Precio, Qty};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use super::{Broker, Clase, Op, TipoOp, asegurar_cuenta, asegurar_valor, check_op, insert_op};
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Un valor tal como lo nombra el bróker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValorLeido {
    /// Clave en el extracto: el ISIN si lo hay; si no, el ticker.
    pub clave: String,
    #[serde(default)]
    pub isin: String,
    pub ticker: String,
    #[serde(default)]
    pub nombre: String,
    pub divisa: String,
    #[serde(default)]
    pub clase: Clase,
    #[serde(default)]
    pub pais: String,
}

/// Una operación leída (ya en euros cuando el bróker da el cambio).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpLeida {
    pub fecha: Date,
    pub hora: String,
    pub tipo: TipoOp,
    /// Clave del valor en el extracto.
    #[serde(default)]
    pub valor: Option<String>,
    #[serde(default)]
    pub cantidad: Qty,
    #[serde(default)]
    pub precio: Option<Precio>,
    pub divisa: String,
    pub importe: Money,
    #[serde(default)]
    pub comision: Money,
    /// Euros por unidad de la divisa (vacío en euros).
    #[serde(default)]
    pub cambio: Option<Precio>,
    pub euros: Money,
    #[serde(default)]
    pub comision_eur: Money,
    #[serde(default)]
    pub pais: String,
    #[serde(default)]
    pub nota: String,
    /// `ibkr`, `xtb` o `xtb-posiciones`.
    pub fuente: String,
    pub huella: String,
    /// Huellas de las comisiones que van sumadas en `comision` (la SEC fee de una venta de XTB,
    /// que el informe trae como operación aparte): si luego llegan solas, ya están.
    #[serde(default)]
    pub comision_huella: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrecioLeido {
    pub fecha: Date,
    pub valor: String,
    pub precio: Precio,
    pub divisa: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CambioLeido {
    pub fecha: Date,
    pub divisa: String,
    pub eur_por_unidad: Precio,
}

/// Títulos de un valor en la cuenta a una fecha, según el bróker (para cuadrar).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PosicionLeida {
    pub fecha: Date,
    pub valor: String,
    pub cantidad: Qty,
}

/// Lo leído de un fichero o de una descarga, de una cuenta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extracto {
    pub broker: Broker,
    pub cuenta: String,
    pub divisa: String,
    #[serde(default)]
    pub desde: Option<Date>,
    #[serde(default)]
    pub hasta: Option<Date>,
    /// Periodo de las operaciones en efectivo (XTB): lo que sustituye a lo leído de posiciones.
    #[serde(default)]
    pub efectivo: Option<(Date, Date)>,
    #[serde(default)]
    pub valores: Vec<ValorLeido>,
    #[serde(default)]
    pub ops: Vec<OpLeida>,
    #[serde(default)]
    pub precios: Vec<PrecioLeido>,
    #[serde(default)]
    pub cambios: Vec<CambioLeido>,
    #[serde(default)]
    pub posiciones: Vec<PosicionLeida>,
    #[serde(default)]
    pub avisos: Vec<String>,
}

impl Extracto {
    pub fn nuevo(broker: Broker, cuenta: &str, divisa: &str) -> Self {
        Extracto {
            broker,
            cuenta: cuenta.to_owned(),
            divisa: divisa.to_owned(),
            desde: None,
            hasta: None,
            efectivo: None,
            valores: Vec::new(),
            ops: Vec::new(),
            precios: Vec::new(),
            cambios: Vec::new(),
            posiciones: Vec::new(),
            avisos: Vec::new(),
        }
    }

    /// Primera y última fecha de las operaciones.
    pub fn fechas(&self) -> (Option<Date>, Option<Date>) {
        (
            self.ops.iter().map(|o| o.fecha).min(),
            self.ops.iter().map(|o| o.fecha).max(),
        )
    }

    /// «12 compras, 11 ventas, 4 intereses…».
    pub fn resumen(&self) -> String {
        let mut n: BTreeMap<&'static str, usize> = BTreeMap::new();
        for o in &self.ops {
            *n.entry(o.tipo.label()).or_default() += 1;
        }
        let mut partes: Vec<String> = TipoOp::TODOS
            .iter()
            .filter_map(|t| n.get(t.label()).map(|c| format!("{c} × {}", t.label())))
            .collect();
        if !self.precios.is_empty() {
            partes.push(format!("{} precios", self.precios.len()));
        }
        if partes.is_empty() {
            "sin operaciones".into()
        } else {
            partes.join(", ")
        }
    }
}

/// Lectura de uno o varios ficheros, para enseñarla antes de importar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lectura {
    pub ficheros: Vec<Fichero>,
    pub extractos: Vec<Extracto>,
    pub nuevas: usize,
    pub duplicadas: usize,
    pub precios: usize,
    pub avisos: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fichero {
    pub nombre: String,
    /// `XTB (PDF)`, `Interactive Brokers (Flex XML)`… o vacío si no se ha podido leer.
    pub formato: String,
    pub cuenta: String,
    pub resumen: String,
    pub desde: Option<Date>,
    pub hasta: Option<Date>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Lo hecho al aplicar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aplicado {
    pub nuevas: usize,
    pub duplicadas: usize,
    /// Leídas de posiciones que ya estaban con las operaciones en efectivo (o al revés).
    pub sustituidas: usize,
    pub precios: usize,
    pub cuentas_nuevas: usize,
    pub avisos: Vec<String>,
}

fn huella_fichero(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Qué es un fichero (sin abrir el almacén). `None` si no es de un bróker que se entienda.
pub fn reconocer(name: &str, bytes: &[u8]) -> Option<&'static str> {
    let lower = name.to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "pdf" => crate::pdf_text::text(bytes)
            .filter(|t| super::xtb::es_informe(t))
            .map(|_| "XTB (PDF)"),
        "xlsx" => super::xtb_xlsx::es_informe(bytes).then_some("XTB (Excel)"),
        "xml" => {
            let t = String::from_utf8_lossy(bytes);
            super::flex::es_flex_xml(&t).then_some("Interactive Brokers (Flex XML)")
        }
        "csv" | "txt" => {
            let t = String::from_utf8_lossy(bytes);
            super::flex::es_flex_csv(&t).then_some("Interactive Brokers (Flex CSV)")
        }
        _ => None,
    }
}

/// Lee un fichero de bróker: el PDF de XTB (su texto; si no lo tiene, el que reconoce el Mac),
/// el Excel de XTB, el Flex XML o CSV de IBKR. Error con el motivo si no se entiende.
pub fn leer_fichero(name: &str, bytes: &[u8]) -> std::result::Result<Vec<Extracto>, String> {
    let lower = name.to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "pdf" => {
            let texto = crate::pdf_text::text(bytes)
                .filter(|t| super::xtb::es_informe(t))
                .or_else(|| {
                    crate::carpeta::ocr_text(name, bytes).filter(|t| super::xtb::es_informe(t))
                })
                .ok_or_else(|| {
                    "no es un informe de XTB que se pueda leer (exporta de nuevo el PDF desde \
                     «Historial de la cuenta» de xStation 5)"
                        .to_owned()
                })?;
            Ok(vec![super::xtb::leer(&texto)?])
        }
        "xlsx" => Ok(vec![super::xtb_xlsx::leer(name, bytes)?]),
        "xml" => super::flex::leer_xml(&String::from_utf8_lossy(bytes)),
        "csv" | "txt" => super::flex::leer_csv(&String::from_utf8_lossy(bytes)),
        _ => Err("formato no admitido: PDF o Excel de XTB, o Flex de IBKR (XML o CSV)".into()),
    }
}

/// Lee varios ficheros (nombre y contenido) y dice qué entraría.
pub fn leer(v: &Vault, ficheros: &[(String, Vec<u8>)]) -> Result<Lectura> {
    let mut l = leer_ficheros(ficheros);
    contar(v, &mut l)?;
    Ok(l)
}

/// Lee los ficheros sin el almacén (lo lento: el texto del PDF o el del Mac). Falta [`contar`].
pub fn leer_ficheros(ficheros: &[(String, Vec<u8>)]) -> Lectura {
    let mut out = Lectura {
        ficheros: Vec::new(),
        extractos: Vec::new(),
        nuevas: 0,
        duplicadas: 0,
        precios: 0,
        avisos: Vec::new(),
    };
    for (name, bytes) in ficheros {
        match leer_fichero(name, bytes) {
            Ok(ex) => {
                let lower = name.to_lowercase();
                let formato = if lower.ends_with(".pdf") {
                    "XTB (PDF)"
                } else if lower.ends_with(".xlsx") {
                    "XTB (Excel)"
                } else if lower.ends_with(".xml") {
                    "Interactive Brokers (Flex XML)"
                } else {
                    "Interactive Brokers (Flex CSV)"
                };
                for e in ex {
                    let (desde, hasta) = e.fechas();
                    out.ficheros.push(Fichero {
                        nombre: name.clone(),
                        formato: formato.into(),
                        cuenta: e.cuenta.clone(),
                        resumen: e.resumen(),
                        desde,
                        hasta,
                        error: None,
                    });
                    out.extractos.push(e);
                }
            }
            Err(motivo) => out.ficheros.push(Fichero {
                nombre: name.clone(),
                formato: String::new(),
                cuenta: String::new(),
                resumen: String::new(),
                desde: None,
                hasta: None,
                error: Some(motivo),
            }),
        }
    }
    out
}

/// Cuenta lo nuevo, lo que ya estaba (y lo repetido entre los propios ficheros) y los precios
/// que faltan.
pub fn contar(v: &Vault, out: &mut Lectura) -> Result<()> {
    out.nuevas = 0;
    out.duplicadas = 0;
    out.precios = 0;
    out.avisos.clear();
    let conn = v.connection();
    let mut vistas: BTreeSet<String> = BTreeSet::new();
    for e in &out.extractos {
        for o in &e.ops {
            if !vistas.insert(o.huella.clone()) || ya_esta(conn, &e.cuenta, e.broker, o)? {
                out.duplicadas += 1;
            } else {
                out.nuevas += 1;
                out.avisos.extend(falta_anterior(e, o));
            }
        }
        for p in &e.precios {
            if !precio_guardado(conn, p)? {
                out.precios += 1;
            }
        }
        out.avisos.extend(e.avisos.iter().cloned());
    }
    Ok(())
}

/// ¿Ya está ese precio (mismo valor, día y precio)? Uno puesto a mano ese día también cuenta:
/// el del bróker no lo pisa.
fn precio_guardado(conn: &Connection, p: &PrecioLeido) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM security_price sp JOIN security s ON s.id = sp.security_id
             WHERE sp.date = ?1 AND (sp.price = ?2 OR sp.source = 'manual')
               AND (s.isin = ?3 OR s.ticker = ?3)",
            params![
                p.fecha.to_string(),
                p.precio.to_text(),
                p.valor.to_uppercase()
            ],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .is_some())
}

/// ¿Hay ya una operación con esa huella, o que la lleva sumada (la SEC fee de una venta de XTB,
/// que va en la comisión de la venta con su huella en la nota)?
fn existe_huella(conn: &Connection, huella: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM security_op WHERE fingerprint = ?1
             UNION ALL
             SELECT 1 FROM security_op WHERE instr(note, '(' || ?1 || ')') > 0
             LIMIT 1",
            [huella],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .is_some())
}

/// ¿Ya está? Por la huella o, si es una compra o venta de XTB leída de posiciones, porque ya
/// está la misma con las operaciones en efectivo (mismo valor y día, y el mismo precio o los
/// mismos títulos: el precio de apertura de la posición es el medio de sus compras).
fn ya_esta(conn: &Connection, cuenta: &str, broker: Broker, o: &OpLeida) -> Result<bool> {
    if existe_huella(conn, &o.huella)? {
        return Ok(true);
    }
    if o.fuente != "xtb-posiciones" {
        return Ok(false);
    }
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM security_op o JOIN security_account a ON a.id = o.account_id
         JOIN security s ON s.id = o.security_id
         WHERE a.broker = ?1 AND a.number = ?2 AND o.source = 'xtb' AND o.kind = ?3
           AND o.date = ?4 AND (o.price = ?5 OR o.qty = ?7) AND s.ticker = ?6",
        params![
            broker.code(),
            cuenta,
            o.tipo.code(),
            o.fecha.to_string(),
            o.precio.map(Precio::to_text),
            o.valor.as_deref().unwrap_or_default().to_uppercase(),
            o.cantidad.to_text()
        ],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Una compra de XTB que no está en las operaciones en efectivo del informe (es de antes de su
/// periodo) y se toma de las posiciones: falta el informe anterior.
fn falta_anterior(e: &Extracto, o: &OpLeida) -> Option<String> {
    (e.broker == Broker::Xtb
        && o.fuente == "xtb-posiciones"
        && o.tipo == TipoOp::Compra
        && e.efectivo.is_some_and(|(a, _)| o.fecha < a))
    .then(|| {
        format!(
            "Falta el informe anterior con la compra de {} ({} títulos el {}): se toma de las \
             posiciones del informe. Importa también el de ese periodo (lo que ya esté no se \
             duplica).",
            o.valor.as_deref().unwrap_or("?"),
            o.cantidad,
            o.fecha
        )
    })
}

/// Guarda lo leído. `kind`: `flex`, `api` o `xtb-pdf` (para la última sincronización).
pub fn aplicar(
    v: &Vault,
    extractos: &[Extracto],
    kind: &str,
    file_sha: &str,
    now: OffsetDateTime,
) -> Result<Aplicado> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let ts = now_str(now);
    let mut out = Aplicado::default();
    for e in extractos {
        // Cada extracto con lo suyo (un PDF de XTB y un Flex de IBKR pueden ir juntos).
        let kind = match (e.broker, kind) {
            (Broker::Xtb, _) => "xtb-pdf",
            (_, "xtb-pdf") => "flex",
            (_, k) => k,
        };
        let (cuenta, nueva) = asegurar_cuenta(&tx, e.broker, &e.cuenta, &e.divisa, &ts)?;
        if nueva {
            out.cuentas_nuevas += 1;
        }
        let mut ids: BTreeMap<String, i64> = BTreeMap::new();
        for l in &e.valores {
            ids.insert(l.clave.clone(), asegurar_valor(&tx, l, &ts)?);
        }
        let (mut nuevas, mut dup) = (0usize, 0usize);
        for o in &e.ops {
            if ya_esta(&tx, &e.cuenta, e.broker, o)? {
                dup += 1;
                sumar_comision(v, &tx, o)?;
                continue;
            }
            out.avisos.extend(falta_anterior(e, o));
            let security_id = match &o.valor {
                Some(k) => Some(match ids.get(k) {
                    Some(id) => *id,
                    None => {
                        let l = ValorLeido {
                            clave: k.clone(),
                            isin: String::new(),
                            ticker: k.clone(),
                            nombre: String::new(),
                            divisa: e.divisa.clone(),
                            clase: Clase::Accion,
                            pais: String::new(),
                        };
                        let id = asegurar_valor(&tx, &l, &ts)?;
                        ids.insert(k.clone(), id);
                        id
                    }
                }),
                None => None,
            };
            // Las compras y ventas de XTB de las operaciones en efectivo sustituyen a las mismas
            // leídas antes de las posiciones (sin asiento).
            if o.fuente == "xtb" && matches!(o.tipo, TipoOp::Compra | TipoOp::Venta) {
                out.sustituidas += tx.execute(
                    "DELETE FROM security_op WHERE account_id = ?1 AND source = 'xtb-posiciones'
                       AND kind = ?2 AND security_id = ?3 AND date = ?4
                       AND (price = ?5 OR qty = ?6) AND entry_id IS NULL",
                    params![
                        cuenta,
                        o.tipo.code(),
                        security_id,
                        o.fecha.to_string(),
                        o.precio.map(Precio::to_text),
                        o.cantidad.to_text()
                    ],
                )?;
            }
            // En otra divisa y sin el cambio del extracto: el del día que ya haya (del bróker o
            // del BCE descargado); si no hay, queda sin euros y el informe lo avisa.
            let (mut fx, mut eur, mut fee_eur) = (o.cambio, o.euros, o.comision_eur);
            if o.divisa != "EUR"
                && fx.is_none()
                && eur.is_zero()
                && (!o.importe.is_zero() || !o.comision.is_zero())
                && let Some((_, f)) = super::cambio_a(v, &o.divisa, o.fecha)?
            {
                fx = Some(f);
                eur = super::a_euros(o.importe, f);
                fee_eur = super::a_euros(o.comision, f);
            }
            let op = Op {
                id: 0,
                account_id: cuenta,
                to_account_id: None,
                security_id,
                date: o.fecha,
                time: o.hora.clone(),
                kind: o.tipo,
                qty: o.cantidad,
                price: o.precio,
                currency: o.divisa.clone(),
                amount: o.importe,
                fee: o.comision,
                fx,
                eur,
                fee_eur,
                country: o.pais.clone(),
                note: o.nota.clone(),
                source: o.fuente.clone(),
                fingerprint: Some(o.huella.clone()),
                entry_id: None,
            };
            check_op(&op).map_err(|err| {
                AppError::Other(format!(
                    "{} del {} ({}): {err}",
                    o.tipo.label(),
                    o.fecha,
                    o.valor.as_deref().unwrap_or("sin valor")
                ))
            })?;
            insert_op(&tx, &op, &ts)?;
            nuevas += 1;
        }
        for p in &e.precios {
            let Some(id) = ids.get(&p.valor).copied() else {
                continue;
            };
            if super::poner_precio(
                &tx,
                &super::PrecioCierre {
                    date: p.fecha,
                    security_id: id,
                    price: p.precio,
                    currency: p.divisa.clone(),
                    source: e.broker.code().into(),
                },
                &ts,
            )? {
                out.precios += 1;
            }
        }
        for c in &e.cambios {
            super::poner_cambio(&tx, c.fecha, &c.divisa, c.eur_por_unidad, e.broker.code())?;
        }
        tx.execute(
            "INSERT INTO security_import(account_id, kind, file_sha256, cash_from, cash_to, ops_new,
               ops_dup, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                cuenta,
                kind,
                file_sha,
                e.efectivo.map(|(a, _)| a.to_string()),
                e.efectivo.map(|(_, b)| b.to_string()),
                i64::try_from(nuevas).unwrap_or(i64::MAX),
                i64::try_from(dup).unwrap_or(i64::MAX),
                ts
            ],
        )?;
        let nota = format!(
            "{} · {nuevas} nuevas, {dup} ya estaban",
            match kind {
                "api" => "Sincronizado con el Flex Web Service",
                "xtb-pdf" => "Informe de XTB importado",
                _ => "Fichero Flex importado",
            }
        );
        tx.execute(
            "UPDATE security_account SET last_sync = ?1, last_sync_note = ?2 WHERE id = ?3",
            params![ts, nota, cuenta],
        )?;
        out.nuevas += nuevas;
        out.duplicadas += dup;
        out.avisos.extend(e.avisos.iter().cloned());
    }
    audit::append(
        &tx,
        &ts,
        Actor::User,
        "valores.importar",
        kind,
        &serde_json::json!({
            "nuevas": out.nuevas,
            "duplicadas": out.duplicadas,
            "sustituidas": out.sustituidas,
            "precios": out.precios,
        }),
    )?;
    tx.commit()?;
    // Cuadre con las posiciones que dice el bróker (con todo ya guardado).
    for e in extractos {
        out.avisos.extend(cuadre(v, e)?);
    }
    Ok(out)
}

/// Una venta que ya estaba llega ahora con su comisión aparte sumada (la SEC fee del día
/// siguiente, que el informe anterior no traía): se le suma a la guardada, si no la tiene ya y
/// no está contabilizada.
fn sumar_comision(v: &Vault, conn: &Connection, o: &OpLeida) -> Result<()> {
    if o.comision_huella.is_empty() || o.comision.is_zero() {
        return Ok(());
    }
    let row: Option<(i64, String, Option<String>, String)> = conn
        .query_row(
            "SELECT id, currency, fx, note FROM security_op
             WHERE fingerprint = ?1 AND entry_id IS NULL",
            [&o.huella],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((id, divisa, fx, nota)) = row else {
        return Ok(());
    };
    if o.comision_huella
        .iter()
        .any(|h| nota.contains(&format!("({h})")))
    {
        return Ok(());
    }
    let fee_eur = if divisa == "EUR" {
        o.comision
    } else {
        match fx.as_deref().and_then(Precio::parse) {
            Some(f) => super::a_euros(o.comision, f),
            None => super::cambio_a(v, &divisa, o.fecha)?
                .map(|(_, f)| super::a_euros(o.comision, f))
                .unwrap_or_default(),
        }
    };
    let extra: Vec<String> = o.comision_huella.iter().map(|h| format!("({h})")).collect();
    conn.execute(
        "UPDATE security_op SET fee = fee + ?1, fee_eur = fee_eur + ?2,
           note = note || ' · comisión ' || ?3 WHERE id = ?4",
        params![o.comision.cents(), fee_eur.cents(), extra.join(" "), id],
    )?;
    Ok(())
}

/// Importa un fichero de bróker (lo que hace la carpeta de la empresa).
pub fn importar_fichero(
    v: &Vault,
    name: &str,
    bytes: &[u8],
    now: OffsetDateTime,
) -> Result<Aplicado> {
    let ex = leer_fichero(name, bytes).map_err(AppError::Other)?;
    let kind = if ex.iter().any(|e| e.broker == Broker::Xtb) {
        "xtb-pdf"
    } else {
        "flex"
    };
    aplicar(v, &ex, kind, &huella_fichero(bytes), now)
}

/// Periodo que se sincroniza con el Flex Web Service para el ejercicio `year`: del 1 de enero
/// al 31 de diciembre (o hasta hoy, si es el año en curso).
pub fn periodo_sincronizacion(year: i32, hoy: Date) -> Result<(Date, Date)> {
    let desde = Date::from_calendar_date(year, time::Month::January, 1)
        .map_err(|e| AppError::Other(e.to_string()))?;
    let fin = Date::from_calendar_date(year, time::Month::December, 31)
        .map_err(|e| AppError::Other(e.to_string()))?;
    if desde > hoy {
        return Err(AppError::Other(format!(
            "{year} aún no ha empezado: no hay nada que sincronizar"
        )));
    }
    // IBKR da los datos hasta el último día hábil cerrado: hasta ayer.
    let ayer = hoy.previous_day().unwrap_or(hoy);
    Ok((desde, fin.min(ayer).max(desde)))
}

/// Aplica lo descargado del Flex Web Service para la cuenta `id`: cada extracto ha de ser de esa
/// cuenta (si ya tiene número; si no, se le pone el del extracto) y se guarda sin duplicar.
pub fn aplicar_descarga(
    v: &Vault,
    id: i64,
    textos: &[String],
    now: OffsetDateTime,
) -> Result<Aplicado> {
    let c = super::cuenta(v, id)?;
    let mut extractos = Vec::new();
    for t in textos {
        let ex = if super::flex::es_flex_xml(t) {
            super::flex::leer_xml(t)
        } else {
            super::flex::leer_csv(t)
        }
        .map_err(AppError::Other)?;
        extractos.extend(ex);
    }
    for e in &extractos {
        if !c.number.is_empty() && e.cuenta != c.number {
            return Err(AppError::Other(format!(
                "la Flex Query devuelve la cuenta {}, no la {} de «{}»: revisa el Query ID",
                e.cuenta, c.number, c.name
            )));
        }
    }
    if c.number.is_empty()
        && let Some(e) = extractos.first()
    {
        v.connection().execute(
            "UPDATE security_account SET number = ?1 WHERE id = ?2 AND number = ''",
            params![e.cuenta, id],
        )?;
    }
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "valores.ibkr.sincronizar",
        &id.to_string(),
        &serde_json::json!({ "host": taxpain_banca::ibkr::HOST, "extractos": textos.len() }),
    )?;
    aplicar(v, &extractos, "api", "", now)
}

/// Títulos de cada valor de la cuenta según el bróker frente a los que salen de las
/// operaciones guardadas.
fn cuadre(v: &Vault, e: &Extracto) -> Result<Vec<String>> {
    let mut avisos = Vec::new();
    if e.posiciones.is_empty() {
        return Ok(avisos);
    }
    let conn = v.connection();
    let Some(cuenta) = conn
        .query_row(
            "SELECT id FROM security_account WHERE broker = ?1 AND number = ?2",
            params![e.broker.code(), e.cuenta],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    else {
        return Ok(avisos);
    };
    let fecha = e
        .posiciones
        .iter()
        .map(|p| p.fecha)
        .max()
        .unwrap_or(Date::MIN);
    // Sólo si lo guardado cubre hasta esa fecha (si no, faltan operaciones a la fuerza).
    if e.hasta.is_some_and(|h| h < fecha) {
        return Ok(avisos);
    }
    let mut segun_broker: BTreeMap<String, Qty> = BTreeMap::new();
    for p in e.posiciones.iter().filter(|p| p.fecha == fecha) {
        let q = segun_broker.entry(p.valor.to_uppercase()).or_default();
        *q = q.checked_add(p.cantidad).unwrap_or(*q);
    }
    let ops = super::ops(v, None)?;
    let valores = super::valores(v)?;
    let clave_de = |id: i64| {
        valores
            .iter()
            .find(|x| x.id == id)
            .map(|x| {
                let isin = x.isin.to_uppercase();
                if e.valores.iter().any(|l| l.clave == isin) {
                    isin
                } else {
                    x.ticker.to_uppercase()
                }
            })
            .unwrap_or_default()
    };
    let mut segun_ops: BTreeMap<String, Qty> = BTreeMap::new();
    for o in ops.iter().filter(|o| o.date <= fecha) {
        let Some(sid) = o.security_id else { continue };
        let signo = match o.kind {
            TipoOp::Compra | TipoOp::Split if o.account_id == cuenta => 1,
            TipoOp::Venta if o.account_id == cuenta => -1,
            TipoOp::Traspaso if o.account_id == cuenta => -1,
            TipoOp::Traspaso if o.to_account_id == Some(cuenta) => 1,
            _ => 0,
        };
        if signo == 0 {
            continue;
        }
        let q = segun_ops.entry(clave_de(sid)).or_default();
        *q = if signo > 0 {
            q.checked_add(o.qty).unwrap_or(*q)
        } else {
            q.checked_sub(o.qty).unwrap_or(*q)
        };
    }
    for (k, q) in &segun_broker {
        let nuestro = segun_ops.get(k).copied().unwrap_or_default();
        if nuestro != *q {
            avisos.push(format!(
                "{k}: el bróker dice {q} títulos el {fecha} y las operaciones guardadas dan \
                 {nuestro}. ¿Falta un extracto anterior?"
            ));
        }
    }
    for (k, q) in &segun_ops {
        if !q.is_zero() && !segun_broker.contains_key(k) {
            avisos.push(format!(
                "{k}: las operaciones guardadas dan {q} títulos el {fecha} y el bróker no tiene \
                 ninguno."
            ));
        }
    }
    Ok(avisos)
}

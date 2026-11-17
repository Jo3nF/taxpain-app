//! Guía «Dejar la gestoría» (G24): el plan de puesta en marcha de quien deja de tener la
//! contabilidad y los impuestos en una gestoría, paso a paso, con lo que el usuario marca
//! («hecho» o «no aplica») guardado en `puesta_marcha_paso`.
//!
//! Ordena pantallas que ya existen (datos de facturación y perfil fiscal, certificado, importar
//! la carpeta y el ejercicio del gestor, saldos iniciales, revisión de la carpeta, comparar con
//! la gestoría, calendario) y añade lo que no estaba: el correo para pedir a la gestoría lo que
//! hace falta (sólo el texto, para copiarlo: no se envía nada) y los trámites de salida
//! (revocar el apoderamiento y la representación, los avisos y las domiciliaciones), que se
//! hacen en la Sede o por escrito; TaxPain sólo dice dónde.
//!
//! - Apoderamiento ante la AEAT: arts. 46 LGT y 46 RGAT; registro de apoderamientos, Resolución
//!   de 18 de mayo de 2010 de la AEAT. La colaboración social (arts. 79 a 81 RGAT) se apoya en
//!   el documento de representación firmado con la gestoría.
//! - Conservación de libros y justificantes: art. 30 CCom (seis años) y art. 66 LGT (cuatro).

use serde::{Deserialize, Serialize};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::params;
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoPaso {
    Pendiente,
    Hecho,
    NoAplica,
}

impl EstadoPaso {
    fn code(self) -> &'static str {
        match self {
            EstadoPaso::Pendiente => "pendiente",
            EstadoPaso::Hecho => "hecho",
            EstadoPaso::NoAplica => "no_aplica",
        }
    }
    fn parse(s: &str) -> EstadoPaso {
        match s {
            "hecho" => EstadoPaso::Hecho,
            "no_aplica" => EstadoPaso::NoAplica,
            _ => EstadoPaso::Pendiente,
        }
    }
}

/// Un enlace oficial (la Sede de la AEAT, la DEHú…).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Enlace {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PasoGuia {
    pub id: String,
    pub titulo: String,
    /// Qué hay que hacer, en claro.
    pub texto: String,
    /// Dónde se hace en TaxPain («importar», «contabilidad:apertura»…), si es en la app.
    pub destino: Option<String>,
    pub enlaces: Vec<Enlace>,
    pub estado: EstadoPaso,
    pub fecha: Option<Date>,
    pub nota: String,
    /// TaxPain ve que ya está hecho (no lo marca solo: lo dice).
    pub detectado: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Guia {
    pub pasos: Vec<PasoGuia>,
    /// Hechos o que no aplican.
    pub resueltos: usize,
    /// El usuario ha marcado algún paso: la guía está empezada (el Panel la recuerda).
    pub empezada: bool,
    /// El correo para pedir a la gestoría lo que hace falta, listo para copiar.
    pub correo: Correo,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Correo {
    pub asunto: String,
    pub cuerpo: String,
}

const URL_APODERAMIENTOS: &str = "https://sede.agenciatributaria.gob.es/static_files/common/html/selector_acceso/SelectorAccesos.html?ref=%2Fwlpl%2FITTI-APOD%2FSvApodPoderQuery&aut=CPE";
const URL_NOTIFICACIONES: &str =
    "https://sede.agenciatributaria.gob.es/Sede/procedimientoini/ZN01.shtml";
const URL_DEHU: &str = "https://dehu.redsara.es/";
const URL_SEG_SOCIAL: &str = "https://sede.seg-social.gob.es/";

/// (id, título, texto, destino, enlaces)
type Def = (
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static [(&'static str, &'static str)],
);

const PASOS: [Def; 11] = [
    (
        "empresa",
        "Datos de la empresa y perfil fiscal",
        "Pon los datos de facturación (NIF, razón social, domicilio) y el perfil fiscal: qué \
         modelos presentas y cada cuánto. Si ya tienes modelos de la gestoría importados, el \
         perfil se puede rellenar con lo presentado.",
        Some("ajustes"),
        &[],
    ),
    (
        "certificado",
        "Certificado electrónico",
        "Instala el certificado de representante de la sociedad (o el tuyo, si eres autónomo): \
         con él se consultan los datos de la AEAT, se descargan las notificaciones y se presenta. \
         Hasta ahora lo usaba la gestoría con su propio certificado.",
        Some("ajustes"),
        &[],
    ),
    (
        "pedir",
        "Pedir a la gestoría lo que es tuyo",
        "Copia el correo de abajo y mándaselo tú (TaxPain no envía nada). Pide los libros y el \
         diario en un formato que se pueda importar, los modelos presentados con sus \
         justificantes, las cuentas anuales, las actas, los contratos y las claves y \
         autorizaciones que usan en tu nombre. Hay que conservarlos seis años (art. 30 CCom).",
        None,
        &[],
    ),
    (
        "carpeta",
        "Importar su carpeta",
        "Cuando te lo den, junta todo en una carpeta e impórtala: TaxPain clasifica los \
         documentos y lee los modelos y los libros registro.",
        Some("importar"),
        &[],
    ),
    (
        "ejercicio",
        "Importar el ejercicio del gestor",
        "Carga el diario del ejercicio en curso (y, si quieres, los anteriores) tal como lo \
         exporta su programa: a3, Sage, ContaSOL…",
        Some("importar"),
        &[],
    ),
    (
        "saldos",
        "Fecha de corte y saldos iniciales",
        "Elige la fecha desde la que llevas tú la contabilidad y, si no importas el diario \
         entero, crea los saldos iniciales a esa fecha. Cotéjalos con el último balance y los \
         últimos modelos de la gestoría. Apunta aquí la fecha de corte.",
        Some("contabilidad:apertura"),
        &[],
    ),
    (
        "revisar",
        "Revisar lo que hicieron",
        "Revisa la carpeta (lo que falta, lo que no cuadra) y comprueba lo presentado en la AEAT \
         con tu certificado. Lo que salga, pídeselo a la gestoría antes de despedirte.",
        Some("importar"),
        &[],
    ),
    (
        "paralelo",
        "Un trimestre en paralelo",
        "Si puedes, deja que la gestoría haga un último trimestre mientras lo preparas tú en \
         TaxPain y compara sus modelos con los tuyos («Comparar con la gestoría»).",
        Some("importar"),
        &[],
    ),
    (
        "revocar",
        "Revocar el apoderamiento y la representación",
        "En la Sede de la AEAT, en «Apoderamientos», consulta los poderes que diste a la \
         gestoría y revócalos (con tu certificado; nada se hace desde TaxPain). Si presentaba \
         como colaborador social, la representación es el documento que firmaste con ella: \
         comunícale por escrito que queda sin efecto. En la Seguridad Social, si era tu \
         autorizado RED, pide que se desvincule o cámbialo en su Sede.",
        None,
        &[
            ("Apoderamientos (Sede de la AEAT)", URL_APODERAMIENTOS),
            ("Sede de la Seguridad Social", URL_SEG_SOCIAL),
        ],
    ),
    (
        "avisos",
        "Avisos de notificaciones y domiciliaciones",
        "Pon tu correo para los avisos de notificaciones de la AEAT y de la DEHú (si estaba el \
         de la gestoría, dejarías de enterarte) y revisa la cuenta en la que se domicilian los \
         pagos: la eliges en cada modelo al presentarlo.",
        Some("notificaciones"),
        &[
            (
                "Notificaciones electrónicas (Sede de la AEAT)",
                URL_NOTIFICACIONES,
            ),
            ("Dirección Electrónica Habilitada única (DEHú)", URL_DEHU),
        ],
    ),
    (
        "calendario",
        "Quedarte con el calendario",
        "Revisa el calendario de los próximos doce meses con tu perfil, activa los avisos de \
         plazos y repasa las otras obligaciones de la sociedad (depósito de cuentas, protección \
         de datos…). Cada trimestre, «Cerrar el trimestre» te dice qué falta antes de presentar.",
        Some("calendario"),
        &[],
    ),
];

pub(crate) fn fecha_iso(s: &str) -> Option<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).ok()
}

fn count(v: &Vault, sql: &str) -> i64 {
    v.connection().query_row(sql, [], |r| r.get(0)).unwrap_or(0)
}

/// Lo que TaxPain ve ya hecho de cada paso (el certificado lo mira la interfaz: está en el
/// Llavero, no en la base de datos).
fn detectado(v: &Vault, id: &str) -> Result<Option<String>> {
    let perfil = count(
        v,
        "SELECT count(*) FROM settings WHERE key = 'taxpayer_profile'",
    ) > 0;
    Ok(match id {
        "empresa" => {
            let facturacion = crate::invoicing::settings(v)?.is_some();
            match (facturacion, perfil) {
                (true, true) => Some("Tienes los datos de facturación y el perfil fiscal.".into()),
                (true, false) => None,
                (false, _) => None,
            }
        }
        "carpeta" => {
            let docs = count(v, "SELECT count(*) FROM company_document");
            let modelos = count(v, "SELECT count(*) FROM filed_model");
            (docs + modelos > 0).then(|| {
                format!("Hay {docs} documentos de la empresa y {modelos} modelos importados.")
            })
        }
        "ejercicio" => {
            let n = count(v, "SELECT count(*) FROM gestor_import");
            (n > 0).then(|| format!("Hay {n} diarios del gestor importados."))
        }
        "saldos" => (count(
            v,
            "SELECT (SELECT count(*) FROM opening_balance)
                  + (SELECT count(*) FROM journal_entry WHERE origin_kind = 'Apertura')",
        ) > 0)
            .then(|| "Hay un asiento de apertura.".into()),
        "revisar" => (count(
            v,
            "SELECT count(*) FROM settings WHERE key = 'revision_carpeta'",
        ) > 0)
            .then(|| "Hay una revisión de la carpeta guardada.".into()),
        "calendario" => perfil.then(|| "El calendario ya sale de tu perfil fiscal.".into()),
        _ => None,
    })
}

fn nombre_empresa(v: &Vault) -> Result<(String, String)> {
    Ok(match crate::invoicing::settings(v)? {
        Some(s) => {
            let nif = match &s.issuer.id {
                taxpain_core::invoice::PartyId::Nif(n) => n.clone(),
                _ => String::new(),
            };
            (s.issuer.name, nif)
        }
        None => ("[nombre de la empresa]".into(), "[NIF]".into()),
    })
}

/// El correo para pedir a la gestoría lo que hace falta. Sólo texto: no se envía nada.
pub fn correo(v: &Vault, hoy: Date) -> Result<Correo> {
    let (nombre, nif) = nombre_empresa(v)?;
    let year = hoy.year();
    let cuerpo = format!(
        "Hola:\n\n\
         A partir de ahora llevaremos nosotros la contabilidad y los impuestos de {nombre} \
         (NIF {nif}). Gracias por estos años. Para poder seguir sin huecos, os pedimos, por favor:\n\n\
         1. El libro diario y el mayor de {anterior} y de {year} hasta la fecha, exportados del \
         programa de contabilidad (fichero del diario o Excel), y el balance de sumas y saldos.\n\
         2. Los libros registro de IVA (facturas emitidas y recibidas) de los mismos años.\n\
         3. Los modelos presentados de los últimos cuatro años (303, 390, 111, 115, 123, 190, 180, \
         193, 202, 200, 347, 349 y los que haya), con sus justificantes de presentación y de pago.\n\
         4. Las cuentas anuales depositadas, los libros legalizados y los acuses del Registro \
         Mercantil, y la declaración de titularidad real.\n\
         5. El libro de actas y el de socios, y los contratos que tengáis (alquiler, préstamos, \
         seguros, trabajadores).\n\
         6. El cuadro de amortización del inmovilizado y las cuentas que usáis para cada cosa.\n\
         7. Las notificaciones y requerimientos de la AEAT o de la Seguridad Social que hayáis \
         recibido por nosotros y su estado.\n\
         8. Qué apoderamientos, autorizaciones (colaborador social, autorizado RED) y claves \
         usáis en nuestro nombre, para revocarlos nosotros en la Sede, y la dirección de correo \
         que figura para los avisos.\n\n\
         Si podéis, mandadlo todo en una carpeta comprimida o por un enlace de descarga. Si el \
         último trimestre lo presentáis vosotros, decidnos cuál para no presentarlo dos veces.\n\n\
         Un saludo.\n",
        anterior = year - 1,
    );
    Ok(Correo {
        asunto: format!("{nombre}: documentación para el traspaso de la contabilidad"),
        cuerpo,
    })
}

/// La guía con el estado de cada paso.
pub fn guia(v: &Vault, hoy: Date) -> Result<Guia> {
    let conn = v.connection();
    let mut stmt = conn.prepare("SELECT paso, estado, fecha, nota FROM puesta_marcha_paso")?;
    let guardados: Vec<(String, String, Option<String>, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let mut pasos = Vec::new();
    for (id, titulo, texto, destino, enlaces) in PASOS {
        let g = guardados.iter().find(|g| g.0 == id);
        pasos.push(PasoGuia {
            id: id.into(),
            titulo: titulo.into(),
            texto: texto.into(),
            destino: destino.map(Into::into),
            enlaces: enlaces
                .iter()
                .map(|(label, url)| Enlace {
                    label: (*label).into(),
                    url: (*url).into(),
                })
                .collect(),
            estado: g.map_or(EstadoPaso::Pendiente, |g| EstadoPaso::parse(&g.1)),
            fecha: g.and_then(|g| g.2.as_deref()).and_then(fecha_iso),
            nota: g.map(|g| g.3.clone()).unwrap_or_default(),
            detectado: detectado(v, id)?,
        });
    }
    let resueltos = pasos
        .iter()
        .filter(|p| p.estado != EstadoPaso::Pendiente)
        .count();
    Ok(Guia {
        empezada: !guardados.is_empty(),
        resueltos,
        pasos,
        correo: correo(v, hoy)?,
    })
}

/// Marca un paso de la guía (y su fecha y nota). Queda en la auditoría.
pub fn marcar(
    v: &Vault,
    paso: &str,
    estado: EstadoPaso,
    fecha: Option<Date>,
    nota: &str,
    now: OffsetDateTime,
) -> Result<()> {
    if !PASOS.iter().any(|d| d.0 == paso) {
        return Err(AppError::Other(format!(
            "paso de la guía desconocido: {paso}"
        )));
    }
    let conn = v.connection();
    conn.execute(
        "INSERT INTO puesta_marcha_paso (paso, estado, fecha, nota, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(paso) DO UPDATE SET estado = ?2, fecha = ?3, nota = ?4, updated_at = ?5",
        params![
            paso,
            estado.code(),
            fecha.map(|d| d.to_string()),
            nota.trim(),
            now_str(now)
        ],
    )?;
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "onboarding.step",
        paso,
        &serde_json::json!({ "estado": estado.code(), "fecha": fecha.map(|d| d.to_string()) }),
    )?;
    Ok(())
}

//! Borradores de escritos de un expediente en RTF (Pages, Word) o PDF, con los datos del
//! expediente y de la empresa y huecos «[…]» para lo que escribe el usuario: contestación a un
//! requerimiento con la relación de documentos que se aportan, alegaciones, recurso de
//! reposición, solicitud de aplazamiento o fraccionamiento y solicitud de ampliación del plazo.
//!
//! Nada se presenta ni se envía: el escrito se presenta en el registro electrónico de la sede
//! del organismo (en la AEAT, «Contestar requerimientos o presentar documentación» o el trámite
//! del procedimiento) o, el aplazamiento, con el formulario de la Sede, que pide estos datos.
//! Son modelos generales: no sustituyen el consejo de un asesor (`TODO(VERIFY)` cada caso).

use serde::{Deserialize, Serialize};
use taxpain_invoicing::pdf::{self, Para};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use time::{Date, OffsetDateTime};

use super::{Calculo, Expediente, Organismo, Tipo, cargar, fecha, perfil, titulo};
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Escrito {
    Contestacion,
    Alegaciones,
    RecursoReposicion,
    Aplazamiento,
    Ampliacion,
}

impl Escrito {
    fn code(self) -> &'static str {
        match self {
            Escrito::Contestacion => "contestacion",
            Escrito::Alegaciones => "alegaciones",
            Escrito::RecursoReposicion => "recurso_reposicion",
            Escrito::Aplazamiento => "aplazamiento",
            Escrito::Ampliacion => "ampliacion",
        }
    }

    pub fn nombre(self) -> &'static str {
        match self {
            Escrito::Contestacion => "Contestación al requerimiento",
            Escrito::Alegaciones => "Alegaciones",
            Escrito::RecursoReposicion => "Recurso de reposición",
            Escrito::Aplazamiento => "Solicitud de aplazamiento o fraccionamiento",
            Escrito::Ampliacion => "Solicitud de ampliación del plazo",
        }
    }
}

/// Lo que se elige al preparar el escrito.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opciones {
    /// Motivo (del aplazamiento, de la ampliación): si está vacío, queda el hueco.
    #[serde(default)]
    pub motivo: String,
    /// Fraccionamiento en estos plazos mensuales (vacío o 1: aplazamiento en un solo pago).
    #[serde(default)]
    pub plazos: Option<u32>,
    #[serde(default)]
    pub lugar: String,
}

/// Escritos que tienen sentido para cada tipo de expediente.
pub fn disponibles(tipo: Tipo) -> Vec<Escrito> {
    match tipo {
        Tipo::Requerimiento => vec![Escrito::Contestacion, Escrito::Ampliacion],
        Tipo::PropuestaLiquidacion | Tipo::Audiencia => {
            vec![Escrito::Alegaciones, Escrito::Ampliacion]
        }
        Tipo::PropuestaSancion => vec![Escrito::Alegaciones],
        Tipo::Liquidacion | Tipo::AcuerdoSancionador | Tipo::ProvidenciaApremio => {
            vec![Escrito::RecursoReposicion, Escrito::Aplazamiento]
        }
        Tipo::DiligenciaEmbargo => vec![Escrito::RecursoReposicion],
        Tipo::Otro => vec![Escrito::Contestacion, Escrito::Alegaciones],
    }
}

fn hueco(s: &str, hueco: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        format!("[{hueco}]")
    } else {
        s.to_owned()
    }
}

fn del_acto(e: &Expediente) -> String {
    let acto = match e.tipo {
        Tipo::Requerimiento => "el requerimiento",
        Tipo::PropuestaLiquidacion => "la propuesta de liquidación",
        Tipo::Audiencia => "el trámite de audiencia",
        Tipo::PropuestaSancion => "la propuesta de imposición de sanción",
        Tipo::Liquidacion => "la liquidación",
        Tipo::AcuerdoSancionador => "el acuerdo de imposición de sanción",
        Tipo::ProvidenciaApremio => "la providencia de apremio",
        Tipo::DiligenciaEmbargo => "la diligencia de embargo",
        Tipo::Otro => "el acto",
    };
    let mut s = acto.to_owned();
    if !e.referencia.is_empty() {
        s.push_str(&format!(" con referencia {}", e.referencia));
    }
    if !e.asunto.is_empty() && e.asunto != e.tipo.nombre() {
        s.push_str(&format!(" («{}»)", e.asunto));
    }
    if !e.concepto.is_empty() {
        s.push_str(&format!(", relativo a {}", e.concepto));
    }
    s
}

fn plazo_de(c: &Calculo, claves: &[&str]) -> String {
    c.plazos
        .iter()
        .find(|p| claves.contains(&p.clave))
        .map_or_else(|| "[último día del plazo]".to_owned(), |p| fecha(p.vence))
}

/// ¿Cita retenciones o IVA, que no se pueden aplazar (art. 65.2 LGT)?
fn no_aplazable(e: &Expediente) -> bool {
    let t = crate::documentos::fold(&format!("{} {}", e.concepto, e.asunto));
    [
        "111",
        "115",
        "123",
        "303",
        "iva",
        "retencion",
        "ingreso a cuenta",
    ]
    .iter()
    .any(|x| t.contains(x))
}

/// Párrafos del escrito `escrito` del expediente `id`.
pub fn paragraphs(
    v: &Vault,
    id: i64,
    escrito: Escrito,
    op: &Opciones,
    hoy: Date,
) -> Result<Vec<Para>> {
    let e = cargar(v.connection(), id)?;
    if !disponibles(e.tipo).contains(&escrito) {
        return Err(AppError::Other(format!(
            "un escrito de «{}» no tiene sentido para {}",
            escrito.nombre(),
            e.tipo.nombre().to_lowercase()
        )));
    }
    let c = super::calculo(&e, hoy, perfil(v)?.as_ref());
    let info = crate::companies::info(v)?;
    let nif = info.nif.clone().unwrap_or_else(|| "[NIF]".into());
    let domicilio = crate::invoicing::settings(v)?
        .and_then(|s| s.issuer.address.map(|a| a.lines().join(", ")))
        .unwrap_or_else(|| "[domicilio]".into());
    let organo = if e.organo.is_empty() {
        format!("{} [órgano que dicta el acto]", e.organismo.nombre())
    } else {
        format!("{} ({})", e.organo, e.organismo.nombre())
    };
    let notificado = c
        .notificado
        .map_or_else(|| "[fecha de notificación]".to_owned(), fecha);
    let acto = del_acto(&e);
    let lgt = matches!(
        e.organismo,
        Organismo::Aeat | Organismo::Autonomica | Organismo::Ayuntamiento
    );
    let titulo_escrito = match escrito {
        Escrito::Contestacion => "CONTESTACIÓN A REQUERIMIENTO".to_owned(),
        Escrito::Alegaciones => "ESCRITO DE ALEGACIONES".to_owned(),
        Escrito::RecursoReposicion if lgt => "RECURSO DE REPOSICIÓN".to_owned(),
        Escrito::RecursoReposicion => "RECURSO".to_owned(),
        Escrito::Aplazamiento if op.plazos.unwrap_or(1) > 1 => {
            "SOLICITUD DE FRACCIONAMIENTO DE PAGO".to_owned()
        }
        Escrito::Aplazamiento => "SOLICITUD DE APLAZAMIENTO DE PAGO".to_owned(),
        Escrito::Ampliacion => "SOLICITUD DE AMPLIACIÓN DE PLAZO".to_owned(),
    };
    let mut out = vec![
        Para::Text(
            "[BORRADOR preparado con TaxPain con los datos del expediente: completa lo que va \
             entre corchetes y borra esta nota antes de presentarlo. Es un modelo general y no \
             sustituye el consejo de un asesor; en una inspección o una sanción, consúltalo.]"
                .into(),
        ),
        Para::Text(format!("A la {organo}")),
        Para::Gap,
        Para::Title(titulo_escrito),
    ];
    let mut datos = Vec::new();
    if !e.referencia.is_empty() {
        datos.push(Para::Row(
            "Expediente o referencia".into(),
            e.referencia.clone(),
        ));
    }
    if !e.concepto.is_empty() {
        datos.push(Para::Row("Concepto".into(), e.concepto.clone()));
    }
    datos.push(Para::Row("Notificado el".into(), notificado.clone()));
    if let Some(i) = e.importe {
        datos.push(Para::Row("Importe".into(), i.to_string()));
    }
    out.extend(datos);
    out.push(Para::Gap);
    out.push(Para::Text(format!(
        "{}, con NIF {nif} y domicilio a efectos de notificaciones en {domicilio}, y en su \
         nombre y representación D./D.ª [nombre y apellidos], con NIF [NIF], en calidad de \
         [administrador único / apoderado], según acredita [escritura o apoderamiento], ante \
         ese órgano comparece y, como mejor proceda,",
        info.name
    )));
    out.push(Para::Bold("EXPONE".into()));
    let documentos = |out: &mut Vec<Para>, titulo: &str| {
        out.push(Para::Bold(titulo.into()));
        if e.documentos.is_empty() {
            out.push(Para::Text(
                "Documento n.º 1. [descripción del documento]".into(),
            ));
        }
        for (i, d) in e.documentos.iter().enumerate() {
            out.push(Para::Text(format!(
                "Documento n.º {}. {}{}",
                i + 1,
                d.descripcion,
                if d.listo {
                    ""
                } else {
                    " [pendiente de reunir]"
                }
            )));
        }
    };
    match escrito {
        Escrito::Contestacion => {
            out.push(Para::Text(format!(
                "PRIMERO. Que el {notificado} se notificó a esta sociedad {acto}."
            )));
            out.push(Para::Text(format!(
                "SEGUNDO. Que, dentro del plazo concedido, que termina el {}, atiende el \
                 requerimiento aportando los documentos que se relacionan a continuación.",
                plazo_de(&c, &["contestar"])
            )));
            out.push(Para::Text(
                "TERCERO. [Aclaraciones sobre lo requerido, punto por punto. Si algo no se \
                 aporta, explica por qué (no existe, ya obra en poder de la Administración — \
                 art. 34.1.h LGT — o se aportará más adelante) y cuándo.]"
                    .into(),
            ));
            documentos(&mut out, "RELACIÓN DE DOCUMENTOS QUE SE APORTAN");
            out.push(Para::Bold("SOLICITA".into()));
            out.push(Para::Text(
                "Que tenga por presentado este escrito con los documentos que lo acompañan, por \
                 atendido en plazo el requerimiento y continúe el procedimiento."
                    .into(),
            ));
        }
        Escrito::Alegaciones => {
            out.push(Para::Text(format!(
                "PRIMERO. Que el {notificado} se notificó a esta sociedad {acto}."
            )));
            out.push(Para::Text(format!(
                "SEGUNDO. Que, dentro del plazo de alegaciones, que termina el {}, formula las \
                 siguientes",
                plazo_de(&c, &["alegar", "contestar"])
            )));
            out.push(Para::Bold("ALEGACIONES".into()));
            out.push(Para::Text(
                "PRIMERA. [Hechos: lo que ocurrió de verdad y en qué no coincide con lo que dice \
                 la Administración.]"
                    .into(),
            ));
            out.push(Para::Text(if e.tipo == Tipo::PropuestaSancion {
                "SEGUNDA. [Fundamentos: por qué no procede la sanción o su graduación; p. ej., \
                 falta de culpabilidad o interpretación razonable de la norma (arts. 179.2.d y \
                 183.1 LGT), o error en los criterios de graduación (art. 187 LGT).]"
                    .to_owned()
            } else {
                "SEGUNDA. [Fundamentos de derecho: las normas y los criterios (consultas de la \
                 DGT, resoluciones del TEAC, sentencias) que apoyan tu postura.]"
                    .to_owned()
            }));
            out.push(Para::Text(
                "TERCERA. [Qué debe corregirse y cómo queda el cálculo.]".into(),
            ));
            if !e.documentos.is_empty() {
                documentos(&mut out, "DOCUMENTOS QUE SE ACOMPAÑAN");
            }
            out.push(Para::Bold("SOLICITA".into()));
            out.push(Para::Text(format!(
                "Que tenga por presentadas en plazo estas alegaciones y, en su virtud, {}.",
                if e.tipo == Tipo::PropuestaSancion {
                    "archive el procedimiento sancionador o, subsidiariamente, [reduzca la \
                     sanción propuesta]"
                } else {
                    "[dicte la resolución conforme a lo alegado / archive el procedimiento]"
                }
            )));
        }
        Escrito::RecursoReposicion => {
            let (norma, motivos) = match e.tipo {
                Tipo::ProvidenciaApremio => (
                    "arts. 222 a 225 y 167.3 LGT",
                    "[Motivo del art. 167.3 LGT: extinción o prescripción de la deuda, \
                     aplazamiento pedido en periodo voluntario, falta de notificación de la \
                     liquidación, anulación de la liquidación o error en la identificación.]",
                ),
                Tipo::DiligenciaEmbargo => (
                    "arts. 222 a 225 y 170.3 LGT",
                    "[Motivo del art. 170.3 LGT: extinción de la deuda, falta de notificación \
                     de la providencia de apremio, incumplimiento de las normas del embargo \
                     (bienes inembargables, orden de embargo) o suspensión del procedimiento.]",
                ),
                _ => (
                    "arts. 222 a 225 LGT y RD 520/2005",
                    "[Motivos por los que el acto no es conforme a derecho.]",
                ),
            };
            let norma = if lgt {
                if e.organismo == Organismo::Ayuntamiento {
                    "art. 14.2 TRLRHL (RDLeg. 2/2004)"
                } else {
                    norma
                }
            } else {
                "[norma del recurso que indica el documento]"
            };
            out.push(Para::Text(format!(
                "PRIMERO. Que el {notificado} se notificó a esta sociedad {acto}."
            )));
            out.push(Para::Text(format!(
                "SEGUNDO. Que, no estando conforme, dentro del plazo de un mes, que termina el \
                 {}, interpone {} ({norma}) con base en los siguientes",
                plazo_de(&c, &["reposicion"]),
                if lgt {
                    "RECURSO DE REPOSICIÓN"
                } else {
                    "[RECURSO DE ALZADA / REPOSICIÓN, el que indique el documento]"
                }
            )));
            out.push(Para::Bold("HECHOS".into()));
            out.push(Para::Text("[Los hechos, numerados.]".into()));
            out.push(Para::Bold("FUNDAMENTOS DE DERECHO".into()));
            out.push(Para::Text(motivos.into()));
            if !e.documentos.is_empty() {
                documentos(&mut out, "DOCUMENTOS QUE SE ACOMPAÑAN");
            }
            out.push(Para::Bold("SOLICITA".into()));
            out.push(Para::Text(format!(
                "Que tenga por interpuesto en plazo este recurso y, en su virtud, anule {acto} \
                 [o lo modifique en el sentido expuesto]."
            )));
            out.push(Para::Text(if e.tipo == Tipo::AcuerdoSancionador {
                "OTROSÍ DIGO: que, conforme al art. 212.3 LGT, la ejecución de la sanción \
                 recurrida queda suspendida automáticamente, sin necesidad de aportar garantía, \
                 hasta que sea firme en vía administrativa."
                    .to_owned()
            } else {
                "[Si no quieres pagar mientras se resuelve: OTROSÍ DIGO que solicita la \
                 suspensión de la ejecución del acto impugnado y aporta [garantía: depósito, aval \
                 o fianza] (art. 224 LGT y art. 25 RD 520/2005). Si no, borra este párrafo.]"
                    .to_owned()
            }));
        }
        Escrito::Aplazamiento => {
            let n = op.plazos.unwrap_or(1).max(1);
            let importe = e
                .importe
                .or(e.sancion)
                .map_or_else(|| "[importe]".to_owned(), |m| m.to_string());
            out.push(Para::Text(format!(
                "PRIMERO. Que el {notificado} se notificó a esta sociedad {acto}, por importe de \
                 {importe}, cuyo plazo de pago termina el {}.",
                plazo_de(&c, &["pago"])
            )));
            out.push(Para::Text(format!(
                "SEGUNDO. Que la sociedad atraviesa dificultades económico-financieras de \
                 carácter transitorio que le impiden temporalmente pagar en plazo (art. 65.1 \
                 LGT y arts. 44 a 46 del Reglamento General de Recaudación, RD 939/2005): {}.",
                hueco(
                    &op.motivo,
                    "explica el motivo: caída de ventas, cobros retrasados, un pago \
                     extraordinario…, y por qué es pasajero"
                )
            )));
            out.push(Para::Text(
                "TERCERO. Que su previsión de tesorería le permitirá pagar la deuda en los \
                 plazos que se proponen. [Si se piden más de 50.000 € en total: acompaña el plan \
                 de tesorería y los documentos que pide el art. 46 RGR.]"
                    .into(),
            ));
            out.push(Para::Bold("SOLICITA".into()));
            out.push(Para::Text(if n > 1 {
                format!(
                    "El fraccionamiento de la deuda en {n} plazos mensuales, el primero el [día] \
                     de [mes] y los siguientes el mismo día de cada mes, con cargo en la cuenta \
                     [IBAN] de la que es titular la sociedad."
                )
            } else {
                "El aplazamiento de la deuda hasta el [fecha], con cargo en la cuenta [IBAN] de \
                 la que es titular la sociedad."
                    .to_owned()
            }));
            out.push(Para::Text(
                "[Garantía: por deudas de hasta 50.000 € en total no se exige (Orden \
                 HAC/2178/2015 y sus modificaciones; TODO(VERIFY) el límite vigente). Si se \
                 supera, indica la garantía que se ofrece (aval bancario, hipoteca…) o pide la \
                 dispensa justificándola (art. 82.2 LGT).]"
                    .into(),
            ));
            if no_aplazable(&e) {
                out.push(Para::Text(
                    "[Ojo: no se pueden aplazar ni fraccionar las retenciones e ingresos a cuenta \
                     ni, salvo que se justifique que no se han cobrado, el IVA repercutido (art. \
                     65.2.b y f LGT). TODO(VERIFY) si la deuda es de ese tipo.]"
                        .into(),
                ));
            }
            out.push(Para::Text(
                "[En la AEAT se pide normalmente con el formulario de la Sede (Aplazamientos y \
                 fraccionamientos), que pide estos mismos datos: usa este borrador para \
                 prepararlos. Pedirlo en periodo voluntario evita que empiece el periodo \
                 ejecutivo mientras se resuelve (art. 65.5 LGT).]"
                    .into(),
            ));
        }
        Escrito::Ampliacion => {
            let n = e.plazo_dias.unwrap_or(match e.tipo {
                Tipo::Requerimiento => super::plazos::DIAS_REQUERIMIENTO,
                _ => super::plazos::DIAS_ALEGACIONES,
            });
            out.push(Para::Text(format!(
                "PRIMERO. Que el {notificado} se notificó a esta sociedad {acto}, con un plazo \
                 que termina el {}.",
                plazo_de(&c, &["contestar", "alegar"])
            )));
            out.push(Para::Text(format!(
                "SEGUNDO. Que no es posible atenderlo en ese plazo porque {}.",
                hueco(
                    &op.motivo,
                    "motivo: el volumen de la documentación, que la tiene un tercero, una baja…"
                )
            )));
            out.push(Para::Bold("SOLICITA".into()));
            out.push(Para::Text(format!(
                "La ampliación del plazo en {} días hábiles más (la mitad del concedido), \
                 conforme al art. 91 del RGAT (RD 1065/2007), sin perjuicio de derechos de \
                 terceros.",
                n / 2
            )));
            out.push(Para::Text(format!(
                "[Preséntala como tarde el {}: tres días antes de que acabe el plazo (art. 91.2 \
                 RGAT; TODO(VERIFY)). Si no te contestan, se entiende concedida.]",
                plazo_de(&c, &["pedir_ampliacion"])
            )));
        }
    }
    out.push(Para::Gap);
    out.push(Para::Text(format!(
        "En {}, a [fecha].",
        hueco(&op.lugar, "lugar")
    )));
    out.push(Para::Text("Fdo.: [nombre, apellidos y firma]".into()));
    Ok(out)
}

/// Nombre de fichero propuesto: «Recurso de reposición 2026GRC00001.rtf».
pub fn nombre_fichero(v: &Vault, id: i64, escrito: Escrito, ext: &str) -> Result<String> {
    let e = cargar(v.connection(), id)?;
    let r = if e.referencia.is_empty() {
        e.asunto.clone()
    } else {
        e.referencia.clone()
    };
    Ok(format!(
        "{} {}.{ext}",
        escrito.nombre(),
        crate::notices::safe_file_component(&r)
    ))
}

/// El escrito en RTF (`pdf = false`) o PDF. Queda en la auditoría (qué escrito y de qué
/// expediente; no el texto).
pub fn generar(
    v: &Vault,
    id: i64,
    escrito: Escrito,
    op: &Opciones,
    hoy: Date,
    as_pdf: bool,
    now: OffsetDateTime,
) -> Result<Vec<u8>> {
    let paras = paragraphs(v, id, escrito, op, hoy)?;
    let bytes = if as_pdf {
        let e = cargar(v.connection(), id)?;
        pdf::text_document(&format!("{} · {}", escrito.nombre(), titulo(&e)), &[paras])
            .map_err(AppError::Other)?
    } else {
        crate::memoria::to_rtf(&paras).into_bytes()
    };
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "expediente.escrito",
        &id.to_string(),
        &serde_json::json!({ "escrito": escrito.code(), "formato": if as_pdf { "pdf" } else { "rtf" } }),
    )?;
    Ok(bytes)
}

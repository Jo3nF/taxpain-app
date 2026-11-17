//! Lista de control de las obligaciones no fiscales de una sociedad de servicios digitales
//! (G44): depósito de cuentas, legalización de libros, titular real, protección de datos, LSSI,
//! prevención de riesgos, igualdad y registro horario, seguros, licencias y cambios en el
//! Registro. No asesora: dice qué es, a quién suele aplicar, la norma y, cuando es seguro, el
//! enlace oficial. El usuario marca si le aplica, si ya está hecho y, si quiere, una fecha que
//! recordar (tabla `obligacion_no_fiscal`). Sin fila: sin revisar.
//!
//! Las que dependen de tener trabajadores llevan `con_empleados`: sin nadie en Nóminas, la
//! interfaz lo dice («no tienes empleados en Nóminas»), pero no la marca sola.

use serde::{Deserialize, Serialize};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::params;
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;
use crate::puesta_marcha::{Enlace, fecha_iso};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoObligacion {
    SinRevisar,
    Aplica,
    NoAplica,
    Hecho,
}

impl EstadoObligacion {
    fn code(self) -> &'static str {
        match self {
            EstadoObligacion::SinRevisar => "sin_revisar",
            EstadoObligacion::Aplica => "aplica",
            EstadoObligacion::NoAplica => "no_aplica",
            EstadoObligacion::Hecho => "hecho",
        }
    }
    fn parse(s: &str) -> EstadoObligacion {
        match s {
            "aplica" => EstadoObligacion::Aplica,
            "no_aplica" => EstadoObligacion::NoAplica,
            "hecho" => EstadoObligacion::Hecho,
            _ => EstadoObligacion::SinRevisar,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Obligacion {
    pub id: String,
    pub titulo: String,
    pub que_es: String,
    pub a_quien: String,
    pub plazo: String,
    pub norma: String,
    pub enlaces: Vec<Enlace>,
    /// Sólo si hay trabajadores por cuenta ajena.
    pub con_empleados: bool,
    pub estado: EstadoObligacion,
    pub fecha: Option<Date>,
    pub nota: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Obligaciones {
    pub lista: Vec<Obligacion>,
    /// Hay alguien de alta en Nóminas que no es administrador.
    pub hay_empleados: bool,
}

const BOE_LSC: &str = "https://www.boe.es/buscar/act.php?id=BOE-A-2010-10544";
const BOE_CCOM: &str = "https://www.boe.es/buscar/act.php?id=BOE-A-1885-6627";
const BOE_LEY_14_2013: &str = "https://www.boe.es/buscar/act.php?id=BOE-A-2013-10074";
const BOE_LEY_10_2010: &str = "https://www.boe.es/buscar/act.php?id=BOE-A-2010-6737";
const BOE_LOPDGDD: &str = "https://www.boe.es/buscar/act.php?id=BOE-A-2018-16673";
const RGPD: &str = "https://eur-lex.europa.eu/eli/reg/2016/679/oj";
const AEPD: &str = "https://www.aepd.es/";
const BOE_LSSI: &str = "https://www.boe.es/buscar/act.php?id=BOE-A-2002-13758";
const BOE_PRL: &str = "https://www.boe.es/buscar/act.php?id=BOE-A-1995-24292";
const BOE_ET: &str = "https://www.boe.es/buscar/act.php?id=BOE-A-2015-11430";
const BOE_IGUALDAD: &str = "https://www.boe.es/buscar/act.php?id=BOE-A-2007-6115";

/// (id, título, qué es, a quién, plazo, norma, enlaces, con empleados)
type Def = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
    bool,
);

const CATALOGO: [Def; 12] = [
    (
        "deposito_cuentas",
        "Depósito de las cuentas anuales",
        "Las cuentas que aprueba la junta se depositan en el Registro Mercantil del domicilio. \
         TaxPain prepara el paquete en Contabilidad → Cuentas anuales.",
        "Todas las sociedades de capital (SL y SA).",
        "Un mes desde la aprobación; la junta, en los seis meses siguientes al cierre (con \
         cierre a 31/12, hasta el 30 de julio).",
        "Arts. 164 y 279 LSC (RDLeg 1/2010)",
        &[("Ley de Sociedades de Capital (BOE)", BOE_LSC)],
        false,
    ),
    (
        "legalizacion_libros",
        "Legalización de los libros",
        "El diario, el de inventarios y cuentas anuales, el de actas y el de socios se legalizan \
         en el Registro Mercantil por vía telemática (TaxPain genera los ficheros de Legalia).",
        "Todos los empresarios que llevan contabilidad mercantil (las sociedades, siempre).",
        "Cuatro meses desde el cierre del ejercicio (con cierre a 31/12, hasta el 30 de abril).",
        "Art. 27 CCom; art. 18 Ley 14/2013",
        &[
            ("Código de Comercio (BOE)", BOE_CCOM),
            ("Ley 14/2013 (BOE)", BOE_LEY_14_2013),
        ],
        false,
    ),
    (
        "titular_real",
        "Declaración de titularidad real",
        "Quién tiene, directa o indirectamente, más del 25 % de la sociedad o la controla. Se \
         declara con el depósito de cuentas (en su formulario) y hay que mantenerla al día.",
        "Las sociedades mercantiles.",
        "Con cada depósito de cuentas y cuando cambia.",
        "Art. 4 Ley 10/2010; Orden JUS/319/2018. TODO(VERIFY): Registro de Titularidades \
         Reales (RD 609/2023)",
        &[("Ley 10/2010 (BOE)", BOE_LEY_10_2010)],
        false,
    ),
    (
        "registro_actividades",
        "Protección de datos: registro de actividades de tratamiento",
        "Qué datos personales tratas (clientes, proveedores, empleados, usuarios de la web), \
         para qué, con qué base, cuánto los guardas y con qué medidas. Revísalo cada año y \
         cuando cambie algo.",
        "Casi cualquier empresa que trate datos personales de forma no ocasional (art. 30.5 \
         RGPD limita la excepción).",
        "Permanente; revisión al menos anual (recomendación).",
        "Art. 30 RGPD (UE 2016/679); art. 31 LOPDGDD (LO 3/2018)",
        &[
            ("Reglamento General de Protección de Datos (EUR-Lex)", RGPD),
            ("Ley Orgánica 3/2018 (BOE)", BOE_LOPDGDD),
            ("Agencia Española de Protección de Datos", AEPD),
        ],
        false,
    ),
    (
        "rgpd_informacion",
        "Protección de datos: información, contratos de encargado y brechas",
        "Informar a los interesados (política de privacidad), firmar el contrato de encargado \
         con quien trate datos por ti (alojamiento, correo, nóminas) y saber cómo notificar una \
         brecha a la AEPD en 72 horas.",
        "Quien trate datos personales.",
        "Permanente.",
        "Arts. 13, 28 y 33 RGPD",
        &[
            ("Reglamento General de Protección de Datos (EUR-Lex)", RGPD),
            ("Agencia Española de Protección de Datos", AEPD),
        ],
        false,
    ),
    (
        "lssi_web",
        "Web: aviso legal y cookies",
        "La web de la empresa ha de mostrar la razón social, el NIF, el domicilio, los datos \
         del Registro Mercantil y un correo de contacto; si usa cookies que no son técnicas, \
         pedir el consentimiento antes de instalarlas.",
        "Quien tenga una web o una app con la que preste servicios o se dé a conocer.",
        "Permanente.",
        "Arts. 10 y 22.2 Ley 34/2002 (LSSI)",
        &[
            ("Ley 34/2002 (BOE)", BOE_LSSI),
            ("Agencia Española de Protección de Datos", AEPD),
        ],
        false,
    ),
    (
        "prevencion_riesgos",
        "Prevención de riesgos laborales",
        "Evaluación de riesgos, plan de prevención, formación e información a los trabajadores y \
         vigilancia de la salud, con un servicio de prevención (propio o ajeno) o asumiéndolo el \
         empresario cuando la ley lo permite.",
        "Quien tenga trabajadores por cuenta ajena.",
        "Desde la primera contratación.",
        "Ley 31/1995 de Prevención de Riesgos Laborales",
        &[("Ley 31/1995 (BOE)", BOE_PRL)],
        true,
    ),
    (
        "registro_horario",
        "Registro de la jornada",
        "Registro diario de la hora de entrada y salida de cada trabajador, que se conserva \
         cuatro años a disposición de los trabajadores, sus representantes y la Inspección.",
        "Quien tenga trabajadores por cuenta ajena.",
        "Cada día.",
        "Art. 34.9 Estatuto de los Trabajadores (RDLeg 2/2015)",
        &[("Estatuto de los Trabajadores (BOE)", BOE_ET)],
        true,
    ),
    (
        "igualdad",
        "Igualdad: registro retributivo, protocolo de acoso y plan de igualdad",
        "El registro retributivo y las medidas contra el acoso sexual y por razón de sexo son \
         para toda empresa con trabajadores; el plan de igualdad, a partir de 50.",
        "Quien tenga trabajadores por cuenta ajena (el plan, desde 50).",
        "Permanente.",
        "Art. 28.2 ET; arts. 45 y 48 LO 3/2007",
        &[
            ("Ley Orgánica 3/2007 (BOE)", BOE_IGUALDAD),
            ("Estatuto de los Trabajadores (BOE)", BOE_ET),
        ],
        true,
    ),
    (
        "seguros",
        "Seguros",
        "Qué pólizas tiene la empresa (responsabilidad civil, local, ciberriesgos, accidentes \
         del convenio) y cuándo vencen. Algunas actividades y algunos convenios colectivos las \
         exigen; las demás son decisión tuya.",
        "Según la actividad y el convenio.",
        "Apunta la fecha de renovación.",
        "Según la actividad (no hay una norma general)",
        &[],
        false,
    ),
    (
        "licencia_municipal",
        "Licencia o comunicación de actividad",
        "Abrir un local u oficina suele pedir una licencia o una declaración responsable o \
         comunicación previa en el Ayuntamiento; trabajar desde el domicilio, según la \
         ordenanza.",
        "Quien tenga un local abierto; depende de cada Ayuntamiento.",
        "Antes de empezar la actividad en el local.",
        "Ordenanzas municipales; art. 84 Ley 7/1985 de Bases del Régimen Local. TODO(VERIFY)",
        &[],
        false,
    ),
    (
        "cambios_registro",
        "Comunicar los cambios al Registro Mercantil",
        "Los cambios de administradores, domicilio, objeto social, capital o estatutos se \
         elevan a escritura (o certificación) y se inscriben en el Registro Mercantil; los \
         fiscales, además, con el 036.",
        "Todas las sociedades.",
        "El nombramiento de administradores, en los diez días siguientes a la aceptación.",
        "Art. 215 LSC; Reglamento del Registro Mercantil (RD 1784/1996)",
        &[("Ley de Sociedades de Capital (BOE)", BOE_LSC)],
        false,
    ),
];

/// La lista de control con lo que ha marcado el usuario.
pub fn lista(v: &Vault) -> Result<Obligaciones> {
    let conn = v.connection();
    let mut stmt = conn.prepare("SELECT id, estado, fecha, nota FROM obligacion_no_fiscal")?;
    let guardadas: Vec<(String, String, Option<String>, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let hay_empleados = crate::payroll::employees(v)?
        .iter()
        .any(|e| e.active && !e.administrator);
    let lista = CATALOGO
        .iter()
        .map(
            |(id, titulo, que_es, a_quien, plazo, norma, enlaces, con_empleados)| {
                let g = guardadas.iter().find(|g| g.0 == *id);
                Obligacion {
                    id: (*id).into(),
                    titulo: (*titulo).into(),
                    que_es: (*que_es).into(),
                    a_quien: (*a_quien).into(),
                    plazo: (*plazo).into(),
                    norma: (*norma).into(),
                    enlaces: enlaces
                        .iter()
                        .map(|(label, url)| Enlace {
                            label: (*label).into(),
                            url: (*url).into(),
                        })
                        .collect(),
                    con_empleados: *con_empleados,
                    estado: g.map_or(EstadoObligacion::SinRevisar, |g| {
                        EstadoObligacion::parse(&g.1)
                    }),
                    fecha: g.and_then(|g| g.2.as_deref()).and_then(fecha_iso),
                    nota: g.map(|g| g.3.clone()).unwrap_or_default(),
                }
            },
        )
        .collect();
    Ok(Obligaciones {
        lista,
        hay_empleados,
    })
}

/// Guarda si aplica, si está hecha, la fecha que recordar y una nota. Queda en la auditoría.
pub fn marcar(
    v: &Vault,
    id: &str,
    estado: EstadoObligacion,
    fecha: Option<Date>,
    nota: &str,
    now: OffsetDateTime,
) -> Result<()> {
    if !CATALOGO.iter().any(|d| d.0 == id) {
        return Err(AppError::Other(format!("obligación desconocida: {id}")));
    }
    let conn = v.connection();
    conn.execute(
        "INSERT INTO obligacion_no_fiscal (id, estado, fecha, nota, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET estado = ?2, fecha = ?3, nota = ?4, updated_at = ?5",
        params![
            id,
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
        "obligations.mark",
        id,
        &serde_json::json!({ "estado": estado.code(), "fecha": fecha.map(|d| d.to_string()) }),
    )?;
    Ok(())
}

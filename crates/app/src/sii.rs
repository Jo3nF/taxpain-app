//! Suministro Inmediato de Información del IVA (SII): cuándo es obligatorio y en qué plazo hay
//! que enviar cada factura a la Sede de la AEAT.
//!
//! **Obligados** (art. 62.6 RIVA): quienes tienen periodo de liquidación mensual (art. 71.3 RIVA:
//! volumen de operaciones del año anterior de más de 6.010.121,04 €, inscritos en el registro de
//! devolución mensual —REDEME, art. 30 RIVA— y grupos de entidades) y quienes opten por él
//! voluntariamente. Con SII no se presentan el 390 ni el 347 (Orden EHA/3111/2009 art. 1.3;
//! art. 32.1.e RGAT) y el 303 es mensual, hasta el día 30 del mes siguiente (art. 71.4 RIVA).
//!
//! **Plazos** (art. 69 bis.1 RIVA): las facturas expedidas, en cuatro días naturales desde su
//! expedición (ocho si las expide el destinatario o un tercero) y, en todo caso, antes del día
//! 16 del mes siguiente al del devengo; las recibidas, en cuatro días desde su registro contable
//! y antes del día 16 del mes siguiente al del periodo de liquidación en que se incluyan. En el
//! cómputo de los cuatro días no cuentan los sábados, los domingos ni los festivos nacionales.
//! `TODO(VERIFY)`: las facturas que expide el destinatario o un tercero (ocho días) y las
//! importaciones (desde la contabilización del DUA) no se distinguen: se cuentan cuatro.
//!
//! **Libros en el formato del SII**: TaxPain no tiene los esquemas del SII (los XSD y los WSDL
//! de `SuministroLR`, `SuministroInformacion` del SII; los de `fixtures/aeat/verifactu` son de
//! VERI*FACTU, con otro espacio de nombres), así que no genera los mensajes del SII
//! (`TODO(VERIFY)`: añadir los esquemas oficiales a `fixtures/aeat/sii` antes de escribirlos).
//! Nada se envía: TaxPain avisa de los plazos y los libros registro salen en Libros.

use serde::Serialize;
use taxpain_core::calendar::holidays;
use taxpain_core::calendar::{Periodicity, TaxpayerProfile};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use time::{Date, Duration, Month, Weekday};

use crate::error::{AppError, Result};

/// Si el SII es obligatorio, voluntario o no se lleva, y por qué.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Estado {
    /// Obligatorio por el perfil o los datos (gran empresa, REDEME, IVA mensual).
    pub obligatorio: bool,
    /// Acogida voluntariamente (ajustes de los modelos o perfil «con SII»).
    pub voluntario: bool,
    /// Por qué es obligatorio (o por qué lo será).
    pub motivos: Vec<String>,
    /// Lo que no cuadra entre el perfil y los datos (p. ej., REDEME sin IVA mensual).
    pub avisos: Vec<String>,
    /// TaxPain tiene los esquemas del SII y genera sus mensajes. Hoy, no.
    pub formato_sii: bool,
    pub norma: &'static str,
}

const NORMA: &str = "Arts. 62.6, 68 bis, 69 bis y 71.3 RIVA (RD 1624/1992); art. 30 RIVA (REDEME)";

/// Estado del SII de la empresa abierta el día `hoy`.
pub fn estado(v: &Vault, hoy: Date) -> Result<Estado> {
    let p: Option<TaxpayerProfile> = v.setting("taxpayer_profile")?;
    let s = crate::models::settings(v)?;
    let mut motivos = Vec::new();
    let mut avisos = Vec::new();
    let mensual = p
        .as_ref()
        .and_then(|p| p.iva)
        .is_some_and(|i| i.periodicity == Periodicity::Mensual);
    let perfil_sii = p.as_ref().and_then(|p| p.iva).is_some_and(|i| i.sii);
    if mensual {
        motivos.push(
            "El perfil fiscal tiene el IVA mensual: con periodo mensual los libros registro van \
             por el SII (art. 62.6 RIVA)."
                .to_owned(),
        );
    }
    if p.as_ref().is_some_and(|p| p.gran_empresa) {
        motivos.push(
            "Gran empresa (más de 6.010.121,04 € de volumen de operaciones el año anterior): IVA \
             mensual y SII (arts. 71.3 y 62.6 RIVA)."
                .to_owned(),
        );
    }
    if s.redeme {
        motivos.push(
            "Inscrita en el registro de devolución mensual (REDEME): IVA mensual y SII \
             (arts. 30, 71.3 y 62.6 RIVA)."
                .to_owned(),
        );
        if !mensual {
            avisos.push(
                "Está marcado el REDEME, pero el perfil fiscal tiene el IVA trimestral: cámbialo \
                 a «Mensual (con SII)» para que el calendario traiga los 303 de cada mes."
                    .to_owned(),
            );
        }
    }
    let anterior = crate::umbrales::volumen(v, hoy.year() - 1).unwrap_or(Money::ZERO);
    if anterior > crate::umbrales::UMBRAL_GRAN_EMPRESA {
        motivos.push(format!(
            "Las facturas emitidas en {} suman {anterior} (sin IVA): más de 6.010.121,04 €, el \
             umbral de la gran empresa (art. 121 LIVA).",
            hoy.year() - 1
        ));
        if !mensual {
            avisos.push(format!(
                "Con ese volumen, desde enero de {} el 303 es mensual y los libros van por el \
                 SII: cambia el perfil fiscal.",
                hoy.year()
            ));
        }
    }
    let actual = crate::umbrales::volumen(v, hoy.year()).unwrap_or(Money::ZERO);
    if anterior <= crate::umbrales::UMBRAL_GRAN_EMPRESA
        && actual > crate::umbrales::UMBRAL_GRAN_EMPRESA
    {
        avisos.push(format!(
            "Las facturas emitidas en {} ya suman {actual}: desde enero de {} tendrás que \
             llevar los libros por el SII. Prepara el alta en el censo (036).",
            hoy.year(),
            hoy.year() + 1
        ));
    }
    let obligatorio = !motivos.is_empty();
    Ok(Estado {
        obligatorio,
        voluntario: !obligatorio && (s.sii_voluntario || perfil_sii),
        motivos,
        avisos,
        formato_sii: false,
        norma: NORMA,
    })
}

/// Plazo de envío al SII de una factura.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Plazo {
    /// `emitida` (libro de facturas expedidas) o `recibida`.
    pub libro: &'static str,
    pub numero: String,
    pub nif: String,
    pub nombre: String,
    /// Expedición (emitidas) o registro contable (recibidas).
    pub fecha: Date,
    /// Último día para enviarla.
    pub limite: Date,
    /// Días naturales desde hoy hasta el límite (negativo: vencido).
    pub dias: i64,
    /// Sin los festivos nacionales de ese año en TaxPain: el límite sólo salta los fines de
    /// semana y puede ser algún día después (nunca antes).
    pub aproximado: bool,
}

/// Festivos nacionales de un año, si TaxPain los tiene.
fn nacionales(year: i32) -> Option<Vec<Date>> {
    holidays::national(year)
}

/// El cuarto día hábil a efectos del SII (sin sábados, domingos ni festivos nacionales) desde
/// `desde`, y si es aproximado (faltan los festivos de algún año del cómputo).
pub fn cuatro_dias(desde: Date) -> (Date, bool) {
    let mut d = desde;
    let mut contados = 0;
    let mut aproximado = false;
    while contados < 4 {
        d += Duration::days(1);
        let festivos = nacionales(d.year());
        aproximado |= festivos.is_none();
        let festivo = festivos.is_some_and(|f| f.contains(&d));
        if !matches!(d.weekday(), Weekday::Saturday | Weekday::Sunday) && !festivo {
            contados += 1;
        }
    }
    (d, aproximado)
}

/// El día 15 del mes siguiente al de `d` (el envío debe hacerse «antes del día 16»).
fn dia_15_siguiente(d: Date) -> Date {
    let (y, m) = if d.month() == Month::December {
        (d.year() + 1, Month::January)
    } else {
        (d.year(), d.month().next())
    };
    Date::from_calendar_date(y, m, 15).unwrap_or(d)
}

/// Último día para enviar una factura expedida el día `expedicion` con el IVA devengado el día
/// `devengo` (art. 69 bis.1.a RIVA).
pub fn limite_emitida(expedicion: Date, devengo: Date) -> (Date, bool) {
    let (d, aprox) = cuatro_dias(expedicion);
    (d.min(dia_15_siguiente(devengo)), aprox)
}

/// Último día para enviar una factura recibida registrada el día `registro` (art. 69 bis.1.b
/// RIVA: antes del día 16 del mes siguiente al del periodo de liquidación en que se incluye, el
/// mes del registro con periodo mensual).
pub fn limite_recibida(registro: Date) -> (Date, bool) {
    let (d, aprox) = cuatro_dias(registro);
    (d.min(dia_15_siguiente(registro)), aprox)
}

/// Plazos del SII de las facturas expedidas o registradas entre `desde` y `hasta`, ordenados por
/// el límite. No envía nada.
pub fn plazos(v: &Vault, desde: Date, hasta: Date, hoy: Date) -> Result<Vec<Plazo>> {
    if hasta < desde {
        return Err(AppError::Other("el periodo acaba antes de empezar".into()));
    }
    let mut out: Vec<Plazo> = crate::models::facturas_sii(v, desde, hasta)?
        .into_iter()
        .map(|f| {
            let (limite, aproximado) = if f.emitida {
                limite_emitida(f.fecha, f.devengo)
            } else {
                limite_recibida(f.fecha)
            };
            Plazo {
                libro: if f.emitida { "emitida" } else { "recibida" },
                numero: f.numero,
                nif: f.nif,
                nombre: f.nombre,
                fecha: f.fecha,
                limite,
                dias: (limite - hoy).whole_days(),
                aproximado,
            }
        })
        .collect();
    out.sort_by(|a, b| a.limite.cmp(&b.limite).then(a.numero.cmp(&b.numero)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn cuatro_dias_sin_fines_de_semana_ni_festivos_nacionales() {
        // Lunes 9 de noviembre de 2026: martes, miércoles, jueves y viernes.
        assert_eq!(
            cuatro_dias(date!(2026 - 11 - 09)),
            (date!(2026 - 11 - 13), false)
        );
        // Jueves 3 de diciembre de 2026: el 8 (Inmaculada) no cuenta.
        assert_eq!(cuatro_dias(date!(2026 - 12 - 03)).0, date!(2026 - 12 - 10));
    }

    #[test]
    fn antes_del_dia_16_del_mes_siguiente() {
        // Expedida el 14 de diciembre con el devengo en noviembre: hasta el 15 de diciembre.
        let (d, _) = limite_emitida(date!(2026 - 12 - 14), date!(2026 - 11 - 30));
        assert_eq!(d, date!(2026 - 12 - 15));
        // Expedida y devengada el 14 de diciembre: cuatro días hábiles (el 18).
        let (d, _) = limite_emitida(date!(2026 - 12 - 14), date!(2026 - 12 - 14));
        assert_eq!(d, date!(2026 - 12 - 18));
        // Recibida registrada el 30 de noviembre de 2026 (lunes): 4 de diciembre.
        assert_eq!(
            limite_recibida(date!(2026 - 11 - 30)).0,
            date!(2026 - 12 - 04)
        );
    }
}

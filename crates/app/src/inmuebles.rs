//! Inmueble arrendado de cada arrendador (G43): situación, referencia catastral y dirección,
//! guardados por el NIF del proveedor (tabla `inmueble_arrendado`), y la acreditación del
//! arrendador para no retener.
//!
//! - El borrador del 180 los pone en el registro de cada arrendador (`Rent180.property`) y,
//!   si todos los arrendadores tienen su inmueble, genera el fichero (antes se mandaba al
//!   formulario de la Sede).
//! - El certificado de retenciones del 180 escribe el inmueble guardado (antes se tecleaba al
//!   generarlo).
//! - El 115 avisa de los alquileres sin retención cuyo arrendador no tiene una acreditación
//!   vigente para no retener (art. 75.3.g RIRPF: arrendador con alta en el epígrafe 861 del IAE
//!   sin cuota cero, que lo acredita), salvo que lo pagado en el año no pase de 900 €
//!   (art. 75.3.g RIRPF).
//!
//! Un arrendador, un inmueble: las facturas recibidas no dicen de qué inmueble son. Con dos
//! inmuebles del mismo arrendador, el 180 lleva un registro por inmueble y habría que repartir
//! las rentas a mano en el formulario (`TODO(VERIFY)` si hace falta).

use serde::{Deserialize, Serialize};
use taxpain_models::informative::Property;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

/// Inmueble arrendado de un arrendador.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inmueble {
    /// NIF del arrendador.
    pub nif: String,
    /// 1: con referencia catastral en territorio común; 2: País Vasco; 3: Navarra; 4: sin
    /// referencia catastral.
    pub situacion: u8,
    #[serde(default)]
    pub ref_catastral: String,
    #[serde(default)]
    pub via: String,
    #[serde(default)]
    pub numero: String,
    #[serde(default)]
    pub municipio: String,
    /// Código de provincia (2 cifras).
    #[serde(default)]
    pub provincia: String,
    #[serde(default)]
    pub codigo_postal: String,
    /// Hasta cuándo vale la acreditación del arrendador para no retener (art. 75.3.g RIRPF).
    #[serde(default)]
    pub exonerado_hasta: Option<Date>,
}

impl Inmueble {
    /// Datos del registro de perceptor del 180.
    pub fn property(&self) -> Property {
        Property {
            situation: self.situacion,
            cadastral_ref: self.ref_catastral.trim().to_uppercase(),
            street: self.via.trim().to_uppercase(),
            number: self.numero.trim().to_uppercase(),
            municipality: self.municipio.trim().to_uppercase(),
            province_code: self.provincia.trim().to_owned(),
            postal_code: self.codigo_postal.trim().to_owned(),
        }
    }

    /// Descripción para el certificado de retenciones.
    pub fn texto(&self) -> String {
        let mut s = format!("{} {}", self.via.trim(), self.numero.trim())
            .trim()
            .to_owned();
        let lugar = format!("{} {}", self.codigo_postal.trim(), self.municipio.trim())
            .trim()
            .to_owned();
        if !lugar.is_empty() {
            if !s.is_empty() {
                s.push_str(", ");
            }
            s.push_str(&lugar);
        }
        if !self.ref_catastral.trim().is_empty() {
            if !s.is_empty() {
                s.push_str(" · ");
            }
            s.push_str("ref. catastral ");
            s.push_str(self.ref_catastral.trim());
        }
        s
    }
}

/// Comprueba los datos como los pide el diseño de registro del 180 (`180-notas.md`).
pub fn validar(i: &Inmueble) -> Result<()> {
    if i.nif.trim().is_empty() {
        return Err(other("falta el NIF del arrendador"));
    }
    if !(1..=4).contains(&i.situacion) {
        return Err(other(
            "situación del inmueble: 1 (territorio común), 2 (País Vasco), 3 (Navarra) o 4 (sin \
             referencia catastral)",
        ));
    }
    let r = i.ref_catastral.trim();
    if i.situacion != 4 && r.chars().filter(|c| c.is_ascii_alphanumeric()).count() != 20 {
        return Err(other(
            "la referencia catastral tiene 20 caracteres (con situación 4 no hace falta)",
        ));
    }
    if i.situacion == 4 && !r.is_empty() {
        return Err(other(
            "con situación 4 (sin referencia catastral) deja la referencia en blanco",
        ));
    }
    if i.via.trim().is_empty() || i.municipio.trim().is_empty() {
        return Err(other("pon la vía y el municipio del inmueble"));
    }
    let p = i.provincia.trim();
    let prov_ok = p.len() == 2 && p.parse::<u8>().is_ok_and(|n| (1..=53).contains(&n));
    if !prov_ok {
        return Err(other(
            "el código de provincia son dos cifras (01 a 52, 53 La Palma)",
        ));
    }
    let cp = i.codigo_postal.trim();
    if cp.len() != 5 || !cp.chars().all(|c| c.is_ascii_digit()) || !cp.starts_with(p) {
        return Err(other(
            "el código postal son cinco cifras y empieza por el código de la provincia",
        ));
    }
    Ok(())
}

const COLS: &str = "nif, situacion, ref_catastral, via, numero, municipio, provincia, \
                    codigo_postal, exonerado_hasta";

fn of_row(r: &taxpain_vault::rusqlite::Row<'_>) -> taxpain_vault::rusqlite::Result<Inmueble> {
    let hasta: Option<String> = r.get(8)?;
    Ok(Inmueble {
        nif: r.get(0)?,
        situacion: r.get(1)?,
        ref_catastral: r.get(2)?,
        via: r.get(3)?,
        numero: r.get(4)?,
        municipio: r.get(5)?,
        provincia: r.get(6)?,
        codigo_postal: r.get(7)?,
        exonerado_hasta: hasta.and_then(|s| crate::libros_societarios::parse_date(&s).ok()),
    })
}

/// Todos los inmuebles guardados, por NIF.
pub fn list(v: &Vault) -> Result<Vec<Inmueble>> {
    let mut st = v.connection().prepare(&format!(
        "SELECT {COLS} FROM inmueble_arrendado ORDER BY nif"
    ))?;
    let rows = st.query_map([], of_row)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub(crate) fn of_nif(conn: &Connection, nif: &str) -> Result<Option<Inmueble>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM inmueble_arrendado WHERE nif = ?1"),
            [nif.trim().to_uppercase()],
            of_row,
        )
        .optional()?)
}

/// Guarda (o sustituye) el inmueble de un arrendador.
pub fn save(v: &Vault, i: &Inmueble, now: OffsetDateTime) -> Result<()> {
    validar(i)?;
    let nif = i.nif.trim().to_uppercase();
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO inmueble_arrendado(nif, situacion, ref_catastral, via, numero, municipio,
           provincia, codigo_postal, exonerado_hasta, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(nif) DO UPDATE SET situacion = excluded.situacion,
           ref_catastral = excluded.ref_catastral, via = excluded.via, numero = excluded.numero,
           municipio = excluded.municipio, provincia = excluded.provincia,
           codigo_postal = excluded.codigo_postal, exonerado_hasta = excluded.exonerado_hasta,
           updated_at = excluded.updated_at",
        params![
            nif,
            i.situacion,
            i.ref_catastral.trim().to_uppercase(),
            i.via.trim(),
            i.numero.trim(),
            i.municipio.trim(),
            i.provincia.trim(),
            i.codigo_postal.trim(),
            i.exonerado_hasta.map(|d| d.to_string()),
            now_str(now),
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "inmueble.save",
        &nif,
        &serde_json::json!({
            "situacion": i.situacion,
            "ref_catastral": i.ref_catastral.trim().to_uppercase(),
        }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Quita el inmueble de un arrendador (con la confirmación de la pantalla).
pub fn delete(v: &Vault, nif: &str, confirmed: bool, now: OffsetDateTime) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let nif = nif.trim().to_uppercase();
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if tx.execute("DELETE FROM inmueble_arrendado WHERE nif = ?1", [&nif])? == 0 {
        return Err(other("ese arrendador no tiene inmueble guardado"));
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "inmueble.delete",
        &nif,
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Límite anual por arrendador por debajo del cual no hay que retener (art. 75.3.g RIRPF).
pub const LIMITE_SIN_RETENCION_CENTS: i64 = 90_000;

/// Aviso del 115 para un arrendador con alquileres sin retención: `None` si tiene acreditación
/// vigente a `fecha` o si lo pagado en el año (`anual`, sin IVA) no pasa de 900 €.
pub(crate) fn aviso_sin_retencion(
    conn: &Connection,
    nif: &str,
    nombre: &str,
    fecha: Date,
    anual: taxpain_core::Money,
) -> Result<Option<String>> {
    if anual.cents() <= LIMITE_SIN_RETENCION_CENTS {
        return Ok(None);
    }
    let i = of_nif(conn, nif)?;
    if i.as_ref()
        .and_then(|i| i.exonerado_hasta)
        .is_some_and(|h| h >= fecha)
    {
        return Ok(None);
    }
    Ok(Some(format!(
        "{nombre} ({nif}): alquiler sin retención y {anual} en el año, sin acreditación vigente \
         del arrendador para no retener (art. 75.3.g RIRPF). Si no la tienes, hay que retener el \
         19 %; si la tienes, apunta hasta cuándo vale en los datos del inmueble."
    )))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use std::sync::Arc;

    use taxpain_core::Money;
    use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider};
    use time::macros::{date, datetime};

    use super::*;

    fn vault() -> Vault {
        Vault::open_in_memory(
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
        )
        .unwrap()
    }

    pub(crate) fn oficina() -> Inmueble {
        Inmueble {
            nif: "a22222228".into(),
            situacion: 1,
            ref_catastral: "9872023VH5797S0001WX".into(),
            via: "Calle Ejemplo".into(),
            numero: "10".into(),
            municipio: "València".into(),
            provincia: "46".into(),
            codigo_postal: "46001".into(),
            exonerado_hasta: None,
        }
    }

    #[test]
    fn valida_como_el_diseno_del_180() {
        assert!(validar(&oficina()).is_ok());
        let mut i = oficina();
        i.ref_catastral = "123".into();
        assert!(validar(&i).is_err());
        i.situacion = 4;
        assert!(validar(&i).is_err());
        i.ref_catastral.clear();
        assert!(validar(&i).is_ok());
        let mut i = oficina();
        i.codigo_postal = "28001".into();
        assert!(validar(&i).is_err());
        i.provincia = "99".into();
        assert!(validar(&i).is_err());
    }

    #[test]
    fn guarda_sustituye_y_quita() {
        let v = vault();
        let now = datetime!(2026-11-17 10:00 UTC);
        save(&v, &oficina(), now).unwrap();
        let mut i = oficina();
        i.numero = "12".into();
        save(&v, &i, now).unwrap();
        let all = list(&v).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].nif, "A22222228");
        assert_eq!(all[0].numero, "12");
        assert_eq!(all[0].property().municipality, "VALÈNCIA");
        assert!(
            all[0]
                .texto()
                .contains("ref. catastral 9872023VH5797S0001WX")
        );
        assert!(delete(&v, "A22222228", false, now).is_err());
        delete(&v, "a22222228", true, now).unwrap();
        assert!(list(&v).unwrap().is_empty());
    }

    #[test]
    fn avisa_del_alquiler_sin_retencion_sin_acreditacion() {
        let v = vault();
        let now = datetime!(2026-11-17 10:00 UTC);
        let c = v.connection();
        let d = date!(2026 - 05 - 01);
        let mucho = Money::from_euros(6_000, 0);
        assert!(
            aviso_sin_retencion(c, "A22222228", "Inmuebles", d, Money::from_euros(900, 0))
                .unwrap()
                .is_none()
        );
        assert!(
            aviso_sin_retencion(c, "A22222228", "Inmuebles", d, mucho)
                .unwrap()
                .is_some()
        );
        let mut i = oficina();
        i.exonerado_hasta = Some(date!(2026 - 12 - 31));
        save(&v, &i, now).unwrap();
        assert!(
            aviso_sin_retencion(c, "A22222228", "Inmuebles", d, mucho)
                .unwrap()
                .is_none()
        );
        assert!(
            aviso_sin_retencion(c, "A22222228", "Inmuebles", date!(2027 - 01 - 10), mucho)
                .unwrap()
                .is_some()
        );
    }
}

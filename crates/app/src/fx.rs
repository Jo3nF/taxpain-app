//! Tipos de cambio descargados del BCE («Tipo del BCE»): caché en la bóveda y auditoría.
//!
//! La descarga la hace la app (`taxpain_aeat::bce`) sólo cuando el usuario pulsa el botón; aquí
//! se guarda lo descargado para no repetir la consulta y queda en la auditoría (`fx.bce`, con
//! el host consultado). Sólo se guarda un tipo cuando ya no puede cambiar: el de un día pasado
//! o el publicado ese mismo día (hoy, antes de las 16:00, aún no está y se usaría el de ayer).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::fx::{ExchangeRate, ForeignCurrency, RateSource};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

const CACHE_KEY: &str = "fx_bce_cache";
/// Entradas como mucho (las más antiguas se descartan).
const CACHE_MAX: usize = 400;

/// Tipo del BCE para una fecha pedida.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BceRate {
    pub currency: String,
    /// Fecha pedida (la del devengo).
    pub requested: Date,
    /// Fecha de publicación del tipo (la pedida o el último día hábil anterior).
    pub date: Date,
    /// Unidades de la moneda por 1 €.
    pub rate: ExchangeRate,
    /// Salió de la caché (sin consultar al BCE).
    #[serde(default)]
    pub cached: bool,
}

impl BceRate {
    /// La moneda de una factura con este tipo.
    pub fn currency(&self) -> ForeignCurrency {
        ForeignCurrency {
            code: self.currency.clone(),
            rate: self.rate,
            rate_date: self.date,
            source: RateSource::Bce,
        }
    }
}

fn key(currency: &str, requested: Date) -> String {
    format!("{currency}:{requested}")
}

fn load(v: &Vault) -> Result<BTreeMap<String, BceRate>> {
    Ok(v.setting(CACHE_KEY)?.unwrap_or_default())
}

/// La caché leída de la conexión (dentro de una transacción ya abierta).
pub(crate) fn cache_conn(conn: &Connection) -> Result<BTreeMap<String, BceRate>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [CACHE_KEY],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e| AppError::Other(e.to_string()))?
        .unwrap_or_default())
}

/// Tipo del BCE de `currency` para el día `date` en la caché ya leída: el pedido para ese día
/// (el publicado ese día o, en un día sin publicación, el del último día hábil anterior) o, si
/// no se pidió ese día, uno publicado justo ese día.
pub(crate) fn rate_in(
    cache: &BTreeMap<String, BceRate>,
    currency: &str,
    date: Date,
) -> Option<BceRate> {
    cache
        .get(&key(currency, date))
        .or_else(|| {
            cache
                .values()
                .find(|r| r.currency == currency && r.date == date)
        })
        .map(|r| BceRate {
            cached: true,
            ..r.clone()
        })
}

/// [`rate_in`] leyendo la caché. Sin red: si no está, `None`.
pub(crate) fn rate_on(conn: &Connection, currency: &str, date: Date) -> Result<Option<BceRate>> {
    Ok(rate_in(&cache_conn(conn)?, currency, date))
}

/// El último tipo ya descargado de `currency` publicado no después de `hasta`.
pub fn ultimo_cached(v: &Vault, currency: &str, hasta: Date) -> Result<Option<BceRate>> {
    Ok(load(v)?
        .into_values()
        .filter(|r| r.currency == currency && r.date <= hasta)
        .max_by_key(|r| r.date)
        .map(|mut r| {
            r.cached = true;
            r
        }))
}

/// Tipo ya descargado para `currency` y la fecha `requested`, si lo hay.
pub fn cached(v: &Vault, currency: &str, requested: Date) -> Result<Option<BceRate>> {
    Ok(load(v)?.remove(&key(currency, requested)).map(|mut r| {
        r.cached = true;
        r
    }))
}

/// Un tipo marcado como del BCE tiene que ser el que el BCE publicó para esa moneda ese día, si
/// está en la caché (p. ej. el de otra moneda que se quedó al cambiarla en el formulario).
pub fn check_bce(v: &Vault, c: Option<&ForeignCurrency>) -> Result<()> {
    let Some(c) = c.filter(|c| c.source == RateSource::Bce) else {
        return Ok(());
    };
    let published = load(v)?
        .into_values()
        .find(|r| r.currency == c.code && r.date == c.rate_date);
    match published {
        Some(r) if r.rate != c.rate => Err(AppError::Other(format!(
            "el tipo {} no es el del BCE para {} del {} ({}): pulsa «Tipo del BCE» de nuevo o \
             escríbelo a mano",
            c.rate.decimal_es(),
            c.code,
            c.rate_date,
            r.rate.decimal_es()
        ))),
        _ => Ok(()),
    }
}

/// Guarda en la caché (si ya no puede cambiar) y audita un tipo recién descargado.
pub fn record(
    v: &Vault,
    host: &str,
    rate: &BceRate,
    today: Date,
    now: OffsetDateTime,
) -> Result<()> {
    if rate.date > rate.requested {
        return Err(AppError::Other(
            "el BCE devolvió un tipo posterior a la fecha pedida".into(),
        ));
    }
    if rate.requested < today || rate.date == rate.requested {
        let mut cache = load(v)?;
        cache.insert(
            key(&rate.currency, rate.requested),
            BceRate {
                cached: false,
                ..rate.clone()
            },
        );
        while cache.len() > CACHE_MAX {
            // Las claves empiezan por la moneda: se quita la de fecha pedida más antigua.
            let oldest = cache
                .iter()
                .min_by_key(|(_, r)| r.requested)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => {
                    cache.remove(&k);
                }
                None => break,
            }
        }
        v.set_setting(CACHE_KEY, &cache)?;
    }
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "fx.bce",
        &rate.currency,
        &serde_json::json!({
            "host": host,
            "requested": rate.requested.to_string(),
            "date": rate.date.to_string(),
            "rate": rate.rate.decimal(),
        }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    fn rate(requested: Date, date: Date) -> BceRate {
        BceRate {
            currency: "USD".into(),
            requested,
            date,
            rate: ExchangeRate::parse("1.1193").unwrap(),
            cached: false,
        }
    }

    #[test]
    fn guarda_lo_que_ya_no_cambia_y_lo_audita() {
        let v = Vault::open_in_memory(
            std::sync::Arc::new(taxpain_vault::MemoryKeyProvider::default()),
            std::sync::Arc::new(taxpain_vault::AlwaysConfirm),
        )
        .unwrap();
        let now = datetime!(2026-10-24 10:00 UTC);
        let today = date!(2026 - 10 - 24);
        // Hoy, sin publicar aún: se usa el de ayer pero no se guarda.
        record(
            &v,
            "data-api.ecb.europa.eu",
            &rate(today, date!(2026 - 10 - 23)),
            today,
            now,
        )
        .unwrap();
        assert!(cached(&v, "USD", today).unwrap().is_none());
        // Un día pasado (sábado → viernes): se guarda.
        let sat = date!(2026 - 10 - 17);
        record(
            &v,
            "data-api.ecb.europa.eu",
            &rate(sat, date!(2026 - 10 - 16)),
            today,
            now,
        )
        .unwrap();
        let c = cached(&v, "USD", sat).unwrap().unwrap();
        assert!(c.cached);
        assert_eq!(c.date, date!(2026 - 10 - 16));
        assert_eq!(c.currency().rate_date, date!(2026 - 10 - 16));
        assert_eq!(c.currency().source, RateSource::Bce);
        // Nunca un tipo posterior a la fecha pedida.
        assert!(record(&v, "x", &rate(sat, date!(2026 - 10 - 18)), today, now).is_err());
        let n: i64 = v
            .connection()
            .query_row(
                "SELECT count(*) FROM audit_log WHERE action = 'fx.bce'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn un_tipo_del_bce_tiene_que_ser_el_publicado() {
        let v = Vault::open_in_memory(
            std::sync::Arc::new(taxpain_vault::MemoryKeyProvider::default()),
            std::sync::Arc::new(taxpain_vault::AlwaysConfirm),
        )
        .unwrap();
        let (d, today) = (date!(2026 - 10 - 16), date!(2026 - 10 - 20));
        record(&v, "x", &rate(d, d), today, datetime!(2026-10-20 10:00 UTC)).unwrap();
        let mut c = rate(d, d).currency();
        check_bce(&v, Some(&c)).unwrap();
        // El de dólares que se quedó al cambiar a libras, marcado como del BCE: no.
        c.code = "GBP".into();
        check_bce(&v, Some(&c)).unwrap(); // sin libras en la caché: no se sabe
        c.code = "USD".into();
        c.rate = ExchangeRate::parse("0.8712").unwrap();
        let err = check_bce(&v, Some(&c)).unwrap_err().to_string();
        assert!(err.contains("no es el del BCE"), "{err}");
        // Escrito a mano, sí (el usuario responde de él).
        c.source = RateSource::Manual;
        check_bce(&v, Some(&c)).unwrap();
        check_bce(&v, None).unwrap();
    }
}

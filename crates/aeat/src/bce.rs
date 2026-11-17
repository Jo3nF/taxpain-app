//! Tipo de cambio de referencia del euro del BCE para una fecha, a petición del usuario (botón
//! «Tipo del BCE» de las facturas en moneda extranjera).
//!
//! El art. 79.Once LIVA manda aplicar el tipo vendedor fijado por el Banco de España vigente al
//! devengo; los cambios oficiales del Banco de España son los tipos de referencia que publica el
//! BCE (art. 36 Ley 46/1998). Se piden a la API de datos del BCE (`EXR`, serie diaria
//! `D.<moneda>.EUR.SP00.A`) los últimos días hasta la fecha y se toma el último publicado: un
//! sábado, domingo o festivo de TARGET vale el del último día hábil.
//!
//! Se envían sólo el código de la moneda y las fechas. Mismas reglas que con la AEAT (D6):
//! sólo `https`, sólo el host de la lista, sin redirecciones ni proxies, servidor verificado
//! con el almacén de confianza del sistema, sin certificado de cliente, tiempo de espera corto
//! y en el registro sólo el host y el estado.

use std::time::Duration;

use serde::Serialize;
use taxpain_core::fx::ExchangeRate;
use time::Date;
use url::Url;

use crate::http::AeatError;

pub const HOST: &str = "data-api.ecb.europa.eu";
const BASE: &str = "https://data-api.ecb.europa.eu/service/data/EXR/";
/// Días hacia atrás que se piden: cubren el hueco más largo sin publicación (Semana Santa,
/// Navidad) con margen.
const WINDOW_DAYS: i64 = 10;

/// Tipo de referencia publicado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EcbRate {
    pub currency: String,
    /// Día de publicación del tipo (el pedido o el último hábil anterior).
    pub date: Date,
    /// Unidades de la moneda por 1 €.
    pub rate: ExchangeRate,
}

fn valid_currency(c: &str) -> bool {
    c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase()) && c != "EUR"
}

fn iso(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

/// URL de la consulta: la serie diaria de `currency` entre `date − 10 días` y `date`.
pub fn url(currency: &str, date: Date) -> Result<Url, AeatError> {
    if !valid_currency(currency) {
        return Err(AeatError::InvalidUrl);
    }
    let start = date
        .checked_sub(time::Duration::days(WINDOW_DAYS))
        .ok_or(AeatError::InvalidUrl)?;
    let u = Url::parse(&format!(
        "{BASE}D.{currency}.EUR.SP00.A?startPeriod={}&endPeriod={}&format=csvdata&detail=dataonly",
        iso(start),
        iso(date)
    ))
    .map_err(|_| AeatError::InvalidUrl)?;
    if u.scheme() != "https" || u.host_str() != Some(HOST) {
        return Err(AeatError::HostNotAllowed(u.host_str().unwrap_or("").into()));
    }
    Ok(u)
}

/// URL de la serie diaria de `currency` entre `start − 10 días` y `end` (una sola consulta para
/// todas las fechas de un periodo: las operaciones de una cuenta de bróker en dólares).
pub fn url_range(currency: &str, start: Date, end: Date) -> Result<Url, AeatError> {
    if start > end || (end - start).whole_days() > 800 {
        return Err(AeatError::InvalidUrl);
    }
    let mut u = url(currency, end)?;
    let desde = start
        .checked_sub(time::Duration::days(WINDOW_DAYS))
        .ok_or(AeatError::InvalidUrl)?;
    u.set_query(Some(&format!(
        "startPeriod={}&endPeriod={}&format=csvdata&detail=dataonly",
        iso(desde),
        iso(end)
    )));
    Ok(u)
}

/// Todos los tipos de `currency` de la respuesta CSV, por fecha de publicación.
pub fn parse_csv_all(body: &str, currency: &str) -> Result<Vec<EcbRate>, AeatError> {
    let mut out: Vec<EcbRate> = Vec::new();
    // Se reutiliza el lector de un día: cada fecha publicada, con la suya como tope.
    let mut lines = body.lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else {
        return Ok(out);
    };
    let cols: Vec<&str> = header.split(',').map(str::trim).collect();
    let Some(i_date) = cols.iter().position(|c| *c == "TIME_PERIOD") else {
        return Err(AeatError::InvalidResponse(
            "falta la columna TIME_PERIOD".into(),
        ));
    };
    let mut fechas: Vec<Date> = lines
        .filter_map(|l| l.split(',').map(str::trim).nth(i_date).map(str::to_owned))
        .filter_map(|d| {
            Date::parse(
                &d,
                time::macros::format_description!("[year]-[month]-[day]"),
            )
            .ok()
        })
        .collect();
    fechas.sort_unstable();
    fechas.dedup();
    for d in fechas {
        if let Some(r) = parse_csv(body, currency, d)?
            && r.date == d
        {
            out.push(r);
        }
    }
    Ok(out)
}

/// Descarga en una sola consulta los tipos de referencia de `currency` publicados entre
/// `start − 10 días` y `end` (botón «Pasar a euros con el BCE» de Inversiones en valores).
pub async fn reference_rates(
    currency: &str,
    start: Date,
    end: Date,
) -> Result<Vec<EcbRate>, AeatError> {
    let url = url_range(currency, start, end)?;
    let http = crate::http::public_client(Duration::from_secs(30))?;
    tracing::info!(host = HOST, "consulta tipos de cambio BCE (periodo)");
    let res = http
        .get(url)
        .header("Accept", "text/csv")
        .send()
        .await
        .map_err(|e| AeatError::Network(crate::http::without_url(&e)))?;
    let status = res.status();
    tracing::info!(status = status.as_u16(), "respuesta BCE");
    if status.as_u16() == 404 {
        return Err(AeatError::InvalidResponse(format!(
            "el BCE no publica tipo de referencia para {currency}: escríbelo a mano"
        )));
    }
    if !status.is_success() {
        return Err(AeatError::Status(status.as_u16()));
    }
    let body = crate::http::read_text_limited(res, crate::http::limits::SMALL).await?;
    parse_csv_all(&body, currency)
}

/// Lee la respuesta CSV (`TIME_PERIOD`, `OBS_VALUE`…) y devuelve el último tipo de `currency`
/// publicado como muy tarde el `date`. `None` si no hay ninguno.
pub fn parse_csv(body: &str, currency: &str, date: Date) -> Result<Option<EcbRate>, AeatError> {
    let mut lines = body.lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else {
        return Ok(None);
    };
    let cols: Vec<&str> = header.split(',').map(str::trim).collect();
    let col = |name: &str| {
        cols.iter()
            .position(|c| *c == name)
            .ok_or_else(|| AeatError::InvalidResponse(format!("falta la columna {name}")))
    };
    let (i_cur, i_date, i_value) = (col("CURRENCY")?, col("TIME_PERIOD")?, col("OBS_VALUE")?);
    let mut best: Option<EcbRate> = None;
    for line in lines {
        let f: Vec<&str> = line.split(',').map(str::trim).collect();
        let (Some(cur), Some(d), Some(v)) = (f.get(i_cur), f.get(i_date), f.get(i_value)) else {
            continue;
        };
        if *cur != currency {
            continue;
        }
        let Ok(d) = Date::parse(d, time::macros::format_description!("[year]-[month]-[day]"))
        else {
            continue;
        };
        let Some(rate) = ExchangeRate::parse(v) else {
            continue;
        };
        if d <= date && best.as_ref().is_none_or(|b| d > b.date) {
            best = Some(EcbRate {
                currency: currency.to_owned(),
                date: d,
                rate,
            });
        }
    }
    Ok(best)
}

/// Descarga el tipo de referencia de `currency` vigente el `date` (el de ese día o el último
/// publicado antes).
pub async fn reference_rate(currency: &str, date: Date) -> Result<EcbRate, AeatError> {
    let url = url(currency, date)?;
    let http = crate::http::public_client(Duration::from_secs(20))?;
    tracing::info!(host = HOST, "consulta tipo de cambio BCE");
    let res = http
        .get(url)
        .header("Accept", "text/csv")
        .send()
        .await
        .map_err(|e| AeatError::Network(crate::http::without_url(&e)))?;
    let status = res.status();
    tracing::info!(status = status.as_u16(), "respuesta BCE");
    // 404: el BCE no publica esa moneda.
    if status.as_u16() == 404 {
        return Err(AeatError::InvalidResponse(format!(
            "el BCE no publica tipo de referencia para {currency}: escríbelo a mano"
        )));
    }
    if !status.is_success() {
        return Err(AeatError::Status(status.as_u16()));
    }
    let body = crate::http::read_text_limited(res, crate::http::limits::SMALL).await?;
    parse_csv(&body, currency, date)?.ok_or_else(|| {
        AeatError::InvalidResponse(format!(
            "el BCE no tiene tipo de {currency} para esa fecha ni los días anteriores"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn url_con_la_ventana_de_dias() {
        let u = url("USD", date!(2026 - 10 - 24)).unwrap();
        assert_eq!(u.host_str(), Some(HOST));
        assert_eq!(
            u.as_str(),
            "https://data-api.ecb.europa.eu/service/data/EXR/D.USD.EUR.SP00.A?startPeriod=2026-10-14&endPeriod=2026-10-24&format=csvdata&detail=dataonly"
        );
        for bad in ["usd", "EUR", "US", "USD/../x", "U$D"] {
            assert!(url(bad, date!(2026 - 10 - 24)).is_err(), "{bad}");
        }
    }

    #[test]
    fn toma_el_ultimo_publicado_hasta_la_fecha() {
        // Respuesta real (recortada) de la API del BCE.
        let body = "KEY,FREQ,CURRENCY,CURRENCY_DENOM,EXR_TYPE,EXR_SUFFIX,TIME_PERIOD,OBS_VALUE\n\
            EXR.D.USD.EUR.SP00.A,D,USD,EUR,SP00,A,2026-10-21,1.1234\n\
            EXR.D.USD.EUR.SP00.A,D,USD,EUR,SP00,A,2026-10-23,1.1193\n\
            EXR.D.USD.EUR.SP00.A,D,USD,EUR,SP00,A,2026-10-22,1.1205\n";
        // Sábado: el del viernes.
        let r = parse_csv(body, "USD", date!(2026 - 10 - 24))
            .unwrap()
            .unwrap();
        assert_eq!(r.date, date!(2026 - 10 - 23));
        assert_eq!(r.rate.decimal(), "1.1193");
        // Un día concreto: el suyo, no uno posterior.
        let r = parse_csv(body, "USD", date!(2026 - 10 - 22))
            .unwrap()
            .unwrap();
        assert_eq!(r.rate.decimal(), "1.1205");
        assert!(
            parse_csv(body, "USD", date!(2026 - 10 - 20))
                .unwrap()
                .is_none()
        );
        assert!(
            parse_csv(body, "GBP", date!(2026 - 10 - 24))
                .unwrap()
                .is_none()
        );
        // Respuesta vacía (sólo fines de semana) o sin las columnas.
        assert!(
            parse_csv("", "USD", date!(2026 - 10 - 24))
                .unwrap()
                .is_none()
        );
        assert!(parse_csv("A,B\n1,2", "USD", date!(2026 - 10 - 24)).is_err());
    }

    #[test]
    fn un_periodo_en_una_sola_consulta() {
        let u = url_range("USD", date!(2026 - 01 - 01), date!(2026 - 11 - 02)).unwrap();
        assert_eq!(
            u.as_str(),
            "https://data-api.ecb.europa.eu/service/data/EXR/D.USD.EUR.SP00.A?startPeriod=2025-12-22&endPeriod=2026-11-02&format=csvdata&detail=dataonly"
        );
        assert!(url_range("USD", date!(2026 - 11 - 02), date!(2026 - 01 - 01)).is_err());
        assert!(url_range("EUR", date!(2026 - 01 - 01), date!(2026 - 11 - 02)).is_err());
        let body = "KEY,FREQ,CURRENCY,CURRENCY_DENOM,EXR_TYPE,EXR_SUFFIX,TIME_PERIOD,OBS_VALUE\n\
            EXR.D.USD.EUR.SP00.A,D,USD,EUR,SP00,A,2026-10-23,1.1193\n\
            EXR.D.USD.EUR.SP00.A,D,USD,EUR,SP00,A,2026-10-22,1.1205\n";
        let r = parse_csv_all(body, "USD").unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].date, date!(2026 - 10 - 22));
        assert_eq!(r[1].rate.decimal(), "1.1193");
    }
}

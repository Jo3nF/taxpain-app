//! Cliente de la API de Enable Banking (`https://api.enablebanking.com`), con los campos de su
//! referencia (https://enablebanking.com/docs/api/reference/). Sólo lo de información de
//! cuentas: bancos, autorización, sesión, cuentas, saldos y movimientos. Nada de pagos.
//!
//! Mismas reglas que el resto de conexiones de TaxPain: sólo `https` al host exacto, servidor
//! verificado con el almacén de confianza del sistema, sin redirecciones ni proxies, errores sin
//! la URL y, en el registro, sólo el tipo de petición y el estado.

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use taxpain_vault::rustls::ClientConfig;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use url::Url;

use crate::jwt::Firmante;
use crate::{BancaError, Result};

/// Único host al que habla el cliente en producción.
pub const HOST: &str = "api.enablebanking.com";
const BASE: &str = "https://api.enablebanking.com/";
/// Páginas de movimientos como mucho en una descarga (cada una trae decenas o cientos).
const MAX_PAGINAS: usize = 500;

/// Una lista que puede venir como `null` (igual que si no viniera).
fn nulo_vacio<'de, D, T>(d: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

/// Un banco de la lista de Enable Banking.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aspsp {
    pub name: String,
    pub country: String,
    /// «business», «personal».
    #[serde(default, deserialize_with = "nulo_vacio")]
    pub psu_types: Vec<String>,
    /// Segundos que puede durar el permiso como mucho.
    #[serde(default)]
    pub maximum_consent_validity: Option<i64>,
    #[serde(default)]
    pub beta: bool,
}

/// Lo que hace falta para pedir la autorización al banco.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeticionAuth {
    /// Nombre del banco tal como lo da [`Cliente::bancos`] («BBVA», «Revolut»).
    pub banco: String,
    /// País del banco (ISO 3166, «ES»).
    pub pais: String,
    /// «business» (cuenta de empresa) o «personal».
    pub tipo: String,
    /// Dirección de vuelta registrada en la aplicación.
    pub url_vuelta: String,
    /// Valor al azar que vuelve igual en la dirección de vuelta.
    pub state: String,
    /// Hasta cuándo dura el permiso.
    pub hasta: OffsetDateTime,
}

/// Dirección del banco a la que hay que llevar al titular para que autorice.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Autorizacion {
    pub url: String,
    #[serde(default)]
    pub authorization_id: Option<String>,
}

/// Identificación de una cuenta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct IdCuenta {
    #[serde(default)]
    pub iban: Option<String>,
    /// Otra identificación (el número enmascarado de una tarjeta, por ejemplo).
    #[serde(default)]
    pub other: Option<OtraId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OtraId {
    #[serde(default)]
    pub identification: String,
    #[serde(default)]
    pub scheme_name: Option<String>,
}

/// Una cuenta autorizada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cuenta {
    /// Identificador de la cuenta en Enable Banking para esta sesión.
    pub uid: String,
    #[serde(default)]
    pub account_id: Option<IdCuenta>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub product: Option<String>,
    /// Tipo de cuenta ISO 20022 («CACC» corriente, «CARD» tarjeta…).
    #[serde(default)]
    pub cash_account_type: Option<String>,
    /// Huella estable de la cuenta (la misma en sesiones distintas).
    #[serde(default)]
    pub identification_hash: Option<String>,
}

impl Cuenta {
    pub fn iban(&self) -> Option<&str> {
        self.account_id
            .as_ref()
            .and_then(|a| a.iban.as_deref())
            .filter(|i| !i.trim().is_empty())
    }

    /// Es una tarjeta (tipo de cuenta ISO 20022 «CARD»).
    pub fn es_tarjeta(&self) -> bool {
        self.cash_account_type
            .as_deref()
            .is_some_and(|t| t.eq_ignore_ascii_case("CARD"))
    }

    /// Últimos cuatro dígitos de la otra identificación (el número enmascarado de una tarjeta).
    pub fn ultimos(&self) -> Option<String> {
        let id = &self.account_id.as_ref()?.other.as_ref()?.identification;
        let digitos: Vec<char> = id.chars().filter(char::is_ascii_digit).collect();
        (digitos.len() >= 4).then(|| digitos[digitos.len() - 4..].iter().collect())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AspspRef {
    pub name: String,
    pub country: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acceso {
    pub valid_until: String,
}

/// Una sesión abierta con el código de vuelta: el permiso y sus cuentas.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sesion {
    pub session_id: String,
    #[serde(default, deserialize_with = "nulo_vacio")]
    pub accounts: Vec<Cuenta>,
    pub aspsp: AspspRef,
    #[serde(default)]
    pub psu_type: Option<String>,
    pub access: Acceso,
}

/// Estado de una sesión.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct EstadoSesion {
    #[serde(default)]
    pub status: Option<String>,
    pub access: Acceso,
}

/// Importe con su divisa (el importe va como texto: «1.23»).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Importe {
    pub currency: String,
    pub amount: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Parte {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CodigoBanco {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub sub_code: Option<String>,
}

/// Un movimiento.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaccion {
    #[serde(default)]
    pub entry_reference: Option<String>,
    #[serde(default)]
    pub transaction_id: Option<String>,
    pub transaction_amount: Importe,
    /// «CRDT» (abono, entra dinero) o «DBIT» (cargo).
    pub credit_debit_indicator: String,
    /// «BOOK» contabilizado, «PDNG» pendiente…
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub booking_date: Option<String>,
    #[serde(default)]
    pub value_date: Option<String>,
    #[serde(default)]
    pub transaction_date: Option<String>,
    #[serde(default, deserialize_with = "nulo_vacio")]
    pub remittance_information: Vec<String>,
    #[serde(default)]
    pub creditor: Option<Parte>,
    #[serde(default)]
    pub debtor: Option<Parte>,
    /// Saldo después del movimiento: según el banco, un importe o un saldo con su importe.
    #[serde(default)]
    pub balance_after_transaction: Option<serde_json::Value>,
    #[serde(default)]
    pub bank_transaction_code: Option<CodigoBanco>,
    #[serde(default)]
    pub note: Option<String>,
}

impl Transaccion {
    /// Contabilizado (no pendiente, rechazado ni cancelado).
    pub fn contabilizado(&self) -> bool {
        match self.status.as_deref() {
            Some(s) => s.eq_ignore_ascii_case("BOOK"),
            None => self.booking_date.is_some(),
        }
    }

    /// Identificador estable del movimiento en el banco, si lo da: la referencia de la entrada
    /// (`entry_reference`), única e inmutable en la cuenta según la referencia de Enable Banking
    /// (también entre sesiones). El `transaction_id` no sirve para eso.
    pub fn id(&self) -> Option<&str> {
        self.entry_reference
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Importe con signo (+ abono, − cargo) en céntimos.
    pub fn centimos(&self) -> Option<i64> {
        let c = centimos(&self.transaction_amount.amount)?.abs();
        Some(
            if self.credit_debit_indicator.eq_ignore_ascii_case("DBIT") {
                -c
            } else {
                c
            },
        )
    }

    /// Saldo después del movimiento en céntimos, si lo da en la divisa `divisa`.
    pub fn saldo(&self, divisa: &str) -> Option<i64> {
        let v = self.balance_after_transaction.as_ref()?;
        let importe = v.get("balance_amount").unwrap_or(v);
        let d = importe.get("currency")?.as_str()?;
        if !d.eq_ignore_ascii_case(divisa) {
            return None;
        }
        centimos(importe.get("amount")?.as_str()?)
    }

    /// Concepto para el extracto: lo que dice el banco del movimiento.
    pub fn concepto(&self) -> String {
        let mut partes: Vec<String> = self
            .remittance_information
            .iter()
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty())
            .collect();
        if partes.is_empty() {
            let otra = if self.credit_debit_indicator.eq_ignore_ascii_case("DBIT") {
                self.creditor.as_ref()
            } else {
                self.debtor.as_ref()
            };
            if let Some(n) = otra.and_then(|p| p.name.clone()) {
                partes.push(n);
            }
        }
        if partes.is_empty()
            && let Some(d) = self
                .bank_transaction_code
                .as_ref()
                .and_then(|c| c.description.clone())
        {
            partes.push(d);
        }
        if partes.is_empty()
            && let Some(n) = &self.note
        {
            partes.push(n.clone());
        }
        partes.join(" · ")
    }
}

/// Saldo de una cuenta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saldo {
    #[serde(default)]
    pub name: Option<String>,
    pub balance_amount: Importe,
    /// «CLBD» contable al cierre, «ITAV» disponible…
    #[serde(default)]
    pub balance_type: Option<String>,
    #[serde(default)]
    pub reference_date: Option<String>,
}

/// «123.45», «-0.5», «1000» → céntimos (redondea a 2 decimales).
pub fn centimos(texto: &str) -> Option<i64> {
    let t = texto.trim();
    let (signo, t) = match t.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, t.strip_prefix('+').unwrap_or(t)),
    };
    let (entero, dec) = t.split_once('.').unwrap_or((t, ""));
    if entero.is_empty() && dec.is_empty() {
        return None;
    }
    if !entero.chars().all(|c| c.is_ascii_digit()) || !dec.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let e: i64 = if entero.is_empty() {
        0
    } else {
        entero.parse().ok()?
    };
    let mut d: Vec<u8> = dec.bytes().map(|b| b - b'0').collect();
    d.resize(3, 0);
    let mut c = i64::from(d[0]) * 10 + i64::from(d[1]);
    if d[2] >= 5 {
        c += 1;
    }
    Some(signo * e.checked_mul(100)?.checked_add(c)?)
}

/// Cliente de Enable Banking con la clave de la aplicación de la empresa.
pub struct Cliente {
    http: reqwest::Client,
    base: Url,
    firmante: Firmante,
}

impl std::fmt::Debug for Cliente {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cliente")
            .field("host", &self.base.host_str())
            .field("firmante", &self.firmante)
            .finish_non_exhaustive()
    }
}

fn ahora() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// Mensaje del error sin la URL (lleva el uid de la cuenta).
fn sin_url(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    if let Some(u) = e.url() {
        msg = msg.replace(u.as_str(), "<url>");
    }
    let mut fuente = std::error::Error::source(e);
    while let Some(s) = fuente {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        fuente = s.source();
    }
    msg
}

fn red(e: &reqwest::Error) -> BancaError {
    if e.is_connect() {
        BancaError::SinConexion(sin_url(e))
    } else {
        BancaError::Red(sin_url(e))
    }
}

/// Error de la API: con su código, y [`BancaError::Caducado`] si la sesión ya no vale.
fn error_api(estado: u16, texto: &str) -> BancaError {
    #[derive(Deserialize)]
    struct E {
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        error: Option<String>,
    }
    let e: Option<E> = serde_json::from_str(texto).ok();
    let codigo = e.as_ref().and_then(|x| x.error.clone());
    let mensaje: String = e
        .and_then(|x| x.message)
        .unwrap_or_else(|| format!("error {estado}"))
        .chars()
        .take(300)
        .collect();
    let c = codigo.as_deref().unwrap_or_default().to_uppercase();
    if c.contains("SESSION")
        || c.contains("EXPIRED")
        || c.contains("REVOKED")
        || c == "ACCESS_DENIED"
        || c.contains("PSU_ACTION_REQUIRED")
    {
        return BancaError::Caducado;
    }
    BancaError::Api {
        estado,
        codigo,
        mensaje,
    }
}

/// Un identificador para ponerlo en la ruta (uid o id de sesión): sólo letras, cifras y guiones.
fn id_en_ruta(id: &str) -> Result<&str> {
    if !id.is_empty()
        && id.len() <= 100
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        Ok(id)
    } else {
        Err(BancaError::Respuesta("identificador no válido".into()))
    }
}

impl Cliente {
    /// Cliente de producción (`api.enablebanking.com`, servidor verificado con el almacén del
    /// sistema).
    pub fn nuevo(firmante: Firmante) -> Result<Self> {
        let proveedor = Arc::new(taxpain_vault::rustls::crypto::ring::default_provider());
        let verificador = rustls_platform_verifier::Verifier::new(proveedor.clone())
            .map_err(|e| BancaError::Tls(e.to_string()))?;
        let tls = ClientConfig::builder_with_provider(proveedor)
            .with_safe_default_protocol_versions()
            .map_err(|e| BancaError::Tls(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verificador))
            .with_no_client_auth();
        Self::con_tls(firmante, tls, BASE)
    }

    /// Para las pruebas: contra `base` (p. ej. `https://localhost:<puerto>/`) con una CA propia.
    /// Sólo con la feature `mock`: la app no lo lleva.
    #[cfg(any(test, feature = "mock"))]
    pub fn con_raiz(firmante: Firmante, raiz_der: &[u8], base: &str) -> Result<Self> {
        let proveedor = Arc::new(taxpain_vault::rustls::crypto::ring::default_provider());
        let mut raices = taxpain_vault::rustls::RootCertStore::empty();
        raices
            .add(taxpain_vault::rustls::pki_types::CertificateDer::from(
                raiz_der.to_vec(),
            ))
            .map_err(|e| BancaError::Tls(e.to_string()))?;
        let tls = ClientConfig::builder_with_provider(proveedor)
            .with_safe_default_protocol_versions()
            .map_err(|e| BancaError::Tls(e.to_string()))?
            .with_root_certificates(raices)
            .with_no_client_auth();
        Self::con_tls(firmante, tls, base)
    }

    fn con_tls(firmante: Firmante, mut tls: ClientConfig, base: &str) -> Result<Self> {
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        let base = Url::parse(base).map_err(|e| BancaError::Respuesta(e.to_string()))?;
        if base.scheme() != "https" {
            return Err(BancaError::Tls("sólo https".into()));
        }
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("TaxPain/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| BancaError::Tls(e.to_string()))?;
        Ok(Self {
            http,
            base,
            firmante,
        })
    }

    async fn pedir<T: DeserializeOwned>(
        &self,
        metodo: reqwest::Method,
        tipo: &'static str,
        ruta: &str,
        consulta: &[(&str, String)],
        cuerpo: Option<serde_json::Value>,
    ) -> Result<T> {
        let mut url = self
            .base
            .join(ruta)
            .map_err(|e| BancaError::Respuesta(e.to_string()))?;
        if !consulta.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(consulta.iter().map(|(k, v)| (*k, v.as_str())));
        }
        let token = self.firmante.token(ahora())?;
        let mut peticion = self
            .http
            .request(metodo, url)
            .bearer_auth(token)
            .header("Accept", "application/json");
        if let Some(c) = cuerpo {
            peticion = peticion.json(&c);
        }
        let respuesta = peticion.send().await.map_err(|e| red(&e))?;
        let estado = respuesta.status().as_u16();
        tracing::info!(
            host = self.base.host_str(),
            peticion = tipo,
            estado,
            "enable banking"
        );
        let texto = crate::lectura::leer_texto_acotado(respuesta, crate::lectura::topes::ENABLE)
            .await
            .map_err(|e| match e {
                crate::lectura::Lectura::Grande => BancaError::Grande,
                crate::lectura::Lectura::Red(e) => red(&e),
            })?;
        if !(200..300).contains(&estado) {
            return Err(error_api(estado, &texto));
        }
        serde_json::from_str(&texto).map_err(|e| BancaError::Respuesta(e.to_string()))
    }

    /// Bancos de un país.
    pub async fn bancos(&self, pais: &str) -> Result<Vec<Aspsp>> {
        #[derive(Deserialize)]
        struct R {
            #[serde(default, deserialize_with = "nulo_vacio")]
            aspsps: Vec<Aspsp>,
        }
        let r: R = self
            .pedir(
                reqwest::Method::GET,
                "bancos",
                "aspsps",
                &[("country", pais.to_owned())],
                None,
            )
            .await?;
        Ok(r.aspsps)
    }

    /// Empieza la autorización: devuelve la dirección del banco.
    pub async fn autorizar(&self, p: &PeticionAuth) -> Result<Autorizacion> {
        let hasta = p
            .hasta
            .format(&Rfc3339)
            .map_err(|e| BancaError::Respuesta(e.to_string()))?;
        let cuerpo = serde_json::json!({
            "access": { "valid_until": hasta },
            "aspsp": { "name": p.banco, "country": p.pais },
            "state": p.state,
            "redirect_url": p.url_vuelta,
            "psu_type": p.tipo,
            "language": "es",
        });
        self.pedir(
            reqwest::Method::POST,
            "autorizar",
            "auth",
            &[],
            Some(cuerpo),
        )
        .await
    }

    /// Abre la sesión con el código de la dirección de vuelta.
    pub async fn sesion(&self, code: &str) -> Result<Sesion> {
        self.pedir(
            reqwest::Method::POST,
            "sesion",
            "sessions",
            &[],
            Some(serde_json::json!({ "code": code })),
        )
        .await
    }

    /// Estado de una sesión (para saber si sigue autorizada).
    pub async fn estado_sesion(&self, id: &str) -> Result<EstadoSesion> {
        let ruta = format!("sessions/{}", id_en_ruta(id)?);
        self.pedir(reqwest::Method::GET, "estado", &ruta, &[], None)
            .await
    }

    /// Retira el permiso (cierra la sesión en Enable Banking).
    pub async fn borrar_sesion(&self, id: &str) -> Result<()> {
        let ruta = format!("sessions/{}", id_en_ruta(id)?);
        let _: serde_json::Value = self
            .pedir(reqwest::Method::DELETE, "retirar", &ruta, &[], None)
            .await?;
        Ok(())
    }

    /// Saldos de una cuenta.
    pub async fn saldos(&self, uid: &str) -> Result<Vec<Saldo>> {
        #[derive(Deserialize)]
        struct R {
            #[serde(default, deserialize_with = "nulo_vacio")]
            balances: Vec<Saldo>,
        }
        let ruta = format!("accounts/{}/balances", id_en_ruta(uid)?);
        let r: R = self
            .pedir(reqwest::Method::GET, "saldos", &ruta, &[], None)
            .await?;
        Ok(r.balances)
    }

    /// Movimientos de una cuenta desde `desde` (o, sin fecha, todo lo que dé el banco: la
    /// primera descarga pide el historial más largo, porque después muchos bancos sólo dan 90
    /// días), con todas sus páginas.
    pub async fn movimientos(
        &self,
        uid: &str,
        desde: Option<time::Date>,
    ) -> Result<Vec<Transaccion>> {
        #[derive(Deserialize)]
        struct R {
            #[serde(default, deserialize_with = "nulo_vacio")]
            transactions: Vec<Transaccion>,
            #[serde(default)]
            continuation_key: Option<String>,
        }
        let ruta = format!("accounts/{}/transactions", id_en_ruta(uid)?);
        let mut todos = Vec::new();
        let mut clave: Option<String> = None;
        for _ in 0..MAX_PAGINAS {
            let mut consulta: Vec<(&str, String)> = Vec::new();
            match desde {
                Some(d) => consulta.push(("date_from", d.to_string())),
                None => consulta.push(("strategy", "longest".to_owned())),
            }
            if let Some(k) = &clave {
                consulta.push(("continuation_key", k.clone()));
            }
            let r: R = self
                .pedir(reqwest::Method::GET, "movimientos", &ruta, &consulta, None)
                .await?;
            todos.extend(r.transactions);
            match r.continuation_key.filter(|k| !k.is_empty()) {
                Some(k) if Some(&k) != clave.as_ref() => clave = Some(k),
                _ => return Ok(todos),
            }
        }
        Err(BancaError::Respuesta(format!(
            "demasiadas páginas de movimientos (más de {MAX_PAGINAS})"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importes_en_centimos() {
        assert_eq!(centimos("123.45"), Some(12_345));
        assert_eq!(centimos("-0.5"), Some(-50));
        assert_eq!(centimos("1000"), Some(100_000));
        assert_eq!(centimos("2.005"), Some(201));
        assert_eq!(centimos(".75"), Some(75));
        assert_eq!(centimos("1,23"), None);
        assert_eq!(centimos(""), None);
        assert_eq!(centimos("abc"), None);
    }

    fn tx(json: serde_json::Value) -> Transaccion {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn movimiento_con_signo_saldo_y_concepto() {
        let t = tx(serde_json::json!({
            "entry_reference": "E1",
            "transaction_id": "T1",
            "transaction_amount": { "currency": "EUR", "amount": "12.30" },
            "credit_debit_indicator": "DBIT",
            "status": "BOOK",
            "booking_date": "2026-10-02",
            "value_date": "2026-10-03",
            "remittance_information": ["RECIBO  LUZ", " OCTUBRE "],
            "balance_after_transaction": { "balance_amount": { "currency": "EUR", "amount": "100.00" } }
        }));
        assert!(t.contabilizado());
        assert_eq!(t.centimos(), Some(-1_230));
        assert_eq!(t.saldo("EUR"), Some(10_000));
        assert_eq!(t.saldo("USD"), None);
        // El identificador es la referencia de la entrada, no el transaction_id.
        assert_eq!(t.id(), Some("E1"));
        assert_eq!(t.concepto(), "RECIBO LUZ · OCTUBRE");
        // Sin texto: el nombre de la otra parte; saldo como importe directo.
        let t = tx(serde_json::json!({
            "transaction_amount": { "currency": "EUR", "amount": "50" },
            "credit_debit_indicator": "CRDT",
            "status": "PDNG",
            "debtor": { "name": "CLIENTE DEMO SL" },
            "balance_after_transaction": { "currency": "EUR", "amount": "150.00" }
        }));
        assert!(!t.contabilizado());
        assert_eq!(t.centimos(), Some(5_000));
        assert_eq!(t.saldo("EUR"), Some(15_000));
        assert_eq!(t.id(), None);
        assert_eq!(t.concepto(), "CLIENTE DEMO SL");
        // Listas a null, como si no vinieran; una referencia vacía no es identificador.
        let t = tx(serde_json::json!({
            "entry_reference": "  ",
            "transaction_id": "T7",
            "transaction_amount": { "currency": "EUR", "amount": "1.00" },
            "credit_debit_indicator": "CRDT",
            "remittance_information": null
        }));
        assert_eq!(t.id(), None);
        assert!(t.remittance_information.is_empty());
    }

    #[test]
    fn errores_de_la_api() {
        assert!(matches!(
            error_api(
                401,
                r#"{"message":"Session expired","error":"EXPIRED_SESSION"}"#
            ),
            BancaError::Caducado
        ));
        match error_api(
            422,
            r#"{"message":"Wrong parameter","code":422,"error":"WRONG_REQUEST_PARAMETERS"}"#,
        ) {
            BancaError::Api {
                estado,
                codigo,
                mensaje,
            } => {
                assert_eq!(estado, 422);
                assert_eq!(codigo.as_deref(), Some("WRONG_REQUEST_PARAMETERS"));
                assert_eq!(mensaje, "Wrong parameter");
            }
            e => panic!("{e:?}"),
        }
        assert!(matches!(
            error_api(500, "no json"),
            BancaError::Api { estado: 500, .. }
        ));
        assert!(matches!(
            error_api(
                403,
                r#"{"message":"Access denied","error":"ACCESS_DENIED"}"#
            ),
            BancaError::Caducado
        ));
    }

    #[test]
    fn ids_seguros_en_la_ruta() {
        assert!(id_en_ruta("0a1b2c3d-4e5f-6789-abcd-ef0123456789").is_ok());
        assert!(id_en_ruta("../sessions").is_err());
        assert!(id_en_ruta("a/b").is_err());
        assert!(id_en_ruta("").is_err());
    }
}

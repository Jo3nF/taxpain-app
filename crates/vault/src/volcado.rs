//! Volcado de la BD a formatos abiertos (CSV y JSON por tabla, documentos tal cual), para no
//! depender de TaxPain: la exportación completa de una empresa y el rescate de lo que se pueda
//! de una que no abre.
//!
//! - Cada tabla va a `tablas/<tabla>.csv` (UTF-8, separada por comas, con cabecera) y a
//!   `tablas/<tabla>.json` (lista de objetos, con los valores tal cual).
//! - Las columnas binarias (los documentos: PDF de las facturas, recibidas, justificantes…) se
//!   escriben aparte, en `adjuntos/<tabla>/<fila>-<columna>.<ext>`, y la celda lleva esa ruta.
//! - No salen los secretos: ni las claves de los certificados, ni la de las copias, ni las de
//!   los servicios conectados (ver [`TABLAS_SECRETAS`] y [`COLUMNAS_SECRETAS`]).
//! - Una tabla que no se puede leer entera (BD dañada) deja lo que se haya leído y el error en
//!   el resumen: se sigue con las demás.

use std::path::Path;

use rusqlite::Connection;
use rusqlite::types::ValueRef;
use serde::Serialize;

use crate::error::{Result, VaultError};
use taxpain_secrets::{KeyId, KeyProvider};

/// Tablas que no salen nunca (las mismas que quita una copia de seguridad).
pub const TABLAS_SECRETAS: &[&str] = &[
    "certificate_secret",
    "certificate_ref",
    "backup_key",
    "app_secret",
];

/// Columnas que se dejan en blanco (sesiones de servicios conectados).
pub const COLUMNAS_SECRETAS: &[(&str, &str)] =
    &[("bank_link", "session_id"), ("bank_link", "state")];

/// Dónde se escribe el volcado (un ZIP, una carpeta…).
pub trait Destino {
    fn fichero(&mut self, ruta: &str, bytes: &[u8]) -> Result<()>;
}

/// Destino en una carpeta del disco.
#[derive(Debug)]
pub struct Carpeta<'a>(pub &'a Path);

impl Destino for Carpeta<'_> {
    fn fichero(&mut self, ruta: &str, bytes: &[u8]) -> Result<()> {
        let p = self.0.join(ruta);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, bytes)?;
        Ok(())
    }
}

/// Lo que ha salido de una tabla.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TablaVolcada {
    pub tabla: String,
    pub filas: usize,
    pub adjuntos: usize,
    /// No se ha podido leer entera: lo que ha fallado.
    pub error: Option<String>,
}

/// Resumen del volcado.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Volcado {
    pub tablas: Vec<TablaVolcada>,
    /// Versión del esquema de la BD (`user_version`).
    pub esquema: i64,
}

impl Volcado {
    pub fn filas(&self) -> usize {
        self.tablas.iter().map(|t| t.filas).sum()
    }
    pub fn con_error(&self) -> usize {
        self.tablas.iter().filter(|t| t.error.is_some()).count()
    }
}

/// Extensión de un documento por sus primeros bytes.
pub fn extension(bytes: &[u8]) -> &'static str {
    let head: &[u8] = bytes.get(..bytes.len().min(64)).unwrap_or(bytes);
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if bytes.starts_with(b"%PDF") {
        "pdf"
    } else if bytes.starts_with(b"PK\x03\x04") {
        "zip"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "jpg"
    } else if bytes.starts_with(b"\x89PNG") {
        "png"
    } else if t.starts_with("<?xml") || t.starts_with('<') {
        "xml"
    } else if t.starts_with('{') || t.starts_with('[') {
        "json"
    } else if std::str::from_utf8(bytes).is_ok() {
        "txt"
    } else {
        "bin"
    }
}

/// Celda de CSV: entre comillas si lleva coma, comillas o saltos de línea. Un texto que una
/// hoja de cálculo tomaría por fórmula (`=`, `+`, `@`, tabulador, o `-` seguido de algo que no
/// es un número) lleva delante un apóstrofo; los números no se tocan. El JSON lleva el valor
/// exacto.
pub fn celda_csv(s: &str) -> String {
    let formula = match s.as_bytes().first() {
        Some(b'=' | b'+' | b'@' | b'\t' | b'\r') => true,
        Some(b'-') => s.parse::<f64>().is_err(),
        _ => false,
    };
    let s = if formula {
        format!("'{s}")
    } else {
        s.to_owned()
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

fn tablas(conn: &Connection, schema: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT name FROM {schema}.sqlite_master WHERE type = 'table'
           AND name NOT LIKE 'sqlite_%' ORDER BY name"
    ))?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Vuelca todas las tablas de `schema` (`main`) de `conn` en `destino`.
pub fn volcar(conn: &Connection, destino: &mut dyn Destino) -> Result<Volcado> {
    let esquema: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(0);
    let mut out = Volcado {
        tablas: Vec::new(),
        esquema,
    };
    for tabla in tablas(conn, "main")? {
        if TABLAS_SECRETAS.contains(&tabla.as_str()) {
            continue;
        }
        out.tablas.push(volcar_tabla(conn, &tabla, destino)?);
    }
    Ok(out)
}

fn volcar_tabla(conn: &Connection, tabla: &str, destino: &mut dyn Destino) -> Result<TablaVolcada> {
    let mut csv = String::new();
    let mut json: Vec<serde_json::Value> = Vec::new();
    let mut adjuntos = 0usize;
    let mut error = None;
    let leido = (|| -> rusqlite::Result<()> {
        let mut stmt = conn
            .prepare(&format!("SELECT rowid AS _fila, * FROM {}", ident(tabla)))
            .or_else(|_| conn.prepare(&format!("SELECT NULL AS _fila, * FROM {}", ident(tabla))))?;
        let cols: Vec<String> = stmt.column_names().skip(1).map(str::to_owned).collect();
        csv.push_str(
            &cols
                .iter()
                .map(|c| celda_csv(c))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push('\n');
        let mut rows = stmt.query([])?;
        let mut n = 0usize;
        while let Some(r) = rows.next()? {
            n += 1;
            let fila: String = match r.get_ref(0)? {
                ValueRef::Integer(i) => i.to_string(),
                _ => n.to_string(),
            };
            let mut obj = serde_json::Map::new();
            let mut celdas = Vec::with_capacity(cols.len());
            for (i, c) in cols.iter().enumerate() {
                let secreta = COLUMNAS_SECRETAS.contains(&(tabla, c.as_str()));
                let (texto, valor) = match r.get_ref(i + 1)? {
                    _ if secreta => (String::new(), serde_json::Value::Null),
                    ValueRef::Null => (String::new(), serde_json::Value::Null),
                    ValueRef::Integer(x) => (x.to_string(), serde_json::json!(x)),
                    ValueRef::Real(x) => (x.to_string(), serde_json::json!(x)),
                    ValueRef::Text(t) => {
                        let t = String::from_utf8_lossy(t).into_owned();
                        (t.clone(), serde_json::Value::String(t))
                    }
                    ValueRef::Blob(b) => {
                        let ruta = format!("adjuntos/{tabla}/{fila}-{c}.{}", extension(b));
                        destino
                            .fichero(&ruta, b)
                            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                        adjuntos += 1;
                        (ruta.clone(), serde_json::json!({ "fichero": ruta }))
                    }
                };
                celdas.push(celda_csv(&texto));
                obj.insert(c.clone(), valor);
            }
            csv.push_str(&celdas.join(","));
            csv.push('\n');
            json.push(serde_json::Value::Object(obj));
        }
        Ok(())
    })();
    if let Err(e) = leido {
        tracing::warn!(tabla, error = %e, "no se pudo leer entera una tabla");
        error = Some(e.to_string());
    }
    let filas = json.len();
    destino.fichero(&format!("tablas/{tabla}.csv"), csv.as_bytes())?;
    let json =
        serde_json::to_vec_pretty(&json).map_err(|e| VaultError::InvalidInput(e.to_string()))?;
    destino.fichero(&format!("tablas/{tabla}.json"), &json)?;
    Ok(TablaVolcada {
        tabla: tabla.to_owned(),
        filas,
        adjuntos,
        error,
    })
}

/// Lo que se ha podido rescatar de una empresa que no abre.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Rescate {
    /// Ficheros originales copiados tal cual (cifrados) a `originales/`.
    pub originales: Vec<String>,
    /// Volcado de las tablas, si se ha podido abrir la BD con la clave del Llavero.
    pub volcado: Option<Volcado>,
    /// Por qué no se ha podido volcar.
    pub sin_volcado: Option<String>,
}

const FICHEROS_DATOS: &[&str] = &[
    "taxpain.db",
    "taxpain.db-wal",
    "taxpain.db-shm",
    "audit.head",
    "recovery.sealed",
];

/// Rescata lo que se pueda de la empresa de `dir` **sin tocarla**: copia sus ficheros tal cual
/// a `originales/` y, si el Llavero tiene la clave, abre una copia temporal (sin migrar, con la
/// conexión endurecida como la de una copia de seguridad) y vuelca sus tablas. La carpeta de la
/// empresa no se abre ni se escribe: se lee y se copia.
pub fn rescatar(dir: &Path, keys: &dyn KeyProvider, destino: &mut dyn Destino) -> Result<Rescate> {
    let mut originales = Vec::new();
    for f in FICHEROS_DATOS {
        let p = dir.join(f);
        if p.is_file() {
            destino.fichero(&format!("originales/{f}"), &std::fs::read(&p)?)?;
            originales.push((*f).to_owned());
        }
    }
    if !dir.join("taxpain.db").is_file() {
        return Ok(Rescate {
            originales,
            volcado: None,
            sin_volcado: Some("no está el fichero de la base de datos (taxpain.db)".into()),
        });
    }
    let key = match keys.get(KeyId::DbMaster, "Rescatar los datos de una empresa") {
        Ok(Some(k)) => k,
        Ok(None) => {
            return Ok(Rescate {
                originales,
                volcado: None,
                sin_volcado: Some(
                    "el Llavero de este Mac no tiene la clave que cifra los datos: sólo se han \
                     podido copiar los ficheros cifrados"
                        .into(),
                ),
            });
        }
        Err(e) => {
            return Ok(Rescate {
                originales,
                volcado: None,
                sin_volcado: Some(format!("el Llavero no ha dado la clave: {e}")),
            });
        }
    };
    // Una copia aparte: abrirla (y deshacer su WAL) no toca la de la empresa.
    let tmp = std::env::temp_dir().join(format!("taxpain-rescate-{}", hex::encode(aleatorio16())));
    std::fs::create_dir_all(&tmp)?;
    taxpain_secrets::keys::restrict_permissions(&tmp, 0o700)?;
    let volcado = (|| -> Result<Volcado> {
        for f in ["taxpain.db", "taxpain.db-wal"] {
            if dir.join(f).is_file() {
                std::fs::copy(dir.join(f), tmp.join(f))?;
            }
        }
        let conn = crate::db::open_unmigrated(&tmp.join("taxpain.db"), &key)?;
        crate::db::harden(&conn)?;
        volcar(&conn, destino)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(match volcado {
        Ok(v) => Rescate {
            originales,
            volcado: Some(v),
            sin_volcado: None,
        },
        Err(e) => Rescate {
            originales,
            volcado: None,
            sin_volcado: Some(format!("no se ha podido leer la base de datos: {e}")),
        },
    })
}

/// 16 bytes aleatorios para el nombre de la carpeta temporal.
fn aleatorio16() -> [u8; 16] {
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err() {
        // Sin aleatorio, la hora: el nombre sólo ha de no chocar con otro rescate a la vez.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        b = t.to_le_bytes();
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AlwaysConfirm, MemoryKeyProvider, Vault};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[derive(Default)]
    struct Memoria(BTreeMap<String, Vec<u8>>);

    impl Destino for Memoria {
        fn fichero(&mut self, ruta: &str, bytes: &[u8]) -> Result<()> {
            self.0.insert(ruta.to_owned(), bytes.to_vec());
            Ok(())
        }
    }

    #[test]
    fn la_celda_csv_se_escapa_y_no_es_formula() {
        assert_eq!(celda_csv("hola"), "hola");
        assert_eq!(celda_csv("a,b"), "\"a,b\"");
        assert_eq!(celda_csv("di \"sí\""), "\"di \"\"sí\"\"\"");
        assert_eq!(celda_csv("=SUMA(A1)"), "'=SUMA(A1)");
        assert_eq!(celda_csv("-125.50"), "-125.50");
        assert_eq!(celda_csv("-cmd"), "'-cmd");
    }

    #[test]
    fn la_extension_sale_de_los_primeros_bytes() {
        assert_eq!(extension(b"%PDF-1.7"), "pdf");
        assert_eq!(extension(b"<?xml version"), "xml");
        assert_eq!(extension(b"PK\x03\x04..."), "zip");
        assert_eq!(extension(&[0, 159, 146, 150]), "bin");
    }

    fn empresa(dir: &Path, keys: Arc<MemoryKeyProvider>) {
        let v = Vault::open(dir, keys, Arc::new(AlwaysConfirm)).unwrap();
        let c = v.connection();
        c.execute(
            "INSERT INTO document(uuid, kind, filename, mime, sha256, size, content, created_at)
             VALUES ('u1', 'factura_pdf', 'F-1.pdf', 'application/pdf', 'x', 9, ?1, '2026-01-01')",
            [b"%PDF-1.7\n".to_vec()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO app_secret(name, value, updated_at) VALUES ('clave', 'secreto', 'x')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO settings(key, value) VALUES ('nota', '\"=1+1, con coma\"')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn el_volcado_saca_tablas_y_documentos_sin_secretos() {
        let tmp = tempfile::tempdir().unwrap();
        let keys = Arc::new(MemoryKeyProvider::default());
        empresa(tmp.path(), keys.clone());
        let v = Vault::open(tmp.path(), keys, Arc::new(AlwaysConfirm)).unwrap();
        let mut m = Memoria::default();
        let r = volcar(v.connection(), &mut m).unwrap();
        assert_eq!(r.esquema, crate::db::schema_version() as i64);
        assert_eq!(r.con_error(), 0);
        assert!(m.0.contains_key("tablas/document.csv"));
        assert!(m.0.contains_key("tablas/journal_entry.json"));
        assert!(
            !m.0.keys().any(|k| k.contains("app_secret")),
            "sin secretos"
        );
        assert!(!m.0.keys().any(|k| k.contains("certificate_secret")));
        let pdf =
            m.0.keys()
                .find(|k| k.starts_with("adjuntos/document/"))
                .unwrap();
        assert!(pdf.ends_with("-content.pdf"));
        assert_eq!(m.0[pdf], b"%PDF-1.7\n");
        let csv = String::from_utf8(m.0["tablas/document.csv"].clone()).unwrap();
        assert!(
            csv.contains(pdf.as_str()),
            "la celda lleva la ruta del documento"
        );
        let ajustes = String::from_utf8(m.0["tablas/settings.csv"].clone()).unwrap();
        assert!(ajustes.contains("\"\"\"=1+1, con coma\"\"\""));
        let json: serde_json::Value = serde_json::from_slice(&m.0["tablas/settings.json"]).unwrap();
        assert!(json.as_array().unwrap().iter().any(|o| o["key"] == "nota"));
    }

    #[test]
    fn el_rescate_copia_los_originales_y_vuelca_sin_tocar_la_empresa() {
        let tmp = tempfile::tempdir().unwrap();
        let keys = Arc::new(MemoryKeyProvider::default());
        empresa(tmp.path(), keys.clone());
        let antes = std::fs::read(tmp.path().join("taxpain.db")).unwrap();
        let mut m = Memoria::default();
        let r = rescatar(tmp.path(), keys.as_ref(), &mut m).unwrap();
        assert!(r.originales.contains(&"taxpain.db".to_owned()));
        assert_eq!(m.0["originales/taxpain.db"], antes);
        assert!(r.volcado.unwrap().filas() > 0);
        assert_eq!(std::fs::read(tmp.path().join("taxpain.db")).unwrap(), antes);

        // Sin la clave: sólo los ficheros cifrados.
        let mut m = Memoria::default();
        let r = rescatar(tmp.path(), &MemoryKeyProvider::default(), &mut m).unwrap();
        assert!(r.volcado.is_none());
        assert!(r.sin_volcado.unwrap().contains("Llavero"));
        assert!(m.0.contains_key("originales/taxpain.db"));
    }

    #[test]
    fn el_rescate_de_una_bd_destrozada_lo_dice_y_guarda_los_originales() {
        let tmp = tempfile::tempdir().unwrap();
        let keys = Arc::new(MemoryKeyProvider::default());
        empresa(tmp.path(), keys.clone());
        std::fs::write(
            tmp.path().join("taxpain.db"),
            b"esto no es una base de datos",
        )
        .unwrap();
        let _ = std::fs::remove_file(tmp.path().join("taxpain.db-wal"));
        let mut m = Memoria::default();
        let r = rescatar(tmp.path(), keys.as_ref(), &mut m).unwrap();
        assert!(r.volcado.is_none());
        assert!(r.sin_volcado.unwrap().contains("no se ha podido leer"));
        assert_eq!(
            m.0["originales/taxpain.db"],
            b"esto no es una base de datos"
        );
    }
}

//! Base de datos SQLCipher (D3): apertura con clave bruta de 256 bits y migraciones.

use std::path::Path;

use rusqlite::Connection;

use crate::error::{Result, VaultError};
use taxpain_secrets::Key256;

/// Migraciones en orden; la posición + 1 es el `user_version` resultante.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_settings_reminders.sql"),
    include_str!("../migrations/0003_invoicing.sql"),
    include_str!("../migrations/0004_received.sql"),
    include_str!("../migrations/0005_accounting.sql"),
    include_str!("../migrations/0006_tax_returns.sql"),
    include_str!("../migrations/0007_tax_return_receipts.sql"),
    include_str!("../migrations/0008_received_nature.sql"),
    include_str!("../migrations/0009_aeat_notices.sql"),
    include_str!("../migrations/0010_app_secrets.sql"),
    include_str!("../migrations/0011_verifactu_resubmission.sql"),
    include_str!("../migrations/0012_received_self_assessed.sql"),
    include_str!("../migrations/0013_invoice_ubl.sql"),
    include_str!("../migrations/0014_entry_templates.sql"),
    include_str!("../migrations/0015_cartera.sql"),
    include_str!("../migrations/0016_ledger_ticks.sql"),
    include_str!("../migrations/0017_party_terms.sql"),
    include_str!("../migrations/0018_remittances.sql"),
    include_str!("../migrations/0019_import_batch.sql"),
    include_str!("../migrations/0020_analytics_budgets.sql"),
    include_str!("../migrations/0021_fiscal_year_dates.sql"),
    include_str!("../migrations/0022_cash_basis.sql"),
    include_str!("../migrations/0023_tax_payment.sql"),
    include_str!("../migrations/0024_bank_rules.sql"),
    include_str!("../migrations/0025_payroll.sql"),
    include_str!("../migrations/0026_party_counterpart.sql"),
    include_str!("../migrations/0027_entry_notes.sql"),
    include_str!("../migrations/0028_asset_disposal.sql"),
    include_str!("../migrations/0029_model_adjustment.sql"),
    include_str!("../migrations/0030_import_dua.sql"),
    include_str!("../migrations/0031_leasing.sql"),
    include_str!("../migrations/0032_prorrata.sql"),
    include_str!("../migrations/0033_depreciation_plan.sql"),
    include_str!("../migrations/0034_analytics_levels.sql"),
    include_str!("../migrations/0035_partners.sql"),
    include_str!("../migrations/0036_folder_import.sql"),
    include_str!("../migrations/0037_company_documents.sql"),
    include_str!("../migrations/0038_ai_reading.sql"),
    include_str!("../migrations/0039_backup_key.sql"),
    include_str!("../migrations/0040_book_import.sql"),
    include_str!("../migrations/0041_quotes.sql"),
    include_str!("../migrations/0042_socios_administradores_viajes.sql"),
    include_str!("../migrations/0043_foreign_currency.sql"),
    include_str!("../migrations/0044_bank_currency.sql"),
    include_str!("../migrations/0046_irpf_regime.sql"),
    include_str!("../migrations/0049_saldos_iniciales.sql"),
    include_str!("../migrations/0050_gestor_import.sql"),
    include_str!("../migrations/0051_cripto.sql"),
    include_str!("../migrations/0052_dividend_allocation.sql"),
    include_str!("../migrations/0053_settlement_method.sql"),
    include_str!("../migrations/0054_party_terms_key_190.sql"),
    include_str!("../migrations/0055_aeat_filing.sql"),
    include_str!("../migrations/0056_aeat_filing_boxes.sql"),
    include_str!("../migrations/0057_journal_line_entry.sql"),
    include_str!("../migrations/0058_bank_link.sql"),
    include_str!("../migrations/0059_bank_tx_ignored.sql"),
    include_str!("../migrations/0060_venta_inmovilizado.sql"),
    include_str!("../migrations/0061_assistant_conversations.sql"),
    include_str!("../migrations/0062_cripto_inicio.sql"),
    include_str!("../migrations/0063_valores.sql"),
    include_str!("../migrations/0064_valores_derivados.sql"),
    include_str!("../migrations/0065_audit_hash_version.sql"),
    include_str!("../migrations/0066_verifactu_pruebas.sql"),
    include_str!("../migrations/0067_prestamos.sql"),
    include_str!("../migrations/0068_libros_societarios.sql"),
    include_str!("../migrations/0069_vinculadas.sql"),
    include_str!("../migrations/0070_autonomo_societario.sql"),
    include_str!("../migrations/0073_entidad_patrimonial.sql"),
    include_str!("../migrations/0076_ajustes_cierre.sql"),
    include_str!("../migrations/0077_modelo200_completo.sql"),
    include_str!("../migrations/0078_moneda_extranjera.sql"),
    include_str!("../migrations/0082_acuerdos_dividendo.sql"),
    include_str!("../migrations/0083_rectificativas_incobrables.sql"),
    include_str!("../migrations/0084_trimestre_puesta_marcha.sql"),
    include_str!("../migrations/0085_expedientes.sql"),
    include_str!("../migrations/0087_bienes_extranjero.sql"),
    include_str!("../migrations/0089_iva_holding.sql"),
    include_str!("../migrations/0090_periodos_archivo.sql"),
    include_str!("../migrations/0091_extractos_incentivos.sql"),
    include_str!("../migrations/0092_no_residentes_inmuebles.sql"),
    include_str!("../migrations/0093_socio_subvenciones.sql"),
    include_str!("../migrations/0094_oss.sql"),
    include_str!("../migrations/0096_verifactu_cola.sql"),
    include_str!("../migrations/0097_nominas_calculo.sql"),
];

/// Marca de las migraciones que rehacen una tabla a la que apuntan claves foráneas de otras
/// (SQLite no deja quitar una restricción sin rehacerla): se aplican con las claves foráneas
/// desactivadas, como pide SQLite, y se comprueban antes de confirmar la migración.
const SIN_CLAVES_FORANEAS: &str = "-- taxpain:sin-claves-foraneas";

fn sin_claves_foraneas(sql: &str) -> bool {
    sql.starts_with(SIN_CLAVES_FORANEAS)
}

/// Abre (o crea) la BD cifrada y aplica las migraciones pendientes.
pub fn open(path: &Path, key: &Key256) -> Result<Connection> {
    let conn = keyed(|| Connection::open(path), key)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Abre con clave sin migrar: destino de una copia que se restaura (el esquema viene en ella).
pub(crate) fn open_unmigrated(path: &Path, key: &Key256) -> Result<Connection> {
    let conn = keyed(|| Connection::open(path), key)?;
    configure(&conn)?;
    Ok(conn)
}

/// Abre una BD que ya existe, sin crearla ni migrarla: para quien sólo consulta o deja borradores
/// (el servidor MCP) y no debe cambiar el esquema por su cuenta. Si el fichero no existe,
/// [`VaultError::NotFound`]; si su esquema no es el de esta compilación, un error que dice qué
/// hay que actualizar. No cambia el modo del diario (ya es WAL desde que se creó).
pub(crate) fn open_existing(path: &Path, key: &Key256) -> Result<Connection> {
    use rusqlite::OpenFlags;
    if !path.is_file() {
        return Err(VaultError::NotFound);
    }
    // Sin `SQLITE_OPEN_CREATE`: si el fichero desaparece entre la comprobación y la apertura,
    // SQLite falla en vez de crear una BD vacía.
    let flags = OpenFlags::default().difference(OpenFlags::SQLITE_OPEN_CREATE);
    let conn = keyed(|| Connection::open_with_flags(path, flags), key)?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA secure_delete = ON;
         PRAGMA busy_timeout = 10000;
         PRAGMA trusted_schema = OFF;",
    )?;
    let version = user_version(&conn)?;
    let expected = MIGRATIONS.len();
    if version > expected {
        return Err(VaultError::InvalidInput(format!(
            "la base de datos es de una versión más nueva de TaxPain (esquema {version}; este \
             programa entiende el {expected}): actualiza este programa"
        )));
    }
    if version < expected {
        return Err(VaultError::InvalidInput(format!(
            "la base de datos es de una versión anterior de TaxPain (esquema {version}; este \
             programa necesita el {expected}): abre antes la app TaxPain para que la actualice"
        )));
    }
    Ok(conn)
}

/// Endurece una conexión que va a leer una BD que no es de fiar (la de una copia de seguridad,
/// que puede venir de otra persona): el esquema no puede llamar a funciones ni tablas virtuales
/// que no sean inocuas (`trusted_schema`), las celdas se comprueban al leerlas
/// (`cell_size_check`) y no se puede escribir en el esquema ni en las tablas internas por SQL
/// (`SQLITE_DBCONFIG_DEFENSIVE`). Hay que llamarla antes de adjuntar la BD.
pub(crate) fn harden(conn: &Connection) -> Result<()> {
    use rusqlite::config::DbConfig;
    conn.execute_batch(
        "PRAGMA trusted_schema = OFF;
         PRAGMA cell_size_check = ON;",
    )?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)?;
    Ok(())
}

/// Un objeto del esquema (`sqlite_master`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SchemaObject {
    pub(crate) kind: String,
    pub(crate) name: String,
    pub(crate) table: String,
    /// Página raíz: 0 en vistas, triggers y tablas virtuales.
    pub(crate) rootpage: i64,
    /// `None` en los índices automáticos de las restricciones `UNIQUE` y `PRIMARY KEY`.
    pub(crate) sql: Option<String>,
}

/// Objetos del esquema `schema` de `conn` (`main`, o el nombre con el que se adjuntó la BD).
pub(crate) fn schema_objects(conn: &Connection, schema: &str) -> Result<Vec<SchemaObject>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT type, name, tbl_name, coalesce(rootpage, 0), sql
         FROM {schema}.sqlite_master ORDER BY type, name"
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok(SchemaObject {
            kind: r.get(0)?,
            name: r.get(1)?,
            table: r.get(2)?,
            rootpage: r.get(3)?,
            sql: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// El esquema que dejan las migraciones de esta compilación hasta la versión `version`, en una
/// BD nueva en memoria: con él se compara el de una copia que se restaura.
pub(crate) fn clean_schema(version: usize) -> Result<Vec<SchemaObject>> {
    if version > MIGRATIONS.len() {
        return Err(VaultError::InvalidInput(
            "la base de datos es de una versión más nueva de TaxPain".into(),
        ));
    }
    let conn = keyed(Connection::open_in_memory, &Key256::random()?)?;
    configure(&conn)?;
    migrate_to(&conn, version)?;
    schema_objects(&conn, "main")
}

/// BD cifrada en memoria, vacía y sin migrar (para volcar en ella una copia y consultarla).
pub(crate) fn open_in_memory_unmigrated(key: &Key256) -> Result<Connection> {
    let conn = keyed(Connection::open_in_memory, key)?;
    configure(&conn)?;
    Ok(conn)
}

/// Recifra la BD de `path` de la clave `from` a la clave `to` (`PRAGMA rekey`): al recuperar un
/// almacén en un Mac cuyo Llavero ya tiene otra `db-master`. Se vacía el WAL al terminar para
/// que no quede ninguna página con la clave antigua.
pub(crate) fn rekey(path: &Path, from: &Key256, to: &Key256) -> Result<()> {
    let conn = keyed(|| Connection::open(path), from)?;
    let hex = to.to_hex();
    let pragma = zeroize::Zeroizing::new(format!("PRAGMA rekey = \"x'{}'\";", hex.as_str()));
    conn.execute_batch(&pragma)?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

/// Versión del esquema que entiende esta compilación (número de migraciones).
pub fn schema_version() -> usize {
    MIGRATIONS.len()
}

/// BD cifrada en memoria (tests).
pub fn open_in_memory(key: &Key256) -> Result<Connection> {
    let conn = keyed(Connection::open_in_memory, key)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Serializa la apertura con clave. En el CI ha fallado dos veces «PRAGMA key requires a key of
/// one or more characters» al abrir y cerrar varias BD en paralelo (tests; en la app, los avisos
/// y la carpeta vigilada recorren varias empresas a la vez). SQLCipher activa su proveedor
/// criptográfico con la primera conexión cifrada y lo desactiva al cerrarse la última: abrir
/// mientras otro hilo cierra la última puede pillarlo a medias. Por eso, además del cerrojo, se
/// mantiene abierta una conexión cifrada en memoria durante todo el proceso ([`KEEPALIVE`]),
/// de modo que el proveedor no se desactive nunca. Abrir el almacén es raro: el coste es nulo.
/// `TODO(VERIFY)`: la causa es una hipótesis; no se ha reproducido en local (3.600 aperturas y
/// cierres en 8 hilos sin fallo, con y sin este cambio).
static KEYING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Conexión cifrada que vive lo que el proceso (ver [`KEYING`]).
static KEEPALIVE: std::sync::OnceLock<std::sync::Mutex<Connection>> = std::sync::OnceLock::new();

fn keyed(open: impl FnOnce() -> rusqlite::Result<Connection>, key: &Key256) -> Result<Connection> {
    let _guard = KEYING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if KEEPALIVE.get().is_none() {
        let keep = Connection::open_in_memory()?;
        apply_key(&keep, &Key256::random()?)?;
        let _ = KEEPALIVE.set(std::sync::Mutex::new(keep));
    }
    let conn = open()?;
    apply_key(&conn, key)?;
    Ok(conn)
}

/// `PRAGMA key` con la clave bruta; llamar con [`KEYING`] tomado.
fn apply_key(conn: &Connection, key: &Key256) -> Result<()> {
    // Clave bruta: x'<64 hex>' evita el KDF de contraseña (la clave ya es aleatoria).
    let hex = key.to_hex();
    let pragma = zeroize::Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", hex.as_str()));
    conn.execute_batch(&pragma)?;
    // Con una clave equivocada SQLCipher falla en la primera lectura real.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })
    .map_err(|_| VaultError::Corrupted)?;
    Ok(())
}

fn configure(conn: &Connection) -> Result<()> {
    // `trusted_schema = OFF`: los triggers, vistas, CHECK, DEFAULT e índices del esquema no
    // pueden llamar a funciones de la aplicación ni a tablas virtuales que no sean inocuas. El
    // esquema de TaxPain no usa ninguna; el de una copia restaurada de otra persona tampoco podrá.
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA secure_delete = ON;
         PRAGMA busy_timeout = 10000;
         PRAGMA trusted_schema = OFF;",
    )?;
    Ok(())
}

pub(crate) fn user_version(conn: &Connection) -> Result<usize> {
    let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    usize::try_from(v).map_err(|_| VaultError::Corrupted)
}

/// `ALTER TABLE t ADD COLUMN c …;` de una migración cuya columna ya existe: se quita, para que
/// la migración se pueda repetir sin «duplicate column name».
fn skip_existing_columns(conn: &Connection, sql: &str) -> Result<String> {
    let mut out = String::with_capacity(sql.len());
    let mut rest = sql;
    while let Some(at) = rest.find("ALTER TABLE ") {
        let end = rest[at..].find(';').map_or(rest.len(), |e| at + e + 1);
        let stmt = &rest[at..end];
        let words: Vec<&str> = stmt.split_whitespace().collect();
        let exists = match words.as_slice() {
            ["ALTER", "TABLE", table, "ADD", "COLUMN", column, ..] => {
                let mut q = conn.prepare(&format!("PRAGMA table_info({table})"))?;
                let names: Vec<String> = q
                    .query_map([], |r| r.get::<_, String>(1))?
                    .collect::<std::result::Result<_, _>>()?;
                names.iter().any(|n| n == column)
            }
            _ => false,
        };
        out.push_str(&rest[..at]);
        if !exists {
            out.push_str(stmt);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    Ok(out)
}

fn migrate(conn: &Connection) -> Result<()> {
    migrate_to(conn, MIGRATIONS.len())
}

/// Aplica las migraciones pendientes hasta dejar la BD en la versión `target` (la última, salvo
/// para reconstruir el esquema de una versión anterior: ver [`clean_schema`]).
fn migrate_to(conn: &Connection, target: usize) -> Result<()> {
    if user_version(conn)? > MIGRATIONS.len() {
        return Err(VaultError::InvalidInput(
            "la base de datos es de una versión más nueva de TaxPain".into(),
        ));
    }
    let pending = MIGRATIONS.get(..target).unwrap_or(MIGRATIONS);
    loop {
        // Las claves foráneas sólo se pueden desactivar fuera de una transacción: se mira antes
        // qué migración toca y, dentro, que siga siendo esa.
        let sin_fk = pending
            .get(user_version(conn)?)
            .is_some_and(|sql| sin_claves_foraneas(sql));
        if sin_fk {
            conn.pragma_update(None, "foreign_keys", false)?;
        }
        let paso = migrate_one(conn, sin_fk, pending);
        if sin_fk {
            conn.pragma_update(None, "foreign_keys", true)?;
        }
        if paso? {
            return Ok(());
        }
    }
}

/// Aplica la migración de `pending` que toca, en su transacción. `true` si ya no queda ninguna.
fn migrate_one(conn: &Connection, sin_fk: bool, pending: &[&str]) -> Result<bool> {
    // BEGIN IMMEDIATE: si otra conexión (otra ventana, otro hilo) está migrando el mismo
    // fichero, se espera a que acabe y se vuelve a mirar la versión, en vez de aplicar dos
    // veces la misma migración.
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let done = (|| -> Result<bool> {
        let current = user_version(conn)?;
        let Some(sql) = pending.get(current) else {
            return Ok(true);
        };
        // Otra conexión ha migrado mientras tanto y ahora toca otra: se vuelve a mirar.
        if sin_claves_foraneas(sql) != sin_fk {
            return Ok(false);
        }
        conn.execute_batch(&skip_existing_columns(conn, sql)?)?;
        if sin_fk {
            let rotas: i64 =
                conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                    r.get(0)
                })?;
            if rotas > 0 {
                return Err(VaultError::InvalidInput(format!(
                    "la migración {} deja {rotas} referencias rotas",
                    current + 1
                )));
            }
        }
        conn.pragma_update(
            None,
            "user_version",
            i64::try_from(current + 1).unwrap_or(i64::MAX),
        )?;
        tracing::info!(version = current + 1, "migración aplicada");
        Ok(false)
    })();
    match done {
        Ok(finished) => {
            conn.execute_batch("COMMIT")?;
            Ok(finished)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// BD cifrada en `path` con el esquema de la versión `version` (tests: almacenes y copias de
/// versiones anteriores).
#[cfg(test)]
pub(crate) fn open_at_version(path: &Path, key: &Key256, version: usize) -> Result<Connection> {
    let conn = keyed(|| Connection::open(path), key)?;
    configure(&conn)?;
    migrate_to(&conn, version)?;
    Ok(conn)
}

/// Como [`open_at_version`], en memoria.
#[cfg(test)]
pub(crate) fn open_in_memory_at_version(key: &Key256, version: usize) -> Result<Connection> {
    let conn = keyed(Connection::open_in_memory, key)?;
    configure(&conn)?;
    migrate_to(&conn, version)?;
    Ok(conn)
}

/// Aplica las migraciones que falten (tests: lo que hace la app al abrir un almacén anterior).
#[cfg(test)]
pub(crate) fn migrate_pending(conn: &Connection) -> Result<()> {
    migrate(conn)
}

/// Versión de SQLCipher enlazada (vacía si SQLCipher no está activo).
pub fn cipher_version(conn: &Connection) -> Result<String> {
    Ok(conn
        .query_row("PRAGMA cipher_version", [], |r| r.get(0))
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dos_aperturas_a_la_vez_no_migran_dos_veces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let key = Key256::random().unwrap();
        let (a, b) = std::thread::scope(|s| {
            let h1 = s.spawn(|| open(&path, &key).map(|_| ()));
            let h2 = s.spawn(|| open(&path, &key).map(|_| ()));
            (h1.join().unwrap(), h2.join().unwrap())
        });
        assert!(a.is_ok() && b.is_ok(), "{a:?} {b:?}");
        let conn = open(&path, &key).unwrap();
        assert_eq!(user_version(&conn).unwrap(), MIGRATIONS.len());
    }

    #[test]
    fn una_migracion_a_medias_se_puede_repetir() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (a INTEGER); ALTER TABLE t ADD COLUMN b TEXT;")
            .unwrap();
        let sql = "-- comentario\nALTER TABLE t ADD COLUMN b TEXT NOT NULL DEFAULT 'x'\n  CHECK (b IN ('x','y'));\nALTER TABLE t ADD COLUMN c INTEGER;\nCREATE TABLE u (id INTEGER);";
        let fixed = skip_existing_columns(&conn, sql).unwrap();
        assert!(!fixed.contains("COLUMN b"), "{fixed}");
        conn.execute_batch(&fixed).unwrap();
    }

    #[test]
    fn file_is_encrypted_and_needs_the_right_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let key = Key256::random().unwrap();
        {
            let conn = open(&path, &key).unwrap();
            assert!(
                !cipher_version(&conn).unwrap().is_empty(),
                "SQLCipher no está enlazado"
            );
            conn.execute(
                "INSERT INTO settings(key, value) VALUES ('marca', 'texto-en-claro-XYZ')",
                [],
            )
            .unwrap();
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .unwrap();
        }
        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.starts_with(b"SQLite format 3"));
        assert!(!raw.windows(18).any(|w| w == b"texto-en-claro-XYZ"));

        let wrong = Key256::random().unwrap();
        assert!(matches!(open(&path, &wrong), Err(VaultError::Corrupted)));

        let conn = open(&path, &key).unwrap();
        let v: String = conn
            .query_row("SELECT value FROM settings WHERE key='marca'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(v, "texto-en-claro-XYZ");
        let ver: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, MIGRATIONS.len() as i64);
    }

    /// 0044: la tabla de cuentas se rehace (clave única por número y divisa) sin perder sus
    /// `id` ni los movimientos que apuntan a ellas, y las claves foráneas siguen activas.
    #[test]
    fn migration_0044_keeps_accounts_and_movements() {
        let conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        for sql in &MIGRATIONS[..43] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 43).unwrap();
        conn.execute_batch(
            "INSERT INTO bank_account(id, iban, bank_code, number, account, name)
               VALUES (7, NULL, '2100', '210004180200051332', '5720001', 'EMPRESA DEMO SL (1332)');
             INSERT INTO bank_tx(id, bank_account, op_date, value_date, amount, description,
               import_hash, created_at)
               VALUES (30, 7, '2026-01-05', '2026-01-05', -12100, 'RECIBO', 'h1', 'x');
             INSERT INTO tax_payment(model, year, period, date, account, treasury, amount,
               bank_tx, created_at)
               VALUES ('303', 2025, '4T', '2026-01-20', '4750', '5720001', 500, 30, 'x');",
        )
        .unwrap();
        migrate(&conn).unwrap();
        assert_eq!(user_version(&conn).unwrap(), MIGRATIONS.len());
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
        let row: (i64, String, String, String) = conn
            .query_row(
                "SELECT id, number, currency, kind FROM bank_account",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                7,
                "210004180200051332".into(),
                "EUR".into(),
                "cuenta".into()
            )
        );
        let (account, balance): (i64, Option<i64>) = conn
            .query_row(
                "SELECT bank_account, balance FROM bank_tx WHERE id = 30",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((account, balance), (7, None));
        // La misma cuenta en otra divisa es otra cuenta; en la misma, no.
        conn.execute(
            "INSERT INTO bank_account(number, account, name, currency)
             VALUES ('210004180200051332', '5720002', 'USD', 'USD')",
            [],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO bank_account(number, account, name) VALUES ('210004180200051332', '5720003', 'x')",
                [],
            )
            .is_err()
        );
        // La clave foránea de los movimientos apunta a la tabla nueva y se sigue comprobando.
        assert!(
            conn.execute(
                "INSERT INTO bank_tx(bank_account, op_date, value_date, amount, description,
                   import_hash, created_at) VALUES (99, '2026-01-06', '2026-01-06', 1, 'x', 'h2', 'x')",
                [],
            )
            .is_err()
        );
        let rotas: i64 = conn
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(rotas, 0);
    }

    /// 0017: los vencimientos fijados a mano pasan a `invoice_installment` con el importe de la
    /// factura y los cobros registrados quedan como del vencimiento 1.
    #[test]
    fn migration_0017_keeps_due_dates_and_settlements() {
        let conn = Connection::open_in_memory().unwrap();
        // Filas sueltas, sin serie ni documento: sin claves foráneas para prepararlas.
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        for sql in &MIGRATIONS[..16] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO invoice_issued(id, series_id, year, seq, number, issue_date, kind, base, vat,
               surcharge, withholding, total, payable, status, data, created_at)
             VALUES (1, 1, 2026, 1, 'A1', '2026-09-01', 'F1', 1000, 210, 0, 0, 1210, 1210, 'emitida', '{}', 'x');
             INSERT INTO invoice_received(id, document_id, file_sha256, filename, source, status,
               extraction, base, vat, withholding, total, issue_date, created_at)
             VALUES (2, 1, 'h', 'f', 'manual', 'revisada', '{}', 200, 42, 0, 242, '2026-09-01', 'x');
             INSERT INTO invoice_due VALUES ('emitida', 1, '2026-12-31'), ('recibida', 2, '2026-10-10'),
               ('emitida', 99, '2026-10-10');
             INSERT INTO settlement(id, kind, invoice_id, date, account, created_at)
               VALUES (7, 'recibida', 2, '2026-10-01', '570', 'x');",
        )
        .unwrap();
        conn.execute_batch(MIGRATIONS[16]).unwrap();
        let rows: Vec<(String, i64, i64, String, i64)> = conn
            .prepare("SELECT kind, invoice_id, seq, due_date, amount FROM invoice_installment ORDER BY kind")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("emitida".into(), 1, 1, "2026-12-31".into(), 1210),
                ("recibida".into(), 2, 1, "2026-10-10".into(), 242)
            ]
        );
        let s: (i64, i64) = conn
            .query_row("SELECT id, seq FROM settlement", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(s, (7, 1));
    }
    /// 0046: los ejercicios con algún 130 presentado (desde TaxPain en producción o por el
    /// gestor) quedan apuntados para no pasarlos a simplificada en silencio.
    #[test]
    fn migration_0046_lists_years_with_130_filed() {
        let conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        let before = MIGRATIONS
            .iter()
            .position(|m| m.contains("models.irpf_regime_pending"))
            .unwrap();
        for sql in &MIGRATIONS[..before] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", before as i64)
            .unwrap();
        conn.execute_batch(
            "INSERT INTO tax_return(model, year, period, environment, declaration, result, boxes,
               file_sha256, file, csv, receipt_number, receipt, presented_at)
               VALUES ('130', 2025, '4T', 'produccion', 'I', 100, '[]', 'x', 'x', 'x', 'x', '{}', 'x'),
                      ('130', 2024, '1T', 'pruebas', 'I', 100, '[]', 'x', 'x', 'x', 'x', '{}', 'x'),
                      ('303', 2023, '1T', 'produccion', 'I', 100, '[]', 'x', 'x', 'x', 'x', '{}', 'x');
             INSERT INTO filed_model(model, year, period, filename, file_sha256, source, boxes,
               imported_at)
               VALUES ('130', 2026, '1T', 'a.txt', 'h1', 'manual', '{}', 'x'),
                      ('130', 2025, '1T', 'b.txt', 'h2', 'manual', '{}', 'x');",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let v: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'models.irpf_regime_pending'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, "[2025,2026]");
        // En una base nueva, la lista está vacía.
        let fresh = Connection::open_in_memory().unwrap();
        configure(&fresh).unwrap();
        migrate(&fresh).unwrap();
        let v: String = fresh
            .query_row(
                "SELECT value FROM settings WHERE key = 'models.irpf_regime_pending'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, "[]");
    }

    /// 0061: una conversación por tema (las generales, todas las que se quieran), un documento
    /// siempre con su nombre y su huella, y al borrar una conversación se borran sus mensajes y
    /// lo añadido.
    #[test]
    fn migracion_conversaciones_borra_en_cascada_y_un_tema_no_se_repite() {
        let conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        migrate(&conn).unwrap();
        let nueva = |kind: &str, key: &str| {
            conn.execute(
                "INSERT INTO assistant_conversation(kind, topic_key, topic, title, created_at,
                   updated_at) VALUES (?1, ?2, '{}', 'Título', 'x', 'x')",
                [kind, key],
            )
        };
        nueva("ahorro", "capitalizacion@2026").unwrap();
        assert!(
            nueva("ahorro", "capitalizacion@2026").is_err(),
            "tema repetido"
        );
        nueva("ahorro", "capitalizacion@2027").unwrap();
        nueva("general", "").unwrap();
        nueva("general", "").unwrap();
        let id: i64 = conn
            .query_row(
                "SELECT id FROM assistant_conversation WHERE topic_key = 'capitalizacion@2026'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO assistant_message(conversation_id, seq, role, content, created_at)
             VALUES (?1, 1, 'user', '¿Me conviene?', 'x')",
            [id],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO assistant_message(conversation_id, seq, role, content, created_at)
                 VALUES (?1, 2, 'system', 'x', 'x')",
                [id],
            )
            .is_err(),
            "sólo usuario o asistente"
        );
        conn.execute(
            "INSERT INTO assistant_material(conversation_id, kind, content, created_at)
             VALUES (?1, 'dato', 'Tenemos 3 trabajadores.', 'x')",
            [id],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO assistant_material(conversation_id, kind, content, created_at)
                 VALUES (?1, 'documento', 'Texto', 'x')",
                [id],
            )
            .is_err(),
            "un documento sin nombre ni huella"
        );
        conn.execute(
            "INSERT INTO assistant_material(conversation_id, kind, content, filename,
               file_sha256, file_size, read_by, created_at)
             VALUES (?1, 'documento', 'Texto', 'certificado.pdf', 'abc', 10, 'mac', 'x')",
            [id],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO assistant_material(conversation_id, kind, content, read_by,
                   created_at) VALUES (?1, 'dato', 'x', 'mac', 'x')",
                [id],
            )
            .is_err(),
            "un dato no se lee de ningún fichero"
        );
        assert!(
            conn.execute(
                "INSERT INTO assistant_message(conversation_id, seq, role, content, created_at)
                 VALUES (999, 1, 'user', 'x', 'x')",
                [],
            )
            .is_err(),
            "de una conversación que no existe"
        );
        conn.execute("DELETE FROM assistant_conversation WHERE id = ?1", [id])
            .unwrap();
        let quedan = |t: &str| -> i64 {
            conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(quedan("assistant_message"), 0);
        assert_eq!(quedan("assistant_material"), 0);
        assert_eq!(quedan("assistant_conversation"), 3);
        // El id de una borrada no se vuelve a usar, aunque fuera la última (ni al borrarlas
        // todas): una respuesta que llega tarde no puede acabar en otra conversación.
        let ultima = |conn: &Connection| -> i64 {
            conn.query_row("SELECT max(id) FROM assistant_conversation", [], |r| {
                r.get(0)
            })
            .unwrap()
        };
        let antes = ultima(&conn);
        conn.execute("DELETE FROM assistant_conversation WHERE id = ?1", [antes])
            .unwrap();
        nueva("general", "").unwrap();
        assert_eq!(ultima(&conn), antes + 1);
        conn.execute("DELETE FROM assistant_conversation", [])
            .unwrap();
        nueva("general", "").unwrap();
        assert_eq!(ultima(&conn), antes + 2);
    }

    /// 0062: punto de partida de Criptomonedas. Una sola cabecera; por posición, cantidad en
    /// texto y coste no negativo; la misma moneda una vez por plataforma (y una sin plataforma);
    /// la plataforma, de las que existen.
    #[test]
    fn migracion_punto_de_partida_de_criptomonedas() {
        let conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO crypto_opening(id, date, account, source, updated_at)
             VALUES (1, '2025-12-31', '2050004', 'gestoría', 'x')",
            [],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO crypto_opening(id, date, updated_at) VALUES (2, '2024-12-31', 'x')",
                [],
            )
            .is_err(),
            "una sola cabecera"
        );
        conn.execute(
            "INSERT INTO crypto_platform(name, kind, created_at) VALUES ('Kraken', 'exchange', 'x')",
            [],
        )
        .unwrap();
        let pos = |ticker: &str, platform: Option<i64>, cost: i64| {
            conn.execute(
                "INSERT INTO crypto_opening_position(ticker, platform_id, qty, cost)
                 VALUES (?1, ?2, '1.5', ?3)",
                rusqlite::params![ticker, platform, cost],
            )
        };
        pos("BTC", None, 3_000_000).unwrap();
        pos("BTC", Some(1), 100).unwrap();
        pos("ETH", None, 0).unwrap();
        assert!(pos("BTC", None, 1).is_err(), "BTC sin plataforma dos veces");
        assert!(pos("BTC", Some(1), 1).is_err(), "BTC en Kraken dos veces");
        assert!(pos("SOL", None, -1).is_err(), "coste negativo");
        assert!(pos("SOL", Some(99), 1).is_err(), "plataforma que no existe");
        let n: i64 = conn
            .query_row("SELECT count(*) FROM crypto_opening_position", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 3);
    }

    /// 0066: lo que constaba como `correcto` (sólo había envío al entorno de pruebas) pasa a
    /// `pruebas`; el resto y los enlaces de reenvío se conservan, y siguen los triggers.
    #[test]
    fn migracion_0066_lo_correcto_era_de_pruebas() {
        // La versión del esquema es el número de migraciones, no el del fichero (faltan algunos).
        let before = MIGRATIONS
            .iter()
            .position(|m| {
                m.contains("taxpain:sin-claves-foraneas") && m.contains("verifactu_record_new")
            })
            .unwrap();
        let conn = open_in_memory_at_version(&Key256::random().unwrap(), before).unwrap();
        conn.execute_batch(
            "INSERT INTO invoice_series(id, code, data, created_at) VALUES (1, 'A', '{}', 'x');
             INSERT INTO invoice_issued(id, series_id, year, seq, number, issue_date, kind, base,
               vat, surcharge, withholding, total, payable, status, data, created_at)
               VALUES (1, 1, 2026, 1, 'A-1', '2026-10-13', 'F1', 100, 21, 0, 0, 121, 121,
                 'emitida', '{}', 'x');
             INSERT INTO verifactu_record(id, invoice_id, record_type, emisor_nif, num_serie, fecha,
               huella, generated_at, xml, send_status)
               VALUES (1, 1, 'alta', 'B12345674', 'A-1', '13-10-2026', 'H1', 'x', '<a/>',
                 'aceptado_con_errores'),
                      (2, 1, 'alta', 'B12345674', 'A-1', '13-10-2026', 'H2', 'x', '<a/>',
                 'correcto');
             UPDATE verifactu_record SET superseded_by = 2 WHERE id = 1;",
        )
        .unwrap();
        migrate(&conn).unwrap();
        assert_eq!(user_version(&conn).unwrap(), MIGRATIONS.len());
        let rows: Vec<(String, Option<i64>)> = conn
            .prepare("SELECT send_status, superseded_by FROM verifactu_record ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(std::result::Result::unwrap)
            .collect();
        assert_eq!(
            rows,
            [
                ("aceptado_con_errores".into(), Some(2)),
                ("pruebas".into(), None)
            ]
        );
        assert!(conn.execute("DELETE FROM verifactu_record", []).is_err());
        assert!(
            conn.execute("UPDATE verifactu_record SET huella = 'X' WHERE id = 1", [])
                .is_err()
        );
        assert!(
            conn.execute(
                "UPDATE verifactu_record SET superseded_by = 1 WHERE id = 1",
                []
            )
            .is_err(),
            "sólo se sustituye una vez"
        );
        conn.execute(
            "UPDATE verifactu_record SET send_status = 'correcto' WHERE id = 2",
            [],
        )
        .unwrap();
        assert!(
            conn.execute(
                "UPDATE verifactu_record SET send_status = 'otro' WHERE id = 2",
                []
            )
            .is_err()
        );
    }
}

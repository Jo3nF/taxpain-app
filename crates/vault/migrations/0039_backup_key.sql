-- Clave de las copias de seguridad (derivada de la contraseña con scrypt; la contraseña no se
-- guarda). Sólo hay una fila. No viaja en las copias (ver crates/vault/src/backup.rs).
CREATE TABLE backup_key (
  id         INTEGER PRIMARY KEY CHECK (id = 1),
  kek        BLOB NOT NULL,
  salt       BLOB NOT NULL,
  log_n      INTEGER NOT NULL,
  r          INTEGER NOT NULL,
  p          INTEGER NOT NULL,
  created_at TEXT NOT NULL
) STRICT;

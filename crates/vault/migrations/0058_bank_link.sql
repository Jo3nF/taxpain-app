-- Conexiones de solo lectura con los bancos (PSD2, información de cuentas) a través de Enable
-- Banking. Una conexión es el permiso que da el titular en su banco (hasta 180 días), con su
-- sesión; cada cuenta que autoriza se vuelca en una cuenta de `bank_account`, como un extracto.
-- La clave privada de la aplicación de Enable Banking va en `app_secret` (fuera de las copias).
CREATE TABLE bank_link (
  id          INTEGER PRIMARY KEY,
  aspsp       TEXT NOT NULL,               -- banco, como lo llama Enable Banking («BBVA»)
  country     TEXT NOT NULL,               -- ISO 3166 («ES»)
  psu_type    TEXT NOT NULL CHECK (psu_type IN ('business', 'personal')),
  state       TEXT NOT NULL,               -- valor al azar que vuelve en la dirección de vuelta
  session_id  TEXT,                        -- NULL mientras se espera la vuelta del banco
  valid_until TEXT,                        -- fin del permiso (RFC 3339)
  status      TEXT NOT NULL CHECK (status IN ('pendiente', 'activa', 'caducada')),
  created_at  TEXT NOT NULL,
  synced_at   TEXT,                        -- última descarga (RFC 3339)
  last_error  TEXT
) STRICT;

CREATE TABLE bank_link_account (
  id           INTEGER PRIMARY KEY,
  link_id      INTEGER NOT NULL REFERENCES bank_link(id) ON DELETE CASCADE,
  uid          TEXT NOT NULL,              -- id de la cuenta en Enable Banking (de la sesión)
  iban         TEXT,
  name         TEXT NOT NULL DEFAULT '',
  currency     TEXT NOT NULL DEFAULT 'EUR',
  cash_type    TEXT NOT NULL DEFAULT '',   -- ISO 20022: CACC corriente, CARD tarjeta…
  ident_hash   TEXT,                       -- huella estable de la cuenta entre sesiones
  card_last    TEXT,                       -- últimos dígitos, si es una tarjeta
  enabled      INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  bank_account INTEGER REFERENCES bank_account(id), -- cuenta de TaxPain en la que entra
  synced_until TEXT,                       -- día de la última descarga
  balance      INTEGER,                    -- saldo que da el banco (céntimos)
  balance_date TEXT,
  UNIQUE (link_id, uid)
) STRICT;

-- Identificador del movimiento en el banco (los que llegan por una conexión): con él no se
-- repite un movimiento aunque se descargue otra vez.
ALTER TABLE bank_tx ADD COLUMN external_id TEXT;
CREATE INDEX IF NOT EXISTS bank_tx_external ON bank_tx(bank_account, external_id)
  WHERE external_id IS NOT NULL;

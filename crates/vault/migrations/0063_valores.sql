-- Inversiones en valores (VALORES-01): cuentas de bróker, valores, operaciones, precios de
-- cierre, tipos de cambio del bróker y lo importado o sincronizado.
-- Cantidades y precios como texto decimal con punto (nunca REAL); importes en céntimos.

-- Cuentas de bróker. El token del Flex Web Service de IBKR no va aquí: va en `app_secret`
-- (BD cifrada, clave maestra en el Llavero) y nunca vuelve a la interfaz.
CREATE TABLE security_account (
  id             INTEGER PRIMARY KEY,
  broker         TEXT NOT NULL CHECK (broker IN ('xtb','ibkr','otro')),
  name           TEXT NOT NULL UNIQUE,
  number         TEXT NOT NULL DEFAULT '',     -- número de cuenta en el bróker
  currency       TEXT NOT NULL DEFAULT 'EUR',  -- divisa base de la cuenta
  country        TEXT NOT NULL DEFAULT '',     -- país del depositario (ISO 3166-1 alfa-2)
  abroad         INTEGER NOT NULL DEFAULT 1 CHECK (abroad IN (0,1)),  -- cuenta para el 720
  account        TEXT NOT NULL DEFAULT '',     -- subcuenta del efectivo en el bróker (vacía = la de los ajustes)
  flex_query     TEXT NOT NULL DEFAULT '',     -- IBKR: Query ID de la Activity Flex Query
  last_sync      TEXT,                         -- última sincronización o importación
  last_sync_note TEXT NOT NULL DEFAULT '',
  created_at     TEXT NOT NULL
) STRICT;

-- Valores (grupos homogéneos: los que tienen iguales derechos). El ISIN, si se conoce, junta el
-- mismo valor en brókeres distintos.
CREATE TABLE security (
  id          INTEGER PRIMARY KEY,
  isin        TEXT NOT NULL DEFAULT '',
  ticker      TEXT NOT NULL,
  name        TEXT NOT NULL DEFAULT '',
  currency    TEXT NOT NULL DEFAULT 'EUR',
  kind        TEXT NOT NULL DEFAULT 'accion' CHECK (kind IN ('accion','etf','fondo','bono','cfd','otro')),
  country     TEXT NOT NULL DEFAULT '',  -- país del emisor (del ISIN o del bróker)
  account     TEXT NOT NULL DEFAULT '',  -- subcuenta contable asignada
  created_at  TEXT NOT NULL
) STRICT;
CREATE UNIQUE INDEX security_isin ON security(isin) WHERE isin <> '';

-- Operaciones. `qty`/`price` en títulos y en la divisa del valor; `amount` y `fee` en la divisa
-- (céntimos); `fx` euros por unidad de la divisa (texto decimal); `eur` y `fee_eur` en euros.
-- Dividendos, retenciones, intereses, comisiones e impuestos llevan signo en `amount`/`eur`
-- (negativo = anulación o devolución; en los intereses, pagados).
CREATE TABLE security_op (
  id             INTEGER PRIMARY KEY,
  account_id     INTEGER NOT NULL REFERENCES security_account(id),
  to_account_id  INTEGER REFERENCES security_account(id),
  security_id    INTEGER REFERENCES security(id),
  date           TEXT NOT NULL,              -- YYYY-MM-DD
  time           TEXT NOT NULL DEFAULT '00:00:00',
  kind           TEXT NOT NULL CHECK (kind IN ('compra','venta','dividendo','retencion','interes','comision','impuesto','traspaso','split','deposito','retirada')),
  qty            TEXT NOT NULL DEFAULT '0',
  price          TEXT,
  currency       TEXT NOT NULL DEFAULT 'EUR',
  amount         INTEGER NOT NULL DEFAULT 0,
  fee            INTEGER NOT NULL DEFAULT 0 CHECK (fee >= 0),
  fx             TEXT,
  eur            INTEGER NOT NULL DEFAULT 0,
  fee_eur        INTEGER NOT NULL DEFAULT 0 CHECK (fee_eur >= 0),
  country        TEXT NOT NULL DEFAULT '',   -- país de la retención en origen
  note           TEXT NOT NULL DEFAULT '',
  source         TEXT NOT NULL DEFAULT 'manual',  -- manual | ibkr | xtb | xtb-posiciones | demo
  fingerprint    TEXT UNIQUE,
  entry_id       INTEGER REFERENCES journal_entry(id),
  created_at     TEXT NOT NULL
) STRICT;
CREATE INDEX security_op_date ON security_op(date, time);

-- Precios de cierre en la divisa del valor (del bróker o tecleados).
CREATE TABLE security_price (
  date         TEXT NOT NULL,
  security_id  INTEGER NOT NULL REFERENCES security(id),
  price        TEXT NOT NULL,
  currency     TEXT NOT NULL DEFAULT 'EUR',
  source       TEXT NOT NULL DEFAULT 'manual',
  updated_at   TEXT NOT NULL,
  PRIMARY KEY (date, security_id)
) STRICT;

-- Tipos de cambio que da el bróker (ConversionRates de IBKR): euros por unidad de la divisa.
CREATE TABLE security_fx (
  date        TEXT NOT NULL,
  currency    TEXT NOT NULL,
  eur_per_unit TEXT NOT NULL,
  source      TEXT NOT NULL DEFAULT 'manual',
  PRIMARY KEY (date, currency)
) STRICT;

-- Lo importado o descargado: periodo que cubren las operaciones en efectivo (para no duplicar
-- con lo leído de las posiciones) y el resumen de cada vez.
CREATE TABLE security_import (
  id          INTEGER PRIMARY KEY,
  account_id  INTEGER NOT NULL REFERENCES security_account(id),
  kind        TEXT NOT NULL,           -- flex | api | xtb-pdf
  file_sha256 TEXT NOT NULL DEFAULT '',
  cash_from   TEXT,                    -- periodo de las operaciones en efectivo (XTB)
  cash_to     TEXT,
  ops_new     INTEGER NOT NULL DEFAULT 0,
  ops_dup     INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL
) STRICT;

-- Criptomonedas: activos, plataformas, operaciones y precios de cierre (CRIPTO-01).
-- Cantidades como texto decimal con punto y hasta 18 decimales (nunca REAL); euros en céntimos.

-- Activos (ticker y nombre en llano).
CREATE TABLE crypto_asset (
  id          INTEGER PRIMARY KEY,
  ticker      TEXT NOT NULL UNIQUE,       -- en mayúsculas: BTC, ETH…
  name        TEXT NOT NULL,
  account     TEXT NOT NULL DEFAULT '',   -- subcuenta de existencias o intangible asignada al contabilizar
  created_at  TEXT NOT NULL
) STRICT;

-- Plataformas: exchange o monedero propio; «abroad» = custodiada por una entidad que no
-- presenta el 172/173 en España (art. 42 quater.2 RGAT), lo que cuenta para el 721.
CREATE TABLE crypto_platform (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  kind        TEXT NOT NULL CHECK (kind IN ('exchange','monedero')),
  country     TEXT NOT NULL DEFAULT 'ES', -- ISO 3166-1 alfa-2
  abroad      INTEGER NOT NULL DEFAULT 0 CHECK (abroad IN (0,1)),
  account     TEXT NOT NULL DEFAULT '',   -- contrapartida de los euros en esa plataforma (vacía = la de los ajustes)
  created_at  TEXT NOT NULL
) STRICT;

-- Operaciones. `ticker`/`qty` es el activo principal (el entregado en una permuta);
-- `ticker2`/`qty2` el recibido en una permuta. `eur` es el contravalor sin comisión y `fee_eur`
-- la comisión en euros, ambos en céntimos. `fingerprint` deduplica las importaciones.
CREATE TABLE crypto_op (
  id             INTEGER PRIMARY KEY,
  date           TEXT NOT NULL,            -- YYYY-MM-DD
  time           TEXT NOT NULL DEFAULT '00:00:00',
  kind           TEXT NOT NULL CHECK (kind IN ('compra','venta','permuta','traspaso','comision','recompensa','perdida','pago','cobro')),
  platform_id    INTEGER NOT NULL REFERENCES crypto_platform(id),
  to_platform_id INTEGER REFERENCES crypto_platform(id),
  ticker         TEXT NOT NULL,
  qty            TEXT NOT NULL,
  ticker2        TEXT,
  qty2           TEXT,
  eur            INTEGER NOT NULL DEFAULT 0 CHECK (eur >= 0),
  fee_eur        INTEGER NOT NULL DEFAULT 0 CHECK (fee_eur >= 0),
  ref_price      TEXT,                     -- precio de referencia en euros por unidad (texto decimal)
  note           TEXT NOT NULL DEFAULT '',
  source         TEXT NOT NULL DEFAULT 'manual',  -- manual | hoja | cointracking | kraken | demo
  fingerprint    TEXT UNIQUE,
  entry_id       INTEGER REFERENCES journal_entry(id),
  created_at     TEXT NOT NULL
) STRICT;
CREATE INDEX crypto_op_date ON crypto_op(date, time);

-- Precios de cierre (euros por unidad, texto decimal con hasta 8 decimales) que el usuario
-- teclea o importa de una hoja de saldos. La app no consulta internet para esto.
CREATE TABLE crypto_price (
  date        TEXT NOT NULL,
  ticker      TEXT NOT NULL,
  price       TEXT NOT NULL,
  source      TEXT NOT NULL DEFAULT 'manual',
  updated_at  TEXT NOT NULL,
  PRIMARY KEY (date, ticker)
) STRICT;

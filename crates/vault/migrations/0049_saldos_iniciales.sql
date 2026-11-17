-- Saldos iniciales: la empresa que empieza a usar TaxPain a mitad de camino (asistente de
-- apertura, CONTABILIDAD-05).

-- Asiento de apertura hecho con el asistente: ejercicio, fecha, asiento y una copia del borrador
-- (para deshacerlo y volver a editarlo).
CREATE TABLE opening_balance (
  id          INTEGER PRIMARY KEY,
  fiscal_year INTEGER NOT NULL,
  date        TEXT NOT NULL,
  entry_id    INTEGER REFERENCES journal_entry(id),
  data        TEXT NOT NULL,              -- JSON: el borrador aplicado
  created_at  TEXT NOT NULL
) STRICT;

-- Cobros y pagos pendientes de antes de TaxPain (facturas de clientes y proveedores que no están
-- en la app). Entran en la cartera como vencimientos con el id cambiado de signo (-id) para no
-- chocar con los de las facturas, y se cobran o pagan y concilian con el banco igual.
CREATE TABLE opening_item (
  id          INTEGER PRIMARY KEY,
  opening_id  INTEGER REFERENCES opening_balance(id),
  kind        TEXT NOT NULL CHECK (kind IN ('emitida','recibida')),
  account     TEXT NOT NULL,              -- subcuenta del cliente (43x) o proveedor (40x/41x)
  tax_id      TEXT,
  name        TEXT NOT NULL,
  number      TEXT NOT NULL,              -- número de la factura o documento
  issue_date  TEXT NOT NULL,
  due_date    TEXT NOT NULL,
  amount      INTEGER NOT NULL CHECK (amount <> 0),
  created_at  TEXT NOT NULL
) STRICT;

-- Amortización acumulada de un elemento antes de TaxPain (CONTABILIDAD-05): importe y día hasta
-- el que está incluida. La amortización de TaxPain sigue desde el día siguiente.
ALTER TABLE fixed_asset ADD COLUMN prior_depreciation INTEGER NOT NULL DEFAULT 0
    CHECK (prior_depreciation >= 0);
ALTER TABLE fixed_asset ADD COLUMN prior_until TEXT;
ALTER TABLE fixed_asset ADD COLUMN opening_id INTEGER REFERENCES opening_balance(id);

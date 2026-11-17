-- Pago (o cargo domiciliado) de un modelo a ingresar, con su asiento: 4750 (303), 4751 (111 y
-- 115), 473 (130 y 202) o 4752 (200) contra la tesorería. Uno por modelo y periodo.
CREATE TABLE tax_payment (
  id         INTEGER PRIMARY KEY,
  model      TEXT NOT NULL,
  year       INTEGER NOT NULL,
  period     TEXT NOT NULL,
  date       TEXT NOT NULL,
  account    TEXT NOT NULL,             -- cuenta de Hacienda
  treasury   TEXT NOT NULL,             -- 570, 572x…
  amount     INTEGER NOT NULL CHECK (amount > 0),
  entry_id   INTEGER REFERENCES journal_entry(id),
  bank_tx    INTEGER REFERENCES bank_tx(id),
  created_at TEXT NOT NULL,
  UNIQUE (model, year, period)
) STRICT;

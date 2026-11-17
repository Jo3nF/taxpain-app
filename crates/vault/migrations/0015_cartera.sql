-- Cartera de cobros y pagos. El vencimiento de cada factura es su fecha más el plazo de la
-- empresa, salvo que se fije otro (`invoice_due`). Una factura queda cobrada o pagada si se
-- concilió con un movimiento del banco (`bank_tx`) o si se registró aquí (`settlement`: caja u
-- otra cuenta, con su asiento).
CREATE TABLE invoice_due (
  kind       TEXT NOT NULL CHECK (kind IN ('emitida','recibida')),
  invoice_id INTEGER NOT NULL,
  due_date   TEXT NOT NULL,
  PRIMARY KEY (kind, invoice_id)
) STRICT;

CREATE TABLE settlement (
  id         INTEGER PRIMARY KEY,
  kind       TEXT NOT NULL CHECK (kind IN ('emitida','recibida')),
  invoice_id INTEGER NOT NULL,
  date       TEXT NOT NULL,
  account    TEXT NOT NULL,
  entry_id   INTEGER REFERENCES journal_entry(id),
  created_at TEXT NOT NULL,
  UNIQUE (kind, invoice_id)
) STRICT;

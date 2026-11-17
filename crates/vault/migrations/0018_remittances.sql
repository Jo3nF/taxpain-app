-- Remesas SEPA: recibos domiciliados (pain.008, Cuaderno 19.14) y órdenes de transferencia
-- (pain.001, Cuaderno 34.14). Datos bancarios del tercero en sus condiciones de pago.
ALTER TABLE party_terms ADD COLUMN iban TEXT;
ALTER TABLE party_terms ADD COLUMN mandate_ref TEXT;     -- referencia del mandato SEPA
ALTER TABLE party_terms ADD COLUMN mandate_date TEXT;    -- fecha de firma del mandato

CREATE TABLE remittance (
  id           INTEGER PRIMARY KEY,
  kind         TEXT NOT NULL CHECK (kind IN ('cobros','pagos')),
  reference    TEXT NOT NULL UNIQUE,      -- REM-2026-0001 / PAG-2026-0001
  bank_iban    TEXT NOT NULL,
  bridge       TEXT NOT NULL,             -- 4312 (cobros) o 555 (pagos) hasta que el banco lo apunta
  charge_date  TEXT NOT NULL,             -- fecha de cargo o de ejecución
  total        INTEGER NOT NULL,
  file         TEXT NOT NULL,             -- XML enviado al banco
  bank_tx_id   INTEGER REFERENCES bank_tx(id),
  created_at   TEXT NOT NULL
) STRICT;

-- Vencimientos de cada remesa. Un recibo devuelto vuelve a la cartera y puede ir en otra.
CREATE TABLE remittance_item (
  remittance_id INTEGER NOT NULL REFERENCES remittance(id) ON DELETE CASCADE,
  kind          TEXT NOT NULL CHECK (kind IN ('emitida','recibida')),
  invoice_id    INTEGER NOT NULL,
  seq           INTEGER NOT NULL,
  amount        INTEGER NOT NULL,
  returned_on   TEXT,
  PRIMARY KEY (remittance_id, kind, invoice_id, seq)
) STRICT;

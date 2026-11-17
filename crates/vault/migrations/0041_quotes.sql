-- Presupuestos y facturas proforma: no son facturas (sin VERI*FACTU, sin asiento, sin modelos).
-- Numeración propia por tipo y año. El borrador (cliente, líneas…) va en JSON.
CREATE TABLE IF NOT EXISTS quote (
  id             INTEGER PRIMARY KEY,
  kind           TEXT NOT NULL CHECK (kind IN ('presupuesto','proforma')),
  number         TEXT NOT NULL UNIQUE,
  year           INTEGER NOT NULL,
  seq            INTEGER NOT NULL,
  issue_date     TEXT NOT NULL,
  valid_until    TEXT,
  status         TEXT NOT NULL CHECK (status IN ('borrador','enviado','aceptado','rechazado','facturado')),
  recipient_name TEXT,
  total          INTEGER NOT NULL,
  draft          TEXT NOT NULL,
  language       TEXT,
  invoice_id     INTEGER REFERENCES invoice_issued(id),
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL,
  UNIQUE (kind, year, seq)
) STRICT;

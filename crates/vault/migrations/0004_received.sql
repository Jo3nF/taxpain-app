-- Fase 3: facturas recibidas. Nada cuenta (contabilidad, modelos) hasta que se revisa.
CREATE TABLE invoice_received (
  id             INTEGER PRIMARY KEY,
  document_id    INTEGER NOT NULL REFERENCES document(id),
  file_sha256    TEXT NOT NULL UNIQUE,
  filename       TEXT NOT NULL,
  source         TEXT NOT NULL CHECK (source IN ('manual','carpeta','correo')),
  status         TEXT NOT NULL CHECK (status IN ('pendiente','revisada','descartada')),
  extraction     TEXT NOT NULL,      -- Extraction serializada (con confianza por campo)
  supplier_nif   TEXT,
  supplier_name  TEXT,
  number         TEXT,
  issue_date     TEXT,
  base           INTEGER,
  vat            INTEGER,
  withholding    INTEGER,
  total          INTEGER,
  deductible_pct INTEGER NOT NULL DEFAULT 100 CHECK (deductible_pct BETWEEN 0 AND 100),
  reviewed_at    TEXT,
  created_at     TEXT NOT NULL
) STRICT;

CREATE UNIQUE INDEX invoice_received_unique_reviewed
  ON invoice_received(supplier_nif, number) WHERE status = 'revisada';

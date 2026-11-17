-- Fase 3: facturación. Importes en céntimos. Ver docs/DATA_MODEL.md.

CREATE TABLE contact (
  id         INTEGER PRIMARY KEY,
  kind       TEXT NOT NULL CHECK (kind IN ('cliente','proveedor','ambos')),
  tax_id     TEXT NOT NULL,            -- NIF o identificador extranjero
  country    TEXT NOT NULL DEFAULT 'ES',
  name       TEXT NOT NULL,
  data       TEXT NOT NULL,            -- Party serializado (domicilio, nombre…)
  created_at TEXT NOT NULL,
  UNIQUE (tax_id, country)
) STRICT;

CREATE TABLE invoice_series (
  id         INTEGER PRIMARY KEY,
  code       TEXT NOT NULL UNIQUE,
  data       TEXT NOT NULL,            -- Series serializada (prefijo, plantilla…)
  created_at TEXT NOT NULL
) STRICT;

-- Último número usado por serie y año (numeración correlativa sin huecos).
CREATE TABLE invoice_counter (
  series_id INTEGER NOT NULL REFERENCES invoice_series(id),
  year      INTEGER NOT NULL,
  last_seq  INTEGER NOT NULL,
  PRIMARY KEY (series_id, year)
) STRICT;

CREATE TABLE document (
  id         INTEGER PRIMARY KEY,
  uuid       TEXT NOT NULL UNIQUE,
  kind       TEXT NOT NULL,            -- factura_pdf, facturae, recibida…
  filename   TEXT NOT NULL,
  mime       TEXT NOT NULL,
  sha256     TEXT NOT NULL,
  size       INTEGER NOT NULL,
  content    BLOB NOT NULL,
  created_at TEXT NOT NULL
) STRICT;

CREATE TABLE invoice_issued (
  id                   INTEGER PRIMARY KEY,
  series_id            INTEGER NOT NULL REFERENCES invoice_series(id),
  year                 INTEGER NOT NULL,
  seq                  INTEGER NOT NULL,
  number               TEXT NOT NULL UNIQUE,
  issue_date           TEXT NOT NULL,
  kind                 TEXT NOT NULL,   -- F1, F2, F3, R1…R5
  contact_id           INTEGER REFERENCES contact(id),
  recipient_name       TEXT,
  recipient_tax_id     TEXT,
  base                 INTEGER NOT NULL,
  vat                  INTEGER NOT NULL,
  surcharge            INTEGER NOT NULL,
  withholding          INTEGER NOT NULL,
  total                INTEGER NOT NULL,
  payable              INTEGER NOT NULL,
  status               TEXT NOT NULL CHECK (status IN ('emitida','anulada')),
  rectifies_id         INTEGER REFERENCES invoice_issued(id),
  data                 TEXT NOT NULL,   -- Invoice + Totals serializados (fuente de verdad)
  pdf_document_id      INTEGER REFERENCES document(id),
  facturae_document_id INTEGER REFERENCES document(id),
  exported_path        TEXT,
  created_at           TEXT NOT NULL,
  UNIQUE (series_id, year, seq)
) STRICT;

CREATE INDEX invoice_issued_date ON invoice_issued(issue_date);

-- Cadena VERI*FACTU del obligado tributario: append-only, cada registro enlaza al anterior.
CREATE TABLE verifactu_record (
  id           INTEGER PRIMARY KEY,
  invoice_id   INTEGER NOT NULL REFERENCES invoice_issued(id),
  record_type  TEXT NOT NULL CHECK (record_type IN ('alta','anulacion')),
  emisor_nif   TEXT NOT NULL,
  num_serie    TEXT NOT NULL,
  fecha        TEXT NOT NULL,          -- DD-MM-YYYY
  huella       TEXT NOT NULL UNIQUE,
  prev_huella  TEXT,
  generated_at TEXT NOT NULL,
  xml          TEXT NOT NULL,
  qr_url       TEXT,
  send_status  TEXT NOT NULL DEFAULT 'pendiente' CHECK (send_status IN
                 ('pendiente','correcto','aceptado_con_errores','incorrecto')),
  error_code   TEXT,
  error_text   TEXT,
  csv          TEXT,
  sent_at      TEXT
) STRICT;

CREATE TRIGGER verifactu_record_no_delete BEFORE DELETE ON verifactu_record
BEGIN SELECT RAISE(ABORT, 'los registros VERI*FACTU no se borran'); END;

CREATE TRIGGER verifactu_record_immutable BEFORE UPDATE OF
  invoice_id, record_type, emisor_nif, num_serie, fecha, huella, prev_huella, generated_at, xml
  ON verifactu_record
BEGIN SELECT RAISE(ABORT, 'los registros VERI*FACTU son inmutables'); END;

-- Importación de la carpeta de la empresa (lo que da la gestoría):
-- - facturas emitidas de antes de usar TaxPain, sacadas del libro registro del gestor: van a los
--   libros y a los modelos, pero no generan registro VERI*FACTU ni asiento (la contabilidad
--   viene del diario del gestor);
-- - facturas recibidas del libro registro, también sin asiento;
-- - modelos presentados por el gestor, para compararlos con los que calcula TaxPain.
ALTER TABLE invoice_issued ADD COLUMN origin TEXT NOT NULL DEFAULT 'app'
  CHECK (origin IN ('app', 'importada'));
ALTER TABLE invoice_received ADD COLUMN posted INTEGER NOT NULL DEFAULT 1
  CHECK (posted IN (0, 1));

CREATE TABLE filed_model (
  id          INTEGER PRIMARY KEY,
  model       TEXT NOT NULL,
  year        INTEGER NOT NULL,
  -- '1T'…'4T', '01'…'12', '0A' (anual) o '1P'…'3P' (pagos fraccionados del 202).
  period      TEXT NOT NULL,
  filename    TEXT NOT NULL,
  file_sha256 TEXT NOT NULL UNIQUE,
  -- 'fichero' (formato BOE de la AEAT), 'pdf' (leído del PDF) o 'manual'.
  source      TEXT NOT NULL CHECK (source IN ('fichero', 'pdf', 'manual')),
  -- Casillas leídas: {"27": 98700, …} en céntimos.
  boxes       TEXT NOT NULL,
  document_id INTEGER REFERENCES document(id),
  imported_at TEXT NOT NULL
) STRICT;
CREATE INDEX filed_model_period ON filed_model(model, year, period);

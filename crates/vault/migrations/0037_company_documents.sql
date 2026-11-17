-- Documentos de la empresa (escrituras, tarjeta NIF, alta en el ROI, 036/037, certificados,
-- nota simple…) con los datos leídos de ellos; el fichero va en `document`.
CREATE TABLE company_document (
  id          INTEGER PRIMARY KEY,
  kind        TEXT NOT NULL,
  title       TEXT NOT NULL,
  filename    TEXT NOT NULL,
  file_sha256 TEXT NOT NULL UNIQUE,
  document_id INTEGER NOT NULL REFERENCES document(id),
  -- Datos leídos (JSON de `documentos::Facts`).
  facts       TEXT NOT NULL,
  doc_date    TEXT,
  imported_at TEXT NOT NULL
) STRICT;

-- Comunicaciones y notificaciones de la AEAT (servicio GNNO-JDIT). Metadatos públicos al
-- consultar; contenido (PDF del envío y certificación de comparecencia) al acceder. Una vez
-- accedido, el contenido no cambia ni se borra.
CREATE TABLE aeat_notice (
  id               TEXT PRIMARY KEY,           -- NumeroCertificado (13 dígitos)
  kind             TEXT NOT NULL,              -- C comunicación, N notificación
  state            TEXT NOT NULL,              -- A accedido, P pendiente
  role             TEXT NOT NULL,              -- T titular, A autorizado
  subject          TEXT NOT NULL,
  available_at     TEXT NOT NULL,
  holder_nif       TEXT NOT NULL,
  environment      TEXT NOT NULL CHECK (environment IN ('pruebas', 'produccion')),
  seen_at          TEXT NOT NULL,              -- primera vez que se listó
  accessed_at      TEXT,                       -- cuándo la descargó o compareció TaxPain
  metadata         TEXT,                       -- JSON de metadatos privados
  document_id      INTEGER REFERENCES document(id),
  certification_id INTEGER REFERENCES document(id)
) STRICT;

CREATE TRIGGER aeat_notice_no_delete BEFORE DELETE ON aeat_notice
BEGIN SELECT RAISE(ABORT, 'las notificaciones no se borran'); END;

CREATE TRIGGER aeat_notice_content_immutable BEFORE UPDATE OF document_id, certification_id, metadata
  ON aeat_notice WHEN OLD.document_id IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'el contenido de una notificación accedida no cambia'); END;

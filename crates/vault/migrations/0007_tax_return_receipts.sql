-- Justificante en PDF de cada declaración presentada (el justificante legal es el PDF, no la
-- respuesta JSON: especificación de presentación directa v30, §1.3). Tabla aparte porque
-- tax_return es inmutable y el PDF se descarga después de grabar la presentación.
CREATE TABLE tax_return_receipt (
  tax_return_id INTEGER PRIMARY KEY REFERENCES tax_return(id),
  document_id   INTEGER NOT NULL REFERENCES document(id),
  downloaded_at TEXT NOT NULL
) STRICT;

CREATE TRIGGER tax_return_receipt_no_delete BEFORE DELETE ON tax_return_receipt
BEGIN SELECT RAISE(ABORT, 'los justificantes no se borran'); END;

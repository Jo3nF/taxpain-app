-- Observaciones y documentos adjuntos de un asiento (justificantes, contratos…). Los documentos
-- van a la tabla `document` cifrada, como los de las facturas.
ALTER TABLE journal_entry ADD COLUMN notes TEXT;
CREATE TABLE entry_document (
  entry_id    INTEGER NOT NULL REFERENCES journal_entry(id),
  document_id INTEGER NOT NULL REFERENCES document(id),
  PRIMARY KEY (entry_id, document_id)
) STRICT;

-- Al borrar un asiento (manual, nómina, pago…) se van sus adjuntos y, con ellos, el documento.
CREATE TRIGGER journal_entry_docs BEFORE DELETE ON journal_entry
BEGIN DELETE FROM entry_document WHERE entry_id = OLD.id; END;

CREATE TRIGGER entry_document_gone AFTER DELETE ON entry_document
BEGIN DELETE FROM document WHERE id = OLD.document_id AND kind = 'asiento'; END;

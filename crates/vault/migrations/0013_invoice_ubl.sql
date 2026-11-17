-- Factura electrónica UBL (EN 16931, Anexo I Orden HAC/1028/2026) de cada emitida, si se genera.
ALTER TABLE invoice_issued ADD COLUMN ubl_document_id INTEGER REFERENCES document(id);

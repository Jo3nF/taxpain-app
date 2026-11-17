-- Lotes de importación de facturas a los libros de TaxPain (libros registro del gestor y
-- facturas sueltas leídas con el asistente): cada factura importada lleva su lote para poder
-- deshacer la importación entera, como los lotes de asientos (`import_batch`).
CREATE TABLE IF NOT EXISTS book_import (
  id         INTEGER PRIMARY KEY,
  -- Libro al que fueron: 'emitidas' o 'recibidas'.
  kind       TEXT NOT NULL CHECK (kind IN ('emitidas', 'recibidas')),
  -- 'libro' (libro registro del gestor) o 'lectura' (factura leída con el asistente).
  source     TEXT NOT NULL CHECK (source IN ('libro', 'lectura')),
  filename   TEXT NOT NULL,
  sha256     TEXT NOT NULL,
  invoices   INTEGER NOT NULL,
  created_at TEXT NOT NULL
) STRICT;
ALTER TABLE invoice_issued ADD COLUMN book_import_id INTEGER REFERENCES book_import(id);
ALTER TABLE invoice_received ADD COLUMN book_import_id INTEGER REFERENCES book_import(id);

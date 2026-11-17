-- Lotes de importación de asientos (SUENLACE de a3, Excel o CSV). Cada asiento importado lleva
-- el lote como origen (`Importado`), para poder deshacer el lote entero. El mismo fichero no se
-- importa dos veces.
CREATE TABLE import_batch (
  id         INTEGER PRIMARY KEY,
  source     TEXT NOT NULL CHECK (source IN ('suenlace','tabla')),
  filename   TEXT NOT NULL,
  sha256     TEXT NOT NULL UNIQUE,
  entries    INTEGER NOT NULL,
  created_at TEXT NOT NULL
) STRICT;

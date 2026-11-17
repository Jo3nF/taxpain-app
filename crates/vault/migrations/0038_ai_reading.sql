-- Lecturas de ficheros de la carpeta hechas con el asistente («Full IA»), por huella del
-- fichero: no se vuelve a pagar la misma lectura y al importar se usa lo que dijo.
CREATE TABLE ai_reading (
  file_sha256 TEXT PRIMARY KEY,
  reading     TEXT NOT NULL,
  full        INTEGER NOT NULL CHECK (full IN (0, 1)),
  created_at  TEXT NOT NULL
) STRICT;

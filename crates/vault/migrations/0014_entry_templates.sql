-- Asientos predefinidos (plantillas) y periódicos. `lines` es JSON (cuenta, descripción, lado e
-- importe fijo o a completar). Los periódicos llevan `every_months` y `next_date`.
CREATE TABLE entry_template (
  id           INTEGER PRIMARY KEY,
  name         TEXT NOT NULL UNIQUE CHECK (length(trim(name)) > 0),
  concept      TEXT NOT NULL,
  lines        TEXT NOT NULL,
  every_months INTEGER CHECK (every_months BETWEEN 1 AND 12),
  next_date    TEXT,
  counter      INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL,
  CHECK ((every_months IS NULL) = (next_date IS NULL))
) STRICT;

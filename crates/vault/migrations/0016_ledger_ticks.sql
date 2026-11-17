-- Punteo del mayor: marca auxiliar de líneas revisadas o casadas. Va aparte de `journal_line`
-- para poder puntear también en ejercicios cerrados (los libros no cambian).
CREATE TABLE line_tick (
  line_id    INTEGER PRIMARY KEY REFERENCES journal_line(id) ON DELETE CASCADE,
  ticked_at  TEXT NOT NULL
) STRICT;

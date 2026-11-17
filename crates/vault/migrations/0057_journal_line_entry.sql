-- Las líneas de cada asiento se leen por `entry_id` (cada vez que se carga el diario): sin este
-- índice, cada asiento recorría la tabla entera y un diario de miles de asientos tardaba segundos.
CREATE INDEX IF NOT EXISTS journal_line_entry ON journal_line(entry_id);

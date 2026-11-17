-- Reparto (aplicación del resultado de ese ejercicio: journal_entry con origin_kind
-- 'AplicacionResultado' y origin_id = ejercicio) con cargo al que se paga cada dividendo: la
-- última aplicación contabilizada a la fecha de exigibilidad. NULL en los pagos anteriores a
-- esta columna y cuando la 526 viene de un asiento manual.
ALTER TABLE dividend_payment ADD COLUMN allocation_year INTEGER;

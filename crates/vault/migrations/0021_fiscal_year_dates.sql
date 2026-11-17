-- Ejercicios no naturales: cada ejercicio guarda sus fechas. Los existentes eran naturales.
ALTER TABLE fiscal_year ADD COLUMN start_date TEXT;
ALTER TABLE fiscal_year ADD COLUMN end_date TEXT;
UPDATE fiscal_year
   SET start_date = printf('%04d-01-01', year), end_date = printf('%04d-12-31', year)
 WHERE start_date IS NULL;

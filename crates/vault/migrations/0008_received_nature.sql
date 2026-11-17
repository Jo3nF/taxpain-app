-- Naturaleza de las facturas recibidas, para los modelos (Fase 6): bien de inversión (art. 108
-- LIVA; casillas [30]/[31] del 303) y tipo de gasto con retención (alquiler → 115/180;
-- profesional → 111/190). Las ya revisadas quedan como 'general' (se sigue detectando el
-- alquiler por la retención del 19 % sólo en ellas).
ALTER TABLE invoice_received ADD COLUMN capital_good INTEGER NOT NULL DEFAULT 0
  CHECK (capital_good IN (0, 1));
ALTER TABLE invoice_received ADD COLUMN nature TEXT NOT NULL DEFAULT 'general'
  CHECK (nature IN ('general', 'alquiler', 'profesional'));

-- Baja o venta de un elemento del inmovilizado: precio de venta y asiento de la baja.
ALTER TABLE fixed_asset ADD COLUMN sale_price INTEGER;
ALTER TABLE fixed_asset ADD COLUMN disposal_entry INTEGER REFERENCES journal_entry(id);

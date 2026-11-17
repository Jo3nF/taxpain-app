-- Facturas recibidas en moneda extranjera: moneda, tipo de cambio (art. 79.Once LIVA) e
-- importes en la moneda, para consulta. Los importes de la fila (base, IVA, total…) siguen en
-- euros, que son los que se contabilizan y declaran. JSON; NULL si la factura es en euros.
ALTER TABLE invoice_received ADD COLUMN fx TEXT;

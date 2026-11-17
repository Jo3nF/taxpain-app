-- Importaciones: el DUA (documento de aduanas) se registra como recibida con la base (valor en
-- aduana) y el IVA liquidado en la aduana. Sólo se paga (y se contabiliza) el IVA: la mercancía
-- va en la factura del proveedor extranjero.
ALTER TABLE invoice_received ADD COLUMN import_dua INTEGER NOT NULL DEFAULT 0 CHECK (import_dua IN (0,1));

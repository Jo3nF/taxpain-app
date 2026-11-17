-- Criterio de caja del IVA: la recibida es de un proveedor acogido al régimen especial (su IVA se
-- deduce al pagarla aunque la empresa no esté en el régimen; art. 163 terdecies LIVA).
ALTER TABLE invoice_received ADD COLUMN cash_basis INTEGER NOT NULL DEFAULT 0 CHECK (cash_basis IN (0,1));

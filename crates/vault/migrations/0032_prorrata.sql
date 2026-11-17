-- Prorrata especial (art. 106 LIVA): destino de cada compra (comun, plena o ninguna).
-- Regularización de bienes de inversión (arts. 107 a 110): los inmuebles se regularizan
-- durante nueve años y el resto durante cuatro.
ALTER TABLE invoice_received ADD COLUMN prorata_use TEXT NOT NULL DEFAULT 'comun'
    CHECK (prorata_use IN ('comun', 'plena', 'ninguna'));
ALTER TABLE invoice_received ADD COLUMN property INTEGER NOT NULL DEFAULT 0;

-- Facturas recibidas sin IVA con inversión del sujeto pasivo (arts. 84.Uno.2º y 85 LIVA):
-- adquisiciones intracomunitarias o servicios de empresas no establecidas. El destinatario
-- autorrepercute el IVA con el tipo que corresponda. El proveedor puede tener un NIF-IVA de
-- otro país (`supplier_country`, ISO 3166-1 alfa-2; NULL = España).
ALTER TABLE invoice_received ADD COLUMN vat_regime TEXT NOT NULL DEFAULT 'interior'
  CHECK (vat_regime IN ('interior', 'intracomunitaria', 'isp'));
ALTER TABLE invoice_received ADD COLUMN supplier_country TEXT;
ALTER TABLE invoice_received ADD COLUMN self_assessed_rate INTEGER;
-- Para el 349: adquisición intracomunitaria de bienes (clave A) o de servicios (clave I).
ALTER TABLE invoice_received ADD COLUMN intra_goods INTEGER NOT NULL DEFAULT 0
  CHECK (intra_goods IN (0, 1));

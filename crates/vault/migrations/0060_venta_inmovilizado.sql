-- Facturas emitidas con las que se vende un elemento del inmovilizado (CONTABILIDAD-08). La baja
-- deja el precio en la 543; al vincular la factura, un asiento pasa su base de ventas (70x) a la
-- 543 y, si no coinciden, la diferencia al resultado de la baja (67x/77x): el ingreso cuenta una
-- sola vez. Una factura puede vender varios elementos (cada uno con su parte) y un elemento lleva
-- su factura de venta y las rectificativas de ésta. Mientras está vinculada, no se puede anular.
CREATE TABLE IF NOT EXISTS asset_sale_invoice (
  asset_id   INTEGER NOT NULL REFERENCES fixed_asset(id),
  invoice_id INTEGER NOT NULL REFERENCES invoice_issued(id),
  amount     INTEGER NOT NULL CHECK (amount <> 0),          -- base pasada, en céntimos (negativa: la rectificativa rebaja el precio)
  entry_id   INTEGER NOT NULL REFERENCES journal_entry(id), -- asiento que la pasa (origen BajaInmovilizado)
  created_at TEXT NOT NULL,
  PRIMARY KEY (asset_id, invoice_id)
) STRICT;

CREATE INDEX IF NOT EXISTS asset_sale_invoice_invoice ON asset_sale_invoice(invoice_id);

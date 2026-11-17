-- Cuenta de ventas (cliente) o de gastos (proveedor) propia de un tercero: sus facturas se
-- contabilizan contra ella en vez de la cuenta por defecto (705, 629…).
CREATE TABLE party_counterpart (
  account     TEXT PRIMARY KEY,         -- subcuenta del tercero
  counterpart TEXT NOT NULL,
  updated_at  TEXT NOT NULL
) STRICT;

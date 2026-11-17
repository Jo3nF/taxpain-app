-- Cuentas del banco en otra divisa (USD, GBP…): cada movimiento guarda, además de su importe en
-- la divisa de la cuenta (`amount`), su contravalor en euros con el que se contabiliza, el tipo
-- (unidades de la divisa por 1 €, en millonésimas, como lo publica el BCE), la fecha del tipo y
-- de dónde sale: `bce` (tipo de referencia del BCE ya descargado), `manual` (tecleado: el tipo o
-- los euros que da el propio extracto) o `traspaso` (los euros reales de la otra cara de un
-- traspaso entre cuentas propias de distinta divisa). En las cuentas en euros, vacío.
ALTER TABLE bank_tx ADD COLUMN amount_eur INTEGER;
ALTER TABLE bank_tx ADD COLUMN fx_rate INTEGER;
ALTER TABLE bank_tx ADD COLUMN fx_date TEXT;
ALTER TABLE bank_tx ADD COLUMN fx_source TEXT CHECK (fx_source IN ('bce', 'manual', 'traspaso'));

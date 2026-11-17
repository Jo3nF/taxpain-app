-- Acuerdos de reparto de dividendos (arts. 273-277 LSC), con el semáforo de comprobaciones
-- legales del día del acuerdo y el acta que lo documenta:
-- - `a_cuenta`: dividendo a cuenta del resultado del ejercicio en curso, con el estado contable
--   de liquidez del art. 277 LSC (557 «Dividendo activo a cuenta» al debe, 526 al haber). Al
--   aplicar el resultado, la 557 se salda (cierre::post_allocation).
-- - `reservas`: con cargo a reservas de libre disposición (113 o 120 al debe, 526 al haber).
-- - `resultado`: el dividendo que acuerda la junta al aplicar el resultado del ejercicio
--   `allocation_year` (el asiento es el de la aplicación, que se contabiliza en Cierre).
-- `account`: la cuenta cargada (557, 113, 120 o 129). `checks`: JSON con las comprobaciones.
CREATE TABLE IF NOT EXISTS dividend_agreement (
  id              INTEGER PRIMARY KEY,
  kind            TEXT NOT NULL CHECK (kind IN ('a_cuenta','reservas','resultado')),
  date            TEXT NOT NULL,
  amount          INTEGER NOT NULL CHECK (amount > 0),
  payment_date    TEXT,
  statement_date  TEXT,
  tax_rate_bp     INTEGER,
  account         TEXT NOT NULL,
  allocation_year INTEGER,
  entry_id        INTEGER REFERENCES journal_entry(id),
  minute_id       INTEGER REFERENCES minute(id) ON DELETE SET NULL,
  checks          TEXT NOT NULL DEFAULT '[]',
  created_at      TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS dividend_agreement_by_date ON dividend_agreement(date, id);

-- Acuerdo del que sale cada pago (NULL en los anteriores y cuando la 526 viene de un asiento
-- manual sin acuerdo registrado).
ALTER TABLE dividend_payment ADD COLUMN agreement_id INTEGER REFERENCES dividend_agreement(id);

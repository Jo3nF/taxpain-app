-- Ajustes de cierre (PGC de PYMES, marco conceptual: principio de devengo; NRV 9.ª y 15.ª):
-- periodificaciones de gastos e ingresos (480/485), facturas pendientes de recibir o de emitir
-- (4009/4309), deterioro de créditos comerciales (694/490) y provisiones (14x, 499, 529).
--
-- Cada fila es un ajuste de un ejercicio (`year`) con su asiento (`entry_id`, con la fecha de
-- cierre) y, si ya se revirtió, el de la reversión (`reversal_entry_id`): las periodificaciones
-- y las facturas pendientes se revierten solas el primer día del ejercicio siguiente (se graban
-- a la vez); los deterioros y las provisiones, cuando el usuario lo pide en un ejercicio
-- posterior (`reversal_year`), por cobro, aplicación o exceso.
--
-- `tipo`: gasto_anticipado, ingreso_anticipado, pendiente_recibir, pendiente_emitir,
-- deterioro, provision.
-- `cuenta`: la de gasto o ingreso (6xx/7xx); `contrapartida`: la del balance (480, 485, 4009,
-- 4309, 490, 14x…).
-- `factura_kind` y `factura_id`: la factura periodificada (recibida o emitida) o, en un
-- deterioro, la emitida (id negativo: cobro pendiente de los saldos iniciales) y su plazo
-- (`vencimiento`).
-- `deducible`: el gasto es fiscalmente deducible en el ejercicio (arts. 13.1 y 14 LIS); si no,
-- es una corrección en aumento en el 200 de `year` y en disminución en el de la reversión.
CREATE TABLE IF NOT EXISTS ajuste_cierre (
  id                INTEGER PRIMARY KEY,
  year              INTEGER NOT NULL,
  tipo              TEXT NOT NULL CHECK (tipo IN ('gasto_anticipado','ingreso_anticipado',
                      'pendiente_recibir','pendiente_emitir','deterioro','provision')),
  descripcion       TEXT NOT NULL,
  importe           INTEGER NOT NULL CHECK (importe > 0),
  cuenta            TEXT NOT NULL,
  contrapartida     TEXT NOT NULL,
  factura_kind      TEXT CHECK (factura_kind IS NULL OR factura_kind IN ('recibida','emitida')),
  factura_id        INTEGER,
  vencimiento       INTEGER,
  servicio_desde    TEXT,
  servicio_hasta    TEXT,
  tercero_nif       TEXT,
  tercero           TEXT,
  deducible         INTEGER NOT NULL DEFAULT 1 CHECK (deducible IN (0,1)),
  motivo_fiscal     TEXT NOT NULL DEFAULT '',
  entry_id          INTEGER NOT NULL REFERENCES journal_entry(id),
  reversal_entry_id INTEGER REFERENCES journal_entry(id),
  reversal_year     INTEGER,
  created_at        TEXT NOT NULL,
  CHECK ((reversal_entry_id IS NULL) = (reversal_year IS NULL))
) STRICT;

CREATE INDEX IF NOT EXISTS ajuste_cierre_year ON ajuste_cierre(year);
CREATE INDEX IF NOT EXISTS ajuste_cierre_factura ON ajuste_cierre(factura_kind, factura_id);

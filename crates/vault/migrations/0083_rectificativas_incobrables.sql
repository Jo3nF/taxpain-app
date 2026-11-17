-- Rectificativas emitidas (RD 1619/2012 art. 15; Orden HAC/1177/2024, TipoRectificativa) y
-- créditos incobrables (art. 80.Cuatro LIVA; art. 24 RIVA).
--
-- En `invoice_issued` la factura guarda sus importes tal como se expidieron. Una rectificativa
-- por sustitución (`rect_method` = 'S') recoge los importes correctos completos, así que lo que
-- cambia de verdad es su importe menos el de las facturas que sustituye: `replaced_*` es la suma
-- de las facturas rectificadas (lo que va en ImporteRectificacion de VERI*FACTU), para que los
-- totales por SQL resten lo sustituido (`base - COALESCE(replaced_base, 0)`). Por diferencias
-- ('I') no hay `replaced_*`. Las anteriores a esta migración quedan con `rect_method` vacío.
ALTER TABLE invoice_issued ADD COLUMN rect_method TEXT CHECK (rect_method IS NULL OR rect_method IN ('S', 'I'));
ALTER TABLE invoice_issued ADD COLUMN replaced_base INTEGER;
ALTER TABLE invoice_issued ADD COLUMN replaced_vat INTEGER;
ALTER TABLE invoice_issued ADD COLUMN replaced_surcharge INTEGER;
ALTER TABLE invoice_issued ADD COLUMN replaced_payable INTEGER;

-- Facturas que rectifica cada rectificativa (una rectificativa puede agrupar varias del mismo
-- cliente; `rectifies_id` sólo guarda la primera).
CREATE TABLE IF NOT EXISTS invoice_rectified (
  rectifying_id INTEGER NOT NULL REFERENCES invoice_issued(id),
  rectified_id  INTEGER NOT NULL REFERENCES invoice_issued(id),
  PRIMARY KEY (rectifying_id, rectified_id)
) STRICT;
CREATE INDEX IF NOT EXISTS invoice_rectified_original ON invoice_rectified(rectified_id);

-- Seguimiento de un crédito incobrable (art. 80.Cuatro LIVA), por factura emitida:
-- `plazo_meses`: 6 o 12 (6 sólo si el volumen de operaciones del año anterior no pasó de
-- 6.010.121,04 €); `reclamacion_medio`: judicial, notarial u otro medio fehaciente;
-- `rectificativa_id`: la R3 que reduce la base; `comunicada_el`: la comunicación a la AEAT
-- (art. 24.2 RIVA, en el mes siguiente a la R3); `estado`: abierto, rectificado, comunicado,
-- cobrado (se cobró antes de rectificar) o desistido (se desistió de la reclamación o hubo
-- acuerdo de cobro después de la R3: hay que rectificar al alza).
CREATE TABLE IF NOT EXISTS credito_incobrable (
  id                  INTEGER PRIMARY KEY,
  invoice_id          INTEGER NOT NULL UNIQUE REFERENCES invoice_issued(id),
  plazo_meses         INTEGER NOT NULL CHECK (plazo_meses IN (6, 12)),
  estado              TEXT NOT NULL DEFAULT 'abierto'
                        CHECK (estado IN ('abierto','rectificado','comunicado','cobrado','desistido')),
  exclusion           TEXT NOT NULL DEFAULT 'ninguna',
  reclamacion_fecha   TEXT,
  reclamacion_medio   TEXT CHECK (reclamacion_medio IS NULL
                        OR reclamacion_medio IN ('judicial','notarial','fehaciente')),
  reclamacion_ref     TEXT,
  rectificativa_id    INTEGER REFERENCES invoice_issued(id),
  comunicada_el       TEXT,
  comunicacion_ref    TEXT,
  cerrado_el          TEXT,
  notas               TEXT NOT NULL DEFAULT '',
  created_at          TEXT NOT NULL,
  updated_at          TEXT NOT NULL
) STRICT;

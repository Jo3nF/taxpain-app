-- Condiciones de cobro y pago de cada cliente o proveedor, guardadas en su subcuenta (430…,
-- 400…, 410…) como en la ficha de a3, y vencimientos fraccionados.
CREATE TABLE party_terms (
  account   TEXT PRIMARY KEY,         -- subcuenta del tercero
  days      TEXT NOT NULL,            -- JSON: días de cada plazo desde la factura, p. ej. [30,60]
  pay_day   INTEGER CHECK (pay_day BETWEEN 1 AND 31),  -- día de pago fijo del mes
  method    TEXT NOT NULL DEFAULT 'transferencia'
            CHECK (method IN ('transferencia','recibo','efectivo','tarjeta','otro')),
  treasury  TEXT,                     -- cuenta de cobro o pago por defecto (570, 572x)
  updated_at TEXT NOT NULL
) STRICT;

-- Vencimientos fijados de una factura (fraccionados o con fecha a mano). Sin filas, salen de las
-- condiciones del tercero o, si no tiene, del plazo de la empresa.
CREATE TABLE invoice_installment (
  kind       TEXT NOT NULL CHECK (kind IN ('emitida','recibida')),
  invoice_id INTEGER NOT NULL,
  seq        INTEGER NOT NULL CHECK (seq >= 1),
  due_date   TEXT NOT NULL,
  amount     INTEGER NOT NULL,
  PRIMARY KEY (kind, invoice_id, seq)
) STRICT;

INSERT INTO invoice_installment(kind, invoice_id, seq, due_date, amount)
SELECT d.kind, d.invoice_id, 1, d.due_date,
       CASE d.kind
         WHEN 'emitida' THEN (SELECT i.payable FROM invoice_issued i WHERE i.id = d.invoice_id)
         ELSE (SELECT COALESCE(r.base, 0) + COALESCE(r.vat, 0) - COALESCE(r.withholding, 0)
               FROM invoice_received r WHERE r.id = d.invoice_id)
       END
FROM invoice_due d
WHERE EXISTS (SELECT 1 FROM invoice_issued i WHERE d.kind = 'emitida' AND i.id = d.invoice_id)
   OR EXISTS (SELECT 1 FROM invoice_received r WHERE d.kind = 'recibida' AND r.id = d.invoice_id);

DROP TABLE invoice_due;

-- Un cobro o pago registrado aquí es de un vencimiento (`seq`); los anteriores eran de la
-- factura entera, que tenía uno solo.
CREATE TABLE settlement_new (
  id         INTEGER PRIMARY KEY,
  kind       TEXT NOT NULL CHECK (kind IN ('emitida','recibida')),
  invoice_id INTEGER NOT NULL,
  seq        INTEGER NOT NULL DEFAULT 1 CHECK (seq >= 1),
  date       TEXT NOT NULL,
  account    TEXT NOT NULL,
  entry_id   INTEGER REFERENCES journal_entry(id),
  created_at TEXT NOT NULL,
  UNIQUE (kind, invoice_id, seq)
) STRICT;
INSERT INTO settlement_new(id, kind, invoice_id, seq, date, account, entry_id, created_at)
SELECT id, kind, invoice_id, 1, date, account, entry_id, created_at FROM settlement;
DROP TABLE settlement;
ALTER TABLE settlement_new RENAME TO settlement;

-- Movimiento del banco casado con un vencimiento concreto (NULL: la factura entera).
ALTER TABLE bank_tx ADD COLUMN matched_seq INTEGER;

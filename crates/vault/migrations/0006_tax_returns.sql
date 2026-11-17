-- Declaraciones presentadas a la AEAT (Fase 6). Una fila por presentación aceptada, con su
-- justificante. Inmutables: «no es posible anular declaraciones ya grabadas» (especificación de
-- presentación directa v30, §1.3); una corrección es otra presentación (complementaria o
-- rectificativa).
CREATE TABLE tax_return (
  id              INTEGER PRIMARY KEY,
  model           TEXT NOT NULL,
  year            INTEGER NOT NULL,
  period          TEXT NOT NULL,                 -- 1T…4T, 1P…3P, 0A
  environment     TEXT NOT NULL CHECK (environment IN ('pruebas', 'produccion')),
  declaration     TEXT NOT NULL,                 -- forma de pago / tipo de declaración
  result          INTEGER NOT NULL,              -- céntimos
  boxes           TEXT NOT NULL,                 -- JSON [[casilla, céntimos], …]
  file_sha256     TEXT NOT NULL,
  file            TEXT NOT NULL,                 -- fichero DR presentado
  csv             TEXT NOT NULL,                 -- Código Seguro de Verificación
  receipt_number  TEXT NOT NULL,                 -- número de justificante
  receipt         TEXT NOT NULL,                 -- JSON con la respuesta completa
  presented_at    TEXT NOT NULL
) STRICT;

CREATE INDEX tax_return_by_period ON tax_return(model, year, period, environment);

CREATE TRIGGER tax_return_no_delete BEFORE DELETE ON tax_return
BEGIN SELECT RAISE(ABORT, 'las declaraciones presentadas no se borran'); END;

CREATE TRIGGER tax_return_immutable BEFORE UPDATE ON tax_return
BEGIN SELECT RAISE(ABORT, 'las declaraciones presentadas son inmutables'); END;

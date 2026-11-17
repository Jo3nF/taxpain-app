-- Extractos importados (G36) e incentivos fiscales del inmovilizado (G35).

-- Cada extracto que se importa: de qué cuenta y fichero, su periodo y saldos, y cuántos
-- movimientos traía y cuántos entraron nuevos. `bank_statement_tx` dice qué movimientos traía
-- (los nuevos y los que ya estaban por otro extracto que se solapa): al deshacer un extracto se
-- borran sólo los que no trae ningún otro.
CREATE TABLE bank_statement (
  id            INTEGER PRIMARY KEY,
  bank_account  INTEGER NOT NULL REFERENCES bank_account(id),
  fichero       TEXT NOT NULL,
  formato       TEXT NOT NULL DEFAULT '',
  desde         TEXT,
  hasta         TEXT,
  saldo_inicial INTEGER,
  saldo_final   INTEGER,
  movimientos   INTEGER NOT NULL DEFAULT 0,
  nuevos        INTEGER NOT NULL DEFAULT 0,
  importado_en  TEXT NOT NULL,
  -- 1: rehecho al migrar con los movimientos de antes (por `bank_tx.source`).
  heredado      INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE INDEX bank_statement_cuenta ON bank_statement(bank_account, desde);

CREATE TABLE bank_statement_tx (
  statement_id INTEGER NOT NULL REFERENCES bank_statement(id) ON DELETE CASCADE,
  tx_id        INTEGER NOT NULL REFERENCES bank_tx(id) ON DELETE CASCADE,
  PRIMARY KEY (statement_id, tx_id)
) STRICT;
CREATE INDEX bank_statement_tx_tx ON bank_statement_tx(tx_id);

-- Los movimientos importados antes: un extracto por cuenta y fichero de origen.
INSERT INTO bank_statement(bank_account, fichero, desde, hasta, saldo_inicial, saldo_final,
                           movimientos, nuevos, importado_en, heredado)
SELECT t.bank_account, t.source, min(t.op_date), max(t.op_date),
       (SELECT f.balance - f.amount FROM bank_tx f
         WHERE f.bank_account = t.bank_account AND f.source = t.source
         ORDER BY f.op_date, f.id LIMIT 1),
       (SELECT l.balance FROM bank_tx l
         WHERE l.bank_account = t.bank_account AND l.source = t.source
         ORDER BY l.op_date DESC, l.id DESC LIMIT 1),
       count(*), count(*), min(t.created_at), 1
  FROM bank_tx t
 WHERE t.source IS NOT NULL AND t.source <> ''
 GROUP BY t.bank_account, t.source;
INSERT INTO bank_statement_tx(statement_id, tx_id)
SELECT s.id, t.id FROM bank_statement s
  JOIN bank_tx t ON t.bank_account = s.bank_account AND t.source = s.fichero;

-- Incentivo fiscal con el que se amortiza un elemento (su coeficiente fiscal va en
-- `fixed_asset.tax_rate_bp`): `escaso_valor` (art. 12.3.e LIS), `erd_empleo` (libertad de
-- amortización con creación de empleo, art. 102 LIS; `param` = incremento de la plantilla media
-- en centésimas), `erd_acelerada` (art. 103 LIS; `param` = coeficiente lineal máximo de tablas
-- en puntos básicos) u `otro` (libertad de amortización de otro precepto, sin topes que mirar).
CREATE TABLE asset_tax_incentive (
  asset_id   INTEGER PRIMARY KEY REFERENCES fixed_asset(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL CHECK (kind IN ('escaso_valor','erd_empleo','erd_acelerada','otro')),
  param      INTEGER,
  created_at TEXT NOT NULL
) STRICT;

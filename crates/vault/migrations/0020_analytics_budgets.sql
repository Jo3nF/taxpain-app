-- Contabilidad analítica (centros de coste) y presupuestos. Van aparte del diario: repartir un
-- gasto entre centros o presupuestar no cambia los libros, así que vale también en ejercicios
-- cerrados.
CREATE TABLE cost_center (
  code       TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  created_at TEXT NOT NULL
) STRICT;

-- Reparto por defecto de las cuentas que empiezan por `prefix` (6…, 7…): suma 100 %.
CREATE TABLE cost_rule (
  prefix  TEXT NOT NULL,
  center  TEXT NOT NULL REFERENCES cost_center(code) ON DELETE CASCADE,
  pct_bp  INTEGER NOT NULL CHECK (pct_bp > 0 AND pct_bp <= 10000),  -- centésimas de punto
  PRIMARY KEY (prefix, center)
) STRICT;

-- Reparto de una línea concreta (manda sobre la regla).
CREATE TABLE line_cost (
  line_id INTEGER NOT NULL REFERENCES journal_line(id) ON DELETE CASCADE,
  center  TEXT NOT NULL REFERENCES cost_center(code) ON DELETE CASCADE,
  pct_bp  INTEGER NOT NULL CHECK (pct_bp > 0 AND pct_bp <= 10000),
  PRIMARY KEY (line_id, center)
) STRICT;

-- Presupuesto mensual por cuenta (o prefijo) y, si se quiere, por centro ('' = toda la empresa).
CREATE TABLE budget (
  year    INTEGER NOT NULL,
  account TEXT NOT NULL,
  center  TEXT NOT NULL DEFAULT '',
  month   INTEGER NOT NULL CHECK (month BETWEEN 1 AND 12),
  amount  INTEGER NOT NULL,
  PRIMARY KEY (year, account, center, month)
) STRICT;

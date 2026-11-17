-- Reglas del banco: un texto del concepto del movimiento (normalizado) y su sentido llevan a una
-- cuenta. Se aprenden al contabilizar un movimiento contra una cuenta y se pueden borrar.
CREATE TABLE bank_rule (
  id         INTEGER PRIMARY KEY,
  pattern    TEXT NOT NULL CHECK (length(pattern) >= 3),
  sign       TEXT NOT NULL CHECK (sign IN ('cargo', 'abono')),
  account    TEXT NOT NULL,
  hits       INTEGER NOT NULL DEFAULT 1,
  updated_at TEXT NOT NULL,
  UNIQUE (pattern, sign)
) STRICT;

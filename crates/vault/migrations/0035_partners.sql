-- Entidades en régimen de atribución de rentas (comunidades de bienes, sociedades civiles):
-- socios o comuneros con su participación (diezmilésimas de punto: 100 % = 1.000.000) para
-- atribuirles las rentas en el modelo 184.
CREATE TABLE partner (
  id         INTEGER PRIMARY KEY,
  nif        TEXT NOT NULL,
  name       TEXT NOT NULL,
  province   TEXT NOT NULL,
  address    TEXT NOT NULL DEFAULT '',
  pct        INTEGER NOT NULL CHECK (pct > 0 AND pct <= 1000000),
  joined_on  TEXT,
  left_on    TEXT,
  created_at TEXT NOT NULL
) STRICT;

-- taxpain:sin-claves-foraneas
-- Inversiones en valores: las opciones (y otros derivados con prima) como clase de valor
-- propia, `derivado`, para llevar su resultado en una línea aparte del informe.
--
-- SQLite no deja cambiar un CHECK sin rehacer la tabla: se rehace `security` con los mismos `id`
-- (a los que apuntan `security_op` y `security_price`). Se aplica con las claves foráneas
-- desactivadas (la primera línea lo pide al ejecutor), que las comprueba antes de confirmarla.
DROP TABLE IF EXISTS security_new;
CREATE TABLE security_new (
  id          INTEGER PRIMARY KEY,
  isin        TEXT NOT NULL DEFAULT '',
  ticker      TEXT NOT NULL,
  name        TEXT NOT NULL DEFAULT '',
  currency    TEXT NOT NULL DEFAULT 'EUR',
  kind        TEXT NOT NULL DEFAULT 'accion'
              CHECK (kind IN ('accion','etf','fondo','bono','cfd','derivado','otro')),
  country     TEXT NOT NULL DEFAULT '',  -- país del emisor (del ISIN o del bróker)
  account     TEXT NOT NULL DEFAULT '',  -- subcuenta contable asignada
  created_at  TEXT NOT NULL
) STRICT;
INSERT INTO security_new(id, isin, ticker, name, currency, kind, country, account, created_at)
  SELECT id, isin, ticker, name, currency, kind, country, account, created_at FROM security;
DROP TABLE security;
ALTER TABLE security_new RENAME TO security;
CREATE UNIQUE INDEX security_isin ON security(isin) WHERE isin <> '';

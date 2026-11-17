-- taxpain:sin-claves-foraneas
-- Extractos de cualquier banco, formato y divisa. Una cuenta es su clave (`number`) y su
-- divisa juntas: Revolut, Wise… tienen un mismo IBAN con varias divisas, y cada una es una
-- cuenta aparte, con su subcuenta. Además, qué es (cuenta, tarjeta de crédito, de débito o
-- monedero como PayPal), el banco y el titular.
--
-- SQLite no deja quitar el UNIQUE (number): se rehace la tabla con los mismos `id` (a los que
-- apunta `bank_tx.bank_account`). Esta migración se aplica con las claves foráneas
-- desactivadas (la primera línea lo pide al ejecutor), que las comprueba antes de confirmarla.
DROP TABLE IF EXISTS bank_account_new;
CREATE TABLE bank_account_new (
  id        INTEGER PRIMARY KEY,
  iban      TEXT,
  bank_code TEXT,
  number    TEXT NOT NULL,                  -- clave: entidad+oficina+cuenta (N43), IBAN, tarjeta:…
  account   TEXT NOT NULL,                  -- 572x; 573x otra divisa; 5201x crédito; '' débito
  name      TEXT NOT NULL,
  currency  TEXT NOT NULL DEFAULT 'EUR',    -- ISO 4217
  kind      TEXT NOT NULL DEFAULT 'cuenta'
            CHECK (kind IN ('cuenta','tarjeta_credito','tarjeta_debito','monedero')),
  bank      TEXT NOT NULL DEFAULT '',       -- nombre del banco
  holder    TEXT NOT NULL DEFAULT '',       -- titular
  UNIQUE (number, currency)
) STRICT;
INSERT INTO bank_account_new(id, iban, bank_code, number, account, name)
  SELECT id, iban, bank_code, number, account, name FROM bank_account;
DROP TABLE bank_account;
ALTER TABLE bank_account_new RENAME TO bank_account;

-- Saldo después del movimiento, si el extracto lo da (sirve para no duplicar movimientos
-- iguales del mismo día entre extractos que se solapan), y de qué extracto salió.
ALTER TABLE bank_tx ADD COLUMN balance INTEGER;
ALTER TABLE bank_tx ADD COLUMN source TEXT;
CREATE INDEX IF NOT EXISTS bank_tx_account_date ON bank_tx(bank_account, op_date);

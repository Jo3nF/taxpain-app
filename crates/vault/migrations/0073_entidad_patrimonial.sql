-- Entidad patrimonial (art. 5.2 LIS) y patrimonio neto frente al capital (arts. 327 y 363.1.e
-- LSC): lo que el usuario marca y no sale de la contabilidad.
--
-- `patrimonial_marca`: clase de una partida del activo (una subcuenta de los grupos 2 y 5, o una
-- cuenta de tres cifras de los grupos 3 y 4) desde el ejercicio `year`; vale para los siguientes
-- mientras no se marque otra cosa. Sin marca, la clase es la que propone TaxPain por la cuenta.
CREATE TABLE IF NOT EXISTS patrimonial_marca (
  year       INTEGER NOT NULL,
  cuenta     TEXT NOT NULL,
  clase      TEXT NOT NULL CHECK (clase IN ('afecto','no_afecto','valores','participacion','tesoreria')),
  motivo     TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL,
  PRIMARY KEY (year, cuenta)
) STRICT;

-- Ajustes del ejercicio: tesorería que se justifica como afecta, beneficios no distribuidos de
-- la actividad (NULL: sin confirmar), préstamos participativos (computan en el patrimonio neto a
-- efectos de reducción de capital y disolución, art. 20.1.d RDL 7/1996) y si la sociedad es
-- dominante de un grupo del art. 42 CCom (el test va con los balances consolidados).
CREATE TABLE IF NOT EXISTS patrimonial_ejercicio (
  year                       INTEGER PRIMARY KEY,
  tesoreria_afecta           INTEGER NOT NULL DEFAULT 0 CHECK (tesoreria_afecta >= 0),
  tesoreria_motivo           TEXT NOT NULL DEFAULT '',
  beneficios_actividad       INTEGER CHECK (beneficios_actividad IS NULL OR beneficios_actividad >= 0),
  beneficios_motivo          TEXT NOT NULL DEFAULT '',
  prestamos_participativos   INTEGER NOT NULL DEFAULT 0 CHECK (prestamos_participativos >= 0),
  dominante                  INTEGER NOT NULL DEFAULT 0 CHECK (dominante IN (0, 1)),
  updated_at                 TEXT NOT NULL
) STRICT;

-- Ventas de elementos afectos o de participaciones que no computan: el dinero o el crédito que
-- dejan no computa en el ejercicio de la venta ni en los dos siguientes (art. 5.2 LIS).
CREATE TABLE IF NOT EXISTS patrimonial_venta (
  id         INTEGER PRIMARY KEY,
  fecha      TEXT NOT NULL,                      -- AAAA-MM-DD
  importe    INTEGER NOT NULL CHECK (importe > 0), -- céntimos
  motivo     TEXT NOT NULL,
  created_at TEXT NOT NULL
) STRICT;

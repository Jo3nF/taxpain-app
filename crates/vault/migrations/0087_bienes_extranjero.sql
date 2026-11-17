-- Modelos 720 y 721 (G19): datos de cada bien en el extranjero que la contabilidad no tiene
-- (condición del declarante, fecha de apertura, NIF y domicilio de la entidad o del custodio) y
-- lo declarado otros años, para la regla de los 20.000 € de incremento y de las extinciones.

-- Un bien: una cuenta bancaria (`banco`, bank_account.id), una cuenta de valores (`valores`,
-- security_account.id) o una plataforma de criptomonedas (`cripto`, crypto_platform.id).
CREATE TABLE foreign_asset_info (
  id            INTEGER PRIMARY KEY,
  source        TEXT NOT NULL CHECK (source IN ('banco','valores','cripto')),
  ref_id        INTEGER NOT NULL,
  -- titular, representante, autorizado, beneficiario, usufructuario, tomador, poder_disposicion,
  -- titular_real, otra.
  condition     TEXT NOT NULL DEFAULT 'titular',
  -- Porcentaje de participación en centésimas (10000 = 100 %).
  share_bp      INTEGER NOT NULL DEFAULT 10000 CHECK (share_bp BETWEEN 1 AND 10000),
  -- Apertura de la cuenta o primera adquisición (NULL: la del primer movimiento).
  opened_on     TEXT,
  -- Cancelación o extinción de la titularidad (NULL: sigue).
  closed_on     TEXT,
  entity_name   TEXT NOT NULL DEFAULT '',
  -- NIF de la entidad o del custodio en su país.
  entity_tax_id TEXT NOT NULL DEFAULT '',
  bic           TEXT NOT NULL DEFAULT '',
  address       TEXT NOT NULL DEFAULT '',
  -- País (ISO 3166-1 alfa-2); vacío = el del IBAN o el de la cuenta o plataforma.
  country       TEXT NOT NULL DEFAULT '',
  -- Sólo cuentas bancarias: 1 la cuenta es del extranjero aunque el IBAN no lo diga, 0 no lo es
  -- aunque lo diga, NULL según el IBAN.
  abroad        INTEGER CHECK (abroad IN (0,1)),
  updated_at    TEXT NOT NULL,
  UNIQUE (source, ref_id)
) STRICT;

-- Lo declarado en un 720 o un 721 por bloque: el valor que determinó la presentación (y, en las
-- cuentas, el saldo medio del último trimestre) y la lista de bienes declarados (JSON con su
-- identificación), para saber si al año siguiente hay que volver a presentar.
CREATE TABLE foreign_asset_filing (
  id          INTEGER PRIMARY KEY,
  model       TEXT NOT NULL CHECK (model IN ('720','721')),
  year        INTEGER NOT NULL,
  -- cuentas, valores (720) o monedas (721).
  block       TEXT NOT NULL CHECK (block IN ('cuentas','valores','monedas')),
  value       INTEGER NOT NULL,
  avg_balance INTEGER,
  assets      TEXT NOT NULL DEFAULT '[]',
  -- borrador (marcado como presentado desde TaxPain) o tecleado (de un año anterior).
  origin      TEXT NOT NULL CHECK (origin IN ('borrador','tecleado')),
  created_at  TEXT NOT NULL,
  UNIQUE (model, year, block)
) STRICT;

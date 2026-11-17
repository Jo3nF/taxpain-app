-- Libros societarios (G08 y G09): libro registro de socios (arts. 104-106 LSC), libro registro
-- de contratos del socio único con la sociedad (art. 16 LSC) y libro de actas (arts. 26 CCom,
-- 202 LSC y 97-106 RRM). Son libros que se legalizan en el Registro Mercantil (art. 18 Ley
-- 14/2013; Instrucción DGRN de 12/2/2015).

-- Movimientos de las participaciones, en orden cronológico: la situación actual (quién tiene
-- qué numeración) sale de reproducirlos. `from_*` es el transmitente o el titular de las
-- participaciones gravadas; `to_*` el adquirente, el suscriptor o el beneficiario de la carga.
-- `unit_nominal` en céntimos por participación; `price`, el precio o valor declarado.
CREATE TABLE IF NOT EXISTS share_movement (
  id             INTEGER PRIMARY KEY,
  date           TEXT NOT NULL,
  kind           TEXT NOT NULL CHECK (kind IN ('constitucion','ampliacion','reduccion',
                   'transmision','carga','cancelacion_carga')),
  from_nif       TEXT NOT NULL DEFAULT '',
  from_name      TEXT NOT NULL DEFAULT '',
  to_nif         TEXT NOT NULL DEFAULT '',
  to_name        TEXT NOT NULL DEFAULT '',
  to_address     TEXT NOT NULL DEFAULT '',
  to_nationality TEXT NOT NULL DEFAULT '',
  number_from    INTEGER NOT NULL CHECK (number_from >= 1),
  number_to      INTEGER NOT NULL CHECK (number_to >= number_from),
  unit_nominal   INTEGER NOT NULL CHECK (unit_nominal > 0),
  price          INTEGER CHECK (price IS NULL OR price >= 0),
  charge         TEXT NOT NULL DEFAULT '',
  title          TEXT NOT NULL DEFAULT '',
  notary         TEXT NOT NULL DEFAULT '',
  protocol       TEXT NOT NULL DEFAULT '',
  title_date     TEXT,
  notes          TEXT NOT NULL DEFAULT '',
  created_at     TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS share_movement_by_date ON share_movement(date, id);

-- Contratos entre el socio único y la sociedad. `source`: la propuesta de TaxPain de la que
-- salió (nóminas, dividendo…), para no proponerla otra vez.
CREATE TABLE IF NOT EXISTS sole_partner_contract (
  id         INTEGER PRIMARY KEY,
  date       TEXT NOT NULL,
  object     TEXT NOT NULL,
  amount     INTEGER CHECK (amount IS NULL OR amount >= 0),
  conditions TEXT NOT NULL DEFAULT '',
  document   TEXT NOT NULL DEFAULT '',
  source     TEXT UNIQUE,
  created_at TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS sole_partner_contract_by_date ON sole_partner_contract(date, id);

-- Actas y decisiones. `number`: correlativo dentro del ejercicio, que se da al aprobarla (la
-- aprobada ya no se edita ni se borra; se puede reabrir con confirmación).
CREATE TABLE IF NOT EXISTS minute (
  id          INTEGER PRIMARY KEY,
  number      INTEGER,
  fiscal_year INTEGER NOT NULL,
  date        TEXT NOT NULL,
  place       TEXT NOT NULL DEFAULT '',
  kind        TEXT NOT NULL CHECK (kind IN ('junta_ordinaria','junta_extraordinaria',
                'junta_universal','socio_unico','consejo','administrador')),
  template    TEXT NOT NULL DEFAULT 'libre',
  attendees   TEXT NOT NULL DEFAULT '',
  chair       TEXT NOT NULL DEFAULT '',
  secretary   TEXT NOT NULL DEFAULT '',
  agreements  TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'borrador' CHECK (status IN ('borrador','aprobada')),
  approved_at TEXT,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL,
  UNIQUE (fiscal_year, number)
) STRICT;
CREATE INDEX IF NOT EXISTS minute_by_date ON minute(date, id);

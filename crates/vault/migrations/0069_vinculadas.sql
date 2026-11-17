-- Operaciones vinculadas (art. 18 LIS; arts. 13 a 16 RIS; modelo 232, Orden HFP/816/2017) y
-- ficha mínima de participadas.
--
-- Los socios con el 25 % o más (`shareholder`), los administradores (`employee.administrator`)
-- y las participadas con el 25 % o más (`participada`) son vinculadas sin más: no hace falta
-- repetirlos en `vinculada`. Aquí van los demás (familiares, entidades del grupo, una persona
-- marcada desde su ficha de cliente o proveedor) y los datos que se quieran añadir a uno de
-- aquéllos (vínculo distinto, grupo fiscal, clave de sus facturas).

-- `nif`: normalizado (letras y cifras en mayúsculas), uno por persona o entidad.
-- `vinculo`: socio, administrador, familiar, participada, participada_indirecta, grupo,
-- administrador_grupo, socios_comunes, establecimiento_permanente.
-- `grupo_fiscal`: del mismo grupo de consolidación fiscal, o AIE/UTE con sus miembros: sus
-- operaciones no van al 232 (Orden HFP/816/2017, art. 2.2).
-- `clave_emitidas` y `clave_recibidas`: tipo de operación (1 a 11 del 232) de sus facturas.
CREATE TABLE IF NOT EXISTS vinculada (
  id              INTEGER PRIMARY KEY,
  nif             TEXT NOT NULL UNIQUE,
  name            TEXT NOT NULL,
  legal_person    INTEGER NOT NULL DEFAULT 0 CHECK (legal_person IN (0,1)),
  province        TEXT NOT NULL DEFAULT '',
  country         TEXT NOT NULL DEFAULT 'ES',
  vinculo         TEXT NOT NULL,
  detalle         TEXT NOT NULL DEFAULT '',
  pct             INTEGER CHECK (pct IS NULL OR (pct > 0 AND pct <= 1000000)),
  grupo_fiscal    INTEGER NOT NULL DEFAULT 0 CHECK (grupo_fiscal IN (0,1)),
  clave_emitidas  INTEGER NOT NULL DEFAULT 6 CHECK (clave_emitidas BETWEEN 1 AND 11),
  clave_recibidas INTEGER NOT NULL DEFAULT 6 CHECK (clave_recibidas BETWEEN 1 AND 11),
  active          INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0,1)),
  created_at      TEXT NOT NULL
) STRICT;

-- Participadas de la sociedad (ficha mínima; la completa con valor fiscal, fondos propios y
-- dividendos por año es la carencia G15). Participación en millonésimas, coste en céntimos.
CREATE TABLE IF NOT EXISTS participada (
  id           INTEGER PRIMARY KEY,
  nif          TEXT NOT NULL UNIQUE,
  name         TEXT NOT NULL,
  province     TEXT NOT NULL DEFAULT '',
  country      TEXT NOT NULL DEFAULT 'ES',
  pct          INTEGER NOT NULL CHECK (pct > 0 AND pct <= 1000000),
  acquired     TEXT NOT NULL,
  cost         INTEGER NOT NULL CHECK (cost >= 0),
  account      TEXT NOT NULL DEFAULT '',
  grupo_fiscal INTEGER NOT NULL DEFAULT 0 CHECK (grupo_fiscal IN (0,1)),
  notes        TEXT NOT NULL DEFAULT '',
  active       INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0,1)),
  created_at   TEXT NOT NULL
) STRICT;

-- Subcuentas que son de una vinculada (5510001 de la socia, 2420001 del préstamo a la
-- participada, 7620001 de sus intereses…): sus movimientos del año entran en el registro.
CREATE TABLE IF NOT EXISTS vinculada_cuenta (
  account    TEXT PRIMARY KEY,
  nif        TEXT NOT NULL,
  created_at TEXT NOT NULL
) STRICT;

-- Operaciones que TaxPain no ve en facturas, nóminas ni cuentas (compraventa de un inmueble o
-- de participaciones, cesión de un intangible, garantías…), tecleadas por año.
CREATE TABLE IF NOT EXISTS vinculada_operacion (
  id          INTEGER PRIMARY KEY,
  year        INTEGER NOT NULL,
  nif         TEXT NOT NULL,
  clave       INTEGER NOT NULL CHECK (clave BETWEEN 1 AND 11),
  ingreso     INTEGER NOT NULL CHECK (ingreso IN (0,1)),
  amount      INTEGER NOT NULL CHECK (amount > 0),
  especifica  INTEGER NOT NULL DEFAULT 0 CHECK (especifica IN (0,1)),
  description TEXT NOT NULL DEFAULT '',
  created_at  TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS vinculada_operacion_by_year ON vinculada_operacion(year);

-- Valoración de cada grupo de operaciones (año, vinculada, concepto, ingreso o pago): método
-- del art. 18.4 LIS y lo que escribe el usuario para la documentación simplificada.
CREATE TABLE IF NOT EXISTS vinculada_valoracion (
  year          INTEGER NOT NULL,
  nif           TEXT NOT NULL,
  concepto      TEXT NOT NULL,
  ingreso       INTEGER NOT NULL CHECK (ingreso IN (0,1)),
  metodo        TEXT NOT NULL,
  descripcion   TEXT NOT NULL DEFAULT '',
  justificacion TEXT NOT NULL DEFAULT '',
  updated_at    TEXT NOT NULL,
  PRIMARY KEY (year, nif, concepto, ingreso)
) STRICT;

-- Textos generales de la documentación simplificada del año (art. 16.4 RIS).
CREATE TABLE IF NOT EXISTS vinculadas_documentacion (
  year          INTEGER PRIMARY KEY,
  actividad     TEXT NOT NULL DEFAULT '',
  justificacion TEXT NOT NULL DEFAULT '',
  updated_at    TEXT NOT NULL
) STRICT;

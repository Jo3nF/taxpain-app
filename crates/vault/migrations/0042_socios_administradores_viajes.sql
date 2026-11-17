-- Sociedades: socios (reparto de dividendos y modelos 123 y 193), pagos de dividendos con su
-- detalle por socio, administradores en Nóminas (clave E del 190) y gastos de viaje (dietas y
-- kilometraje, clave L.01 del 190).

-- Participación en millonésimas (100 % = 1.000.000), como `partner`. `exempt`: sociedad
-- residente con derecho a la exención del art. 21 LIS que lo ha acreditado (sin retención,
-- art. 128.4.d LIS). Los no residentes van al 216/296, no al 123/193.
CREATE TABLE IF NOT EXISTS shareholder (
  id           INTEGER PRIMARY KEY,
  nif          TEXT NOT NULL,
  name         TEXT NOT NULL,
  address      TEXT NOT NULL DEFAULT '',
  province     TEXT NOT NULL DEFAULT '',
  country      TEXT NOT NULL DEFAULT 'ES',
  pct          INTEGER NOT NULL CHECK (pct > 0 AND pct <= 1000000),
  legal_person INTEGER NOT NULL DEFAULT 0 CHECK (legal_person IN (0,1)),
  resident     INTEGER NOT NULL DEFAULT 1 CHECK (resident IN (0,1)),
  exempt       INTEGER NOT NULL DEFAULT 0 CHECK (exempt IN (0,1)),
  active       INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0,1)),
  created_at   TEXT NOT NULL
) STRICT;

-- Un pago de dividendos (526 al debe; 4751 y tesorería al haber) y su reparto por socio, con
-- los datos del socio el día del pago.
CREATE TABLE IF NOT EXISTS dividend_payment (
  id          INTEGER PRIMARY KEY,
  date        TEXT NOT NULL,
  gross       INTEGER NOT NULL CHECK (gross > 0),
  withheld    INTEGER NOT NULL CHECK (withheld >= 0),
  treasury    TEXT NOT NULL,
  entry_id    INTEGER REFERENCES journal_entry(id),
  created_at  TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS dividend_payment_by_date ON dividend_payment(date);

CREATE TABLE IF NOT EXISTS dividend_line (
  id             INTEGER PRIMARY KEY,
  payment_id     INTEGER NOT NULL REFERENCES dividend_payment(id) ON DELETE CASCADE,
  shareholder_id INTEGER,
  nif            TEXT NOT NULL,
  name           TEXT NOT NULL,
  province       TEXT NOT NULL,
  legal_person   INTEGER NOT NULL CHECK (legal_person IN (0,1)),
  resident       INTEGER NOT NULL CHECK (resident IN (0,1)),
  pct            INTEGER NOT NULL,
  gross          INTEGER NOT NULL CHECK (gross >= 0),
  rate_bp        INTEGER NOT NULL CHECK (rate_bp >= 0),
  withheld       INTEGER NOT NULL CHECK (withheld >= 0)
) STRICT;

-- Consejeros y administradores (clave E del 190): régimen de la Seguridad Social y, en cada
-- recibo, la subclave E que le tocaba al pagarlo (NULL: empleado, clave A).
ALTER TABLE employee ADD COLUMN administrator INTEGER NOT NULL DEFAULT 0
  CHECK (administrator IN (0,1));
ALTER TABLE employee ADD COLUMN ss_regime TEXT NOT NULL DEFAULT 'Ninguno';
ALTER TABLE payslip ADD COLUMN director_subkey INTEGER
  CHECK (director_subkey IS NULL OR director_subkey BETWEEN 1 AND 4);

-- Gastos de viaje (art. 9 RIRPF): lo pagado por cada concepto, la parte exceptuada de
-- gravamen (clave L.01 del 190) y la que es retribución (exceso sobre los límites, o dietas y
-- kilometraje de un administrador sin relación laboral). `labor`: relación laboral con la
-- sociedad, sin la cual no se aplica el art. 9 (DGT V1796-25).
CREATE TABLE IF NOT EXISTS travel_expense (
  id              INTEGER PRIMARY KEY,
  date            TEXT NOT NULL,
  nif             TEXT NOT NULL,
  name            TEXT NOT NULL,
  employee_id     INTEGER REFERENCES employee(id),
  origin          TEXT NOT NULL,
  destination     TEXT NOT NULL,
  reason          TEXT NOT NULL,
  labor           INTEGER NOT NULL DEFAULT 1 CHECK (labor IN (0,1)),
  abroad          INTEGER NOT NULL DEFAULT 0 CHECK (abroad IN (0,1)),
  overnight       INTEGER NOT NULL DEFAULT 0 CHECK (overnight IN (0,1)),
  days            INTEGER NOT NULL DEFAULT 0 CHECK (days >= 0),
  km              INTEGER NOT NULL DEFAULT 0 CHECK (km >= 0),
  km_amount       INTEGER NOT NULL DEFAULT 0 CHECK (km_amount >= 0),
  meals           INTEGER NOT NULL DEFAULT 0 CHECK (meals >= 0),
  lodging         INTEGER NOT NULL DEFAULT 0 CHECK (lodging >= 0),
  tolls_parking   INTEGER NOT NULL DEFAULT 0 CHECK (tolls_parking >= 0),
  transport       INTEGER NOT NULL DEFAULT 0 CHECK (transport >= 0),
  exempt          INTEGER NOT NULL CHECK (exempt >= 0),
  taxable         INTEGER NOT NULL CHECK (taxable >= 0),
  counter_account TEXT NOT NULL,
  entry_id        INTEGER REFERENCES journal_entry(id),
  created_at      TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS travel_expense_by_date ON travel_expense(date);

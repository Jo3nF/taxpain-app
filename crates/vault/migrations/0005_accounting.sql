-- Fase 4: contabilidad. Importes en céntimos. Ver docs/DATA_MODEL.md y docs/ACCOUNTING_RULES.md.

CREATE TABLE account (
  code       TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  created_at TEXT NOT NULL
) STRICT;

-- Subcuenta de cada tercero (cliente 430xxxx, proveedor/acreedor 400/410xxxx).
CREATE TABLE third_party_account (
  tax_id   TEXT NOT NULL,
  role     TEXT NOT NULL CHECK (role IN ('cliente','proveedor')),
  account  TEXT NOT NULL UNIQUE,
  name     TEXT NOT NULL,
  PRIMARY KEY (tax_id, role)
) STRICT;

CREATE TABLE fiscal_year (
  year      INTEGER PRIMARY KEY,
  closed    INTEGER NOT NULL DEFAULT 0 CHECK (closed IN (0,1)),
  closed_at TEXT
) STRICT;

CREATE TABLE journal_entry (
  id          INTEGER PRIMARY KEY,
  fiscal_year INTEGER NOT NULL,
  number      INTEGER NOT NULL,
  date        TEXT NOT NULL,
  concept     TEXT NOT NULL,
  origin_kind TEXT NOT NULL,
  origin_id   INTEGER,
  created_at  TEXT NOT NULL,
  UNIQUE (fiscal_year, number)
) STRICT;

-- Un documento se contabiliza una vez por ejercicio (un activo se amortiza cada año).
CREATE UNIQUE INDEX journal_entry_origin ON journal_entry(origin_kind, origin_id, fiscal_year)
  WHERE origin_id IS NOT NULL AND origin_kind IN ('FacturaEmitida','FacturaRecibida','Banco','Amortizacion','LiquidacionIva');

CREATE TABLE journal_line (
  id          INTEGER PRIMARY KEY,
  entry_id    INTEGER NOT NULL REFERENCES journal_entry(id),
  account     TEXT NOT NULL,
  debit       INTEGER NOT NULL CHECK (debit >= 0),
  credit      INTEGER NOT NULL CHECK (credit >= 0),
  description TEXT,
  CHECK ((debit = 0) <> (credit = 0))
) STRICT;

CREATE INDEX journal_line_account ON journal_line(account);

-- Ejercicio cerrado: no se tocan sus asientos.
CREATE TRIGGER journal_entry_closed_insert BEFORE INSERT ON journal_entry
WHEN (SELECT closed FROM fiscal_year WHERE year = NEW.fiscal_year) = 1
BEGIN SELECT RAISE(ABORT, 'el ejercicio está cerrado'); END;

CREATE TRIGGER journal_entry_closed_update BEFORE UPDATE ON journal_entry
WHEN (SELECT closed FROM fiscal_year WHERE year = OLD.fiscal_year) = 1
BEGIN SELECT RAISE(ABORT, 'el ejercicio está cerrado'); END;

CREATE TRIGGER journal_entry_closed_delete BEFORE DELETE ON journal_entry
WHEN (SELECT closed FROM fiscal_year WHERE year = OLD.fiscal_year) = 1
BEGIN SELECT RAISE(ABORT, 'el ejercicio está cerrado'); END;

CREATE TRIGGER journal_line_closed BEFORE UPDATE ON journal_line
WHEN (SELECT f.closed FROM fiscal_year f JOIN journal_entry e ON e.fiscal_year = f.year WHERE e.id = OLD.entry_id) = 1
BEGIN SELECT RAISE(ABORT, 'el ejercicio está cerrado'); END;

CREATE TRIGGER journal_line_closed_delete BEFORE DELETE ON journal_line
WHEN (SELECT f.closed FROM fiscal_year f JOIN journal_entry e ON e.fiscal_year = f.year WHERE e.id = OLD.entry_id) = 1
BEGIN SELECT RAISE(ABORT, 'el ejercicio está cerrado'); END;

CREATE TABLE fixed_asset (
  id               INTEGER PRIMARY KEY,
  name             TEXT NOT NULL,
  account          TEXT NOT NULL,           -- 21x / 20x
  depreciation_acc TEXT NOT NULL,           -- 281x / 280x
  expense_acc      TEXT NOT NULL,           -- 681 / 680
  acquired_on      TEXT NOT NULL,
  cost             INTEGER NOT NULL CHECK (cost > 0),
  residual         INTEGER NOT NULL DEFAULT 0,
  rate_bp          INTEGER NOT NULL CHECK (rate_bp > 0 AND rate_bp <= 10000),  -- coeficiente anual
  disposed_on      TEXT,
  created_at       TEXT NOT NULL
) STRICT;

CREATE TABLE bank_account (
  id        INTEGER PRIMARY KEY,
  iban      TEXT,
  bank_code TEXT,
  number    TEXT NOT NULL,                  -- entidad+oficina+cuenta (N43)
  account   TEXT NOT NULL,                  -- subcuenta 572x
  name      TEXT NOT NULL,
  UNIQUE (number)
) STRICT;

CREATE TABLE bank_tx (
  id            INTEGER PRIMARY KEY,
  bank_account  INTEGER NOT NULL REFERENCES bank_account(id),
  op_date       TEXT NOT NULL,
  value_date    TEXT NOT NULL,
  amount        INTEGER NOT NULL,           -- + ingreso, − cargo
  concept_code  TEXT,
  description   TEXT NOT NULL,
  import_hash   TEXT NOT NULL UNIQUE,       -- evita duplicar al reimportar
  entry_id      INTEGER REFERENCES journal_entry(id),
  matched_kind  TEXT CHECK (matched_kind IN ('emitida','recibida','cuenta')),
  matched_id    INTEGER,
  created_at    TEXT NOT NULL
) STRICT;

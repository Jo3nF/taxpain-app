-- Nóminas: fichas de empleado (con los datos de la clave A del 190) y recibos de salarios, cada
-- uno con su asiento (640/642 a 4751/476/465).
CREATE TABLE employee (
  id               INTEGER PRIMARY KEY,
  nif              TEXT NOT NULL UNIQUE,
  name             TEXT NOT NULL,
  province         TEXT NOT NULL DEFAULT '',
  birth_year       INTEGER NOT NULL DEFAULT 0,
  family           INTEGER NOT NULL DEFAULT 3 CHECK (family BETWEEN 1 AND 3),
  spouse_nif       TEXT,
  disability       INTEGER NOT NULL DEFAULT 0 CHECK (disability BETWEEN 0 AND 3),
  contract         INTEGER NOT NULL DEFAULT 1 CHECK (contract BETWEEN 1 AND 4),
  mobility         INTEGER NOT NULL DEFAULT 0 CHECK (mobility IN (0,1)),
  children_under3  INTEGER NOT NULL DEFAULT 0,
  children_rest    INTEGER NOT NULL DEFAULT 0,
  children_whole   INTEGER NOT NULL DEFAULT 0 CHECK (children_whole IN (0,1)),
  active           INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0,1)),
  updated_at       TEXT NOT NULL
) STRICT;

CREATE TABLE payslip (
  id           INTEGER PRIMARY KEY,
  employee_id  INTEGER NOT NULL REFERENCES employee(id),
  date         TEXT NOT NULL,
  gross        INTEGER NOT NULL CHECK (gross > 0),
  irpf         INTEGER NOT NULL CHECK (irpf >= 0),
  ss_employee  INTEGER NOT NULL CHECK (ss_employee >= 0),
  ss_company   INTEGER NOT NULL CHECK (ss_company >= 0),
  entry_id     INTEGER REFERENCES journal_entry(id),
  created_at   TEXT NOT NULL
) STRICT;
CREATE INDEX payslip_by_date ON payslip(date);

-- Fase 1: configuración, empresa, certificados y auditoría. Ver docs/DATA_MODEL.md.

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;

CREATE TABLE company (
  id                INTEGER PRIMARY KEY,
  nif               TEXT NOT NULL UNIQUE,
  name              TEXT NOT NULL,
  legal_form        TEXT NOT NULL CHECK (legal_form IN
                      ('autonomo','sl','slu','sa','cb','scp','cooperativa','asociacion','otra')),
  vat_regime        TEXT NOT NULL DEFAULT 'general' CHECK (vat_regime IN
                      ('general','simplificado','recargo_equivalencia','rebu','agricultura','exento')),
  iae_epigraphs     TEXT NOT NULL DEFAULT '[]',
  cnae              TEXT,
  territory         TEXT NOT NULL DEFAULT 'comun' CHECK (territory IN
                      ('comun','alava','bizkaia','gipuzkoa','navarra','canarias','ceuta_melilla')),
  fiscal_year_start TEXT NOT NULL DEFAULT '01-01',
  redeme            INTEGER NOT NULL DEFAULT 0 CHECK (redeme IN (0,1)),
  sii               INTEGER NOT NULL DEFAULT 0 CHECK (sii IN (0,1)),
  cash_criterion    INTEGER NOT NULL DEFAULT 0 CHECK (cash_criterion IN (0,1)),
  irpf_method       TEXT CHECK (irpf_method IN ('ed_normal','ed_simplificada','modulos')),
  created_at        TEXT NOT NULL
) STRICT;

CREATE TABLE certificate_ref (
  id                 INTEGER PRIMARY KEY,
  fingerprint_sha256 TEXT NOT NULL UNIQUE,
  subject_dn         TEXT NOT NULL,
  issuer_dn          TEXT NOT NULL,
  serial             TEXT NOT NULL,
  valid_from         TEXT NOT NULL,
  valid_to           TEXT NOT NULL,
  kind               TEXT NOT NULL CHECK (kind IN ('persona_fisica','representante_pj','otro')),
  issuer_kind        TEXT NOT NULL CHECK (issuer_kind IN ('fnmt','otro')),
  holder_name        TEXT,
  holder_nif         TEXT NOT NULL,
  entity_nif         TEXT,
  entity_name        TEXT,
  key_algorithm      TEXT NOT NULL,
  created_at         TEXT NOT NULL,
  last_used_at       TEXT
) STRICT;

-- Clave privada (PKCS#8) + cadena DER, selladas con la clave `cert-seal` (D5).
CREATE TABLE certificate_secret (
  certificate_id INTEGER PRIMARY KEY REFERENCES certificate_ref(id) ON DELETE CASCADE,
  sealed         BLOB NOT NULL
) STRICT;

-- Append-only con hash encadenado. Los triggers impiden modificar o borrar entradas.
CREATE TABLE audit_log (
  seq       INTEGER PRIMARY KEY,
  at        TEXT NOT NULL,
  actor     TEXT NOT NULL CHECK (actor IN ('user','system','assistant')),
  action    TEXT NOT NULL,
  subject   TEXT NOT NULL,
  details   TEXT NOT NULL,
  prev_hash TEXT NOT NULL,
  hash      TEXT NOT NULL UNIQUE
) STRICT;

CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit_log es de solo anexado'); END;

CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit_log es de solo anexado'); END;

-- Autónomo societario (G18) y retribución del administrador (G17).
--
-- `reta_ficha`: datos del RETA de un administrador o socio con control (ficha de Nóminas con
-- `ss_regime = 'Autonomo'`) para un año: base de cotización elegida, rendimiento neto mensual
-- previsto comunicado a la TGSS, meses de alta, cómo se computan los dividendos, rendimientos y
-- gastos de fuera de la sociedad, quién paga la cuota y con qué cuentas, y el acuerdo de la
-- junta que fija la retribución del cargo (art. 217 LSC). Importes en céntimos.
CREATE TABLE IF NOT EXISTS reta_ficha (
  employee_id          INTEGER NOT NULL REFERENCES employee(id) ON DELETE CASCADE,
  year                 INTEGER NOT NULL,
  base_elegida         INTEGER NOT NULL CHECK (base_elegida >= 0),
  rendimiento_previsto INTEGER,
  meses_alta           INTEGER NOT NULL DEFAULT 12 CHECK (meses_alta BETWEEN 1 AND 12),
  control_efectivo     INTEGER NOT NULL DEFAULT 1 CHECK (control_efectivo IN (0,1)),
  pct_dividendos_bp    INTEGER NOT NULL DEFAULT 10000
    CHECK (pct_dividendos_bp BETWEEN 0 AND 10000),
  otros_rendimientos   INTEGER NOT NULL DEFAULT 0,
  gastos_deducibles    INTEGER NOT NULL DEFAULT 0 CHECK (gastos_deducibles >= 0),
  pagador              TEXT NOT NULL DEFAULT 'Sociedad'
    CHECK (pagador IN ('Sociedad', 'Administrador')),
  cuenta_gasto         TEXT NOT NULL DEFAULT '642',
  cuenta_tesoreria     TEXT NOT NULL DEFAULT '572',
  estatutos_retribuido INTEGER NOT NULL DEFAULT 0 CHECK (estatutos_retribuido IN (0,1)),
  acuerdo_fecha        TEXT,
  acuerdo_importe      INTEGER CHECK (acuerdo_importe IS NULL OR acuerdo_importe >= 0),
  acuerdo_nota         TEXT NOT NULL DEFAULT '',
  updated_at           TEXT NOT NULL,
  PRIMARY KEY (employee_id, year)
) STRICT;

-- Cuotas del RETA pagadas (una por mes): quién la pagó y, si fue la sociedad y se contabilizó
-- desde aquí, su asiento (cuenta de gasto al debe, tesorería al haber).
CREATE TABLE IF NOT EXISTS reta_cuota (
  id          INTEGER PRIMARY KEY,
  employee_id INTEGER NOT NULL REFERENCES employee(id),
  date        TEXT NOT NULL,
  amount      INTEGER NOT NULL CHECK (amount > 0),
  pagador     TEXT NOT NULL CHECK (pagador IN ('Sociedad', 'Administrador')),
  entry_id    INTEGER REFERENCES journal_entry(id),
  created_at  TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS reta_cuota_by_employee ON reta_cuota(employee_id, date);

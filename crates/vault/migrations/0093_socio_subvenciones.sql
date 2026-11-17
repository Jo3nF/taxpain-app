-- Autónomo societario: fecha de alta en el RETA como societario. Con menos de noventa días de
-- alta como tal en el año no se aplican la deducción del 3 % ni la base mínima de los
-- societarios (art. 308.1.a, regla 4.ª, y c, regla 2.ª, TRLGSS; DT 7.ª RDL 13/2022). NULL: de
-- años anteriores.
ALTER TABLE reta_ficha ADD COLUMN alta_societario TEXT;

-- Subvenciones, donaciones y legados recibidos (G41; PGC y PGC de PYMES, NRV 18.ª). Importes en
-- céntimos. `tipo`: 'capital' (130, a resultados con la 746 al ritmo de la amortización del
-- bien), 'donacion' (131, igual), 'gastos' (132, a la 747 cuando se devengan los gastos que
-- financia) o 'explotacion' (740, directamente a resultados). Mientras sea reintegrable es una
-- deuda (172 o 522). `efecto_impositivo`: la parte del impuesto diferido va a la 479 (pasivo por
-- diferencias temporarias imponibles) y la 13x queda neta.
CREATE TABLE IF NOT EXISTS subvencion (
  id                    INTEGER PRIMARY KEY,
  concedente            TEXT NOT NULL,
  concedente_nif        TEXT NOT NULL DEFAULT '',
  fecha_concesion       TEXT NOT NULL,
  importe               INTEGER NOT NULL CHECK (importe > 0),
  finalidad             TEXT NOT NULL DEFAULT '',
  tipo                  TEXT NOT NULL
    CHECK (tipo IN ('capital', 'donacion', 'gastos', 'explotacion')),
  condiciones           TEXT NOT NULL DEFAULT '',
  reintegrable          INTEGER NOT NULL DEFAULT 0 CHECK (reintegrable IN (0,1)),
  fixed_asset_id        INTEGER REFERENCES fixed_asset(id),
  efecto_impositivo     INTEGER NOT NULL DEFAULT 1 CHECK (efecto_impositivo IN (0,1)),
  tipo_impositivo_bp    INTEGER NOT NULL DEFAULT 2500
    CHECK (tipo_impositivo_bp BETWEEN 0 AND 10000),
  minimis               INTEGER NOT NULL DEFAULT 0 CHECK (minimis IN (0,1)),
  cuenta_deudora        TEXT NOT NULL DEFAULT '4708',
  notas                 TEXT NOT NULL DEFAULT '',
  created_at            TEXT NOT NULL,
  updated_at            TEXT NOT NULL
) STRICT;

-- Asientos de cada subvención: concesión, paso a no reintegrable, cobro e imputación anual a
-- resultados (importe bruto imputado). Uno de cada por subvención, salvo cobros (varios) e
-- imputaciones (una por ejercicio).
CREATE TABLE IF NOT EXISTS subvencion_asiento (
  id            INTEGER PRIMARY KEY,
  subvencion_id INTEGER NOT NULL REFERENCES subvencion(id),
  tipo          TEXT NOT NULL
    CHECK (tipo IN ('concesion', 'no_reintegrable', 'cobro', 'imputacion')),
  fecha         TEXT NOT NULL,
  ejercicio     INTEGER NOT NULL,
  importe       INTEGER NOT NULL CHECK (importe > 0),
  entry_id      INTEGER REFERENCES journal_entry(id),
  created_at    TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS subvencion_asiento_by_subvencion
  ON subvencion_asiento(subvencion_id, tipo);
CREATE UNIQUE INDEX IF NOT EXISTS subvencion_asiento_unico
  ON subvencion_asiento(subvencion_id, tipo) WHERE tipo IN ('concesion', 'no_reintegrable');
CREATE UNIQUE INDEX IF NOT EXISTS subvencion_imputacion_unica
  ON subvencion_asiento(subvencion_id, ejercicio) WHERE tipo = 'imputacion';

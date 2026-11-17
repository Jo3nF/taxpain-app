-- Analítica en varios niveles: cada centro puede colgar de otro (hasta cuatro niveles) y los
-- resultados se suman hacia arriba.
ALTER TABLE cost_center ADD COLUMN parent TEXT REFERENCES cost_center(code) ON DELETE SET NULL;

-- Reparto secundario: lo de un centro (p. ej. servicios generales) se reparte entre otros al
-- final, por porcentaje. Un centro que se reparte no recibe repartos de otros.
CREATE TABLE cost_redistribution (
  source TEXT NOT NULL REFERENCES cost_center(code) ON DELETE CASCADE,
  target TEXT NOT NULL REFERENCES cost_center(code) ON DELETE CASCADE,
  pct_bp INTEGER NOT NULL CHECK (pct_bp > 0 AND pct_bp <= 10000),
  PRIMARY KEY (source, target),
  CHECK (source <> target)
) STRICT;

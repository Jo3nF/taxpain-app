-- Casillas de un modelo fijadas a mano (sustituyen al cálculo hasta que se vuelve al automático).
CREATE TABLE model_adjustment (
  model      TEXT NOT NULL,
  year       INTEGER NOT NULL,
  period     TEXT NOT NULL,
  box        INTEGER NOT NULL,
  amount     INTEGER NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (model, year, period, box)
) STRICT;

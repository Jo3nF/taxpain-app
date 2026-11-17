-- Ejercicio entero del gestor (a3, Sage, ContaSOL…) cargado asiento a asiento para tener el
-- histórico. Cada asiento corriente lleva el lote como origen (`Gestor`); la apertura, la
-- regularización y el cierre del ejercicio, si vienen, van con sus orígenes de siempre y se
-- apuntan aquí para poder deshacer el lote entero. El mismo fichero no se importa dos veces.
CREATE TABLE gestor_import (
  id                   INTEGER PRIMARY KEY,
  fiscal_year          INTEGER NOT NULL,
  source               TEXT NOT NULL CHECK (source IN ('diario','resumido','suenlace')),
  filename             TEXT NOT NULL,
  sha256               TEXT NOT NULL UNIQUE,
  entries              INTEGER NOT NULL,          -- asientos del lote (sin contar los tres de abajo)
  lines                INTEGER NOT NULL,          -- apuntes en total
  accounts_created     INTEGER NOT NULL,          -- cuentas dadas de alta con el lote
  opening_entry        INTEGER REFERENCES journal_entry(id),
  regularization_entry INTEGER REFERENCES journal_entry(id),
  closing_entry        INTEGER REFERENCES journal_entry(id),
  next_opening_entry   INTEGER REFERENCES journal_entry(id), -- apertura del siguiente, si la hizo
  closed_year          INTEGER NOT NULL DEFAULT 0, -- 1: el lote dejó el ejercicio cerrado
  created_at           TEXT NOT NULL
) STRICT;

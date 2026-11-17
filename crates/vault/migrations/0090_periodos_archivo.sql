-- G46 · Bloqueo de periodos: un trimestre con el 303 presentado o un ejercicio cerrado quedan
-- «cerrados». Emitir, registrar o cambiar algo con fecha de un periodo cerrado pide confirmación
-- explícita (no se impide: la ley deja rectificar) y queda en la auditoría.
CREATE TABLE periodo_cerrado (
  id          INTEGER PRIMARY KEY,
  tipo        TEXT NOT NULL CHECK (tipo IN ('trimestre', 'ejercicio')),
  ejercicio   INTEGER NOT NULL,
  -- 1…4 en los trimestres; NULL en los ejercicios.
  trimestre   INTEGER CHECK (trimestre IS NULL OR trimestre BETWEEN 1 AND 4),
  desde       TEXT NOT NULL,                 -- AAAA-MM-DD, incluido
  hasta       TEXT NOT NULL,                 -- AAAA-MM-DD, incluido
  motivo      TEXT NOT NULL DEFAULT '',
  cerrado_en  TEXT NOT NULL,
  CHECK ((tipo = 'trimestre') = (trimestre IS NOT NULL)),
  CHECK (desde <= hasta)
) STRICT;

CREATE UNIQUE INDEX periodo_cerrado_unico ON periodo_cerrado(tipo, ejercicio, coalesce(trimestre, 0));
CREATE INDEX periodo_cerrado_fechas ON periodo_cerrado(desde, hasta);

-- G38 · Archivos del ejercicio generados (el ZIP no se guarda: sólo su huella, para comprobar
-- más adelante que el que se conserva es el mismo).
CREATE TABLE archivo_ejercicio (
  id          INTEGER PRIMARY KEY,
  ejercicio   INTEGER NOT NULL,
  fichero     TEXT NOT NULL,
  sha256      TEXT NOT NULL,
  bytes       INTEGER NOT NULL,
  ficheros    INTEGER NOT NULL,
  creado_en   TEXT NOT NULL
) STRICT;

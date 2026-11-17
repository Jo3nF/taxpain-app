-- Cierre de trimestre guiado (G25), guía «Dejar la gestoría» (G24) y lista de control de las
-- obligaciones no fiscales de la sociedad (G44). Sólo guarda lo que marca el usuario: lo demás
-- (recibidas por revisar, banco sin conciliar, borradores y cuadres) se calcula cada vez.

-- Pasos del cierre de un trimestre que el usuario marca a mano: hecho (p. ej. las nóminas las
-- lleva una gestoría laboral y ya están) o no aplica.
CREATE TABLE IF NOT EXISTS cierre_trimestre_marca (
  year       INTEGER NOT NULL,
  quarter    INTEGER NOT NULL CHECK (quarter BETWEEN 1 AND 4),
  paso       TEXT NOT NULL,
  estado     TEXT NOT NULL CHECK (estado IN ('hecho', 'no_aplica')),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (year, quarter, paso)
) STRICT;

-- Pasos de la guía de puesta en marcha para quien deja la gestoría. `fecha`: la del paso si la
-- tiene (la fecha de corte en el de los saldos iniciales, la de la revocación…).
CREATE TABLE IF NOT EXISTS puesta_marcha_paso (
  paso       TEXT PRIMARY KEY,
  estado     TEXT NOT NULL CHECK (estado IN ('pendiente', 'hecho', 'no_aplica')),
  fecha      TEXT,
  nota       TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL
) STRICT;

-- Obligaciones no fiscales (depósito de cuentas, protección de datos, LSSI…): si aplican a la
-- empresa y si ya están hechas, con la fecha que el usuario quiera recordar (la próxima
-- revisión, el vencimiento de la póliza…). Sin fila: sin revisar.
CREATE TABLE IF NOT EXISTS obligacion_no_fiscal (
  id         TEXT PRIMARY KEY,
  estado     TEXT NOT NULL CHECK (estado IN ('sin_revisar', 'aplica', 'no_aplica', 'hecho')),
  fecha      TEXT,
  nota       TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL
) STRICT;

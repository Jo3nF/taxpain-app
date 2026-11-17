-- Modelo 200 completo (G05) y ficha fiscal de las participadas (G15).
--
-- Importes en céntimos. Los ejercicios son el año en que empieza el periodo impositivo, como en
-- el resto de modelos.

-- Datos fiscales de una participada que no están en su ficha mínima (`participada`, 0069):
-- valor nominal de la participación, valor fiscal (distinto del contable tras una aportación no
-- dineraria o un canje con diferimiento, arts. 76-89 LIS; NULL = igual al coste), cómo se
-- adquirió, y si una no residente tributa al 10 % nominal o más (art. 21.1.b LIS).
CREATE TABLE IF NOT EXISTS participada_fiscal (
  participada_id     INTEGER PRIMARY KEY REFERENCES participada(id) ON DELETE CASCADE,
  nominal            INTEGER NOT NULL DEFAULT 0 CHECK (nominal >= 0),
  valor_fiscal       INTEGER CHECK (valor_fiscal IS NULL OR valor_fiscal >= 0),
  origen             TEXT NOT NULL DEFAULT 'compra'
                       CHECK (origen IN ('compra','constitucion','aportacion','canje')),
  tributacion_minima INTEGER NOT NULL DEFAULT 1 CHECK (tributacion_minima IN (0,1)),
  updated_at         TEXT NOT NULL
) STRICT;

-- Fondos propios y resultado de la participada en un ejercicio (página 02 del 200 y prueba de
-- deterioro), y la corrección valorativa que la declarante contabilizó ese año.
CREATE TABLE IF NOT EXISTS participada_ejercicio (
  participada_id INTEGER NOT NULL REFERENCES participada(id) ON DELETE CASCADE,
  year           INTEGER NOT NULL,
  capital        INTEGER NOT NULL DEFAULT 0,
  reservas       INTEGER NOT NULL DEFAULT 0,
  otras_pn       INTEGER NOT NULL DEFAULT 0,
  resultado      INTEGER NOT NULL DEFAULT 0,
  updated_at     TEXT NOT NULL,
  PRIMARY KEY (participada_id, year)
) STRICT;

-- Dividendos cobrados de una participada: fecha del acuerdo (cuando es exigible, para el año
-- de posesión del art. 21.1.a LIS), fecha de cobro, bruto y retención practicada.
CREATE TABLE IF NOT EXISTS participada_dividendo (
  id             INTEGER PRIMARY KEY,
  participada_id INTEGER NOT NULL REFERENCES participada(id) ON DELETE CASCADE,
  fecha_acuerdo  TEXT NOT NULL,
  fecha_cobro    TEXT,
  bruto          INTEGER NOT NULL CHECK (bruto > 0),
  retencion      INTEGER NOT NULL DEFAULT 0 CHECK (retencion >= 0),
  documento      TEXT NOT NULL DEFAULT '',
  notas          TEXT NOT NULL DEFAULT '',
  created_at     TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS participada_dividendo_por_fecha
  ON participada_dividendo(fecha_acuerdo);

-- Correcciones al resultado contable del 200 de un ejercicio. `clave`: la de la propuesta de
-- TaxPain que se corrige («multas», «art21:B00000000»…) o «manual:N» para las tecleadas.
-- `importe` NULL: propuesta descartada.
CREATE TABLE IF NOT EXISTS is_ajuste (
  year       INTEGER NOT NULL,
  clave      TEXT NOT NULL,
  casilla    INTEGER NOT NULL,
  importe    INTEGER CHECK (importe IS NULL OR importe >= 0),
  concepto   TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL,
  PRIMARY KEY (year, clave)
) STRICT;

-- Importes que pasan de un ejercicio a otro, por ejercicio de origen: bases imponibles
-- negativas (art. 26 LIS), minoraciones de la reserva de nivelación por adicionar (art. 105),
-- reducciones de la reserva de capitalización por aplicar (art. 25) y deducción por doble
-- imposición internacional pendiente (art. 31.6). `pendiente`: lo que queda al empezar el
-- ejercicio `ejercicio`.
CREATE TABLE IF NOT EXISTS is_arrastre (
  tipo       TEXT NOT NULL CHECK (tipo IN ('bin','nivelacion','capitalizacion','doble_imposicion')),
  origen     INTEGER NOT NULL,
  ejercicio  INTEGER NOT NULL,
  pendiente  INTEGER NOT NULL CHECK (pendiente >= 0),
  nota       TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL,
  PRIMARY KEY (tipo, origen, ejercicio)
) STRICT;

-- Datos del 200 de un ejercicio que teclea el usuario (JSON: incremento de fondos propios
-- corregido, porcentaje de la capitalización, nivelación pedida, nueva creación, deducciones y
-- bonificaciones…).
CREATE TABLE IF NOT EXISTS is_ejercicio (
  year       INTEGER PRIMARY KEY,
  datos      TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;

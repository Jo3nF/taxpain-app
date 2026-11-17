-- IVA de una sociedad con cartera o holding (G28): reglas de afectación de gastos comunes,
-- sectores diferenciados (art. 9.1.º.c LIVA) y datos de IVA de los bienes de inversión del
-- inmovilizado para su regularización (arts. 107 a 110 LIVA).

-- Sector diferenciado de actividad marcado por el usuario: grupo CNAE, porcentaje de deducción
-- propio y volumen de operaciones del año anterior (en céntimos) para saber cuál es el principal.
CREATE TABLE IF NOT EXISTS iva_sector (
  id         INTEGER PRIMARY KEY,
  nombre     TEXT NOT NULL CHECK (length(trim(nombre)) > 0),
  cnae       TEXT NOT NULL DEFAULT '',
  pct        INTEGER NOT NULL CHECK (pct BETWEEN 0 AND 100),
  volumen    INTEGER NOT NULL DEFAULT 0 CHECK (volumen >= 0),
  notas      TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;

-- Regla de afectación: porcentaje de la cuota deducible de las recibidas de un proveedor (NIF
-- normalizado) o con una cuenta de gasto (prefijo de la cuenta del asiento) registradas desde
-- `desde`, con el criterio del usuario (art. 93.Cinco LIVA: criterio razonable y homogéneo,
-- mantenido en el tiempo). Con sectores diferenciados, la regla puede llevar la compra a uno.
CREATE TABLE IF NOT EXISTS iva_afectacion (
  id         INTEGER PRIMARY KEY,
  ambito     TEXT NOT NULL CHECK (ambito IN ('proveedor','cuenta')),
  clave      TEXT NOT NULL CHECK (length(clave) > 0),
  pct        INTEGER NOT NULL CHECK (pct BETWEEN 0 AND 100),
  sector_id  INTEGER REFERENCES iva_sector(id),
  criterio   TEXT NOT NULL CHECK (length(trim(criterio)) > 0),
  desde      TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (ambito, clave)
) STRICT;

-- Datos de IVA de un elemento del inmovilizado: la recibida de la que sale (la revisión de un
-- bien de inversión lo enlaza) o, si no la hay (saldos iniciales), la cuota soportada y el
-- porcentaje deducible tecleados; y, si se vendió sin factura en TaxPain o para corregirlo, cómo
-- tributó la entrega (para el art. 110 LIVA).
CREATE TABLE IF NOT EXISTS iva_bien_inversion (
  asset_id     INTEGER PRIMARY KEY REFERENCES fixed_asset(id),
  received_id  INTEGER REFERENCES invoice_received(id),
  cuota        INTEGER CHECK (cuota IS NULL OR cuota >= 0),
  deducible    INTEGER CHECK (deducible IS NULL OR deducible BETWEEN 0 AND 100),
  inmueble     INTEGER NOT NULL DEFAULT 0 CHECK (inmueble IN (0,1)),
  venta        TEXT CHECK (venta IS NULL OR venta IN ('sujeta','exenta','no_sujeta','transmision_empresa','perdida')),
  iva_venta    INTEGER CHECK (iva_venta IS NULL OR iva_venta >= 0),
  updated_at   TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS iva_bien_inversion_recibida ON iva_bien_inversion(received_id);

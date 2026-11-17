-- Ventanilla única (OSS, régimen de la Unión; G32): clase de las ventas a particulares de
-- otros Estados miembros para el umbral de 10.000 € del art. 73 LIVA y tipos de IVA por país
-- confirmados por el usuario. Las facturas emitidas por la ventanilla única llevan el IVA de
-- destino en sus totales (`invoice_issued.data`); estas tablas no lo repiten.

-- Clase de una factura emitida (sin IVA de destino) a un particular de la UE: servicio TBE o
-- venta a distancia de bienes (cuentan para el umbral) u otra operación (no cuenta).
CREATE TABLE IF NOT EXISTS oss_clase_factura (
  invoice_id INTEGER PRIMARY KEY REFERENCES invoice_issued(id),
  clase      TEXT NOT NULL CHECK (clase IN ('servicios','bienes','otro')),
  updated_at TEXT NOT NULL
) STRICT;

-- Tipo general de IVA de un Estado miembro tal como lo ha confirmado el usuario (en
-- centésimas de punto: 20 % = 2000), con la fecha de la confirmación.
CREATE TABLE IF NOT EXISTS oss_tipo_pais (
  pais          TEXT PRIMARY KEY CHECK (length(pais) = 2),
  tipo_bp       INTEGER NOT NULL CHECK (tipo_bp BETWEEN 1 AND 3500),
  confirmado_at TEXT NOT NULL
) STRICT;

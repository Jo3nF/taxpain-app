-- G30: rentas pagadas a no residentes sin establecimiento permanente (modelos 216 y 296) y
-- certificados de residencia fiscal de los perceptores.
-- G43: datos del inmueble arrendado de cada arrendador (modelos 115 y 180).

-- Certificado de residencia fiscal de un perceptor no residente, expedido por la autoridad
-- fiscal de su país. Acredita la residencia a efectos del convenio de doble imposición o de la
-- exención del art. 14 TRLIRNR; vale un año desde su expedición (`valido_hasta`).
CREATE TABLE certificado_residencia (
  id           INTEGER PRIMARY KEY,
  nif          TEXT NOT NULL,                      -- identificador fiscal del perceptor en su país
  nombre       TEXT NOT NULL,
  pais         TEXT NOT NULL CHECK (length(pais) = 2), -- ISO 3166-1 alfa-2
  expedido     TEXT NOT NULL,
  valido_hasta TEXT NOT NULL,
  -- 1: «a efectos del convenio»; 0: sólo de residencia (exenciones UE).
  convenio     INTEGER NOT NULL DEFAULT 1 CHECK (convenio IN (0, 1)),
  documento    TEXT NOT NULL DEFAULT '',           -- nombre o referencia del documento
  notas        TEXT NOT NULL DEFAULT '',
  created_at   TEXT NOT NULL
) STRICT;
CREATE INDEX certificado_residencia_nif ON certificado_residencia(nif);

-- Una renta pagada a un no residente sin establecimiento permanente (intereses, cánones,
-- servicios sujetos, otros). Los dividendos a socios no residentes salen de `dividend_line`
-- (resident = 0) y no se apuntan aquí.
CREATE TABLE renta_no_residente (
  id          INTEGER PRIMARY KEY,
  fecha       TEXT NOT NULL,                       -- devengo (exigibilidad)
  nif         TEXT NOT NULL,
  nombre      TEXT NOT NULL,
  pais        TEXT NOT NULL CHECK (length(pais) = 2),
  naturaleza  TEXT NOT NULL
              CHECK (naturaleza IN ('dividendo','interes','canon','servicio','otros')),
  bruto       INTEGER NOT NULL CHECK (bruto > 0),  -- céntimos
  tipo_bp     INTEGER NOT NULL CHECK (tipo_bp BETWEEN 0 AND 10000),
  retencion   INTEGER NOT NULL CHECK (retencion >= 0),
  regimen     TEXT NOT NULL CHECK (regimen IN ('general','convenio','exenta')),
  -- Exención aplicada (régimen «exenta»): art14_1_c, art14_1_h, art14_1_m u otra.
  exencion    TEXT,
  concepto    TEXT NOT NULL DEFAULT '',
  recibida_id INTEGER REFERENCES invoice_received(id) ON DELETE SET NULL,
  created_at  TEXT NOT NULL
) STRICT;
CREATE INDEX renta_no_residente_fecha ON renta_no_residente(fecha);

-- Inmueble arrendado de un arrendador (proveedor), por su NIF: situación (1 con referencia
-- catastral en territorio común, 2 País Vasco, 3 Navarra, 4 sin referencia), referencia
-- catastral y dirección, como los pide el registro de perceptor del 180. `exonerado_hasta`:
-- fecha hasta la que vale la acreditación del arrendador para no retener (art. 75.3.g RIRPF).
CREATE TABLE inmueble_arrendado (
  nif             TEXT PRIMARY KEY,
  situacion       INTEGER NOT NULL CHECK (situacion BETWEEN 1 AND 4),
  ref_catastral   TEXT NOT NULL DEFAULT '',
  via             TEXT NOT NULL DEFAULT '',
  numero          TEXT NOT NULL DEFAULT '',
  municipio       TEXT NOT NULL DEFAULT '',
  provincia       TEXT NOT NULL DEFAULT '',        -- código de provincia (2 cifras)
  codigo_postal   TEXT NOT NULL DEFAULT '',
  exonerado_hasta TEXT,
  updated_at      TEXT NOT NULL
) STRICT;

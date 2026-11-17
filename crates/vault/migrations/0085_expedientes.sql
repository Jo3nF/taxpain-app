-- Expedientes con la Administración (carencias G26 y G27): requerimientos, propuestas,
-- liquidaciones, sanciones y apremios de la AEAT o de otros buzones (DEHú, Seguridad Social,
-- comunidad autónoma, ayuntamiento), con su fecha de notificación para calcular los plazos
-- (`taxpain_app::expedientes`). Nada se presenta desde TaxPain.
-- - `puesto_el`: puesta a disposición en la sede (sin `notificado_el`, se entiende notificada a
--   los diez días naturales, art. 43.2 Ley 39/2015).
-- - `plazo_dias`: plazo en días hábiles que da el propio documento (si no, el general del tipo).
-- - `vence_manual`: último día fijado a mano (un nuevo plazo concedido, lo que diga el escrito).
-- - `importe`: deuda o cuota liquidada (céntimos); `sancion`: sanción sin reducciones.
-- - `fin_voluntario`: último día del periodo voluntario de la deuda (apremio, recargo del art. 28).
-- - `documentos`: JSON con la relación de documentos que se aportan (descripción y si ya está).
CREATE TABLE IF NOT EXISTS expediente (
  id             INTEGER PRIMARY KEY,
  organismo      TEXT NOT NULL CHECK (organismo IN
                   ('aeat','dehu','seguridad_social','autonomica','ayuntamiento','otro')),
  organo         TEXT NOT NULL DEFAULT '',
  referencia     TEXT NOT NULL DEFAULT '',
  tipo           TEXT NOT NULL CHECK (tipo IN
                   ('requerimiento','propuesta_liquidacion','audiencia','propuesta_sancion',
                    'liquidacion','acuerdo_sancionador','providencia_apremio',
                    'diligencia_embargo','otro')),
  asunto         TEXT NOT NULL DEFAULT '',
  concepto       TEXT NOT NULL DEFAULT '',
  puesto_el      TEXT,
  notificado_el  TEXT,
  plazo_dias     INTEGER CHECK (plazo_dias IS NULL OR plazo_dias BETWEEN 1 AND 365),
  vence_manual   TEXT,
  importe        INTEGER CHECK (importe IS NULL OR importe >= 0),
  sancion        INTEGER CHECK (sancion IS NULL OR sancion >= 0),
  fin_voluntario TEXT,
  estado         TEXT NOT NULL DEFAULT 'abierto' CHECK (estado IN
                   ('abierto','contestado','recurrido','cerrado')),
  notice_id      TEXT REFERENCES aeat_notice(id),
  document_id    INTEGER REFERENCES company_document(id) ON DELETE SET NULL,
  documentos     TEXT NOT NULL DEFAULT '[]',
  notas          TEXT NOT NULL DEFAULT '',
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL,
  CHECK (puesto_el IS NOT NULL OR notificado_el IS NOT NULL)
) STRICT;
CREATE INDEX IF NOT EXISTS expediente_by_estado ON expediente(estado, notificado_el);

-- Lo que se ha hecho en cada expediente: contestación, alegaciones, recurso, solicitud de
-- aplazamiento o de ampliación del plazo, pago… (con su fecha y el número de registro).
CREATE TABLE IF NOT EXISTS expediente_actuacion (
  id            INTEGER PRIMARY KEY,
  expediente_id INTEGER NOT NULL REFERENCES expediente(id) ON DELETE CASCADE,
  fecha         TEXT NOT NULL,
  tipo          TEXT NOT NULL CHECK (tipo IN
                  ('contestacion','alegaciones','recurso_reposicion','reclamacion_tea',
                   'aplazamiento','ampliacion','pago','otro')),
  registro      TEXT NOT NULL DEFAULT '',
  descripcion   TEXT NOT NULL DEFAULT '',
  created_at    TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS expediente_actuacion_by_exp ON expediente_actuacion(expediente_id, fecha);

-- Certificados administrativos y apoderamientos con caducidad (G27): estar al corriente con la
-- AEAT o la Seguridad Social, residencia fiscal, situación censal, apoderamientos en el registro
-- de la AEAT o de la DEHú (REA)… Se avisa antes de que caduquen.
CREATE TABLE IF NOT EXISTS certificado_admin (
  id          INTEGER PRIMARY KEY,
  tipo        TEXT NOT NULL CHECK (tipo IN
                ('corriente_aeat','corriente_ss','residencia_fiscal','situacion_censal',
                 'apoderamiento','otro')),
  descripcion TEXT NOT NULL DEFAULT '',
  emitido_el  TEXT,
  caduca_el   TEXT NOT NULL,
  document_id INTEGER REFERENCES company_document(id) ON DELETE SET NULL,
  notas       TEXT NOT NULL DEFAULT '',
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
) STRICT;

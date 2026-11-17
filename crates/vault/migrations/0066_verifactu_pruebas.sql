-- taxpain:sin-claves-foraneas
-- VERI*FACTU: lo que acepta el entorno de pruebas de la AEAT (prewww1) queda como `pruebas`,
-- no como `correcto`: no está remitido a la AEAT (art. 16 RD 1007/2023) y, cuando haya envío a
-- producción, hay que volver a enviarlo para que la cadena allí empiece por el primer registro.
-- Los que ya constan como `correcto` sólo pueden venir del entorno de pruebas (no hay envío a
-- producción en ninguna versión anterior): pasan a `pruebas`.
--
-- SQLite no deja cambiar un CHECK sin rehacer la tabla: se rehace `verifactu_record` con los
-- mismos `id` (a los que apuntan `resubmits` y `superseded_by`) y sus triggers. Se aplica con
-- las claves foráneas desactivadas (la primera línea lo pide al ejecutor), que las comprueba
-- antes de confirmarla.
DROP TABLE IF EXISTS verifactu_record_new;
CREATE TABLE verifactu_record_new (
  id             INTEGER PRIMARY KEY,
  invoice_id     INTEGER NOT NULL REFERENCES invoice_issued(id),
  record_type    TEXT NOT NULL CHECK (record_type IN ('alta','anulacion')),
  emisor_nif     TEXT NOT NULL,
  num_serie      TEXT NOT NULL,
  fecha          TEXT NOT NULL,          -- DD-MM-YYYY
  huella         TEXT NOT NULL UNIQUE,
  prev_huella    TEXT,
  generated_at   TEXT NOT NULL,
  xml            TEXT NOT NULL,
  qr_url         TEXT,
  send_status    TEXT NOT NULL DEFAULT 'pendiente' CHECK (send_status IN
                   ('pendiente','pruebas','correcto','aceptado_con_errores','incorrecto')),
  error_code     TEXT,
  error_text     TEXT,
  csv            TEXT,
  sent_at        TEXT,
  subsanacion    INTEGER NOT NULL DEFAULT 0,
  rechazo_previo TEXT CHECK (rechazo_previo IN ('S','X')),
  resubmits      INTEGER REFERENCES verifactu_record(id),
  superseded_by  INTEGER REFERENCES verifactu_record(id)
) STRICT;
INSERT INTO verifactu_record_new(id, invoice_id, record_type, emisor_nif, num_serie, fecha,
    huella, prev_huella, generated_at, xml, qr_url, send_status, error_code, error_text, csv,
    sent_at, subsanacion, rechazo_previo, resubmits, superseded_by)
  SELECT id, invoice_id, record_type, emisor_nif, num_serie, fecha, huella, prev_huella,
    generated_at, xml, qr_url,
    CASE send_status WHEN 'correcto' THEN 'pruebas' ELSE send_status END,
    error_code, error_text, csv, sent_at, subsanacion, rechazo_previo, resubmits, superseded_by
  FROM verifactu_record;
DROP TABLE verifactu_record;
ALTER TABLE verifactu_record_new RENAME TO verifactu_record;

CREATE TRIGGER verifactu_record_no_delete BEFORE DELETE ON verifactu_record
BEGIN SELECT RAISE(ABORT, 'los registros VERI*FACTU no se borran'); END;

CREATE TRIGGER verifactu_record_immutable BEFORE UPDATE OF
  invoice_id, record_type, emisor_nif, num_serie, fecha, huella, prev_huella, generated_at, xml
  ON verifactu_record
BEGIN SELECT RAISE(ABORT, 'los registros VERI*FACTU son inmutables'); END;

CREATE TRIGGER verifactu_record_resubmission_immutable BEFORE UPDATE OF
  subsanacion, rechazo_previo, resubmits
  ON verifactu_record
BEGIN SELECT RAISE(ABORT, 'los registros VERI*FACTU son inmutables'); END;

-- Un registro sólo se sustituye una vez.
CREATE TRIGGER verifactu_record_superseded_once BEFORE UPDATE OF superseded_by ON verifactu_record
WHEN OLD.superseded_by IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'el registro ya se reenvió'); END;

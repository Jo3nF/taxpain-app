-- Declaraciones presentadas según la propia AEAT (servicio de consulta de declaraciones
-- presentadas), también las que presentó la gestoría: una fila por justificante. El CSV da
-- acceso al documento en la Sede: sólo se guarda aquí, dentro del almacén cifrado.
CREATE TABLE aeat_filing (
  id               INTEGER PRIMARY KEY,
  nif              TEXT NOT NULL,
  model            TEXT NOT NULL,
  year             INTEGER NOT NULL,
  -- Como lo da la AEAT: '1T'…'4T', '01'…'12', '0A', '1P'…'3P'.
  period           TEXT NOT NULL,
  csv              TEXT NOT NULL UNIQUE,
  file_number      TEXT NOT NULL DEFAULT '',
  receipt          TEXT NOT NULL,
  -- Justificante de la declaración a la que sustituye o complementa.
  previous_receipt TEXT,
  presented_at     TEXT NOT NULL,
  -- El justificante en PDF descargado por su CSV. Si se borra el documento, se vuelve a pedir.
  document_id      INTEGER REFERENCES document(id) ON DELETE SET NULL,
  -- Las casillas leídas (del fichero de la declaración o del PDF), para compararlas. Quitar el
  -- modelo en Importar → Comparar deja la declaración sin casillas, no la borra.
  filed_model_id   INTEGER REFERENCES filed_model(id) ON DELETE SET NULL,
  -- Último intento de descarga que no dio documento (mensaje para el usuario).
  download_error   TEXT,
  -- Respuesta definitiva del cotejo ('no_encontrado', 'anulado', 'no_cotejable'): no se vuelve
  -- a pedir, porque la AEAT limita los accesos fallidos al cotejo por día.
  download_state   TEXT,
  checked_at       TEXT NOT NULL
) STRICT;
CREATE INDEX aeat_filing_period ON aeat_filing(year, model, period);

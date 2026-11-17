-- Conversaciones con el asistente: generales o sobre un tema de TaxPain (una recomendación de
-- «Ahorrar impuestos», el plan de ahorro del año…), con sus mensajes y lo que el usuario añade
-- (datos escritos y el texto de documentos; nunca el fichero). En el almacén cifrado de la
-- empresa; lo que se envía se anonimiza de nuevo en cada pregunta.
CREATE TABLE IF NOT EXISTS assistant_conversation (
  -- sin reutilizar los de las borradas: una respuesta que llega tarde no acaba en otra
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  -- 'general', 'ahorro', 'ahorro_plan'… (lo valida la app: sin CHECK, para añadir tipos sin rehacer la tabla)
  kind       TEXT NOT NULL,
  -- clave del tema con su ejercicio ('capitalizacion@2026', '2026'); '' en las generales
  topic_key  TEXT NOT NULL,
  topic      TEXT NOT NULL,          -- el tema entero (JSON)
  title      TEXT NOT NULL,
  -- lo que se envió del tema en la última pregunta (JSON: block, header, at, terceros; en claro
  -- sólo aquí)
  snapshot   TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;
CREATE UNIQUE INDEX IF NOT EXISTS assistant_conversation_topic
  ON assistant_conversation(kind, topic_key) WHERE kind <> 'general';
CREATE INDEX IF NOT EXISTS assistant_conversation_updated ON assistant_conversation(updated_at);

CREATE TABLE IF NOT EXISTS assistant_message (
  conversation_id INTEGER NOT NULL REFERENCES assistant_conversation(id) ON DELETE CASCADE,
  seq             INTEGER NOT NULL,
  role            TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
  content         TEXT NOT NULL,     -- como se ve en el Mac (marcadores restaurados)
  notes           TEXT,              -- sólo respuestas (JSON): model, replaced, used, sources, unverified
  created_at      TEXT NOT NULL,
  PRIMARY KEY (conversation_id, seq)
) STRICT;

CREATE TABLE IF NOT EXISTS assistant_material (
  id              INTEGER PRIMARY KEY,
  conversation_id INTEGER NOT NULL REFERENCES assistant_conversation(id) ON DELETE CASCADE,
  kind            TEXT NOT NULL CHECK (kind IN ('dato', 'documento')),
  content         TEXT NOT NULL,
  filename        TEXT,              -- sólo para enseñarlo en el Mac; no se envía
  file_sha256     TEXT,
  file_size       INTEGER,
  read_by         TEXT CHECK (read_by IN ('mac', 'ia')),
  truncated       INTEGER NOT NULL DEFAULT 0 CHECK (truncated IN (0, 1)),
  created_at      TEXT NOT NULL,
  -- un documento lleva su nombre, su huella y quién lo leyó; un dato, nada de eso
  CHECK (CASE kind
    WHEN 'documento' THEN filename IS NOT NULL AND file_sha256 IS NOT NULL AND read_by IS NOT NULL
    ELSE filename IS NULL AND file_sha256 IS NULL AND file_size IS NULL AND read_by IS NULL
  END)
) STRICT;
CREATE INDEX IF NOT EXISTS assistant_material_conversation ON assistant_material(conversation_id);

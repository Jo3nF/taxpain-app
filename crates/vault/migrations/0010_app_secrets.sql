-- Fase 7: secretos de la app que no son claves criptográficas (p. ej. la clave de API del
-- proveedor del asistente). Viven en la BD cifrada y nunca se devuelven a la interfaz.
CREATE TABLE app_secret (
  name       TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;

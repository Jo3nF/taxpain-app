-- Fase 2: avisos ya enviados (para no repetir notificaciones).
CREATE TABLE reminder_sent (
  reminder_key TEXT PRIMARY KEY,   -- modelo|ejercicio|periodo|fecha del aviso
  sent_at      TEXT NOT NULL
) STRICT;

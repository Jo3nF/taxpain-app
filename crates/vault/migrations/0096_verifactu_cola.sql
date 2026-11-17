-- VERI*FACTU: cola de remisión (Orden HAC/1177/2024 art. 16; RD 1007/2023 art. 16).
--
-- Por registro: cuántas veces se ha enviado, cuántas la AEAT respondió sin una línea para él
-- (se reintenta una vez; a la segunda queda «sin respuesta» y se consulta antes de reenviar),
-- si se envió con `Incidencia = S` y, si la AEAT lo rechazó por duplicado, el estado con que
-- ya lo tenía (`Correcta`, `AceptadaConErrores` o `Anulada`). Son columnas de seguimiento del
-- envío: los triggers de inmutabilidad no las cubren, como `send_status`.
ALTER TABLE verifactu_record ADD COLUMN intentos INTEGER NOT NULL DEFAULT 0;
ALTER TABLE verifactu_record ADD COLUMN sin_linea INTEGER NOT NULL DEFAULT 0;
ALTER TABLE verifactu_record ADD COLUMN incidencia INTEGER NOT NULL DEFAULT 0
  CHECK (incidencia IN (0, 1));
ALTER TABLE verifactu_record ADD COLUMN duplicado TEXT
  CHECK (duplicado IN ('Correcta', 'AceptadaConErrores', 'Anulada'));

-- Estado de la cola de la empresa (una fila): control de flujo y reintentos. Las horas, en
-- segundos Unix (UTC).
--   proximo_envio     no se envía antes (TiempoEsperaEnvio de la última respuesta o el
--                     reintento tras un fallo), salvo que se junten 1.000 registros.
--   tiempo_espera     último TiempoEsperaEnvio (60 s al empezar).
--   incidencia_desde  primer fallo técnico sin resolver: el siguiente envío lleva
--                     Incidencia = S.
--   bloqueada         la AEAT rechazó el envío entero por un error del mensaje (SOAP Fault
--                     Client): no se reintenta solo hasta que el usuario lo pida.
CREATE TABLE verifactu_cola (
  id               INTEGER PRIMARY KEY CHECK (id = 1),
  proximo_envio    INTEGER,
  tiempo_espera    INTEGER NOT NULL DEFAULT 60,
  incidencia_desde INTEGER,
  fallos_seguidos  INTEGER NOT NULL DEFAULT 0,
  ultimo_intento   INTEGER,
  ultimo_resultado TEXT,
  ultimo_error     TEXT,
  bloqueada        INTEGER NOT NULL DEFAULT 0 CHECK (bloqueada IN (0, 1))
) STRICT;
INSERT INTO verifactu_cola(id) VALUES (1);

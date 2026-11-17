-- Casillas de cada declaración de la AEAT («Comprobar lo presentado»), de todos los modelos
-- (no sólo de los que TaxPain compara): para la evolución de la empresa año a año y los cruces
-- entre modelos. JSON {casilla: céntimos}; en las informativas, totales del registro de
-- declarante con claves propias (ver `taxpain-app::presentado_aeat`).
ALTER TABLE aeat_filing ADD COLUMN boxes TEXT;
-- De dónde salen: 'fichero' (visor de la declaración), 'informativa' o 'pdf'.
ALTER TABLE aeat_filing ADD COLUMN boxes_source TEXT;
-- Respuesta definitiva del visor de la declaración: 'no_disponible' (no tiene el documento,
-- como las declaraciones antiguas), 'sin_fichero' o 'leido' (se guardó el fichero). NULL: aún
-- no se ha pedido o falló la red.
ALTER TABLE aeat_filing ADD COLUMN visor_state TEXT;
-- El fichero de la declaración que dio el visor: se vuelve a leer sin pedirlo a la AEAT cuando
-- TaxPain tenga el diseño de registro de ese ejercicio (si aún no lo tenía).
ALTER TABLE aeat_filing ADD COLUMN file TEXT;

-- Versión del hash de cada entrada de la auditoría (ver crates/vault/src/audit.rs). Las entradas
-- que ya existen quedan como v1 (campos unidos con saltos de línea) y siguen verificando tal
-- cual; las nuevas se escriben como v2 (cada campo con su longitud delante). Un programa
-- anterior que siga abierto durante la actualización no escribe la columna: sus entradas quedan
-- como v1, que es como las calcula.
ALTER TABLE audit_log ADD COLUMN hash_version INTEGER NOT NULL DEFAULT 1;

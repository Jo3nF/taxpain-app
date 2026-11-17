-- Movimientos que llegaron por una conexión con el banco y el usuario borró (un duplicado, por
-- ejemplo): su identificador queda aquí para que la descarga siguiente no los vuelva a meter.
CREATE TABLE bank_tx_ignored (
  bank_account INTEGER NOT NULL REFERENCES bank_account(id),
  external_id  TEXT NOT NULL,
  deleted_at   TEXT NOT NULL,
  PRIMARY KEY (bank_account, external_id)
) STRICT;

-- Día del movimiento pendiente más antiguo de la última descarga: la siguiente pide desde unos
-- días antes, para que un pendiente que tarda en contabilizarse no se quede fuera.
ALTER TABLE bank_link_account ADD COLUMN pending_since TEXT;

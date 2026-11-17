-- Punto de partida de Criptomonedas: lo que había a una fecha de corte y su coste pendiente,
-- tal como lo calculó la gestoría y figura en el balance del último ejercicio cerrado y
-- declarado. Desde el día siguiente el cálculo sigue sobre estas posiciones; las operaciones
-- hasta la fecha de corte quedan como historial (se ven, no cuentan para el coste ni para las
-- ganancias). Sirve cuando faltan registros de años antiguos (exchanges que cerraron, DeFi).

-- Una sola fila: la fecha de corte, la cuenta del diario donde está ese coste (la de la
-- gestoría, p. ej. una 200xxxx «Criptomonedas»; vacía si no se sabe), de dónde sale y una nota.
CREATE TABLE crypto_opening (
  id          INTEGER PRIMARY KEY CHECK (id = 1),
  date        TEXT NOT NULL,              -- YYYY-MM-DD (último día que no cuenta)
  account     TEXT NOT NULL DEFAULT '',
  source      TEXT NOT NULL DEFAULT '',
  note        TEXT NOT NULL DEFAULT '',
  updated_at  TEXT NOT NULL
) STRICT;

-- Posiciones a la fecha de corte: cantidad (texto decimal con punto, como crypto_op.qty) y coste
-- pendiente en céntimos. Sin plataforma (NULL), sus unidades sirven para las salidas de la
-- plataforma a la que le falten.
CREATE TABLE crypto_opening_position (
  id          INTEGER PRIMARY KEY,
  ticker      TEXT NOT NULL,
  platform_id INTEGER REFERENCES crypto_platform(id),
  qty         TEXT NOT NULL,
  cost        INTEGER NOT NULL CHECK (cost >= 0)
) STRICT;
CREATE UNIQUE INDEX crypto_opening_position_key
  ON crypto_opening_position(ticker, COALESCE(platform_id, 0));

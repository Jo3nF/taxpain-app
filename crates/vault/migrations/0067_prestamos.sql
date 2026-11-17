-- Préstamos concedidos y recibidos (PGC de PYMES, NRV 9.ª): la ficha del contrato, las cuotas
-- registradas (o marcadas como ya contabilizadas fuera de TaxPain) y los intereses devengados a
-- una fecha (periodificación al cierre). El cuadro se calcula con la ficha
-- (`taxpain_core::accounting::prestamo::cuadro`): no se guarda.
CREATE TABLE prestamo (
  id                    INTEGER PRIMARY KEY,
  nombre                TEXT NOT NULL,
  sentido               TEXT NOT NULL CHECK (sentido IN ('concedido','recibido')),
  -- Contraparte: un contacto, un socio, una participada (por nombre y NIF) o texto libre.
  contraparte_origen    TEXT NOT NULL DEFAULT 'texto'
                        CHECK (contraparte_origen IN ('contacto','socio','participada','texto')),
  contraparte_ref       INTEGER,            -- id del contacto o del socio
  contraparte_nif       TEXT NOT NULL DEFAULT '',
  contraparte_nombre    TEXT NOT NULL,
  contraparte_provincia TEXT NOT NULL DEFAULT '',   -- 01–53, para el 193
  persona_fisica        INTEGER NOT NULL DEFAULT 0 CHECK (persona_fisica IN (0,1)),
  entidad_credito       INTEGER NOT NULL DEFAULT 0 CHECK (entidad_credito IN (0,1)),
  -- Parte vinculada (art. 18.2 LIS): grupo, asociada u otra (socio, administrador…).
  vinculo               TEXT NOT NULL DEFAULT 'ninguno'
                        CHECK (vinculo IN ('ninguno','grupo','asociada','otra')),
  vinculada             INTEGER NOT NULL DEFAULT 0 CHECK (vinculada IN (0,1)),
  principal             INTEGER NOT NULL CHECK (principal > 0),       -- céntimos
  fecha_inicio          TEXT NOT NULL,
  tipo_bp               INTEGER NOT NULL CHECK (tipo_bp BETWEEN 0 AND 10000),
  sistema               TEXT NOT NULL
                        CHECK (sistema IN ('frances','lineal','intereses_periodicos','al_vencimiento')),
  periodicidad_meses    INTEGER NOT NULL CHECK (periodicidad_meses IN (1,3,6,12)),
  plazo_meses           INTEGER NOT NULL CHECK (plazo_meses BETWEEN 1 AND 600),
  carencia_meses        INTEGER NOT NULL DEFAULT 0 CHECK (carencia_meses >= 0),
  carencia_tipo         TEXT NOT NULL DEFAULT 'capital' CHECK (carencia_tipo IN ('capital','total')),
  cuenta_largo          TEXT NOT NULL,
  cuenta_corto          TEXT NOT NULL,
  cuenta_intereses      TEXT NOT NULL,
  cuenta_devengo        TEXT NOT NULL,
  retencion_bp          INTEGER NOT NULL DEFAULT 0 CHECK (retencion_bp BETWEEN 0 AND 10000),
  notas                 TEXT NOT NULL DEFAULT '',
  -- Alta (entrega o recepción del principal): pendiente, contabilizada desde aquí (con su
  -- asiento) o ya contabilizada fuera de TaxPain (saldos iniciales, diario del gestor).
  alta_estado           TEXT NOT NULL DEFAULT 'pendiente'
                        CHECK (alta_estado IN ('pendiente','contabilizada','fuera')),
  alta_entry            INTEGER REFERENCES journal_entry(id),
  created_at            TEXT NOT NULL,
  updated_at            TEXT NOT NULL,
  CHECK ((vinculada = 1) = (vinculo <> 'ninguno')),
  CHECK ((alta_estado = 'contabilizada') = (alta_entry IS NOT NULL))
) STRICT;

-- Cuotas registradas, con los importes con que se contabilizaron. `entry_id` NULL: la cuota ya
-- estaba contabilizada fuera de TaxPain (en el diario del gestor o en los saldos iniciales). El
-- movimiento del banco conciliado es el `bank_tx` que apunta a `entry_id`.
CREATE TABLE prestamo_cuota (
  prestamo_id   INTEGER NOT NULL REFERENCES prestamo(id),
  n             INTEGER NOT NULL,
  fecha         TEXT NOT NULL,              -- vencimiento del cuadro (devengo de la retención)
  capital       INTEGER NOT NULL,
  intereses     INTEGER NOT NULL,
  capitalizados INTEGER NOT NULL DEFAULT 0,
  retencion     INTEGER NOT NULL DEFAULT 0,
  entry_id      INTEGER REFERENCES journal_entry(id),
  created_at    TEXT NOT NULL,
  PRIMARY KEY (prestamo_id, n)
) STRICT;

-- Intereses devengados y no vencidos de la cuota `n`, contabilizados a una fecha (547/762,
-- 662/528…). Al registrar la cuota, lo devengado sale de la cuenta de devengo.
CREATE TABLE prestamo_devengo (
  id          INTEGER PRIMARY KEY,
  prestamo_id INTEGER NOT NULL REFERENCES prestamo(id),
  n           INTEGER NOT NULL,
  fecha       TEXT NOT NULL,
  importe     INTEGER NOT NULL CHECK (importe > 0),
  entry_id    INTEGER REFERENCES journal_entry(id),
  created_at  TEXT NOT NULL
) STRICT;
CREATE INDEX prestamo_devengo_por_prestamo ON prestamo_devengo(prestamo_id, n);

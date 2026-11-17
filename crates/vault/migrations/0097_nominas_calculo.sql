-- Nóminas calculadas en TaxPain (G45). Importes en céntimos.
--
-- `nomina_ficha`: datos de cotización y salario de un trabajador por cuenta ajena para calcular
-- su nómina cada mes (grupo de cotización, contrato, tarifa de AT/EP de su CNAE, salario,
-- complementos, pagas y tipo de retención tecleado). `datos` es el JSON de
-- `taxpain_app::nominas::FichaNomina`.
CREATE TABLE IF NOT EXISTS nomina_ficha (
  employee_id INTEGER PRIMARY KEY REFERENCES employee(id),
  datos       TEXT NOT NULL,
  updated_at  TEXT NOT NULL
) STRICT;

-- Cálculo con el que se registró un recibo (`payslip`): los datos del mes y la nómina
-- calculada (devengos, bases, cuotas por concepto, retención), para el recibo de salarios en PDF
-- y el resumen de los seguros sociales del mes. `periodo`: mes de devengo 'AAAA-MM' (el del
-- recibo, que puede pagarse el mes siguiente). Se borra con el recibo.
CREATE TABLE IF NOT EXISTS nomina_calculo (
  payslip_id INTEGER PRIMARY KEY REFERENCES payslip(id) ON DELETE CASCADE,
  periodo    TEXT NOT NULL CHECK (periodo GLOB '[0-9][0-9][0-9][0-9]-[0-1][0-9]'),
  datos      TEXT NOT NULL,
  created_at TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS nomina_calculo_por_periodo ON nomina_calculo(periodo);

-- Pago de los seguros sociales de un mes (la liquidación de cuotas a la TGSS, antiguo TC1, hoy
-- RLC): asiento 476 al debe contra tesorería. Uno por mes de devengo. Si se borra el asiento en el
-- diario, el pago queda sin asiento y se puede volver a registrar.
CREATE TABLE IF NOT EXISTS seguros_sociales_pago (
  periodo    TEXT PRIMARY KEY CHECK (periodo GLOB '[0-9][0-9][0-9][0-9]-[0-1][0-9]'),
  fecha      TEXT NOT NULL,
  importe    INTEGER NOT NULL CHECK (importe > 0),
  cuenta     TEXT NOT NULL,
  entry_id   INTEGER REFERENCES journal_entry(id) ON DELETE SET NULL,
  created_at TEXT NOT NULL
) STRICT;

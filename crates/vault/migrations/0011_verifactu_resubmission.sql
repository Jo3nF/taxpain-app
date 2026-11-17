-- Subsanación de registros VERI*FACTU rechazados o aceptados con errores (docs/VERIFACTU.md §9).
-- Cada reenvío es un registro NUEVO encadenado; el original no cambia (salvo `superseded_by`).
ALTER TABLE verifactu_record ADD COLUMN subsanacion INTEGER NOT NULL DEFAULT 0;
ALTER TABLE verifactu_record ADD COLUMN rechazo_previo TEXT CHECK (rechazo_previo IN ('S','X'));
ALTER TABLE verifactu_record ADD COLUMN resubmits INTEGER REFERENCES verifactu_record(id);
ALTER TABLE verifactu_record ADD COLUMN superseded_by INTEGER REFERENCES verifactu_record(id);

CREATE TRIGGER verifactu_record_resubmission_immutable BEFORE UPDATE OF
  subsanacion, rechazo_previo, resubmits
  ON verifactu_record
BEGIN SELECT RAISE(ABORT, 'los registros VERI*FACTU son inmutables'); END;

-- Un registro sólo se sustituye una vez.
CREATE TRIGGER verifactu_record_superseded_once BEFORE UPDATE OF superseded_by ON verifactu_record
WHEN OLD.superseded_by IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'el registro ya se reenvió'); END;

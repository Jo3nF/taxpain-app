import { ChevronLeft, ChevronRight } from 'lucide-react';
import { PROVINCES } from './provincias';

/** ‹ 2026 ›: cambia de ejercicio. */
export function YearStepper({
  year,
  onChange,
  label = 'Ejercicio',
}: {
  year: number;
  onChange: (y: number) => void;
  label?: string;
}) {
  return (
    <fieldset className="stepper" aria-label={label}>
      <button type="button" aria-label="Año anterior" onClick={() => onChange(year - 1)}>
        <ChevronLeft size={16} aria-hidden="true" />
      </button>
      <span aria-live="polite">{year}</span>
      <button type="button" aria-label="Año siguiente" onClick={() => onChange(year + 1)}>
        <ChevronRight size={16} aria-hidden="true" />
      </button>
    </fieldset>
  );
}

/** Sugerencias de provincia para un campo de texto (`list={id}`). */
export function ProvinceDatalist({ id }: { id: string }) {
  return (
    <datalist id={id}>
      {PROVINCES.map(([code, name]) => (
        <option key={code} value={name} />
      ))}
    </datalist>
  );
}

/** Desplegable de provincias que guarda el código (46 = Valencia), como piden los modelos. */
export function ProvinceCodeSelect({
  value,
  onChange,
  className,
}: {
  value: string;
  onChange: (code: string) => void;
  className?: string;
}) {
  const known = PROVINCES.some(([c]) => c === value);
  return (
    <select className={className} value={value} onChange={(e) => onChange(e.target.value)}>
      <option value="">Elige la provincia…</option>
      {value && !known && <option value={value}>{value}</option>}
      {PROVINCES.map(([code, name]) => (
        <option key={code} value={code}>
          {code} · {name}
        </option>
      ))}
    </select>
  );
}

import { useEffect, useState } from 'react';
import { errorMessage, euros, financialRatios, type Ratio } from './api';

const MAX = 9_000_000_000_000_000;

/** Valor de un ratio en su unidad (los valores van en centésimas; los euros, en céntimos). */
export function show(unit: Ratio['unit'], v: number | null): string {
  if (v === null) return '—';
  if (unit === 'euros') return euros(v);
  const n = (v / 100).toLocaleString('es-ES', { maximumFractionDigits: unit === 'dias' ? 0 : 2 });
  return unit === 'porcentaje' ? `${n} %` : unit === 'dias' ? `${n} días` : n;
}

function range(r: Ratio): string {
  if (!r.range) return '';
  const [lo, hi] = r.range;
  if (hi >= MAX) return `≥ ${show(r.unit, lo)}`;
  if (lo === 0 && r.unit === 'dias') return `≤ ${show(r.unit, hi)}`;
  return `${show(r.unit, lo)} – ${show(r.unit, hi)}`;
}

/** Ratios financieros del ejercicio, con el anterior y una señal orientativa. */
export function Ratios({ year }: { year: number }) {
  const [rows, setRows] = useState<Ratio[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    financialRatios(year)
      .then(setRows)
      .catch((e) => setError(errorMessage(e)));
  }, [year]);
  if (error) return <p role="alert">{error}</p>;
  if (!rows) return null;
  const groups = [...new Set(rows.map((r) => r.group))];
  const hasPrevious = rows.some((r) => r.previous !== null);
  return (
    <section aria-label="Ratios" className="mt-8 text-sm">
      <h3 className="font-medium">Ratios financieros {year}</h3>
      <p className="text-stone-600">
        Los rangos son orientativos y dependen del sector: úsalos como señal, no como juicio.
      </p>
      <table className="mt-2 w-full">
        <thead className="text-left text-stone-500">
          <tr>
            <th>Ratio</th>
            <th className="px-2 text-right">{year}</th>
            {hasPrevious && <th className="px-2 text-right">{year - 1}</th>}
            <th className="px-2">Orientativo</th>
          </tr>
        </thead>
        {groups.map((g) => (
          <tbody key={g}>
            <tr>
              <th colSpan={4} className="pt-3 text-left font-medium text-stone-700">
                {g}
              </th>
            </tr>
            {rows
              .filter((r) => r.group === g)
              .map((r) => (
                <tr key={r.id} className="border-t border-stone-200 align-top">
                  <td className="py-1 pr-2">
                    <span title={r.formula}>{r.name}</span>
                    <span className="block text-xs text-stone-500">{r.explanation}</span>
                  </td>
                  <td
                    className={`px-2 text-right tabular-nums ${r.status === 'atencion' ? 'text-amber-800' : ''}`}
                  >
                    {show(r.unit, r.value)}
                    {r.status === 'atencion' && <span className="sr-only"> (fuera del rango)</span>}
                  </td>
                  {hasPrevious && (
                    <td className="px-2 text-right tabular-nums text-stone-500">
                      {show(r.unit, r.previous)}
                    </td>
                  )}
                  <td className="px-2 text-stone-500">{range(r)}</td>
                </tr>
              ))}
          </tbody>
        ))}
      </table>
    </section>
  );
}

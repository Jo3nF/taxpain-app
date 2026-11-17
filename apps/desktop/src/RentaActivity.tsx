import { useEffect, useState } from 'react';
import { type ActivityDraft, criptoQty, errorMessage, euros, rentaActivity } from './api';

/** Renta del autónomo (modelo 100): rendimiento de la actividad y casillas para Renta Web. */
export function RentaActivity({ year: initialYear }: { year: number }) {
  const [year, setYear] = useState(initialYear);
  const [draft, setDraft] = useState<ActivityDraft | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    setDraft(null);
    rentaActivity(year)
      .then(setDraft)
      .catch((e) => setError(errorMessage(e)));
  }, [year]);

  const r = draft?.result;
  return (
    <section
      aria-labelledby="renta-act-h"
      className="mt-8 rounded-lg border border-stone-200 bg-white p-4"
    >
      <h3 id="renta-act-h" className="font-medium">
        Renta: rendimiento de tu actividad (modelo 100)
      </h3>
      <p className="mt-1 text-sm text-stone-600">
        Borrador con tu contabilidad para rellenar el apartado de actividades económicas en Renta
        Web. TaxPain no presenta la Renta: revisa las cifras y cópialas tú.
      </p>
      <label className="mt-3 block text-sm">
        Ejercicio
        <select
          className="mt-1 block"
          value={year}
          onChange={(e) => setYear(Number(e.target.value))}
        >
          {[initialYear + 1, initialYear, initialYear - 1, initialYear - 2].map((y) => (
            <option key={y} value={y}>
              {y}
            </option>
          ))}
        </select>
      </label>
      {error && (
        <p role="alert" className="mt-2 text-sm text-red-700">
          {error}
        </p>
      )}
      {draft && r && (
        <div className="mt-3 text-sm">
          <p>
            Estimación directa{' '}
            <strong>{draft.regime === 'Simplificada' ? 'simplificada' : 'normal'}</strong>
            {' · '}cámbiala en «Datos de los modelos».
          </p>
          <dl className="mt-2 grid grid-cols-2 gap-x-6 gap-y-1 sm:grid-cols-4">
            {(
              [
                ['Ingresos', r.income],
                ['Gastos deducibles', r.expenses],
                ['Rendimiento neto', r.net],
                ['Rendimiento neto reducido total (casilla 0235)', r.net_reduced],
                ['Retenciones', r.withholdings],
                ['Pagos del 130', r.installments],
              ] as const
            ).map(([k, v]) => (
              <div key={k}>
                <dt className="text-stone-500">{k}</dt>
                <dd className="tabular-nums font-medium">{euros(v)}</dd>
              </div>
            ))}
          </dl>
          <table className="mt-4 w-full">
            <caption className="text-left font-medium">Casillas</caption>
            <thead className="text-left text-stone-500">
              <tr>
                <th scope="col" className="w-16">
                  Casilla
                </th>
                <th scope="col">Concepto</th>
                <th scope="col" className="text-right">
                  Importe
                </th>
              </tr>
            </thead>
            <tbody>
              {r.boxes.map((b) => (
                <tr key={b.label}>
                  <td className="font-mono">{b.code ?? '—'}</td>
                  <td>{b.label}</td>
                  <td className="text-right tabular-nums">{euros(b.amount)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="mt-1 text-xs text-stone-500">
            «—»: búscala por su nombre en Renta Web. Comprueba siempre el número de casilla del
            ejercicio en Renta Web.
          </p>
          {draft.check.length > 0 && (
            <table className="mt-4 w-full">
              <caption className="text-left font-medium">
                Cotejo con los datos fiscales de la AEAT
                {draft.check_date &&
                  ` (${draft.check_date.slice(6, 8)}/${draft.check_date.slice(4, 6)}/${draft.check_date.slice(0, 4)})`}
              </caption>
              <thead className="text-left text-stone-500">
                <tr>
                  <th scope="col">Concepto</th>
                  <th scope="col" className="text-right">
                    AEAT
                  </th>
                  <th scope="col" className="text-right">
                    TaxPain
                  </th>
                  <th scope="col">
                    <span className="sr-only">Estado</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {draft.check.map((l) => (
                  <tr key={l.label} className={l.matches ? '' : 'bg-amber-50'}>
                    <td>{l.label}</td>
                    <td className="text-right tabular-nums">{euros(l.aeat)}</td>
                    <td className="text-right tabular-nums">{euros(l.taxpain)}</td>
                    <td className="pl-2">{l.matches ? '✓' : '≠'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <ul className="mt-3 list-disc pl-5 text-xs text-stone-600">
            {[...r.warnings, ...draft.notes].map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>
          {draft.cripto && (
            <section
              aria-labelledby="renta-cripto-h"
              className="mt-4 rounded border border-stone-200 p-3"
            >
              <h4 id="renta-cripto-h" className="font-medium">
                Ganancias y pérdidas por criptomonedas {draft.cripto.year} (FIFO)
              </h4>
              <dl className="mt-2 grid grid-cols-2 gap-x-6 gap-y-1 sm:grid-cols-4">
                {(
                  [
                    ['Ganancias', draft.cripto.ganancias],
                    ['Pérdidas', draft.cripto.perdidas],
                    ['Neto (base del ahorro)', draft.cripto.neto],
                    ['Recompensas', draft.cripto.recompensas],
                  ] as const
                ).map(([k, v]) => (
                  <div key={k}>
                    <dt className="text-stone-500">{k}</dt>
                    <dd className="tabular-nums font-medium">{euros(v)}</dd>
                  </div>
                ))}
              </dl>
              {draft.cripto.lineas.length > 0 && (
                <table className="mt-2 w-full">
                  <caption className="text-left text-stone-500">
                    Una línea por transmisión (casillas 1800 a 1814 de Renta Web; clave F euros, N
                    permuta, B pago)
                  </caption>
                  <thead className="text-left text-stone-500">
                    <tr>
                      <th scope="col">Fecha</th>
                      <th scope="col">Moneda</th>
                      <th scope="col">Clave</th>
                      <th scope="col" className="text-right">
                        Cantidad
                      </th>
                      <th scope="col" className="text-right">
                        Transmisión
                      </th>
                      <th scope="col" className="text-right">
                        Adquisición
                      </th>
                      <th scope="col" className="text-right">
                        Resultado
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {draft.cripto.lineas.map((l) => (
                      <tr key={`${l.fecha}-${l.ticker}-${l.cantidad}-${l.valor_transmision}`}>
                        <td>{l.fecha}</td>
                        <td className="font-mono">{l.ticker}</td>
                        <td>{l.clave}</td>
                        <td className="text-right tabular-nums">{criptoQty(l.cantidad)}</td>
                        <td className="text-right tabular-nums">{euros(l.valor_transmision)}</td>
                        <td className="text-right tabular-nums">{euros(l.valor_adquisicion)}</td>
                        <td className="text-right tabular-nums">{euros(l.resultado)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
              <ul className="mt-2 list-disc pl-5 text-xs text-stone-600">
                {draft.cripto.notas.map((n) => (
                  <li key={n}>{n}</li>
                ))}
              </ul>
            </section>
          )}
        </div>
      )}
    </section>
  );
}

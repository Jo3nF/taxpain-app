import { useEffect, useState } from 'react';
import { type CrossCheck, errorMessage, euros, rentaCrossCheck, rentaDownload } from './api';
import { CertificateSelect } from './CertificateSelect';

/** Cotejo de la actividad con los datos fiscales de Renta que tiene la AEAT. */
export function RentaCheck({ year: initialYear }: { year: number }) {
  const [year, setYear] = useState(initialYear);
  const [cert, setCert] = useState('');
  const [check, setCheck] = useState<CrossCheck | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    rentaCrossCheck(year)
      .then(setCheck)
      .catch(() => setCheck(null));
  }, [year]);

  const diffs = check?.lines.filter((l) => !l.matches).length ?? 0;
  return (
    <section
      aria-labelledby="renta-h"
      className="mt-8 rounded-lg border border-stone-200 bg-white p-4"
    >
      <h3 id="renta-h" className="font-medium">
        Cotejo con los datos fiscales de la AEAT (Renta)
      </h3>
      <p className="mt-1 text-sm text-stone-600">
        Descarga con tu certificado los datos que tiene Hacienda (lo que tus clientes declararon en
        su 190, tus pagos del 130 y tus cuotas de autónomo) y los compara con TaxPain. Es sólo una
        consulta: no presenta nada. Los datos se guardan cifrados en tu Mac.
      </p>
      <div className="mt-3 flex flex-wrap items-end gap-3 text-sm">
        <label>
          Ejercicio
          <select
            className="mt-1 block"
            value={year}
            onChange={(e) => setYear(Number(e.target.value))}
          >
            {[initialYear, initialYear - 1, initialYear - 2].map((y) => (
              <option key={y} value={y}>
                {y}
              </option>
            ))}
          </select>
        </label>
        <CertificateSelect value={cert} onChange={setCert} />
        <button
          type="button"
          disabled={busy || !cert}
          className="rounded border border-stone-300 px-3 py-1 disabled:opacity-50"
          onClick={async () => {
            setBusy(true);
            setError(null);
            try {
              setCheck(await rentaDownload(cert, year));
            } catch (e) {
              setError(errorMessage(e));
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? 'Descargando…' : check ? 'Volver a descargar' : 'Descargar y cotejar'}
        </button>
      </div>
      {error && (
        <p role="alert" className="mt-2 text-sm text-red-700">
          {error}
        </p>
      )}
      {check && (
        <div className="mt-3 text-sm">
          <p className={diffs ? 'text-amber-800' : 'text-emerald-800'}>
            {diffs ? `${diffs} diferencia(s) que revisar.` : 'Todo coincide.'}
            {check.process_date &&
              ` Datos de la AEAT del ${check.process_date.slice(6, 8)}/${check.process_date.slice(4, 6)}/${check.process_date.slice(0, 4)}.`}
          </p>
          {check.lines.length > 0 && (
            <table className="mt-2 w-full">
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
                {check.lines.map((l) => (
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
          <ul className="mt-2 list-disc pl-5 text-xs text-stone-600">
            {check.notes.map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}

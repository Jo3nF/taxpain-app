import { useState } from 'react';
import {
  type AccountsModel,
  type CompanyOverview,
  type Consolidated,
  consolidatedAccounts,
  errorMessage,
  euros,
} from './api';
import { Lines } from './Statements';

/**
 * Cuentas del grupo: balance y cuenta de resultados sumados de varias empresas del Mac, con la
 * eliminación de lo que se deben y se venden entre ellas.
 */
export function Consolidation({ companies }: { companies: CompanyOverview[] }) {
  const [ids, setIds] = useState<string[]>(companies.map((c) => c.id));
  const [year, setYear] = useState(new Date().getFullYear());
  const [model, setModel] = useState<AccountsModel>('Pymes');
  const [eliminate, setEliminate] = useState(true);
  const [r, setR] = useState<Consolidated | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const run = () =>
    consolidatedAccounts(ids, year, model, eliminate)
      .then((x) => {
        setR(x);
        setMsg(null);
      })
      .catch((e) => {
        setR(null);
        setMsg(errorMessage(e));
      });
  return (
    <details className="mt-4 rounded-lg border border-stone-200 bg-white p-3 text-sm">
      <summary className="font-medium">Cuentas del grupo (consolidación)</summary>
      <fieldset className="choices mt-3">
        <legend>Empresas</legend>
        {companies.map((c) => (
          <label key={c.id} className="flex items-start gap-2">
            <input
              type="checkbox"
              checked={ids.includes(c.id)}
              onChange={(e) =>
                setIds(e.target.checked ? [...ids, c.id] : ids.filter((i) => i !== c.id))
              }
            />
            {c.name}
          </label>
        ))}
      </fieldset>
      <div className="mt-4 flex flex-wrap items-end gap-3">
        <label>
          Ejercicio
          <input
            className="mt-1 block w-24"
            inputMode="numeric"
            value={year}
            onChange={(e) => setYear(Number(e.target.value) || year)}
          />
        </label>
        <label>
          Modelo
          <select
            className="mt-1 block"
            value={model}
            onChange={(e) => setModel(e.target.value as AccountsModel)}
          >
            <option value="Pymes">PYMES</option>
            <option value="Abreviado">Abreviado</option>
            <option value="Normal">Normal</option>
          </select>
        </label>
        <label className="flex min-h-9 items-center gap-2">
          <input
            type="checkbox"
            checked={eliminate}
            onChange={(e) => setEliminate(e.target.checked)}
          />
          Eliminar operaciones y saldos entre ellas
        </label>
        <button
          type="button"
          className="rounded btn-primary px-3 py-1"
          disabled={ids.length < 2}
          onClick={run}
        >
          Calcular
        </button>
      </div>
      {msg && (
        <p role="alert" className="mt-2 text-red-700">
          {msg}
        </p>
      )}
      {r && (
        <div className="mt-3 space-y-4">
          {r.warnings.length > 0 && (
            <ul className="text-amber-800">
              {r.warnings.map((w) => (
                <li key={w}>⚠ {w}</li>
              ))}
            </ul>
          )}
          {!r.accounts.balanced && (
            <p role="alert" className="text-red-700">
              El balance agregado no cuadra.
            </p>
          )}
          {r.eliminations.length > 0 && (
            <section>
              <h4 className="font-medium">Eliminaciones</h4>
              <table>
                <tbody>
                  {r.eliminations.map((e) => (
                    <tr key={`${e.company}-${e.account}`}>
                      <td className="pr-3">{e.company}</td>
                      <td className="pr-3 font-mono">{e.account}</td>
                      <td className="pr-3">{e.counterpart}</td>
                      <td className="text-right tabular-nums">{euros(e.amount)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </section>
          )}
          <section>
            <h4 className="font-medium">Cuenta de pérdidas y ganancias del grupo</h4>
            <Lines lines={r.accounts.income.lines} year={year} />
          </section>
          <section>
            <h4 className="font-medium">Balance del grupo</h4>
            <Lines lines={r.accounts.balance.lines} year={year} />
          </section>
        </div>
      )}
    </details>
  );
}

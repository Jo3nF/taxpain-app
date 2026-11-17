import { useCallback, useEffect, useState } from 'react';
import {
  errorMessage,
  euros,
  parseEuros,
  type StockLine,
  stockLines,
  stockRegularize,
} from './api';

const plain = (c: number) => (c / 100).toFixed(2).replace('.', ',');

/** Regularización de existencias al cierre: inventario final de cada cuenta del grupo 3. */
export function Existencias({ year }: { year: number }) {
  const [lines, setLines] = useState<StockLine[]>([]);
  const [values, setValues] = useState<Record<string, string>>({});
  const [extra, setExtra] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    stockLines(year)
      .then((l) => {
        setLines(l);
        setValues(Object.fromEntries(l.map((x) => [x.account, plain(x.final_amount)])));
      })
      .catch((e) => setError(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);
  const save = () => {
    const finals: { account: string; amount: number }[] = [];
    for (const [account, v] of Object.entries(values)) {
      const amount = parseEuros(v || '0');
      if (amount === null) {
        setError(`Revisa el importe de ${account}.`);
        return;
      }
      if (amount !== 0) finals.push({ account, amount });
    }
    stockRegularize(year, finals)
      .then((id) => {
        setError(null);
        setMsg(id ? 'Existencias regularizadas.' : 'Sin existencias que regularizar.');
        load();
      })
      .catch((e) => setError(errorMessage(e)));
  };
  return (
    <section aria-label="Existencias" className="mt-6 text-sm">
      <p className="font-medium">Existencias finales {year}</p>
      <p className="text-stone-600">
        Si la empresa tiene existencias (mercaderías, materias primas, productos), indica el
        inventario a 31/12. Se da de baja el saldo inicial y de alta el final contra las cuentas de
        variación (610–612, 710–713). Hazlo antes de cerrar; se puede repetir.
      </p>
      {Object.keys(values).length > 0 && (
        <table className="mt-2">
          <thead className="text-left text-stone-500">
            <tr>
              <th>Cuenta</th>
              <th className="px-2 text-right">Inicial</th>
              <th className="px-2 text-right">Final</th>
            </tr>
          </thead>
          <tbody>
            {Object.keys(values).map((account) => {
              const l = lines.find((x) => x.account === account);
              return (
                <tr key={account}>
                  <td className="pr-2">
                    <span className="font-mono">{account}</span> {l?.name}
                  </td>
                  <td className="px-2 text-right tabular-nums">{euros(l?.initial ?? 0)}</td>
                  <td className="px-2">
                    <input
                      aria-label={`Existencias finales de ${account}`}
                      className="w-32 text-right"
                      value={values[account] ?? ''}
                      onChange={(e) => setValues({ ...values, [account]: e.target.value })}
                    />
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      <div className="mt-3 flex flex-wrap items-end gap-2">
        <label>
          Añadir cuenta (30–36)
          <input
            className="mt-1 block w-32 font-mono"
            value={extra}
            onChange={(e) => setExtra(e.target.value.trim())}
          />
        </label>
        <button
          type="button"
          className="rounded border border-stone-300 px-3"
          onClick={() => {
            if (/^3[0-6]\d*$/.test(extra) && !(extra in values)) {
              setValues({ ...values, [extra]: '0,00' });
              setExtra('');
            } else setError('Pon una cuenta de existencias (de 30 a 36).');
          }}
        >
          Añadir
        </button>
      </div>
      <button type="button" className="mt-3 rounded btn-primary px-3 py-1" onClick={save}>
        Regularizar existencias
      </button>
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}
    </section>
  );
}

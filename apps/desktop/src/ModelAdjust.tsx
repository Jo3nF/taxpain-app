import { useCallback, useEffect, useState } from 'react';
import { errorMessage, euros, modelAdjust, modelAdjustments, parseEuros } from './api';

/**
 * Casillas del modelo fijadas a mano: sustituyen al cálculo (los totales y el resultado se
 * recalculan con ellas) hasta que se vuelve al automático: 303, 130, 111, 115 y el 202 por la
 * base ([07], [08], [14], [27], [28], [30] y [44]).
 */
export function ModelAdjust({
  model,
  year,
  quarter,
  onChange,
}: {
  model: string;
  year: number;
  quarter: number;
  onChange: () => void;
}) {
  const [list, setList] = useState<[number, number][]>([]);
  const [box, setBox] = useState('');
  const [amount, setAmount] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(() => {
    modelAdjustments(model, year, quarter)
      .then(setList)
      .catch(() => setList([]));
  }, [model, year, quarter]);
  useEffect(load, [load]);
  if (!['303', '130', '111', '115', '202'].includes(model)) return null;
  const run = async (f: () => Promise<void>, ok: string) => {
    try {
      await f();
      setMsg(ok);
      load();
      onChange();
    } catch (e) {
      setMsg(errorMessage(e));
    }
  };
  return (
    <details className="mt-4 rounded-lg border border-stone-200 bg-white p-3 text-sm">
      <summary>
        Corregir una casilla a mano{list.length > 0 ? ` (${list.length} fijadas)` : ''}
      </summary>
      <p className="mt-1 text-stone-600">
        Úsalo para lo que TaxPain no calcula. La casilla queda fijada en este periodo y los totales
        se recalculan; vuelve al automático cuando ya no haga falta.
      </p>
      {list.length > 0 && (
        <ul className="mt-2">
          {list.map(([b, m]) => (
            <li key={b} className="flex items-center gap-2">
              [{String(b).padStart(2, '0')}] = {euros(m)}
              <button
                type="button"
                className="underline"
                onClick={() =>
                  run(() => modelAdjust(model, year, quarter, b, null), `[${b}] vuelve al cálculo.`)
                }
              >
                Volver al automático
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="mt-3 flex flex-wrap items-end gap-3">
        <label>
          Casilla
          <input
            className="mt-1 block w-24 font-mono"
            inputMode="numeric"
            value={box}
            onChange={(e) => setBox(e.target.value)}
          />
        </label>
        <label>
          Importe
          <input
            className="mt-1 block w-36 text-right"
            inputMode="decimal"
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
          />
        </label>
        <button
          type="button"
          className="rounded border border-stone-300 px-3"
          onClick={() => {
            const b = Number.parseInt(box, 10);
            const m = parseEuros(amount);
            if (!(b > 0) || m === null) {
              setMsg('Indica el número de casilla y el importe');
              return;
            }
            void run(() => modelAdjust(model, year, quarter, b, m), `[${b}] fijada a mano.`);
          }}
        >
          Fijar
        </button>
      </div>
      {msg && <p role="status">{msg}</p>}
    </details>
  );
}

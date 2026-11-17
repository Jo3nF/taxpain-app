import { useEffect, useState } from 'react';
import { type Company, chartCopyFrom, companiesList, errorMessage, plural } from './api';

/** Copia el plan de cuentas de otra empresa del mismo Mac (útil al dar de alta una nueva). */
export function ChartCopy({ onDone }: { onDone: () => void }) {
  const [others, setOthers] = useState<Company[]>([]);
  const [chosen, setChosen] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    companiesList()
      .then((l) => setOthers(l.filter((c) => !c.active)))
      .catch(() => setOthers([]));
  }, []);
  if (others.length === 0) return null;
  return (
    <div className="mt-4 flex flex-wrap items-end gap-3">
      <label>
        Copiar el plan de cuentas de
        <select className="mt-1 block" value={chosen} onChange={(e) => setChosen(e.target.value)}>
          <option value="">elige una empresa</option>
          {others.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
      </label>
      <button
        type="button"
        disabled={!chosen}
        className="rounded border border-stone-300 px-2 py-1 disabled:opacity-50"
        onClick={async () => {
          try {
            const r = await chartCopyFrom(chosen);
            setMsg(
              `${plural(r.accounts, 'cuenta copiada', 'cuentas copiadas')}${
                r.posting_accounts ? ' y las cuentas por defecto' : ''
              }.`,
            );
            onDone();
          } catch (e) {
            setMsg(errorMessage(e));
          }
        }}
      >
        Copiar
      </button>
      {msg && (
        <span role="status" className="pb-2">
          {msg}
        </span>
      )}
    </div>
  );
}

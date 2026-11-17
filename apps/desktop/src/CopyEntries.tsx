import { useEffect, useState } from 'react';
import { type Company, companiesList, copyEntriesTo, errorMessage } from './api';

/** Copia a otra empresa del Mac los asientos que salen en la búsqueda del diario. */
export function CopyEntries({ ids, year }: { ids: number[]; year: number }) {
  const [companies, setCompanies] = useState<Company[]>([]);
  const [target, setTarget] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    companiesList()
      .then((l) => setCompanies(l.filter((c) => !c.active)))
      .catch(() => setCompanies([]));
  }, []);
  if (companies.length === 0 || ids.length === 0) return null;
  return (
    <div className="mt-3 flex flex-wrap items-end gap-3 text-sm">
      <label>
        Copiar los {ids.length} asientos que salen a
        <select className="mt-1 block" value={target} onChange={(e) => setTarget(e.target.value)}>
          <option value="">elige empresa…</option>
          {companies.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
      </label>
      <button
        type="button"
        className="rounded border border-stone-300 px-3 disabled:opacity-50"
        disabled={!target}
        onClick={() =>
          copyEntriesTo(ids, year, target)
            .then((r) =>
              setMsg(
                `${r.copied} asiento(s) copiados como manuales.` +
                  (r.third_party_accounts.length
                    ? ` Revisa en la otra empresa las subcuentas de clientes o proveedores ${r.third_party_accounts.join(', ')}: allí pueden ser de otro tercero.`
                    : ''),
              ),
            )
            .catch((e) => setMsg(errorMessage(e)))
        }
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

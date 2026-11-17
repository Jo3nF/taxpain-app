import { type FormEvent, useState } from 'react';
import { useAccounts } from './accounts';
import { errorMessage, euros, setAccount } from './api';
import { ChartCopy } from './ChartCopy';
import { PostingAccountsForm } from './PostingAccounts';

const SOURCE: Record<string, string> = {
  propia: 'Propia',
  tercero: 'Cliente/proveedor',
  pgc: 'PGC',
};

/** Plan de cuentas de la empresa: buscar, dar de alta o renombrar y abrir el mayor. */
export function ChartOfAccounts({
  year,
  onOpenLedger,
}: {
  year: number;
  onOpenLedger: (code: string) => void;
}) {
  const { plan, error, reload } = useAccounts(year);
  const [filter, setFilter] = useState('');
  const [code, setCode] = useState('');
  const [name, setName] = useState('');
  const [msg, setMsg] = useState<string | null>(null);

  const f = filter.trim().toLowerCase();
  const rows = plan.filter(
    (a) =>
      !f ||
      a.code.startsWith(f) ||
      a.name.toLowerCase().includes(f) ||
      a.nif?.toLowerCase().includes(f),
  );

  async function save(e: FormEvent) {
    e.preventDefault();
    try {
      await setAccount(code, name);
      setMsg(`Cuenta ${code.trim()} guardada.`);
      setCode('');
      setName('');
      reload();
    } catch (err) {
      setMsg(errorMessage(err));
    }
  }

  return (
    <div className="mt-4 text-sm">
      <form onSubmit={save} className="flex flex-wrap items-end gap-3" aria-label="Alta de cuenta">
        <label>
          Código
          <input
            className="mt-1 block w-32 rounded border px-1 font-mono"
            value={code}
            onChange={(e) => setCode(e.target.value)}
            inputMode="numeric"
            required
          />
        </label>
        <label>
          Nombre
          <input
            className="mt-1 block w-72 rounded border px-1"
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
          />
        </label>
        <button type="submit" className="rounded border border-stone-300 px-2 py-1">
          Dar de alta o renombrar
        </button>
      </form>
      {msg && <p role="status">{msg}</p>}
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      <PostingAccountsForm />
      <ChartCopy onDone={reload} />
      <label className="mt-4 block">
        Buscar
        <input
          aria-label="Buscar cuenta"
          className="mt-1 block w-64"
          placeholder="código, nombre o NIF"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      </label>
      <table className="mt-2 w-full">
        <thead className="text-left text-stone-500">
          <tr>
            <th>Cuenta</th>
            <th>Nombre</th>
            <th>Tipo</th>
            <th className="text-right">Debe</th>
            <th className="text-right">Haber</th>
            <th className="text-right">Saldo</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((a) => (
            <tr key={a.code} className="border-t border-stone-200">
              <td className="font-mono">
                <button
                  type="button"
                  className="text-link underline-offset-2 hover:underline"
                  title="Ver el mayor"
                  onClick={() => onOpenLedger(a.code)}
                >
                  {a.code}
                </button>
              </td>
              <td>
                {a.name}
                {a.nif && <span className="text-stone-500"> · {a.nif}</span>}
              </td>
              <td className="text-stone-500">{SOURCE[a.source]}</td>
              <td className="text-right tabular-nums">{a.debit ? euros(a.debit) : ''}</td>
              <td className="text-right tabular-nums">{a.credit ? euros(a.credit) : ''}</td>
              <td className="text-right tabular-nums">{euros(a.debit - a.credit)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {rows.length === 0 && <p className="mt-2 text-stone-500">Sin cuentas que coincidan.</p>}
    </div>
  );
}

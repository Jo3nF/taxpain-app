import { useEffect, useState } from 'react';
import {
  type PostingAccounts as Accounts,
  errorMessage,
  postingAccounts,
  savePostingAccounts,
} from './api';

const FIELDS: { key: Exclude<keyof Accounts, 'subaccount_width'>; label: string }[] = [
  { key: 'revenue', label: 'Ventas e ingresos (705, 700…)' },
  { key: 'expense', label: 'Compras y gastos (629, 600…)' },
  { key: 'customers', label: 'Clientes (430)' },
  { key: 'suppliers', label: 'Proveedores o acreedores (400, 410)' },
  { key: 'vat_output', label: 'IVA repercutido (477)' },
  { key: 'vat_input', label: 'IVA soportado (472)' },
  { key: 'withholding_receivable', label: 'Retenciones que te practican (473)' },
  { key: 'withholding_payable', label: 'Retenciones que practicas (4751)' },
  { key: 'bank', label: 'Banco por defecto (572)' },
];

/** Cuentas que usa la contabilización automática de facturas, cobros y pagos. */
export function PostingAccountsForm() {
  const [a, setA] = useState<Accounts | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    postingAccounts()
      .then(setA)
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  if (!a) return msg ? <p role="alert">{msg}</p> : null;
  return (
    <details className="mt-4 rounded border border-stone-200 p-3">
      <summary>Cuentas al contabilizar facturas (por defecto)</summary>
      <form
        className="form-grid mt-3"
        onSubmit={async (e) => {
          e.preventDefault();
          try {
            await savePostingAccounts(a);
            setMsg('Cuentas guardadas: se usan en lo que se contabilice a partir de ahora.');
          } catch (err) {
            setMsg(errorMessage(err));
          }
        }}
      >
        {FIELDS.map((f) => (
          <label key={f.key}>
            {f.label}
            <input
              className="font-mono"
              value={a[f.key]}
              inputMode="numeric"
              onChange={(e) => setA({ ...a, [f.key]: e.target.value.trim() })}
            />
          </label>
        ))}
        <label>
          Dígitos de las subcuentas de clientes y proveedores
          <input
            type="number"
            min={6}
            max={12}
            value={a.subaccount_width}
            onChange={(e) => setA({ ...a, subaccount_width: Number(e.target.value) })}
          />
        </label>
        <p className="span-full text-stone-600">
          Los dígitos sólo se pueden cambiar antes de tener clientes o proveedores. Para un cliente
          o proveedor concreto, pon su cuenta en «Cobros y pagos → Condiciones».
        </p>
        <div className="span-full form-actions">
          <button type="submit" className="rounded border border-stone-300 px-2 py-1">
            Guardar cuentas
          </button>
        </div>
      </form>
      {msg && <p role="status">{msg}</p>}
    </details>
  );
}

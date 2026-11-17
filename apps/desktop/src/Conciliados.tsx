import { useCallback, useEffect, useState } from 'react';
import {
  bankReconciled,
  bankUnreconcile,
  errorMessage,
  formatDate,
  money,
  type Reconciled,
  type ReconciledList,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const lowerFirst = (s: string) => s.charAt(0).toLowerCase() + s.slice(1);

/** Qué pasa al deshacer, según con qué se concilió. */
export function undoQuestion(r: Reconciled): string {
  const base = `¿Deshacer la conciliación del movimiento del ${formatDate(r.tx.op_date)} (${r.tx.description})?`;
  switch (r.kind) {
    case 'emitida':
      return `${base} Se borra el asiento del cobro y la ${lowerFirst(r.counterpart)} vuelve a estar pendiente de cobro en la cartera.`;
    case 'recibida':
      return `${base} Se borra el asiento del pago y la ${lowerFirst(r.counterpart)} vuelve a estar pendiente de pago en la cartera.`;
    case 'remesa':
      return `${base} Se borra el asiento del abono del banco; la ${lowerFirst(r.counterpart)} sigue hecha, a la espera de casarla con el movimiento correcto.`;
    case 'devolucion':
      return `${base} El movimiento vuelve a pendientes; la devolución sigue anotada en Remesas.`;
    case 'impuesto':
      return `${base} Se borra el asiento del pago y el ${lowerFirst(r.counterpart)} vuelve a estar pendiente de pago.`;
    case 'traspaso':
      return `${base} Se borra el asiento del traspaso y los dos movimientos (el de cada cuenta) vuelven a pendientes.`;
    default:
      return `${base} Se borra su asiento contra la cuenta ${r.counterpart} y el movimiento vuelve a pendientes.`;
  }
}

/**
 * Movimientos del banco ya conciliados, con a qué factura, remesa, modelo o cuenta se llevaron,
 * para deshacer una conciliación equivocada.
 */
export function Conciliados({ version, onDone }: { version: number; onDone: () => void }) {
  const [open, setOpen] = useState(false);
  const [account, setAccount] = useState('');
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [data, setData] = useState<ReconciledList | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const load = useCallback(() => {
    bankReconciled(account || null, from || null, to || null)
      .then(setData)
      .catch((e) => setMsg(errorMessage(e)));
  }, [account, from, to]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: recarga cuando cambian los movimientos
  useEffect(() => {
    if (open) load();
  }, [open, load, version]);

  return (
    <section aria-label="Movimientos conciliados" className="mt-4 text-sm">
      <button type="button" className="text-link" onClick={() => setOpen(!open)}>
        {open ? 'Ocultar los movimientos conciliados' : 'Ver los movimientos conciliados…'}
      </button>
      {open && (
        <div className="mt-2 space-y-2">
          <div className="flex flex-wrap items-center gap-2">
            <label className="flex items-center gap-1">
              Cuenta
              <select
                className="rounded border px-1"
                value={account}
                onChange={(e) => setAccount(e.target.value)}
              >
                <option value="">Todas</option>
                {data?.accounts.map((a) => (
                  <option key={a.account} value={a.account}>
                    {a.name} ({a.account})
                  </option>
                ))}
              </select>
            </label>
            <label className="flex items-center gap-1">
              Desde
              <input
                type="date"
                className="rounded border px-1"
                value={from}
                onChange={(e) => setFrom(e.target.value)}
              />
            </label>
            <label className="flex items-center gap-1">
              Hasta
              <input
                type="date"
                className="rounded border px-1"
                value={to}
                onChange={(e) => setTo(e.target.value)}
              />
            </label>
          </div>
          {msg && (
            <p role="status" className="text-stone-700">
              {msg}
            </p>
          )}
          {data && data.items.length === 0 && (
            <p className="text-stone-600">No hay movimientos conciliados con esos filtros.</p>
          )}
          <ul className="divide-y divide-stone-200">
            {data?.items.map((r) => (
              <li key={r.tx.id} className="py-2">
                <div className="flex justify-between gap-2">
                  <span>
                    {formatDate(r.tx.op_date)} · {r.tx.description}
                  </span>
                  <span className={r.tx.amount < 0 ? 'text-red-700' : 'text-emerald-800'}>
                    {money(r.tx.amount, r.tx.currency)}
                  </span>
                </div>
                <div className="text-stone-600">
                  → {r.kind === 'cuenta' ? `Cuenta ${r.counterpart}` : r.counterpart}
                  {r.party ? ` · ${r.party}` : ''}
                  {r.rule ? ` (regla «${r.rule}»)` : ''} · {r.tx.bank_account}
                </div>
                {r.blocked ? (
                  <p className="text-xs text-amber-800">No se puede deshacer: {r.blocked}.</p>
                ) : (
                  <ConfirmAction
                    label="Deshacer conciliación"
                    className="text-xs text-red-700 underline"
                    question={undoQuestion(r)}
                    confirmLabel="Sí, deshacer"
                    onConfirm={async () => {
                      await bankUnreconcile(r.tx.id, true);
                      setMsg('Conciliación deshecha: el movimiento vuelve a estar pendiente.');
                      onDone();
                    }}
                  />
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}

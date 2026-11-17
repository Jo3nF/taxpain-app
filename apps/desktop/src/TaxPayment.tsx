import { useCallback, useEffect, useRef, useState } from 'react';
import {
  errorMessage,
  euros,
  formatDate,
  type TaxPaymentView,
  taxPay,
  taxPayBank,
  taxPayment,
  taxUnpay,
} from './api';
import { ConfirmAction } from './ConfirmAction';

function today(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

/** Pago del modelo contabilizado: con un cargo del banco ya importado o a mano. */
export function TaxPaymentPanel({
  model,
  year,
  quarter,
  amount,
}: {
  model: string;
  year: number;
  quarter: number;
  amount: number;
}) {
  const [view, setView] = useState<TaxPaymentView | null>(null);
  const [date, setDate] = useState(today());
  const [treasury, setTreasury] = useState('572');
  const [msg, setMsg] = useState<string | null>(null);
  // Una acción a la vez: un doble clic registraría el pago dos veces (y el error taparía el éxito).
  const [busy, setBusy] = useState(false);
  const running = useRef(false);

  const load = useCallback(() => {
    taxPayment(model, year, quarter)
      .then(setView)
      .catch(() => setView(null));
  }, [model, year, quarter]);
  useEffect(load, [load]);

  if (!view?.payable || amount <= 0) return null;
  const run = async (f: () => Promise<unknown>, ok: string) => {
    if (running.current) return;
    running.current = true;
    setBusy(true);
    try {
      await f();
      setMsg(ok);
      load();
    } catch (e) {
      setMsg(errorMessage(e));
    } finally {
      running.current = false;
      setBusy(false);
    }
  };
  const p = view.payment;

  return (
    <section
      aria-label="Pago del modelo"
      className="mt-4 rounded-lg border border-stone-200 bg-white p-3 text-sm"
    >
      <h3 className="font-medium">Pago</h3>
      {p ? (
        <div className="mt-1 flex flex-wrap items-center gap-3">
          <span>
            Pagado el {formatDate(p.date)}: {euros(p.amount)} ({p.account} contra {p.treasury}
            {p.bank_tx ? ', conciliado con el banco' : ''}).
          </span>
          {!p.bank_tx && (
            <ConfirmAction
              label="Deshacer"
              className="rounded border border-stone-300 px-2 py-1"
              question={`Se borrará de la contabilidad el asiento del pago del ${formatDate(p.date)}.`}
              confirmLabel="Sí, deshacer el pago"
              onConfirm={() => run(() => taxUnpay(model, year, quarter), 'Pago deshecho.')}
            />
          )}
        </div>
      ) : (
        <>
          <p className="mt-1 text-stone-600">
            Registra el pago de {euros(amount)} para que quede en la contabilidad.
            {model === '303' && ' Si falta, antes se contabiliza la liquidación del trimestre.'}
          </p>
          {view.candidates.length > 0 && (
            <ul className="mt-2 space-y-1">
              {view.candidates.map((t) => (
                <li key={t.id} className="flex flex-wrap items-center gap-2">
                  <span>
                    Cargo del {formatDate(t.op_date)} en {t.bank_account}: {t.description}
                  </span>
                  <button
                    type="button"
                    disabled={busy}
                    className="rounded btn-primary px-2 py-1"
                    onClick={() =>
                      run(
                        () => taxPayBank(model, year, quarter, t.id),
                        'Pago registrado y movimiento conciliado.',
                      )
                    }
                  >
                    Es este cargo
                  </button>
                </li>
              ))}
            </ul>
          )}
          <div className="mt-3 flex flex-wrap items-end gap-3">
            <label className="flex flex-col">
              Fecha
              <input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
            </label>
            <label className="flex flex-col">
              Cuenta de pago
              <input
                value={treasury}
                onChange={(e) => setTreasury(e.target.value)}
                className="w-32 font-mono"
              />
            </label>
            <button
              type="button"
              disabled={busy}
              className="rounded border border-stone-300 px-3 py-1"
              onClick={() =>
                run(() => taxPay(model, year, quarter, date, treasury), 'Pago registrado.')
              }
            >
              Registrar pago
            </button>
          </div>
        </>
      )}
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}
    </section>
  );
}

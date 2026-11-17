import { useCallback, useEffect, useState } from 'react';
import {
  closingFxValuation,
  closingFxValuationPost,
  closingFxValuationUndo,
  errorMessage,
  euros,
  type FxValuation,
  type FxValuationInput,
  formatDate,
  fxBceRate,
  money,
  parseEuros,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const cell = 'px-2 py-1 text-right tabular-nums';
const plain = (c: number) => (c / 100).toFixed(2).replace('.', ',');
const KIND: Record<string, string> = { banco: 'Banco', cliente: 'Cliente', proveedor: 'Proveedor' };

/**
 * Paso del cierre: valoración de las partidas monetarias en moneda extranjera (bancos en divisa,
 * clientes y proveedores con facturas en divisa pendientes) al tipo de cierre, con su asiento a
 * 668/768 el último día del ejercicio y la reversión el primero del siguiente (PGC, NRV 11.ª).
 */
export function ValoracionDivisa({ year, onChanged }: { year: number; onChanged: () => void }) {
  const [p, setP] = useState<FxValuation | null>(null);
  // Sólo lo que el usuario cambia (lo demás sale del BCE o del extracto).
  const [rates, setRates] = useState<Record<string, string>>({});
  const [balances, setBalances] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const input = useCallback((): FxValuationInput | null => {
    const saldos: Record<string, number> = {};
    for (const [account, text] of Object.entries(balances)) {
      const c = parseEuros(text || '0');
      if (c === null) {
        setError(`Revisa el saldo en divisa de ${account}.`);
        return null;
      }
      saldos[account] = c;
    }
    const tipos: Record<string, string> = {};
    for (const [c, text] of Object.entries(rates)) {
      if (text.trim()) tipos[c] = text.trim().replace(',', '.');
    }
    return { tipos, saldos };
  }, [rates, balances]);
  const load = useCallback(
    (i?: FxValuationInput) => {
      closingFxValuation(year, i)
        .then((r) => {
          setP(r ?? null);
          setError(null);
        })
        .catch((e) => setError(errorMessage(e)));
    },
    [year],
  );
  useEffect(() => load(), [load]);
  if (!p?.items || (p.items.length === 0 && p.entry_id === null)) {
    return error ? (
      <p role="alert" className="text-red-700">
        {error}
      </p>
    ) : null;
  }
  const posted = p.entry_id !== null;
  const recalc = () => {
    const i = input();
    if (i) load(i);
  };
  return (
    <section aria-label="Saldos en moneda extranjera" className="card p-5">
      <h3 className="font-semibold">Saldos en moneda extranjera al cierre de {year}</h3>
      <p className="mt-2 text-stone-700">
        Las cuentas del banco en divisa y lo pendiente de cobrar o pagar de facturas en divisa se
        valoran al tipo de cierre (el del BCE del {formatDate(p.date)} o el último día hábil
        anterior). La diferencia con los euros contabilizados va a 768 (a favor) o 668 (en contra)
        con fecha {formatDate(p.date)}, y se revierte el {formatDate(p.reversal_date)}: así el cobro
        o pago del año que viene sigue saliendo por los euros de la factura (NRV 11.ª del PGC). Los
        anticipos (438, 407) no se valoran: son partidas no monetarias.
      </p>
      <table className="mt-3">
        <thead className="text-left text-stone-500">
          <tr>
            <th className="font-normal">Divisa</th>
            <th className="px-2 font-normal">Tipo de cierre (por 1 €)</th>
            <th className="font-normal" />
          </tr>
        </thead>
        <tbody>
          {p.rates.map((r) => (
            <tr key={r.currency}>
              <td className="pr-2 font-mono">{r.currency}</td>
              <td className="px-2">
                <input
                  aria-label={`Tipo de cierre del ${r.currency}`}
                  className="w-28 text-right"
                  disabled={posted}
                  value={rates[r.currency] ?? (r.rate ?? '').replace('.', ',')}
                  onChange={(e) => setRates({ ...rates, [r.currency]: e.target.value })}
                />{' '}
                <span className="text-stone-600">
                  {r.source === 'bce' && r.date
                    ? `BCE del ${formatDate(r.date)}`
                    : r.source === 'manual'
                      ? 'tecleado'
                      : 'falta'}
                </span>
              </td>
              <td>
                {!posted && (
                  <button
                    type="button"
                    className="rounded border border-stone-300 px-2"
                    onClick={() => {
                      fxBceRate(r.currency, p.date)
                        .then(() => {
                          const rest = { ...rates };
                          delete rest[r.currency];
                          setRates(rest);
                          load({ ...(input() ?? { saldos: {} }), tipos: rest });
                        })
                        .catch((e) => setError(errorMessage(e)));
                    }}
                  >
                    Tipo del BCE del {formatDate(p.date)}
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <table className="mt-4 w-full">
        <thead className="text-left text-stone-500">
          <tr>
            <th className="font-normal">Partida</th>
            <th className="px-2 text-right font-normal">En divisa</th>
            <th className="px-2 text-right font-normal">Contabilizado</th>
            <th className="px-2 text-right font-normal">Al tipo de cierre</th>
            <th className="px-2 text-right font-normal">Diferencia</th>
          </tr>
        </thead>
        <tbody>
          {p.items.map((i) => (
            <tr key={`${i.kind}-${i.account}-${i.label}`}>
              <td className="pr-2">
                {KIND[i.kind]} <span className="font-mono">{i.account}</span> {i.label}
              </td>
              <td className={cell}>
                {i.kind === 'banco' && !posted ? (
                  <input
                    aria-label={`Saldo en ${i.currency} de ${i.account}`}
                    className="w-32 text-right"
                    value={balances[i.account] ?? plain(i.amount)}
                    onChange={(e) => setBalances({ ...balances, [i.account]: e.target.value })}
                  />
                ) : (
                  money(i.amount, i.currency)
                )}
              </td>
              <td className={cell}>{euros(i.booked)}</td>
              <td className={cell}>{i.valued === null ? '—' : euros(i.valued)}</td>
              <td className={cell}>{i.difference === null ? '—' : euros(i.difference)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {!posted && (
        <button
          type="button"
          className="mt-2 rounded border border-stone-300 px-3 py-1"
          onClick={recalc}
        >
          Recalcular con lo tecleado
        </button>
      )}
      {p.lines.length > 0 && (
        <>
          <p className="mt-4 text-stone-700">
            Asiento del {formatDate(p.date)} (diferencias a favor {euros(p.gains)}, en contra{' '}
            {euros(p.losses)}):
          </p>
          <table className="mt-2 w-full">
            <thead className="text-left text-stone-500">
              <tr>
                <th className="font-normal">Cuenta</th>
                <th className="px-2 text-right font-normal">Debe</th>
                <th className="px-2 text-right font-normal">Haber</th>
              </tr>
            </thead>
            <tbody>
              {p.lines.map((l) => (
                <tr key={l.account}>
                  <td>
                    <span className="font-mono">{l.account}</span> {l.name}
                  </td>
                  <td className={cell}>{l.debit ? euros(l.debit) : ''}</td>
                  <td className={cell}>{l.credit ? euros(l.credit) : ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
      {p.warnings.length > 0 && (
        <ul className="mt-3 space-y-1 rounded-md bg-amber-50 p-3 text-amber-950">
          {p.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
      <div className="mt-3">
        {!posted ? (
          p.lines.length > 1 &&
          !p.closed && (
            <ConfirmAction
              label="Contabilizar la valoración"
              question={`¿Contabilizar las diferencias de cambio al ${formatDate(p.date)} (a favor ${euros(p.gains)}, en contra ${euros(p.losses)}) y su reversión el ${formatDate(p.reversal_date)}?`}
              confirmLabel="Sí, contabilizar"
              danger={false}
              className="btn-primary rounded-md px-3 py-1.5"
              onConfirm={async () => {
                const i = input();
                if (!i) return;
                await closingFxValuationPost(year, i);
                setRates({});
                setBalances({});
                load();
                onChanged();
              }}
            />
          )
        ) : (
          <p className="flex flex-wrap items-center gap-3">
            <span className="pill pill-green">Contabilizada</span>
            <ConfirmAction
              label="Deshacer"
              question="¿Quitar el asiento de la valoración y su reversión? Sólo con los dos ejercicios abiertos."
              confirmLabel="Sí, quitarlos"
              onConfirm={async () => {
                await closingFxValuationUndo(year);
                load();
                onChanged();
              }}
            />
          </p>
        )}
      </div>
    </section>
  );
}

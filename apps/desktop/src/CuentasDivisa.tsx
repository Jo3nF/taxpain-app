import { useEffect, useState } from 'react';
import {
  type BankBalance,
  type BankTx,
  bankBalances,
  bankSetFx,
  errorMessage,
  euros,
  formatDate,
  fxBceRate,
  money,
  parseEuros,
} from './api';

const SOURCE: Record<string, string> = {
  bce: 'tipo del BCE',
  manual: 'tecleado',
  traspaso: 'euros del traspaso',
};

/**
 * Contravalor en euros de un movimiento de una cuenta en divisa: con él se contabiliza. Sale del
 * tipo del BCE de su día (si ya está descargado), de los euros que da el extracto (tecleados) o,
 * en un traspaso con una cuenta en euros, de la otra cara.
 */
export function ContravalorMovimiento({ m, onDone }: { m: BankTx; onDone: () => void }) {
  const [eur, setEur] = useState('');
  const [error, setError] = useState<string | null>(null);
  if (!m.currency || m.currency === 'EUR') return null;
  const run = (p: Promise<unknown>) =>
    p.then(
      () => {
        setError(null);
        setEur('');
        onDone();
      },
      (e) => setError(errorMessage(e)),
    );
  return (
    <div className="mt-1 flex flex-wrap items-center gap-2 pl-4 text-stone-700">
      {m.amount_eur != null ? (
        <span>
          = {euros(m.amount_eur)} (1 € = {(m.fx_rate ?? '').replace('.', ',')} {m.currency},{' '}
          {SOURCE[m.fx_source ?? ''] ?? m.fx_source})
        </span>
      ) : (
        <span className="text-amber-800">
          Sin contravalor en euros: falta el tipo del BCE del {formatDate(m.op_date)}.
        </span>
      )}
      <button
        type="button"
        className="rounded border px-2"
        onClick={() => run(fxBceRate(m.currency ?? '', m.op_date))}
      >
        Tipo del BCE
      </button>
      <input
        aria-label={`Euros de ${money(m.amount, m.currency)}`}
        placeholder="Euros del extracto"
        className="w-32 rounded border px-1 text-right"
        value={eur}
        onChange={(e) => setEur(e.target.value)}
      />
      <button
        type="button"
        className="rounded border px-2"
        onClick={() => {
          const c = parseEuros(eur);
          if (c === null || c === 0) {
            setError('Escribe los euros del movimiento.');
            return;
          }
          // Los euros llevan el signo del movimiento.
          run(bankSetFx(m.id, m.amount < 0 ? -Math.abs(c) : Math.abs(c), null));
        }}
      >
        Guardar euros
      </button>
      {m.fx_source === 'manual' && (
        <button
          type="button"
          className="text-link underline"
          onClick={() => run(bankSetFx(m.id, null, null))}
        >
          Quitar lo tecleado
        </button>
      )}
      {error && (
        <p role="alert" className="w-full text-red-700">
          {error}
        </p>
      )}
    </div>
  );
}

/** Saldos de las cuentas del banco en su divisa y en euros (sólo si hay alguna en otra divisa). */
export function SaldosCuentas({ version }: { version: number }) {
  const [list, setList] = useState<BankBalance[]>([]);
  useEffect(() => {
    // Se vuelve a leer tras cada importación o conciliación.
    void version;
    bankBalances()
      .then((l) => setList(Array.isArray(l) ? l : []))
      .catch(() => setList([]));
  }, [version]);
  if (!list.some((b) => b.currency !== 'EUR')) return null;
  return (
    <section aria-label="Saldos de las cuentas" className="mt-4 text-sm">
      <h3 className="font-medium">Saldos de las cuentas</h3>
      <table className="mt-1 w-full">
        <thead className="text-left text-stone-500">
          <tr>
            <th className="font-normal">Cuenta</th>
            <th className="px-2 text-right font-normal">Saldo (extracto)</th>
            <th className="px-2 text-right font-normal">Contabilidad</th>
            <th className="px-2 text-right font-normal">Al último tipo del BCE</th>
          </tr>
        </thead>
        <tbody>
          {list.map((b) => (
            <tr key={`${b.account}-${b.currency}-${b.name}`}>
              <td className="pr-2">
                {b.name} <span className="font-mono">{b.account}</span>
                {b.sin_contravalor > 0 && (
                  <span className="block text-amber-800">
                    {b.sin_contravalor === 1
                      ? '1 movimiento sin contravalor en euros'
                      : `${b.sin_contravalor} movimientos sin contravalor en euros`}
                  </span>
                )}
              </td>
              <td className="px-2 text-right tabular-nums">
                {b.saldo === null ? '—' : money(b.saldo, b.currency)}
              </td>
              <td className="px-2 text-right tabular-nums">{euros(b.contable)}</td>
              <td className="px-2 text-right tabular-nums">
                {b.currency === 'EUR'
                  ? ''
                  : b.al_tipo === null
                    ? '—'
                    : `${euros(b.al_tipo)} (${(b.tipo ?? '').replace('.', ',')} del ${formatDate(b.tipo_fecha ?? '')})`}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="text-xs text-stone-600">
        Las cuentas en divisa se contabilizan en euros: cada movimiento por su contravalor del día.
        La diferencia con el saldo al tipo de hoy se reconoce al cierre (Cierre → saldos en moneda
        extranjera).
      </p>
    </section>
  );
}

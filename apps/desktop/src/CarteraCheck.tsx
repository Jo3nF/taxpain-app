import { useEffect, useState } from 'react';
import {
  carteraMismatches,
  errorMessage,
  euros,
  type Mismatch,
  type PaymentPeriod,
  paymentPeriod,
} from './api';

function today(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

const days = (hundredths: number) =>
  (hundredths / 100).toLocaleString('es-ES', { maximumFractionDigits: 2 });

/**
 * Cuadre de la cartera con la contabilidad y periodo medio de pago a proveedores (dato de la
 * memoria de las cuentas anuales, Resolución del ICAC de 29 de enero de 2016).
 */
export function CarteraCheck() {
  const [year, setYear] = useState(new Date().getFullYear());
  const [rows, setRows] = useState<Mismatch[] | null>(null);
  const [pmp, setPmp] = useState<PaymentPeriod | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    carteraMismatches(today())
      .then(setRows)
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  useEffect(() => {
    paymentPeriod(year)
      .then(setPmp)
      .catch((e) => setMsg(errorMessage(e)));
  }, [year]);
  return (
    <div className="space-y-4 text-sm">
      <section aria-label="Cuadre con la contabilidad">
        <h3 className="font-medium">Cuadre con la contabilidad</h3>
        {rows === null ? null : rows.length === 0 ? (
          <p className="text-emerald-800">
            Los saldos de clientes y proveedores coinciden con lo pendiente de cobro y pago.
          </p>
        ) : (
          <>
            <p className="text-stone-600">
              No coinciden: suele ser un cobro o pago contabilizado a mano, un saldo inicial o un
              anticipo. Revísalo en el mayor de la subcuenta.
            </p>
            <table className="mt-1 w-full">
              <thead>
                <tr className="text-left text-stone-600">
                  <th>Subcuenta</th>
                  <th>Nombre</th>
                  <th className="text-right">Contabilidad</th>
                  <th className="text-right">Cartera</th>
                  <th className="text-right">Diferencia</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.account}>
                    <td className="font-mono">{r.account}</td>
                    <td>{r.name}</td>
                    <td className="text-right tabular-nums">{euros(r.ledger)}</td>
                    <td className="text-right tabular-nums">{euros(r.pending)}</td>
                    <td className="text-right tabular-nums">{euros(r.ledger - r.pending)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        )}
      </section>
      <section aria-label="Periodo medio de pago a proveedores">
        <div className="flex flex-wrap items-center gap-3">
          <h3 className="font-medium">Periodo medio de pago a proveedores</h3>
          <select
            aria-label="Ejercicio"
            value={year}
            onChange={(e) => setYear(Number(e.target.value))}
          >
            {[0, 1, 2].map((n) => {
              const y = new Date().getFullYear() - n;
              return (
                <option key={y} value={y}>
                  {y}
                </option>
              );
            })}
          </select>
        </div>
        {pmp && (
          <dl className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1 sm:grid-cols-4">
            <dt className="text-stone-600">Periodo medio de pago</dt>
            <dd className="font-semibold">{days(pmp.average)} días</dd>
            <dt className="text-stone-600">Ratio de operaciones pagadas</dt>
            <dd>
              {days(pmp.paid_ratio)} días ({euros(pmp.paid_total)})
            </dd>
            <dt className="text-stone-600">Ratio de operaciones pendientes</dt>
            <dd>
              {days(pmp.pending_ratio)} días ({euros(pmp.pending_total)})
            </dd>
          </dl>
        )}
        <p className="mt-1 text-stone-600">
          Cuenta desde la fecha de la factura. El plazo legal máximo es de 60 días (Ley 3/2004).
        </p>
      </section>
      {msg && <p role="status">{msg}</p>}
    </div>
  );
}

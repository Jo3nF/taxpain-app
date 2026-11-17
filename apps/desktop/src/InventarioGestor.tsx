import { useState } from 'react';
import {
  errorMessage,
  euros,
  type FixedAssetInventoryPreview,
  fixedAssetInventoryImport,
  fixedAssetInventoryPreview,
  formatDate,
  pickOpeningFile,
  plural,
} from './api';
import { ConfirmAction } from './ConfirmAction';

/**
 * El inventario de activos del gestor en Inmovilizado, sin pasar por los saldos iniciales: cada
 * bien entra con lo ya amortizado hasta el cierre del listado y sin ningún asiento (sus saldos ya
 * vienen del diario del gestor o de la apertura), con el coeficiente que da lo ya amortizado y
 * las subcuentas del gestor si ya están en TaxPain. Lo que ya está no se repite y los terrenos no
 * entran, porque no se amortizan.
 */
export function InventarioGestor({ year, onDone }: { year: number; onDone: () => void }) {
  const [plan, setPlan] = useState<FixedAssetInventoryPreview | null>(null);
  // Coeficiente anual (%) de cada bien, por si hay que cambiar el que propone TaxPain.
  const [rates, setRates] = useState<string[]>([]);
  const [until, setUntil] = useState('');
  const [msg, setMsg] = useState<string | null>(null);

  async function choose() {
    setMsg(null);
    const f = await pickOpeningFile();
    if (!f) return;
    try {
      const p = await fixedAssetInventoryPreview(f);
      setPlan(p);
      setRates(p.items.map((i) => String(i.asset.rate / 100).replace('.', ',')));
      setUntil(p.until ?? `${year - 1}-12-31`);
    } catch (e) {
      setPlan(null);
      setMsg(errorMessage(e));
    }
  }

  const nuevo = (n: number) => plan?.items[n]?.existing === null && !plan.items[n]?.skip;
  const nuevos = plan ? plan.items.filter((_, n) => nuevo(n)).length : 0;

  async function run() {
    if (!plan) return;
    // Van todos, con el coeficiente de los nuevos: así TaxPain casa cada bien con lo que ya está
    // igual que en la vista previa (dos bienes iguales del listado son dos).
    const assets = plan.items.map((i, n) =>
      nuevo(n)
        ? { ...i.asset, rate: Math.round(Number(rates[n]?.replace(',', '.')) * 100) }
        : i.asset,
    );
    if (!until || assets.some((a, n) => nuevo(n) && !(a.rate > 0 && a.rate <= 10_000))) {
      setMsg('Revisa los coeficientes y la fecha');
      return;
    }
    try {
      const r = await fixedAssetInventoryImport(assets, until);
      setMsg(
        [
          `${plural(r.added, 'bien dado de alta', 'bienes dados de alta')} en Inmovilizado con lo ya amortizado hasta el ${formatDate(until)}, sin ningún asiento.`,
          r.existing > 0 ? `${plural(r.existing, 'ya estaba', 'ya estaban')}.` : '',
          ...r.warnings,
        ]
          .filter(Boolean)
          .join(' '),
      );
      setPlan(null);
      onDone();
    } catch (e) {
      setMsg(errorMessage(e));
    }
  }

  return (
    <div className="space-y-2">
      <button type="button" className="rounded border border-stone-300 px-3 py-1" onClick={choose}>
        Dar de alta el inventario del gestor en Inmovilizado…
      </button>
      <p className="text-xs text-stone-600">
        El «Inventario de activos» de a3 (o un listado con Descripción, Valor de adquisición, Valor
        contable y Fecha de inicio de amortización): cada bien entra con lo ya amortizado y sin
        ningún asiento, porque sus saldos ya vienen del diario del gestor o de la apertura. Su
        coeficiente es el que da lo ya amortizado (el plan del gestor) y, si el diario del gestor ya
        está en TaxPain, su amortización va a las mismas subcuentas. Lo que ya está no se repite y
        los terrenos no entran, porque no se amortizan (PGC, norma de registro y valoración 3.ª a).
      </p>
      {plan && (
        <div className="space-y-2 rounded-md bg-stone-50 p-3">
          <table className="w-full">
            <caption className="sr-only">Bienes del inventario del gestor</caption>
            <thead>
              <tr className="text-left text-stone-600">
                <th scope="col">Bien</th>
                <th scope="col">Cuenta</th>
                <th scope="col">Alta</th>
                <th scope="col" className="text-right">
                  Coste
                </th>
                <th scope="col" className="text-right">
                  % anual
                </th>
                <th scope="col" className="text-right">
                  Ya amortizado
                </th>
                <th scope="col">Qué se hace</th>
              </tr>
            </thead>
            <tbody>
              {plan.items.map((i, n) => (
                <tr key={`${i.asset.account}-${i.asset.acquired_on}-${i.asset.name}`}>
                  <td>{i.asset.name}</td>
                  <td className="tabular-nums">
                    {i.asset.account}
                    {nuevo(n) && (
                      <span className="block text-xs text-stone-600">
                        amortización {i.asset.depreciation_account} · gasto{' '}
                        {i.asset.expense_account}
                      </span>
                    )}
                  </td>
                  <td>{formatDate(i.asset.acquired_on)}</td>
                  <td className="text-right tabular-nums">{euros(i.asset.cost)}</td>
                  <td className="text-right">
                    {nuevo(n) ? (
                      <input
                        aria-label={`Coeficiente anual de ${i.asset.name}`}
                        className="w-16 text-right"
                        inputMode="decimal"
                        value={rates[n] ?? ''}
                        onChange={(e) =>
                          setRates(rates.map((r, m) => (m === n ? e.target.value : r)))
                        }
                      />
                    ) : (
                      '—'
                    )}
                  </td>
                  <td className="text-right tabular-nums">{euros(i.asset.accumulated)}</td>
                  <td>
                    {i.existing !== null
                      ? 'Ya está en Inmovilizado'
                      : i.skip
                        ? `No entra: ${i.skip}`
                        : 'Se da de alta'}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <label>
            Amortizado hasta el
            <input
              type="date"
              className="ml-2"
              value={until}
              onChange={(e) => setUntil(e.target.value)}
            />
          </label>
          {plan.warnings.length > 0 && (
            <ul className="list-disc space-y-1 pl-5 text-amber-800">
              {plan.warnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          )}
          {nuevos > 0 ? (
            <ConfirmAction
              label={`Dar de alta ${plural(nuevos, 'bien', 'bienes')}`}
              className="rounded btn-primary px-3 py-1"
              danger={false}
              question={`Se da${nuevos === 1 ? '' : 'n'} de alta ${plural(nuevos, 'bien', 'bienes')} en Inmovilizado con lo ya amortizado hasta el ${until ? formatDate(until) : '…'}. No se contabiliza nada: sus saldos ya están en el diario del gestor o en la apertura. ¿Seguir?`}
              confirmLabel="Sí, dar de alta"
              onConfirm={run}
            />
          ) : (
            <p>No hay bienes nuevos que dar de alta.</p>
          )}
        </div>
      )}
      {msg && <p role="status">{msg}</p>}
    </div>
  );
}

import { useCallback, useEffect, useState } from 'react';
import { errorMessage, euros, formatDate } from './api';
import { ConfirmAction } from './ConfirmAction';
import {
  type CierreValores,
  type TratamientoCierre,
  valoresClosing,
  valoresClosingPost,
  valoresClosingUndo,
} from './valores';

const cell = 'px-2 py-1 text-right tabular-nums';

const TRATAMIENTO: Record<TratamientoCierre, string> = {
  valor_razonable: 'A valor razonable',
  deterioro: 'Deterioro',
  sin_deterioro: 'Sin deterioro',
  excluido: 'No se deteriora',
  no_aplica: 'No se valora aquí',
  sin_precio: 'Falta el precio',
};

/**
 * Paso del cierre: valoración de la cartera de valores (Inversiones) según su clasificación: a
 * valor razonable con cambios en resultados (negociación) o deterioro (a coste), con un asiento
 * el último día del ejercicio que se puede deshacer.
 */
export function ValoracionValores({ year, onChanged }: { year: number; onChanged: () => void }) {
  const [p, setP] = useState<CierreValores | null>(null);
  const [excluidos, setExcluidos] = useState<number[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(
    (ex?: number[]) => {
      valoresClosing(year, ex ? { excluidos: ex } : undefined)
        .then((r) => {
          setP(r ?? null);
          setError(null);
        })
        .catch((e) => setError(errorMessage(e)));
    },
    [year],
  );
  useEffect(() => load(), [load]);
  if (!p?.valores || (p.valores.length === 0 && p.entry_id === null && p.reversion === 0)) {
    return error ? (
      <p role="alert" className="text-red-700">
        {error}
      </p>
    ) : null;
  }
  const posted = p.entry_id !== null;
  const ex = excluidos ?? p.input.excluidos;
  const toggle = (id: number) => {
    const next = ex.includes(id) ? ex.filter((x) => x !== id) : [...ex, id];
    setExcluidos(next);
    load(next);
  };
  const negociacion = p.cartera === 'negociacion';
  const resumen = negociacion
    ? `subidas ${euros(p.beneficios)}, bajadas ${euros(p.perdidas)}`
    : `deterioro ${euros(p.deterioro)}, reversión ${euros(p.reversion)}`;
  return (
    <section aria-label="Cartera de valores al cierre" className="card p-5">
      <h3 className="font-semibold">Cartera de valores al cierre de {year}</h3>
      <p className="mt-2 text-stone-700">
        {negociacion
          ? 'Cartera mantenida para negociar: cada valor se lleva a su cotización del cierre y la diferencia con su valor en libros va a resultados (7630 / 6630).'
          : 'Cartera a coste: sólo se registra deterioro cuando el valor del cierre queda por debajo del coste (698 / 596 a corto, 6963 / 296 a largo), revirtiendo antes el de cierres anteriores. Si una bajada es pasajera, marca «No es deterioro».'}{' '}
        Los precios salen de Inversiones → «Precios de cierre».
      </p>
      <table className="mt-3 w-full">
        <thead className="text-left text-stone-500">
          <tr>
            <th className="font-normal">Valor</th>
            <th className="px-2 text-right font-normal">En libros</th>
            <th className="px-2 text-right font-normal">Al cierre</th>
            <th className="px-2 text-right font-normal">Ajuste</th>
            <th className="px-2 font-normal">Tratamiento</th>
          </tr>
        </thead>
        <tbody>
          {p.valores.map((x) => (
            <tr key={x.security_id} className="border-t border-stone-100">
              <td className="pr-2">
                <span className="font-mono">{x.cuenta}</span> {x.ticker}{' '}
                <span className="text-stone-500">{x.name}</span>
              </td>
              <td className={cell}>{euros(x.libros)}</td>
              <td className={cell}>{x.valor === null ? '—' : euros(x.valor)}</td>
              <td className={cell}>{x.ajuste ? euros(x.ajuste) : ''}</td>
              <td className="px-2">
                {TRATAMIENTO[x.tratamiento]}
                {!negociacion &&
                  !posted &&
                  (x.tratamiento === 'deterioro' || x.tratamiento === 'excluido') && (
                    <label className="ml-2 text-stone-600">
                      <input
                        type="checkbox"
                        checked={ex.includes(x.security_id)}
                        onChange={() => toggle(x.security_id)}
                      />{' '}
                      No es deterioro
                    </label>
                  )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {p.lines.length > 0 && (
        <>
          <p className="mt-4 text-stone-700">
            Asiento del {formatDate(p.fecha)} ({resumen}):
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
                <tr key={`${l.account}-${l.debit}-${l.credit}`}>
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
      {p.notas.length > 0 && (
        <ul className="mt-3 list-disc space-y-1 pl-5 text-stone-700">
          {p.notas.map((n) => (
            <li key={n}>{n}</li>
          ))}
        </ul>
      )}
      {p.avisos.length > 0 && (
        <ul className="mt-3 space-y-1 rounded-md bg-amber-50 p-3 text-amber-950">
          {p.avisos.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
      {p.stale && (
        <p className="mt-3 text-amber-800">
          La cartera ha cambiado desde que se valoró: deshaz la valoración y vuelve a
          contabilizarla.
        </p>
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
              question={`¿Contabilizar la valoración de la cartera al ${formatDate(p.fecha)} (${resumen})?`}
              confirmLabel="Sí, contabilizar"
              danger={false}
              className="btn-primary rounded-md px-3 py-1.5"
              onConfirm={async () => {
                await valoresClosingPost(year, { excluidos: ex });
                setExcluidos(null);
                load();
                onChanged();
              }}
            />
          )
        ) : (
          <p className="flex flex-wrap items-center gap-3">
            <span className="pill pill-green">Contabilizada</span>
            {!p.closed && (
              <ConfirmAction
                label="Deshacer"
                ariaLabel="Deshacer la valoración de la cartera"
                question="¿Quitar el asiento de la valoración de la cartera al cierre?"
                confirmLabel="Sí, quitarlo"
                onConfirm={async () => {
                  await valoresClosingUndo(year);
                  load();
                  onChanged();
                }}
              />
            )}
          </p>
        )}
      </div>
    </section>
  );
}

import { useCallback, useEffect, useState } from 'react';
import {
  bankStatements,
  bankStatementUndo,
  type ExtractoGuardado,
  errorMessage,
  formatDate,
  type ListaExtractos,
  money,
  plural,
} from './api';
import { ConfirmAction } from './ConfirmAction';

/** «del 01/08/2026 al 31/08/2026» (o lo que se sepa). */
export function periodo(e: { desde: string | null; hasta: string | null }): string {
  if (e.desde && e.hasta) return `del ${formatDate(e.desde)} al ${formatDate(e.hasta)}`;
  if (e.desde) return `desde el ${formatDate(e.desde)}`;
  if (e.hasta) return `hasta el ${formatDate(e.hasta)}`;
  return 'sin fechas';
}

/** Qué pasa al deshacer un extracto. */
export function undoStatementQuestion(e: ExtractoGuardado): string {
  const base = `¿Deshacer la importación de ${e.fichero} (${periodo(e)})?`;
  if (e.propios === 0)
    return `${base} Todos sus movimientos los trae otro extracto: se quedan, y sólo sale de la lista.`;
  const borra = `Se ${e.propios === 1 ? 'borra 1 movimiento' : `borran ${e.propios} movimientos`} que sólo trae este extracto`;
  const comparte =
    e.movimientos > e.propios ? '; los que también trae otro extracto se quedan' : '';
  const concilia =
    e.conciliados > 0
      ? ` ${e.conciliados === 1 ? 'Uno está conciliado' : `${e.conciliados} están conciliados`}: antes se deshace su conciliación (y se borra el asiento que creó), como en «Movimientos conciliados».`
      : '';
  return `${base} ${borra}${comparte}.${concilia} Se puede volver a importar el fichero.`;
}

/**
 * Los extractos importados de cada cuenta (fichero, periodo, movimientos y saldos), los huecos
 * entre ellos y deshacer uno importado por error.
 */
export function ExtractosImportados({ version, onDone }: { version: number; onDone: () => void }) {
  const [open, setOpen] = useState(false);
  const [data, setData] = useState<ListaExtractos | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(() => {
    bankStatements()
      .then(setData)
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: recarga cuando cambian los movimientos
  useEffect(() => {
    if (open) load();
  }, [open, load, version]);

  const cuentas = [...new Set((data?.extractos ?? []).map((e) => e.cuenta_id))];
  return (
    <section aria-label="Extractos importados" className="mt-4 text-sm">
      <button type="button" className="text-link" onClick={() => setOpen(!open)}>
        {open ? 'Ocultar los extractos importados' : 'Ver los extractos importados…'}
      </button>
      {open && (
        <div className="mt-2 space-y-3">
          {msg && <p role="status">{msg}</p>}
          {data && data.extractos.length === 0 && <p>Todavía no hay extractos importados.</p>}
          {cuentas.map((c) => {
            const lista = data?.extractos.filter((e) => e.cuenta_id === c) ?? [];
            const huecos = data?.huecos.filter((h) => h.cuenta_id === c) ?? [];
            const nombre = lista[0]?.cuenta ?? '';
            return (
              <div key={c}>
                <table className="w-full text-sm">
                  <caption className="text-left font-semibold">{nombre}</caption>
                  <thead>
                    <tr className="text-left text-stone-600">
                      <th className="pr-3">Fichero</th>
                      <th className="pr-3">Periodo</th>
                      <th className="pr-3 text-right">Movimientos</th>
                      <th className="pr-3 text-right">Saldo inicial</th>
                      <th className="pr-3 text-right">Saldo final</th>
                      <th>
                        <span className="sr-only">Acciones</span>
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {lista.map((e) => (
                      <tr key={e.id} className="border-t border-stone-200 align-top">
                        <td className="pr-3">
                          {e.fichero}
                          {e.formato && e.formato !== e.fichero && (
                            <span className="text-stone-600"> · {e.formato}</span>
                          )}
                          {e.heredado && (
                            <span className="block text-xs text-stone-600">
                              Importado antes de esta versión: periodo y saldos según sus
                              movimientos.
                            </span>
                          )}
                        </td>
                        <td className="pr-3">{periodo(e)}</td>
                        <td className="pr-3 text-right">
                          {e.movimientos}
                          {e.nuevos !== e.movimientos && (
                            <span className="block text-xs text-stone-600">
                              {plural(e.nuevos, 'nuevo', 'nuevos')}
                            </span>
                          )}
                          {e.conciliados > 0 && (
                            <span className="block text-xs text-stone-600">
                              {plural(e.conciliados, 'conciliado', 'conciliados')}
                            </span>
                          )}
                        </td>
                        <td className="pr-3 text-right">
                          {e.saldo_inicial == null ? '—' : money(e.saldo_inicial, e.divisa)}
                        </td>
                        <td className="pr-3 text-right">
                          {e.saldo_final == null ? '—' : money(e.saldo_final, e.divisa)}
                        </td>
                        <td>
                          <ConfirmAction
                            label="Deshacer"
                            ariaLabel={`Deshacer la importación de ${e.fichero}`}
                            className="text-red-700 underline"
                            question={undoStatementQuestion(e)}
                            confirmLabel={
                              e.conciliados > 0 ? 'Sí, desconciliar y deshacer' : 'Sí, deshacer'
                            }
                            onConfirm={async () => {
                              const r = await bankStatementUndo(e.id, e.conciliados > 0);
                              setMsg(
                                [
                                  `${e.fichero}: ${plural(r.borrados, 'movimiento borrado', 'movimientos borrados')}`,
                                  r.desconciliados > 0
                                    ? `${plural(r.desconciliados, 'desconciliado', 'desconciliados')} antes`
                                    : '',
                                  r.compartidos > 0
                                    ? `${plural(r.compartidos, 'se queda', 'se quedan')} porque ${r.compartidos === 1 ? 'lo trae' : 'los trae'} otro extracto`
                                    : '',
                                ]
                                  .filter(Boolean)
                                  .join('; ')
                                  .concat('.'),
                              );
                              load();
                              onDone();
                            }}
                          />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {huecos.length > 0 && (
                  <ul className="mt-1 list-disc pl-5">
                    {huecos.map((h) => (
                      <li
                        key={`${h.desde}-${h.hasta}`}
                        className={h.seguro ? 'text-amber-800' : 'text-stone-700'}
                      >
                        {h.seguro ? 'Faltan movimientos' : 'Posible hueco'} {periodo(h)}:{' '}
                        {h.detalle}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

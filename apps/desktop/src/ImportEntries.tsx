import { useCallback, useEffect, useState } from 'react';
import {
  errorMessage,
  euros,
  formatDate,
  type ImportBatch,
  type ImportPreview,
  importBatches,
  importEntries,
  importPreview,
  importUndo,
  pickImportFile,
  plural,
} from './api';
import { ConfirmAction } from './ConfirmAction';

/** Importa asientos de a3 (SUENLACE.DAT) o de una tabla en Excel/CSV, con vista previa y lote. */
export function ImportEntries() {
  const [path, setPath] = useState<string | null>(null);
  const [p, setP] = useState<ImportPreview | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [list, setList] = useState<ImportBatch[]>([]);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    importBatches()
      .then(setList)
      .catch(() => setList([]));
  }, []);
  useEffect(load, [load]);

  async function choose() {
    setMsg(null);
    const f = await pickImportFile();
    if (!f) return;
    setPath(f);
    try {
      setP(await importPreview(f));
    } catch (e) {
      setP(null);
      setMsg(errorMessage(e));
    }
  }

  async function run() {
    if (!path) return;
    setBusy(true);
    try {
      const r = await importEntries(path);
      setMsg(`${r.entries} asientos importados.`);
      setP(null);
      setPath(null);
      load();
    } catch (e) {
      setMsg(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section aria-label="Importar asientos" className="text-sm">
      <p className="font-medium">Importar asientos (a3, Excel o CSV)</p>
      <p className="text-stone-600">
        SUENLACE.DAT de a3 (formatos 4 y 5) o una tabla con columnas Fecha, Nº asiento, Concepto,
        Cuenta, Debe y Haber. Antes de importar verás un resumen; cada importación se puede deshacer
        entera.
      </p>
      <button
        type="button"
        className="mt-3 rounded border border-stone-300 px-3 py-1"
        onClick={choose}
      >
        Elegir fichero…
      </button>
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}
      {p && (
        <section
          className="mt-2 rounded border border-stone-300 bg-stone-50 p-3"
          aria-label="Vista previa de la importación"
        >
          <p>
            {p.source === 'suenlace' ? 'SUENLACE de a3' : 'Tabla de asientos'}:{' '}
            {plural(p.entries, 'asiento')}
            {p.invoices > 0 ? ` (${plural(p.invoices, 'factura')})` : ''},{' '}
            {plural(p.accounts, 'cuenta con nombre', 'cuentas con nombre')},{' '}
            {p.first_date && p.last_date
              ? `del ${formatDate(p.first_date)} al ${formatDate(p.last_date)}`
              : ''}
            , por {euros(p.total)}.
          </p>
          {p.already && <p className="text-red-700">Este fichero ya se importó: {p.already}.</p>}
          {p.warnings.length > 0 && (
            <ul className="mt-1 list-disc pl-5 text-amber-900">
              {p.warnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          )}
          <table className="mt-2 w-full text-xs">
            <tbody>
              {p.sample.map((e, i) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: muestra fija
                <tr key={i} className="border-t border-stone-200 align-top">
                  <td className="pr-2">{formatDate(e.date)}</td>
                  <td className="pr-2">{e.concept}</td>
                  <td className="font-mono">
                    {e.lines.map((l) => `${l.account} ${euros(l.debit || -l.credit)}`).join(' · ')}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="form-actions mt-3">
            <button
              type="button"
              className="rounded btn-primary px-3 py-1 disabled:opacity-50"
              disabled={busy || !!p.already}
              onClick={run}
            >
              Importar {p.entries} asientos
            </button>
            <button
              type="button"
              className="rounded border border-stone-300 px-3 py-1"
              onClick={() => setP(null)}
            >
              Cancelar
            </button>
          </div>
        </section>
      )}
      {list.length > 0 && (
        <ul className="mt-3">
          {list.map((b) => (
            <li
              key={b.id}
              className="flex flex-wrap items-center gap-3 border-t border-stone-200 py-1"
            >
              <span>
                {b.filename} · {b.entries} asientos · {formatDate(b.created_at.slice(0, 10))}
              </span>
              <ConfirmAction
                label="Deshacer"
                question={`¿Borrar los ${b.entries} asientos importados de ${b.filename}?`}
                confirmLabel="Sí, deshacer"
                onConfirm={() =>
                  importUndo(b.id, true)
                    .then((n) => {
                      setMsg(`${n} asientos borrados.`);
                      load();
                    })
                    .catch((e) => setMsg(errorMessage(e)))
                }
              />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

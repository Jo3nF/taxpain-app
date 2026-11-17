import { useCallback, useEffect, useId, useState } from 'react';
import {
  errorMessage,
  euros,
  formatDate,
  type GestorBalanceComparison,
  type GestorLot,
  type GestorModelsComparison,
  type GestorPreview,
  gestorCompareBalance,
  gestorCompareModels,
  gestorImport,
  gestorLots,
  gestorPreview,
  gestorUndo,
  pickOpeningFile,
  plural,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const cell = 'px-2 py-1 text-right tabular-nums';

function Notes({ items, alert = false }: { items: string[]; alert?: boolean }) {
  if (!items.length) return null;
  return (
    <ul
      role={alert ? 'alert' : undefined}
      className={`mt-2 list-disc space-y-1 pl-5 ${alert ? 'text-red-700' : 'text-amber-800'}`}
    >
      {items.map((w) => (
        <li key={w}>{w}</li>
      ))}
    </ul>
  );
}

const SOURCES: Record<GestorLot['source'], string> = {
  diario: 'diario completo',
  resumido: 'diario resumido (un asiento por mes)',
  suenlace: 'SUENLACE de a3',
};

/**
 * Importar un ejercicio entero del gestor: la empresa que llevaba la contabilidad en una gestoría
 * carga un ejercicio ya cerrado, asiento a asiento, para tener el histórico, cotejarlo con el
 * balance y los modelos del gestor y cerrarlo con TaxPain (que abre el siguiente).
 */
export function EjercicioGestor({
  year,
  onClose,
}: {
  year: number;
  onClose?: (() => void) | undefined;
}) {
  const [path, setPath] = useState<string | null>(null);
  const [p, setP] = useState<GestorPreview | null>(null);
  const [nextOpening, setNextOpening] = useState(true);
  const [lots, setLots] = useState<GestorLot[]>([]);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showAccounts, setShowAccounts] = useState(false);
  const nextId = useId();

  const load = useCallback(() => {
    gestorLots()
      .then(setLots)
      .catch(() => setLots([]));
  }, []);
  useEffect(load, [load]);

  async function choose() {
    setMsg(null);
    const f = await pickOpeningFile();
    if (!f) return;
    setPath(f);
    setP(null);
    try {
      const preview = await gestorPreview(f);
      // Si el ejercicio siguiente tiene su propio diario con su apertura, la del cierre no se
      // marca: con las dos, los saldos iniciales irían dos veces.
      setNextOpening(!preview.next_ledger);
      setP(preview);
    } catch (e) {
      setMsg(errorMessage(e));
    }
  }

  async function run() {
    if (!path || !p || p.fiscal_year === null) return;
    setBusy(true);
    try {
      const lot = await gestorImport(path, p.fiscal_year, { next_opening: nextOpening });
      setMsg(
        `Ejercicio ${lot.fiscal_year} importado: ${plural(lot.entries, 'asiento')} del gestor` +
          `${lot.has_opening ? ', su apertura' : ''}${lot.has_regularization ? ', su regularización' : ''}` +
          `${lot.has_closing ? ' y su cierre' : ''}; ${plural(lot.accounts_created, 'cuenta nueva', 'cuentas nuevas')}.` +
          (lot.closed_year
            ? ` El ejercicio queda cerrado${lot.next_opening ? ` y ${lot.fiscal_year + 1} arranca con su apertura` : ''}.`
            : ' El ejercicio queda abierto: ciérralo con TaxPain cuando lo hayas cotejado.'),
      );
      setP(null);
      setPath(null);
      load();
    } catch (e) {
      setMsg(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const ready = p !== null && p.errors.length === 0 && p.fiscal_year !== null;

  return (
    <section aria-labelledby="gestor-title" className="mt-4 space-y-4 text-sm">
      <div>
        <h3 id="gestor-title" className="text-base font-semibold">
          Importar un ejercicio entero del gestor
        </h3>
        <p className="mt-1 max-w-3xl text-stone-700">
          Si tu gestoría llevaba la contabilidad (a3, Sage, ContaSOL…), pídele el diario de un
          ejercicio ya cerrado en Excel o CSV y cárgalo aquí asiento a asiento: tendrás el histórico
          en TaxPain, podrás comparar ejercicios y cotejar los modelos que presentó. Luego cierra
          ese ejercicio con TaxPain (Contabilidad → Cierre): el siguiente arranca con la apertura
          que sale de ese cierre, y el asistente de saldos iniciales sólo hace falta para lo que un
          diario no trae: los cobros y pagos pendientes y el inmovilizado con lo ya amortizado.
        </p>
      </div>

      {lots.length > 0 && (
        <LotList
          lots={lots}
          year={year}
          onClose={onClose}
          onUndone={(n) => {
            setMsg(
              `Lote deshecho: ${plural(n, 'asiento borrado', 'asientos borrados')}; el ejercicio queda vacío.`,
            );
            load();
          }}
        />
      )}

      <div className="card space-y-3 p-5">
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            className="rounded btn-primary px-3 py-1"
            onClick={choose}
            disabled={busy}
          >
            Elegir el diario del gestor…
          </button>
          <span className="text-stone-600">
            Diario completo (una hoja por mes o todo junto), diario resumido, SUENLACE o CSV con
            columnas Fecha, Asiento, Cuenta, Debe y Haber.
          </span>
        </div>

        {p && (
          <div className="space-y-2">
            <h4 className="font-semibold">Vista previa de «{p.filename}»</h4>
            <dl className="grid max-w-xl grid-cols-[auto_1fr] gap-x-4 gap-y-1">
              <dt className="text-stone-600">Formato</dt>
              <dd>{SOURCES[p.source]}</dd>
              <dt className="text-stone-600">Ejercicio</dt>
              <dd>
                {p.fiscal_year_label ?? '—'}
                {p.first_date && p.last_date && (
                  <span className="text-stone-600">
                    {' '}
                    (del {formatDate(p.first_date)} al {formatDate(p.last_date)})
                  </span>
                )}
              </dd>
              <dt className="text-stone-600">Asientos</dt>
              <dd>
                {plural(p.entries, 'asiento')} con {plural(p.lines, 'apunte')}; todos cuadran (debe
                = haber = {euros(p.total)})
              </dd>
              <dt className="text-stone-600">Resultado</dt>
              <dd>
                {p.result < 0 ? 'Pérdida de ' : 'Beneficio de '}
                {euros(Math.abs(p.result))} según el diario
              </dd>
              <dt className="text-stone-600">Cuentas</dt>
              <dd>
                {plural(p.accounts, 'cuenta')} en el diario;{' '}
                {p.new_accounts.length === 0 ? (
                  'ninguna nueva'
                ) : (
                  <button
                    type="button"
                    className="underline"
                    aria-expanded={showAccounts}
                    onClick={() => setShowAccounts((v) => !v)}
                  >
                    {plural(p.new_accounts.length, 'nueva', 'nuevas')} que se darán de alta con el
                    nombre del gestor
                  </button>
                )}
                {showAccounts && (
                  <ul className="mt-1 max-h-48 overflow-auto text-stone-700">
                    {p.new_accounts.map((a) => (
                      <li key={a.code}>
                        <span className="tabular-nums">{a.code}</span> {a.name}
                      </li>
                    ))}
                  </ul>
                )}
              </dd>
              <dt className="text-stone-600">Trae</dt>
              <dd>
                {[
                  p.has_opening ? 'la apertura' : null,
                  p.has_regularization ? 'la regularización' : null,
                  p.has_closing ? 'el cierre' : null,
                ]
                  .filter(Boolean)
                  .join(', ') || 'ni apertura, ni regularización, ni cierre'}
              </dd>
            </dl>
            {p.has_closing && (
              <label className="flex items-center gap-2" htmlFor={nextId}>
                <input
                  id={nextId}
                  type="checkbox"
                  checked={nextOpening && p.next_opening}
                  disabled={!p.next_opening}
                  onChange={(e) => setNextOpening(e.target.checked)}
                />
                Crear la apertura de {(p.fiscal_year ?? year) + 1} con los saldos del cierre
                {!p.next_opening && ' (ya la tiene)'}
                {p.next_opening &&
                  p.next_ledger &&
                  ` (no se marca: ${(p.fiscal_year ?? year) + 1} tiene su propio diario con su apertura)`}
              </label>
            )}
            <Notes items={p.errors} alert />
            <Notes items={p.warnings} />
            {ready && !busy ? (
              <ConfirmAction
                label={`Importar el ejercicio ${p.fiscal_year_label ?? ''}`}
                className="rounded btn-primary px-3 py-1"
                danger={false}
                question={`Se crean ${plural(p.entries, 'asiento')} y ${plural(p.new_accounts.length, 'cuenta nueva', 'cuentas nuevas')} en el ejercicio ${p.fiscal_year_label ?? ''}. Se puede deshacer entero mientras no cierres el ejercicio con TaxPain. ¿Importar?`}
                confirmLabel="Sí, importar"
                onConfirm={run}
              />
            ) : (
              <button type="button" className="rounded btn-primary px-3 py-1 opacity-50" disabled>
                {busy ? 'Importando…' : 'No se puede importar: mira los avisos en rojo'}
              </button>
            )}
          </div>
        )}
      </div>

      {msg && (
        <p role="status" className="text-stone-800">
          {msg}
        </p>
      )}
    </section>
  );
}

function LotList({
  lots,
  year,
  onClose,
  onUndone,
}: {
  lots: GestorLot[];
  year: number;
  onClose?: (() => void) | undefined;
  onUndone: (n: number) => void;
}) {
  return (
    <div className="space-y-3 rounded-md bg-stone-50 p-3">
      <p className="font-medium">Ejercicios del gestor ya importados</p>
      <ul className="space-y-3">
        {lots.map((l) => (
          <LotItem key={l.id} lot={l} current={year} onClose={onClose} onUndone={onUndone} />
        ))}
      </ul>
    </div>
  );
}

function LotItem({
  lot,
  current,
  onClose,
  onUndone,
}: {
  lot: GestorLot;
  current: number;
  onClose?: (() => void) | undefined;
  onUndone: (n: number) => void;
}) {
  const [balance, setBalance] = useState<GestorBalanceComparison | null>(null);
  const [models, setModels] = useState<GestorModelsComparison | null>(null);
  const [all, setAll] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function compareBalance() {
    setErr(null);
    const f = await pickOpeningFile();
    if (!f) return;
    try {
      setBalance(await gestorCompareBalance(lot.fiscal_year, f));
    } catch (e) {
      setErr(errorMessage(e));
    }
  }
  async function compareModels() {
    setErr(null);
    try {
      setModels(await gestorCompareModels(lot.fiscal_year));
    } catch (e) {
      setErr(errorMessage(e));
    }
  }

  const brought = [
    lot.has_opening ? 'apertura' : null,
    lot.has_regularization ? 'regularización' : null,
    lot.has_closing ? 'cierre' : null,
  ].filter(Boolean);
  const balanceRows = balance ? balance.rows.filter((r) => all || Math.abs(r.diff) > 1) : [];

  return (
    <li className="space-y-2">
      <div className="flex flex-wrap items-center gap-3">
        <span>
          Ejercicio {lot.fiscal_year}: «{lot.filename}» ({SOURCES[lot.source]}),{' '}
          {plural(lot.entries, 'asiento')},{' '}
          {plural(lot.accounts_created, 'cuenta nueva', 'cuentas nuevas')}
          {brought.length > 0 && `, con ${brought.join(', ')}`}.{' '}
          {lot.closed_year
            ? `Cerrado con el cierre del gestor${lot.next_opening ? ` (abre ${lot.fiscal_year + 1})` : ''}.`
            : 'Abierto: ciérralo con TaxPain cuando lo hayas cotejado.'}
        </span>
        <button type="button" className="text-stone-700 underline" onClick={compareBalance}>
          Cotejar con el balance de sumas y saldos…
        </button>
        <button type="button" className="text-stone-700 underline" onClick={compareModels}>
          Cotejar con los modelos presentados
        </button>
        {!lot.closed_year && onClose && (
          <button
            type="button"
            className="rounded border border-stone-300 px-2 py-0.5"
            onClick={onClose}
          >
            Cerrar {lot.fiscal_year} con TaxPain
          </button>
        )}
        <ConfirmAction
          label="Deshacer"
          className="text-stone-700 underline"
          question={`Se borran los ${plural(lot.entries, 'asiento')} del gestor y los de apertura, regularización y cierre que trajo${lot.next_opening ? `, y la apertura de ${lot.fiscal_year + 1}` : ''}: el ejercicio ${lot.fiscal_year} queda vacío. No se puede si lo cerraste con TaxPain (reábrelo antes). ¿Seguir?`}
          confirmLabel="Sí, deshacer"
          onConfirm={async () => onUndone(await gestorUndo(lot.id, true))}
        />
      </div>
      {lot.fiscal_year !== current && (
        <p className="text-stone-600">
          Estás en {current}: cambia el ejercicio arriba para ver su diario y sus libros.
        </p>
      )}
      {err && (
        <p role="alert" className="text-red-700">
          {err}
        </p>
      )}
      {balance && (
        <div className="rounded border border-stone-200 bg-white p-3">
          <p className="font-medium">
            Balance del gestor frente a TaxPain ({balance.fiscal_year},{' '}
            {balance.basis === 'tras_regularizacion'
              ? 'saldos tras la regularización'
              : 'saldos antes de la regularización'}
            ):{' '}
            {balance.mismatches === 0
              ? 'todas las cuentas coinciden.'
              : `${plural(balance.mismatches, 'cuenta')} no coinciden.`}
          </p>
          <Notes items={balance.warnings} />
          {balance.rows.length > 0 && (
            <>
              <label className="mt-2 flex items-center gap-2">
                <input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} />
                Ver también las que coinciden
              </label>
              {balanceRows.length > 0 && (
                <table className="mt-2 w-full">
                  <caption className="sr-only">Saldos por cuenta</caption>
                  <thead>
                    <tr className="text-left text-stone-600">
                      <th scope="col">Cuenta</th>
                      <th scope="col" className={cell}>
                        Gestor
                      </th>
                      <th scope="col" className={cell}>
                        TaxPain
                      </th>
                      <th scope="col" className={cell}>
                        Diferencia
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {balanceRows.map((r) => (
                      <tr key={r.account} className={Math.abs(r.diff) > 1 ? 'text-red-700' : ''}>
                        <td>
                          <span className="tabular-nums">{r.account}</span> {r.name}
                        </td>
                        <td className={cell}>{r.theirs === null ? 'no está' : euros(r.theirs)}</td>
                        <td className={cell}>{euros(r.ours)}</td>
                        <td className={cell}>{euros(r.diff)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </>
          )}
        </div>
      )}
      {models && (
        <div className="rounded border border-stone-200 bg-white p-3">
          <p className="font-medium">
            Modelos presentados frente al diario ({models.fiscal_year}):{' '}
            {models.rows.length === 0
              ? 'no hay modelos guardados ni IVA en el diario.'
              : models.mismatches === 0
                ? 'todo lo que hay guardado coincide.'
                : `${plural(models.mismatches, 'cifra')} no coinciden.`}
          </p>
          <Notes items={models.warnings} />
          {models.rows.length > 0 && (
            <table className="mt-2 w-full">
              <caption className="sr-only">Casillas de los modelos</caption>
              <thead>
                <tr className="text-left text-stone-600">
                  <th scope="col">Modelo</th>
                  <th scope="col">Casilla</th>
                  <th scope="col" className={cell}>
                    Diario
                  </th>
                  <th scope="col" className={cell}>
                    Presentado
                  </th>
                  <th scope="col" className={cell}>
                    Diferencia
                  </th>
                </tr>
              </thead>
              <tbody>
                {models.rows.map((r) => (
                  <tr
                    key={`${r.model}-${r.period}-${r.box}`}
                    className={r.theirs !== null && Math.abs(r.diff) > 1 ? 'text-red-700' : ''}
                  >
                    <td>
                      {r.model} {r.period}
                    </td>
                    <td>
                      [{r.box}] {r.label}
                    </td>
                    <td className={cell}>{euros(r.ours)}</td>
                    <td className={cell}>{r.theirs === null ? 'sin modelo' : euros(r.theirs)}</td>
                    <td className={cell}>{r.theirs === null ? '—' : euros(r.diff)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}
    </li>
  );
}

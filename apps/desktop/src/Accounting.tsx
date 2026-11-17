import { Download } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { Actas } from './Actas';
import { AjustesCierre, type IrA } from './AjustesCierre';
import { Analitica } from './Analitica';
import { Apertura } from './Apertura';
import { useAccounts } from './accounts';
import {
  type AccountBalance,
  type AssetPrior,
  type AssetSale,
  addFixedAsset,
  addFixedAssetWithPrior,
  assetDispose,
  assetIncentives,
  assetSaleCandidates,
  assetSaleLink,
  assetSales,
  assetSaleUnlink,
  closeYear,
  type DepreciationMethod,
  deleteEntries,
  deleteEntry,
  errorMessage,
  euros,
  exportListings,
  type FiscalYearInfo,
  type FixedAsset,
  fiscalYearInfo,
  fixedAssetPriors,
  fixedAssets,
  formatDate,
  type IncentivoBien,
  journal,
  type LedgerLine,
  ledger,
  originLabel,
  parseEuros,
  pickFolder,
  plural,
  reopenYear,
  type SaleCandidate,
  type StoredEntry,
  setDepreciationPlan,
  setTick,
  type Template,
  templatesDue,
  transferBalance,
  trialBalance,
} from './api';
import { Cartera } from './Cartera';
import { ChartOfAccounts } from './ChartOfAccounts';
import { GuidedClosing } from './Closing';
import { ConfirmAction } from './ConfirmAction';
import { YearStepper } from './Controls';
import { CopyEntries } from './CopyEntries';
import { Criptomonedas } from './Criptomonedas';
import { Deposito } from './Deposito';
import { EntidadPatrimonial } from './EntidadPatrimonial';
import { EntryExtras } from './EntryExtras';
import { Existencias } from './Existencias';
import {
  AmortizacionesEjercicio,
  DeshacerBaja,
  EditarBien,
  IncentivoFiscal,
  incentiveLabel,
} from './InmovilizadoAcciones';
import { InventarioGestor } from './InventarioGestor';
import { IvaHolding } from './IvaHolding';
import { JournalEntryForm, localToday } from './JournalEntryForm';
import { LeasingView } from './LeasingView';
import { LibrosSocietarios } from './LibrosSocietarios';
import { Oss } from './Oss';
import { Payroll } from './Payroll';
import { Prestamos } from './Prestamos';
import { Ratios } from './Ratios';
import { AnnualAccountsView, Bank, TaxActions } from './Statements';
import { Subvenciones } from './Subvenciones';
import { TypedInvoice } from './TypedInvoice';
import { Vinculadas } from './Vinculadas';

export type View =
  | 'diario'
  | 'plan'
  | 'cartera'
  | 'sumas'
  | 'mayor'
  | 'banco'
  | 'inmovilizado'
  | 'prestamos'
  | 'subvenciones'
  | 'cuentas'
  | 'analitica'
  | 'nominas'
  | 'impuestos'
  | 'cierre'
  | 'societarios'
  | 'apertura'
  | 'cripto'
  | 'vinculadas';

const VIEWS: { id: View; label: string }[] = [
  { id: 'diario', label: 'Libro diario' },
  { id: 'plan', label: 'Plan de cuentas' },
  { id: 'cartera', label: 'Cobros y pagos' },
  { id: 'sumas', label: 'Sumas y saldos' },
  { id: 'mayor', label: 'Libro mayor' },
  { id: 'banco', label: 'Banco' },
  { id: 'inmovilizado', label: 'Inmovilizado' },
  { id: 'prestamos', label: 'Préstamos' },
  { id: 'subvenciones', label: 'Subvenciones' },
  { id: 'cuentas', label: 'Cuentas anuales' },
  { id: 'analitica', label: 'Analítica y presupuesto' },
  { id: 'nominas', label: 'Nóminas' },
  { id: 'impuestos', label: 'IVA y libros' },
  { id: 'cierre', label: 'Cierre' },
  { id: 'societarios', label: 'Libros societarios' },
  { id: 'apertura', label: 'Saldos iniciales' },
  { id: 'cripto', label: 'Criptomonedas' },
  { id: 'vinculadas', label: 'Vinculadas' },
];

const cell = 'px-2 py-1 text-right tabular-nums';

/** Un error al cargar no es «no hay datos»: se dice. */
function LoadError({ message }: { message: string }) {
  return (
    <p role="alert" className="mt-4 text-sm text-red-700">
      No se pudo cargar: {message}
    </p>
  );
}

/** El foco está en un campo de texto (los atajos de teclado no deben saltar ahí). */
export function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])') !==
      null
  );
}

/** Conceptos más usados en los asientos manuales (para proponerlos al escribir). */
export function frequentConcepts(entries: StoredEntry[], max = 30): string[] {
  const n = new Map<string, number>();
  for (const s of entries) {
    if (s.entry.origin !== 'Manual') continue;
    const c = s.entry.concept.trim();
    if (c) n.set(c, (n.get(c) ?? 0) + 1);
  }
  return [...n.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, max)
    .map(([c]) => c);
}

function Journal({
  year,
  focus,
  onClearFocus,
}: {
  year: number;
  /** Número de asiento a enseñar solo (desde el mayor). */
  focus?: number | null;
  onClearFocus?: () => void;
}) {
  const [entries, setEntries] = useState<StoredEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [typing, setTyping] = useState(false);
  const [version, setVersion] = useState(0);
  const [due, setDue] = useState<Template[]>([]);
  const [start, setStart] = useState<Template | undefined>(undefined);
  const [editing, setEditing] = useState<StoredEntry | undefined>(undefined);
  const [query, setQuery] = useState('');
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const accounts = useAccounts(year);
  const reloadAccounts = accounts.reload;
  // biome-ignore lint/correctness/useExhaustiveDependencies: `version` recarga tras grabar un asiento
  useEffect(() => {
    setError(null);
    journal(year)
      .then(setEntries)
      .catch((e) => {
        setEntries([]);
        setError(errorMessage(e));
      });
    reloadAccounts();
    templatesDue(localToday())
      .then(setDue)
      .catch(() => setDue([]));
  }, [year, version, reloadAccounts]);
  // Atajos del diario: ⌥N (Alt+N) abre un asiento nuevo; Esc lo cierra (en el formulario).
  // Escribiendo no: en el Mac ⌥N es la tecla muerta de la «ñ».
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (!e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
      if (e.code !== 'KeyN' && e.key.toLowerCase() !== 'n') return;
      if (isTyping(e.target)) return;
      e.preventDefault();
      setEditing(undefined);
      setStart(undefined);
      setAdding(true);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
  if (error) return <LoadError message={error} />;
  return (
    <>
      {due.length > 0 && (
        <section
          aria-label="Asientos periódicos pendientes"
          className="mt-4 rounded-md bg-amber-50 p-3 text-sm text-amber-900"
        >
          <p className="font-medium">Asientos periódicos pendientes</p>
          <ul>
            {due.map((t) => (
              <li key={t.id} className="flex justify-between py-0.5">
                <span>
                  {t.name} · {t.next_date && formatDate(t.next_date)}
                </span>
                <button
                  type="button"
                  className="text-link"
                  onClick={() => {
                    setStart(t);
                    setAdding(true);
                  }}
                >
                  Revisar y registrar
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      <button
        type="button"
        className="mt-4 rounded-md border border-stone-300 px-3 py-1 text-sm"
        aria-expanded={adding}
        aria-keyshortcuts="Alt+N"
        onClick={() => {
          setAdding(!adding);
          setStart(undefined);
          setEditing(undefined);
        }}
      >
        {adding ? 'Cerrar el asiento' : 'Nuevo asiento'}
      </button>
      <span aria-hidden="true" className="ml-2 text-xs text-stone-500">
        {adding ? 'Esc' : '⌥N'}
      </span>
      <button
        type="button"
        className="ml-3 mt-4 rounded-md border border-stone-300 px-3 py-1 text-sm"
        aria-expanded={typing}
        onClick={() => setTyping(!typing)}
      >
        {typing ? 'Cerrar la factura' : 'Factura recibida sin documento'}
      </button>
      {typing && (
        <TypedInvoice onClose={() => setTyping(false)} onSaved={() => setVersion((v) => v + 1)} />
      )}
      {adding && (
        <JournalEntryForm
          key={editing ? `e${editing.id}` : (start?.id ?? 'nuevo')}
          year={year}
          concepts={frequentConcepts(entries)}
          onClose={() => {
            setAdding(false);
            setEditing(undefined);
            setStart(undefined);
          }}
          start={start}
          editing={editing}
          onSaved={() => {
            setStart(undefined);
            if (editing) {
              setEditing(undefined);
              setAdding(false);
            }
            setVersion((v) => v + 1);
          }}
        />
      )}
      <div className="mt-4 flex flex-wrap items-end gap-3 text-sm">
        <label>
          Buscar
          <input
            aria-label="Buscar en el diario"
            className="mt-1 block w-64 rounded border px-1"
            placeholder="concepto, cuenta o importe"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <label>
          Desde
          <input
            type="date"
            className="mt-1 block rounded border px-1"
            value={from}
            onChange={(e) => setFrom(e.target.value)}
          />
        </label>
        <label>
          Hasta
          <input
            type="date"
            className="mt-1 block rounded border px-1"
            value={to}
            onChange={(e) => setTo(e.target.value)}
          />
        </label>
      </div>
      {focus ? (
        <p className="mt-3 text-sm">
          Asiento nº {focus}.{' '}
          <button type="button" className="text-link underline" onClick={onClearFocus}>
            Ver todo el diario
          </button>
        </p>
      ) : null}
      {!focus && (query || from || to) && (
        <CopyEntries
          year={year}
          ids={filterEntries(entries, query, from, to, accounts.name)
            .filter(
              (s) =>
                !['Apertura', 'Regularizacion', 'Cierre'].includes(
                  typeof s.entry.origin === 'string' ? s.entry.origin : '',
                ),
            )
            .map((s) => s.id)}
        />
      )}
      {!focus &&
        (query || from || to) &&
        (() => {
          const shown = filterEntries(entries, query, from, to, accounts.name).filter(
            (s) => s.entry.origin === 'Manual' || 'Importado' in Object(s.entry.origin),
          );
          return shown.length > 1 ? (
            <div className="mt-2 text-sm">
              <ConfirmAction
                label={`Borrar los ${shown.length} asientos manuales o importados que salen`}
                question={`¿Borrar ${shown.length} asientos (${shown
                  .slice(0, 5)
                  .map((s) => s.number)
                  .join(', ')}${shown.length > 5 ? '…' : ''})? Quedan en la auditoría.`}
                confirmLabel={`Sí, borrar ${shown.length}`}
                onConfirm={async () => {
                  // Si falla, el error sale en el propio panel de confirmación.
                  await deleteEntries(shown.map((s) => s.id));
                  setVersion((v) => v + 1);
                }}
              />
            </div>
          ) : null;
        })()}
      {entries.length === 0 ? (
        <p className="mt-4 text-sm text-stone-500">Sin asientos en {year}.</p>
      ) : (
        <JournalTable
          entries={
            focus
              ? entries.filter((s) => s.number === focus)
              : filterEntries(entries, query, from, to, accounts.name)
          }
          name={accounts.name}
          onEdit={(s) => {
            setEditing(s);
            setStart(undefined);
            setAdding(true);
          }}
          onDelete={async (s) => {
            // Si falla, el error sale en el panel de confirmación de la fila.
            await deleteEntry(s.id, true);
            setVersion((v) => v + 1);
          }}
          onChange={() => setVersion((v) => v + 1)}
        />
      )}
    </>
  );
}

/** Asientos que cumplen la búsqueda (concepto, cuenta, nombre de cuenta o importe) y las fechas. */
export function filterEntries(
  entries: StoredEntry[],
  query: string,
  from: string,
  to: string,
  name: (code: string) => string | undefined,
): StoredEntry[] {
  const q = query.trim().toLowerCase();
  const amount = q ? parseEuros(q) : null;
  return entries.filter((s) => {
    if (from && s.entry.date < from) return false;
    if (to && s.entry.date > to) return false;
    if (!q) return true;
    if (s.entry.concept.toLowerCase().includes(q)) return true;
    return s.entry.lines.some(
      (l) =>
        l.account.startsWith(q) ||
        (name(l.account) ?? '').toLowerCase().includes(q) ||
        (l.description ?? '').toLowerCase().includes(q) ||
        (amount !== null && amount > 0 && (l.debit === amount || l.credit === amount)),
    );
  });
}

function JournalTable({
  entries,
  name,
  onEdit,
  onDelete,
  onChange,
}: {
  entries: StoredEntry[];
  name: (code: string) => string | undefined;
  onEdit: (s: StoredEntry) => void;
  onDelete: (s: StoredEntry) => Promise<void>;
  onChange?: () => void;
}) {
  const [open, setOpen] = useState<number | null>(null);
  if (entries.length === 0)
    return <p className="mt-4 text-sm text-stone-500">Ningún asiento coincide.</p>;
  return (
    <table className="mt-4 w-full text-sm">
      <thead className="text-left text-stone-500">
        <tr>
          <th>Nº</th>
          <th>Fecha</th>
          <th>Cuenta</th>
          <th>Concepto</th>
          <th className="text-right">Debe</th>
          <th className="text-right">Haber</th>
          <th />
        </tr>
      </thead>
      <tbody>
        {entries
          .flatMap((s) => [
            ...s.entry.lines.map((l, n) => ({ s, l, first: n === 0, key: `${s.id}:${n}` })),
            ...(open === s.id ? [{ s, l: null, first: false, key: `${s.id}:extras` }] : []),
          ])
          .map(({ s, l, first, key }) =>
            l === null ? (
              <tr key={key}>
                <td colSpan={7}>
                  <EntryExtras s={s} onChange={() => onChange?.()} />
                </td>
              </tr>
            ) : (
              <tr key={key} className={first ? 'border-t border-stone-300' : ''}>
                <td>{first ? s.number : ''}</td>
                <td>{first ? formatDate(s.entry.date) : ''}</td>
                <td>
                  <span className="font-mono">{l.account}</span>{' '}
                  <span className="text-stone-500">{name(l.account)}</span>
                </td>
                <td>
                  {first && (
                    <>
                      {s.entry.concept} · {originLabel(s.entry.origin)}{' '}
                      <button
                        type="button"
                        className="text-xs text-stone-600 underline"
                        aria-expanded={open === s.id}
                        aria-label={`Observaciones y adjuntos del asiento ${s.number}`}
                        onClick={() => setOpen(open === s.id ? null : s.id)}
                      >
                        {[
                          s.notes ? 'nota' : '',
                          s.attachments
                            ? `${s.attachments} adjunto${s.attachments > 1 ? 's' : ''}`
                            : '',
                        ]
                          .filter(Boolean)
                          .join(' · ') || 'notas y adjuntos'}
                      </button>
                    </>
                  )}
                </td>
                <td className={cell}>{l.debit ? euros(l.debit) : ''}</td>
                <td className={cell}>{l.credit ? euros(l.credit) : ''}</td>
                <td className="whitespace-nowrap pl-2 text-right">
                  {first &&
                    (s.entry.origin === 'Manual' || 'Importado' in Object(s.entry.origin)) && (
                      <>
                        <button
                          type="button"
                          className="text-link"
                          aria-label={`Modificar el asiento ${s.number}`}
                          onClick={() => onEdit(s)}
                        >
                          Modificar
                        </button>{' '}
                        <ConfirmAction
                          label="Borrar"
                          className="text-red-800"
                          question={`¿Borrar el asiento nº ${s.number} «${s.entry.concept}»? Queda constancia en la auditoría.`}
                          confirmLabel={`Sí, borrar el ${s.number}`}
                          onConfirm={() => onDelete(s)}
                        />
                      </>
                    )}
                </td>
              </tr>
            ),
          )}
      </tbody>
    </table>
  );
}

function TrialBalance({
  year,
  onOpenLedger,
}: {
  year: number;
  onOpenLedger?: (code: string) => void;
}) {
  const [digits, setDigits] = useState<number | null>(3);
  const [rows, setRows] = useState<AccountBalance[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setError(null);
    trialBalance(year, digits)
      .then(setRows)
      .catch((e) => {
        setRows([]);
        setError(errorMessage(e));
      });
  }, [year, digits]);
  const d = rows.reduce((s, r) => s + r.debit, 0);
  const c = rows.reduce((s, r) => s + r.credit, 0);
  if (error) return <LoadError message={error} />;
  return (
    <div className="mt-4">
      <label className="text-sm">
        Nivel{' '}
        <select
          value={digits ?? 0}
          onChange={(e) => setDigits(Number(e.target.value) || null)}
          className="rounded border px-1"
        >
          <option value={1}>Grupos</option>
          <option value={3}>Cuentas (3 dígitos)</option>
          <option value={0}>Subcuentas</option>
        </select>
      </label>
      <table className="mt-2 w-full text-sm">
        <thead className="text-left text-stone-500">
          <tr>
            <th>Cuenta</th>
            <th className="text-right">Debe</th>
            <th className="text-right">Haber</th>
            <th className="text-right">Saldo</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.account} className="border-t border-stone-200">
              <td className="font-mono">
                {onOpenLedger ? (
                  <button
                    type="button"
                    className="text-link underline"
                    aria-label={`Mayor de la ${r.account}`}
                    onClick={() => onOpenLedger(r.account)}
                  >
                    {r.account}
                  </button>
                ) : (
                  r.account
                )}
              </td>
              <td className={cell}>{euros(r.debit)}</td>
              <td className={cell}>{euros(r.credit)}</td>
              <td className={cell}>{euros(r.debit - r.credit)}</td>
            </tr>
          ))}
          <tr className="border-t-2 border-stone-400 font-semibold">
            <td>Total</td>
            <td className={cell}>{euros(d)}</td>
            <td className={cell}>{euros(c)}</td>
            <td className={`${cell} ${d === c ? 'text-emerald-700' : 'text-red-700'}`}>
              {d === c ? 'Cuadra' : 'No cuadra'}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  );
}

/** Traspasa el saldo de la cuenta del mayor a otra (asiento manual). */
function TransferBalance({
  from,
  year,
  onDone,
}: {
  from: string;
  year: number;
  onDone: () => void;
}) {
  const [to, setTo] = useState('');
  const [date, setDate] = useState(() => {
    const t = localToday();
    return t.startsWith(String(year)) ? t : `${year}-12-31`;
  });
  const [msg, setMsg] = useState<string | null>(null);
  return (
    <details className="mt-2 text-sm">
      <summary>Traspasar el saldo a otra cuenta</summary>
      <div className="mt-2 flex flex-wrap items-end gap-3">
        <label>
          A la cuenta
          <input
            className="mt-1 block w-32 font-mono"
            value={to}
            onChange={(e) => setTo(e.target.value.trim())}
          />
        </label>
        <label>
          Fecha
          <input
            type="date"
            className="mt-1 block"
            value={date}
            onChange={(e) => setDate(e.target.value)}
          />
        </label>
        <ConfirmAction
          label="Traspasar"
          className="rounded border border-stone-300 px-3"
          danger={false}
          question={`¿Traspasar el saldo de la ${from} a la ${to || '…'} con un asiento el ${formatDate(date)}?`}
          confirmLabel="Sí, traspasar"
          onConfirm={async () => {
            try {
              const id = await transferBalance(from, to, date);
              setMsg(id ? 'Saldo traspasado.' : 'La cuenta no tenía saldo a esa fecha.');
              onDone();
            } catch (e) {
              setMsg(errorMessage(e));
            }
          }}
        />
      </div>
      {msg && <p role="status">{msg}</p>}
    </details>
  );
}

function Ledger({
  year,
  account,
  setAccount,
  onOpenEntry,
}: {
  year: number;
  account: string;
  setAccount: (a: string) => void;
  onOpenEntry?: (number: number) => void;
}) {
  const accounts = useAccounts(year);
  const [rows, setRows] = useState<LedgerLine[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [tickFilter, setTickFilter] = useState<TickFilter>('todos');
  const [version, setVersion] = useState(0);
  // Un fallo al puntear no es un fallo de carga: se avisa sin ocultar el mayor.
  const [actionError, setActionError] = useState<string | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `version` recarga tras un traspaso
  useEffect(() => {
    setError(null);
    setActionError(null);
    if (/^\d{3,12}$/.test(account))
      ledger(year, account)
        .then(setRows)
        .catch((e) => {
          setRows([]);
          setError(errorMessage(e));
        });
  }, [year, account, version]);
  if (error) return <LoadError message={error} />;
  return (
    <div className="mt-4">
      <div className="flex flex-wrap items-end gap-3 text-sm">
        <label>
          Cuenta
          <input
            aria-label="Cuenta del mayor"
            value={account}
            onChange={(e) => setAccount(e.target.value.trim())}
            className="mt-1 block w-32 font-mono"
          />
        </label>
        <span className="min-w-0 pb-2 text-stone-600">{accounts.name(account)}</span>
        <label>
          Desde
          <input
            type="date"
            aria-label="Mayor desde"
            className="mt-1 block"
            value={from}
            onChange={(e) => setFrom(e.target.value)}
          />
        </label>
        <label>
          Hasta
          <input
            type="date"
            aria-label="Mayor hasta"
            className="mt-1 block"
            value={to}
            onChange={(e) => setTo(e.target.value)}
          />
        </label>
        <label>
          Punteo
          <select
            className="mt-1 block"
            value={tickFilter}
            onChange={(e) => setTickFilter(e.target.value as TickFilter)}
          >
            <option value="todos">Todos</option>
            <option value="sin">Sin puntear</option>
            <option value="si">Punteados</option>
          </select>
        </label>
      </div>
      {/^\d{4,12}$/.test(account) && rows.length > 0 && (
        <TransferBalance from={account} year={year} onDone={() => setVersion((n) => n + 1)} />
      )}
      <LedgerTable
        rows={rows}
        from={from}
        to={to}
        tickFilter={tickFilter}
        onOpenEntry={onOpenEntry}
        onTick={(line, ticked) => {
          setActionError(null);
          setRows((rs) => rs.map((r) => (r.line_id === line ? { ...r, ticked } : r)));
          setTick(line, ticked).catch((e) => {
            setRows((rs) => rs.map((r) => (r.line_id === line ? { ...r, ticked: !ticked } : r)));
            setActionError(`No se pudo puntear: ${errorMessage(e)}`);
          });
        }}
      />
      {actionError && (
        <p role="alert" className="mt-2 text-sm text-red-700">
          {actionError}
        </p>
      )}
    </div>
  );
}

/** Movimientos entre `from` y `to` con el saldo anterior (lo acumulado antes de `from`). */
export function ledgerRange(rows: LedgerLine[], from: string, to: string) {
  const before = from ? rows.filter((r) => r.date < from) : [];
  const opening = before.length > 0 ? (before[before.length - 1]?.balance ?? 0) : 0;
  const shown = rows.filter((r) => (!from || r.date >= from) && (!to || r.date <= to));
  return { opening, shown };
}

type TickFilter = 'todos' | 'sin' | 'si';

function LedgerTable({
  rows,
  from,
  to,
  tickFilter,
  onTick,
  onOpenEntry,
}: {
  rows: LedgerLine[];
  from: string;
  to: string;
  tickFilter: TickFilter;
  onTick: (line: number, ticked: boolean) => void;
  onOpenEntry?: ((number: number) => void) | undefined;
}) {
  const range = ledgerRange(rows, from, to);
  const opening = range.opening;
  const shown = range.shown.filter(
    (r) => tickFilter === 'todos' || (tickFilter === 'si') === r.ticked,
  );
  const tickedNet = range.shown.filter((r) => r.ticked).reduce((s, r) => s + r.debit - r.credit, 0);
  const d = shown.reduce((s, r) => s + r.debit, 0);
  const c = shown.reduce((s, r) => s + r.credit, 0);
  return (
    <div>
      <table className="mt-2 w-full text-sm">
        <thead className="text-left text-stone-500">
          <tr>
            <th className="w-6" />
            <th>Fecha</th>
            <th>Nº</th>
            <th>Concepto</th>
            <th className="text-right">Debe</th>
            <th className="text-right">Haber</th>
            <th className="text-right">Saldo</th>
          </tr>
        </thead>
        <tbody>
          {from && (
            <tr className="text-stone-600">
              <td colSpan={6}>Saldo anterior al {formatDate(from)}</td>
              <td className={cell}>{euros(opening)}</td>
            </tr>
          )}
          {shown.map((r) => (
            <tr
              key={r.line_id}
              className={`border-t border-stone-200 ${r.ticked ? 'bg-emerald-50' : ''}`}
            >
              <td>
                <input
                  type="checkbox"
                  aria-label={`Puntear ${r.concept} ${formatDate(r.date)}`}
                  checked={r.ticked}
                  onChange={(e) => onTick(r.line_id, e.target.checked)}
                />
              </td>
              <td>{formatDate(r.date)}</td>
              <td>
                {onOpenEntry ? (
                  <button
                    type="button"
                    className="text-link underline"
                    aria-label={`Ver el asiento ${r.number}`}
                    onClick={() => onOpenEntry(r.number)}
                  >
                    {r.number}
                  </button>
                ) : (
                  r.number
                )}
              </td>
              <td>{r.concept}</td>
              <td className={cell}>{r.debit ? euros(r.debit) : ''}</td>
              <td className={cell}>{r.credit ? euros(r.credit) : ''}</td>
              <td className={cell}>{euros(r.balance)}</td>
            </tr>
          ))}
          <tr className="border-t-2 border-stone-400 font-semibold">
            <td colSpan={4}>
              Total del periodo
              <span className="ml-3 font-normal text-stone-500">punteado: {euros(tickedNet)}</span>
            </td>
            <td className={cell}>{euros(d)}</td>
            <td className={cell}>{euros(c)}</td>
            <td className={cell}>{euros(opening + d - c)}</td>
          </tr>
        </tbody>
      </table>
    </div>
  );
}

const METHOD_LABEL: Record<DepreciationMethod, string> = {
  lineal: 'lineal',
  constante: 'porcentaje constante',
  digitos: 'suma de dígitos',
};

function MethodOptions() {
  return (
    <>
      <option value="lineal">Lineal</option>
      <option value="constante">Porcentaje constante (degresivo)</option>
      <option value="digitos">Suma de dígitos</option>
    </>
  );
}

/**
 * Tipos de bien del alta de inmovilizado con su coeficiente lineal máximo (tabla del art.
 * 12.1.a LIS): equipos para procesos de información 25 %, mobiliario 10 %, elementos de transporte
 * 16 %, maquinaria 12 %, sistemas y programas informáticos 33 %. Mismos coeficientes que las
 * clases de «Recibidas».
 */
const ASSET_TYPES: { account: string; label: string; maxRate: number }[] = [
  { account: '217', label: 'Equipos informáticos', maxRate: 25 },
  { account: '216', label: 'Mobiliario', maxRate: 10 },
  { account: '218', label: 'Elementos de transporte', maxRate: 16 },
  { account: '213', label: 'Maquinaria', maxRate: 12 },
  { account: '206', label: 'Aplicaciones informáticas', maxRate: 33 },
];

/** «25» o «12,5» → puntos básicos; vacío → null; inválido → undefined. */
function parsePercentBp(s: string): number | null | undefined {
  if (!s.trim()) return null;
  const n = Number(s.replace(',', '.'));
  return n > 0 && n <= 100 ? Math.round(n * 100) : undefined;
}

/** Céntimos → «1234,56», para rellenar un importe que se puede retocar. */
const plain = (c: number) => (c / 100).toFixed(2).replace('.', ',');

/** «A2026-0050 · 30/06/2026 · Cliente Uno SL · 10.000,00 €» (y lo que queda, si no es todo). */
function candidateLabel(c: SaleCandidate): string {
  const parts = [c.number, formatDate(c.issue_date)];
  if (c.recipient_name) parts.push(c.recipient_name);
  parts.push(euros(c.base));
  let label = parts.join(' · ');
  if (c.open !== c.base) label += ` (sin vincular ${euros(c.open)})`;
  if (c.rectifies) label += ` · rectifica ${c.rectifies}`;
  return label;
}

/**
 * « · vendido por … (resultado)» o, sin factura vinculada, lo que sigue en la 543 (según su saldo:
 * si un cobro o un asiento ya la bajó, se dice).
 */
function saleText(s: AssetSale): string {
  const billed = s.invoices.some((i) => i.rectifies === null);
  if (!billed && s.price === 0) return '';
  if (!billed && s.unbilled >= s.price)
    return ` · vendido sin factura vinculada: ${euros(s.unbilled)} en la 543`;
  if (!billed && s.unbilled > 0)
    return ` · vendido por ${euros(s.price)} sin factura vinculada: sólo ${euros(s.unbilled)} siguen en la 543 (lo demás se cobró o se corrigió con otro asiento)`;
  if (!billed)
    return ` · vendido por ${euros(s.price)} sin factura vinculada; la 543 ya no tiene su precio (se cobró o se corrigió con otro asiento)`;
  const result =
    s.result === 0
      ? 'sin beneficio ni pérdida'
      : s.result > 0
        ? `beneficio de ${euros(s.result)}`
        : `pérdida de ${euros(-s.result)}`;
  return ` · vendido por ${euros(s.invoiced)} (${result})`;
}

/** Cómo cambia el resultado de la venta al vincular una factura. */
function resultChange(c: number): string {
  if (c === 0) return '';
  return c > 0
    ? `El resultado de la venta sube ${euros(c)}.`
    : `El resultado de la venta baja ${euros(-c)}.`;
}

/** Las facturas encontradas y, si la búsqueda la deja fuera, la ya elegida (no se pierde). */
function withPicked(list: SaleCandidate[], picked: SaleCandidate | null): SaleCandidate[] {
  return picked && !list.some((c) => c.invoice_id === picked.invoice_id) ? [picked, ...list] : list;
}

/**
 * Buscador de la factura de venta (número, cliente o NIF), con el número de las encontradas para
 * el lector de pantalla (`found`: null mientras no hay resultado de lo que se ha escrito).
 */
function InvoiceSearch({
  value,
  found,
  describedBy,
  onChange,
}: {
  value: string;
  found: number | null;
  describedBy: string;
  onChange: (q: string) => void;
}) {
  return (
    <>
      <label>
        Buscar la factura
        <input
          type="search"
          className="mt-1 block w-56"
          placeholder="número, cliente o NIF"
          aria-describedby={describedBy}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
      </label>
      <span className="sr-only" aria-live="polite">
        {found !== null ? plural(found, 'factura encontrada', 'facturas encontradas') : ''}
      </span>
    </>
  );
}

function Assets({ year }: { year: number }) {
  const [assets, setAssets] = useState<FixedAsset[]>([]);
  const blank = () => ({
    name: '',
    cost: '',
    date: `${year}-01-01`,
    rate: '25',
    // El usuario ha tocado el coeficiente: al cambiar el tipo no se le pisa.
    rateEdited: false,
    account: '217',
    method: 'lineal' as DepreciationMethod,
    taxRate: '',
    // Amortización de antes de TaxPain (CONTABILIDAD-05) y día hasta el que llega.
    prior: '',
    priorUntil: `${year - 1}-12-31`,
  });
  const [form, setForm] = useState(blank);
  const [priors, setPriors] = useState<AssetPrior[]>([]);
  const [adding, setAdding] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [disposing, setDisposing] = useState<number | null>(null);
  const [disposal, setDisposal] = useState({ date: '', price: '', invoice: '' });
  const [planning, setPlanning] = useState<number | null>(null);
  // Elemento abierto para editar sus datos o su incentivo fiscal, los incentivos y una versión
  // que cambia al recargar (para la previsión de las amortizaciones).
  const [editing, setEditing] = useState<number | null>(null);
  const [incentiving, setIncentiving] = useState<number | null>(null);
  const [incentives, setIncentives] = useState<IncentivoBien[]>([]);
  const [version, setVersion] = useState(0);
  const [plan, setPlan] = useState({ method: 'lineal' as DepreciationMethod, taxRate: '' });
  // Ventas (CONTABILIDAD-08): facturas vinculadas a cada baja y las que se pueden vincular al
  // elemento abierto (null mientras se buscan por primera vez), con lo escrito en el buscador, la
  // búsqueda a la que responden y la ya elegida.
  const [sales, setSales] = useState<AssetSale[]>([]);
  const [candidates, setCandidates] = useState<SaleCandidate[] | null>(null);
  const [candidatesError, setCandidatesError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [searched, setSearched] = useState('');
  const [picked, setPicked] = useState<SaleCandidate | null>(null);
  const request = useRef(0);
  const searchTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(searchTimer.current), []);
  const [linking, setLinking] = useState<number | null>(null);
  const [link, setLink] = useState({ invoice: '', amount: '' });
  const load = useCallback(() => {
    fixedAssets()
      .then(setAssets)
      .catch((e) => {
        setAssets([]);
        setMsg(errorMessage(e));
      });
    fixedAssetPriors()
      .then(setPriors)
      .catch(() => setPriors([]));
    assetSales()
      .then(setSales)
      .catch(() => setSales([]));
    assetIncentives()
      .then(setIncentives)
      .catch(() => setIncentives([]));
    setVersion((n) => n + 1);
  }, []);
  useEffect(load, [load]);

  /**
   * Busca las facturas que se pueden vincular al elemento `id` (descarta respuestas viejas). Si
   * falla, lo dice: no es que no haya facturas.
   */
  function loadCandidates(id: number, q = '') {
    const mine = ++request.current;
    setCandidatesError(null);
    assetSaleCandidates(id, q)
      .then((c) => {
        if (request.current !== mine) return;
        setCandidates(c);
        setSearched(q);
      })
      .catch((e) => {
        if (request.current !== mine) return;
        const m = `No se han podido buscar las facturas: ${errorMessage(e)}`;
        setCandidates([]);
        setSearched(q);
        setCandidatesError(m);
        setMsg(m);
      });
  }

  /** Al abrir un panel: sin búsqueda, sin factura elegida (o `preselect`) y por cargar. */
  function openCandidates(id: number, preselect?: SaleCandidate) {
    clearTimeout(searchTimer.current);
    setQuery('');
    setSearched('');
    setPicked(preselect ?? null);
    setCandidates(null);
    loadCandidates(id);
  }

  /** Busca por número, cliente o NIF un poco después de dejar de escribir. */
  function search(id: number, q: string) {
    setQuery(q);
    clearTimeout(searchTimer.current);
    searchTimer.current = setTimeout(() => loadCandidates(id, q.trim()), 300);
  }

  /** Abre (o cierra) el panel para vincular una factura a la venta de `id`. */
  function toggleLink(id: number, preselect?: SaleCandidate) {
    if (linking === id && !preselect) {
      setLinking(null);
      return;
    }
    setDisposing(null);
    setLinking(id);
    setLink({
      invoice: preselect ? String(preselect.invoice_id) : '',
      amount: preselect?.suggested != null ? plain(preselect.suggested) : '',
    });
    openCandidates(id, preselect);
  }

  async function add() {
    if (adding) return;
    const cost = parseEuros(form.cost);
    const rate = Number(form.rate.replace(',', '.'));
    if (!form.name.trim() || cost === null || !(rate > 0 && rate <= 100))
      return setMsg('Revisa los datos');
    const taxRate = parsePercentBp(form.taxRate);
    if (taxRate === undefined) return setMsg('Revisa el coeficiente fiscal');
    // Por encima del máximo de la tabla del art. 12.1.a LIS el exceso no es deducible: se admite
    // sólo con un coeficiente fiscal aparte dentro del máximo (el ajuste lo hace el 130/200).
    const type = ASSET_TYPES.find((t) => t.account === form.account);
    if (type && rate > type.maxRate && (taxRate === null || taxRate > type.maxRate * 100)) {
      return setMsg(
        `El máximo fiscal para ${type.label.toLowerCase()} es el ${type.maxRate} % anual (art. 12.1.a LIS): con más, el exceso no se deduce. Baja el coeficiente o pon el fiscal aparte (hasta ${type.maxRate} %).`,
      );
    }
    const prior = form.prior.trim() ? parseEuros(form.prior) : 0;
    if (prior === null || prior < 0 || prior > cost)
      return setMsg('Lo ya amortizado no puede pasar del coste');
    setAdding(true);
    try {
      const asset: FixedAsset = {
        id: 0,
        name: form.name.trim(),
        account: form.account,
        depreciation_account: `28${form.account.slice(1)}`,
        expense_account: form.account.startsWith('20') ? '680' : '681',
        acquired_on: form.date,
        cost,
        residual: 0,
        rate: Math.round(rate * 100),
        disposed_on: null,
        method: form.method,
        tax_rate: taxRate,
      };
      if (prior > 0) await addFixedAssetWithPrior(asset, prior, form.priorUntil);
      else await addFixedAsset(asset);
      // El alta sólo es para amortizarlo: no hace el cargo en la 21x.
      setMsg(
        `«${form.name.trim()}» añadido al inmovilizado para amortizarlo. La compra no se contabiliza aquí: regístrala con su factura recibida (a la ${form.account}); si es de antes de TaxPain, va en Saldos iniciales.`,
      );
      setForm(blank());
      load();
    } catch (e) {
      setMsg(errorMessage(e));
    } finally {
      setAdding(false);
    }
  }

  // Facturas encontradas con lo escrito en el buscador, cuando ya ha llegado la respuesta.
  const found =
    query.trim() && searched === query.trim() && candidates && !candidatesError
      ? candidates.length
      : null;

  return (
    <div className="mt-4 space-y-3 text-sm">
      <ul className="divide-y divide-stone-200">
        {assets.map((a) => {
          const sale = sales.find((s) => s.asset_id === a.id);
          const saleInvoice = sale?.invoices.find((i) => i.rectifies === null);
          const options = withPicked(candidates ?? [], picked);
          const toSell = options.find((c) => String(c.invoice_id) === disposal.invoice);
          const toLink = options.find((c) => String(c.invoice_id) === link.invoice);
          const linkAmount = parseEuros(link.amount);
          const incentivo = incentives.find((i) => i.asset_id === a.id)?.incentivo ?? null;
          return (
            <li key={a.id} className="py-1">
              <div className="flex justify-between">
                <span>
                  {a.name} · {a.account} · desde {formatDate(a.acquired_on)} ·{' '}
                  {(a.rate / 100).toLocaleString('es-ES')} % anual
                  {a.method && a.method !== 'lineal' && ` · ${METHOD_LABEL[a.method]}`}
                  {a.tax_rate != null &&
                    ` · fiscal ${(a.tax_rate / 100).toLocaleString('es-ES')} % lineal`}
                  {incentivo && ` · ${incentiveLabel(incentivo)}`}
                  {a.disposed_on && ` · de baja el ${formatDate(a.disposed_on)}`}
                  {sale && saleText(sale)}
                  {priors
                    .filter((p) => p.id === a.id && p.amount > 0)
                    .map(
                      (p) =>
                        ` · ${euros(p.amount)} amortizados antes de TaxPain${p.until ? ` (hasta el ${formatDate(p.until)})` : ''}`,
                    )}
                </span>
                <span>
                  {euros(a.cost)}
                  {!a.disposed_on && (
                    <button
                      type="button"
                      className="ml-3 text-stone-700 underline"
                      aria-expanded={editing === a.id}
                      onClick={() => setEditing(editing === a.id ? null : a.id)}
                    >
                      Editar
                    </button>
                  )}
                  {!a.disposed_on && (
                    <button
                      type="button"
                      className="ml-3 text-stone-700 underline"
                      aria-expanded={incentiving === a.id}
                      onClick={() => setIncentiving(incentiving === a.id ? null : a.id)}
                    >
                      Incentivo fiscal
                    </button>
                  )}
                  {!saleInvoice && <DeshacerBaja asset={a} onDone={load} onMsg={setMsg} />}
                  {!a.disposed_on && (
                    <button
                      type="button"
                      className="ml-3 text-stone-700 underline"
                      onClick={() => {
                        setPlan({
                          method: a.method ?? 'lineal',
                          taxRate: a.tax_rate == null ? '' : String(a.tax_rate / 100),
                        });
                        setPlanning(planning === a.id ? null : a.id);
                      }}
                    >
                      Plan de amortización
                    </button>
                  )}
                  {!a.disposed_on && (
                    <button
                      type="button"
                      className="ml-3 text-stone-700 underline"
                      onClick={() => {
                        if (disposing === a.id) {
                          setDisposing(null);
                          return;
                        }
                        setLinking(null);
                        setDisposing(a.id);
                        setDisposal({ date: '', price: '', invoice: '' });
                        openCandidates(a.id);
                      }}
                    >
                      Dar de baja o vender
                    </button>
                  )}
                  {sale && !saleInvoice && (
                    <button
                      type="button"
                      className="ml-3 text-stone-700 underline"
                      onClick={() => toggleLink(a.id)}
                    >
                      Vincular la factura de venta
                    </button>
                  )}
                </span>
              </div>
              {sale && sale.invoices.length > 0 && (
                <ul className="mt-1 space-y-1 pl-4 text-stone-600">
                  {sale.invoices.map((i) => (
                    <li key={i.invoice_id} className="flex flex-wrap items-center gap-3">
                      <span>
                        {i.rectifies
                          ? `Rectificativa ${i.number} de ${i.rectifies}`
                          : `Factura de venta ${i.number}`}{' '}
                        · {formatDate(i.issue_date)} · {euros(i.amount)}
                      </span>
                      <ConfirmAction
                        label={`Desvincular ${i.number}`}
                        className="text-red-700 underline"
                        question={`${
                          i.rectifies
                            ? `${i.number} deja de pasar al resultado de la venta y su diferencia vuelve a ventas.`
                            : `${i.number} vuelve a contar como ventas y el precio de la baja queda otra vez en la 543.`
                        } Se borra el asiento que la pasaba: sus documentos y observaciones del Diario, si los tiene, pasan al asiento de la factura. ¿Seguir?`}
                        confirmLabel="Sí, desvincular"
                        onConfirm={async () => {
                          try {
                            await assetSaleUnlink(a.id, i.invoice_id);
                            setMsg(
                              `${i.number} desvinculada de ${a.name}: vuelve a contar como ventas.`,
                            );
                            load();
                          } catch (e) {
                            setMsg(errorMessage(e));
                          }
                        }}
                      />
                    </li>
                  ))}
                </ul>
              )}
              {sale && sale.pending.length > 0 && (
                <ul className="mt-1 space-y-1 pl-4">
                  {sale.pending.map((p) => (
                    <li key={p.invoice_id} className="flex flex-wrap items-center gap-3">
                      <span>
                        Rectificativa {p.number} de {p.rectifies} sin pasar a la venta:{' '}
                        {euros(p.open)}
                        {p.cancels_sale && ' (anula la venta)'}
                      </span>
                      <button
                        type="button"
                        className="text-stone-700 underline"
                        onClick={() => toggleLink(a.id, p)}
                      >
                        Pasar {p.number} a la venta
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              {editing === a.id && (
                <EditarBien
                  asset={a}
                  onMsg={setMsg}
                  onDone={() => {
                    setEditing(null);
                    load();
                  }}
                />
              )}
              {incentiving === a.id && (
                <IncentivoFiscal
                  asset={a}
                  incentivo={incentivo}
                  onMsg={setMsg}
                  onDone={() => {
                    setIncentiving(null);
                    load();
                  }}
                />
              )}
              {planning === a.id && (
                <div className="mt-2 flex flex-wrap items-end gap-3 rounded-md bg-stone-50 p-3">
                  <label>
                    Método contable
                    <select
                      className="mt-1 block"
                      value={plan.method}
                      onChange={(e) =>
                        setPlan({ ...plan, method: e.target.value as DepreciationMethod })
                      }
                    >
                      <MethodOptions />
                    </select>
                  </label>
                  <label>
                    Coeficiente fiscal % (vacío si es el mismo plan)
                    <input
                      className="mt-1 block w-24"
                      inputMode="decimal"
                      value={plan.taxRate}
                      onChange={(e) => setPlan({ ...plan, taxRate: e.target.value })}
                    />
                  </label>
                  <button
                    type="button"
                    className="rounded btn-primary px-3 py-1"
                    onClick={async () => {
                      const t = parsePercentBp(plan.taxRate);
                      if (t === undefined) {
                        setMsg('Revisa el coeficiente fiscal');
                        return;
                      }
                      try {
                        await setDepreciationPlan(a.id, plan.method, t);
                        setMsg(
                          'Plan guardado: vale para los ejercicios sin la amortización registrada.',
                        );
                        setPlanning(null);
                        load();
                      } catch (e) {
                        setMsg(errorMessage(e));
                      }
                    }}
                  >
                    Guardar plan
                  </button>
                </div>
              )}
              {disposing === a.id && (
                <div className="mt-2 flex flex-wrap items-end gap-3 rounded-md bg-stone-50 p-3">
                  <label>
                    Fecha
                    <input
                      type="date"
                      className="mt-1 block"
                      value={disposal.date}
                      onChange={(e) => setDisposal({ ...disposal, date: e.target.value })}
                    />
                  </label>
                  <InvoiceSearch
                    value={query}
                    found={found}
                    describedBy={`venta-ayuda-${a.id}`}
                    onChange={(q) => search(a.id, q)}
                  />
                  <label>
                    Factura de venta (opcional)
                    <select
                      className="mt-1 block max-w-md"
                      value={disposal.invoice}
                      aria-describedby={`venta-ayuda-${a.id}`}
                      onChange={(e) => {
                        const c = options.find((x) => String(x.invoice_id) === e.target.value);
                        setPicked(c ?? null);
                        setDisposal({
                          ...disposal,
                          invoice: e.target.value,
                          ...(c && {
                            price: plain(c.suggested ?? c.open),
                            date: c.operation_date ?? c.issue_date,
                          }),
                        });
                      }}
                    >
                      <option value="">Sin factura (el precio queda en la 543)</option>
                      {options.map((c) => (
                        <option key={c.invoice_id} value={c.invoice_id}>
                          {candidateLabel(c)}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label>
                    Precio de venta (sin IVA; 0 si se tira)
                    <input
                      className="mt-1 block w-36 text-right"
                      inputMode="decimal"
                      value={disposal.price}
                      onChange={(e) => setDisposal({ ...disposal, price: e.target.value })}
                    />
                  </label>
                  <ConfirmAction
                    label="Registrar la baja"
                    className="rounded btn-primary px-3 py-1"
                    question={
                      toSell
                        ? `Se amortiza ${a.name} hasta ese día y se da de baja con su asiento; la base de ${toSell.number} pasa de ventas a la 543, así que el ingreso cuenta una sola vez, como beneficio o pérdida de la venta. ¿Seguir?`
                        : `Se amortiza ${a.name} hasta ese día y se da de baja con su asiento (el precio va a la 543). ¿Seguir?`
                    }
                    confirmLabel="Sí, registrar"
                    danger={false}
                    onConfirm={async () => {
                      const price = parseEuros(disposal.price || '0');
                      if (price === null || !disposal.date) {
                        setMsg('Revisa la fecha y el precio');
                        return;
                      }
                      try {
                        const r = await assetDispose(
                          a.id,
                          disposal.date,
                          price,
                          toSell ? toSell.invoice_id : null,
                        );
                        const parts = [
                          r.result === 0
                            ? 'Baja registrada sin beneficio ni pérdida.'
                            : r.result > 0
                              ? `Baja registrada con un beneficio de ${euros(r.result)}.`
                              : `Baja registrada con una pérdida de ${euros(-r.result)}.`,
                        ];
                        if (r.sale_entry !== null && toSell)
                          parts.push(`La factura ${toSell.number} ya no cuenta como ventas.`);
                        if (r.depreciation_removed > 0)
                          parts.push(
                            `La amortización del ejercicio se ha rehecho hasta el día de la baja (${euros(r.depreciation_removed)} menos).`,
                          );
                        setMsg([...parts, ...r.warnings].join(' '));
                        setDisposing(null);
                        load();
                      } catch (e) {
                        setMsg(errorMessage(e));
                      }
                    }}
                  />
                  <p id={`venta-ayuda-${a.id}`} className="basis-full text-xs text-stone-600">
                    Con su factura, la base deja de contar como ventas: el ingreso queda una sola
                    vez, como beneficio o pérdida de la venta. Salen las más recientes sin las
                    simplificadas: si no está, búscala por número, cliente o NIF. Si aún no la has
                    emitido, véndelo sin factura y vincúlala después.
                  </p>
                </div>
              )}
              {linking === a.id && (
                <div className="mt-2 flex flex-wrap items-end gap-3 rounded-md bg-stone-50 p-3">
                  {!saleInvoice && (
                    <InvoiceSearch
                      value={query}
                      found={found}
                      describedBy={`vincular-ayuda-${a.id}`}
                      onChange={(q) => search(a.id, q)}
                    />
                  )}
                  {candidates === null ? (
                    <p>Buscando las facturas…</p>
                  ) : candidatesError ? (
                    <p className="text-red-700">{candidatesError}</p>
                  ) : options.length === 0 ? (
                    saleInvoice ? (
                      <p>No hay rectificativas de su factura de venta sin pasar a la venta.</p>
                    ) : (
                      <p className="basis-full">
                        {searched
                          ? `Ninguna factura que se pueda vincular lleva «${searched}».`
                          : 'No hay facturas que se puedan vincular.'}
                      </p>
                    )
                  ) : (
                    <>
                      <label>
                        {saleInvoice ? 'Rectificativa de la factura de venta' : 'Factura de venta'}
                        <select
                          className="mt-1 block max-w-md"
                          value={link.invoice}
                          aria-describedby={saleInvoice ? undefined : `vincular-ayuda-${a.id}`}
                          onChange={(e) => {
                            const c = options.find((x) => String(x.invoice_id) === e.target.value);
                            setPicked(c ?? null);
                            setLink({
                              invoice: e.target.value,
                              amount: c?.suggested != null ? plain(c.suggested) : '',
                            });
                          }}
                        >
                          <option value="">Elige la factura</option>
                          {options.map((c) => (
                            <option key={c.invoice_id} value={c.invoice_id}>
                              {candidateLabel(c)}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label>
                        Importe de este elemento (sin IVA)
                        <input
                          className="mt-1 block w-36 text-right"
                          inputMode="decimal"
                          value={link.amount}
                          onChange={(e) => setLink({ ...link, amount: e.target.value })}
                        />
                      </label>
                      <ConfirmAction
                        label="Vincular"
                        className="rounded btn-primary px-3 py-1 disabled:opacity-50"
                        disabled={!toLink}
                        question={
                          !toLink
                            ? ''
                            : toLink.rectifies
                              ? `La diferencia de ${toLink.number} pasa de ventas al resultado de la venta de ${a.name}, con un asiento del ${formatDate(toLink.issue_date)}. ¿Seguir?`
                              : `La base de ${toLink.number} pasa de ventas a la 543 de la venta de ${a.name}, con un asiento del ${formatDate(toLink.issue_date)}.${
                                  sale && linkAmount !== null && linkAmount !== sale.price
                                    ? ` El precio de la baja era ${euros(sale.price)}: la diferencia (${euros(linkAmount - sale.price)}) va al resultado de la venta.`
                                    : ''
                                } Si ya lo corregiste con un asiento manual, no la vincules. ¿Seguir?`
                        }
                        confirmLabel="Sí, vincular"
                        danger={false}
                        onConfirm={async () => {
                          if (!toLink) return;
                          const amount = link.amount.trim() ? linkAmount : null;
                          if (link.amount.trim() && amount === null) {
                            setMsg('Revisa el importe');
                            return;
                          }
                          try {
                            const r = await assetSaleLink(a.id, toLink.invoice_id, amount);
                            const done = toLink.rectifies
                              ? `${toLink.number} pasa al resultado de la venta de ${a.name}.`
                              : `${toLink.number} vinculada a la venta de ${a.name}: ya no cuenta como ventas.`;
                            setMsg(
                              [done, resultChange(r.result_change), ...r.warnings]
                                .filter(Boolean)
                                .join(' '),
                            );
                            setLinking(null);
                            load();
                          } catch (e) {
                            setMsg(errorMessage(e));
                          }
                        }}
                      />
                      {toLink?.cancels_sale && (
                        <p className="basis-full text-red-700">
                          {toLink.number} anula la venta entera: TaxPain no deshace la baja. Si el
                          bien vuelve, dalo de alta de nuevo y ajusta el resultado con un asiento
                          manual; si no vuelve, vincúlala.
                        </p>
                      )}
                    </>
                  )}
                  {!saleInvoice && sale && sale.unbilled < sale.price && (
                    <p className="basis-full text-red-700">
                      La 543 ya no tiene todo el precio de esta venta (le quedan{' '}
                      {euros(sale.unbilled)} de {euros(sale.price)}): si cobraste contra la 543,
                      deshaz esa conciliación en Banco antes de vincular la factura; si lo
                      corregiste con un asiento manual, no la vincules.
                    </p>
                  )}
                  {!saleInvoice && (
                    <p id={`vincular-ayuda-${a.id}`} className="basis-full text-xs text-stone-600">
                      Salen las más cercanas a la baja, sin las simplificadas (ésas, buscándolas), y
                      sólo las emitidas en TaxPain (no las del libro de la gestoría), sin anular y
                      de un ejercicio abierto. Si la factura está en un ejercicio cerrado, reábrelo
                      desde Cierre; si aún no la has emitido, emítela en Facturas; si no se puede
                      vincular, salda la 543 con un asiento manual.
                    </p>
                  )}
                </div>
              )}
            </li>
          );
        })}
      </ul>
      <div className="form-grid">
        <label className="span-2">
          Elemento
          <input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
        </label>
        <label className="span-2">
          Tipo
          <select
            value={form.account}
            onChange={(e) => {
              const max = ASSET_TYPES.find((t) => t.account === e.target.value)?.maxRate;
              setForm({
                ...form,
                account: e.target.value,
                rate: form.rateEdited || max === undefined ? form.rate : String(max),
              });
            }}
          >
            {ASSET_TYPES.map((t) => (
              <option key={t.account} value={t.account}>
                {t.label} ({t.account} · {t.maxRate} %)
              </option>
            ))}
          </select>
        </label>
        <label>
          Coste
          <input
            className="text-right"
            inputMode="decimal"
            value={form.cost}
            onChange={(e) => setForm({ ...form, cost: e.target.value })}
          />
        </label>
        <label>
          Fecha de alta
          <input
            type="date"
            value={form.date}
            onChange={(e) => setForm({ ...form, date: e.target.value })}
          />
        </label>
        <label>
          Coeficiente %
          <input
            inputMode="decimal"
            value={form.rate}
            onChange={(e) => setForm({ ...form, rate: e.target.value, rateEdited: true })}
          />
        </label>
        <label>
          Método
          <select
            value={form.method}
            onChange={(e) => setForm({ ...form, method: e.target.value as DepreciationMethod })}
          >
            <MethodOptions />
          </select>
        </label>
        <label>
          Coeficiente fiscal % (si difiere)
          <input
            inputMode="decimal"
            value={form.taxRate}
            onChange={(e) => setForm({ ...form, taxRate: e.target.value })}
          />
        </label>
        <label>
          Ya amortizado antes de TaxPain (si es de antes)
          <input
            className="text-right"
            inputMode="decimal"
            value={form.prior}
            onChange={(e) => setForm({ ...form, prior: e.target.value })}
          />
        </label>
        {form.prior.trim() !== '' && (
          <label>
            Amortizado hasta el
            <input
              type="date"
              value={form.priorUntil}
              onChange={(e) => setForm({ ...form, priorUntil: e.target.value })}
            />
          </label>
        )}
        <div className="form-actions">
          <button
            type="button"
            className="rounded btn-primary px-3 py-1 disabled:opacity-50"
            disabled={adding}
            onClick={add}
          >
            Añadir
          </button>
        </div>
      </div>
      <InventarioGestor year={year} onDone={load} />
      <AmortizacionesEjercicio year={year} version={version} onDone={load} onMsg={setMsg} />
      {msg && <p role="status">{msg}</p>}
    </div>
  );
}

function Close({ year, onGo }: { year: number; onGo?: IrA }) {
  const [msg, setMsg] = useState<string | null>(null);
  // Tras cerrar o reabrir se vuelve a leer todo (comprobaciones, IS, aplicación).
  const [version, setVersion] = useState(0);
  return (
    <GuidedClosing
      key={version}
      year={year}
      ajustes={(onChanged) => <AjustesCierre year={year} onGo={onGo} onChanged={onChanged} />}
    >
      <p>
        El cierre regulariza ingresos y gastos contra la cuenta 129, deja todas las cuentas a cero,
        abre el ejercicio {year + 1} y <strong>bloquea {year}</strong>: después no se podrán añadir
        ni cambiar asientos ni facturas de ese año. Antes, registra las amortizaciones y revisa las
        recibidas pendientes.
      </p>
      <div id="paso-existencias">
        <Existencias year={year} />
      </div>
      <EntidadPatrimonial year={year} />
      <div className="form-actions mt-5 border-t border-stone-200 pt-4">
        <ConfirmAction
          className="rounded btn-danger px-3 py-1.5"
          label={`Cerrar el ejercicio ${year}`}
          question={
            <>
              ¿Cerrar el ejercicio {year}? Ya no se podrán añadir ni cambiar asientos ni facturas de
              ese año; si luego hay que corregir algo, se puede reabrir con «Reabrir {year}». Los
              movimientos del banco de {year} que sigan sin conciliar dejarán de salir como
              pendientes: concílialos antes.
            </>
          }
          confirmLabel={`Sí, cerrar ${year}`}
          onConfirm={async () => {
            try {
              const r = await closeYear(year, true);
              setMsg(
                `Ejercicio ${year} cerrado. Resultado: ${euros(r.result)}.${r.aviso ? ` ${r.aviso}` : ''}`,
              );
              setVersion((v) => v + 1);
            } catch (e) {
              setMsg(errorMessage(e));
            }
          }}
        />
        <ConfirmAction
          className="rounded border border-stone-300 px-3 py-1.5"
          label={`Reabrir ${year}`}
          question={
            <>
              ¿Reabrir el ejercicio {year}? Se quitan su regularización, su cierre y la apertura del
              siguiente para poder corregirlo; después hay que volver a cerrarlo. Si ya legalizaste
              los libros o depositaste las cuentas, tendrás que rehacerlos.
            </>
          }
          confirmLabel={`Sí, reabrir ${year}`}
          onConfirm={async () => {
            try {
              const n = await reopenYear(year);
              setMsg(`Ejercicio ${year} reabierto (${n} asientos de cierre quitados).`);
              setVersion((v) => v + 1);
            } catch (e) {
              setMsg(errorMessage(e));
            }
          }}
        />
      </div>
      {msg && <p role="status">{msg}</p>}
    </GuidedClosing>
  );
}

export function Accounting({
  initialView = 'diario',
  initialYear,
  initialEntry,
  initialAccount,
}: {
  initialView?: View;
  /** Abrir en un ejercicio, un asiento o el mayor de una cuenta (desde la búsqueda). */
  initialYear?: number | undefined;
  initialEntry?: number | undefined;
  initialAccount?: string | undefined;
} = {}) {
  const [view, setView] = useState<View>(
    initialAccount ? 'mayor' : initialEntry ? 'diario' : initialView,
  );
  const [year, setYear] = useState(initialYear ?? new Date().getFullYear());
  const [fy, setFy] = useState<FiscalYearInfo | null>(null);
  const [exported, setExported] = useState<string | null>(null);
  const [ledgerAccount, setLedgerAccount] = useState(initialAccount ?? '572');
  const [focusEntry, setFocusEntry] = useState<number | null>(initialEntry ?? null);
  // Con un año pedido (búsqueda), no se cambia al ejercicio en curso.
  const asked = useRef(initialYear !== undefined);
  useEffect(() => {
    // La primera vez, el ejercicio en curso (en uno no natural puede ser el del año anterior).
    const first = !asked.current;
    asked.current = true;
    fiscalYearInfo(first ? null : year)
      .then((f) => {
        setFy(f);
        if (first && f.year !== year) setYear(f.year);
      })
      .catch(() => setFy(null));
  }, [year]);
  return (
    <section className="mt-6">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold">
          Contabilidad {fy && fy.year === year ? fy.label : year}
          {fy && fy.year === year && !fy.natural && (
            <span className="ml-2 text-sm font-normal text-stone-600">
              del {formatDate(fy.start)} al {formatDate(fy.end)}
            </span>
          )}
        </h2>
        <div className="flex items-center gap-2 text-sm">
          <button
            type="button"
            className="btn-ghost"
            onClick={async () => {
              const dir = await pickFolder();
              if (!dir) return;
              try {
                setExported(`Guardado en ${await exportListings(year, dir)}`);
              } catch (e) {
                setExported(errorMessage(e));
              }
            }}
          >
            <Download size={16} aria-hidden="true" />
            Exportar a Excel…
          </button>
          <YearStepper
            year={year}
            onChange={(y) => {
              setExported(null);
              setYear(y);
            }}
          />
        </div>
      </div>
      {exported && (
        <p role="status" className="text-sm">
          {exported}
        </p>
      )}
      <nav className="mt-2 flex gap-1" aria-label="Libros">
        {VIEWS.map((v) => (
          <button
            key={v.id}
            type="button"
            aria-current={view === v.id ? 'page' : undefined}
            onClick={() => {
              setFocusEntry(null);
              setExported(null);
              setView(v.id);
            }}
            className={`rounded px-2 py-1 text-sm ${view === v.id ? 'bg-stone-200 font-medium' : 'text-stone-600'}`}
          >
            {v.label}
          </button>
        ))}
      </nav>
      {view === 'diario' && (
        <Journal year={year} focus={focusEntry} onClearFocus={() => setFocusEntry(null)} />
      )}
      {view === 'sumas' && (
        <TrialBalance
          year={year}
          onOpenLedger={(code) => {
            setLedgerAccount(code);
            setView('mayor');
          }}
        />
      )}
      {view === 'cartera' && <Cartera />}
      {view === 'plan' && (
        <ChartOfAccounts
          year={year}
          onOpenLedger={(code) => {
            setLedgerAccount(code);
            setView('mayor');
          }}
        />
      )}
      {view === 'mayor' && (
        <Ledger
          year={year}
          account={ledgerAccount}
          setAccount={setLedgerAccount}
          onOpenEntry={(n) => {
            setFocusEntry(n);
            setView('diario');
          }}
        />
      )}
      {view === 'banco' && <Bank />}
      {view === 'inmovilizado' && (
        <>
          <Assets year={year} />
          <LeasingView year={year} />
        </>
      )}
      {view === 'prestamos' && <Prestamos year={year} />}
      {view === 'subvenciones' && <Subvenciones year={year} />}
      {view === 'cuentas' && (
        <>
          <AnnualAccountsView year={year} />
          <Ratios year={year} />
          <Deposito year={year} />
          <Actas year={year} />
        </>
      )}
      {view === 'analitica' && <Analitica year={year} />}
      {view === 'nominas' && <Payroll year={year} />}
      {view === 'impuestos' && (
        <>
          <TaxActions year={year} />
          <IvaHolding year={year} />
          <Oss year={year} />
        </>
      )}
      {view === 'cierre' && (
        <Close
          year={year}
          onGo={(v, account) => {
            if (account) setLedgerAccount(account);
            setFocusEntry(null);
            setView(VIEWS.some((x) => x.id === v) ? (v as View) : 'diario');
          }}
        />
      )}
      {view === 'societarios' && <LibrosSocietarios year={year} />}
      {view === 'apertura' && <Apertura year={year} onClose={() => setView('cierre')} />}
      {view === 'cripto' && <Criptomonedas year={year} />}
      {view === 'vinculadas' && <Vinculadas year={year} />}
    </section>
  );
}

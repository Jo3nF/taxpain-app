import { useCallback, useEffect, useId, useState } from 'react';
import {
  aperturaApplied,
  aperturaApply,
  aperturaBanks,
  aperturaDiscard,
  aperturaDraft,
  aperturaReadBalance,
  aperturaReadInventory,
  aperturaReadPending,
  aperturaSave,
  aperturaSummary,
  aperturaUndo,
  emptyOpeningDraft,
  errorMessage,
  euros,
  fiscalYearInfo,
  formatDate,
  type OpeningApplied,
  type OpeningAsset,
  type OpeningBankInfo,
  type OpeningDraft,
  type OpeningLine,
  type OpeningPending,
  type OpeningSummary,
  parseEuros,
  pickOpeningFile,
  plural,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { EjercicioGestor } from './EjercicioGestor';

/** Pasos del asistente, con lo que se explica en cada uno. */
const STEPS = [
  'Fecha',
  'Balance del gestor',
  'Clientes pendientes',
  'Proveedores pendientes',
  'Inmovilizado',
  'Hacienda y bancos',
  'Revisar y contabilizar',
] as const;

const LAST = STEPS.length - 1;

/** Día anterior a una fecha ISO. */
function dayBefore(iso: string): string {
  const d = new Date(`${iso}T12:00:00Z`);
  d.setUTCDate(d.getUTCDate() - 1);
  return d.toISOString().slice(0, 10);
}

function addDays(iso: string, days: number): string {
  const d = new Date(`${iso}T12:00:00Z`);
  d.setUTCDate(d.getUTCDate() + days);
  return d.toISOString().slice(0, 10);
}

/** «1.234,56» (sin el símbolo) para un campo de importe. */
function amountText(cents: number): string {
  return cents ? euros(cents).replace(/\s?€$/, '') : '';
}

/** Saldo (debe − haber) de las líneas del balance cuya cuenta es `code` o una subcuenta suya. */
function balanceOf(lines: OpeningLine[], code: string): number | null {
  const hits = lines.filter((l) => l.account.startsWith(code) || code.startsWith(l.account));
  return hits.length ? hits.reduce((s, l) => s + l.debit - l.credit, 0) : null;
}

/** Subcuenta del balance con el mismo nombre que el tercero (para no tener que buscarla). */
function guessAccount(name: string, lines: OpeningLine[], prefixes: string[]): string | null {
  const n = name.trim().toLowerCase();
  if (!n) return null;
  const hit = lines.find(
    (l) =>
      prefixes.some((p) => l.account.startsWith(p)) &&
      l.account.length > 3 &&
      l.name.trim().toLowerCase() === n,
  );
  return hit?.account ?? null;
}

/** Campo de importe en euros: se interpreta al salir del campo. */
function Euros({
  value,
  onChange,
  label,
  className = 'w-28 text-right',
}: {
  value: number;
  onChange: (cents: number) => void;
  label: string;
  className?: string;
}) {
  const [text, setText] = useState(amountText(value));
  const [bad, setBad] = useState(false);
  useEffect(() => setText(amountText(value)), [value]);
  return (
    <input
      aria-label={label}
      aria-invalid={bad || undefined}
      inputMode="decimal"
      className={`${className} ${bad ? 'border-red-600' : ''}`}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => {
        const c = text.trim() ? parseEuros(text) : 0;
        if (c === null) {
          setBad(true);
          return;
        }
        setBad(false);
        onChange(c);
      }}
    />
  );
}

/** Lista de avisos de una lectura o de la revisión. */
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

/**
 * Asistente de saldos iniciales: para quien empieza con TaxPain a mitad de camino. Recoge lo que
 * la empresa ya tenía (balance del gestor, cobros y pagos pendientes, inmovilizado, Hacienda y
 * bancos) y lo contabiliza en un asiento de apertura. El borrador se guarda para seguir después.
 */
export function Apertura({ year, onClose }: { year: number; onClose?: (() => void) | undefined }) {
  const [mode, setMode] = useState<'saldos' | 'gestor'>('saldos');
  const [draft, setDraft] = useState<OpeningDraft | null>(null);
  const [applied, setApplied] = useState<OpeningApplied[]>([]);
  const [msg, setMsg] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);

  const load = useCallback(() => {
    aperturaDraft()
      .then((d) => {
        setDraft(d);
        setDirty(false);
      })
      .catch((e) => {
        setDraft(emptyOpeningDraft());
        setMsg(errorMessage(e));
      });
    aperturaApplied()
      .then(setApplied)
      .catch(() => setApplied([]));
  }, []);
  useEffect(load, [load]);

  if (!draft) return <p className="mt-4 text-sm">Cargando…</p>;

  const update = (patch: Partial<OpeningDraft>) => {
    setDraft({ ...draft, ...patch });
    setDirty(true);
  };
  const step = Math.min(Math.max(draft.step, 0), LAST);

  async function save(next: OpeningDraft, note: string | null) {
    try {
      await aperturaSave(next);
      setDirty(false);
      if (note) setMsg(note);
    } catch (e) {
      setMsg(errorMessage(e));
    }
  }
  function go(to: number) {
    if (!draft) return;
    const next = { ...draft, step: to };
    setDraft(next);
    setMsg(null);
    void save(next, null);
  }

  const modes = (
    <fieldset className="mt-4 text-sm">
      <legend className="font-medium">¿Qué tienes del gestor?</legend>
      <div className="mt-1 flex flex-wrap gap-2">
        {(
          [
            ['saldos', 'Un balance: pongo los saldos iniciales'],
            ['gestor', 'Un diario entero: importo el ejercicio asiento a asiento'],
          ] as const
        ).map(([m, label]) => (
          <button
            key={m}
            type="button"
            aria-pressed={mode === m}
            onClick={() => setMode(m)}
            className={`rounded border px-2 py-1 ${mode === m ? 'border-stone-500 bg-stone-200 font-medium' : 'border-stone-300 text-stone-700'}`}
          >
            {label}
          </button>
        ))}
      </div>
    </fieldset>
  );
  if (mode === 'gestor') {
    return (
      <>
        {modes}
        <EjercicioGestor year={year} onClose={onClose} />
      </>
    );
  }

  return (
    <section aria-labelledby="apertura-title" className="mt-4 space-y-4 text-sm">
      {modes}
      <div>
        <h3 id="apertura-title" className="text-base font-semibold">
          Saldos iniciales
        </h3>
        <p className="mt-1 max-w-3xl text-stone-700">
          Si empiezas con TaxPain cuando la empresa ya tenía historia, aquí pones lo que había el
          día en que empiezas: el balance que te da tu gestor, lo que te deben y lo que debes, tus
          bienes (ordenador, furgoneta…) con lo que ya se amortizaron, lo pendiente con Hacienda y
          lo que hay en el banco. Con todo, TaxPain hace el asiento de apertura. Puedes dejarlo a
          medias y seguir otro día.
        </p>
      </div>

      {applied.length > 0 && (
        <AppliedList
          items={applied}
          onUndone={() => {
            setMsg('Apertura deshecha: tienes otra vez el borrador para corregirlo.');
            load();
          }}
        />
      )}

      <nav aria-label="Pasos del asistente">
        <ol className="flex flex-wrap gap-1">
          {STEPS.map((s, i) => (
            <li key={s}>
              <button
                type="button"
                aria-current={i === step ? 'step' : undefined}
                onClick={() => go(i)}
                className={`rounded px-2 py-1 ${i === step ? 'bg-stone-200 font-medium' : 'text-stone-700'}`}
              >
                {i + 1}. {s}
              </button>
            </li>
          ))}
        </ol>
      </nav>

      <div className="card p-5">
        <h4 className="font-semibold">
          Paso {step + 1} de {STEPS.length}: {STEPS[step]}
        </h4>
        {step === 0 && <DateStep year={year} draft={draft} update={update} />}
        {step === 1 && <BalanceStep draft={draft} update={update} />}
        {step === 2 && <PendingStep kind="emitida" draft={draft} update={update} />}
        {step === 3 && <PendingStep kind="recibida" draft={draft} update={update} />}
        {step === 4 && <AssetsStep draft={draft} update={update} />}
        {step === 5 && <TaxBankStep draft={draft} update={update} />}
        {step === LAST && (
          <ReviewStep
            draft={draft}
            onApplied={(a) => {
              setMsg(
                `Apertura contabilizada: asiento nº ${a.entry_number ?? '—'} del ${formatDate(a.date)}, ${plural(a.pending, 'pendiente')} en Cobros y pagos y ${plural(a.assets, 'elemento')} en Inmovilizado.`,
              );
              load();
            }}
          />
        )}
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          className="rounded border border-stone-300 px-3 py-1"
          disabled={step === 0}
          onClick={() => go(step - 1)}
        >
          Anterior
        </button>
        {step < LAST && (
          <button
            type="button"
            className="rounded btn-primary px-3 py-1"
            onClick={() => go(step + 1)}
          >
            Siguiente
          </button>
        )}
        <button
          type="button"
          className="btn-ghost"
          onClick={() => save(draft, 'Guardado: puedes seguir otro día desde aquí.')}
        >
          Guardar y seguir después
        </button>
        {dirty && <span className="text-stone-600">Hay cambios sin guardar.</span>}
        <ConfirmAction
          label="Empezar de nuevo"
          className="ml-auto text-stone-700 underline"
          question="Se borra este borrador (no lo ya contabilizado). ¿Seguir?"
          confirmLabel="Sí, borrar el borrador"
          onConfirm={async () => {
            await aperturaDiscard();
            setMsg('Borrador borrado.');
            load();
          }}
        />
      </div>
      {msg && (
        <p role="status" className="text-stone-800">
          {msg}
        </p>
      )}
    </section>
  );
}

type StepProps = { draft: OpeningDraft; update: (patch: Partial<OpeningDraft>) => void };

function AppliedList({ items, onUndone }: { items: OpeningApplied[]; onUndone: () => void }) {
  return (
    <div className="rounded-md bg-stone-50 p-3">
      <p className="font-medium">Aperturas ya contabilizadas con este asistente</p>
      <ul className="mt-1 space-y-1">
        {items.map((a) => (
          <li key={a.id} className="flex flex-wrap items-center gap-3">
            <span>
              Ejercicio {a.fiscal_year}: asiento nº {a.entry_number ?? '—'} del {formatDate(a.date)}
              , {plural(a.pending, 'pendiente')}, {plural(a.assets, 'elemento')} de inmovilizado
            </span>
            <ConfirmAction
              label="Deshacer"
              className="text-stone-700 underline"
              question="Se borran el asiento de apertura, sus pendientes de la cartera y su inmovilizado, y vuelve el borrador para corregirlo. No se puede si ya cobraste o pagaste alguno, ni si ya contabilizaste amortizaciones de su inmovilizado (esas no se pueden borrar). ¿Seguir?"
              confirmLabel="Sí, deshacer"
              onConfirm={async () => {
                await aperturaUndo(a.id, true);
                onUndone();
              }}
            />
          </li>
        ))}
      </ul>
    </div>
  );
}

function DateStep({ year, draft, update }: StepProps & { year: number }) {
  const [start, setStart] = useState<string | null>(null);
  const id = useId();
  useEffect(() => {
    fiscalYearInfo(year)
      .then((f) => setStart(f.start))
      .catch(() => setStart(null));
  }, [year]);
  return (
    <div className="mt-2 space-y-3">
      <p className="max-w-3xl text-stone-700">
        ¿Desde qué día llevas las cuentas con TaxPain? Lo normal es el primer día del ejercicio (el
        1 de enero si tu año fiscal es el natural): así los saldos son los del cierre del año
        anterior que te da el gestor. Si empiezas a mitad de año, los saldos tienen que ser los de
        ese día, con los gastos e ingresos que ya llevas.
      </p>
      <div className="flex flex-wrap items-end gap-3">
        <label htmlFor={id}>
          Fecha de los saldos
          <input
            id={id}
            type="date"
            className="mt-1 block"
            value={draft.date ?? ''}
            onChange={(e) => update({ date: e.target.value || null })}
          />
        </label>
        {start && draft.date !== start && (
          <button
            type="button"
            className="text-stone-700 underline"
            onClick={() => update({ date: start })}
          >
            Usar el primer día del ejercicio {year} ({formatDate(start)})
          </button>
        )}
      </div>
    </div>
  );
}

function BalanceStep({ draft, update }: StepProps) {
  const [notes, setNotes] = useState<string[]>([]);
  const [read, setRead] = useState<string | null>(null);
  const lines = draft.lines;
  const set = (i: number, patch: Partial<OpeningLine>) =>
    update({ lines: lines.map((l, j) => (j === i ? { ...l, ...patch } : l)) });
  const debit = lines.reduce((s, l) => s + l.debit, 0);
  const credit = lines.reduce((s, l) => s + l.credit, 0);
  const diff = debit - credit;

  async function importFile() {
    const path = await pickOpeningFile();
    if (!path) return;
    try {
      const r = await aperturaReadBalance(path);
      const what = {
        sumas_saldos: 'balance de sumas y saldos',
        apertura: 'asiento de apertura del diario',
        cierre: 'asiento de cierre del diario (al revés)',
        diario: 'saldos de todo el diario',
      }[r.kind];
      update({
        lines: r.lines,
        source: path.split(/[\\/]/).pop() ?? path,
        date: draft.date ?? r.date,
      });
      setRead(`Leído: ${what}, ${plural(r.lines.length, 'cuenta')}.`);
      setNotes(r.warnings);
    } catch (e) {
      setRead(null);
      setNotes([errorMessage(e)]);
    }
  }

  return (
    <div className="mt-2 space-y-3">
      <p className="max-w-3xl text-stone-700">
        Pide a tu gestor el <strong>balance de sumas y saldos</strong> del día anterior (o el diario
        con el asiento de apertura) en Excel o CSV y tráelo aquí; también puedes poner las cuentas a
        mano. Cada cuenta va con su saldo: en el <em>debe</em> lo que tienes o te deben (banco,
        clientes, bienes), en el <em>haber</em> lo que debes y tus fondos propios (capital,
        reservas, proveedores, Hacienda). Al final el debe y el haber tienen que sumar lo mismo.
      </p>
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          className="rounded border border-stone-300 px-3 py-1"
          onClick={importFile}
        >
          Importar del gestor…
        </button>
        <button
          type="button"
          className="rounded border border-stone-300 px-3 py-1"
          onClick={() =>
            update({ lines: [...lines, { account: '', name: '', debit: 0, credit: 0 }] })
          }
        >
          Añadir cuenta
        </button>
      </div>
      {read && <p role="status">{read}</p>}
      <Notes items={notes} />
      {draft.source && <p className="text-stone-600">Del fichero «{draft.source}».</p>}
      {lines.length > 0 && (
        <table className="w-full max-w-3xl">
          <caption className="sr-only">Cuentas del balance de apertura</caption>
          <thead>
            <tr className="text-left text-stone-600">
              <th scope="col">Cuenta</th>
              <th scope="col">Nombre</th>
              <th scope="col" className="text-right">
                Debe
              </th>
              <th scope="col" className="text-right">
                Haber
              </th>
              <th scope="col">
                <span className="sr-only">Quitar</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {lines.map((l, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: las líneas no tienen id propio
              <tr key={i}>
                <td>
                  <input
                    aria-label={`Cuenta de la línea ${i + 1}`}
                    inputMode="numeric"
                    className="w-28"
                    value={l.account}
                    onChange={(e) => set(i, { account: e.target.value.trim() })}
                  />
                </td>
                <td>
                  <input
                    aria-label={`Nombre de la cuenta de la línea ${i + 1}`}
                    className="w-full"
                    value={l.name}
                    onChange={(e) => set(i, { name: e.target.value })}
                  />
                </td>
                <td className="text-right">
                  <Euros
                    label={`Debe de la línea ${i + 1}`}
                    value={l.debit}
                    onChange={(c) => set(i, { debit: c })}
                  />
                </td>
                <td className="text-right">
                  <Euros
                    label={`Haber de la línea ${i + 1}`}
                    value={l.credit}
                    onChange={(c) => set(i, { credit: c })}
                  />
                </td>
                <td>
                  <button
                    type="button"
                    className="text-stone-700 underline"
                    aria-label={`Quitar la cuenta ${l.account || i + 1}`}
                    onClick={() => update({ lines: lines.filter((_, j) => j !== i) })}
                  >
                    Quitar
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <p
        aria-live="polite"
        className={diff === 0 && lines.length ? 'text-emerald-800' : 'text-stone-800'}
      >
        Debe {euros(debit)} · Haber {euros(credit)} ·{' '}
        {diff === 0
          ? lines.length
            ? 'cuadra.'
            : 'sin cuentas todavía.'
          : `diferencia ${euros(Math.abs(diff))}: lo de los pasos siguientes (Hacienda, bancos…) puede completarlo.`}
      </p>
    </div>
  );
}

function PendingStep({ kind, draft, update }: StepProps & { kind: 'emitida' | 'recibida' }) {
  const customers = kind === 'emitida';
  const key = customers ? 'customers' : 'suppliers';
  const items = draft[key];
  const prefixes = customers ? ['43'] : ['40', '41'];
  const listId = useId();
  const [notes, setNotes] = useState<string[]>([]);
  const [book, setBook] = useState<OpeningPending[] | null>(null);
  const [chosen, setChosen] = useState<Set<number>>(new Set());
  const set = (i: number, patch: Partial<OpeningPending>) =>
    update({ [key]: items.map((p, j) => (j === i ? { ...p, ...patch } : p)) });
  const withAccounts = (list: OpeningPending[]) =>
    list.map((p) => ({ ...p, account: p.account ?? guessAccount(p.name, draft.lines, prefixes) }));
  const accounts = draft.lines.filter((l) => prefixes.some((p) => l.account.startsWith(p)));
  const total = items.reduce((s, p) => s + p.amount, 0);
  const opening = draft.date ?? new Date().toISOString().slice(0, 10);

  async function importFile() {
    const path = await pickOpeningFile();
    if (!path) return;
    try {
      const r = await aperturaReadPending(kind, path);
      setNotes(r.warnings);
      if (r.from_book) {
        setBook(r.items);
        setChosen(new Set());
      } else {
        update({ [key]: [...items, ...withAccounts(r.items)] });
        setNotes([
          `${plural(r.items.length, 'pendiente añadido', 'pendientes añadidos')}.`,
          ...r.warnings,
        ]);
      }
    } catch (e) {
      setNotes([errorMessage(e)]);
    }
  }

  return (
    <div className="mt-2 space-y-3">
      <p className="max-w-3xl text-stone-700">
        {customers
          ? 'Las facturas de antes de TaxPain que tus clientes aún no te han pagado. Entran en «Cobros y pagos»: cuando llegue el dinero al banco, TaxPain lo reconocerá y las dará por cobradas.'
          : 'Las facturas de antes de TaxPain que aún no has pagado a tus proveedores. Entran en «Cobros y pagos» para que el pago del banco las salde.'}{' '}
        Si el balance del gestor trae la subcuenta de cada{' '}
        {customers ? 'cliente (430…)' : 'proveedor (400… o 410…)'}, elígela para que todo cuadre.
      </p>
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          className="rounded border border-stone-300 px-3 py-1"
          onClick={importFile}
        >
          Importar listado o libro…
        </button>
        <button
          type="button"
          className="rounded border border-stone-300 px-3 py-1"
          onClick={() =>
            update({
              [key]: [
                ...items,
                {
                  account: null,
                  tax_id: null,
                  name: '',
                  number: '',
                  issue_date: dayBefore(opening),
                  due_date: addDays(opening, 30),
                  amount: 0,
                },
              ],
            })
          }
        >
          Añadir {customers ? 'cobro pendiente' : 'pago pendiente'}
        </button>
      </div>
      <Notes items={notes} />
      {book && (
        <fieldset className="rounded-md bg-stone-50 p-3">
          <legend className="font-medium">
            Es el libro de facturas entero: marca las que siguen sin{' '}
            {customers ? 'cobrar' : 'pagar'}
          </legend>
          <ul className="mt-2 max-h-64 space-y-1 overflow-auto">
            {book.map((p, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: facturas del libro, en su orden
              <li key={i}>
                <label className="flex gap-2">
                  <input
                    type="checkbox"
                    checked={chosen.has(i)}
                    onChange={(e) => {
                      const next = new Set(chosen);
                      if (e.target.checked) next.add(i);
                      else next.delete(i);
                      setChosen(next);
                    }}
                  />
                  {formatDate(p.issue_date)} · {p.number} · {p.name} · {euros(p.amount)}
                </label>
              </li>
            ))}
          </ul>
          <div className="mt-2 flex gap-2">
            <button
              type="button"
              className="rounded btn-primary px-3 py-1"
              disabled={chosen.size === 0}
              onClick={() => {
                update({
                  [key]: [...items, ...withAccounts(book.filter((_, i) => chosen.has(i)))],
                });
                setBook(null);
              }}
            >
              Añadir las marcadas ({chosen.size})
            </button>
            <button type="button" className="btn-ghost" onClick={() => setBook(null)}>
              Cancelar
            </button>
          </div>
        </fieldset>
      )}
      <datalist id={listId}>
        {accounts.map((l) => (
          <option key={l.account} value={l.account}>
            {l.name}
          </option>
        ))}
      </datalist>
      {items.length > 0 && (
        <table className="w-full">
          <caption className="sr-only">
            {customers ? 'Cobros pendientes de clientes' : 'Pagos pendientes a proveedores'}
          </caption>
          <thead>
            <tr className="text-left text-stone-600">
              <th scope="col">{customers ? 'Cliente' : 'Proveedor'}</th>
              <th scope="col">NIF</th>
              <th scope="col">Factura</th>
              <th scope="col">Fecha</th>
              <th scope="col">Vence</th>
              <th scope="col" className="text-right">
                Pendiente
              </th>
              <th scope="col">Subcuenta</th>
              <th scope="col">
                <span className="sr-only">Quitar</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {items.map((p, i) => {
              const n = i + 1;
              return (
                // biome-ignore lint/suspicious/noArrayIndexKey: los pendientes no tienen id propio
                <tr key={i}>
                  <td>
                    <input
                      aria-label={`Nombre del pendiente ${n}`}
                      className="w-full"
                      value={p.name}
                      onChange={(e) => set(i, { name: e.target.value })}
                    />
                  </td>
                  <td>
                    <input
                      aria-label={`NIF del pendiente ${n}`}
                      className="w-28"
                      value={p.tax_id ?? ''}
                      onChange={(e) => set(i, { tax_id: e.target.value.trim() || null })}
                    />
                  </td>
                  <td>
                    <input
                      aria-label={`Factura del pendiente ${n}`}
                      className="w-28"
                      value={p.number}
                      onChange={(e) => set(i, { number: e.target.value })}
                    />
                  </td>
                  <td>
                    <input
                      type="date"
                      aria-label={`Fecha de la factura del pendiente ${n}`}
                      value={p.issue_date}
                      onChange={(e) => e.target.value && set(i, { issue_date: e.target.value })}
                    />
                  </td>
                  <td>
                    <input
                      type="date"
                      aria-label={`Vencimiento del pendiente ${n}`}
                      value={p.due_date}
                      onChange={(e) => e.target.value && set(i, { due_date: e.target.value })}
                    />
                  </td>
                  <td className="text-right">
                    <Euros
                      label={`Importe del pendiente ${n}`}
                      value={p.amount}
                      onChange={(c) => set(i, { amount: c })}
                    />
                  </td>
                  <td>
                    <input
                      aria-label={`Subcuenta del pendiente ${n}`}
                      className="w-28"
                      list={listId}
                      placeholder="nueva"
                      value={p.account ?? ''}
                      onChange={(e) => set(i, { account: e.target.value.trim() || null })}
                    />
                  </td>
                  <td>
                    <button
                      type="button"
                      className="text-stone-700 underline"
                      aria-label={`Quitar el pendiente ${n}`}
                      onClick={() => update({ [key]: items.filter((_, j) => j !== i) })}
                    >
                      Quitar
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      <p>
        Total pendiente: {euros(total)} en {plural(items.length, 'factura')}.
      </p>
    </div>
  );
}

/** Tipos de bien con su coeficiente máximo de la tabla del art. 12.1.a LIS. */
const ASSET_TYPES: { account: string; label: string; rate: number }[] = [
  { account: '217', label: 'Equipos informáticos', rate: 25 },
  { account: '216', label: 'Mobiliario', rate: 10 },
  { account: '218', label: 'Elementos de transporte', rate: 16 },
  { account: '213', label: 'Maquinaria', rate: 12 },
  { account: '206', label: 'Aplicaciones informáticas', rate: 33 },
  { account: '211', label: 'Construcciones', rate: 3 },
];

/** Cuentas de amortización y gasto que van con la del bien (la 28x del balance, si la trae). */
function companions(account: string, lines: OpeningLine[]) {
  const base = `28${account.slice(1, 3)}`;
  const inBalance = lines.find((l) => l.account.startsWith(base))?.account;
  return {
    depreciation_account: inBalance ?? base,
    expense_account: account.startsWith('20') ? '680' : '681',
  };
}

function AssetsStep({ draft, update }: StepProps) {
  const assets = draft.assets;
  const listId = useId();
  const [notes, setNotes] = useState<string[]>([]);
  async function importInventory() {
    setNotes([]);
    const f = await pickOpeningFile();
    if (!f) return;
    try {
      const r = await aperturaReadInventory(f);
      update({ assets: [...assets, ...r.assets] });
      setNotes([
        `${plural(r.assets.length, 'bien añadido', 'bienes añadidos')} del inventario.`,
        ...r.warnings,
      ]);
    } catch (e) {
      setNotes([errorMessage(e)]);
    }
  }
  const set = (i: number, patch: Partial<OpeningAsset>) =>
    update({ assets: assets.map((a, j) => (j === i ? { ...a, ...patch } : a)) });
  const until = draft.date ? formatDate(dayBefore(draft.date)) : 'el día anterior a la apertura';
  const fixed = draft.lines.filter((l) => /^2[012]/.test(l.account));
  return (
    <div className="mt-2 space-y-3">
      <p className="max-w-3xl text-stone-700">
        Los bienes que la empresa ya tenía y se siguen amortizando (ordenadores, muebles, vehículos,
        maquinaria…). Pon lo que costaron y lo que ya se amortizó hasta el {until}: lo encuentras en
        el plan de amortización del gestor o en las cuentas 281 del balance. TaxPain seguirá
        amortizando desde ahí, sin volver a empezar y sin pasar nunca del precio de compra.
      </p>
      <datalist id={listId}>
        {[
          ...ASSET_TYPES.map((t) => [t.account, t.label]),
          ...fixed.map((l) => [l.account, l.name]),
        ].map(([a, n]) => (
          <option key={a} value={a}>
            {n}
          </option>
        ))}
      </datalist>
      <button
        type="button"
        className="rounded border border-stone-300 px-3 py-1"
        onClick={() =>
          update({
            assets: [
              ...assets,
              {
                name: '',
                account: '217',
                ...companions('217', draft.lines),
                acquired_on: draft.date ? dayBefore(draft.date) : '',
                cost: 0,
                rate: 2500,
                method: 'lineal',
                accumulated: 0,
              },
            ],
          })
        }
      >
        Añadir un bien
      </button>
      <button
        type="button"
        className="ml-2 rounded border border-stone-300 px-3 py-1"
        onClick={importInventory}
      >
        Importar el inventario del gestor…
      </button>
      <p className="text-stone-600">
        El «Inventario de activos» de a3 (o un listado con Descripción, Valor de adquisición, Valor
        contable y Fecha de inicio de amortización): cada bien entra con lo ya amortizado
        (adquisición − valor contable) y un coeficiente propuesto por su cuenta, que puedes cambiar.
      </p>
      <Notes items={notes} />
      {assets.length > 0 && (
        <table className="w-full">
          <caption className="sr-only">Inmovilizado de antes de TaxPain</caption>
          <thead>
            <tr className="text-left text-stone-600">
              <th scope="col">Bien</th>
              <th scope="col">Cuenta</th>
              <th scope="col">Comprado el</th>
              <th scope="col" className="text-right">
                Precio
              </th>
              <th scope="col" className="text-right">
                % anual
              </th>
              <th scope="col" className="text-right">
                Ya amortizado
              </th>
              <th scope="col">
                <span className="sr-only">Quitar</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {assets.map((a, i) => {
              const n = i + 1;
              return (
                // biome-ignore lint/suspicious/noArrayIndexKey: los bienes del borrador no tienen id
                <tr key={i}>
                  <td>
                    <input
                      aria-label={`Nombre del bien ${n}`}
                      className="w-full"
                      value={a.name}
                      onChange={(e) => set(i, { name: e.target.value })}
                    />
                  </td>
                  <td>
                    <input
                      aria-label={`Cuenta del bien ${n}`}
                      className="w-24"
                      list={listId}
                      value={a.account}
                      onChange={(e) => {
                        const account = e.target.value.trim();
                        const type = ASSET_TYPES.find((t) => t.account === account);
                        set(i, {
                          account,
                          ...companions(account, draft.lines),
                          ...(type ? { rate: type.rate * 100 } : {}),
                        });
                      }}
                    />
                    <span className="block text-xs text-stone-600">
                      amortiza en {a.depreciation_account}
                    </span>
                  </td>
                  <td>
                    <input
                      type="date"
                      aria-label={`Fecha de compra del bien ${n}`}
                      value={a.acquired_on}
                      onChange={(e) => set(i, { acquired_on: e.target.value })}
                    />
                  </td>
                  <td className="text-right">
                    <Euros
                      label={`Precio del bien ${n}`}
                      value={a.cost}
                      onChange={(c) => set(i, { cost: c })}
                    />
                  </td>
                  <td className="text-right">
                    <input
                      aria-label={`Coeficiente anual del bien ${n}`}
                      inputMode="decimal"
                      className="w-16 text-right"
                      value={String(a.rate / 100).replace('.', ',')}
                      onChange={(e) => {
                        const v = Number(e.target.value.replace(',', '.'));
                        if (v > 0 && v <= 100) set(i, { rate: Math.round(v * 100) });
                      }}
                    />
                  </td>
                  <td className="text-right">
                    <Euros
                      label={`Amortización acumulada del bien ${n}`}
                      value={a.accumulated}
                      onChange={(c) => set(i, { accumulated: c })}
                    />
                  </td>
                  <td>
                    <button
                      type="button"
                      className="text-stone-700 underline"
                      aria-label={`Quitar el bien ${n}`}
                      onClick={() => update({ assets: assets.filter((_, j) => j !== i) })}
                    >
                      Quitar
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </div>
  );
}

function TaxBankStep({ draft, update }: StepProps) {
  const [banks, setBanks] = useState<OpeningBankInfo[]>([]);
  useEffect(() => {
    aperturaBanks(draft.date)
      .then(setBanks)
      .catch(() => setBanks([]));
  }, [draft.date]);
  const tax = draft.tax;
  const balanceOfBank = (account: string) =>
    draft.banks.find((b) => b.account === account)?.balance ?? 0;
  const setBank = (account: string, name: string, balance: number) => {
    const rest = draft.banks.filter((b) => b.account !== account);
    update({ banks: balance === 0 && !name ? rest : [...rest, { account, name, balance }] });
  };
  const own = new Set(banks.map((b) => b.account));
  const extra = draft.banks.filter((b) => !own.has(b.account));
  const said = (code: string) => {
    const b = balanceOf(draft.lines, code);
    return b === null ? null : `En el balance: ${euros(Math.abs(b))}`;
  };
  const taxRows: { key: keyof typeof tax; code: string; label: string; help: string }[] = [
    {
      key: 'vat_refund',
      code: '4700',
      label: 'IVA a tu favor (4700)',
      help: 'IVA a compensar de declaraciones anteriores (casilla «a compensar» del último 303). Con la apertura del 1 de enero, TaxPain lo descontará en el 303 del 1T (casilla [110]).',
    },
    {
      key: 'vat_due',
      code: '4750',
      label: 'IVA por pagar (4750)',
      help: 'El resultado a ingresar del último 303 si aún no se ha pagado.',
    },
    {
      key: 'withholdings',
      code: '4751',
      label: 'Retenciones por pagar (4751)',
      help: 'Lo retenido a trabajadores, profesionales o en el alquiler (111, 115) que aún no se ha ingresado.',
    },
  ];
  return (
    <div className="mt-2 space-y-4">
      <fieldset>
        <legend className="font-medium">Hacienda</legend>
        <p className="max-w-3xl text-stone-700">
          Lo pendiente con Hacienda el día de la apertura. Si el balance del gestor ya lo trae,
          déjalo como está y TaxPain comprobará que coincide.
        </p>
        <div className="mt-2 space-y-2">
          {taxRows.map((r) => (
            <div key={r.key} className="flex flex-wrap items-center gap-3">
              <span className="w-56">{r.label}</span>
              <Euros
                label={r.label}
                value={tax[r.key]}
                onChange={(c) => update({ tax: { ...tax, [r.key]: c } })}
              />
              <span className="text-stone-600">
                {said(r.code) ?? r.help}
                {r.code === '4700' && said(r.code) !== null
                  ? ' Con la apertura del 1 de enero, pasará al 303 del 1T (casilla [110]).'
                  : ''}
              </span>
            </div>
          ))}
        </div>
      </fieldset>
      <fieldset>
        <legend className="font-medium">Bancos y caja</legend>
        <p className="max-w-3xl text-stone-700">
          Lo que había en cada cuenta el día anterior a la apertura (en negativo si estaba en
          descubierto). Si ya importaste extractos, TaxPain te propone su saldo.
        </p>
        {banks.length === 0 && (
          <p className="mt-2 text-stone-600">
            Aún no tienes cuentas de banco en TaxPain (se crean al importar el primer extracto).
            Puedes poner aquí el saldo con su cuenta 572.
          </p>
        )}
        <ul className="mt-2 space-y-2">
          {banks.map((b) => (
            <li key={b.account} className="flex flex-wrap items-center gap-3">
              <span className="w-56">
                {b.name} ({b.account}){b.currency !== 'EUR' && ` · ${b.currency}`}
              </span>
              <Euros
                label={`Saldo de ${b.name}`}
                value={balanceOfBank(b.account)}
                onChange={(c) => setBank(b.account, b.name, c)}
              />
              {b.statement_balance !== null && b.statement_balance !== balanceOfBank(b.account) && (
                <button
                  type="button"
                  className="text-stone-700 underline"
                  onClick={() => setBank(b.account, b.name, b.statement_balance ?? 0)}
                >
                  Usar el del extracto ({euros(b.statement_balance)})
                </button>
              )}
              <span className="text-stone-600">{said(b.account)}</span>
            </li>
          ))}
          {extra.map((b) => (
            <li key={b.account} className="flex flex-wrap items-center gap-3">
              <input
                aria-label="Nombre de la cuenta"
                className="w-40"
                value={b.name}
                onChange={(e) => setBank(b.account, e.target.value, b.balance)}
              />
              <span>{b.account}</span>
              <Euros
                label={`Saldo de ${b.name || b.account}`}
                value={b.balance}
                onChange={(c) => setBank(b.account, b.name, c)}
              />
              <button
                type="button"
                className="text-stone-700 underline"
                onClick={() =>
                  update({ banks: draft.banks.filter((x) => x.account !== b.account) })
                }
              >
                Quitar
              </button>
            </li>
          ))}
        </ul>
        <OtherAccount
          onAdd={(account, name) => {
            if (!draft.banks.some((b) => b.account === account))
              update({ banks: [...draft.banks, { account, name, balance: 0 }] });
          }}
        />
      </fieldset>
    </div>
  );
}

function OtherAccount({ onAdd }: { onAdd: (account: string, name: string) => void }) {
  const [account, setAccount] = useState('570');
  const [name, setName] = useState('Caja');
  return (
    <div className="mt-3 flex flex-wrap items-end gap-3">
      <label>
        Otra cuenta (caja, banco aún no importado)
        <input
          className="mt-1 block w-28"
          inputMode="numeric"
          value={account}
          onChange={(e) => setAccount(e.target.value.trim())}
        />
      </label>
      <label>
        Nombre
        <input className="mt-1 block w-40" value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      <button
        type="button"
        className="rounded border border-stone-300 px-3 py-1"
        disabled={!/^5[57]\d+$/.test(account)}
        onClick={() => onAdd(account, name.trim())}
      >
        Añadir
      </button>
    </div>
  );
}

const FROM: Record<string, string> = {
  balance: 'Balance',
  clientes: 'Clientes',
  proveedores: 'Proveedores',
  inmovilizado: 'Inmovilizado',
  hacienda: 'Hacienda',
  bancos: 'Bancos',
};

function ReviewStep({
  draft,
  onApplied,
}: {
  draft: OpeningDraft;
  onApplied: (a: OpeningApplied) => void;
}) {
  const [s, setS] = useState<OpeningSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [again, setAgain] = useState(false);
  const check = useCallback(() => {
    aperturaSummary(draft)
      .then((r) => {
        setS(r);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
  }, [draft]);
  useEffect(check, [check]);
  if (error)
    return (
      <p role="alert" className="mt-2 text-red-700">
        {error}
      </p>
    );
  if (!s) return <p className="mt-2">Comprobando…</p>;
  const blocked = s.errors.length > 0 || (s.duplicate !== null && !again);
  return (
    <div className="mt-2 space-y-3">
      <p className="max-w-3xl text-stone-700">
        Este es el asiento de apertura que se hará
        {s.fiscal_year_label ? ` en el ejercicio ${s.fiscal_year_label}` : ''}
        {draft.date ? ` con fecha ${formatDate(draft.date)}` : ''}. Manda el balance del gestor: lo
        de los otros pasos sólo añade las cuentas que el balance no trae, y si las trae se comprueba
        que coinciden.
      </p>
      {s.errors.length > 0 && (
        <div>
          <p className="font-medium text-red-700">Antes de contabilizar, corrige esto:</p>
          <Notes items={s.errors} alert />
        </div>
      )}
      {s.warnings.length > 0 && (
        <div>
          <p className="font-medium text-amber-800">Conviene que lo mires:</p>
          <Notes items={s.warnings} />
        </div>
      )}
      {(s.also ?? []).length > 0 && (
        <div>
          <p className="font-medium">Además, al contabilizar:</p>
          <Notes items={s.also ?? []} />
        </div>
      )}
      {s.duplicate && (
        <div className="rounded-md border border-amber-300 bg-amber-50 p-3">
          <p>
            Ojo: {s.duplicate}. Otro asiento de apertura sumaría los saldos dos veces. Normalmente
            lo que toca es deshacer el anterior (arriba) o no hacer este.
          </p>
          <label className="mt-2 flex gap-2">
            <input type="checkbox" checked={again} onChange={(e) => setAgain(e.target.checked)} />
            Lo sé y quiero hacer otro asiento de apertura igualmente
          </label>
        </div>
      )}
      <table className="w-full max-w-3xl">
        <caption className="sr-only">Asiento de apertura</caption>
        <thead>
          <tr className="text-left text-stone-600">
            <th scope="col">Cuenta</th>
            <th scope="col">Nombre</th>
            <th scope="col" className="text-right">
              Debe
            </th>
            <th scope="col" className="text-right">
              Haber
            </th>
            <th scope="col">De</th>
          </tr>
        </thead>
        <tbody>
          {s.lines.map((l, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: puede haber dos líneas de una cuenta nueva
            <tr key={i}>
              <td>{l.new_account ? `${l.account}…` : l.account}</td>
              <td>{l.name}</td>
              <td className="text-right tabular-nums">{l.debit ? euros(l.debit) : ''}</td>
              <td className="text-right tabular-nums">{l.credit ? euros(l.credit) : ''}</td>
              <td>{FROM[l.from] ?? l.from}</td>
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr className="font-medium">
            <td colSpan={2}>Total</td>
            <td className="text-right tabular-nums">{euros(s.debit)}</td>
            <td className="text-right tabular-nums">{euros(s.credit)}</td>
            <td>{s.difference === 0 ? 'Cuadra' : `Diferencia ${euros(Math.abs(s.difference))}`}</td>
          </tr>
        </tfoot>
      </table>
      <div className="flex flex-wrap items-center gap-3">
        <button type="button" className="rounded border border-stone-300 px-3 py-1" onClick={check}>
          Volver a comprobar
        </button>
        {blocked ? (
          <span className="text-stone-600">
            {s.errors.length > 0
              ? 'Corrige lo de arriba para poder contabilizar.'
              : 'Marca la casilla de arriba para contabilizar otra apertura.'}
          </span>
        ) : (
          <ConfirmAction
            label="Contabilizar la apertura"
            className="rounded btn-primary px-3 py-1"
            danger={false}
            question={`Se hace el asiento de apertura (${euros(s.debit)} al debe y al haber), los pendientes entran en Cobros y pagos y los bienes en Inmovilizado. Si te equivocas, lo puedes deshacer desde aquí mientras no cobres, pagues o amortices nada. ¿Seguir?`}
            confirmLabel="Sí, contabilizar"
            onConfirm={async () => {
              onApplied(await aperturaApply(draft, again));
            }}
          />
        )}
      </div>
    </div>
  );
}

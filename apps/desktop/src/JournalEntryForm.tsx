import {
  type FormEvent,
  type KeyboardEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';
import { useAccounts } from './accounts';
import {
  type EntryLine,
  errorMessage,
  euros,
  manualEntry,
  parseEuros,
  type StoredEntry,
  type Template,
  templateDelete,
  templatePost,
  templateSave,
  templatesList,
  updateEntry,
} from './api';

interface Row {
  account: string;
  description: string;
  debit: string;
  credit: string;
}

const empty = (): Row => ({ account: '', description: '', debit: '', credit: '' });
const cents = (s: string) => (s.trim() ? parseEuros(s) : 0);
const field = 'w-full rounded border border-stone-300 px-1 py-0.5';

/** Comprueba las filas y devuelve las líneas del asiento, o el error para el usuario. */
export function toLines(rows: Row[]): EntryLine[] | string {
  const used = rows.filter((r) => r.account.trim() || r.debit.trim() || r.credit.trim());
  if (used.length < 2) return 'Un asiento necesita al menos dos líneas.';
  const lines: EntryLine[] = [];
  for (const [i, r] of used.entries()) {
    const n = i + 1;
    const account = r.account.trim();
    if (!/^\d{3,12}$/.test(account)) return `Línea ${n}: la cuenta son de 3 a 12 dígitos.`;
    const d = cents(r.debit);
    const c = cents(r.credit);
    if (d === null || c === null) return `Línea ${n}: importe no válido.`;
    if ((d === 0) === (c === 0) || d < 0 || c < 0)
      return `Línea ${n}: pon un importe positivo en el debe o en el haber (sólo en uno).`;
    lines.push({ account, debit: d, credit: c, description: r.description.trim() || null });
  }
  const sd = lines.reduce((s, l) => s + l.debit, 0);
  const sc = lines.reduce((s, l) => s + l.credit, 0);
  if (sd !== sc) return `El asiento no cuadra: debe ${euros(sd)}, haber ${euros(sc)}.`;
  return lines;
}

const fmt = (cents: number) => (cents / 100).toFixed(2).replace('.', ',');

export function localToday(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

/** Entrada de un asiento manual (como el «Mantenimiento de apuntes» de los programas de
 * contabilidad): buscar la cuenta por código o nombre, ver su nombre, cuadrar y grabar con
 * ⌘/Ctrl+Intro. Las cuentas que no existen se crean al grabar. */
export function JournalEntryForm({
  year,
  onSaved,
  start,
  editing,
  concepts = [],
  onClose,
}: {
  year: number;
  onSaved: () => void;
  /** Conceptos habituales que se proponen al escribir. */
  concepts?: string[];
  /** Cerrar el formulario (Esc). */
  onClose?: () => void;
  /** Periódico pendiente con el que abrir el formulario (y su fecha). */
  start?: Template | undefined;
  /** Asiento manual que se modifica. */
  editing?: StoredEntry | undefined;
}) {
  const accounts = useAccounts(year);
  const [date, setDate] = useState(() => {
    if (editing) return editing.entry.date;
    const t = localToday();
    return t.startsWith(String(year)) ? t : `${year}-12-31`;
  });
  const [concept, setConcept] = useState(editing?.entry.concept ?? '');
  const [rows, setRows] = useState<Row[]>(() =>
    editing
      ? editing.entry.lines.map((l) => ({
          account: l.account,
          description: l.description ?? '',
          debit: l.debit ? fmt(l.debit) : '',
          credit: l.credit ? fmt(l.credit) : '',
        }))
      : [empty(), empty()],
  );
  const [msg, setMsg] = useState<string | null>(null);
  // Los errores aparte, en rojo y con role="alert" (el lector de pantalla los anuncia).
  const [error, setError] = useState<string | null>(null);
  const say = (m: string | null) => {
    setMsg(m);
    setError(null);
  };
  const fail = (m: string) => {
    setError(m);
    setMsg(null);
  };
  // Sin dobles envíos: un doble clic o ⌘↵ dos veces no graba el asiento dos veces.
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [templates, setTemplates] = useState<Template[]>([]);
  const [templateId, setTemplateId] = useState<number | null>(null);
  const [saving, setSaving] = useState<{ name: string; every: string } | null>(null);

  const loadTemplates = useCallback(() => {
    templatesList()
      .then(setTemplates)
      .catch(() => setTemplates([]));
  }, []);
  useEffect(loadTemplates, [loadTemplates]);

  const accountName = accounts.name;
  /** Rellena el formulario con la plantilla: `#` → número, `$` → nombre de la cuenta y
   * `?` → cuenta a completar. */
  const apply = useCallback(
    (t: Template) => {
      setTemplateId(t.id);
      setConcept(t.concept.replaceAll('#', String(t.counter + 1)));
      if (t.next_date) setDate(t.next_date);
      setRows(
        t.lines.map((l) => {
          const account = l.account.replace(/\?$/, '');
          const amount = l.amount === null ? '' : fmt(l.amount);
          return {
            account,
            description: l.description.replaceAll('$', accountName(account) ?? ''),
            debit: l.debit ? amount : '',
            credit: l.debit ? '' : amount,
          };
        }),
      );
      setMsg(
        t.lines.some((l) => l.account.endsWith('?') || l.amount === null)
          ? 'Completa las subcuentas y los importes que faltan.'
          : null,
      );
    },
    [accountName],
  );
  // biome-ignore lint/correctness/useExhaustiveDependencies: sólo al abrir con un periódico
  useEffect(() => {
    if (start) apply(start);
  }, [start]);

  async function saveTemplate() {
    if (!saving) return;
    const lines = rows
      .filter((r) => r.account.trim())
      .map((r) => {
        const d = cents(r.debit) ?? 0;
        const c = cents(r.credit) ?? 0;
        // Sin importe, la línea va al lado que cuadra el asiento (la contrapartida).
        const debit = d || c ? c === 0 : totals.d < totals.c;
        return {
          account: r.account.trim(),
          description: r.description.trim(),
          debit,
          amount: d || c || null,
        };
      });
    const every = Number(saving.every) || null;
    try {
      await templateSave({
        id: 0,
        name: saving.name,
        concept,
        lines,
        every_months: every,
        next_date: every ? date : null,
        counter: 0,
      });
      say(`Plantilla «${saving.name.trim()}» guardada.`);
      setSaving(null);
      loadTemplates();
    } catch (e) {
      fail(errorMessage(e));
    }
  }

  const totals = rows.reduce(
    (t, r) => ({ d: t.d + (cents(r.debit) ?? 0), c: t.c + (cents(r.credit) ?? 0) }),
    { d: 0, c: 0 },
  );
  const diff = totals.d - totals.c;

  const set = (i: number, patch: Partial<Row>) =>
    setRows((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));

  /** Pone en la línea `i` lo que falta para cuadrar. */
  const balance = (i: number) => {
    const r = rows[i];
    if (!r) return;
    const others = diff - ((cents(r.debit) ?? 0) - (cents(r.credit) ?? 0));
    const amount = (Math.abs(others) / 100).toFixed(2).replace('.', ',');
    set(i, others > 0 ? { debit: '', credit: amount } : { debit: amount, credit: '' });
  };

  async function save(e?: FormEvent) {
    e?.preventDefault();
    if (busyRef.current) return;
    if (!concept.trim()) {
      fail('Pon el concepto del asiento.');
      return;
    }
    const lines = toLines(rows);
    if (typeof lines === 'string') {
      fail(lines);
      return;
    }
    busyRef.current = true;
    setBusy(true);
    try {
      const entry = { date, concept: concept.trim(), origin: 'Manual' as const, lines };
      if (editing) {
        await updateEntry(editing.id, entry);
        say(`Asiento nº ${editing.number} modificado.`);
        onSaved();
        return;
      }
      if (templateId !== null) await templatePost(templateId, entry);
      else await manualEntry(entry);
      say(`Asiento «${concept.trim()}» registrado.`);
      setConcept('');
      setRows([empty(), empty()]);
      setTemplateId(null);
      loadTemplates();
      accounts.reload();
      onSaved();
    } catch (err) {
      fail(errorMessage(err));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }

  const [counterpart, setCounterpart] = useState('');

  /** Añade la contrapartida: una línea con `counterpart` por lo que falta para cuadrar. */
  const addCounterpart = () => {
    const code = counterpart.trim();
    if (!/^\d{3,12}$/.test(code) || diff === 0) return;
    const amount = (Math.abs(diff) / 100).toFixed(2).replace('.', ',');
    const line = {
      ...empty(),
      account: code,
      ...(diff > 0 ? { credit: amount } : { debit: amount }),
    };
    setRows((rs) => {
      const blank = rs.findIndex((r) => !r.account.trim() && !r.debit && !r.credit);
      return blank >= 0 ? rs.map((r, j) => (j === blank ? line : r)) : [...rs, line];
    });
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      save();
      return;
    }
    if (e.key === 'Escape' && onClose) {
      e.preventDefault();
      onClose();
      return;
    }
    // Intro sin modificadores pasa al campo siguiente (como en a3) en vez de grabar; en el
    // último campo de las líneas añade una línea nueva.
    const t = e.target as HTMLElement;
    if (e.key === 'Enter' && !e.shiftKey && t.tagName === 'INPUT') {
      e.preventDefault();
      const form = e.currentTarget as HTMLFormElement;
      const inputs = [...form.querySelectorAll<HTMLInputElement>('tbody input')];
      const i = inputs.indexOf(t as HTMLInputElement);
      if (i >= 0 && i === inputs.length - 1) {
        setRows((rs) => [...rs, empty()]);
        setTimeout(() => {
          const next = form.querySelectorAll<HTMLInputElement>('tbody input');
          next[inputs.length]?.focus();
        }, 0);
      } else if (i >= 0) {
        inputs[i + 1]?.focus();
      } else {
        const all = [...form.querySelectorAll<HTMLInputElement>('input')];
        all[all.indexOf(t as HTMLInputElement) + 1]?.focus();
      }
    }
  };

  return (
    <form
      onSubmit={save}
      onKeyDown={onKey}
      aria-label="Nuevo asiento"
      className="mt-4 rounded-md border border-stone-300 bg-white p-3 text-sm"
    >
      <datalist id="cuentas">
        {accounts.plan.map((a) => (
          <option key={a.code} value={a.code}>
            {a.name}
          </option>
        ))}
        {accounts.pgc
          .filter(([c]) => c.length >= 3)
          .map(([c, n]) => (
            <option key={`pgc-${c}`} value={c}>
              {n}
            </option>
          ))}
      </datalist>
      {editing && <p className="mb-2 font-medium">Modificar el asiento nº {editing.number}</p>}
      <div className="flex flex-wrap gap-3">
        <label hidden={!!editing}>
          Plantilla
          <select
            className={`${field} mt-1 block`}
            value={templateId ?? ''}
            onChange={(e) => {
              const t = templates.find((x) => x.id === Number(e.target.value));
              if (t) apply(t);
              else setTemplateId(null);
            }}
          >
            <option value="">Ninguna</option>
            {templates.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
                {t.every_months
                  ? ` (cada ${t.every_months} mes${t.every_months > 1 ? 'es' : ''})`
                  : ''}
              </option>
            ))}
          </select>
        </label>
        <label>
          Fecha
          <input
            type="date"
            className={`${field} mt-1 block`}
            value={date}
            onChange={(e) => setDate(e.target.value)}
            required
          />
        </label>
        <label className="grow">
          Concepto
          <input
            className={`${field} mt-1 block`}
            list="conceptos"
            value={concept}
            onChange={(e) => setConcept(e.target.value)}
          />
          <datalist id="conceptos">
            {concepts.map((c) => (
              <option key={c} value={c} />
            ))}
          </datalist>
        </label>
      </div>
      <table className="mt-3 w-full">
        <thead className="text-left text-stone-500">
          <tr>
            <th className="w-40">Cuenta</th>
            <th>Descripción</th>
            <th className="w-28 text-right">Debe</th>
            <th className="w-28 text-right">Haber</th>
            <th className="w-16" />
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => {
            const code = r.account.trim();
            const name = /^\d{3,12}$/.test(code) ? accounts.name(code) : undefined;
            return (
              // biome-ignore lint/suspicious/noArrayIndexKey: las filas no tienen id propio
              <tr key={i} className="align-top">
                <td>
                  <input
                    aria-label={`Cuenta ${i + 1}`}
                    list="cuentas"
                    inputMode="numeric"
                    className={`${field} font-mono`}
                    value={r.account}
                    onChange={(e) => set(i, { account: e.target.value })}
                  />
                  {code && (
                    <span className="block text-xs text-stone-500">
                      {name ?? 'Cuenta nueva'}
                      {name && !accounts.known(code) && code.length > 4 ? ' · subcuenta nueva' : ''}
                    </span>
                  )}
                </td>
                <td>
                  <input
                    aria-label={`Descripción ${i + 1}`}
                    className={field}
                    placeholder={concept}
                    value={r.description}
                    onChange={(e) => set(i, { description: e.target.value })}
                  />
                </td>
                <td>
                  <input
                    aria-label={`Debe ${i + 1}`}
                    inputMode="decimal"
                    className={`${field} text-right`}
                    value={r.debit}
                    onChange={(e) => set(i, { debit: e.target.value })}
                  />
                </td>
                <td>
                  <input
                    aria-label={`Haber ${i + 1}`}
                    inputMode="decimal"
                    className={`${field} text-right`}
                    value={r.credit}
                    onChange={(e) => set(i, { credit: e.target.value })}
                  />
                </td>
                <td className="whitespace-nowrap text-right">
                  {diff !== 0 && (
                    <button
                      type="button"
                      className="text-link"
                      title="Poner aquí lo que falta para cuadrar"
                      aria-label={`Cuadrar con la línea ${i + 1}`}
                      onClick={() => balance(i)}
                    >
                      =
                    </button>
                  )}
                  {rows.length > 2 && (
                    <button
                      type="button"
                      className="ml-2 text-stone-500"
                      aria-label={`Quitar la línea ${i + 1}`}
                      onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))}
                    >
                      ×
                    </button>
                  )}
                </td>
              </tr>
            );
          })}
          <tr className="border-t border-stone-300 font-medium">
            <td colSpan={2}>
              <button
                type="button"
                className="text-link"
                onClick={() => setRows((rs) => [...rs, empty()])}
              >
                + Línea
              </button>
            </td>
            <td className="text-right tabular-nums">{euros(totals.d)}</td>
            <td className="text-right tabular-nums">{euros(totals.c)}</td>
            <td />
          </tr>
        </tbody>
      </table>
      <div className="mt-3 flex items-center justify-between">
        <span
          role="status"
          aria-label="Descuadre"
          className={diff === 0 ? 'text-emerald-700' : 'text-red-700'}
        >
          {diff === 0 ? 'Cuadra' : `Descuadre: ${euros(Math.abs(diff))}`}
        </span>
        {diff !== 0 && (
          <span className="flex items-center gap-1">
            <input
              aria-label="Contrapartida"
              list="cuentas"
              inputMode="numeric"
              placeholder="Contrapartida"
              className={`${field} w-28 font-mono`}
              value={counterpart}
              onChange={(e) => setCounterpart(e.target.value)}
            />
            <button type="button" className="text-link" onClick={addCounterpart}>
              Cuadrar contra ella
            </button>
          </span>
        )}
        <span className="flex gap-2">
          <button
            type="button"
            className="rounded-md border border-stone-300 px-3 py-1.5"
            onClick={() => setSaving(saving ? null : { name: '', every: '' })}
          >
            Guardar como plantilla…
          </button>
          <button
            type="submit"
            disabled={busy}
            className="rounded-md btn-primary px-3 py-1.5 disabled:opacity-50"
          >
            {editing ? 'Guardar cambios (⌘↵)' : 'Registrar asiento (⌘↵)'}
          </button>
        </span>
      </div>
      {saving && (
        <fieldset
          className="mt-3 flex flex-wrap items-end gap-3 border-t border-stone-200 pt-3"
          aria-label="Guardar plantilla"
        >
          <label>
            Nombre de la plantilla
            <input
              className={`${field} mt-1 block`}
              value={saving.name}
              onChange={(e) => setSaving({ ...saving, name: e.target.value })}
            />
          </label>
          <label>
            Repetir
            <select
              className={`${field} mt-1 block`}
              value={saving.every}
              onChange={(e) => setSaving({ ...saving, every: e.target.value })}
            >
              <option value="">No (sólo plantilla)</option>
              <option value="1">Cada mes</option>
              <option value="3">Cada trimestre</option>
              <option value="12">Cada año</option>
            </select>
          </label>
          <button type="button" className="rounded-md border px-3 py-1.5" onClick={saveTemplate}>
            Guardar plantilla
          </button>
          <p className="w-full text-xs text-stone-500">
            Las líneas sin importe se piden al usarla; «#» en el concepto es el número y «$» en la
            descripción, el nombre de la cuenta.{saving.every && ` Primera fecha: ${date}.`}
          </p>
        </fieldset>
      )}
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}
      {templates.length > 0 && (
        <details className="mt-3">
          <summary className="cursor-pointer text-stone-600">
            Plantillas ({templates.length})
          </summary>
          <ul className="mt-1">
            {templates.map((t) => (
              <li key={t.id} className="flex justify-between py-0.5">
                <span>
                  {t.name}
                  {t.next_date && <span className="text-stone-500"> · próxima {t.next_date}</span>}
                </span>
                <button
                  type="button"
                  className="text-red-800"
                  aria-label={`Borrar la plantilla ${t.name}`}
                  onClick={() =>
                    templateDelete(t.id)
                      .then(loadTemplates)
                      .catch((e) => fail(errorMessage(e)))
                  }
                >
                  Borrar
                </button>
              </li>
            ))}
          </ul>
        </details>
      )}
    </form>
  );
}

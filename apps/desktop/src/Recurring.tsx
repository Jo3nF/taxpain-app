import { Repeat } from 'lucide-react';
import { type FormEvent, useEffect, useState } from 'react';
import {
  errorMessage,
  everyLabel,
  formatDate,
  type InvoiceTemplate,
  type LateVat,
  money,
  parseEuros,
  type RecurringPreview,
  recurringDelete,
  recurringIssue,
  recurringPreview,
  recurringUpdate,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { CurrencyRate, type RateValue, rateValueOf, toForeignCurrency } from './CurrencyRate';

const input = 'mt-1 block w-full rounded-md border border-stone-300 px-2 py-1';

/** «1234,56» a partir de céntimos, para editar. */
function priceText(cents: number): string {
  return (cents / 100).toFixed(2).replace('.', ',');
}

function Edit({
  t,
  onDone,
  onCancel,
}: {
  t: InvoiceTemplate;
  onDone: () => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(t.name);
  const [every, setEvery] = useState(t.every_months);
  // Vacía si ya terminó. Sólo se manda si se cambia: si no, la serie sigue donde iba (cambiar
  // el nombre de una terminada no vuelve a poner por emitir lo ya emitido).
  const [next, setNext] = useState(t.next_date ?? '');
  const [end, setEnd] = useState(t.end ?? '');
  const [active, setActive] = useState(t.active);
  const [lines, setLines] = useState(
    t.draft.lines.map((l) => ({ description: l.description, price: priceText(l.unit_price) })),
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    const prices = lines.map((l) => parseEuros(l.price));
    if (prices.some((p) => p === null)) {
      setError('Revisa los precios.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await recurringUpdate(t.id, {
        name,
        every_months: every,
        next_date: next && next !== (t.next_date ?? '') ? next : null,
        end: end || null,
        active,
        line_descriptions: lines.map((l) => l.description),
        line_prices: prices as number[],
      });
      onDone();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} className="mt-3 space-y-3 rounded-lg border border-stone-200 p-4">
      <div className="form-grid">
        <label className="span-2">
          Nombre
          <input className={input} value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label>
          Frecuencia
          <select
            className={input}
            value={every}
            onChange={(e) => setEvery(Number(e.target.value))}
          >
            {[1, 2, 3, 6, 12].map((n) => (
              <option key={n} value={n}>
                {everyLabel(n)}
              </option>
            ))}
          </select>
        </label>
        <label>
          Siguiente
          <input
            type="date"
            className={input}
            value={next}
            onChange={(e) => setNext(e.target.value)}
          />
          {!t.next_date && (
            <span className="block text-xs text-stone-500">
              Terminada: pon una fecha sólo para volver a empezar.
            </span>
          )}
        </label>
        <label>
          Hasta (opcional)
          <input
            type="date"
            className={input}
            value={end}
            onChange={(e) => setEnd(e.target.value)}
          />
        </label>
        <label>
          <input
            type="checkbox"
            role="switch"
            aria-checked={active}
            checked={active}
            onChange={(e) => setActive(e.target.checked)}
          />
          Activa
        </label>
      </div>
      <table className="w-full">
        <thead>
          <tr className="text-left text-stone-500">
            <th className="font-normal">Concepto</th>
            <th className="w-32 font-normal">Precio</th>
          </tr>
        </thead>
        <tbody>
          {lines.map((l, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: las líneas no tienen id propio
            <tr key={i}>
              <td className="pr-2">
                <input
                  aria-label={`Concepto ${i + 1}`}
                  className={input}
                  value={l.description}
                  onChange={(e) =>
                    setLines(
                      lines.map((x, j) => (j === i ? { ...x, description: e.target.value } : x)),
                    )
                  }
                />
              </td>
              <td>
                <input
                  aria-label={`Precio ${i + 1}`}
                  className={input}
                  value={l.price}
                  onChange={(e) =>
                    setLines(lines.map((x, j) => (j === i ? { ...x, price: e.target.value } : x)))
                  }
                />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="text-stone-600">
        En los conceptos, <code>{'{mes}'}</code>, <code>{'{trimestre}'}</code> y{' '}
        <code>{'{año}'}</code> se cambian por los del periodo («Mantenimiento web {'{mes}'}» →
        «Mantenimiento web octubre 2026»).
      </p>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      <div className="form-actions">
        <button
          type="submit"
          disabled={busy}
          className="btn-primary rounded-md px-4 py-2 disabled:opacity-50"
        >
          Guardar
        </button>
        <button type="button" className="btn-ghost" onClick={onCancel}>
          Cancelar
        </button>
      </div>
    </form>
  );
}

function IssuePanel({
  t,
  onIssued,
  onCancel,
}: {
  t: InvoiceTemplate;
  onIssued: (msg: string) => void;
  onCancel: () => void;
}) {
  const [preview, setPreview] = useState<RecurringPreview | null>(null);
  /** Factura de un periodo de IVA ya cerrado: cómo emitirla (lo decide el usuario). */
  const [late, setLate] = useState<LateVat | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** Plantilla en moneda extranjera: el tipo del día de la operación (el guardado es el de la
   * anterior). */
  const [fx, setFx] = useState<RateValue | null>(null);
  useEffect(() => {
    let cancelled = false;
    recurringPreview(t.id)
      .then((p) => {
        if (cancelled) return;
        setPreview(p);
        if (p.draft.currency) setFx(rateValueOf(p.draft.currency));
      })
      .catch((e) => {
        if (!cancelled) setError(errorMessage(e));
      });
    return () => {
      cancelled = true;
    };
  }, [t.id]);

  if (error) {
    return (
      <p role="alert" className="mt-2 text-red-700">
        {error}
      </p>
    );
  }
  if (!preview) return null;
  const d = preview.draft;
  const code = d.currency?.code ?? null;
  // Sin la fecha de la operación, el devengo (y el tipo de cambio) es el de la emisión.
  const operation = late === 'today' ? null : d.operation_date;
  const blocked = Boolean(preview.late_vat) && late === null;
  return (
    <div className="mt-2 rounded-md border border-stone-200 bg-stone-50 p-3">
      <p>
        Factura a <strong>{d.recipient?.name}</strong> del {formatDate(d.issue_date)}
        {operation && ` (operación del ${formatDate(operation)})`} por{' '}
        <strong>{money(preview.totals.invoice_total, code)}</strong>:
      </p>
      <ul className="mt-1 list-disc pl-5 text-stone-700">
        {d.lines.map((l, i) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: las líneas no tienen id propio
          <li key={i}>
            {l.description} · {money(l.unit_price, code)}
          </li>
        ))}
      </ul>
      {preview.late_vat && d.operation_date && (
        <fieldset className="mt-2 rounded-md border border-amber-300 bg-amber-50 p-3 text-amber-950">
          <legend className="px-1 font-medium">Factura de un periodo atrasado</legend>
          <p>
            La operación es del {formatDate(d.operation_date)}: {preview.late_vat}. ¿Cómo la emites?
          </p>
          <label className="mt-2">
            <input
              type="radio"
              name={`late-${t.id}`}
              checked={late === 'period'}
              onChange={() => setLate('period')}
            />
            <span>
              Con la fecha de la operación ({formatDate(d.operation_date)}): su IVA es de aquel
              periodo y habrá que corregir aquel 303 (autoliquidación rectificativa, con recargo si
              sale a ingresar).
            </span>
          </label>
          <label className="mt-2">
            <input
              type="radio"
              name={`late-${t.id}`}
              checked={late === 'today'}
              onChange={() => setLate('today')}
            />
            <span>
              Sin fecha de la operación: su IVA va al periodo actual. Sólo si el cobro no era
              exigible hasta ahora (art. 75.Uno.7.º LIVA).
            </span>
          </label>
        </fieldset>
      )}
      {code && fx && (
        <div className="form-grid mt-3">
          <CurrencyRate
            code={code}
            value={fx}
            accrual={operation ?? d.issue_date}
            onChange={setFx}
            inputClass={input}
          />
        </div>
      )}
      <div className="mt-3 form-actions">
        {blocked ? (
          <button
            type="button"
            disabled
            title="Elige antes cómo emitirla"
            className="btn-primary rounded-md px-3 py-1.5 opacity-50"
          >
            Emitir esta factura
          </button>
        ) : (
          <ConfirmAction
            label="Emitir esta factura"
            question="Se numera, se registra en VERI*FACTU y no se puede borrar. ¿Emitirla?"
            confirmLabel="Sí, emitir"
            danger={false}
            className="btn-primary rounded-md px-3 py-1.5"
            onConfirm={async () => {
              let currency = null;
              if (code && fx) {
                const c = toForeignCurrency(code, fx);
                if (typeof c === 'string') throw new Error(c);
                currency = c;
              }
              const out = await recurringIssue(t.id, true, currency, late);
              onIssued(`Emitida ${out.number}.`);
            }}
          />
        )}
        <button type="button" className="btn-ghost" onClick={onCancel}>
          Cancelar
        </button>
      </div>
    </div>
  );
}

/** Facturas periódicas (cuotas, mantenimiento…): se emiten a mano cuando tocan. */
export function Recurring({
  templates,
  onChanged,
  onMessage,
}: {
  templates: InvoiceTemplate[];
  onChanged: () => void;
  onMessage: (m: string) => void;
}) {
  const [editing, setEditing] = useState<string | null>(null);
  const [issuing, setIssuing] = useState<string | null>(null);
  if (templates.length === 0) return null;
  const due = templates.filter((t) => t.due).length;
  return (
    <section aria-labelledby="recurring-title" className="card mt-4 p-4 text-sm">
      <h3 id="recurring-title" className="flex items-center gap-2 font-semibold">
        <Repeat size={16} aria-hidden="true" className="text-stone-500" />
        Facturas periódicas
        {due > 0 && (
          <span className="pill pill-amber">{due === 1 ? '1 toca' : `${due} tocan`}</span>
        )}
      </h3>
      <ul className="mt-2 divide-y divide-stone-100">
        {templates.map((t) => (
          <li key={t.id} className="py-2">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="min-w-0">
                <p className="truncate font-medium">{t.name}</p>
                <p className="text-stone-500">
                  {everyLabel(t.every_months)} ·{' '}
                  {!t.active
                    ? 'en pausa'
                    : t.next_date
                      ? `siguiente: ${formatDate(t.next_date)}`
                      : 'terminada'}
                  {t.last && ` · última: ${t.last[0]}`}
                </p>
              </div>
              <div className="flex shrink-0 items-center gap-3">
                {t.due && (
                  <button
                    type="button"
                    className="text-link"
                    aria-expanded={issuing === t.id}
                    onClick={() => {
                      setIssuing(issuing === t.id ? null : t.id);
                      setEditing(null);
                    }}
                  >
                    Emitir…
                  </button>
                )}
                <button
                  type="button"
                  className="text-link"
                  onClick={() => setEditing(editing === t.id ? null : t.id)}
                >
                  Editar
                </button>
                <ConfirmAction
                  label="Borrar"
                  question="¿Borrar la plantilla? Las facturas ya emitidas no se tocan."
                  confirmLabel="Sí, borrar"
                  onConfirm={async () => {
                    await recurringDelete(t.id);
                    onChanged();
                  }}
                />
              </div>
            </div>
            {issuing === t.id && (
              <IssuePanel
                t={t}
                onIssued={(m) => {
                  setIssuing(null);
                  onMessage(m);
                  onChanged();
                }}
                onCancel={() => setIssuing(null)}
              />
            )}
            {editing === t.id && (
              <Edit
                t={t}
                onDone={() => {
                  setEditing(null);
                  onChanged();
                }}
                onCancel={() => setEditing(null)}
              />
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

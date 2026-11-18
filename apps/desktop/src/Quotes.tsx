import { ClipboardList, Plus } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import {
  errorMessage,
  euros,
  formatDate,
  hayFacturacion,
  type InvoiceDraft,
  type InvoicingSettings,
  money,
  plural,
  type Quote,
  type QuoteStatus,
  quoteDelete,
  quoteEmailCompose,
  quoteEmailOpen,
  quoteInvoiceDraft,
  quoteOpen,
  quoteSave,
  quoteStatus,
  quotesList,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { YearStepper } from './Controls';
import { EmailPanel } from './Email';
import { NewInvoice } from './Invoices';

const STATUS: Record<QuoteStatus, { label: string; pill: string }> = {
  borrador: { label: 'Borrador', pill: 'pill-stone' },
  enviado: { label: 'Enviado', pill: 'pill-brand' },
  aceptado: { label: 'Aceptado', pill: 'pill-green' },
  rechazado: { label: 'Rechazado', pill: 'pill-red' },
  facturado: { label: 'Facturado', pill: 'pill-green' },
};

type Editing =
  | { kind: 'presupuesto' | 'proforma'; quote: Quote | null }
  | { invoice: Quote; draft: InvoiceDraft; language: 'Es' | 'En' | null };

/** Presupuestos y facturas proforma: se envían, se aceptan y se convierten en factura. */
export function Quotes({ settings }: { settings: InvoicingSettings }) {
  const [year, setYear] = useState(new Date().getFullYear());
  const [list, setList] = useState<Quote[] | null>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [emailing, setEmailing] = useState<Quote | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const load = useCallback(() => {
    quotesList(year)
      .then(setList)
      .catch((e) => setMessage(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);

  const accepted = (list ?? []).filter((q) => q.status === 'aceptado');
  const fail = (e: unknown) => setMessage(errorMessage(e));

  return (
    <section className="mt-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-lg font-semibold">Presupuestos {year}</h2>
        <div className="flex flex-wrap items-center gap-2 text-sm">
          <YearStepper year={year} onChange={setYear} />
          <button
            type="button"
            className="btn-primary inline-flex items-center gap-1.5 rounded px-3"
            onClick={() => setEditing({ kind: 'presupuesto', quote: null })}
          >
            <Plus size={16} aria-hidden="true" />
            Nuevo presupuesto
          </button>
          <button
            type="button"
            className="rounded border border-stone-300 px-3"
            onClick={() => setEditing({ kind: 'proforma', quote: null })}
          >
            Nueva proforma
          </button>
        </div>
      </div>
      <p className="mt-1 text-sm text-stone-600">
        No son facturas: no llevan VERI*FACTU ni cuentan en la contabilidad ni en los modelos. Al
        aceptarlos, «Facturar» abre la factura con sus datos para revisarla y emitirla. Si esa
        factura se anula, el presupuesto vuelve a «Aceptado».
      </p>
      {accepted.length > 0 && (
        <p className="mt-3 rounded-md bg-emerald-50 p-3 text-sm text-emerald-900">
          {plural(accepted.length, 'presupuesto aceptado', 'presupuestos aceptados')} por facturar:{' '}
          {euros(accepted.reduce((s, q) => s + (q.payable ?? q.total), 0))}.
        </p>
      )}
      {message && (
        <p role="status" className="mt-3 text-sm text-stone-700">
          {message}
        </p>
      )}

      {editing && 'kind' in editing && (
        <NewInvoice
          key={editing.quote?.id ?? `nuevo-${editing.kind}`}
          settings={settings}
          initial={editing.quote?.draft}
          initialLanguage={editing.quote?.language}
          quote={{
            kind: editing.kind,
            id: editing.quote?.id ?? null,
            validUntil: editing.quote?.valid_until ?? null,
            onSaved: (q) => {
              setEditing(null);
              setMessage(`${q.number} guardado.`);
              load();
            },
          }}
          onIssued={() => {}}
          onCancel={() => setEditing(null)}
        />
      )}
      {editing && 'invoice' in editing && (
        <>
          <p className="mt-4 text-sm font-medium">
            Factura de {editing.invoice.number}: revisa los datos (el NIF del cliente es
            obligatorio) y emítela.
          </p>
          <NewInvoice
            key={`factura-${editing.invoice.id}`}
            settings={settings}
            initial={editing.draft}
            initialLanguage={editing.language}
            fromQuote={editing.invoice.id}
            onIssued={(warning) => {
              setEditing(null);
              setMessage(warning ?? `Factura emitida desde ${editing.invoice.number}.`);
              load();
            }}
            onCancel={() => setEditing(null)}
          />
        </>
      )}
      {emailing && (
        <EmailPanel
          key={emailing.id}
          title={`Enviar ${emailing.number} por correo`}
          compose={() => quoteEmailCompose(emailing.id)}
          open={(to, subject, body) => quoteEmailOpen(emailing.id, to, subject, body)}
          doneMessage={`Correo de ${emailing.number} abierto en Mail: revísalo y envíalo.`}
          onClose={() => setEmailing(null)}
          onDone={(m) => {
            setEmailing(null);
            setMessage(m);
            load();
          }}
        />
      )}

      {list && list.length === 0 && !editing && (
        <div className="card mt-4 flex flex-col items-center gap-2 p-10 text-center text-sm text-stone-500">
          <ClipboardList size={28} aria-hidden="true" />
          Aún no hay presupuestos en {year}.
        </div>
      )}
      <ul className="mt-3 divide-y divide-stone-200">
        {(list ?? []).map((q) => {
          const st = STATUS[q.status];
          const open = q.status !== 'facturado';
          return (
            <li
              key={q.id}
              className="flex flex-wrap items-center justify-between gap-3 py-3 text-sm"
            >
              <div className="min-w-0 flex-1">
                <p className="truncate">
                  <span className="font-medium">{q.number}</span>
                  {q.kind === 'proforma' && <span className="text-stone-500"> · proforma</span>} ·{' '}
                  {q.recipient_name}
                </p>
                <p className="flex flex-wrap items-center gap-2 text-stone-500">
                  {formatDate(q.issue_date)}
                  {q.valid_until && ` · válido hasta el ${formatDate(q.valid_until)}`}
                  <span className={`pill ${q.expired ? 'pill-amber' : st.pill}`}>
                    {q.expired ? 'Caducado' : st.label}
                    {q.invoice_number && ` · ${q.invoice_number}`}
                  </span>
                </p>
              </div>
              <div className="flex shrink-0 flex-wrap items-center gap-3">
                <span className="text-right tabular-nums">
                  {/* El total a pagar (sin la retención), como el PDF. */}
                  {q.draft.currency && q.currency_total != null ? (
                    <>
                      <span className="block font-semibold">
                        {money(q.currency_payable ?? q.currency_total, q.draft.currency.code)}
                      </span>
                      <span className="block text-xs text-stone-500">
                        ≈ {euros(q.payable ?? q.total)}
                      </span>
                    </>
                  ) : (
                    <span className="font-semibold">{euros(q.payable ?? q.total)}</span>
                  )}
                </span>
                <button
                  type="button"
                  className="text-link"
                  onClick={() => quoteOpen(q.id).catch(fail)}
                >
                  PDF
                </button>
                <button
                  type="button"
                  className="text-link"
                  onClick={() =>
                    quoteSave(q.id, q.number)
                      .then((ok) => ok && setMessage(`${q.number} guardado.`))
                      .catch(fail)
                  }
                >
                  Guardar
                </button>
                <button type="button" className="text-link" onClick={() => setEmailing(q)}>
                  Correo
                </button>
                {open && (
                  <select
                    aria-label={`Estado de ${q.number}`}
                    className="w-32 rounded border border-stone-300 px-1 py-0.5"
                    value={q.status}
                    onChange={(e) =>
                      quoteStatus(q.id, e.target.value as QuoteStatus)
                        .then(load)
                        .catch(fail)
                    }
                  >
                    {(['borrador', 'enviado', 'aceptado', 'rechazado'] as QuoteStatus[]).map(
                      (s) => (
                        <option key={s} value={s}>
                          {STATUS[s].label}
                        </option>
                      ),
                    )}
                  </select>
                )}
                {open && (
                  <button
                    type="button"
                    className="text-link"
                    onClick={() => setEditing({ kind: q.kind, quote: q })}
                  >
                    Editar
                  </button>
                )}
                {hayFacturacion() && open && q.status !== 'rechazado' && (
                  <button
                    type="button"
                    className="text-link font-medium"
                    onClick={() =>
                      quoteInvoiceDraft(q.id)
                        .then((r) =>
                          setEditing({ invoice: q, draft: r.draft, language: r.language }),
                        )
                        .catch(fail)
                    }
                  >
                    Facturar
                  </button>
                )}
                {/* Sólo los borradores: uno enviado conserva su número (si no sigue, se rechaza). */}
                {q.status === 'borrador' && (
                  <ConfirmAction
                    label="Borrar"
                    question={`¿Borrar ${q.number}? No es una factura: no deja rastro fiscal.`}
                    confirmLabel="Sí, borrar"
                    onConfirm={async () => {
                      await quoteDelete(q.id);
                      load();
                    }}
                  />
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

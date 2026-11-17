import { useCallback, useEffect, useState } from 'react';
import {
  carteraList,
  type DueItem,
  errorMessage,
  euros,
  formatDate,
  KEYS_190,
  type Key190,
  PAY_METHODS,
  type PartyRow,
  type PartyTerms,
  type PayMethod,
  type PaymentTerms,
  parseEuros,
  partyTermsList,
  paymentTerms,
  setDueDate,
  setInstallments,
  setPartyCounterpart,
  setPartyTerms,
  setPaymentTerms,
  settleInvoice,
  unsettleInvoice,
} from './api';
import { CarteraCheck } from './CarteraCheck';
import { ConfirmAction } from './ConfirmAction';
import { InvoiceEmailPanel } from './Email';
import { Incobrables } from './Incobrables';
import { localToday } from './JournalEntryForm';
import { Remesas } from './Remesas';

type Filter = 'pendientes' | 'vencidas' | 'todas';

const small = 'rounded border border-stone-300 px-1 py-0.5';
const plain = (cents: number) => (cents / 100).toFixed(2).replace('.', ',');
const label = (i: DueItem) =>
  i.installments > 1 ? `${i.number} (plazo ${i.seq}/${i.installments})` : i.number;

function addDays(iso: string, days: number) {
  const d = new Date(`${iso}T12:00:00Z`);
  d.setUTCDate(d.getUTCDate() + days);
  return d.toISOString().slice(0, 10);
}

function Status({ i }: { i: DueItem }) {
  if (i.status === 'pendiente')
    return i.overdue_days > 0 ? (
      <span className="text-red-700">Vencida hace {i.overdue_days} d</span>
    ) : (
      <span className="text-stone-600">Pendiente</span>
    );
  const verb = i.kind === 'emitida' ? 'Cobrada' : 'Pagada';
  return (
    <span className="text-emerald-700">
      {verb} {i.settled_on && formatDate(i.settled_on)}
      {i.status === 'banco' ? ' (banco)' : ''}
      {i.remittance ? ` (remesa ${i.remittance})` : ''}
    </span>
  );
}

function Settle({ i, onDone }: { i: DueItem; onDone: (msg: string) => void }) {
  const [open, setOpen] = useState(false);
  const [date, setDate] = useState(localToday());
  const [account, setAccount] = useState(i.treasury ?? '570');
  // Forma de cobro o pago: la de las condiciones del tercero o, mientras no se elija, la que
  // sugiere la cuenta (caja → efectivo). «Efectivo» es lo que el 347 declara como percibido en
  // metálico (> 6.000 € por cliente y año).
  const [chosen, setChosen] = useState<PayMethod | null>(i.method);
  const method: PayMethod =
    chosen ?? (account.trim().startsWith('570') ? 'efectivo' : 'transferencia');
  /** Factura en moneda extranjera: euros que entraron o salieron de verdad. */
  const [eur, setEur] = useState(plain(i.amount));
  // El error se queda aquí, en rojo, y no sale arriba como si fuera un éxito.
  const [error, setError] = useState<string | null>(null);
  const text = i.kind === 'emitida' ? 'Registrar cobro' : 'Registrar pago';
  if (!open)
    return (
      <button type="button" className="text-link" onClick={() => setOpen(true)}>
        {text}…
      </button>
    );
  return (
    <span className="inline-flex flex-wrap items-center justify-end gap-2">
      <input
        type="date"
        aria-label={`Fecha del ${i.kind === 'emitida' ? 'cobro' : 'pago'} de ${label(i)}`}
        className={small}
        value={date}
        onChange={(e) => setDate(e.target.value)}
      />
      <input
        aria-label={`Cuenta de tesorería de ${label(i)}`}
        className={`${small} w-24 font-mono`}
        title="570 caja, 572x banco…"
        value={account}
        onChange={(e) => setAccount(e.target.value)}
      />
      <select
        aria-label={`Forma de ${i.kind === 'emitida' ? 'cobro' : 'pago'} de ${label(i)}`}
        className={small}
        value={method}
        onChange={(e) => setChosen(e.target.value as PayMethod)}
      >
        {Object.entries(PAY_METHODS).map(([k, v]) => (
          <option key={k} value={k}>
            {v}
          </option>
        ))}
      </select>
      {i.currency && (
        <input
          aria-label={`Euros ${i.kind === 'emitida' ? 'cobrados' : 'pagados'} de ${label(i)}`}
          className={`${small} w-24 text-right tabular-nums`}
          title={`Factura en ${i.currency}: los euros que ${i.kind === 'emitida' ? 'entraron' : 'salieron'} de verdad. La diferencia con lo contabilizado va a 768 (a favor) o 668 (en contra).`}
          inputMode="decimal"
          value={eur}
          onChange={(e) => setEur(e.target.value)}
        />
      )}
      <button
        type="button"
        className="rounded btn-primary px-2 py-0.5"
        onClick={() => {
          const amountEur = i.currency ? parseEuros(eur) : null;
          if (i.currency && amountEur === null) {
            setError('Revisa los euros cobrados o pagados');
            return;
          }
          settleInvoice(
            i.kind,
            i.invoice_id,
            i.seq,
            date,
            account,
            amountEur !== null && amountEur !== i.amount ? amountEur : null,
            method,
          )
            .then(() => {
              setOpen(false);
              onDone(`${i.kind === 'emitida' ? 'Cobro' : 'Pago'} de ${label(i)} registrado.`);
            })
            .catch((e) => setError(errorMessage(e)));
        }}
      >
        {text}
      </button>
      <button type="button" className="px-1" onClick={() => setOpen(false)}>
        ×
      </button>
      {i.cash_limited && account.trim().startsWith('570') && (
        <span role="note" className="basis-full text-right text-xs text-amber-800">
          En efectivo no se puede {i.kind === 'emitida' ? 'cobrar' : 'pagar'} una operación de 1.000
          € o más entre empresarios o profesionales (se suman todos sus plazos; 10.000 € si quien
          paga es un particular no residente): art. 7 de la Ley 7/2012, sanción del 25 % para las
          dos partes. Si fue así, puedes registrarlo igual.
        </span>
      )}
      {error && (
        <span role="alert" className="w-full text-red-700">
          {error}
        </span>
      )}
    </span>
  );
}

/** Desglose de lo pendiente de una factura en varios vencimientos. */
function Split({
  rows,
  onClose,
  onDone,
}: {
  rows: DueItem[];
  onClose: () => void;
  onDone: (msg: string) => void;
}) {
  const first = rows[0] as DueItem;
  const pending = rows.filter((r) => r.status === 'pendiente');
  const settled = rows.filter((r) => r.status !== 'pendiente');
  const left = pending.reduce((s, r) => s + r.amount, 0);
  const [lines, setLines] = useState(
    pending.map((r) => ({ date: r.due_date, amount: plain(r.amount) })),
  );
  const [error, setError] = useState<string | null>(null);
  const sum = lines.reduce((s, l) => s + (parseEuros(l.amount) ?? 0), 0);
  const spread = (n: number) => {
    const part = Math.trunc(left / n);
    const start = pending[0]?.due_date ?? first.due_date;
    setLines(
      Array.from({ length: n }, (_, k) => ({
        date: addDays(start, 30 * k),
        amount: plain(k === n - 1 ? left - part * (n - 1) : part),
      })),
    );
  };
  const save = () => {
    const parsed = lines.map((l) => ({ due_date: l.date, amount: parseEuros(l.amount) }));
    if (parsed.some((p) => p.amount === null || !p.due_date)) {
      setError('Revisa las fechas y los importes.');
      return;
    }
    setInstallments(
      first.kind,
      first.invoice_id,
      parsed.map((p) => ({ due_date: p.due_date, amount: p.amount as number })),
    )
      .then(() => onDone(`Vencimientos de ${first.number} guardados.`))
      .catch((e) => setError(errorMessage(e)));
  };
  return (
    <div
      role="dialog"
      aria-label={`Vencimientos de ${first.number}`}
      className="mt-2 rounded border border-stone-300 bg-stone-50 p-3"
    >
      <p className="font-medium">
        Vencimientos de {first.number} · {first.party} · total {euros(first.invoice_amount)}
      </p>
      {settled.length > 0 && (
        <p className="text-stone-600">
          Ya {first.kind === 'emitida' ? 'cobrado' : 'pagado'}:{' '}
          {euros(settled.reduce((s, r) => s + r.amount, 0))}. Se reparte lo pendiente, {euros(left)}
          .
        </p>
      )}
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <span>Repartir a partes iguales en</span>
        {[1, 2, 3, 4, 6].map((n) => (
          <button
            key={n}
            type="button"
            className="rounded border border-stone-300 px-2"
            onClick={() => spread(n)}
          >
            {n}
          </button>
        ))}
        <span className="text-stone-500">(cada 30 días)</span>
      </div>
      <ol className="mt-2 space-y-1">
        {lines.map((l, k) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: filas editables sin id propio
          <li key={k} className="flex items-center gap-2">
            <span className="w-6 text-right text-stone-500">{k + 1}.</span>
            <input
              type="date"
              aria-label={`Fecha del vencimiento ${k + 1}`}
              className={small}
              value={l.date}
              onChange={(e) =>
                setLines(lines.map((x, j) => (j === k ? { ...x, date: e.target.value } : x)))
              }
            />
            <input
              aria-label={`Importe del vencimiento ${k + 1}`}
              className={`${small} w-28 text-right tabular-nums`}
              value={l.amount}
              onChange={(e) =>
                setLines(lines.map((x, j) => (j === k ? { ...x, amount: e.target.value } : x)))
              }
            />
            {lines.length > 1 && (
              <button
                type="button"
                aria-label={`Quitar el vencimiento ${k + 1}`}
                className="px-1"
                onClick={() => setLines(lines.filter((_, j) => j !== k))}
              >
                ×
              </button>
            )}
          </li>
        ))}
      </ol>
      <div className="mt-2 flex flex-wrap items-center gap-3">
        <button
          type="button"
          className="rounded border border-stone-300 px-2"
          onClick={() =>
            setLines([
              ...lines,
              { date: addDays(lines.at(-1)?.date ?? first.due_date, 30), amount: '0,00' },
            ])
          }
        >
          Añadir vencimiento
        </button>
        <span className={sum === left ? 'text-emerald-700' : 'text-red-700'}>
          Suma {euros(sum)} de {euros(left)}
        </span>
      </div>
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
      <div className="mt-3 flex gap-2">
        <button
          type="button"
          className="rounded btn-primary px-3 py-1"
          disabled={sum !== left}
          onClick={save}
        >
          Guardar vencimientos
        </button>
        {first.fixed && settled.length === 0 && (
          <button
            type="button"
            className="rounded border border-stone-300 px-3 py-1"
            title="Vuelve a calcularlos con las condiciones del cliente o proveedor"
            onClick={() =>
              setInstallments(first.kind, first.invoice_id, null)
                .then(() => onDone(`Vencimientos de ${first.number} recalculados.`))
                .catch((e) => setError(errorMessage(e)))
            }
          >
            Recalcular
          </button>
        )}
        <button
          type="button"
          className="rounded border border-stone-300 px-3 py-1"
          onClick={onClose}
        >
          Cancelar
        </button>
      </div>
    </div>
  );
}

/** Antigüedad de lo vencido y sin cobrar (o pagar), por tramos. */
function Aging({ items, kind }: { items: DueItem[]; kind: DueItem['kind'] }) {
  const overdue = items.filter(
    (i) => i.kind === kind && i.status === 'pendiente' && i.overdue_days > 0,
  );
  const buckets: [string, (d: number) => boolean][] = [
    ['1–30 días', (d) => d <= 30],
    ['31–60', (d) => d > 30 && d <= 60],
    ['61–90', (d) => d > 60 && d <= 90],
    ['Más de 90', (d) => d > 90],
  ];
  return (
    <tr>
      <th scope="row" className="pr-3 text-left font-normal text-stone-600">
        {kind === 'emitida' ? 'Cobros vencidos' : 'Pagos vencidos'}
      </th>
      {buckets.map(([name, test]) => (
        <td key={name} className="px-2 text-right tabular-nums">
          {euros(overdue.filter((i) => test(i.overdue_days)).reduce((s, i) => s + i.amount, 0))}
        </td>
      ))}
    </tr>
  );
}

function Dues() {
  const [items, setItems] = useState<DueItem[]>([]);
  const [filter, setFilter] = useState<Filter>('pendientes');
  const [terms, setTerms] = useState<PaymentTerms | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [splitting, setSplitting] = useState<string | null>(null);
  const [reminding, setReminding] = useState<DueItem | null>(null);

  const load = useCallback(() => {
    carteraList(localToday())
      .then(setItems)
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  useEffect(() => {
    load();
    paymentTerms()
      .then(setTerms)
      .catch(() => setTerms(null));
  }, [load]);

  const done = (m: string) => {
    setMsg(m);
    setSplitting(null);
    load();
  };
  const pending = items.filter((i) => i.status === 'pendiente');
  const sum = (kind: DueItem['kind'], overdue = false) =>
    pending
      .filter((i) => i.kind === kind && (!overdue || i.overdue_days > 0))
      .reduce((s, i) => s + i.amount, 0);
  const shown = items.filter((i) =>
    filter === 'todas'
      ? true
      : filter === 'vencidas'
        ? i.status === 'pendiente' && i.overdue_days > 0
        : i.status === 'pendiente',
  );
  const invoiceKey = (i: DueItem) => `${i.kind}-${i.invoice_id}`;

  return (
    <div className="text-sm">
      <dl className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        <div>
          <dt className="text-stone-500">Pendiente de cobro</dt>
          <dd className="text-lg font-semibold">{euros(sum('emitida'))}</dd>
        </div>
        <div>
          <dt className="text-stone-500">Cobros vencidos</dt>
          <dd className="text-lg font-semibold text-red-700">{euros(sum('emitida', true))}</dd>
        </div>
        <div>
          <dt className="text-stone-500">Pendiente de pago</dt>
          <dd className="text-lg font-semibold">{euros(sum('recibida'))}</dd>
        </div>
        <div>
          <dt className="text-stone-500">Pagos vencidos</dt>
          <dd className="text-lg font-semibold text-red-700">{euros(sum('recibida', true))}</dd>
        </div>
      </dl>
      {(sum('emitida', true) !== 0 || sum('recibida', true) !== 0) && (
        <table className="mt-3 text-xs" aria-label="Antigüedad de lo vencido">
          <thead className="text-stone-500">
            <tr>
              <th />
              <th className="px-2 text-right font-normal">1–30 días</th>
              <th className="px-2 text-right font-normal">31–60</th>
              <th className="px-2 text-right font-normal">61–90</th>
              <th className="px-2 text-right font-normal">Más de 90</th>
            </tr>
          </thead>
          <tbody>
            <Aging items={items} kind="emitida" />
            <Aging items={items} kind="recibida" />
          </tbody>
        </table>
      )}
      <div className="mt-4 flex flex-wrap items-end gap-x-6 gap-y-3">
        <label>
          Mostrar
          <select
            className="mt-1 block"
            value={filter}
            onChange={(e) => setFilter(e.target.value as Filter)}
          >
            <option value="pendientes">Pendientes</option>
            <option value="vencidas">Vencidas</option>
            <option value="todas">Todas</option>
          </select>
        </label>
        {terms && (
          <span className="flex flex-wrap items-end gap-3">
            <label>
              Plazo de cobro (días)
              <input
                type="number"
                min={0}
                max={60}
                className="mt-1 block w-24"
                value={terms.customers_days}
                onChange={(e) => setTerms({ ...terms, customers_days: Number(e.target.value) })}
              />
            </label>
            <label>
              Plazo de pago (días)
              <input
                type="number"
                min={0}
                max={60}
                className="mt-1 block w-24"
                value={terms.suppliers_days}
                onChange={(e) => setTerms({ ...terms, suppliers_days: Number(e.target.value) })}
              />
            </label>
            <button
              type="button"
              className="rounded border border-stone-300 px-3"
              onClick={() =>
                setPaymentTerms(terms)
                  .then(() => done('Plazos guardados.'))
                  .catch((e) => setMsg(errorMessage(e)))
              }
            >
              Guardar plazos
            </button>
          </span>
        )}
      </div>
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}
      <table className="mt-3 w-full">
        <thead className="text-left text-stone-500">
          <tr>
            <th>Vence</th>
            <th className="pl-2">Tipo</th>
            <th>Factura</th>
            <th>Cliente / proveedor</th>
            <th className="text-right">Importe</th>
            <th className="pl-4">Estado</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {shown.map((i) => (
            <tr key={`${invoiceKey(i)}-${i.seq}`} className="border-t border-stone-200 align-top">
              <td>
                {i.status === 'pendiente' ? (
                  <input
                    type="date"
                    aria-label={`Vencimiento de ${label(i)}`}
                    className={small}
                    value={i.due_date}
                    onChange={(e) =>
                      e.target.value &&
                      setDueDate(i.kind, i.invoice_id, i.seq, e.target.value)
                        .then(load)
                        .catch((err) => setMsg(errorMessage(err)))
                    }
                  />
                ) : (
                  formatDate(i.due_date)
                )}
              </td>
              <td className="pl-2">{i.kind === 'emitida' ? 'Cobro' : 'Pago'}</td>
              <td>
                {i.number}
                {i.currency && (
                  <span
                    className="ml-1 rounded bg-stone-100 px-1 text-xs text-stone-600"
                    title={`Factura en ${i.currency}: importe en euros al tipo de la factura`}
                  >
                    {i.currency}
                  </span>
                )}
                {i.installments > 1 && (
                  <span className="ml-1 rounded bg-stone-100 px-1 text-xs text-stone-600">
                    {i.seq}/{i.installments}
                  </span>
                )}
                <span className="block text-xs text-stone-500">
                  {formatDate(i.issue_date)}
                  {i.method ? ` · ${PAY_METHODS[i.method]}` : ''}
                </span>
              </td>
              <td>{i.party}</td>
              <td className="text-right tabular-nums">{euros(i.amount)}</td>
              <td className="pl-4">
                <Status i={i} />
              </td>
              <td className="text-right">
                {i.status === 'pendiente' && (
                  <span className="inline-flex flex-wrap justify-end gap-2">
                    {i.kind === 'emitida' && i.overdue_days > 0 && (
                      <button type="button" className="text-link" onClick={() => setReminding(i)}>
                        Recordatorio…
                      </button>
                    )}
                    <Settle i={i} onDone={done} />
                    <button
                      type="button"
                      className="text-stone-700"
                      onClick={() =>
                        setSplitting(splitting === invoiceKey(i) ? null : invoiceKey(i))
                      }
                    >
                      Fraccionar…
                    </button>
                  </span>
                )}
                {i.status === 'registrada' && !i.remittance && (
                  <ConfirmAction
                    label="Deshacer"
                    question={`¿Deshacer el ${i.kind === 'emitida' ? 'cobro' : 'pago'} de ${label(i)}? Se borra su asiento.`}
                    confirmLabel="Sí, deshacer"
                    onConfirm={() =>
                      unsettleInvoice(i.kind, i.invoice_id, i.seq, true)
                        .then(() => done(`${label(i)} vuelve a estar pendiente.`))
                        .catch((e) => setMsg(errorMessage(e)))
                    }
                  />
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {shown.length === 0 && <p className="mt-2 text-stone-500">Nada que mostrar.</p>}
      {reminding && (
        <InvoiceEmailPanel
          key={reminding.invoice_id}
          invoiceId={reminding.invoice_id}
          number={reminding.number}
          reminder
          onClose={() => setReminding(null)}
          onDone={(m) => {
            setReminding(null);
            setMsg(m);
          }}
        />
      )}
      {splitting && (
        <Split
          key={splitting}
          rows={items.filter((i) => invoiceKey(i) === splitting)}
          onClose={() => setSplitting(null)}
          onDone={done}
        />
      )}
      <p className="mt-3 text-xs text-stone-500">
        Las facturas conciliadas con el banco (pestaña Banco) salen como cobradas o pagadas solas.
        Los vencimientos salen de las condiciones de cada cliente o proveedor o, si no tiene, del
        plazo general. Plazo máximo entre empresas: 60 días (Ley 3/2004).
      </p>
    </div>
  );
}

/** Cuenta de ventas (cliente) o de gastos (proveedor) propia del tercero. */
function Counterpart({ p, onDone }: { p: PartyRow; onDone: (msg: string) => void }) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(p.counterpart ?? '');
  const [error, setError] = useState<string | null>(null);
  const what = p.role === 'cliente' ? 'ventas' : 'gastos';
  if (!editing) {
    return (
      <button
        type="button"
        className="block text-xs text-stone-600 underline"
        onClick={() => setEditing(true)}
      >
        {p.counterpart ? `Cuenta de ${what}: ${p.counterpart}` : `Cuenta de ${what}: la general`}
      </button>
    );
  }
  return (
    <span className="mt-1 flex flex-wrap items-center gap-1 text-xs">
      <input
        aria-label={`Cuenta de ${what} de ${p.name}`}
        className={`${small} w-24 font-mono`}
        placeholder={p.role === 'cliente' ? '705…' : '629…'}
        value={value}
        onChange={(e) => setValue(e.target.value)}
      />
      <button
        type="button"
        className="rounded btn-primary px-2 py-0.5"
        onClick={() =>
          setPartyCounterpart(p.account, value.trim() || null)
            .then(() => {
              setEditing(false);
              onDone(`Cuenta de ${what} de ${p.name} guardada.`);
            })
            .catch((e) => setError(errorMessage(e)))
        }
      >
        Guardar
      </button>
      <button type="button" onClick={() => setEditing(false)}>
        Cancelar
      </button>
      {error && (
        <span role="alert" className="w-full text-red-700">
          {error}
        </span>
      )}
    </span>
  );
}

function TermsRow({ p, onDone }: { p: PartyRow; onDone: (msg: string) => void }) {
  const [editing, setEditing] = useState(false);
  const [days, setDays] = useState(p.terms?.days.join(', ') ?? '30');
  const [payDay, setPayDay] = useState(p.terms?.pay_day?.toString() ?? '');
  const [method, setMethod] = useState<PayMethod>(p.terms?.method ?? 'transferencia');
  const [treasury, setTreasury] = useState(p.terms?.treasury ?? '');
  const [iban, setIban] = useState(p.terms?.iban ?? '');
  const [mandateRef, setMandateRef] = useState(p.terms?.mandate_ref ?? '');
  const [mandateDate, setMandateDate] = useState(p.terms?.mandate_date ?? '');
  const [key190, setKey190] = useState<Key190 | ''>(p.terms?.key_190 ?? '');
  const [error, setError] = useState<string | null>(null);
  const describe = (t: PartyTerms) =>
    `${t.days.map((d) => (d === 0 ? 'contado' : `${d} d`)).join(' + ')}${
      t.pay_day ? ` · día ${t.pay_day}` : ''
    } · ${PAY_METHODS[t.method]}${t.treasury ? ` · ${t.treasury}` : ''}${
      t.iban ? ` · IBAN …${t.iban.slice(-4)}` : ''
    }${t.mandate_ref ? ` · mandato ${t.mandate_ref}` : ''}${
      t.key_190 ? ` · 190: ${t.key_190.slice(0, 1)}.${t.key_190.slice(1)}` : ''
    }`;
  const save = () => {
    const list = days
      .split(/[,\s/+]+/)
      .filter(Boolean)
      .map(Number);
    if (list.some((d) => !Number.isInteger(d) || d < 0)) {
      setError('Los días van separados por comas, p. ej. «30, 60».');
      return;
    }
    setPartyTerms(p.account, {
      days: list,
      pay_day: payDay ? Number(payDay) : null,
      method,
      treasury: treasury.trim() || null,
      iban: iban.trim() || null,
      mandate_ref: mandateRef.trim() || null,
      mandate_date: mandateDate || null,
      key_190: key190 || null,
    })
      .then(() => {
        setEditing(false);
        onDone(`Condiciones de ${p.name} guardadas.`);
      })
      .catch((e) => setError(errorMessage(e)));
  };
  return (
    <tr className="border-t border-stone-200 align-top">
      <td className="font-mono">{p.account}</td>
      <td>
        {p.name}
        <span className="block text-xs text-stone-500">{p.tax_id}</span>
        <Counterpart p={p} onDone={onDone} />
      </td>
      <td>
        {editing ? (
          <div className="form-grid my-1 rounded-md bg-stone-50 p-3">
            <label>
              Días de cada plazo
              <input value={days} placeholder="30, 60" onChange={(e) => setDays(e.target.value)} />
            </label>
            <label>
              Día de pago
              <input
                type="number"
                min={1}
                max={31}
                value={payDay}
                onChange={(e) => setPayDay(e.target.value)}
              />
            </label>
            <label>
              Forma
              <select value={method} onChange={(e) => setMethod(e.target.value as PayMethod)}>
                {Object.entries(PAY_METHODS).map(([k, v]) => (
                  <option key={k} value={k}>
                    {v}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Cuenta
              <input
                className="font-mono"
                placeholder="572…"
                value={treasury}
                onChange={(e) => setTreasury(e.target.value)}
              />
            </label>
            <label className="span-2">
              IBAN
              <input
                className="font-mono"
                placeholder="ES00 0000 0000 0000 0000 0000"
                value={iban}
                onChange={(e) => setIban(e.target.value)}
              />
            </label>
            {p.role === 'proveedor' && (
              <label className="span-2">
                Clave del 190
                <select
                  value={key190}
                  onChange={(e) => setKey190(e.target.value as Key190 | '')}
                  title="Clave y subclave con la que va en el resumen anual de retenciones (modelo 190). Sin elegir: G.01, profesional al 15 %."
                >
                  <option value="">G.01 · Profesional, 15 % (por defecto)</option>
                  {Object.entries(KEYS_190)
                    .filter(([k]) => k !== 'G01')
                    .map(([k, v]) => (
                      <option key={k} value={k}>
                        {v}
                      </option>
                    ))}
                </select>
              </label>
            )}
            {p.role === 'cliente' && (
              <>
                <label>
                  Mandato SEPA
                  <input
                    placeholder="Referencia"
                    value={mandateRef}
                    onChange={(e) => setMandateRef(e.target.value)}
                  />
                </label>
                <label>
                  Firmado el
                  <input
                    type="date"
                    value={mandateDate}
                    onChange={(e) => setMandateDate(e.target.value)}
                  />
                </label>
              </>
            )}
            <div className="span-full form-actions">
              <button type="button" className="rounded btn-primary px-3" onClick={save}>
                Guardar
              </button>
              <button
                type="button"
                className="rounded border border-stone-300 px-3"
                onClick={() => setEditing(false)}
              >
                Cancelar
              </button>
            </div>
            {error && (
              <p role="alert" className="span-full text-red-700">
                {error}
              </p>
            )}
          </div>
        ) : p.terms ? (
          describe(p.terms)
        ) : (
          <span className="text-stone-500">Plazo general</span>
        )}
      </td>
      <td className="text-right whitespace-nowrap">
        {!editing && (
          <button type="button" className="text-stone-700" onClick={() => setEditing(true)}>
            {p.terms ? 'Cambiar' : 'Poner condiciones'}
          </button>
        )}
        {!editing && p.terms && (
          <button
            type="button"
            className="ml-3 text-stone-700"
            onClick={() =>
              setPartyTerms(p.account, null)
                .then(() => onDone(`${p.name} vuelve al plazo general.`))
                .catch((e) => setError(errorMessage(e)))
            }
          >
            Quitar
          </button>
        )}
      </td>
    </tr>
  );
}

function Conditions() {
  const [rows, setRows] = useState<PartyRow[]>([]);
  const [msg, setMsg] = useState<string | null>(null);
  const [q, setQ] = useState('');
  const load = useCallback(() => {
    partyTermsList()
      .then(setRows)
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  useEffect(load, [load]);
  const done = (m: string) => {
    setMsg(m);
    load();
  };
  const needle = q.trim().toLowerCase();
  const shown = rows.filter(
    (r) =>
      !needle ||
      r.name.toLowerCase().includes(needle) ||
      r.tax_id.toLowerCase().includes(needle) ||
      r.account.startsWith(needle),
  );
  return (
    <div className="text-sm">
      <p className="text-stone-600">
        Como en la ficha de cada cuenta de a3: días de cada plazo desde la factura («30, 60»
        fracciona en dos), día de pago fijo, forma de pago y cuenta de cobro o pago. Se aplican a
        las facturas cuyos vencimientos no se han fijado (las ya cobradas en parte no cambian).
      </p>
      <label className="mt-3 block">
        Buscar
        <input
          className="mt-1 block w-64"
          value={q}
          placeholder="Nombre, NIF o subcuenta"
          onChange={(e) => setQ(e.target.value)}
        />
      </label>
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}
      {(['cliente', 'proveedor'] as const).map((role) => {
        const list = shown.filter((r) => r.role === role);
        return (
          <section key={role} className="mt-4">
            <h3 className="font-medium">{role === 'cliente' ? 'Clientes' : 'Proveedores'}</h3>
            {list.length === 0 ? (
              <p className="text-stone-500">
                Ninguno todavía: salen al contabilizar su primera factura.
              </p>
            ) : (
              <table className="mt-1 w-full">
                <thead className="text-left text-stone-500">
                  <tr>
                    <th>Subcuenta</th>
                    <th>Nombre</th>
                    <th>Condiciones</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {list.map((p) => (
                    <TermsRow key={p.account} p={p} onDone={done} />
                  ))}
                </tbody>
              </table>
            )}
          </section>
        );
      })}
    </div>
  );
}

/** Cartera de cobros y pagos: vencimientos de las facturas y condiciones de cada tercero. */
export function Cartera() {
  const [tab, setTab] = useState<
    'vencimientos' | 'condiciones' | 'remesas' | 'cuadre' | 'incobrables'
  >('vencimientos');
  const tabClass = (t: typeof tab) =>
    `rounded px-3 py-1 ${tab === t ? 'border border-brand-500 font-medium' : 'border border-stone-300'}`;
  return (
    <div className="mt-4">
      <div className="mb-3 flex gap-2 text-sm">
        <button
          type="button"
          className={tabClass('vencimientos')}
          aria-pressed={tab === 'vencimientos'}
          onClick={() => setTab('vencimientos')}
        >
          Vencimientos
        </button>
        <button
          type="button"
          className={tabClass('condiciones')}
          aria-pressed={tab === 'condiciones'}
          onClick={() => setTab('condiciones')}
        >
          Condiciones de pago
        </button>
        <button
          type="button"
          className={tabClass('remesas')}
          aria-pressed={tab === 'remesas'}
          onClick={() => setTab('remesas')}
        >
          Remesas SEPA
        </button>
        <button
          type="button"
          className={tabClass('cuadre')}
          aria-pressed={tab === 'cuadre'}
          onClick={() => setTab('cuadre')}
        >
          Cuadre y plazo de pago
        </button>
        <button
          type="button"
          className={tabClass('incobrables')}
          aria-pressed={tab === 'incobrables'}
          onClick={() => setTab('incobrables')}
        >
          Créditos incobrables
        </button>
      </div>
      {tab === 'vencimientos' && <Dues />}
      {tab === 'condiciones' && <Conditions />}
      {tab === 'remesas' && <Remesas />}
      {tab === 'cuadre' && <CarteraCheck />}
      {tab === 'incobrables' && <Incobrables />}
    </div>
  );
}

import { type FormEvent, useCallback, useEffect, useState } from 'react';
import {
  COUNTRIES,
  DIVIDEND_AGREEMENT_KINDS,
  type DividendAgreement,
  type DividendAgreementCheck,
  type DividendAgreementInput,
  type DividendAgreementKind,
  type DividendPayment,
  dividendAgreementCheck,
  dividendAgreementRecord,
  dividendAgreements,
  dividendAgreementUndo,
  dividendPay,
  dividendPayments,
  dividendPropose,
  dividendUndo,
  errorMessage,
  euros,
  formatDate,
  MINUTE_KINDS,
  type MinuteKind,
  type PayoutProposal,
  parseEuros,
  type Shareholder,
  shareholderDelete,
  shareholderSave,
  shareholdersList,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { ProvinceCodeSelect } from './Controls';

const input = 'mt-1 block rounded-md border border-stone-300 px-2 py-1.5';
const cell = 'px-2 py-1 text-right tabular-nums';

/** Millonésimas → «33,3333». */
export const showPct = (p: number) =>
  (p / 10000).toLocaleString('es-ES', { maximumFractionDigits: 4 });

/** «33,3333» → millonésimas (333333); inválido → null. */
export function parsePct(s: string): number | null {
  const n = Number(s.trim().replace(',', '.'));
  if (!(n > 0 && n <= 100)) return null;
  return Math.round(n * 10000);
}

const EMPTY: Shareholder = {
  id: 0,
  nif: '',
  name: '',
  address: '',
  province: '',
  country: 'ES',
  pct: 0,
  legal_person: false,
  resident: true,
  exempt: false,
  active: true,
};

/** Trimestre (y plazo) del 123 en que se ingresa la retención de un pago con fecha `iso`. */
export function quarterOf(iso: string): string {
  const [y, m] = iso.split('-').map(Number);
  if (!y || !m) return '';
  const q = Math.ceil(m / 3);
  const deadline =
    q === 4 ? `20 de enero de ${y + 1}` : `20 de ${['abril', 'julio', 'octubre'][q - 1]}`;
  return `${q}T de ${y} (hasta el ${deadline})`;
}

function ShareholderForm({ initial, onSaved }: { initial: Shareholder; onSaved: () => void }) {
  const [s, setS] = useState(initial);
  const [pct, setPct] = useState(initial.pct ? showPct(initial.pct) : '');
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<Shareholder>) => setS({ ...s, ...patch });
  async function submit(ev: FormEvent) {
    ev.preventDefault();
    const p = parsePct(pct);
    if (p === null) return setError('La participación va de 0,0001 % a 100 %.');
    try {
      await shareholderSave({ ...s, pct: p, exempt: s.exempt && s.legal_person && s.resident });
      onSaved();
    } catch (e) {
      setError(errorMessage(e));
    }
  }
  return (
    <form onSubmit={submit} className="form-grid mt-3 rounded-md border border-stone-200 p-4">
      <label>
        NIF
        <input value={s.nif} onChange={(e) => set({ nif: e.target.value })} />
      </label>
      <label className="span-2">
        Apellidos y nombre o razón social
        <input value={s.name} onChange={(e) => set({ name: e.target.value })} />
      </label>
      <label>
        Participación (%)
        <input
          className="text-right"
          inputMode="decimal"
          value={pct}
          onChange={(e) => setPct(e.target.value)}
        />
      </label>
      <label className="span-2">
        Domicilio
        <input value={s.address} onChange={(e) => set({ address: e.target.value })} />
      </label>
      {s.resident ? (
        <label className="span-2">
          Provincia
          <ProvinceCodeSelect value={s.province} onChange={(province) => set({ province })} />
        </label>
      ) : (
        <label className="span-2">
          País de residencia (código, p. ej. FR)
          <input
            maxLength={2}
            list="dividendos-paises"
            value={s.country}
            onChange={(e) => set({ country: e.target.value.trim().toUpperCase() })}
          />
          <datalist id="dividendos-paises">
            {COUNTRIES.filter(([c]) => c !== 'ES' && c !== 'XI').map(([code, name]) => (
              <option key={code} value={code}>
                {name}
              </option>
            ))}
          </datalist>
        </label>
      )}
      <div className="span-full flex flex-wrap gap-x-6 gap-y-2">
        <label className="flex items-start gap-2">
          <input
            type="checkbox"
            checked={s.legal_person}
            onChange={(e) => set({ legal_person: e.target.checked })}
          />
          Es una sociedad
        </label>
        <label className="flex items-start gap-2">
          <input
            type="checkbox"
            checked={!s.resident}
            onChange={(e) =>
              set({
                resident: !e.target.checked,
                country: e.target.checked ? '' : 'ES',
                province: e.target.checked ? '' : s.province,
              })
            }
          />
          No residente en España
        </label>
        {s.legal_person && s.resident && (
          <label className="flex basis-full items-start gap-2">
            <input
              type="checkbox"
              checked={s.exempt}
              onChange={(e) => set({ exempt: e.target.checked })}
            />
            <span>
              Ha comunicado que tiene derecho a la exención del art. 21 LIS (al menos un 5 % durante
              un año): sus dividendos no llevan retención (art. 128.4.d LIS y art. 61.p RIS).
            </span>
          </label>
        )}
      </div>
      <div className="span-full form-actions">
        <button type="submit" className="btn-primary rounded-md px-3 py-1.5">
          Guardar socio
        </button>
        {error && (
          <p role="alert" className="text-red-700">
            {error}
          </p>
        )}
      </div>
    </form>
  );
}

function Shareholders({ list, onChange }: { list: Shareholder[]; onChange: () => void }) {
  const [editing, setEditing] = useState<Shareholder | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const total = list.filter((s) => s.active).reduce((a, s) => a + s.pct, 0);
  return (
    <section aria-label="Socios" className="mt-3">
      <h4 className="font-medium">Socios</h4>
      {list.length === 0 ? (
        <p className="text-stone-600">Aún no hay socios: añádelos con su participación.</p>
      ) : (
        <ul className="mt-1 space-y-1">
          {list.map((s) => (
            <li key={s.id} className="flex flex-wrap items-center gap-3">
              <span className="font-mono">{s.nif}</span>
              <span>{s.name}</span>
              <span className="tabular-nums">{showPct(s.pct)} %</span>
              {s.legal_person && <span className="pill">Sociedad</span>}
              {!s.resident && <span className="pill">No residente ({s.country})</span>}
              {s.exempt && <span className="pill">Sin retención (art. 21 LIS)</span>}
              <button type="button" className="underline" onClick={() => setEditing(s)}>
                Editar
              </button>
              <ConfirmAction
                label="Quitar"
                question={`¿Quitar a ${s.name} de los socios? Los pagos ya hechos conservan sus datos.`}
                confirmLabel="Sí, quitar"
                onConfirm={async () => {
                  try {
                    await shareholderDelete(s.id);
                    onChange();
                  } catch (e) {
                    setMsg(errorMessage(e));
                  }
                }}
              />
            </li>
          ))}
        </ul>
      )}
      {list.length > 0 && total !== 1_000_000 && (
        <p className="mt-1 text-amber-800">
          Las participaciones suman {showPct(total)} %: el reparto se hace en proporción a ellas.
        </p>
      )}
      {editing ? (
        <ShareholderForm
          key={editing.id}
          initial={editing}
          onSaved={() => {
            setEditing(null);
            onChange();
          }}
        />
      ) : (
        <button type="button" className="mt-1 underline" onClick={() => setEditing(EMPTY)}>
          Añadir socio
        </button>
      )}
      {msg && <p role="status">{msg}</p>}
    </section>
  );
}

function Proposal({ p }: { p: PayoutProposal }) {
  return (
    <>
      <table className="mt-3 w-full">
        <caption className="sr-only">Reparto por socio</caption>
        <thead className="text-left text-stone-500">
          <tr>
            <th className="font-normal">Socio</th>
            <th className="px-2 text-right font-normal">%</th>
            <th className="px-2 text-right font-normal">Bruto</th>
            <th className="px-2 text-right font-normal">Retención</th>
            <th className="px-2 text-right font-normal">Líquido</th>
          </tr>
        </thead>
        <tbody>
          {p.lines.map((l) => (
            <tr key={l.nif} className="border-t border-stone-100">
              <td className="py-1">
                {l.name} <span className="text-stone-500">{l.nif}</span>
              </td>
              <td className={cell}>{showPct(l.pct)}</td>
              <td className={cell}>{euros(l.gross)}</td>
              <td className={cell}>
                {euros(l.withheld)}{' '}
                <span className="text-stone-500">({(l.rate / 100).toLocaleString('es-ES')} %)</span>
              </td>
              <td className={cell}>{euros(l.net)}</td>
            </tr>
          ))}
          <tr className="border-t border-stone-200 font-medium">
            <td className="py-1">Total</td>
            <td />
            <td className={cell}>{euros(p.gross)}</td>
            <td className={cell}>{euros(p.withheld)}</td>
            <td className={cell}>{euros(p.net)}</td>
          </tr>
        </tbody>
      </table>
      <table className="mt-3 w-full">
        <caption className="text-left text-stone-600">Asiento del {formatDate(p.date)}</caption>
        <thead className="text-left text-stone-500">
          <tr>
            <th className="font-normal">Cuenta</th>
            <th className="px-2 text-right font-normal">Debe</th>
            <th className="px-2 text-right font-normal">Haber</th>
          </tr>
        </thead>
        <tbody>
          {p.entry.map((l) => (
            <tr key={l.account} className="border-t border-stone-100">
              <td className="py-1">
                {l.account} <span className="text-stone-500">{l.name}</span>
              </td>
              <td className={cell}>{l.debit ? euros(l.debit) : ''}</td>
              <td className={cell}>{l.credit ? euros(l.credit) : ''}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {p.warnings.length > 0 && (
        <ul className="mt-3 space-y-1 rounded-md bg-amber-50 p-3 text-amber-950">
          {p.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
    </>
  );
}

const LIGHTS: Record<DividendAgreementCheck['checks'][number]['light'], [string, string]> = {
  verde: ['Se cumple', 'bg-green-50 text-green-900'],
  ambar: ['Revisar', 'bg-amber-50 text-amber-950'],
  rojo: ['No se cumple', 'bg-red-50 text-red-900'],
};

/** Semáforo de las comprobaciones legales del reparto. */
export function Checks({ c }: { c: DividendAgreementCheck }) {
  return (
    <ul aria-label="Comprobaciones legales" className="mt-3 space-y-1">
      {c.checks.map((x) => {
        const [label, color] = LIGHTS[x.light];
        return (
          <li key={x.code} className={`rounded-md p-2 ${color}`}>
            <span className="font-medium">
              {label} · {x.title}
            </span>{' '}
            <span className="text-stone-600">({x.norm})</span>
            <p>{x.detail}</p>
          </li>
        );
      })}
    </ul>
  );
}

const AGREEMENT_SHORT: Record<DividendAgreementKind, string> = {
  a_cuenta: 'A cuenta',
  reservas: 'Con cargo a reservas',
  resultado: 'Del resultado',
};

/** Acordar un dividendo: comprobaciones, asiento del acuerdo y acta en borrador. */
function NewAgreement({ year, onDone }: { year: number; onDone: (msg: string) => void }) {
  const [kind, setKind] = useState<DividendAgreementKind>('a_cuenta');
  const [date, setDate] = useState('');
  const [amount, setAmount] = useState('');
  const [payment, setPayment] = useState('');
  const [statement, setStatement] = useState('');
  const [rate, setRate] = useState('25');
  const [reserve, setReserve] = useState('113');
  const [organ, setOrgan] = useState<MinuteKind | ''>('');
  const [c, setC] = useState<DividendAgreementCheck | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reset = () => setC(null);
  const cents = amount.trim() ? parseEuros(amount) : null;
  const rateBp = Math.round(Number(rate.replace(',', '.')) * 100);
  const ready = !!date && (kind === 'resultado' || (cents !== null && cents > 0));
  const build = (): DividendAgreementInput => ({
    kind,
    date,
    amount: kind === 'resultado' ? 0 : (cents ?? 0),
    payment_date: payment || null,
    statement_date: kind === 'a_cuenta' && statement ? statement : null,
    tax_rate: kind === 'a_cuenta' && rateBp > 0 ? rateBp : null,
    reserve_account: kind === 'reservas' ? reserve : null,
    year: kind === 'resultado' ? year : null,
    organ: organ || null,
  });
  const l = c?.liquidity;
  return (
    <section
      aria-label="Acordar un dividendo"
      className="mt-3 rounded-md border border-stone-200 p-4"
    >
      <label className="block">
        Clase de dividendo
        <select
          className={input}
          value={kind}
          onChange={(e) => {
            setKind(e.target.value as DividendAgreementKind);
            reset();
          }}
        >
          {(Object.keys(DIVIDEND_AGREEMENT_KINDS) as DividendAgreementKind[]).map((k) => (
            <option key={k} value={k}>
              {k === 'resultado'
                ? `${DIVIDEND_AGREEMENT_KINDS[k]} ${year}`
                : DIVIDEND_AGREEMENT_KINDS[k]}
            </option>
          ))}
        </select>
      </label>
      <div className="mt-2 flex flex-wrap items-end gap-3">
        <label>
          Fecha del acuerdo
          <input
            type="date"
            className={input}
            value={date}
            onChange={(e) => {
              setDate(e.target.value);
              reset();
            }}
          />
        </label>
        {kind !== 'resultado' && (
          <label>
            Dividendo bruto
            <input
              className={`${input} w-36 text-right`}
              inputMode="decimal"
              value={amount}
              onChange={(e) => {
                setAmount(e.target.value);
                reset();
              }}
            />
          </label>
        )}
        <label>
          Fecha de pago acordada
          <input
            type="date"
            className={input}
            value={payment}
            onChange={(e) => {
              setPayment(e.target.value);
              reset();
            }}
          />
        </label>
        {kind === 'a_cuenta' && (
          <>
            <label>
              Fecha del estado contable
              <input
                type="date"
                className={input}
                value={statement}
                onChange={(e) => {
                  setStatement(e.target.value);
                  reset();
                }}
              />
            </label>
            <label>
              Tipo del IS estimado (%)
              <input
                className={`${input} w-20 text-right`}
                inputMode="decimal"
                value={rate}
                onChange={(e) => {
                  setRate(e.target.value);
                  reset();
                }}
              />
            </label>
          </>
        )}
        {kind === 'reservas' && (
          <label>
            Cuenta
            <select
              className={input}
              value={reserve}
              onChange={(e) => {
                setReserve(e.target.value);
                reset();
              }}
            >
              <option value="113">113 Reservas voluntarias</option>
              <option value="120">120 Remanente</option>
            </select>
          </label>
        )}
        <label>
          Órgano que acuerda
          <select
            className={input}
            value={organ}
            onChange={(e) => {
              setOrgan(e.target.value as MinuteKind | '');
              reset();
            }}
          >
            <option value="">El que corresponda (socio único o junta)</option>
            {(Object.keys(MINUTE_KINDS) as MinuteKind[]).map((k) => (
              <option key={k} value={k}>
                {MINUTE_KINDS[k]}
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          className="rounded-md border border-stone-300 px-3 py-1.5"
          disabled={!ready}
          onClick={() => {
            setError(null);
            dividendAgreementCheck(build())
              .then(setC)
              .catch((e) => {
                setC(null);
                setError(errorMessage(e));
              });
          }}
        >
          Comprobar
        </button>
      </div>
      <p className="mt-1 text-stone-600">
        {kind === 'a_cuenta'
          ? 'El dividendo a cuenta lo acuerdan la junta o los administradores con un estado contable que muestre liquidez suficiente; no puede pasar del resultado desde el último cierre menos pérdidas anteriores, reserva legal e impuesto estimado. Se contabiliza 557 a 526; al aprobar las cuentas, la aplicación del resultado salda la 557.'
          : kind === 'reservas'
            ? 'Lo acuerda la junta (o el socio único). Se contabiliza la reserva a la 526.'
            : 'El asiento es la aplicación del resultado contabilizada en Cierre; aquí se registra el acuerdo y se crea el acta de aprobación de cuentas.'}
      </p>
      {c && (
        <>
          <Checks c={c} />
          {l && (
            <table className="mt-3 w-full">
              <caption className="text-left text-stone-600">
                Estado contable de liquidez a {formatDate(l.date)} (art. 277 LSC)
              </caption>
              <tbody>
                {(
                  [
                    [`Resultado desde ${formatDate(l.period_start)}`, l.result],
                    ['Menos pérdidas de ejercicios anteriores', l.prior_losses],
                    ['Menos reserva legal obligatoria', l.legal_reserve],
                    [
                      `Menos impuesto estimado (${(l.tax_rate / 100).toLocaleString('es-ES')} %)`,
                      l.estimated_tax,
                    ],
                    ['Menos ya repartido a cuenta', l.already_distributed],
                    ['Cantidad máxima distribuible', l.limit],
                    ['Tesorería', l.treasury],
                    ['Deudas a corto plazo', l.short_term_debts],
                    ['Tesorería tras el pago', l.treasury_after],
                  ] as [string, number][]
                ).map(([k, m]) => (
                  <tr key={k} className="border-t border-stone-100">
                    <td className="py-1">{k}</td>
                    <td className={cell}>{euros(m)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {c.entry.length > 0 && (
            <p className="mt-2">
              Asiento del {formatDate(c.date)}:{' '}
              {c.entry
                .map(
                  (e) => `${e.account} ${e.debit ? 'debe' : 'haber'} ${euros(e.debit || e.credit)}`,
                )
                .join(' · ')}
            </p>
          )}
          <p className="mt-2">
            Dividendo {euros(c.amount)}, retención {euros(c.withheld)}. El acta se crea en borrador
            como «{MINUTE_KINDS[c.organ]}».
          </p>
          {c.warnings.length > 0 && (
            <ul className="mt-2 space-y-1 rounded-md bg-amber-50 p-3 text-amber-950">
              {c.warnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          )}
          <div className="mt-3">
            {c.blocked ? (
              <p role="alert" className="text-red-700">
                Hay comprobaciones que no se cumplen: no se puede acordar este dividendo.
              </p>
            ) : (
              <ConfirmAction
                label={
                  kind === 'resultado'
                    ? 'Registrar el acuerdo y crear el acta'
                    : 'Contabilizar el acuerdo y crear el acta'
                }
                question={`¿Acordar el dividendo de ${euros(c.amount)} del ${formatDate(c.date)}? ${
                  c.entry.length > 0 ? 'Se contabiliza en el Diario y ' : ''
                }se crea el acta en borrador en Libros societarios.`}
                confirmLabel="Sí, acordar"
                danger={false}
                className="btn-primary rounded-md px-3 py-1.5"
                onConfirm={async () => {
                  try {
                    await dividendAgreementRecord(build());
                    setC(null);
                    setAmount('');
                    onDone(
                      'Acuerdo registrado. El acta está en borrador en Libros societarios → Libro de actas: revísala y apruébala antes de pagar.',
                    );
                  } catch (e) {
                    setError(errorMessage(e));
                  }
                }}
              />
            )}
          </div>
        </>
      )}
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
    </section>
  );
}

function Agreements({
  year,
  list,
  onChange,
  onMessage,
}: {
  year: number;
  list: DividendAgreement[];
  onChange: () => void;
  onMessage: (m: string) => void;
}) {
  const [adding, setAdding] = useState(false);
  return (
    <section aria-label="Acuerdos de dividendos" className="mt-4">
      <h4 className="font-medium">Acuerdos de reparto</h4>
      <p className="text-stone-700">
        Antes de pagar, registra el acuerdo: TaxPain comprueba la reserva legal, el patrimonio neto
        frente al capital, la I+D pendiente y, en el dividendo a cuenta, el límite y la liquidez del
        art. 277 LSC; si algo no se cumple, no deja seguir. Crea el acta en borrador enlazada.
      </p>
      {list.length > 0 && (
        <ul className="mt-1 space-y-1">
          {list.map((a) => (
            <li key={a.id} className="flex flex-wrap items-center gap-3">
              <span className="tabular-nums">{formatDate(a.date)}</span>
              <span>{AGREEMENT_SHORT[a.kind]}</span>
              <span>
                {euros(a.amount)} · pagado {euros(a.paid)} · pendiente {euros(a.pending)}
              </span>
              <span className="pill">
                {a.minute_id === null
                  ? 'Sin acta'
                  : a.minute_approved
                    ? `Acta nº ${a.minute_number}`
                    : 'Acta en borrador'}
              </span>
              {a.paid === 0 && (
                <ConfirmAction
                  label="Deshacer"
                  question={`¿Deshacer el acuerdo del ${formatDate(a.date)}? Se borra su asiento y, si sigue en borrador, su acta.`}
                  confirmLabel="Sí, deshacer"
                  onConfirm={async () => {
                    try {
                      await dividendAgreementUndo(a.id);
                      onChange();
                    } catch (e) {
                      onMessage(errorMessage(e));
                    }
                  }}
                />
              )}
            </li>
          ))}
        </ul>
      )}
      {adding ? (
        <NewAgreement
          year={year}
          onDone={(m) => {
            setAdding(false);
            onMessage(m);
            onChange();
          }}
        />
      ) : (
        <button type="button" className="mt-1 underline" onClick={() => setAdding(true)}>
          Acordar un dividendo
        </button>
      )}
    </section>
  );
}

/**
 * Socios y pago de dividendos: tras la aplicación del resultado (526 «Dividendo activo a
 * pagar»), reparto por participación con la retención del 19 % (modelos 123 y 193).
 */
export function Dividends({ year }: { year: number }) {
  const [list, setList] = useState<Shareholder[]>([]);
  const [paid, setPaid] = useState<DividendPayment[]>([]);
  const [agreements, setAgreements] = useState<DividendAgreement[]>([]);
  const [agreement, setAgreement] = useState('');
  const [date, setDate] = useState('');
  const [gross, setGross] = useState('');
  const [treasury, setTreasury] = useState('572');
  const [p, setP] = useState<PayoutProposal | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(() => {
    Promise.all([
      shareholdersList(),
      dividendPayments(year + 1),
      dividendPayments(year),
      dividendAgreements(),
    ])
      .then(([s, next, same, a]) => {
        setList(s);
        setPaid([...same, ...next]);
        setAgreements(a);
      })
      .catch((e) => setMsg(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);
  const amount = gross.trim() ? parseEuros(gross) : null;
  const ready = !!date && amount !== null && amount > 0 && treasury.trim() !== '';
  const input_ = () => ({
    date,
    gross: amount ?? 0,
    treasury: treasury.trim(),
    agreement: agreement ? Number(agreement) : null,
  });
  const pendingAgreements = agreements.filter((a) => a.pending > 0);

  return (
    <div className="card p-5">
      <h3 className="font-semibold">Socios y dividendos</h3>
      <p className="mt-2 text-stone-700">
        Con el dividendo acordado en la 526 (con su acuerdo registrado abajo o la aplicación del
        resultado), TaxPain lo reparte por participación, retiene el 19 % (art. 101.4 LIRPF y art.
        128.6 LIS) y contabiliza el pago. La retención se ingresa con el modelo 123 del trimestre y
        cada pago va en el 193 de enero.
      </p>
      <Shareholders list={list} onChange={load} />
      {list.length > 0 && (
        <Agreements year={year} list={agreements} onChange={load} onMessage={setMsg} />
      )}
      {list.length > 0 && (
        <section aria-label="Pagar dividendos" className="mt-4">
          <h4 className="font-medium">Repartir y pagar dividendos</h4>
          <div className="mt-2 flex flex-wrap items-end gap-3">
            <label>
              Fecha de pago acordada
              <input
                type="date"
                className={input}
                value={date}
                onChange={(e) => {
                  setDate(e.target.value);
                  setP(null);
                }}
              />
            </label>
            <label>
              Dividendo bruto total
              <input
                className={`${input} w-36 text-right`}
                inputMode="decimal"
                value={gross}
                onChange={(e) => {
                  setGross(e.target.value);
                  setP(null);
                }}
              />
            </label>
            {pendingAgreements.length > 0 && (
              <label>
                Acuerdo que se paga
                <select
                  className={input}
                  value={agreement}
                  onChange={(e) => {
                    setAgreement(e.target.value);
                    setP(null);
                  }}
                >
                  <option value="">El más antiguo pendiente</option>
                  {pendingAgreements.map((a) => (
                    <option key={a.id} value={a.id}>
                      {formatDate(a.date)} · {AGREEMENT_SHORT[a.kind]} · {euros(a.pending)}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <label>
              Pagado desde la cuenta
              <input
                className={`${input} w-28`}
                inputMode="numeric"
                value={treasury}
                onChange={(e) => {
                  setTreasury(e.target.value);
                  setP(null);
                }}
              />
            </label>
            <button
              type="button"
              className="rounded-md border border-stone-300 px-3 py-1.5"
              disabled={!ready}
              onClick={() => {
                setMsg(null);
                dividendPropose(input_())
                  .then(setP)
                  .catch((e) => {
                    setP(null);
                    setMsg(errorMessage(e));
                  });
              }}
            >
              Calcular el reparto
            </button>
          </div>
          <p className="mt-1 text-stone-600">
            La retención se devenga en la fecha de pago que fija el acuerdo de la junta (o el día
            siguiente al acuerdo si no la fija), aunque se pague después. Cuenta: 57x (banco o caja)
            o 551 si se compensa con la cuenta corriente del socio.
          </p>
          {p && (
            <>
              <p className="mt-3 text-stone-700">
                Pendiente de pago en la 526 a esa fecha: {euros(p.available)}.
                {p.agreement &&
                  ` Se paga con cargo al acuerdo del ${formatDate(p.agreement.date)} (${AGREEMENT_SHORT[p.agreement.kind].toLowerCase()}; pendientes ${euros(p.agreement.pending)}).`}
              </p>
              <Proposal p={p} />
              <div className="mt-3">
                <ConfirmAction
                  label="Contabilizar el pago"
                  question={`¿Contabilizar el pago de ${euros(p.gross)} de dividendos con ${euros(p.withheld)} de retención? Se ingresa con el 123 del ${quarterOf(p.date)}.`}
                  confirmLabel="Sí, contabilizar"
                  danger={false}
                  className="btn-primary rounded-md px-3 py-1.5"
                  onConfirm={async () => {
                    try {
                      await dividendPay(input_());
                      setP(null);
                      setGross('');
                      setMsg('Pago contabilizado. Revisa el borrador del 123 en Modelos.');
                      load();
                    } catch (e) {
                      setMsg(errorMessage(e));
                    }
                  }}
                />
              </div>
            </>
          )}
        </section>
      )}
      {paid.length > 0 && (
        <section aria-label="Dividendos pagados" className="mt-4">
          <h4 className="font-medium">Pagos de dividendos</h4>
          <ul className="mt-1 space-y-1">
            {paid.map((d) => (
              <li key={d.id} className="flex flex-wrap items-center gap-3">
                <span className="tabular-nums">{formatDate(d.date)}</span>
                <span>
                  Bruto {euros(d.gross)} · retención {euros(d.withheld)} · líquido {euros(d.net)}
                </span>
                <ConfirmAction
                  label="Deshacer"
                  question={`¿Quitar el pago del ${formatDate(d.date)} y su asiento? Si ya presentaste el 123, tendrás que corregirlo.`}
                  confirmLabel="Sí, quitarlo"
                  onConfirm={async () => {
                    try {
                      await dividendUndo(d.id);
                      load();
                    } catch (e) {
                      setMsg(errorMessage(e));
                    }
                  }}
                />
              </li>
            ))}
          </ul>
        </section>
      )}
      {msg && (
        <p role="status" className="mt-3">
          {msg}
        </p>
      )}
    </div>
  );
}

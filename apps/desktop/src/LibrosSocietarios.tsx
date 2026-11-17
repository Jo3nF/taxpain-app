import { type FormEvent, useCallback, useEffect, useState } from 'react';
import {
  type CertificateInput,
  type CorporateBook,
  corporateBookSave,
  errorMessage,
  euros,
  formatDate,
  type LiquidityStatement,
  MINUTE_KINDS,
  type Minute,
  type MinuteKind,
  type MinuteTemplate,
  MOVEMENT_KINDS,
  type MovementKind,
  minuteApprove,
  minuteCertificateSave,
  minuteDelete,
  minuteDocumentSave,
  minuteReopen,
  minuteSave,
  minutesList,
  minuteTemplate,
  type PartnersRegister,
  parseEuros,
  partnersRegister,
  partnersRegisterApply,
  type ShareMovement,
  type SoleContract,
  type SoleContractProposals,
  shareMovementDelete,
  shareMovementSave,
  soleContractDelete,
  soleContractProposals,
  soleContractSave,
  soleContracts,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { showPct } from './Dividends';

type Tab = 'actas' | 'socios' | 'contratos';

const TABS: { id: Tab; label: string }[] = [
  { id: 'actas', label: 'Libro de actas' },
  { id: 'socios', label: 'Registro de socios' },
  { id: 'contratos', label: 'Contratos con el socio único' },
];

const cell = 'px-2 py-1 text-right tabular-nums';

/** Importe opcional en euros → céntimos (vacío: null; inválido: undefined). */
function optionalEuros(s: string): number | null | undefined {
  if (!s.trim()) return null;
  const n = parseEuros(s);
  return n === null ? undefined : n;
}

/** Botones «PDF…» y «RTF…» para guardar un libro. */
function BookButtons({
  book,
  year,
  name,
  onMessage,
}: {
  book: CorporateBook;
  year: number | null;
  name: string;
  onMessage: (m: string) => void;
}) {
  const save = async (pdf: boolean) => {
    try {
      const path = await corporateBookSave(book, year, name, pdf);
      if (path) onMessage(`Guardado en ${path}.`);
    } catch (e) {
      onMessage(errorMessage(e));
    }
  };
  return (
    <span className="flex gap-2">
      <button type="button" className="btn-ghost" onClick={() => save(true)}>
        {name} en PDF…
      </button>
      <button type="button" className="btn-ghost" onClick={() => save(false)}>
        RTF…
      </button>
    </span>
  );
}

// ---------- Libro de actas ----------

type TemplateKey = MinuteTemplate['template'];

const TEMPLATES: { id: TemplateKey; label: string }[] = [
  { id: 'cuentas', label: 'Aprobación de cuentas y aplicación del resultado' },
  { id: 'dividendo', label: 'Reparto de dividendo con cargo a reservas' },
  { id: 'dividendo_cuenta', label: 'Dividendo a cuenta (con el estado de liquidez)' },
  { id: 'nombramiento', label: 'Nombramiento de administrador' },
  { id: 'cese', label: 'Cese de administrador' },
  { id: 'retribucion', label: 'Retribución del administrador (art. 217 LSC)' },
  { id: 'domicilio', label: 'Cambio de domicilio social' },
  { id: 'libre', label: 'En blanco' },
];

function LiquidityTable({ l }: { l: LiquidityStatement }) {
  const rows: [string, number][] = [
    [`Resultado del ${formatDate(l.period_start)} al ${formatDate(l.date)}`, l.result],
    ['Pérdidas de ejercicios anteriores', -l.prior_losses],
    ['Reserva legal obligatoria', -l.legal_reserve],
    [`Impuesto estimado (${(l.tax_rate / 100).toLocaleString('es-ES')} %)`, -l.estimated_tax],
    ['Ya repartido a cuenta', -l.already_distributed],
    ['Máximo distribuible (art. 277.b)', l.limit],
    ['Tesorería disponible', l.treasury],
    ['Deudas a corto plazo', l.short_term_debts],
    ['Tesorería tras el pago', l.treasury_after],
  ];
  return (
    <table className="mt-2 w-full max-w-xl">
      <caption className="text-left font-medium">
        Estado contable de liquidez (art. 277 LSC)
      </caption>
      <tbody>
        {rows.map(([label, m]) => (
          <tr key={label} className="border-t border-stone-100">
            <td className="py-1">{label}</td>
            <td className={cell}>{euros(m)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function NewMinute({
  year,
  onDraft,
}: {
  year: number;
  onDraft: (m: Minute, w: string[], l: LiquidityStatement | null) => void;
}) {
  const [key, setKey] = useState<TemplateKey>('cuentas');
  const [date, setDate] = useState('');
  const [f, setF] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const field = (name: string, label: string, type = 'text') => (
    <label>
      {label}
      <input
        type={type}
        value={f[name] ?? ''}
        onChange={(e) => setF({ ...f, [name]: e.target.value })}
      />
    </label>
  );
  function build(): MinuteTemplate | string {
    const money = (n: string) => parseEuros(f[n] ?? '');
    switch (key) {
      case 'cuentas': {
        const y = Number(f.accounts_year || year - 1);
        if (!Number.isInteger(y) || y < 1900) return 'Pon el ejercicio de las cuentas.';
        return { template: 'cuentas', year: y };
      }
      case 'dividendo': {
        const amount = money('amount');
        if (!amount) return 'Pon el importe del dividendo.';
        return { template: 'dividendo', amount, payment_date: f.payment_date || null };
      }
      case 'dividendo_cuenta': {
        const amount = money('amount');
        if (!amount) return 'Pon el importe del dividendo a cuenta.';
        if (!f.statement_date) return 'Pon la fecha del estado contable.';
        const rate = f.tax_rate ? Math.round(Number(f.tax_rate.replace(',', '.')) * 100) : null;
        return {
          template: 'dividendo_cuenta',
          amount,
          payment_date: f.payment_date || null,
          statement_date: f.statement_date,
          tax_rate: rate,
        };
      }
      case 'nombramiento':
        return {
          template: 'nombramiento',
          name: f.name ?? '',
          nif: f.nif ?? '',
          address: f.address ?? '',
          position: f.position ?? '',
        };
      case 'cese':
        return {
          template: 'cese',
          name: f.name ?? '',
          nif: f.nif ?? '',
          position: f.position ?? '',
        };
      case 'retribucion': {
        const max = money('max_amount');
        if (!max) return 'Pon el importe máximo anual.';
        return { template: 'retribucion', year, max_amount: max, concept: f.concept ?? '' };
      }
      case 'domicilio':
        return { template: 'domicilio', address: f.address ?? '' };
      default:
        return { template: 'libre' };
    }
  }
  async function submit(ev: FormEvent) {
    ev.preventDefault();
    setError(null);
    if (!date) return setError('Pon la fecha del acta.');
    const t = build();
    if (typeof t === 'string') return setError(t);
    try {
      const d = await minuteTemplate(t, date);
      onDraft(d.minute, d.warnings, d.liquidity);
    } catch (e) {
      setError(errorMessage(e));
    }
  }
  return (
    <form
      onSubmit={submit}
      aria-label="Nueva acta"
      className="form-grid mt-3 rounded-md border border-stone-200 p-4"
    >
      <label className="span-2">
        Plantilla
        <select value={key} onChange={(e) => setKey(e.target.value as TemplateKey)}>
          {TEMPLATES.map((t) => (
            <option key={t.id} value={t.id}>
              {t.label}
            </option>
          ))}
        </select>
      </label>
      <label>
        Fecha del acta
        <input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
      </label>
      {key === 'cuentas' && (
        <>
          <label>
            Ejercicio de las cuentas
            <input
              type="number"
              value={f.accounts_year ?? String(year - 1)}
              onChange={(e) => setF({ ...f, accounts_year: e.target.value })}
            />
          </label>
          <p className="span-full text-stone-600">
            El ejercicio ha de estar cerrado. Lleva la aplicación del resultado contabilizada en
            Cierre o, si aún no, la que propone TaxPain.
          </p>
        </>
      )}
      {(key === 'dividendo' || key === 'dividendo_cuenta') && (
        <>
          {field('amount', 'Dividendo bruto (€)')}
          {field('payment_date', 'Fecha de pago', 'date')}
        </>
      )}
      {key === 'dividendo_cuenta' && (
        <>
          {field('statement_date', 'Fecha del estado contable', 'date')}
          {field('tax_rate', 'Tipo del IS estimado (%)')}
        </>
      )}
      {(key === 'nombramiento' || key === 'cese') && (
        <>
          {field('name', 'Nombre del administrador')}
          {field('nif', 'NIF')}
          {field('position', 'Cargo (administrador único si se deja vacío)')}
        </>
      )}
      {key === 'nombramiento' && field('address', 'Domicilio')}
      {key === 'retribucion' && (
        <>
          {field('max_amount', `Importe máximo anual ${year} (€)`)}
          {field('concept', 'Sistema (asignación fija si se deja vacío)')}
        </>
      )}
      {key === 'domicilio' && field('address', 'Nuevo domicilio social')}
      <div className="span-full form-actions">
        <button type="submit" className="btn-primary rounded-md px-3 py-1.5">
          Preparar el borrador
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

function MinuteEditor({
  initial,
  warnings,
  liquidity,
  onDone,
}: {
  initial: Minute;
  warnings: string[];
  liquidity: LiquidityStatement | null;
  onDone: (saved: boolean) => void;
}) {
  const [m, setM] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<Minute>) => setM({ ...m, ...patch });
  async function submit(ev: FormEvent) {
    ev.preventDefault();
    try {
      await minuteSave(m);
      onDone(true);
    } catch (e) {
      setError(errorMessage(e));
    }
  }
  return (
    <form
      onSubmit={submit}
      aria-label="Borrador del acta"
      className="form-grid mt-3 rounded-md border border-stone-200 p-4"
    >
      <label>
        Fecha
        <input type="date" value={m.date} onChange={(e) => set({ date: e.target.value })} />
      </label>
      <label>
        Lugar
        <input value={m.place} onChange={(e) => set({ place: e.target.value })} />
      </label>
      <label>
        Tipo de acta
        <select value={m.kind} onChange={(e) => set({ kind: e.target.value as MinuteKind })}>
          {Object.entries(MINUTE_KINDS).map(([k, label]) => (
            <option key={k} value={k}>
              {label}
            </option>
          ))}
        </select>
      </label>
      <label className="span-full">
        Asistentes
        <textarea
          rows={3}
          value={m.attendees}
          onChange={(e) => set({ attendees: e.target.value })}
        />
      </label>
      <label>
        Presidente
        <input value={m.chair} onChange={(e) => set({ chair: e.target.value })} />
      </label>
      <label>
        Secretario
        <input value={m.secretary} onChange={(e) => set({ secretary: e.target.value })} />
      </label>
      <label className="span-full">
        Acuerdos
        <textarea
          rows={14}
          value={m.agreements}
          onChange={(e) => set({ agreements: e.target.value })}
        />
      </label>
      {liquidity && (
        <div className="span-full">
          <LiquidityTable l={liquidity} />
        </div>
      )}
      {warnings.length > 0 && (
        <ul className="span-full space-y-1 rounded-md bg-amber-50 p-3 text-amber-950">
          {warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
      <div className="span-full form-actions">
        <button type="submit" className="btn-primary rounded-md px-3 py-1.5">
          Guardar borrador
        </button>
        <button type="button" className="btn-ghost" onClick={() => onDone(false)}>
          Cancelar
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

function CertificateForm({ m, onMessage }: { m: Minute; onMessage: (s: string) => void }) {
  const [c, setC] = useState<CertificateInput>({ name: '', position: '', place: '', date: m.date });
  const save = async (pdf: boolean) => {
    try {
      const path = await minuteCertificateSave(
        m.id,
        c,
        `Certificación acta ${m.number ?? ''} ${m.fiscal_year}`,
        pdf,
      );
      if (path) onMessage(`Certificación guardada en ${path}.`);
    } catch (e) {
      onMessage(errorMessage(e));
    }
  };
  return (
    <fieldset className="form-grid mt-2 rounded-md border border-stone-200 p-3">
      <legend className="span-full font-medium">Certificación</legend>
      <label>
        Quién certifica
        <input
          placeholder="El administrador"
          value={c.name}
          onChange={(e) => setC({ ...c, name: e.target.value })}
        />
      </label>
      <label>
        Cargo
        <input
          placeholder="administrador único"
          value={c.position}
          onChange={(e) => setC({ ...c, position: e.target.value })}
        />
      </label>
      <label>
        Lugar
        <input value={c.place} onChange={(e) => setC({ ...c, place: e.target.value })} />
      </label>
      <label>
        Fecha de la certificación
        <input type="date" value={c.date} onChange={(e) => setC({ ...c, date: e.target.value })} />
      </label>
      <div className="span-full form-actions">
        <button type="button" className="btn-ghost" onClick={() => save(true)}>
          Certificación en PDF…
        </button>
        <button type="button" className="btn-ghost" onClick={() => save(false)}>
          RTF…
        </button>
      </div>
    </fieldset>
  );
}

function MinutesBook({ year }: { year: number }) {
  const [list, setList] = useState<Minute[]>([]);
  const [msg, setMsg] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<{
    m: Minute;
    w: string[];
    l: LiquidityStatement | null;
  } | null>(null);
  const [certifying, setCertifying] = useState<number | null>(null);
  const load = useCallback(() => {
    minutesList(year)
      .then(setList)
      .catch((e) => setMsg(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);
  const drafts = list.filter((m) => m.status === 'borrador').length;
  return (
    <section aria-label="Libro de actas" className="mt-3">
      <p className="text-stone-700">
        Decisiones del socio único, actas de junta, de consejo y del administrador. Al aprobar un
        acta recibe el número siguiente del ejercicio y entra en el libro, que se legaliza en los
        cuatro meses siguientes al cierre. Son documentos privados: las escrituras y las
        inscripciones siguen pasando por la notaría y el Registro.
      </p>
      <div className="mt-2 flex flex-wrap items-center gap-3">
        <BookButtons book="actas" year={year} name={`Libro de actas ${year}`} onMessage={setMsg} />
        {!creating && !editing && (
          <button type="button" className="underline" onClick={() => setCreating(true)}>
            Nueva acta
          </button>
        )}
      </div>
      {drafts > 0 && (
        <p className="mt-1 text-amber-800">
          {drafts === 1 ? 'Hay un acta en borrador' : `Hay ${drafts} actas en borrador`}: no van en
          el libro hasta que se aprueban.
        </p>
      )}
      {creating && (
        <NewMinute
          year={year}
          onDraft={(m, w, l) => {
            setCreating(false);
            setEditing({ m, w, l });
          }}
        />
      )}
      {editing && (
        <MinuteEditor
          key={editing.m.id}
          initial={editing.m}
          warnings={editing.w}
          liquidity={editing.l}
          onDone={(saved) => {
            setEditing(null);
            if (saved) {
              setMsg('Borrador guardado.');
              load();
            }
          }}
        />
      )}
      {list.length === 0 ? (
        <p className="mt-3 text-stone-600">Aún no hay actas del ejercicio {year}.</p>
      ) : (
        <ul className="mt-3 space-y-2">
          {list.map((m) => (
            <li key={m.id} className="rounded-md border border-stone-200 p-3">
              <div className="flex flex-wrap items-center gap-3">
                <span className="font-medium">
                  {m.number ? `Acta nº ${m.number}` : 'Sin número'}
                </span>
                <span className="tabular-nums">{formatDate(m.date)}</span>
                <span>{MINUTE_KINDS[m.kind]}</span>
                <span className="pill">{m.status === 'aprobada' ? 'Aprobada' : 'Borrador'}</span>
                {m.dividend_agreement && (
                  <span className="pill">Acuerdo de dividendos (Cierre → Socios y dividendos)</span>
                )}
                {m.status === 'borrador' ? (
                  <>
                    <button
                      type="button"
                      className="underline"
                      onClick={() => setEditing({ m, w: [], l: null })}
                    >
                      Editar
                    </button>
                    <ConfirmAction
                      label="Aprobar"
                      danger={false}
                      question={`¿Aprobar el acta del ${formatDate(m.date)}? Entra en el libro de actas de ${m.fiscal_year} con su número y ya no se podrá cambiar sin reabrirla.`}
                      confirmLabel="Sí, aprobar"
                      onConfirm={async () => {
                        const n = await minuteApprove(m.id);
                        setMsg(`Acta aprobada con el nº ${n}.`);
                        load();
                      }}
                    />
                    {m.number === null && (
                      <ConfirmAction
                        label="Borrar"
                        question={`¿Borrar el borrador del ${formatDate(m.date)}?`}
                        confirmLabel="Sí, borrar"
                        onConfirm={async () => {
                          await minuteDelete(m.id);
                          load();
                        }}
                      />
                    )}
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      className="underline"
                      onClick={() => setCertifying(certifying === m.id ? null : m.id)}
                    >
                      Certificación…
                    </button>
                    <ConfirmAction
                      label="Reabrir"
                      question={`¿Volver a borrador el acta nº ${m.number}? Conserva su número. Si ya legalizaste el libro, la corregida no coincidirá con la legalizada: lo correcto suele ser un acta nueva que rectifique.`}
                      confirmLabel="Sí, reabrir"
                      onConfirm={async () => {
                        await minuteReopen(m.id);
                        load();
                      }}
                    />
                  </>
                )}
                <button
                  type="button"
                  className="underline"
                  onClick={async () => {
                    try {
                      const p = await minuteDocumentSave(m.id, `Acta ${m.date}`, true);
                      if (p) setMsg(`Guardada en ${p}.`);
                    } catch (e) {
                      setMsg(errorMessage(e));
                    }
                  }}
                >
                  PDF…
                </button>
              </div>
              <p className="mt-1 line-clamp-2 whitespace-pre-line text-stone-600">{m.agreements}</p>
              {certifying === m.id && <CertificateForm m={m} onMessage={setMsg} />}
            </li>
          ))}
        </ul>
      )}
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}
    </section>
  );
}

// ---------- Registro de socios ----------

const EMPTY_MOVEMENT: ShareMovement = {
  id: 0,
  date: '',
  kind: 'constitucion',
  from_nif: '',
  from_name: '',
  to_nif: '',
  to_name: '',
  to_address: '',
  to_nationality: '',
  number_from: 1,
  number_to: 1,
  unit_nominal: 100,
  price: null,
  charge: '',
  title: '',
  notary: '',
  protocol: '',
  title_date: null,
  notes: '',
};

const hasFrom = (k: MovementKind) => k !== 'constitucion' && k !== 'ampliacion';
const hasTo = (k: MovementKind) => k !== 'reduccion' && k !== 'cancelacion_carga';

function MovementForm({
  initial,
  onDone,
}: {
  initial: ShareMovement;
  onDone: (saved: boolean) => void;
}) {
  const [m, setM] = useState(initial);
  const [nominal, setNominal] = useState(euros(initial.unit_nominal).replace(' €', ''));
  const [price, setPrice] = useState(
    initial.price === null ? '' : euros(initial.price).replace(' €', ''),
  );
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<ShareMovement>) => setM({ ...m, ...patch });
  const text = (key: keyof ShareMovement, label: string, className?: string) => (
    <label className={className}>
      {label}
      <input value={(m[key] as string) ?? ''} onChange={(e) => set({ [key]: e.target.value })} />
    </label>
  );
  async function submit(ev: FormEvent) {
    ev.preventDefault();
    const n = parseEuros(nominal);
    if (!n || n <= 0) return setError('Pon el valor nominal de cada participación.');
    const p = optionalEuros(price);
    if (p === undefined) return setError('El precio no es un importe válido.');
    if (!m.date) return setError('Pon la fecha del movimiento.');
    try {
      await shareMovementSave({ ...m, unit_nominal: n, price: p });
      onDone(true);
    } catch (e) {
      setError(errorMessage(e));
    }
  }
  const from = m.kind === 'transmision' ? 'transmitente' : 'titular';
  const to =
    m.kind === 'carga' ? 'beneficiario' : m.kind === 'transmision' ? 'adquirente' : 'suscriptor';
  return (
    <form
      onSubmit={submit}
      aria-label="Movimiento de participaciones"
      className="form-grid mt-3 rounded-md border border-stone-200 p-4"
    >
      <label>
        Fecha
        <input type="date" value={m.date} onChange={(e) => set({ date: e.target.value })} />
      </label>
      <label className="span-2">
        Movimiento
        <select value={m.kind} onChange={(e) => set({ kind: e.target.value as MovementKind })}>
          {Object.entries(MOVEMENT_KINDS).map(([k, label]) => (
            <option key={k} value={k}>
              {label}
            </option>
          ))}
        </select>
      </label>
      {hasFrom(m.kind) && (
        <>
          {text('from_nif', `NIF del ${from}`)}
          {text('from_name', `Nombre del ${from}`, 'span-2')}
        </>
      )}
      {hasTo(m.kind) && (
        <>
          {text('to_nif', `NIF del ${to}`)}
          {text('to_name', `Nombre del ${to}`, 'span-2')}
          {text('to_address', 'Domicilio', 'span-2')}
          {text('to_nationality', 'Nacionalidad')}
        </>
      )}
      <label>
        Desde el nº
        <input
          type="number"
          min={1}
          value={m.number_from}
          onChange={(e) => set({ number_from: Number(e.target.value) })}
        />
      </label>
      <label>
        Hasta el nº
        <input
          type="number"
          min={1}
          value={m.number_to}
          onChange={(e) => set({ number_to: Number(e.target.value) })}
        />
      </label>
      <label>
        Nominal de cada una (€)
        <input inputMode="decimal" value={nominal} onChange={(e) => setNominal(e.target.value)} />
      </label>
      {m.kind === 'transmision' && (
        <label>
          Precio (€, opcional)
          <input inputMode="decimal" value={price} onChange={(e) => setPrice(e.target.value)} />
        </label>
      )}
      {(m.kind === 'carga' || m.kind === 'cancelacion_carga') &&
        text('charge', 'Carga (prenda, usufructo, embargo…)', 'span-2')}
      {text('title', 'Título (escritura, póliza, documento privado)', 'span-2')}
      {text('notary', 'Notario')}
      {text('protocol', 'Nº de protocolo')}
      <label>
        Fecha del título
        <input
          type="date"
          value={m.title_date ?? ''}
          onChange={(e) => set({ title_date: e.target.value || null })}
        />
      </label>
      {text('notes', 'Notas', 'span-full')}
      <div className="span-full form-actions">
        <button type="submit" className="btn-primary rounded-md px-3 py-1.5">
          Guardar movimiento
        </button>
        <button type="button" className="btn-ghost" onClick={() => onDone(false)}>
          Cancelar
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

function PartnersBook({ year }: { year: number }) {
  const [r, setR] = useState<PartnersRegister | null>(null);
  const [editing, setEditing] = useState<ShareMovement | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(() => {
    partnersRegister()
      .then(setR)
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  useEffect(load, [load]);
  const range = (a: number, b: number) => (a === b ? `${a}` : `${a} a ${b}`);
  return (
    <section aria-label="Registro de socios" className="mt-3">
      <p className="text-stone-700">
        Titularidad originaria, transmisiones y cargas de las participaciones (arts. 104 a 106 LSC),
        en orden de fechas. TaxPain comprueba cada movimiento contra lo anterior y saca la situación
        actual, que coteja con la ficha de socios del reparto de dividendos.
      </p>
      <div className="mt-2 flex flex-wrap items-center gap-3">
        <BookButtons book="socios" year={null} name="Libro registro de socios" onMessage={setMsg} />
        <BookButtons
          book="socios"
          year={year}
          name={`Registro de socios ${year}`}
          onMessage={setMsg}
        />
      </div>
      {r && r.situation.holders.length > 0 && (
        <table className="mt-3 w-full">
          <caption className="text-left font-medium">
            Situación actual: {r.situation.count.toLocaleString('es-ES')} participaciones, capital{' '}
            {euros(r.situation.capital)}
          </caption>
          <thead className="text-left text-stone-500">
            <tr>
              <th className="font-normal">Socio</th>
              <th className="font-normal">Numeración</th>
              <th className="px-2 text-right font-normal">Participaciones</th>
              <th className="px-2 text-right font-normal">Nominal</th>
              <th className="px-2 text-right font-normal">%</th>
            </tr>
          </thead>
          <tbody>
            {r.situation.holders.map((h) => (
              <tr key={h.nif} className="border-t border-stone-100 align-top">
                <td className="py-1">
                  {h.name} <span className="text-stone-500">{h.nif}</span>
                </td>
                <td className="py-1">
                  {h.blocks.map((b) => (
                    <span key={b.from} className="block">
                      {range(b.from, b.to)}
                      {b.charges.map((c) => (
                        <span key={c.charge + c.date} className="pill ml-1">
                          {c.charge} ({c.beneficiary_name})
                        </span>
                      ))}
                    </span>
                  ))}
                </td>
                <td className={cell}>{h.count.toLocaleString('es-ES')}</td>
                <td className={cell}>{euros(h.nominal)}</td>
                <td className={cell}>{showPct(h.pct)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {r && r.checks.length > 0 && (
        <ul className="mt-3 space-y-1 rounded-md bg-amber-50 p-3 text-amber-950">
          {r.checks.map((c) => (
            <li key={c}>{c}</li>
          ))}
        </ul>
      )}
      {r && r.movements.length > 0 && !r.in_sync && (
        <div className="mt-2">
          <ConfirmAction
            label="Pasar los porcentajes a la ficha de socios"
            danger={false}
            className="rounded-md border border-stone-300 px-3 py-1.5"
            question="¿Actualizar la ficha de socios con los porcentajes del libro? Los socios que ya no tienen participaciones se dan de baja; a los que faltan hay que darlos de alta a mano."
            confirmLabel="Sí, actualizar"
            onConfirm={async () => {
              const n = await partnersRegisterApply();
              setMsg(n === 1 ? 'Un socio actualizado.' : `${n} socios actualizados.`);
              load();
            }}
          />
        </div>
      )}
      <h4 className="mt-4 font-medium">Movimientos</h4>
      {r && r.movements.length === 0 && (
        <p className="text-stone-600">
          El libro está vacío: empieza por la constitución (los fundadores y su numeración).
        </p>
      )}
      {r && r.movements.length > 0 && (
        <table className="mt-1 w-full">
          <caption className="sr-only">Movimientos de participaciones</caption>
          <thead className="text-left text-stone-500">
            <tr>
              <th className="font-normal">Fecha</th>
              <th className="font-normal">Movimiento</th>
              <th className="font-normal">De</th>
              <th className="font-normal">A</th>
              <th className="font-normal">Números</th>
              <th className="font-normal">Título</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {r.movements.map((m) => (
              <tr key={m.id} className="border-t border-stone-100">
                <td className="py-1 tabular-nums">{formatDate(m.date)}</td>
                <td>{MOVEMENT_KINDS[m.kind]}</td>
                <td>{m.from_name}</td>
                <td>{m.to_name}</td>
                <td className="tabular-nums">{range(m.number_from, m.number_to)}</td>
                <td>{[m.title, m.notary, m.protocol].filter(Boolean).join(' · ')}</td>
                <td className="flex gap-2">
                  <button type="button" className="underline" onClick={() => setEditing(m)}>
                    Editar
                  </button>
                  <ConfirmAction
                    label="Borrar"
                    question={`¿Borrar el movimiento del ${formatDate(m.date)}?`}
                    confirmLabel="Sí, borrar"
                    onConfirm={async () => {
                      await shareMovementDelete(m.id);
                      load();
                    }}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {editing ? (
        <MovementForm
          key={editing.id}
          initial={editing}
          onDone={(saved) => {
            setEditing(null);
            if (saved) load();
          }}
        />
      ) : (
        <button type="button" className="mt-2 underline" onClick={() => setEditing(EMPTY_MOVEMENT)}>
          Añadir movimiento
        </button>
      )}
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}
    </section>
  );
}

// ---------- Contratos con el socio único ----------

const EMPTY_CONTRACT: SoleContract = {
  id: 0,
  date: '',
  object: '',
  amount: null,
  conditions: '',
  document: '',
  source: null,
};

function ContractForm({
  initial,
  onDone,
}: {
  initial: SoleContract;
  onDone: (saved: boolean) => void;
}) {
  const [c, setC] = useState(initial);
  const [amount, setAmount] = useState(
    initial.amount === null ? '' : euros(initial.amount).replace(' €', ''),
  );
  const [error, setError] = useState<string | null>(null);
  async function submit(ev: FormEvent) {
    ev.preventDefault();
    const a = optionalEuros(amount);
    if (a === undefined) return setError('El importe no es válido.');
    if (!c.date) return setError('Pon la fecha del contrato.');
    try {
      await soleContractSave({ ...c, amount: a });
      onDone(true);
    } catch (e) {
      setError(errorMessage(e));
    }
  }
  return (
    <form
      onSubmit={submit}
      aria-label="Contrato con el socio único"
      className="form-grid mt-3 rounded-md border border-stone-200 p-4"
    >
      <label>
        Fecha
        <input type="date" value={c.date} onChange={(e) => setC({ ...c, date: e.target.value })} />
      </label>
      <label className="span-2">
        Objeto
        <input value={c.object} onChange={(e) => setC({ ...c, object: e.target.value })} />
      </label>
      <label>
        Importe (€)
        <input inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} />
      </label>
      <label className="span-full">
        Condiciones
        <textarea
          rows={3}
          value={c.conditions}
          onChange={(e) => setC({ ...c, conditions: e.target.value })}
        />
      </label>
      <label className="span-full">
        Documento en que consta
        <input value={c.document} onChange={(e) => setC({ ...c, document: e.target.value })} />
      </label>
      <div className="span-full form-actions">
        <button type="submit" className="btn-primary rounded-md px-3 py-1.5">
          Transcribir al libro
        </button>
        <button type="button" className="btn-ghost" onClick={() => onDone(false)}>
          Cancelar
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

function ContractsBook({ year }: { year: number }) {
  const [list, setList] = useState<SoleContract[]>([]);
  const [p, setP] = useState<SoleContractProposals | null>(null);
  const [editing, setEditing] = useState<SoleContract | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(() => {
    Promise.all([soleContracts(), soleContractProposals(year)])
      .then(([l, pr]) => {
        setList(l);
        setP(pr);
      })
      .catch((e) => setMsg(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);
  return (
    <section aria-label="Contratos con el socio único" className="mt-3">
      <p className="text-stone-700">
        Los contratos entre el socio único y la sociedad se transcriben a este libro y se citan en
        la memoria (art. 16 LSC): si no, en un concurso no se pueden oponer a la masa.
      </p>
      {p && !p.sole_partner && (
        <p className="mt-1 text-amber-800">
          Según la ficha de socios la sociedad no es unipersonal: este libro es de las sociedades
          con un socio único.
        </p>
      )}
      {p?.sole_partner && (
        <p className="mt-1">
          Socio único: {p.sole_partner[1]}{' '}
          <span className="text-stone-500">{p.sole_partner[0]}</span>
        </p>
      )}
      <div className="mt-2 flex flex-wrap items-center gap-3">
        <BookButtons
          book="contratos"
          year={year}
          name={`Contratos del socio único ${year}`}
          onMessage={setMsg}
        />
      </div>
      {p && p.proposals.length > 0 && (
        <section aria-label="Propuestas" className="mt-3 rounded-md bg-sky-50 p-3">
          <h4 className="font-medium">Para transcribir (lo que TaxPain ya sabe de {year})</h4>
          <ul className="mt-1 space-y-1">
            {p.proposals.map((c) => (
              <li key={c.source ?? c.object} className="flex flex-wrap items-center gap-3">
                <span className="tabular-nums">{formatDate(c.date)}</span>
                <span>{c.object}</span>
                {c.amount !== null && <span className="tabular-nums">{euros(c.amount)}</span>}
                <button type="button" className="underline" onClick={() => setEditing(c)}>
                  Revisar y transcribir
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      {list.length === 0 ? (
        <p className="mt-3 text-stone-600">Aún no hay contratos transcritos.</p>
      ) : (
        <table className="mt-3 w-full">
          <caption className="sr-only">Contratos transcritos</caption>
          <thead className="text-left text-stone-500">
            <tr>
              <th className="font-normal">Fecha</th>
              <th className="font-normal">Objeto</th>
              <th className="px-2 text-right font-normal">Importe</th>
              <th className="font-normal">Documento</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {list.map((c) => (
              <tr key={c.id} className="border-t border-stone-100">
                <td className="py-1 tabular-nums">{formatDate(c.date)}</td>
                <td>{c.object}</td>
                <td className={cell}>{c.amount === null ? '' : euros(c.amount)}</td>
                <td>{c.document}</td>
                <td className="flex gap-2">
                  <button type="button" className="underline" onClick={() => setEditing(c)}>
                    Editar
                  </button>
                  <ConfirmAction
                    label="Borrar"
                    question={`¿Quitar del libro el contrato del ${formatDate(c.date)}?`}
                    confirmLabel="Sí, quitar"
                    onConfirm={async () => {
                      await soleContractDelete(c.id);
                      load();
                    }}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {editing ? (
        <ContractForm
          key={`${editing.id}-${editing.source ?? ''}`}
          initial={editing}
          onDone={(saved) => {
            setEditing(null);
            if (saved) load();
          }}
        />
      ) : (
        <button type="button" className="mt-2 underline" onClick={() => setEditing(EMPTY_CONTRACT)}>
          Añadir contrato
        </button>
      )}
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}
    </section>
  );
}

/** Libros societarios: actas, registro de socios y contratos con el socio único. */
export function LibrosSocietarios({ year }: { year: number }) {
  const [tab, setTab] = useState<Tab>('actas');
  return (
    <div className="card mt-4 p-5 text-sm">
      <h3 className="font-semibold">Libros societarios</h3>
      <nav className="mt-2 flex gap-1" aria-label="Libros societarios">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            aria-current={tab === t.id ? 'page' : undefined}
            onClick={() => setTab(t.id)}
            className={`rounded px-2 py-1 ${tab === t.id ? 'bg-stone-200 font-medium' : 'text-stone-600'}`}
          >
            {t.label}
          </button>
        ))}
      </nav>
      {tab === 'actas' && <MinutesBook year={year} />}
      {tab === 'socios' && <PartnersBook year={year} />}
      {tab === 'contratos' && <ContractsBook year={year} />}
    </div>
  );
}

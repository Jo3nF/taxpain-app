import { type FormEvent, Fragment, useCallback, useEffect, useState } from 'react';
import { COUNTRIES, errorMessage, euros, formatDate, parseEuros } from './api';
import { ConfirmAction } from './ConfirmAction';
import { ProvinceCodeSelect } from './Controls';
import {
  CLAVES_232,
  CONCEPTO_LABEL,
  type CuentaVinculada,
  METODO_LABEL,
  type Metodo,
  type OperacionManual,
  type OperacionVinculada,
  type Participada,
  type PersonaVinculada,
  parsePct,
  participadaDelete,
  participadaSave,
  participadasList,
  type RegistroVinculadas,
  showPct,
  VINCULO_LABEL,
  type Vinculada,
  type Vinculo,
  vinculadaCuentaSet,
  vinculadaDe,
  vinculadaDelete,
  vinculadaOperacionDelete,
  vinculadaOperacionSave,
  vinculadaSave,
  vinculadasDocumentacionSave,
  vinculadasHoja232Save,
  vinculadasRegistro,
  vinculadasTextosSave,
  vinculadaValoracionSave,
} from './vinculadasApi';

const cell = 'px-2 py-1 text-right tabular-nums';

const emptyVinculada = (nif = '', name = '', country = 'ES'): Vinculada => ({
  id: 0,
  nif,
  name,
  legal_person: /^[A-HJ-NP-SUVW]/i.test(nif.trim()),
  province: '',
  country,
  vinculo: 'familiar',
  detalle: '',
  pct: null,
  grupo_fiscal: false,
  clave_emitidas: 6,
  clave_recibidas: 6,
  active: true,
});

const emptyParticipada = (): Participada => ({
  id: 0,
  nif: '',
  name: '',
  province: '',
  country: 'ES',
  pct: 0,
  acquired: '',
  cost: 0,
  account: '',
  grupo_fiscal: false,
  notes: '',
  active: true,
});

function ClaveSelect({
  value,
  onChange,
  label,
}: {
  value: number;
  onChange: (c: number) => void;
  label: string;
}) {
  return (
    <label>
      {label}
      <select value={value} onChange={(e) => onChange(Number(e.target.value))}>
        {CLAVES_232.map(([c, l]) => (
          <option key={c} value={c}>
            {c}. {l}
          </option>
        ))}
      </select>
    </label>
  );
}

/** Ficha de una vinculada tecleada (o datos añadidos a un socio, administrador o participada). */
export function VinculadaForm({
  initial,
  onSaved,
  onCancel,
}: {
  initial: Vinculada;
  onSaved: () => void;
  onCancel?: () => void;
}) {
  const [x, setX] = useState(initial);
  const [pct, setPct] = useState(initial.pct ? showPct(initial.pct) : '');
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<Vinculada>) => setX({ ...x, ...patch });
  const resident = x.country === 'ES';
  async function submit(ev: FormEvent) {
    ev.preventDefault();
    const p = pct.trim() ? parsePct(pct) : null;
    if (pct.trim() && p === null) return setError('La participación va de 0,0001 % a 100 %.');
    try {
      await vinculadaSave({ ...x, pct: p });
      onSaved();
    } catch (e) {
      setError(errorMessage(e));
    }
  }
  return (
    <form
      onSubmit={submit}
      aria-label="Ficha de vinculada"
      className="form-grid mt-3 rounded-md border border-stone-200 p-4"
    >
      <label>
        NIF
        <input value={x.nif} onChange={(e) => set({ nif: e.target.value })} />
      </label>
      <label className="span-2">
        Apellidos y nombre o razón social
        <input value={x.name} onChange={(e) => set({ name: e.target.value })} />
      </label>
      <label className="span-2">
        Vínculo (art. 18.2 LIS)
        <select value={x.vinculo} onChange={(e) => set({ vinculo: e.target.value as Vinculo })}>
          {(Object.keys(VINCULO_LABEL) as Vinculo[]).map((v) => (
            <option key={v} value={v}>
              {VINCULO_LABEL[v]}
            </option>
          ))}
        </select>
      </label>
      <label className="span-2">
        Detalle (p. ej. «cónyuge de la socia»)
        <input value={x.detalle} onChange={(e) => set({ detalle: e.target.value })} />
      </label>
      <label>
        Participación (%), si la hay
        <input
          className="text-right"
          inputMode="decimal"
          value={pct}
          onChange={(e) => setPct(e.target.value)}
        />
      </label>
      {resident ? (
        <label className="span-2">
          Provincia
          <ProvinceCodeSelect value={x.province} onChange={(province) => set({ province })} />
        </label>
      ) : (
        <label className="span-2">
          País de residencia (código, p. ej. FR)
          <input
            maxLength={2}
            list="vinculadas-paises"
            value={x.country}
            onChange={(e) => set({ country: e.target.value.trim().toUpperCase() })}
          />
          <datalist id="vinculadas-paises">
            {COUNTRIES.filter(([c]) => c !== 'ES' && c !== 'XI').map(([code, name]) => (
              <option key={code} value={code}>
                {name}
              </option>
            ))}
          </datalist>
        </label>
      )}
      <ClaveSelect
        label="Clave de sus facturas emitidas"
        value={x.clave_emitidas}
        onChange={(clave_emitidas) => set({ clave_emitidas })}
      />
      <ClaveSelect
        label="Clave de sus facturas recibidas"
        value={x.clave_recibidas}
        onChange={(clave_recibidas) => set({ clave_recibidas })}
      />
      <div className="span-full flex flex-wrap gap-x-6 gap-y-2">
        <label className="flex items-start gap-2">
          <input
            type="checkbox"
            checked={x.legal_person}
            onChange={(e) => set({ legal_person: e.target.checked })}
          />
          Es una sociedad o entidad
        </label>
        <label className="flex items-start gap-2">
          <input
            type="checkbox"
            checked={!resident}
            onChange={(e) => set({ country: e.target.checked ? '' : 'ES', province: '' })}
          />
          No residente en España
        </label>
        <label className="flex items-start gap-2">
          <input
            type="checkbox"
            checked={x.grupo_fiscal}
            onChange={(e) => set({ grupo_fiscal: e.target.checked })}
          />
          Del mismo grupo de consolidación fiscal (o AIE/UTE): no va al 232
        </label>
      </div>
      <div className="span-full form-actions">
        <button type="submit" className="btn-primary rounded-md px-3 py-1.5">
          Guardar vinculada
        </button>
        {onCancel && (
          <button type="button" className="btn-ghost" onClick={onCancel}>
            Cancelar
          </button>
        )}
        {error && (
          <p role="alert" className="text-red-700">
            {error}
          </p>
        )}
      </div>
    </form>
  );
}

/** En la ficha de un cliente o proveedor: si es vinculada y con qué vínculo. */
export function VinculoContacto({
  taxId,
  name,
  country,
}: {
  taxId: string;
  name: string;
  country: string;
}) {
  const [ficha, setFicha] = useState<Vinculada | null | undefined>(undefined);
  const [editing, setEditing] = useState(false);
  const load = useCallback(() => {
    Promise.resolve()
      .then(() => vinculadaDe(taxId))
      .then((f) => setFicha(f ?? null))
      .catch(() => setFicha(undefined));
  }, [taxId]);
  useEffect(load, [load]);
  if (ficha === undefined) return null;
  if (editing) {
    return (
      <VinculadaForm
        initial={ficha ?? emptyVinculada(taxId, name, country === 'ES' ? 'ES' : country)}
        onSaved={() => {
          setEditing(false);
          load();
        }}
        onCancel={() => setEditing(false)}
      />
    );
  }
  return (
    <p className="flex flex-wrap items-center gap-2 text-sm">
      {ficha ? (
        <>
          <span className="pill">Vinculada</span>
          <span>{VINCULO_LABEL[ficha.vinculo]}</span>
          {ficha.detalle && <span className="text-stone-600">({ficha.detalle})</span>}
        </>
      ) : (
        <span className="text-stone-600">No está marcada como persona o entidad vinculada.</span>
      )}
      <button type="button" className="underline" onClick={() => setEditing(true)}>
        {ficha ? 'Cambiar vínculo' : 'Marcar como vinculada'}
      </button>
    </p>
  );
}

function origenDe(p: PersonaVinculada): string {
  const o: string[] = [];
  if (p.socio) o.push('Socios');
  if (p.administrador) o.push('Nóminas (administrador)');
  if (p.participada) o.push('Participadas');
  if (p.ficha) o.push('A mano');
  return o.join(', ');
}

function Personas({ r, onChange }: { r: RegistroVinculadas; onChange: () => void }) {
  const [editing, setEditing] = useState<Vinculada | null>(null);
  return (
    <section aria-labelledby="vinc-personas" className="mt-4">
      <h4 id="vinc-personas" className="font-medium">
        Personas y entidades vinculadas
      </h4>
      <p className="text-sm text-stone-600">
        Los socios con el 25 % o más, los administradores de Nóminas y las participadas con el 25 %
        o más ya cuentan. Añade los familiares, las entidades del grupo y los demás.
      </p>
      {r.personas.length === 0 ? (
        <p className="mt-1 text-stone-600">No hay ninguna vinculada.</p>
      ) : (
        <table className="mt-2 w-full text-sm">
          <thead>
            <tr className="text-left text-stone-500">
              <th className="font-normal">NIF</th>
              <th className="font-normal">Nombre</th>
              <th className="font-normal">Vínculo</th>
              <th className="font-normal">De</th>
              <th className="text-right font-normal">Operaciones del año</th>
              <th className="font-normal">
                <span className="sr-only">Acciones</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {r.personas.map((p) => (
              <tr key={p.nif} className="border-t border-stone-100 align-top">
                <td className="py-1 font-mono">{p.nif}</td>
                <td>
                  {p.name}
                  {p.detalle && <span className="text-stone-600"> · {p.detalle}</span>}
                  {p.grupo_fiscal && <span className="pill ml-1">Grupo fiscal</span>}
                </td>
                <td>
                  {p.vinculos.map((v) => VINCULO_LABEL[v]).join('; ')}
                  {p.pct !== null && ` · ${showPct(p.pct)} %`}
                </td>
                <td>{origenDe(p)}</td>
                <td className={cell}>
                  {euros(p.total)}
                  {p.supera_umbral && <span className="pill ml-1">Más de 250.000 €</span>}
                </td>
                <td className="space-x-2 whitespace-nowrap">
                  <button
                    type="button"
                    className="underline"
                    onClick={() =>
                      setEditing(
                        p.ficha ?? {
                          ...emptyVinculada(p.nif, p.name, p.country),
                          legal_person: p.legal_person,
                          province: p.province,
                          vinculo: p.vinculos[0] ?? 'familiar',
                          pct: p.pct,
                        },
                      )
                    }
                  >
                    {p.ficha ? 'Editar' : 'Añadir datos'}
                  </button>
                  {p.ficha && (
                    <ConfirmAction
                      label="Quitar"
                      ariaLabel={`Quitar a ${p.name} de las vinculadas`}
                      question={`¿Quitar la ficha de ${p.name}? Si es socio, administrador o participada sigue contando como vinculada.`}
                      confirmLabel="Sí, quitar"
                      onConfirm={async () => {
                        await vinculadaDelete(p.ficha?.id ?? 0);
                        onChange();
                      }}
                    />
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {editing ? (
        <VinculadaForm
          key={editing.nif || 'nueva'}
          initial={editing}
          onSaved={() => {
            setEditing(null);
            onChange();
          }}
          onCancel={() => setEditing(null)}
        />
      ) : (
        <button
          type="button"
          className="mt-2 rounded-md border border-stone-300 px-3 py-1"
          onClick={() => setEditing(emptyVinculada())}
        >
          Añadir vinculada
        </button>
      )}
    </section>
  );
}

function ParticipadaForm({ initial, onSaved }: { initial: Participada; onSaved: () => void }) {
  const [x, setX] = useState(initial);
  const [pct, setPct] = useState(initial.pct ? showPct(initial.pct) : '');
  const [cost, setCost] = useState(initial.cost ? (initial.cost / 100).toFixed(2) : '');
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<Participada>) => setX({ ...x, ...patch });
  async function submit(ev: FormEvent) {
    ev.preventDefault();
    const p = parsePct(pct);
    if (p === null) return setError('La participación va de 0,0001 % a 100 %.');
    const c = parseEuros(cost || '0');
    if (c === null || c < 0) return setError('El coste es un importe en euros.');
    if (!x.acquired) return setError('Pon la fecha de adquisición.');
    try {
      await participadaSave({ ...x, pct: p, cost: c });
      onSaved();
    } catch (e) {
      setError(errorMessage(e));
    }
  }
  return (
    <form
      onSubmit={submit}
      aria-label="Ficha de participada"
      className="form-grid mt-3 rounded-md border border-stone-200 p-4"
    >
      <label>
        NIF
        <input value={x.nif} onChange={(e) => set({ nif: e.target.value })} />
      </label>
      <label className="span-2">
        Razón social
        <input value={x.name} onChange={(e) => set({ name: e.target.value })} />
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
      <label>
        Fecha de adquisición
        <input type="date" value={x.acquired} onChange={(e) => set({ acquired: e.target.value })} />
      </label>
      <label>
        Coste (€)
        <input
          className="text-right"
          inputMode="decimal"
          value={cost}
          onChange={(e) => setCost(e.target.value)}
        />
      </label>
      <label>
        Subcuenta (2403, 2404…)
        <input value={x.account} onChange={(e) => set({ account: e.target.value.trim() })} />
      </label>
      <label className="span-2">
        Provincia
        <ProvinceCodeSelect value={x.province} onChange={(province) => set({ province })} />
      </label>
      <div className="span-full flex flex-wrap gap-x-6 gap-y-2">
        <label className="flex items-start gap-2">
          <input
            type="checkbox"
            checked={x.grupo_fiscal}
            onChange={(e) => set({ grupo_fiscal: e.target.checked })}
          />
          Del mismo grupo de consolidación fiscal
        </label>
        <label className="flex items-start gap-2">
          <input
            type="checkbox"
            checked={x.active}
            onChange={(e) => set({ active: e.target.checked })}
          />
          Se conserva la participación
        </label>
      </div>
      <div className="span-full form-actions">
        <button type="submit" className="btn-primary rounded-md px-3 py-1.5">
          Guardar participada
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

function Participadas({ onChange }: { onChange: () => void }) {
  const [list, setList] = useState<Participada[]>([]);
  const [editing, setEditing] = useState<Participada | null>(null);
  const load = useCallback(() => {
    participadasList()
      .then((l) => setList(l ?? []))
      .catch(() => setList([]));
  }, []);
  useEffect(load, [load]);
  const changed = () => {
    setEditing(null);
    load();
    onChange();
  };
  return (
    <section aria-labelledby="vinc-participadas" className="mt-6">
      <h4 id="vinc-participadas" className="font-medium">
        Participadas
      </h4>
      {list.length === 0 ? (
        <p className="text-sm text-stone-600">
          Ninguna. Si la sociedad tiene participaciones en otras, añádelas: con el 25 % o más son
          vinculadas.
        </p>
      ) : (
        <ul className="mt-1 space-y-1 text-sm">
          {list.map((p) => (
            <li key={p.id} className="flex flex-wrap items-center gap-3">
              <span className="font-mono">{p.nif}</span>
              <span>{p.name}</span>
              <span className="tabular-nums">{showPct(p.pct)} %</span>
              <span>desde {formatDate(p.acquired)}</span>
              <span className="tabular-nums">coste {euros(p.cost)}</span>
              {p.account && <span className="font-mono">{p.account}</span>}
              {p.grupo_fiscal && <span className="pill">Grupo fiscal</span>}
              {!p.active && <span className="pill">Vendida</span>}
              <button type="button" className="underline" onClick={() => setEditing(p)}>
                Editar
              </button>
              <ConfirmAction
                label="Quitar"
                ariaLabel={`Quitar la participada ${p.name}`}
                question={`¿Quitar ${p.name} de las participadas?`}
                confirmLabel="Sí, quitar"
                onConfirm={async () => {
                  await participadaDelete(p.id);
                  changed();
                }}
              />
            </li>
          ))}
        </ul>
      )}
      {editing ? (
        <ParticipadaForm key={editing.id} initial={editing} onSaved={changed} />
      ) : (
        <button
          type="button"
          className="mt-2 rounded-md border border-stone-300 px-3 py-1"
          onClick={() => setEditing(emptyParticipada())}
        >
          Añadir participada
        </button>
      )}
    </section>
  );
}

function ValoracionForm({
  year,
  o,
  onSaved,
}: {
  year: number;
  o: OperacionVinculada;
  onSaved: () => void;
}) {
  const [metodo, setMetodo] = useState<Metodo>(o.metodo);
  const [descripcion, setDescripcion] = useState(o.descripcion);
  const [justificacion, setJustificacion] = useState(o.justificacion);
  const [error, setError] = useState<string | null>(null);
  async function submit(ev: FormEvent) {
    ev.preventDefault();
    try {
      await vinculadaValoracionSave({
        year,
        nif: o.nif,
        concepto: o.concepto,
        ingreso: o.ingreso,
        metodo,
        descripcion,
        justificacion,
      });
      onSaved();
    } catch (e) {
      setError(errorMessage(e));
    }
  }
  return (
    <form onSubmit={submit} aria-label="Valoración" className="form-grid p-2">
      <label className="span-2">
        Método de valoración (art. 18.4 LIS)
        <select value={metodo} onChange={(e) => setMetodo(e.target.value as Metodo)}>
          {(Object.keys(METODO_LABEL) as Metodo[]).map((m) => (
            <option key={m} value={m}>
              {METODO_LABEL[m]}
            </option>
          ))}
        </select>
      </label>
      <label className="span-full">
        Naturaleza y características de la operación
        <textarea rows={2} value={descripcion} onChange={(e) => setDescripcion(e.target.value)} />
      </label>
      <label className="span-full">
        Comparables y valor o intervalo de mercado (justificación)
        <textarea
          rows={3}
          value={justificacion}
          onChange={(e) => setJustificacion(e.target.value)}
        />
      </label>
      <div className="span-full form-actions">
        <button type="submit" className="btn-primary rounded-md px-3 py-1">
          Guardar valoración
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

function Operaciones({ r, onChange }: { r: RegistroVinculadas; onChange: () => void }) {
  const [open, setOpen] = useState<string | null>(null);
  const k = (o: OperacionVinculada) => `${o.nif}-${o.concepto}-${o.ingreso}-${o.especifica}`;
  return (
    <section aria-labelledby="vinc-ops" className="mt-6">
      <h4 id="vinc-ops" className="font-medium">
        Operaciones del ejercicio
      </h4>
      {r.operaciones.length === 0 ? (
        <p className="text-sm text-stone-600">
          No hay operaciones con vinculadas en facturas, nóminas, dividendos ni en las subcuentas
          asignadas.
        </p>
      ) : (
        <table className="mt-2 w-full text-sm">
          <thead>
            <tr className="text-left text-stone-500">
              <th className="font-normal">Con</th>
              <th className="font-normal">Clave</th>
              <th className="font-normal">Operación</th>
              <th className="font-normal">Ingreso o pago</th>
              <th className="text-right font-normal">Importe sin IVA</th>
              <th className="font-normal">Método</th>
              <th className="font-normal">232</th>
              <th className="font-normal">
                <span className="sr-only">Acciones</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {r.operaciones.map((o) => (
              <Fragment key={k(o)}>
                <tr className="border-t border-stone-100 align-top">
                  <td className="py-1">{o.name}</td>
                  <td className="tabular-nums">{o.clave ?? '—'}</td>
                  <td>
                    {CONCEPTO_LABEL[o.concepto]}
                    {o.especifica && <span className="pill ml-1">Específica</span>}
                    <span className="block text-xs text-stone-500">{o.origen.join(', ')}</span>
                  </td>
                  <td>{o.ingreso ? 'Ingreso' : 'Pago'}</td>
                  <td className={cell}>{euros(o.importe)}</td>
                  <td>
                    {METODO_LABEL[o.metodo]}
                    {!o.metodo_elegido && o.excluida === null && (
                      <span className="block text-xs text-stone-500">por defecto</span>
                    )}
                  </td>
                  <td>
                    {o.excluida !== null ? (
                      <span className="text-stone-600" title={o.excluida}>
                        No va ({o.excluida})
                      </span>
                    ) : o.en_232 ? (
                      <span className="pill">Sí</span>
                    ) : (
                      <span className="text-stone-600">No llega</span>
                    )}
                  </td>
                  <td>
                    {o.excluida === null && (
                      <button
                        type="button"
                        className="underline"
                        aria-expanded={open === k(o)}
                        onClick={() => setOpen(open === k(o) ? null : k(o))}
                      >
                        Valoración
                      </button>
                    )}
                  </td>
                </tr>
                {open === k(o) && (
                  <tr>
                    <td colSpan={8}>
                      <ValoracionForm
                        year={r.year}
                        o={o}
                        onSaved={() => {
                          setOpen(null);
                          onChange();
                        }}
                      />
                    </td>
                  </tr>
                )}
              </Fragment>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

function Manuales({ r, onChange }: { r: RegistroVinculadas; onChange: () => void }) {
  const [nif, setNif] = useState('');
  const [clave, setClave] = useState(1);
  const [ingreso, setIngreso] = useState(false);
  const [amount, setAmount] = useState('');
  const [especifica, setEspecifica] = useState(false);
  const [description, setDescription] = useState('');
  const [error, setError] = useState<string | null>(null);
  const name = (n: string) => r.personas.find((p) => p.nif === n)?.name ?? n;
  async function submit(ev: FormEvent) {
    ev.preventDefault();
    const a = parseEuros(amount);
    if (!nif) return setError('Elige con quién.');
    if (a === null || a <= 0) return setError('El importe es un número en euros mayor que cero.');
    const op: OperacionManual = {
      id: 0,
      year: r.year,
      nif,
      clave,
      ingreso,
      amount: a,
      especifica,
      description,
    };
    try {
      await vinculadaOperacionSave(op);
      setAmount('');
      setDescription('');
      setError(null);
      onChange();
    } catch (e) {
      setError(errorMessage(e));
    }
  }
  return (
    <section aria-labelledby="vinc-manuales" className="mt-6">
      <h4 id="vinc-manuales" className="font-medium">
        Operaciones que TaxPain no ve
      </h4>
      <p className="text-sm text-stone-600">
        Compraventa de un inmueble o de participaciones, cesión de un intangible, una garantía…
      </p>
      {r.manuales.length > 0 && (
        <ul className="mt-1 space-y-1 text-sm">
          {r.manuales.map((m) => (
            <li key={m.id} className="flex flex-wrap items-center gap-3">
              <span>{name(m.nif)}</span>
              <span>clave {m.clave}</span>
              <span>{m.ingreso ? 'Ingreso' : 'Pago'}</span>
              <span className="tabular-nums">{euros(m.amount)}</span>
              {m.especifica && <span className="pill">Específica</span>}
              {m.description && <span className="text-stone-600">{m.description}</span>}
              <ConfirmAction
                label="Quitar"
                ariaLabel={`Quitar la operación de ${euros(m.amount)} con ${name(m.nif)}`}
                question={`¿Quitar la operación de ${euros(m.amount)} con ${name(m.nif)}?`}
                confirmLabel="Sí, quitar"
                onConfirm={async () => {
                  await vinculadaOperacionDelete(m.id);
                  onChange();
                }}
              />
            </li>
          ))}
        </ul>
      )}
      {r.personas.length > 0 && (
        <form
          onSubmit={submit}
          aria-label="Añadir operación"
          className="form-grid mt-2 rounded-md border border-stone-200 p-4"
        >
          <label className="span-2">
            Con
            <select value={nif} onChange={(e) => setNif(e.target.value)}>
              <option value="">Elige…</option>
              {r.personas.map((p) => (
                <option key={p.nif} value={p.nif}>
                  {p.name}
                </option>
              ))}
            </select>
          </label>
          <ClaveSelect label="Tipo de operación" value={clave} onChange={setClave} />
          <label>
            Ingreso o pago
            <select
              value={ingreso ? 'i' : 'p'}
              onChange={(e) => setIngreso(e.target.value === 'i')}
            >
              <option value="p">Pago</option>
              <option value="i">Ingreso</option>
            </select>
          </label>
          <label>
            Importe sin IVA (€)
            <input
              className="text-right"
              inputMode="decimal"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
            />
          </label>
          <label className="span-2">
            Descripción
            <input value={description} onChange={(e) => setDescription(e.target.value)} />
          </label>
          <label className="span-full flex items-start gap-2">
            <input
              type="checkbox"
              checked={especifica}
              onChange={(e) => setEspecifica(e.target.checked)}
            />
            Operación específica (art. 18.3 LIS): transmisión de un negocio, de participaciones no
            cotizadas o de inmuebles, u operaciones sobre intangibles
          </label>
          <div className="span-full form-actions">
            <button type="submit" className="btn-primary rounded-md px-3 py-1.5">
              Añadir operación
            </button>
            {error && (
              <p role="alert" className="text-red-700">
                {error}
              </p>
            )}
          </div>
        </form>
      )}
    </section>
  );
}

function Cuentas({ r, onChange }: { r: RegistroVinculadas; onChange: () => void }) {
  const [error, setError] = useState<string | null>(null);
  if (r.cuentas.length === 0) return null;
  async function assign(c: CuentaVinculada, nif: string) {
    try {
      await vinculadaCuentaSet(c.account, nif || null);
      setError(null);
      onChange();
    } catch (e) {
      setError(errorMessage(e));
    }
  }
  return (
    <section aria-labelledby="vinc-cuentas" className="mt-6">
      <h4 id="vinc-cuentas" className="font-medium">
        Subcuentas de préstamos, cuentas corrientes e intereses
      </h4>
      <p className="text-sm text-stone-600">
        Di de quién es cada subcuenta (la 551 de la socia, el préstamo a la participada…): sus
        movimientos del año entran en el registro. Las que no son de una vinculada, déjalas sin
        asignar.
      </p>
      <table className="mt-2 w-full text-sm">
        <thead>
          <tr className="text-left text-stone-500">
            <th className="font-normal">Subcuenta</th>
            <th className="text-right font-normal">Debe</th>
            <th className="text-right font-normal">Haber</th>
            <th className="font-normal">De</th>
          </tr>
        </thead>
        <tbody>
          {r.cuentas.map((c) => (
            <tr key={c.account} className="border-t border-stone-100">
              <td className="py-1">
                <span className="font-mono">{c.account}</span> {c.name}
              </td>
              <td className={cell}>{euros(c.debit)}</td>
              <td className={cell}>{euros(c.credit)}</td>
              <td>
                <select
                  aria-label={`De quién es la subcuenta ${c.account}`}
                  value={c.nif ?? ''}
                  onChange={(e) => assign(c, e.target.value)}
                >
                  <option value="">Sin asignar</option>
                  {r.personas.map((p) => (
                    <option key={p.nif} value={p.nif}>
                      {p.name}
                    </option>
                  ))}
                </select>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}
    </section>
  );
}

function Documentacion({ r }: { r: RegistroVinculadas }) {
  const [actividad, setActividad] = useState(r.textos.actividad);
  const [justificacion, setJustificacion] = useState(r.textos.justificacion);
  const [msg, setMsg] = useState<string | null>(null);
  const run = async (f: () => Promise<string | null>) => {
    try {
      await vinculadasTextosSave({ year: r.year, actividad, justificacion });
      const path = await f();
      setMsg(path ? `Guardado en ${path}` : 'Textos guardados.');
    } catch (e) {
      setMsg(errorMessage(e));
    }
  };
  return (
    <section aria-labelledby="vinc-doc" className="mt-6">
      <h4 id="vinc-doc" className="font-medium">
        Documentación simplificada y borrador del 232
      </h4>
      <p className="text-sm text-stone-600">
        {r.simplificada
          ? 'Con una cifra de negocios de menos de 45 millones la documentación puede tener el contenido simplificado (art. 16.4 RIS): datos de la entidad y de las vinculadas, descripción e importe de las operaciones, método y comparables.'
          : 'La cifra de negocios llega a 45 millones: la documentación no puede ser la simplificada.'}{' '}
        {r.documentar.length > 0 && `Hay que tenerla de: ${r.documentar.join(', ')}.`} No se
        presenta: se guarda por si la pide Hacienda.
      </p>
      <div className="form-grid mt-2">
        <label className="span-full">
          Actividad de la entidad y, si forma grupo, su estructura
          <textarea rows={2} value={actividad} onChange={(e) => setActividad(e.target.value)} />
        </label>
        <label className="span-full">
          Justificación general del valor de mercado
          <textarea
            rows={3}
            value={justificacion}
            onChange={(e) => setJustificacion(e.target.value)}
          />
        </label>
      </div>
      <div className="mt-2 flex flex-wrap gap-2">
        <button
          type="button"
          className="rounded-md border border-stone-300 px-3 py-1"
          onClick={() => run(async () => null)}
        >
          Guardar textos
        </button>
        <button
          type="button"
          className="rounded-md border border-stone-300 px-3 py-1"
          onClick={() => run(() => vinculadasDocumentacionSave(r.year, false))}
        >
          Documentación (RTF)…
        </button>
        <button
          type="button"
          className="rounded-md border border-stone-300 px-3 py-1"
          onClick={() => run(() => vinculadasDocumentacionSave(r.year, true))}
        >
          Documentación (PDF)…
        </button>
        <button
          type="button"
          className="rounded-md border border-stone-300 px-3 py-1"
          onClick={() => run(() => vinculadasHoja232Save(r.year))}
        >
          Borrador del 232 (Excel)…
        </button>
      </div>
      {msg && (
        <p role="status" className="mt-1 text-sm">
          {msg}
        </p>
      )}
    </section>
  );
}

/** Contabilidad → Vinculadas: registro del año, umbrales del 232 y documentación. */
export function Vinculadas({ year }: { year: number }) {
  const [r, setR] = useState<RegistroVinculadas | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    vinculadasRegistro(year)
      .then((x) => {
        setR(x);
        setError(null);
      })
      .catch((e) => {
        setR(null);
        setError(errorMessage(e));
      });
  }, [year]);
  useEffect(load, [load]);
  if (error) {
    return (
      <p role="alert" className="mt-4 text-sm text-red-700">
        {error}
      </p>
    );
  }
  if (!r) return null;
  return (
    <section aria-labelledby="vinc-title" className="mt-4">
      <h3 id="vinc-title" className="text-base font-semibold">
        Operaciones vinculadas {year}
      </h3>
      <p className="text-sm text-stone-600">
        Del {formatDate(r.start)} al {formatDate(r.end)}
        {r.incn !== null && ` · cifra de negocios ${euros(r.incn)} (según ${r.incn_fuente})`}
      </p>
      <div
        role="status"
        className={`mt-2 rounded-md p-3 text-sm ${r.obligado ? 'bg-amber-50' : 'bg-stone-50'}`}
      >
        {r.obligado ? (
          <>
            <p className="font-medium">
              Hay que presentar el modelo 232 de {year} en {r.plazo}.
            </p>
            <ul className="mt-1 list-disc pl-5">
              {r.motivos.map((m) => (
                <li key={m}>{m}</li>
              ))}
            </ul>
          </>
        ) : (
          <p>
            Con los datos de TaxPain no se llega a ningún umbral del 232: más de 250.000 € con la
            misma persona o entidad, más de 100.000 € en operaciones específicas de un tipo, o más
            del 50 % de la cifra de negocios en operaciones del mismo tipo y método.
          </p>
        )}
        {r.avisos.map((a) => (
          <p key={a} className="mt-1 text-stone-700">
            {a}
          </p>
        ))}
      </div>
      <Personas r={r} onChange={load} />
      <Participadas onChange={load} />
      <Operaciones r={r} onChange={load} />
      <Manuales r={r} onChange={load} />
      <Cuentas r={r} onChange={load} />
      <Documentacion key={`${r.year}-doc`} r={r} />
    </section>
  );
}

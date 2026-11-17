import { Plus, Search, Users } from 'lucide-react';
import { type FormEvent, useCallback, useEffect, useId, useMemo, useState } from 'react';
import {
  COUNTRIES,
  type ContactData,
  type ContactDetail,
  type ContactRow,
  contactDelete,
  contactDetail,
  contactSave,
  contactsList,
  countryName,
  EU_COUNTRIES,
  errorMessage,
  euros,
  formatDate,
  type Party,
  type PartyId,
  plural,
  viesCheck,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { ProvinceDatalist, YearStepper } from './Controls';
import { provinceAfterPostCode } from './provincias';
import { VinculoContacto } from './Vinculadas';

type Filter = 'todos' | 'cliente' | 'proveedor';
const input = 'mt-1 block w-full rounded-md border border-stone-300 px-2 py-1.5';

const ROLE_LABEL: Record<ContactRow['role'], string> = {
  cliente: 'Cliente',
  proveedor: 'Proveedor',
  ambos: 'Cliente y proveedor',
};

/** Identificador de la parte según su país: NIF, NIF-IVA (empresa de la UE) u otro documento
 * (de fuera de la UE, o un particular de la UE). */
function partyId(country: string, taxId: string, consumer: boolean): PartyId {
  const id = taxId
    .trim()
    .toUpperCase()
    .replace(/[\s.-]/g, '');
  if (country === 'ES') return { Nif: id };
  return {
    Foreign: {
      country,
      id_type: EU_COUNTRIES.includes(country) && !consumer ? 'NifIva' : 'DocumentoOficial',
      id,
    },
  };
}

/** Particular de otro país de la UE: identificado sin NIF-IVA. */
function isConsumer(p: Party | null | undefined): boolean {
  return (
    !!p &&
    'Foreign' in p.id &&
    p.id.Foreign.id_type !== 'NifIva' &&
    EU_COUNTRIES.includes(p.id.Foreign.country)
  );
}

interface FormState {
  role: ContactRow['role'];
  country: string;
  /** Particular de la UE (sin NIF-IVA). */
  consumer: boolean;
  taxId: string;
  name: string;
  street: string;
  postCode: string;
  town: string;
  province: string;
  email: string;
  language: '' | 'Es' | 'En';
  notes: string;
}

function emptyForm(): FormState {
  return {
    role: 'cliente',
    country: 'ES',
    consumer: false,
    taxId: '',
    name: '',
    street: '',
    postCode: '',
    town: '',
    province: '',
    email: '',
    language: '',
    notes: '',
  };
}

function formFrom(d: ContactDetail): FormState {
  const p = d.data?.party ?? d.party;
  const a = p?.address;
  return {
    role: d.row.role,
    country: p && 'Foreign' in p.id ? p.id.Foreign.country : d.row.country || 'ES',
    consumer: isConsumer(p),
    taxId: p ? ('Nif' in p.id ? p.id.Nif : p.id.Foreign.id) : d.row.tax_id,
    name: p?.name ?? d.row.name,
    street: a?.street ?? '',
    postCode: a?.post_code ?? '',
    town: a?.town ?? '',
    province: a?.province ?? '',
    email: d.data?.email ?? '',
    language: d.data?.language ?? '',
    notes: d.data?.notes ?? '',
  };
}

function ContactForm({
  id,
  initial,
  onSaved,
  onCancel,
}: {
  id: number | null;
  initial: FormState;
  onSaved: (id: number) => void;
  onCancel: () => void;
}) {
  const [f, setF] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<FormState>) => setF({ ...f, ...patch });
  const provinces = useId();

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    const data: ContactData = {
      party: {
        id: partyId(f.country, f.taxId, f.consumer),
        name: f.name,
        address: f.street.trim()
          ? {
              street: f.street.trim(),
              post_code: f.postCode.trim(),
              town: f.town.trim(),
              province: f.province.trim(),
              country: f.country,
            }
          : null,
        person: null,
      },
      email: f.email || null,
      language: f.language || null,
      notes: f.notes || null,
    };
    try {
      onSaved(await contactSave({ id, role: f.role, data }));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  const eu = EU_COUNTRIES.includes(f.country);
  return (
    <form onSubmit={submit} className="space-y-3 text-sm">
      <div className="grid grid-cols-2 gap-3">
        <label>
          Tipo
          <select
            className={input}
            value={f.role}
            onChange={(e) => set({ role: e.target.value as FormState['role'] })}
          >
            <option value="cliente">Cliente</option>
            <option value="proveedor">Proveedor</option>
            <option value="ambos">Cliente y proveedor</option>
          </select>
        </label>
        <label>
          País
          <select
            className={input}
            value={f.country}
            onChange={(e) => set({ country: e.target.value })}
          >
            {COUNTRIES.map(([code, name]) => (
              <option key={code} value={code}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <label className="col-span-2">
          Nombre o razón social
          <input className={input} value={f.name} onChange={(e) => set({ name: e.target.value })} />
        </label>
        {eu && (
          <label className="col-span-2">
            <input
              type="checkbox"
              checked={f.consumer}
              onChange={(e) => set({ consumer: e.target.checked })}
            />
            Es un particular (sin NIF-IVA): se le factura con IVA español
          </label>
        )}
        <label>
          {f.country === 'ES'
            ? 'NIF'
            : eu && !f.consumer
              ? 'NIF-IVA'
              : eu
                ? 'Documento de identidad'
                : 'Identificador fiscal'}
          <input
            className={input}
            value={f.taxId}
            onChange={(e) => set({ taxId: e.target.value })}
          />
        </label>
        <label>
          Correo para las facturas
          <input
            type="email"
            className={input}
            value={f.email}
            onChange={(e) => set({ email: e.target.value })}
          />
        </label>
        <label className="col-span-2">
          Dirección
          <input
            className={input}
            value={f.street}
            onChange={(e) => set({ street: e.target.value })}
          />
        </label>
        <label>
          Código postal
          <input
            className={input}
            value={f.postCode}
            onChange={(e) =>
              set({
                postCode: e.target.value,
                ...(f.country === 'ES'
                  ? { province: provinceAfterPostCode(f.postCode, e.target.value, f.province) }
                  : {}),
              })
            }
          />
        </label>
        <label>
          Población
          <input className={input} value={f.town} onChange={(e) => set({ town: e.target.value })} />
        </label>
        <label>
          {f.country === 'ES' ? 'Provincia' : 'Región'}
          <input
            className={input}
            value={f.province}
            list={f.country === 'ES' ? provinces : undefined}
            onChange={(e) => set({ province: e.target.value })}
          />
          {f.country === 'ES' && <ProvinceDatalist id={provinces} />}
        </label>
        <label>
          Idioma de sus facturas
          <select
            className={input}
            value={f.language}
            onChange={(e) => set({ language: e.target.value as FormState['language'] })}
          >
            <option value="">Como en los ajustes</option>
            <option value="Es">Español</option>
            <option value="En">Inglés</option>
          </select>
        </label>
        <label className="col-span-2">
          Notas
          <textarea
            rows={2}
            className={input}
            value={f.notes}
            onChange={(e) => set({ notes: e.target.value })}
          />
        </label>
      </div>
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
          {busy ? 'Guardando…' : 'Guardar ficha'}
        </button>
        <button type="button" className="btn-ghost" onClick={onCancel}>
          Cancelar
        </button>
      </div>
    </form>
  );
}

function Figures({ row, year }: { row: ContactRow; year: number }) {
  const items: [string, number, string?][] = [];
  if (row.role !== 'proveedor') {
    items.push([`Facturado en ${year}`, row.invoiced_year]);
    items.push(['Pendiente de cobro', row.to_collect]);
  }
  if (row.role !== 'cliente') {
    items.push([`Comprado en ${year}`, row.purchased_year]);
    items.push(['Pendiente de pago', row.to_pay]);
  }
  return (
    <dl className="grid grid-cols-2 gap-3">
      {items.map(([label, cents]) => (
        <div key={label} className="rounded-lg bg-stone-50 px-3 py-2">
          <dt className="text-xs text-stone-500">{label}</dt>
          <dd className="font-semibold tabular-nums">{euros(cents)}</dd>
        </div>
      ))}
      {row.overdue !== 0 && (
        <div className="col-span-2 rounded-lg bg-red-50 px-3 py-2 text-red-900">
          <dt className="text-xs">Vencido sin cobrar ni pagar</dt>
          <dd className="font-semibold tabular-nums">{euros(row.overdue)}</dd>
        </div>
      )}
    </dl>
  );
}

/** Estado de la comprobación del NIF-IVA en VIES, y botón para (re)comprobarlo. */
function ViesLine({
  row,
  data,
  onChecked,
}: {
  row: ContactRow;
  data: ContactData | null;
  onChecked: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const v = data?.vies;
  return (
    <div className="rounded-md bg-stone-50 p-3 text-sm">
      {v ? (
        <p className={v.valid ? 'text-emerald-800' : 'text-red-800'}>
          {v.valid ? 'NIF-IVA válido en VIES' : 'NIF-IVA no válido en VIES'} (comprobado el{' '}
          {new Date(v.checked_at).toLocaleDateString('es-ES')}
          {v.request_id ? `, consulta ${v.request_id}` : ''}).
        </p>
      ) : (
        <p className="text-stone-700">NIF-IVA sin comprobar en VIES.</p>
      )}
      <button
        type="button"
        className="btn-ghost mt-1 -ml-3"
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          setError(null);
          try {
            // Si no tiene ficha, se crea con el tipo de la fila (un proveedor no pasa a cliente).
            await viesCheck(row.country, row.tax_id, row.name, row.role);
            onChecked();
          } catch (e) {
            setError(errorMessage(e));
          } finally {
            setBusy(false);
          }
        }}
      >
        {busy ? 'Consultando VIES…' : v ? 'Volver a comprobar' : 'Comprobar en VIES'}
      </button>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
    </div>
  );
}

function Detail({
  rowKey,
  year,
  onSaved,
  onDeleted,
  onClose,
}: {
  rowKey: string;
  year: number;
  /** Ficha guardada (con otro NIF, su clave cambia: el padre abre la nueva). */
  onSaved: (id: number) => void;
  onDeleted: () => void;
  onClose: () => void;
}) {
  const [d, setD] = useState<ContactDetail | null>(null);
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    contactDetail(rowKey, year)
      .then((r) => {
        setD(r);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
  }, [rowKey, year]);
  useEffect(load, [load]);

  if (error) {
    return (
      <p role="alert" className="text-sm text-red-700">
        {error}
      </p>
    );
  }
  if (!d) return null;
  const r = d.row;
  return (
    <section aria-labelledby="contact-title" className="card space-y-4 p-5">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 id="contact-title" className="truncate text-lg font-semibold">
            {r.name}
          </h3>
          <p className="text-sm text-stone-600">
            {r.tax_id}
            {r.country !== 'ES' && ` · ${countryName(r.country)}`} · {ROLE_LABEL[r.role]}
            {r.email && ` · ${r.email}`}
          </p>
        </div>
        <button type="button" className="btn-ghost" onClick={onClose}>
          Cerrar
        </button>
      </div>
      <Figures row={r} year={year} />
      {editing ? (
        <ContactForm
          id={r.id}
          initial={formFrom(d)}
          onSaved={(id) => {
            setEditing(false);
            onSaved(id);
          }}
          onCancel={() => setEditing(false)}
        />
      ) : (
        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            className="rounded-md border border-stone-300 px-3"
            onClick={() => setEditing(true)}
          >
            {r.id === null ? 'Crear ficha' : 'Editar ficha'}
          </button>
          {r.id !== null && (
            <ConfirmAction
              label="Borrar ficha"
              question="¿Borrar la ficha? Sus facturas no se tocan: seguirá saliendo en la lista por ellas."
              confirmLabel="Borrar ficha"
              className="btn-ghost text-red-700"
              onConfirm={async () => {
                await contactDelete(r.id as number);
                onDeleted();
              }}
            />
          )}
        </div>
      )}
      {!editing && r.tax_id && (
        <VinculoContacto taxId={r.tax_id} name={r.name} country={r.country} />
      )}
      {r.country !== 'ES' &&
        EU_COUNTRIES.includes(r.country) &&
        !isConsumer(d.data?.party ?? d.party) &&
        !editing && <ViesLine row={r} data={d.data} onChecked={load} />}
      {d.data?.notes && !editing && (
        <p className="rounded-md bg-stone-50 p-3 text-sm text-stone-700">{d.data.notes}</p>
      )}
      {d.years.length > 0 && (
        <div>
          <h4 className="text-sm font-semibold">Por años</h4>
          <table className="mt-2 w-full text-sm">
            <thead>
              <tr className="text-left text-stone-500">
                <th className="font-normal">Año</th>
                <th className="text-right font-normal">Facturado</th>
                <th className="text-right font-normal">Comprado</th>
              </tr>
            </thead>
            <tbody>
              {d.years.map((y) => (
                <tr key={y.year} className="border-t border-stone-100">
                  <td className="py-1">{y.year}</td>
                  <td className="text-right tabular-nums">{euros(y.invoiced)}</td>
                  <td className="text-right tabular-nums">{euros(y.purchased)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="mt-1 text-xs text-stone-500">
            Totales con IVA: más de 3.005,06 € al año con el mismo cliente o proveedor van en el
            modelo 347.
          </p>
        </div>
      )}
      <div>
        <h4 className="text-sm font-semibold">Facturas</h4>
        {d.history.length === 0 ? (
          <p className="mt-2 text-sm text-stone-500">Aún no hay facturas.</p>
        ) : (
          <table className="mt-2 w-full text-sm">
            <thead>
              <tr className="text-left text-stone-500">
                <th className="font-normal">Fecha</th>
                <th className="font-normal">Número</th>
                <th className="text-right font-normal">Total</th>
                <th className="text-right font-normal">Pendiente</th>
              </tr>
            </thead>
            <tbody>
              {d.history.slice(0, 50).map((h) => (
                <tr key={`${h.kind}-${h.id}`} className="border-t border-stone-100">
                  <td className="py-1">{formatDate(h.date)}</td>
                  <td>
                    {h.number || '—'}{' '}
                    <span className="text-xs text-stone-500">
                      {h.kind === 'emitida' ? 'emitida' : 'recibida'}
                      {h.status.startsWith('anulada') && ', anulada'}
                    </span>
                  </td>
                  <td className="text-right tabular-nums">{euros(h.total)}</td>
                  <td
                    className={`text-right tabular-nums ${h.status.endsWith('vencida') ? 'text-red-700' : ''}`}
                  >
                    {h.pending ? euros(h.pending) : '—'}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </section>
  );
}

/** Clientes y proveedores: ficha, cifras del año, lo pendiente e historial de facturas. */
export function Contacts({
  now = new Date(),
  initialKey,
}: {
  now?: Date;
  /** Contacto que se abre al entrar (desde la búsqueda). */
  initialKey?: string | undefined;
}) {
  const [year, setYear] = useState(now.getFullYear());
  const [rows, setRows] = useState<ContactRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>('todos');
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<string | null>(initialKey ?? null);
  const [creating, setCreating] = useState(false);
  /** Sube al guardar o borrar: vuelve a cargar la ficha abierta. */
  const [version, setVersion] = useState(0);

  const load = useCallback(
    () =>
      contactsList(year)
        .then((r) => {
          setRows(r);
          setError(null);
          return r;
        })
        .catch((e) => {
          setError(errorMessage(e));
          return null;
        }),
    [year],
  );
  useEffect(() => {
    void load();
  }, [load]);

  /** Tras guardar la ficha `id`: abre su fila (su clave cambia si cambió el NIF). */
  const showSaved = (id: number) => {
    void load().then((r) => {
      const row = r?.find((x) => x.id === id);
      if (row) setSelected(row.key);
      setVersion((n) => n + 1);
    });
  };
  /** Tras borrar la ficha: si el tercero sigue en la lista (por sus facturas), se queda abierto. */
  const showDeleted = (key: string) => {
    void load().then((r) => {
      if (!r?.some((x) => x.key === key)) setSelected(null);
      setVersion((n) => n + 1);
    });
  };

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (rows ?? []).filter(
      (r) =>
        (filter === 'todos' || r.role === filter || r.role === 'ambos') &&
        (!q || r.name.toLowerCase().includes(q) || r.key.toLowerCase().includes(q)),
    );
  }, [rows, filter, query]);

  return (
    <div className="mt-5 space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <label className="relative">
          <span className="sr-only">Buscar</span>
          <Search
            size={16}
            aria-hidden="true"
            className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-stone-400"
          />
          <input
            type="search"
            placeholder="Nombre o NIF"
            className="w-64 rounded-md border border-stone-300 py-1.5 pr-2"
            style={{ paddingLeft: '2rem' }}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <nav className="flex gap-1" aria-label="Tipo de contacto">
          {(
            [
              ['todos', 'Todos'],
              ['cliente', 'Clientes'],
              ['proveedor', 'Proveedores'],
            ] as [Filter, string][]
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              aria-current={filter === id ? 'page' : undefined}
              onClick={() => setFilter(id)}
              className={`rounded px-2 py-1 text-sm ${filter === id ? 'bg-stone-200 font-medium' : 'text-stone-600'}`}
            >
              {label}
            </button>
          ))}
        </nav>
        <YearStepper year={year} onChange={setYear} label="Año de las cifras" />
        <button
          type="button"
          className="btn-primary ml-auto inline-flex items-center gap-1.5 rounded-md px-3 py-1.5"
          onClick={() => {
            setCreating(true);
            setSelected(null);
          }}
        >
          <Plus size={16} aria-hidden="true" />
          Nuevo contacto
        </button>
      </div>

      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}

      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_28rem]">
        <div className="card overflow-hidden">
          {rows && shown.length === 0 ? (
            <div className="flex flex-col items-center gap-2 p-10 text-center text-sm text-stone-500">
              <Users size={28} aria-hidden="true" />
              {rows.length === 0
                ? 'Aún no hay clientes ni proveedores. Aparecen solos al emitir o revisar facturas, o créalos con «Nuevo contacto».'
                : 'Nada coincide con la búsqueda.'}
            </div>
          ) : (
            <table className="w-full text-sm">
              <thead className="bg-stone-50 text-left text-stone-500">
                <tr>
                  <th className="px-4 py-2 font-medium">Nombre</th>
                  <th className="px-2 py-2 font-medium">NIF</th>
                  <th className="px-2 py-2 text-right font-medium">{year}</th>
                  <th className="px-2 py-2 text-right font-medium">Pendiente</th>
                  <th className="px-4 py-2 text-right font-medium">Última</th>
                </tr>
              </thead>
              <tbody>
                {shown.map((r) => {
                  const pending = r.to_collect - r.to_pay;
                  return (
                    <tr
                      key={r.key}
                      className={`border-t border-stone-100 ${selected === r.key ? 'bg-brand-50' : ''}`}
                    >
                      <td className="px-4 py-2">
                        <button
                          type="button"
                          className="text-left font-medium text-link hover:underline"
                          aria-current={selected === r.key ? 'true' : undefined}
                          onClick={() => {
                            setSelected(r.key);
                            setCreating(false);
                          }}
                        >
                          {r.name || r.key}
                        </button>
                        <span className="ml-2 text-xs text-stone-500">
                          {ROLE_LABEL[r.role]}
                          {r.country !== 'ES' && ` · ${countryName(r.country)}`}
                        </span>
                      </td>
                      <td className="px-2 py-2 text-stone-600">{r.tax_id}</td>
                      <td className="px-2 py-2 text-right tabular-nums">
                        {euros(r.invoiced_year + r.purchased_year)}
                      </td>
                      <td
                        className={`px-2 py-2 text-right tabular-nums ${r.overdue ? 'text-red-700' : ''}`}
                      >
                        {pending ? euros(Math.abs(pending)) : '—'}
                        {pending < 0 && <span className="sr-only"> a pagar</span>}
                      </td>
                      <td className="px-4 py-2 text-right text-stone-600">
                        {r.last_date ? formatDate(r.last_date) : '—'}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
          {rows && shown.length > 0 && (
            <p className="border-t border-stone-100 px-4 py-2 text-xs text-stone-500">
              {plural(shown.length, 'contacto')}. Importes con IVA; «Pendiente» según la cartera.
            </p>
          )}
        </div>
        <div>
          {creating && (
            <section aria-labelledby="new-contact" className="card p-5">
              <h3 id="new-contact" className="mb-3 text-lg font-semibold">
                Nuevo contacto
              </h3>
              <ContactForm
                id={null}
                initial={emptyForm()}
                onSaved={(id) => {
                  setCreating(false);
                  showSaved(id);
                }}
                onCancel={() => setCreating(false)}
              />
            </section>
          )}
          {selected && !creating && (
            <Detail
              key={`${selected}-${version}`}
              rowKey={selected}
              year={year}
              onSaved={showSaved}
              onDeleted={() => showDeleted(selected)}
              onClose={() => setSelected(null)}
            />
          )}
        </div>
      </div>
    </div>
  );
}

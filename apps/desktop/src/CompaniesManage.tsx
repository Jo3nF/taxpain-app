import { type FormEvent, useEffect, useId, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { type Company, companyDelete, companyUpdateId, errorMessage } from './api';

/** Igual que `taxpain_app::companies::confirms`: su nombre o su NIF (sin contar mayúsculas). */
export function confirmsDelete(c: Company, typed: string): boolean {
  const norm = (s: string) => s.trim().split(/\s+/).join(' ').toLowerCase();
  const t = norm(typed);
  if (!t) return false;
  if (t === norm(c.name)) return true;
  return !!c.nif && norm(c.nif).replace(/ /g, '') === t.replace(/ /g, '');
}

/**
 * Gestionar empresas: modificar el nombre y el NIF de cualquiera y borrar las que sobren.
 * Borrar mueve su carpeta a la Papelera del Mac. `onChanged`: algo cambió (vuelve a leer la
 * lista); `onReload`: se ha borrado la abierta y ya está abierta otra (recargar la interfaz).
 */
export function CompaniesManage({
  list,
  onClose,
  onChanged,
  onReload,
}: {
  list: Company[];
  onClose: () => void;
  onChanged: () => void;
  onReload: () => void;
}) {
  const [editing, setEditing] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const titleId = useId();
  const box = useRef<HTMLDivElement>(null);

  // Al abrir, el foco va al primer botón.
  useEffect(() => {
    box.current?.querySelector<HTMLElement>('button')?.focus();
  }, []);

  const only = list.length < 2;
  // Al cerrar un formulario, el foco vuelve al botón de su empresa.
  const refocus = (selector: string) =>
    requestAnimationFrame(() => box.current?.querySelector<HTMLElement>(selector)?.focus());
  const busyClose = () => {
    if (!busy) onClose();
  };

  return createPortal(
    // biome-ignore lint/a11y/noStaticElementInteractions: clic fuera para cerrar (con teclado, Escape)
    <div
      className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-stone-900/30 px-4 py-[8vh]"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) busyClose();
      }}
    >
      <div
        ref={box}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="w-full max-w-lg space-y-3 rounded-xl border border-stone-200 bg-white p-4 text-sm shadow-2xl"
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.stopPropagation();
            if (editing || deleting) {
              if (!busy) {
                const sel = editing ? `[data-edit="${editing}"]` : `[data-delete="${deleting}"]`;
                setEditing(null);
                setDeleting(null);
                refocus(sel);
              }
            } else busyClose();
          }
        }}
      >
        <h2 id={titleId} className="text-base font-semibold">
          Gestionar empresas
        </h2>
        <ul className="divide-y divide-stone-200 rounded-lg border border-stone-200">
          {list.map((c) => (
            <li key={c.id} className="p-3">
              <div className="flex flex-wrap items-center gap-2">
                <span className="min-w-0 flex-1">
                  <span className="font-medium">{c.name}</span>
                  {c.active && <span className="ml-1 text-stone-500">(abierta)</span>}
                  {c.nif && <span className="block text-xs text-stone-500">{c.nif}</span>}
                </span>
                <button
                  type="button"
                  className="rounded-md border border-stone-200 px-2.5 py-1 hover:bg-stone-100 disabled:opacity-50"
                  data-edit={c.id}
                  aria-label={`Modificar ${c.name}`}
                  aria-expanded={editing === c.id}
                  disabled={busy}
                  onClick={() => {
                    setDeleting(null);
                    setEditing(editing === c.id ? null : c.id);
                  }}
                >
                  Modificar
                </button>
                {!only && (
                  <button
                    type="button"
                    className="rounded-md border border-stone-200 px-2.5 py-1 text-red-700 hover:bg-stone-100 disabled:opacity-50"
                    data-delete={c.id}
                    aria-label={`Borrar ${c.name}`}
                    aria-expanded={deleting === c.id}
                    disabled={busy}
                    onClick={() => {
                      setEditing(null);
                      setDeleting(deleting === c.id ? null : c.id);
                    }}
                  >
                    Borrar
                  </button>
                )}
              </div>
              {editing === c.id && (
                <EditCompany
                  company={c}
                  busy={busy}
                  setBusy={setBusy}
                  onCancel={() => {
                    setEditing(null);
                    refocus(`[data-edit="${c.id}"]`);
                  }}
                  onSaved={() => {
                    setEditing(null);
                    onChanged();
                    refocus(`[data-edit="${c.id}"]`);
                  }}
                />
              )}
              {deleting === c.id && (
                <DeleteCompany
                  company={c}
                  next={list.find((o) => o.id !== c.id)}
                  busy={busy}
                  setBusy={setBusy}
                  onCancel={() => {
                    setDeleting(null);
                    refocus(`[data-delete="${c.id}"]`);
                  }}
                  onDeleted={(switched) => {
                    setDeleting(null);
                    if (switched) onReload();
                    else {
                      onChanged();
                      refocus('button');
                    }
                  }}
                />
              )}
            </li>
          ))}
        </ul>
        {only && (
          <p className="text-stone-600">
            Es la única empresa: no se puede borrar. Si quieres quitarla, añade antes otra.
          </p>
        )}
        <div className="flex justify-end">
          <button
            type="button"
            className="rounded-md px-3 py-1.5 text-stone-600 hover:bg-stone-100 disabled:opacity-50"
            disabled={busy}
            onClick={onClose}
          >
            Cerrar
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

function EditCompany({
  company,
  busy,
  setBusy,
  onCancel,
  onSaved,
}: {
  company: Company;
  busy: boolean;
  setBusy: (b: boolean) => void;
  onCancel: () => void;
  onSaved: () => void;
}) {
  const [name, setName] = useState(company.name);
  const [nif, setNif] = useState(company.nif ?? '');
  const [error, setError] = useState<string | null>(null);
  const first = useRef<HTMLInputElement>(null);
  useEffect(() => first.current?.focus(), []);

  async function save(e: FormEvent) {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await companyUpdateId(company.id, { name, nif: nif.trim() || null });
      onSaved();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={save} aria-label={`Modificar ${company.name}`} className="mt-3 space-y-2.5">
      <label className="block text-[0.8125rem] font-medium text-stone-700">
        Nombre o razón social
        <input
          ref={first}
          className="mt-1 block w-full"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
        />
      </label>
      <label className="block text-[0.8125rem] font-medium text-stone-700">
        NIF (opcional)
        <input className="mt-1 block w-full" value={nif} onChange={(e) => setNif(e.target.value)} />
      </label>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <button
          type="submit"
          className="rounded-md btn-primary px-3 py-1.5 disabled:opacity-50"
          disabled={busy}
        >
          Guardar
        </button>
        <button
          type="button"
          className="rounded-md px-3 py-1.5 text-stone-600 hover:bg-stone-100"
          disabled={busy}
          onClick={onCancel}
        >
          Cancelar
        </button>
      </div>
    </form>
  );
}

function DeleteCompany({
  company,
  next,
  busy,
  setBusy,
  onCancel,
  onDeleted,
}: {
  company: Company;
  next: Company | undefined;
  busy: boolean;
  setBusy: (b: boolean) => void;
  onCancel: () => void;
  onDeleted: (switched: boolean) => void;
}) {
  const [typed, setTyped] = useState('');
  const [error, setError] = useState<string | null>(null);
  const titleId = useId();
  const first = useRef<HTMLInputElement>(null);
  useEffect(() => first.current?.focus(), []);
  const ok = confirmsDelete(company, typed);

  async function remove(e: FormEvent) {
    e.preventDefault();
    if (busy || !ok) return;
    setBusy(true);
    setError(null);
    try {
      const switched = await companyDelete(company.id, typed);
      onDeleted(switched);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form
      onSubmit={remove}
      aria-labelledby={titleId}
      className="mt-3 space-y-2 rounded-md border border-red-300 bg-red-50 p-3 text-stone-900"
    >
      <p id={titleId} className="font-semibold">
        Borrar «{company.name}»
      </p>
      <p>
        Se quitan de TaxPain <strong>todos los datos de esta empresa</strong>: facturas,
        contabilidad, modelos, documentos, certificados guardados y ajustes. Las demás empresas no
        se tocan.
      </p>
      <p>
        Su carpeta («{company.id === 'principal' ? 'data' : company.id}») va a la{' '}
        <strong>Papelera del Mac</strong>: hasta que la vacíes se puede recuperar, con la app
        cerrada.
      </p>
      <p>
        Antes, haz una copia de seguridad: abre esta empresa y ve a{' '}
        <strong>Ajustes → Copias de seguridad</strong>. Los certificados no van en la copia.
      </p>
      {company.active && next && (
        <p>Es la empresa abierta: antes de borrarla se abre «{next.name}».</p>
      )}
      <label className="block font-medium">
        Para confirmar, escribe su nombre{company.nif ? ' o su NIF' : ''}: {company.name}
        <input
          ref={first}
          className="mt-1 block w-full"
          value={typed}
          autoComplete="off"
          onChange={(e) => setTyped(e.target.value)}
        />
      </label>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <button
          type="submit"
          className="rounded-md btn-danger px-3 py-1.5 disabled:opacity-50"
          disabled={!ok || busy}
        >
          {busy ? 'Borrando…' : 'Borrar empresa'}
        </button>
        <button
          type="button"
          className="rounded-md px-3 py-1.5 text-stone-600 hover:bg-stone-100"
          disabled={busy}
          onClick={onCancel}
        >
          Cancelar
        </button>
      </div>
    </form>
  );
}

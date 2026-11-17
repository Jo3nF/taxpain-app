import { Building2, ChevronsUpDown, Pencil, Plus } from 'lucide-react';
import { type FormEvent, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import {
  type Company,
  companiesList,
  companyCreate,
  companySwitch,
  type Diagnostico,
  errorMessage,
  recuperacionEmpresas,
} from './api';
import { CompaniesManage } from './CompaniesManage';
import { Menu } from './Menu';
import { RecuperacionEmpresa } from './Recuperacion';

/** Iniciales para el distintivo: «Estudio Norte SL» → «EN». */
export function initials(name: string): string {
  const clean = name.replace(
    /[,\s]+(s\.?\s?l\.?\s?[up]?\.?|s\.?\s?a\.?\s?u?\.?|s\.?\s?c\.?\s?p?\.?|c\.?\s?b\.?)\s*$/i,
    '',
  );
  const letters = clean
    .split(/[\s.,·-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0] ?? '')
    .join('');
  return (letters || name.slice(0, 2)).toUpperCase();
}

/** Igual que `MIN_PASSPHRASE` en `taxpain_vault::backup` (el servidor lo vuelve a comprobar). */
const MIN_PASSPHRASE = 10;

/** Selector de empresa: cada una tiene sus datos aparte. Cambiar recarga la interfaz. */
/** Evento de ventana: ha cambiado alguna empresa (nombre, NIF o borrado): las listas se releen. */
export const COMPANIES_CHANGED = 'taxpain:empresas';

export function CompanySwitcher({ onSwitched }: { onSwitched?: () => void }) {
  const [list, setList] = useState<Company[]>([]);
  const [adding, setAdding] = useState(false);
  const [managing, setManaging] = useState(false);
  // Empresa que no abre cuya pantalla de recuperación está abierta.
  const [repairing, setRepairing] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const [name, setName] = useState('');
  const [nif, setNif] = useState('');
  // Contraseña de las copias de la empresa nueva (y su código de recuperación).
  const [pass, setPass] = useState('');
  const [again, setAgain] = useState('');
  const [message, setMessage] = useState<string | null>(null);
  // Sin dobles envíos: dos clics en «Crear y abrir» no crean dos empresas.
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);

  useEffect(() => {
    const load = () =>
      companiesList()
        .then(setList)
        .catch((e) => setMessage(errorMessage(e)));
    load();
    window.addEventListener(COMPANIES_CHANGED, load);
    return () => window.removeEventListener(COMPANIES_CHANGED, load);
  }, []);

  const active = list.find((c) => c.active);
  const reload = onSwitched ?? (() => window.location.reload());

  async function change(id: string) {
    try {
      await companySwitch(id);
      reload();
    } catch (e) {
      setMessage(errorMessage(e));
    }
  }

  async function create(e: FormEvent) {
    e.preventDefault();
    if (busyRef.current) return;
    if ([...pass].length < MIN_PASSPHRASE) {
      setMessage(`La contraseña de las copias ha de tener al menos ${MIN_PASSPHRASE} caracteres.`);
      return;
    }
    if (pass !== again) {
      setMessage('Las dos contraseñas no coinciden.');
      return;
    }
    busyRef.current = true;
    setBusy(true);
    setMessage(null);
    try {
      const c = await companyCreate({ name, nif: nif.trim() || null }, pass);
      await companySwitch(c.id);
      reload();
    } catch (err) {
      setMessage(errorMessage(err));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }

  return (
    <div ref={root} className="text-sm">
      <Menu
        label="Empresa"
        className="company-trigger"
        align="start"
        width="17rem"
        entries={[
          { heading: 'Empresas' },
          ...list.map((c) => ({
            label: c.no_abre ? `${c.name} · no abre` : c.name,
            hint: c.no_abre ? 'Ver qué pasa y recuperarla' : (c.nif ?? undefined),
            checked: c.active,
            onSelect: () => {
              if (c.no_abre) setRepairing(c.id);
              else if (!c.active) change(c.id);
            },
          })),
          { separator: true },
          {
            label: 'Añadir empresa…',
            icon: <Plus size={15} aria-hidden="true" />,
            onSelect: () => setAdding(true),
          },
          {
            label: 'Modificar o borrar empresas…',
            icon: <Pencil size={15} aria-hidden="true" />,
            onSelect: () => setManaging(true),
          },
        ]}
      >
        <span className="company-badge" aria-hidden="true">
          {active ? initials(active.name) : <Building2 size={15} />}
        </span>
        <span className="min-w-0 flex-1 text-left">
          <span className="company-name">{active?.name ?? 'Empresa'}</span>
          {active?.nif && <span className="company-nif">{active.nif}</span>}
        </span>
        <ChevronsUpDown size={15} aria-hidden="true" className="shrink-0" />
      </Menu>
      {managing && (
        <CompaniesManage
          list={list}
          onClose={() => {
            setManaging(false);
            // El foco vuelve al selector de empresa.
            root.current?.querySelector<HTMLElement>('button')?.focus();
          }}
          onChanged={() => window.dispatchEvent(new Event(COMPANIES_CHANGED))}
          onReload={reload}
        />
      )}
      {repairing && (
        <Reparar
          id={repairing}
          onClose={() => setRepairing(null)}
          onRestored={() => window.location.reload()}
        />
      )}
      {adding && (
        <form
          onSubmit={create}
          aria-label="Nueva empresa"
          className="mt-2 space-y-2.5 rounded-lg border border-stone-200 bg-white p-3"
        >
          <label className="block text-[0.8125rem] font-medium text-stone-700">
            Nombre o razón social
            <input
              className="mt-1 block w-full"
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
            />
          </label>
          <label className="block text-[0.8125rem] font-medium text-stone-700">
            NIF (opcional)
            <input
              className="mt-1 block w-full"
              value={nif}
              onChange={(e) => setNif(e.target.value)}
            />
          </label>
          <label className="block text-[0.8125rem] font-medium text-stone-700">
            Contraseña de las copias
            <input
              type="password"
              autoComplete="new-password"
              className="mt-1 block w-full"
              value={pass}
              onChange={(e) => setPass(e.target.value)}
              required
            />
          </label>
          <label className="block text-[0.8125rem] font-medium text-stone-700">
            Repítela
            <input
              type="password"
              autoComplete="new-password"
              className="mt-1 block w-full"
              value={again}
              onChange={(e) => setAgain(e.target.value)}
              required
            />
          </label>
          <p className="text-[0.8125rem] text-stone-600">
            Al menos {MIN_PASSPHRASE} caracteres; mejor una frase de varias palabras. Cifra las
            copias y el código de recuperación que permite abrir los datos en otro Mac.{' '}
            <strong className="text-amber-900">
              Si la olvidas, no se puede recuperar nada: apúntala en un lugar seguro.
            </strong>
          </p>
          <div className="grid gap-1">
            <button
              type="submit"
              className="w-full rounded-md btn-primary px-3 py-1.5 disabled:opacity-50"
              disabled={busy}
            >
              Crear y abrir
            </button>
            <button
              type="button"
              className="w-full rounded-md px-3 py-1.5 text-stone-600 hover:bg-stone-100"
              onClick={() => setAdding(false)}
            >
              Cancelar
            </button>
          </div>
        </form>
      )}
      {message && (
        <p role="alert" className="mt-1 text-red-700">
          {message}
        </p>
      )}
    </div>
  );
}

/** Pantalla de recuperación de una empresa que no abre (desde el selector). */
function Reparar({
  id,
  onClose,
  onRestored,
}: {
  id: string;
  onClose: () => void;
  onRestored: () => void;
}) {
  const [diag, setDiag] = useState<Diagnostico | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    recuperacionEmpresas()
      .then((l) => setDiag(l.find((d) => d.id === id) ?? null))
      .catch((e) => setError(errorMessage(e)));
  }, [id]);
  useEffect(() => {
    box.current?.querySelector<HTMLElement>('button')?.focus();
  }, []);
  return createPortal(
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-stone-900/30 px-4 py-[6vh]">
      <div
        ref={box}
        role="dialog"
        aria-modal="true"
        aria-label="Empresa que no abre"
        className="w-full max-w-lg space-y-3 rounded-xl border border-stone-200 bg-white p-4 text-sm shadow-2xl"
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.stopPropagation();
            onClose();
          }
        }}
      >
        <div className="flex justify-end">
          <button type="button" className="text-stone-600 underline" onClick={onClose}>
            Cerrar
          </button>
        </div>
        {error && <p role="alert">{error}</p>}
        {diag === null && <p>Esta empresa ya se abre bien: cierra y elígela en el selector.</p>}
        {diag && <RecuperacionEmpresa diag={diag} onRestored={onRestored} />}
      </div>
    </div>,
    document.body,
  );
}

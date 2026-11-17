import {
  BookOpen,
  CornerDownLeft,
  FileText,
  FolderOpen,
  Hash,
  Inbox,
  type LucideIcon,
  Navigation,
  Search,
  Users,
} from 'lucide-react';
import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { errorMessage, globalSearch, type Rectificable, type SearchHit } from './api';

/** Adónde lleva un resultado o una orden. */
export type Jump =
  | { tab: string }
  | { tab: 'contactos'; contact: string }
  | { tab: 'contabilidad'; entry: number; year: number }
  | { tab: 'contabilidad'; account: string }
  | { tab: 'facturas'; openInvoice: number }
  | { tab: 'recibidas'; openReceived: number }
  | { tab: 'importar'; openDocument: number }
  | { tab: 'modelos'; rectificar: Rectificable };

export interface PaletteCommand {
  label: string;
  hint: string;
  jump: Jump;
}

const ICONS: Record<string, LucideIcon> = {
  emitida: FileText,
  recibida: Inbox,
  contacto: Users,
  cuenta: Hash,
  asiento: BookOpen,
  documento: FolderOpen,
  orden: Navigation,
};

const KIND_LABEL: Record<string, string> = {
  emitida: 'Facturas emitidas',
  recibida: 'Facturas recibidas',
  contacto: 'Clientes y proveedores',
  cuenta: 'Cuentas',
  asiento: 'Asientos',
  documento: 'Documentos',
  orden: 'Ir a',
};

function jumpOf(h: SearchHit): Jump {
  switch (h.kind) {
    case 'emitida':
      return { tab: 'facturas', openInvoice: Number(h.target) };
    case 'recibida':
      return { tab: 'recibidas', openReceived: Number(h.target) };
    case 'contacto':
      return { tab: 'contactos', contact: h.target };
    case 'cuenta':
      return { tab: 'contabilidad', account: h.target };
    case 'asiento': {
      const [n, y] = h.target.split(':').map(Number);
      return { tab: 'contabilidad', entry: n ?? 0, year: y ?? new Date().getFullYear() };
    }
    default:
      return { tab: 'importar', openDocument: Number(h.target) };
  }
}

type Item = { key: string; kind: string; title: string; subtitle: string; jump: Jump };

/** Sin tildes y en minúsculas, para comparar. */
function plain(s: string): string {
  return s
    .normalize('NFD')
    .replace(/\p{Diacritic}/gu, '')
    .toLowerCase();
}

/**
 * Búsqueda global (⌘K): facturas, clientes, cuentas, asientos y documentos, y órdenes para ir a
 * cada sección. Todo se busca en local.
 */
export function CommandPalette({
  open,
  onClose,
  commands,
  onJump,
}: {
  open: boolean;
  onClose: () => void;
  commands: PaletteCommand[];
  onJump: (j: Jump) => void;
}) {
  const [q, setQ] = useState('');
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const restore = useRef<HTMLElement | null>(null);
  const listId = useId();

  useEffect(() => {
    if (open) {
      restore.current = document.activeElement as HTMLElement | null;
      setQ('');
      setHits([]);
      setActive(0);
      setTimeout(() => input.current?.focus(), 0);
    } else {
      restore.current?.focus?.();
    }
  }, [open]);

  useEffect(() => {
    const t = q.trim();
    if (!open || t.length < 2) {
      setHits([]);
      setError(null);
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      globalSearch(t)
        .then((r) => {
          if (!cancelled) {
            setHits(r);
            setError(null);
            setActive(0);
          }
        })
        .catch((e) => {
          if (!cancelled) setError(errorMessage(e));
        });
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [q, open]);

  const items: Item[] = useMemo(() => {
    const t = plain(q.trim());
    const cmds = commands
      .filter((c) => !t || plain(c.label).includes(t) || plain(c.hint).includes(t))
      .slice(0, t ? 5 : 12)
      .map((c, i) => ({
        key: `orden-${i}`,
        kind: 'orden',
        title: c.label,
        subtitle: c.hint,
        jump: c.jump,
      }));
    const found = hits.map((h, i) => ({
      key: `${h.kind}-${h.target}-${i}`,
      kind: h.kind,
      title: h.title,
      subtitle: h.subtitle,
      jump: jumpOf(h),
    }));
    return [...found, ...cmds];
  }, [q, hits, commands]);

  if (!open) return null;

  const choose = (it: Item | undefined) => {
    if (!it) return;
    onClose();
    onJump(it.jump);
  };

  // Agrupadas por tipo, en el orden en que llegan.
  const groups: { kind: string; items: { it: Item; index: number }[] }[] = [];
  items.forEach((it, index) => {
    const g = groups.find((x) => x.kind === it.kind);
    if (g) g.items.push({ it, index });
    else groups.push({ kind: it.kind, items: [{ it, index }] });
  });

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: clic fuera para cerrar (con teclado, Escape)
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-stone-900/30 px-4 pt-[12vh]"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Buscar en TaxPain"
        className="palette w-full max-w-xl overflow-hidden rounded-xl border border-stone-200 bg-white shadow-2xl"
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.preventDefault();
            onClose();
          } else if (e.key === 'ArrowDown') {
            e.preventDefault();
            setActive((a) => Math.min(a + 1, items.length - 1));
          } else if (e.key === 'ArrowUp') {
            e.preventDefault();
            setActive((a) => Math.max(a - 1, 0));
          } else if (e.key === 'Enter') {
            e.preventDefault();
            choose(items[active]);
          }
        }}
      >
        <div className="flex items-center gap-2 border-b border-stone-200 px-4">
          <Search size={18} aria-hidden="true" className="shrink-0 text-stone-400" />
          <input
            ref={input}
            role="combobox"
            aria-expanded={items.length > 0}
            aria-controls={listId}
            aria-activedescendant={items[active] ? `${listId}-${active}` : undefined}
            aria-autocomplete="list"
            aria-label="Buscar"
            placeholder="Factura, cliente, NIF, importe, cuenta o sección…"
            className="h-12 w-full bg-transparent text-[0.95rem]"
            // El cuadro de búsqueda va sin el borde de los campos de formulario.
            style={{ border: 0, boxShadow: 'none', outline: 'none', paddingInline: 0 }}
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
          <kbd className="shrink-0 rounded border border-stone-200 px-1.5 text-xs text-stone-500">
            esc
          </kbd>
        </div>
        <div
          id={listId}
          role="listbox"
          aria-label="Resultados"
          className="max-h-[55vh] overflow-y-auto py-2"
        >
          {error && (
            <p role="alert" className="px-4 py-2 text-sm text-red-700">
              {error}
            </p>
          )}
          {q.trim().length >= 2 && hits.length === 0 && !error && (
            <p className="px-4 py-2 text-sm text-stone-500">Ningún dato coincide.</p>
          )}
          {groups.map((g) => (
            <div key={g.kind} role="presentation">
              <p
                role="presentation"
                className="px-4 pt-2 pb-1 text-xs font-semibold tracking-wide text-stone-500 uppercase"
              >
                {KIND_LABEL[g.kind] ?? g.kind}
              </p>
              {g.items.map(({ it, index }) => {
                const Icon = ICONS[it.kind] ?? Navigation;
                const selected = index === active;
                return (
                  <div
                    key={it.key}
                    id={`${listId}-${index}`}
                    role="option"
                    aria-selected={selected}
                    tabIndex={-1}
                    className={`mx-2 flex cursor-pointer items-center gap-3 rounded-lg px-2 py-2 text-sm ${
                      selected ? 'bg-stone-100 text-stone-900' : 'text-stone-800'
                    }`}
                    onMouseMove={() => setActive(index)}
                    onClick={() => choose(it)}
                    onKeyDown={() => {}}
                  >
                    <Icon size={16} aria-hidden="true" className="shrink-0 text-stone-500" />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-medium">{it.title}</span>
                      <span className="block truncate text-xs text-stone-500">{it.subtitle}</span>
                    </span>
                    {selected && (
                      <CornerDownLeft size={14} aria-hidden="true" className="text-stone-400" />
                    )}
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

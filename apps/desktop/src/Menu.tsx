import { Check } from 'lucide-react';
import {
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import { createPortal } from 'react-dom';

/** Una opción del menú; `checked` la convierte en opción a elegir (con la marca ✓). */
export interface MenuItem {
  label: string;
  onSelect: () => void;
  hint?: string | undefined;
  icon?: ReactNode;
  danger?: boolean;
  disabled?: boolean;
  checked?: boolean;
}
export type MenuEntry = MenuItem | { separator: true } | { heading: string };

const isItem = (e: MenuEntry): e is MenuItem => 'onSelect' in e;

/**
 * Menú de acciones: un botón que abre una lista flotante (para acciones como «Guardar como…» o
 * «Más…», que no son elegir un valor). Con teclado: flechas, Inicio/Fin, Intro, Escape.
 * Se pinta en <body> (o en su diálogo) para que no lo recorte ninguna tarjeta y se abre hacia
 * arriba si abajo no cabe.
 */
export function Menu({
  label,
  entries,
  children,
  className = 'menu-trigger',
  align = 'end',
  width,
}: {
  /** Nombre accesible del botón (p. ej. «Más acciones de A2026-0006»). */
  label: string;
  entries: MenuEntry[];
  /** Lo que se ve en el botón. */
  children: ReactNode;
  className?: string;
  align?: 'start' | 'end';
  width?: string;
}) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ top: number; left: number; maxHeight: number } | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const menuId = useId();

  const close = useCallback((refocus: boolean) => {
    setOpen(false);
    setPos(null);
    if (refocus) trigger.current?.focus();
  }, []);

  // Colocación: debajo del botón (o encima si abajo no cabe), dentro de la ventana. Al desplazar
  // la página el menú sigue al botón; si el botón sale de la vista, se cierra.
  const place = useCallback(() => {
    if (!trigger.current || !pop.current) return;
    const r = trigger.current.getBoundingClientRect();
    if (r.bottom < 0 || r.top > window.innerHeight) {
      close(false);
      return;
    }
    const h = pop.current.scrollHeight + 2;
    const w = pop.current.offsetWidth;
    const gap = 4;
    const below = window.innerHeight - r.bottom - gap - 8;
    const above = r.top - gap - 8;
    const up = h > below && above > below;
    const room = Math.max(120, up ? above : below);
    const top = up ? Math.max(8, r.top - gap - Math.min(h, room)) : r.bottom + gap;
    const left = Math.min(
      Math.max(8, align === 'end' ? r.right - w : r.left),
      window.innerWidth - w - 8,
    );
    setPos({ top, left, maxHeight: room });
  }, [align, close]);

  useLayoutEffect(() => {
    if (open) place();
  }, [open, place]);

  // Ya colocado, el foco va a la primera opción (sin mover la página).
  const placed = pos !== null;
  useEffect(() => {
    if (!open || !placed) return;
    pop.current
      ?.querySelector<HTMLElement>('[role^="menuitem"]:not([aria-disabled="true"])')
      ?.focus({ preventScroll: true });
  }, [open, placed]);

  // Un clic fuera lo cierra; al desplazar o redimensionar se recoloca.
  useEffect(() => {
    if (!open) return;
    const outside = (e: PointerEvent) => {
      const t = e.target as Node;
      if (!pop.current?.contains(t) && !trigger.current?.contains(t)) close(false);
    };
    const moved = (e: Event) => {
      if (!pop.current?.contains(e.target as Node)) place();
    };
    document.addEventListener('pointerdown', outside, true);
    window.addEventListener('scroll', moved, true);
    window.addEventListener('resize', moved);
    return () => {
      document.removeEventListener('pointerdown', outside, true);
      window.removeEventListener('scroll', moved, true);
      window.removeEventListener('resize', moved);
    };
  }, [open, close, place]);

  function onMenuKey(e: KeyboardEvent<HTMLDivElement>) {
    const items = [
      ...(pop.current?.querySelectorAll<HTMLElement>(
        '[role^="menuitem"]:not([aria-disabled="true"])',
      ) ?? []),
    ];
    const at = items.indexOf(document.activeElement as HTMLElement);
    const go = (i: number) => items[(i + items.length) % items.length]?.focus();
    if (e.key === 'ArrowDown') go(at + 1);
    else if (e.key === 'ArrowUp') go(at - 1);
    else if (e.key === 'Home') go(0);
    else if (e.key === 'End') go(items.length - 1);
    else if (e.key === 'Escape') close(true);
    else if (e.key === 'Tab') {
      close(false);
      return;
    } else if (e.key.length === 1 && /\S/.test(e.key)) {
      // Escribir una letra salta a la siguiente opción que empieza por ella.
      const k = e.key.toLowerCase();
      const next = [...items.slice(at + 1), ...items.slice(0, at + 1)].find((el) =>
        el.textContent?.trim().toLowerCase().startsWith(k),
      );
      next?.focus();
    } else return;
    e.preventDefault();
  }

  function onTriggerKey(e: KeyboardEvent<HTMLButtonElement>) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      setOpen(true);
    }
  }

  return (
    <>
      <button
        ref={trigger}
        type="button"
        className={className}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => (open ? close(false) : setOpen(true))}
        onKeyDown={onTriggerKey}
      >
        {children}
      </button>
      {open &&
        createPortal(
          <div
            ref={pop}
            id={menuId}
            role="menu"
            aria-label={label}
            className="menu-pop"
            tabIndex={-1}
            onKeyDown={onMenuKey}
            style={{
              ...(pos
                ? { top: pos.top, left: pos.left, maxHeight: pos.maxHeight }
                : { top: 0, left: 0, visibility: 'hidden' as const }),
              ...(width ? { width } : {}),
            }}
          >
            {entries.map((entry, i) => {
              if ('separator' in entry)
                // biome-ignore lint/suspicious/noArrayIndexKey: la lista es fija mientras está abierto.
                return <hr key={`s${i}`} className="menu-sep" />;
              if ('heading' in entry)
                return (
                  <p key={`h${entry.heading}`} className="menu-heading" role="presentation">
                    {entry.heading}
                  </p>
                );
              if (!isItem(entry)) return null;
              const radio = entry.checked !== undefined;
              return (
                <button
                  // biome-ignore lint/suspicious/noArrayIndexKey: puede haber etiquetas repetidas.
                  key={`i${i}`}
                  type="button"
                  {...(radio
                    ? { role: 'menuitemradio', 'aria-checked': entry.checked }
                    : { role: 'menuitem' })}
                  aria-disabled={entry.disabled || undefined}
                  tabIndex={-1}
                  className={`menu-item${entry.danger ? ' menu-danger' : ''}`}
                  onClick={() => {
                    if (entry.disabled) return;
                    close(true);
                    entry.onSelect();
                  }}
                >
                  {entry.icon && <span className="menu-icon">{entry.icon}</span>}
                  <span className="menu-label">{entry.label}</span>
                  {entry.hint && <span className="menu-hint">{entry.hint}</span>}
                  {radio && (
                    <Check
                      size={15}
                      aria-hidden="true"
                      className="menu-check"
                      style={{ visibility: entry.checked ? 'visible' : 'hidden' }}
                    />
                  )}
                </button>
              );
            })}
          </div>,
          // Dentro de un diálogo modal, en el diálogo: si no, quedaría debajo de él.
          trigger.current?.closest('dialog') ?? document.body,
        )}
    </>
  );
}

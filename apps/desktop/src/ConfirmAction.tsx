import { type ReactNode, useEffect, useRef, useState } from 'react';
import { errorMessage } from './api';
import { useLayer } from './dialogs';

/**
 * Botón que pide confirmación en la propia interfaz antes de una acción irreversible.
 *
 * No usa `window.confirm`: en la app, el plugin de diálogo de Tauri lo sustituye por una función
 * asíncrona (devuelve una promesa, que siempre es «verdadera»), así que no protege de nada.
 * Mientras la acción se ejecuta, los botones quedan desactivados (sin dobles clics). Si la acción
 * falla, el error se muestra aquí mismo y el panel sigue abierto.
 */
export function ConfirmAction({
  label,
  question,
  confirmLabel,
  onConfirm,
  className = 'text-red-700',
  danger = true,
  disabled = false,
  ariaLabel,
}: {
  label: ReactNode;
  question: ReactNode;
  confirmLabel: string;
  onConfirm: () => Promise<void> | void;
  className?: string;
  danger?: boolean;
  /** No se puede pulsar (hay otra operación en marcha). */
  disabled?: boolean;
  /** El nombre entero del botón, si dice más que lo que se ve («Borrar: …»). */
  ariaLabel?: string;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const wasOpen = useRef(false);

  // Al abrir, el foco va al botón de confirmar; al cerrar, vuelve al que abrió el panel.
  useEffect(() => {
    if (open) confirmRef.current?.focus();
    else if (wasOpen.current) triggerRef.current?.focus();
    wasOpen.current = open;
  }, [open]);

  const close = () => {
    setError(null);
    setOpen(false);
  };
  // Dentro de un diálogo, Esc la cierra aunque el foco esté en otra parte del diálogo.
  useLayer(open, () => {
    if (!busy) close();
  });

  if (!open) {
    return (
      <button
        ref={triggerRef}
        type="button"
        className={className}
        disabled={disabled}
        aria-label={ariaLabel}
        onClick={() => setOpen(true)}
      >
        {label}
      </button>
    );
  }
  return (
    <div
      role="alertdialog"
      aria-label={typeof label === 'string' ? label : confirmLabel}
      className="mt-2 space-y-2 rounded-md border border-red-300 bg-red-50 p-3 text-sm text-stone-900"
      onKeyDown={(e) => {
        // Sólo esta capa: dentro de un diálogo modal, Esc no cierra también el diálogo.
        if (e.key === 'Escape') {
          e.preventDefault();
          e.stopPropagation();
          if (!busy) close();
        }
      }}
    >
      <p>{question}</p>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <button
          ref={confirmRef}
          type="button"
          disabled={busy || disabled}
          className={`rounded px-3 py-1.5 disabled:opacity-50 ${danger ? 'btn-danger' : 'btn-primary'}`}
          onClick={async () => {
            setBusy(true);
            setError(null);
            try {
              await onConfirm();
              setOpen(false);
            } catch (e) {
              setError(errorMessage(e));
            } finally {
              setBusy(false);
            }
          }}
        >
          {confirmLabel}
        </button>
        <button
          type="button"
          disabled={busy}
          className="rounded border border-stone-300 px-3 py-1.5"
          onClick={close}
        >
          Cancelar
        </button>
      </div>
    </div>
  );
}

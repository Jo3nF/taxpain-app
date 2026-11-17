import { Lock } from 'lucide-react';
import { useEffect, useId, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { setPreguntaPeriodo } from './api';

/**
 * Aviso de periodo cerrado: cualquier operación que Rust rechace por caer en un periodo cerrado
 * (emitir, registrar, cambiar un asiento) llega aquí; si el usuario confirma, la operación se
 * repite con una excepción que queda en la auditoría (ver `invoke` en `api.ts`).
 */
export function PeriodoCerradoGate() {
  const [pregunta, setPregunta] = useState<{
    texto: string;
    resolver: (ok: boolean) => void;
  } | null>(null);
  const titleId = useId();
  const cancelRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    setPreguntaPeriodo(
      (texto) =>
        new Promise<boolean>((resolver) => {
          setPregunta({ texto, resolver });
        }),
    );
    return () => setPreguntaPeriodo(null);
  }, []);

  useEffect(() => {
    if (pregunta) cancelRef.current?.focus();
  }, [pregunta]);

  if (!pregunta) return null;
  const responder = (ok: boolean) => {
    pregunta.resolver(ok);
    setPregunta(null);
  };
  return createPortal(
    <div className="fixed inset-0 z-[60] flex items-start justify-center bg-stone-900/30 px-4 py-[12vh]">
      <div
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="w-full max-w-lg space-y-3 rounded-xl border border-stone-200 bg-white p-4 text-sm shadow-2xl"
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.stopPropagation();
            responder(false);
          }
        }}
      >
        <h2 id={titleId} className="flex items-center gap-2 text-base font-semibold">
          <Lock size={16} aria-hidden="true" />
          Periodo cerrado
        </h2>
        <p>{pregunta.texto}</p>
        <p className="text-stone-600">Si sigues, queda anotado en la auditoría.</p>
        <div className="flex justify-end gap-2">
          <button
            ref={cancelRef}
            type="button"
            className="rounded-md border border-stone-300 bg-white px-3 py-1.5"
            onClick={() => responder(false)}
          >
            No, dejarlo
          </button>
          <button
            type="button"
            className="rounded-md bg-amber-700 px-3 py-1.5 text-white"
            onClick={() => responder(true)}
          >
            Sí, seguir igualmente
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

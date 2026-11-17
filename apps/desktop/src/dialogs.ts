import {
  createContext,
  type KeyboardEvent,
  type RefObject,
  useContext,
  useEffect,
  useRef,
} from 'react';

/**
 * Capas abiertas dentro de un diálogo modal (una confirmación, un formulario en línea), por el
 * orden en que se han abierto. El diálogo, al pulsar Esc, cierra la última, esté donde esté el
 * foco; sin ninguna, se cierra él.
 */
export const LayersContext = createContext<{ current: (() => void)[] } | null>(null);

/** Mientras `open`, Esc en el diálogo de alrededor llama a `close` en vez de cerrarlo. */
export function useLayer(open: boolean, close: () => void) {
  const layers = useContext(LayersContext);
  const latest = useRef(close);
  latest.current = close;
  useEffect(() => {
    if (!open || !layers) return;
    const f = () => latest.current();
    layers.current.push(f);
    return () => {
      layers.current = layers.current.filter((x) => x !== f);
    };
  }, [open, layers]);
}

/**
 * Panel en línea que se comporta como un diálogo: al estar listo, el foco va a su primer control;
 * Esc lo cierra (salvo mientras trabaja) y no llega más arriba (dentro de un diálogo modal no
 * cierra también el diálogo).
 */
export function useInlineDialog(ready: boolean, busy: boolean, onClose: () => void) {
  const ref = useRef<HTMLDivElement>(null);
  useLayer(true, () => {
    if (!busy) onClose();
  });
  useEffect(() => {
    if (ready) ref.current?.querySelector<HTMLElement>('select, input, button')?.focus();
  }, [ready]);
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      if (!busy) onClose();
    }
  };
  return { ref, onKeyDown };
}

/** Al cerrarse el panel, el foco vuelve al botón que lo abrió. */
export function useReturnFocus(open: boolean, trigger: RefObject<HTMLButtonElement | null>) {
  const was = useRef(open);
  useEffect(() => {
    if (was.current && !open) trigger.current?.focus();
    was.current = open;
  }, [open, trigger]);
}

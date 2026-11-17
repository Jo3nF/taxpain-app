import { useCallback, useEffect, useRef, useState } from 'react';

/**
 * Texto para un `role="status"` y la función que lo cambia. El mismo mensaje dos veces seguidas
 * («Dato añadido…», «Copiada.») también se anuncia: antes de ponerlo se vacía un momento (si el
 * texto no cambia, el lector de pantalla no dice nada).
 */
export function useAnnounce(): [string, (message: string) => void] {
  const [text, setText] = useState('');
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const say = useCallback((message: string) => {
    clearTimeout(timer.current);
    setText('');
    if (message) timer.current = setTimeout(() => setText(message), 50);
  }, []);
  return [text, say];
}

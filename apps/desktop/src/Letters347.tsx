import { useState } from 'react';
import { errorMessage, modelLetters347, pickFolder } from './api';

/** Cartas a cada cliente y proveedor del 347 con sus importes, para que los comprueben. */
export function Letters347({ year }: { year: number }) {
  const today = new Date().toLocaleDateString('es-ES', {
    day: 'numeric',
    month: 'long',
    year: 'numeric',
  });
  const [place, setPlace] = useState(today);
  const [msg, setMsg] = useState<string | null>(null);
  return (
    <section aria-label="Cartas del 347" className="mt-4 rounded-lg border bg-white p-3 text-sm">
      <h3 className="font-medium">Cartas a clientes y proveedores</h3>
      <p className="text-stone-600">
        Una carta por tercero con el importe anual y por trimestres, para que lo comprueben antes de
        presentar.
      </p>
      <div className="mt-3 flex flex-wrap items-end gap-3">
        <label>
          Lugar y fecha
          <input
            className="mt-1 block w-72"
            value={place}
            onChange={(e) => setPlace(e.target.value)}
          />
        </label>
        <button
          type="button"
          className="rounded border border-stone-300 px-3"
          onClick={async () => {
            const dir = await pickFolder();
            if (!dir) return;
            try {
              setMsg(`Guardadas en ${await modelLetters347(year, place, dir)}`);
            } catch (e) {
              setMsg(errorMessage(e));
            }
          }}
        >
          Generar las cartas (PDF)…
        </button>
      </div>
      {msg && <p role="status">{msg}</p>}
    </section>
  );
}

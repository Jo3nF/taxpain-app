import { useState } from 'react';
import { errorMessage, type UpdateInfo, updateCheck, updateInstall } from './api';
import { ConfirmAction } from './ConfirmAction';

/** Buscar e instalar actualizaciones (firmadas; la app verifica la firma antes de instalar). */
export function Updates({ version }: { version: string | null }) {
  const [state, setState] = useState<'idle' | 'checking' | 'none' | 'error'>('idle');
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  return (
    <section className="mt-8" aria-labelledby="updates-h">
      <h2 id="updates-h" className="text-lg font-semibold">
        Actualizaciones
      </h2>
      <p className="mt-1 text-sm text-stone-600">
        Versión instalada: {version ?? '—'}. Las actualizaciones van firmadas y la app comprueba la
        firma antes de instalarlas.
      </p>
      <div className="mt-2 flex flex-wrap items-center gap-3 text-sm">
        <button
          type="button"
          disabled={state === 'checking'}
          className="rounded border border-stone-300 px-2 py-1"
          onClick={async () => {
            setState('checking');
            setMessage(null);
            try {
              const u = await updateCheck();
              setUpdate(u);
              setState(u ? 'idle' : 'none');
            } catch (e) {
              setMessage(errorMessage(e));
              setState('error');
            }
          }}
        >
          {state === 'checking' ? 'Buscando…' : 'Buscar actualizaciones'}
        </button>
        {state === 'none' && <span role="status">Tienes la última versión.</span>}
      </div>
      {update && (
        <div className="mt-3 rounded-md border border-emerald-300 bg-emerald-50 p-3 text-sm">
          <p className="font-medium">Hay una versión nueva: {update.version}</p>
          {update.notes && <p className="mt-1 whitespace-pre-wrap">{update.notes}</p>}
          <ConfirmAction
            className="mt-2 rounded btn-primary px-3 py-1.5"
            label="Instalar y reiniciar…"
            question="Se descargará la versión nueva, se comprobará su firma y la app se reiniciará. Tus datos no cambian."
            confirmLabel="Sí, actualizar"
            danger={false}
            onConfirm={async () => {
              try {
                await updateInstall();
              } catch (e) {
                setMessage(errorMessage(e));
              }
            }}
          />
        </div>
      )}
      {message && (
        <p role="alert" className="mt-2 text-sm text-red-700">
          {message}
        </p>
      )}
    </section>
  );
}

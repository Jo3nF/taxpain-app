import { BellRing } from 'lucide-react';
import { useEffect, useState } from 'react';
import { autostartGet, autostartSet, errorMessage } from './api';

/**
 * Avisos de plazos aunque TaxPain no esté abierta: arranque al iniciar sesión, sin ventana.
 *
 * Sólo promete lo que de verdad hace en segundo plano (plazos, copias automáticas, carpeta
 * vigilada). Las notificaciones de la AEAT necesitan el certificado y la confirmación del
 * usuario (Touch ID): se consultan al abrir Notificaciones, nunca solas.
 */
export function Background() {
  const [on, setOn] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    autostartGet()
      .then(setOn)
      .catch(() => setOn(null));
  }, []);
  return (
    <section className="mt-8" aria-labelledby="background-title">
      <div className="flex items-start gap-3">
        <span className="icon-tile" aria-hidden="true">
          <BellRing size={18} />
        </span>
        <div>
          <h2 id="background-title" className="text-lg font-semibold">
            Avisos de plazos
          </h2>
          <p className="text-sm text-stone-600">
            Cerrar la ventana no cierra TaxPain: sigue en el Dock, te avisa de los plazos de los
            modelos, hace las copias de seguridad automáticas y recoge las facturas de la carpeta
            vigilada. Para cerrarla del todo, ⌘Q.
          </p>
          <p className="mt-2 text-sm text-stone-600">
            Las notificaciones de la AEAT no se miran solas: hace falta tu certificado y tu
            confirmación, y se consultan al abrir <strong>Notificaciones</strong>. Ábrela a menudo o
            activa los avisos por correo de la AEAT o de la DEHú: una notificación que no abres en
            10 días naturales se da por notificada (art. 43 Ley 39/2015).
          </p>
        </div>
      </div>
      <div className="card mt-4 p-5 text-sm">
        <label className="flex items-start gap-2">
          <input
            type="checkbox"
            role="switch"
            aria-checked={on === true}
            disabled={on === null || busy}
            checked={on === true}
            onChange={async (e) => {
              setBusy(true);
              setError(null);
              try {
                setOn(await autostartSet(e.target.checked));
              } catch (err) {
                setError(errorMessage(err));
              } finally {
                setBusy(false);
              }
            }}
          />
          <span>
            Abrir TaxPain al iniciar sesión, sin ventana
            <span className="block text-stone-500">
              Así los avisos de plazos, las copias automáticas y la carpeta vigilada funcionan
              aunque no la abras. Se puede quitar aquí o en Ajustes del Sistema → General → Ítems de
              inicio.
            </span>
          </span>
        </label>
        {error && (
          <p role="alert" className="mt-2 text-red-700">
            {error}
          </p>
        )}
      </div>
    </section>
  );
}

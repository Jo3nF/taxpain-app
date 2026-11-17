import { CalendarRange } from 'lucide-react';
import type { EstadoVigencia, VigenciaEjercicio } from './api';

const ETIQUETA: Record<EstadoVigencia, string> = {
  listo: 'Cargado',
  supuesto: 'Se usa el del año anterior',
  pendiente: 'Pendiente de publicar',
  revisar: 'Revísalo tú',
};

const PILL: Record<EstadoVigencia, string> = {
  listo: 'pill-stone',
  supuesto: 'pill-amber',
  pendiente: 'pill-amber',
  revisar: 'pill-stone',
};

/**
 * Aviso del Panel en diciembre y enero (`taxpain_app::cambio_ejercicio`): qué datos del año
 * nuevo tiene TaxPain, cuáles se prorrogan del anterior hasta que se publiquen y qué hay que
 * revisar a mano. Lo cargado va plegado al final.
 */
export function CambioEjercicio({ vigencia }: { vigencia: VigenciaEjercicio }) {
  const abiertos = vigencia.comprobaciones.filter((c) => c.estado !== 'listo');
  const listos = vigencia.comprobaciones.filter((c) => c.estado === 'listo');
  if (abiertos.length === 0) return null;
  const titulo = `Cambio de año: qué revisar para ${vigencia.year}`;
  return (
    <section className="card mt-4 p-5" aria-labelledby="cambio-ejercicio-h">
      <h3 id="cambio-ejercicio-h" className="flex items-center gap-2 text-sm font-semibold">
        <CalendarRange size={16} aria-hidden="true" className="text-amber-700" />
        {titulo}
      </h3>
      <p className="mt-1 text-sm text-stone-600">
        Lo que cambia cada año y TaxPain todavía no tiene confirmado. Mientras tanto, los plazos y
        los ficheros se calculan con lo último vigente y lo avisan.
      </p>
      <ul className="mt-3 space-y-2 text-sm">
        {abiertos.map((c) => (
          <li key={c.clave} className="rounded-lg bg-stone-50 px-3 py-2">
            <span className="flex flex-wrap items-center justify-between gap-2">
              <span className="font-medium">{c.titulo}</span>
              <span className={`pill ${PILL[c.estado]}`}>{ETIQUETA[c.estado]}</span>
            </span>
            <span className="mt-1 block text-stone-600">{c.detalle}</span>
          </li>
        ))}
      </ul>
      {listos.length > 0 && (
        <details className="mt-3 text-sm">
          <summary className="cursor-pointer text-stone-500">
            Ya cargado para {vigencia.year} ({listos.length})
          </summary>
          <ul className="mt-2 space-y-1">
            {listos.map((c) => (
              <li key={c.clave}>
                <span className="font-medium">{c.titulo}</span>: {c.detalle}
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}

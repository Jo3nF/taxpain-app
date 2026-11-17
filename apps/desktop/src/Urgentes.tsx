import { OctagonAlert } from 'lucide-react';
import { euros, formatDate, type Hallazgo, type Urgente } from './api';

/** Título de la sección, como en el informe. */
export const URGENTE_TITULO = 'Urgente: puede haber deuda';

/** Los hallazgos urgentes, en el orden de la revisión (ya vienen primero). */
export function urgentes(hs: Hallazgo[]): (Hallazgo & { urgente: Urgente })[] {
  return hs.filter((h): h is Hallazgo & { urgente: Urgente } => !!h.urgente);
}

/** Recargo de hoy y siguiente escalón, en una línea (como `Urgente::resumen` en Rust). */
export function textoUrgente(u: Urgente): string {
  const p: string[] = [];
  if (u.en_juego !== null) p.push(`En juego: ${euros(u.en_juego)}`);
  if (u.vencio) p.push(`venció el ${formatDate(u.vencio)}`);
  if (u.porcentaje_hoy !== null)
    p.push(
      `recargo hoy ${u.porcentaje_hoy} %${u.recargo_hoy !== null ? `: ${euros(u.recargo_hoy)}` : ''}`,
    );
  if (u.siguiente_porcentaje !== null) {
    const sube =
      u.siguiente_recargo !== null
        ? `: ${euros(u.siguiente_recargo)} (${euros(u.siguiente_recargo - (u.recargo_hoy ?? 0))} más)`
        : '';
    p.push(
      u.siguiente_desde
        ? `desde el ${formatDate(u.siguiente_desde)} sube al ${u.siguiente_porcentaje} %${sube}`
        : `siguiente escalón ${u.siguiente_porcentaje} %${sube}`,
    );
  }
  return p.length ? `${p.join('; ')}.` : 'Importe sin calcular.';
}

/**
 * Bloque destacado con lo que puede suponer una deuda con recargo. No pinta nada si no hay.
 * `compacto` (en el Panel): sólo el título de cada uno y su línea de recargo.
 */
export function BloqueUrgente({
  hallazgos,
  id,
  compacto = false,
  onOpen,
}: {
  hallazgos: Hallazgo[];
  /** Para el `aria-labelledby` (único en la pantalla). */
  id: string;
  compacto?: boolean;
  /** Botón para ir a la revisión completa. */
  onOpen?: { label: string; go: () => void };
}) {
  const us = urgentes(hallazgos);
  if (us.length === 0) return null;
  const total = us.reduce(
    (a, h) => a + (h.urgente.en_juego ?? 0) + (h.urgente.recargo_hoy ?? 0),
    0,
  );
  return (
    <section
      aria-labelledby={id}
      className="rounded-lg border-2 border-red-600 bg-red-50 p-4 text-red-900"
    >
      <h3 id={id} className="flex items-center gap-2 font-semibold">
        <OctagonAlert size={18} aria-hidden="true" />
        {URGENTE_TITULO}
        <span className="text-sm font-normal">
          · {us.length === 1 ? '1 aviso' : `${us.length} avisos`}
          {total > 0 ? ` · en juego ${euros(total)}` : ''}
        </span>
      </h3>
      <p className="mt-1 text-sm">
        Pueden suponer dinero pendiente con Hacienda o la Seguridad Social, y el recargo sube si se
        espera. TaxPain no sabe si la deuda ya está en periodo ejecutivo ni si Hacienda la ha
        notificado: compruébalo en la Sede.
      </p>
      <ul className="mt-3 space-y-2">
        {us.map((h) => (
          <li
            key={`${h.codigo}-${h.ejercicio}-${h.titulo}`}
            className="rounded-md bg-white p-3 text-sm text-stone-800"
          >
            <p className="font-semibold text-red-800">{h.titulo}</p>
            <p className="mt-0.5 font-medium">{textoUrgente(h.urgente)}</p>
            {!compacto && <p className="mt-1 text-stone-700">{h.urgente.nota}</p>}
          </li>
        ))}
      </ul>
      {onOpen && (
        <button type="button" className="btn-ghost mt-3" onClick={onOpen.go}>
          {onOpen.label}
        </button>
      )}
    </section>
  );
}

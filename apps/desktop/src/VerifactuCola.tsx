import { useEffect, useState } from 'react';
import {
  errorMessage,
  type VerifactuQueue,
  verifactuQueueReopen,
  verifactuQueueStatus,
} from './api';
import { ConfirmAction } from './ConfirmAction';

function hora(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? iso
    : d.toLocaleTimeString('es-ES', { hour: '2-digit', minute: '2-digit' });
}

/** Estado de la cola de remisión VERI*FACTU: lo que falta por enviar, cuándo se puede volver a
 * enviar (TiempoEsperaEnvio de la AEAT), las incidencias y lo que la AEAT no ha contestado.
 * `refresh` cambia cada vez que se envía o se emite algo. */
export function VerifactuCola({ refresh }: { refresh: number }) {
  const [q, setQ] = useState<VerifactuQueue | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `refresh` sólo dispara la recarga.
  useEffect(() => {
    let live = true;
    Promise.resolve()
      .then(() => verifactuQueueStatus())
      .then((r) => live && setQ(r ?? null))
      .catch(() => live && setQ(null));
    return () => {
      live = false;
    };
  }, [refresh]);
  if (!q) return null;
  const notas: string[] = [];
  if (q.espera_segundos > 0 && q.pendientes > 0 && q.proximo_envio)
    notas.push(
      `La AEAT pide ${q.tiempo_espera} s entre envíos: el siguiente puede salir a partir de las ${hora(q.proximo_envio)}.`,
    );
  if (q.rechazados > 0 || q.aceptados_con_errores > 0)
    notas.push(
      `${q.rechazados} rechazado${q.rechazados === 1 ? '' : 's'} y ${q.aceptados_con_errores} aceptado${q.aceptados_con_errores === 1 ? '' : 's'} con errores por subsanar.`,
    );
  if (!q.aviso && !q.aviso_modo && notas.length === 0) return null;
  return (
    <div className="mt-3 rounded-md border border-amber-300 bg-amber-50 p-3 text-sm">
      <p className="font-medium">
        Remisión VERI*FACTU {q.entorno === 'pruebas' ? '(entorno de pruebas)' : ''}
      </p>
      {q.aviso_modo && <p className="mt-1">{q.aviso_modo}</p>}
      {q.aviso && (
        <p role="alert" className="mt-1">
          {q.aviso}
        </p>
      )}
      {notas.map((n) => (
        <p key={n} className="mt-1">
          {n}
        </p>
      ))}
      {q.bloqueada && (
        <ConfirmAction
          className="mt-2 rounded border border-stone-300 px-2 py-1"
          label="Reintentar…"
          question="La cola volverá a enviar los registros pendientes en el próximo envío. Hazlo sólo si has corregido lo que rechazó la AEAT."
          confirmLabel="Sí, reintentar"
          danger={false}
          onConfirm={async () => {
            try {
              await verifactuQueueReopen();
              setQ(await verifactuQueueStatus());
              setMessage('Cola desbloqueada: pulsa «Enviar a la AEAT» para enviar.');
            } catch (e) {
              setMessage(errorMessage(e));
            }
          }}
        />
      )}
      {message && (
        <p role="status" className="mt-1">
          {message}
        </p>
      )}
    </div>
  );
}

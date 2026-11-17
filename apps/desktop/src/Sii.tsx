import { useEffect, useState } from 'react';
import {
  errorMessage,
  formatDate,
  type SiiEstado,
  type SiiPlazo,
  siiEstado,
  siiPlazos,
} from './api';

/** Fecha local en `aaaa-mm-dd`. */
function iso(d: Date): string {
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${d.getFullYear()}-${m}-${day}`;
}

/**
 * Suministro Inmediato de Información (SII): si es obligatorio y, si se lleva, el plazo de
 * envío de las facturas de los últimos días (cuatro días hábiles y antes del día 16 del mes
 * siguiente, art. 69 bis RIVA). Nada se envía.
 */
export function SiiPanel() {
  const [estado, setEstado] = useState<SiiEstado | null>(null);
  const [plazos, setPlazos] = useState<SiiPlazo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    siiEstado()
      .then(async (e) => {
        if (!live) return;
        setEstado(e);
        if (!e.obligatorio && !e.voluntario) return;
        const hoy = new Date();
        const desde = new Date(hoy.getFullYear(), hoy.getMonth() - 1, 1);
        const p = await siiPlazos(iso(desde), iso(hoy));
        if (live) setPlazos(p);
      })
      .catch((e) => {
        if (live) setError(errorMessage(e));
      });
    return () => {
      live = false;
    };
  }, []);
  if (error) {
    return (
      <p role="alert" className="mt-4 text-sm text-red-700">
        {error}
      </p>
    );
  }
  if (!estado) return null;
  const lleva = estado.obligatorio || estado.voluntario;
  if (!lleva && estado.avisos.length === 0) return null;
  const pendientes = (plazos ?? []).filter((p) => p.dias >= -3);
  return (
    <section aria-labelledby="sii-h" className="card mt-6 p-5">
      <h3 id="sii-h" className="font-semibold">
        Suministro Inmediato de Información (SII)
      </h3>
      <p className="mt-1 text-sm">
        {estado.obligatorio
          ? 'Tienes que llevar los libros registro del IVA por el SII.'
          : estado.voluntario
            ? 'Llevas los libros registro del IVA por el SII de forma voluntaria.'
            : 'Todavía no tienes que llevar los libros por el SII.'}
      </p>
      {estado.motivos.length > 0 && (
        <ul className="mt-2 list-disc pl-5 text-sm">
          {estado.motivos.map((m) => (
            <li key={m}>{m}</li>
          ))}
        </ul>
      )}
      {estado.avisos.map((a) => (
        <p key={a} className="mt-2 text-sm text-amber-800">
          {a}
        </p>
      ))}
      {lleva && (
        <p className="mt-2 text-xs text-stone-600">
          Cada factura se envía en cuatro días (sin sábados, domingos ni festivos nacionales) desde
          que se expide o se registra, y siempre antes del día 16 del mes siguiente. Con SII el 303
          es mensual y no se presentan el 390 ni el 347.{' '}
          {estado.formato_sii
            ? ''
            : 'TaxPain no genera aún los mensajes del SII ni envía nada: los libros registro salen en Libros y se suben desde la Sede o con el programa de la gestoría.'}
        </p>
      )}
      {lleva && plazos && (
        <table className="mt-3 w-full text-sm">
          <caption className="text-left font-medium">Facturas del último mes y su plazo</caption>
          <thead>
            <tr>
              <th className="text-left">Libro</th>
              <th className="text-left">Factura</th>
              <th className="text-left">Fecha</th>
              <th className="text-left">Enviar hasta</th>
            </tr>
          </thead>
          <tbody>
            {pendientes.length === 0 && (
              <tr>
                <td colSpan={4} className="text-stone-500">
                  No hay facturas con el plazo abierto.
                </td>
              </tr>
            )}
            {pendientes.map((p) => (
              <tr key={`${p.libro}-${p.numero}-${p.nif}`} className={p.dias < 0 ? 'bg-red-50' : ''}>
                <td>{p.libro === 'emitida' ? 'Expedidas' : 'Recibidas'}</td>
                <td>
                  {p.numero}
                  {p.nombre ? ` · ${p.nombre}` : ''}
                </td>
                <td>{formatDate(p.fecha)}</td>
                <td>
                  {formatDate(p.limite)}
                  {p.dias < 0 ? ' (vencido)' : p.dias === 0 ? ' (hoy)' : ''}
                  {p.aproximado ? ' · aprox.' : ''}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <p className="mt-2 text-xs text-stone-500">{estado.norma}</p>
    </section>
  );
}

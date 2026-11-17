import { useCallback, useEffect, useState } from 'react';
import { errorMessage, formatDate, openSede } from './api';
import {
  type EstadoObligacion,
  type Obligacion,
  type Obligaciones,
  obligacionesNoFiscales,
  obligacionNoFiscalMarcar,
} from './trimestreApi';

const ESTADOS: [EstadoObligacion, string][] = [
  ['sin_revisar', 'Sin revisar'],
  ['aplica', 'Aplica'],
  ['no_aplica', 'No aplica'],
  ['hecho', 'Hecho'],
];

function Ficha({
  o,
  hayEmpleados,
  onGuardar,
}: {
  o: Obligacion;
  hayEmpleados: boolean;
  onGuardar: (o: Obligacion, estado: EstadoObligacion, fecha: string | null, nota: string) => void;
}) {
  const [fecha, setFecha] = useState(o.fecha ?? '');
  const [nota, setNota] = useState(o.nota);
  return (
    <li className="rounded-lg border border-stone-200 p-3">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <h4 className="font-semibold">{o.titulo}</h4>
        <label className="flex items-center gap-2 text-xs">
          <span className="sr-only">Estado: {o.titulo}</span>
          <select
            value={o.estado}
            onChange={(e) => onGuardar(o, e.target.value as EstadoObligacion, fecha || null, nota)}
            className="rounded border border-stone-300 px-2 py-1"
          >
            {ESTADOS.map(([v, l]) => (
              <option key={v} value={v}>
                {l}
              </option>
            ))}
          </select>
        </label>
      </div>
      <p className="mt-1 text-stone-700">{o.que_es}</p>
      <p className="mt-1 text-stone-600">
        <strong>A quién:</strong> {o.a_quien} <strong>Plazo:</strong> {o.plazo}{' '}
        <strong>Norma:</strong> {o.norma}
      </p>
      {o.con_empleados && !hayEmpleados && o.estado === 'sin_revisar' && (
        <p className="mt-1 text-stone-600">No tienes empleados en Nóminas.</p>
      )}
      {o.enlaces.length > 0 && (
        <p className="mt-1 flex flex-wrap gap-3">
          {o.enlaces.map((e) => (
            <button
              key={e.url}
              type="button"
              className="text-link underline"
              onClick={() => openSede(e.url).catch(() => undefined)}
            >
              {e.label}
            </button>
          ))}
        </p>
      )}
      <div className="mt-2 flex flex-wrap items-center gap-3 text-xs">
        <label className="flex items-center gap-1">
          Fecha que recordar ({o.titulo})
          <input
            type="date"
            value={fecha}
            onChange={(e) => setFecha(e.target.value)}
            onBlur={() => fecha !== (o.fecha ?? '') && onGuardar(o, o.estado, fecha || null, nota)}
            className="rounded border border-stone-300 px-2 py-0.5"
          />
        </label>
        <label className="flex flex-1 items-center gap-1">
          Nota ({o.titulo})
          <input
            type="text"
            value={nota}
            onChange={(e) => setNota(e.target.value)}
            onBlur={() => nota !== o.nota && onGuardar(o, o.estado, fecha || null, nota)}
            className="flex-1 rounded border border-stone-300 px-2 py-0.5"
          />
        </label>
        {o.fecha && <span className="text-stone-500">{formatDate(o.fecha)}</span>}
      </div>
    </li>
  );
}

/**
 * Otras obligaciones de la sociedad, que no son impuestos: una lista de control que el usuario
 * marca. No asesora: dice qué es, a quién suele aplicar y la norma.
 */
export function ObligacionesNoFiscales() {
  const [data, setData] = useState<Obligaciones | null>(null);
  const [error, setError] = useState<string | null>(null);
  const cargar = useCallback(() => {
    obligacionesNoFiscales()
      .then(setData)
      .catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(cargar, [cargar]);
  const guardar = (o: Obligacion, estado: EstadoObligacion, fecha: string | null, nota: string) => {
    setError(null);
    obligacionNoFiscalMarcar(o.id, estado, fecha, nota)
      .then(cargar)
      .catch((e) => setError(errorMessage(e)));
  };
  if (!data && !error) return null;
  const hechas = data?.lista.filter((o) => o.estado === 'hecho' || o.estado === 'no_aplica').length;
  return (
    <section aria-labelledby="obligaciones-t" className="card mt-6 p-5 text-sm">
      <h2 id="obligaciones-t" className="text-lg font-semibold">
        Otras obligaciones (no fiscales)
      </h2>
      <p className="mt-1 text-stone-600">
        Las que no lleva el calendario de la AEAT y que, sin gestoría, nadie te recuerda. TaxPain no
        decide si te aplican: márcalo tú. Ante la duda, consúltalo con un profesional.
      </p>
      {error && (
        <p role="alert" className="mt-3 rounded-md bg-red-50 p-3 text-red-900">
          {error}
        </p>
      )}
      {data && (
        <>
          <p className="mt-2">
            {hechas} de {data.lista.length} revisadas (hechas o que no aplican).
          </p>
          <ul className="mt-3 space-y-3">
            {data.lista.map((o) => (
              <Ficha
                key={`${o.id}-${o.estado}-${o.fecha ?? ''}-${o.nota}`}
                o={o}
                hayEmpleados={data.hay_empleados}
                onGuardar={guardar}
              />
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

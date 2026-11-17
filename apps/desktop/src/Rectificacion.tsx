import { useState } from 'react';
import {
  errorMessage,
  formatDate,
  type IssuedSummary,
  invoiceNumbering,
  plural,
  type SerieNumeracion,
} from './api';

/** Motivos de rectificación (claves R1–R5 de VERI*FACTU; art. 15 RD 1619/2012, art. 80 LIVA). */
export const MOTIVOS: { code: number; titulo: string; explicacion: string }[] = [
  {
    code: 1,
    titulo: 'Error o art. 80.Uno, Dos y Seis LIVA',
    explicacion:
      'Errores de la factura (un dato, el tipo de IVA o el importe mal puestos) y los cambios del art. 80.Uno, Dos y Seis LIVA: devoluciones de mercancía o envases, descuentos y bonificaciones después de la venta, operación anulada o resuelta y alteración del precio.',
  },
  {
    code: 2,
    titulo: 'Concurso del cliente (art. 80.Tres)',
    explicacion:
      'El cliente ha sido declarado en concurso de acreedores después del devengo: se reduce la base y la cuota de lo que no ha pagado, por diferencias. Hay un plazo que cuenta desde el fin del plazo de comunicación de créditos del concurso, y la rectificativa se envía a la administración concursal y se comunica a la AEAT en el mes siguiente.',
  },
  {
    code: 4,
    titulo: 'Resto de causas',
    explicacion:
      'Cualquier otra corrección que no sea un error ni una de las causas del art. 80 LIVA.',
  },
];

export const R3_HINT =
  '¿Un cliente que no paga? La rectificativa por crédito incobrable (R3) se emite desde Cobros y pagos › Créditos incobrables, que comprueba los plazos y la reclamación.';

export type RectMethod = 'Diferencias' | 'Sustitucion';

/** Lo que se elige al rectificar: motivo, tipo y otras facturas que agrupa. */
export interface RectChoice {
  code: number;
  method: RectMethod;
  also: IssuedSummary[];
}

export function RectificacionFields({
  rectifies,
  choice,
  onChange,
  others,
  canSubstitute,
}: {
  rectifies: IssuedSummary;
  choice: RectChoice;
  onChange: (c: RectChoice) => void;
  /** Otras facturas del mismo cliente que se pueden agrupar. */
  others: IssuedSummary[];
  /** Se conoce la factura (para partir de sus líneas al sustituirla). */
  canSubstitute: boolean;
}) {
  const simplified = rectifies.kind === 'F2';
  const motivo = MOTIVOS.find((m) => m.code === choice.code);
  const numbers = [rectifies, ...choice.also].map((i) => i.number).join(', ');
  return (
    <fieldset className="space-y-2 rounded-md border border-stone-200 p-3 text-sm">
      <legend className="px-1 font-medium">Rectificativa de {numbers}</legend>
      {simplified ? (
        <p className="text-stone-700">
          Rectifica una factura simplificada: lleva la clave R5 y va sin los datos del cliente.
        </p>
      ) : (
        <>
          <label className="block">
            Motivo
            <select
              className="ml-2 rounded border border-stone-300 px-2 py-1"
              value={choice.code}
              onChange={(e) => {
                const code = Number(e.target.value);
                onChange({ ...choice, code, method: code === 2 ? 'Diferencias' : choice.method });
              }}
            >
              {MOTIVOS.map((m) => (
                <option key={m.code} value={m.code}>
                  R{m.code} · {m.titulo}
                </option>
              ))}
            </select>
          </label>
          {motivo && <p className="text-stone-600">{motivo.explicacion}</p>}
          <p className="text-stone-600">{R3_HINT}</p>
        </>
      )}
      <div role="radiogroup" aria-label="Tipo de rectificativa" className="flex flex-wrap gap-4">
        <label>
          <input
            type="radio"
            name="rect-method"
            checked={choice.method === 'Diferencias'}
            onChange={() => onChange({ ...choice, method: 'Diferencias' })}
          />{' '}
          Por diferencias
        </label>
        <label>
          <input
            type="radio"
            name="rect-method"
            disabled={choice.code === 2 || !canSubstitute}
            checked={choice.method === 'Sustitucion'}
            onChange={() => onChange({ ...choice, method: 'Sustitucion' })}
          />{' '}
          Por sustitución
        </label>
      </div>
      <p className="text-stone-600">
        {choice.method === 'Diferencias'
          ? 'Indica sólo lo que cambia: importes en negativo para reducir (un abono). Es lo que se declara y se contabiliza.'
          : `Indica los importes correctos completos: sustituye a ${numbers}. En VERI*FACTU lleva el importe de lo sustituido, y en los modelos (303, 349, 347, 130), el libro registro y la contabilidad sólo cuenta la diferencia.`}
      </p>
      {others.length > 0 && (
        <details>
          <summary className="cursor-pointer text-link">
            Agrupar otras facturas del mismo cliente ({choice.also.length})
          </summary>
          <ul className="mt-2 space-y-1">
            {others.map((o) => (
              <li key={o.id}>
                <label>
                  <input
                    type="checkbox"
                    checked={choice.also.some((x) => x.id === o.id)}
                    onChange={(e) =>
                      onChange({
                        ...choice,
                        also: e.target.checked
                          ? [...choice.also, o]
                          : choice.also.filter((x) => x.id !== o.id),
                      })
                    }
                  />{' '}
                  {o.number} del {formatDate(o.issue_date)}
                </label>
              </li>
            ))}
          </ul>
        </details>
      )}
    </fieldset>
  );
}

/** Control de la numeración de las emitidas del año: huecos, orden de fechas y series. */
export function NumberingCheck({ year }: { year: number }) {
  const [rows, setRows] = useState<SerieNumeracion[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  function load() {
    setError(null);
    invoiceNumbering(year)
      .then(setRows)
      .catch((e) => setError(errorMessage(e)));
  }
  return (
    <details
      className="mt-3 text-sm"
      onToggle={(e) => {
        if ((e.target as HTMLDetailsElement).open) load();
      }}
    >
      <summary className="cursor-pointer text-link">Control de numeración {year}</summary>
      <p className="mt-1 text-stone-600">
        Cada clase de factura va en su serie (las rectificativas, en la suya) y la numeración es
        correlativa y sin huecos. Las anuladas conservan su número.
      </p>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      {rows && rows.length === 0 && <p className="mt-1">No hay facturas emitidas en {year}.</p>}
      {rows && rows.length > 0 && (
        <ul className="mt-2 space-y-2">
          {rows.map((r) => {
            const ok =
              r.huecos.length === 0 && r.fuera_de_orden.length === 0 && r.otra_clase.length === 0;
            return (
              <li key={`${r.serie}-${r.year}`}>
                <span className="font-medium">
                  Serie {r.serie} ({r.clase})
                </span>
                : {plural(r.emitidas, 'emitida')}
                {r.anuladas > 0 && `, ${plural(r.anuladas, 'anulada')}`}
                {r.primera && r.ultima && `, de ${r.primera} a ${r.ultima}`}.{' '}
                {ok ? (
                  <span className="pill pill-green">Correcta</span>
                ) : (
                  <span className="pill pill-red">Revisar</span>
                )}
                {r.huecos.length > 0 && (
                  <p className="text-red-700">Faltan los números {r.huecos.join(', ')}.</p>
                )}
                {r.fuera_de_orden.length > 0 && (
                  <p className="text-red-700">
                    Con fecha anterior a la de la factura anterior: {r.fuera_de_orden.join(', ')}.
                  </p>
                )}
                {r.otra_clase.length > 0 && (
                  <p className="text-red-700">
                    No son de la clase de la serie: {r.otra_clase.join(', ')}.
                  </p>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </details>
  );
}

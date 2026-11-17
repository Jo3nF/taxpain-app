import { useCallback, useEffect, useRef, useState } from 'react';
import {
  draftPeriod,
  errorMessage,
  euros,
  formatDate,
  parseEuros,
  pickFolder,
  type Rectificativa,
  type RectificativaAnterior,
  type RectificativaOpciones,
  rectificativaExportar,
  rectificativaPreparar,
} from './api';

/** Casillas propias de la rectificativa o complementaria, por modelo. */
const LABELS: Record<string, Record<number, string>> = {
  '303': {
    69: 'Resultado de la autoliquidación',
    70: 'Ingresado con las autoliquidaciones anteriores del periodo',
    109: 'Devoluciones ya acordadas de las anteriores',
    111: 'Rectificación: lo ingresado de más que se pide devolver',
    71: 'Resultado',
  },
  '111': {
    28: 'Suma de retenciones',
    29: 'Resultado de anteriores declaraciones',
    30: 'A ingresar',
  },
  '115': { 3: 'Retenciones', 4: 'Resultado de anteriores declaraciones', 5: 'A ingresar' },
  '123': {
    12: 'Retenciones y regularización',
    13: 'Ingresado con las anteriores',
    14: 'A ingresar',
  },
  '130': { 17: 'Total', 18: 'Resultado de anteriores liquidaciones', 19: 'Resultado' },
  '202': { 1: 'Base del pago fraccionado', 2: 'Resultado de la anterior', 3: 'A ingresar' },
  '200': {
    1586: 'Resultado de la autoliquidación',
    1578: 'Ingresado con las anteriores',
    1584: 'Devoluciones ya acordadas',
    866: 'Rectificación',
    621: 'Resultado',
  },
};

const FUENTE: Record<RectificativaAnterior['fuente'], string> = {
  taxpain: 'presentado desde TaxPain',
  aeat: 'según la comprobación con la AEAT',
  gestor: 'importado o tecleado en «Modelos presentados»',
};

const CAUCE: Record<Rectificativa['cauce'], string> = {
  rectificativa: 'Autoliquidación rectificativa',
  complementaria: 'Declaración complementaria',
  otra_via: 'Hay que pedir la rectificación por otra vía',
  sin_cambios: 'Sin cambios',
};

function today(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** Lo presentado de un periodo, en una línea («Presentado el 15/04/2026 · justificante …»). */
export function anteriorTexto(a: RectificativaAnterior): string {
  return [
    a.presentada ? `Presentado el ${formatDate(a.presentada)}` : 'Presentado',
    FUENTE[a.fuente],
    a.justificante ? `justificante ${a.justificante}` : 'sin número de justificante',
    `resultado ${euros(a.resultado)}`,
  ].join(' · ');
}

/**
 * «Preparar rectificativa…» de un periodo presentado: lo presentado al lado de lo que sale hoy,
 * la diferencia por casilla, el recargo o los intereses y el fichero con las marcas. Nada se
 * presenta.
 */
export function RectificativaPanel({
  model,
  year,
  quarter,
  anterior,
  labels,
  onClose,
}: {
  model: string;
  year: number;
  quarter: number;
  anterior: RectificativaAnterior;
  /** Nombres de las casillas del modelo. */
  labels: Record<number, string>;
  onClose: () => void;
}) {
  const [justificante, setJustificante] = useState(anterior.justificante ?? '');
  const [fecha, setFecha] = useState(today());
  const [devuelto, setDevuelto] = useState('');
  const [baja, setBaja] = useState(false);
  const [r, setR] = useState<Rectificativa | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const conDevolucion = model === '303' || model === '200';

  const opciones = useCallback((): RectificativaOpciones | null => {
    const d = devuelto.trim() ? parseEuros(devuelto) : null;
    if (devuelto.trim() && d == null) {
      setError('«Ya devuelto» no es un importe');
      return null;
    }
    return {
      justificante: justificante.trim() || null,
      fecha: fecha || null,
      devuelto: d,
      baja_domiciliacion: baja,
    };
  }, [justificante, fecha, devuelto, baja]);

  const calcular = useCallback(() => {
    const o = opciones();
    if (!o) return;
    setError(null);
    setMsg(null);
    setBusy(true);
    rectificativaPreparar(model, year, quarter, o)
      .then(setR)
      .catch((e) => {
        setR(null);
        setError(errorMessage(e));
      })
      .finally(() => setBusy(false));
  }, [model, year, quarter, opciones]);

  // Al abrir, se calcula con lo que ya se sabe.
  // biome-ignore lint/correctness/useExhaustiveDependencies: sólo al abrir el panel.
  useEffect(() => {
    heading.current?.focus();
    calcular();
  }, []);

  const label = (k: number) => LABELS[model]?.[k] ?? labels[k] ?? '';
  const period = draftPeriod(model, year, quarter);

  return (
    <section
      aria-labelledby="rectificativa-h"
      className="mt-4 space-y-3 rounded-lg border border-amber-300 bg-amber-50 p-4 text-sm text-stone-900"
    >
      <div className="flex items-start justify-between gap-3">
        <h3 id="rectificativa-h" ref={heading} tabIndex={-1} className="font-medium">
          Rectificar el {model} de {period}
        </h3>
        <button type="button" className="btn-ghost" onClick={onClose}>
          Cerrar
        </button>
      </div>
      <p>{anteriorTexto(anterior)}</p>
      <p className="text-stone-700">
        Se recalcula el periodo con tu contabilidad de hoy y se compara con lo presentado. TaxPain
        no presenta nada: genera el fichero para la Sede («Presentación mediante fichero») y el
        resumen.
      </p>
      <form
        className="flex flex-wrap items-end gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          calcular();
        }}
      >
        <label>
          Justificante de la anterior
          <input
            className="mt-1 block w-40 tabular-nums"
            inputMode="numeric"
            maxLength={13}
            value={justificante}
            onChange={(e) => setJustificante(e.target.value)}
            placeholder="13 cifras"
          />
        </label>
        <label>
          Fecha prevista de presentación
          <input
            className="mt-1 block"
            type="date"
            value={fecha}
            onChange={(e) => setFecha(e.target.value)}
          />
        </label>
        {conDevolucion && (
          <label>
            Ya devuelto por la AEAT
            <input
              className="mt-1 block w-32 tabular-nums"
              inputMode="decimal"
              value={devuelto}
              onChange={(e) => setDevuelto(e.target.value)}
              placeholder="0,00"
            />
          </label>
        )}
        {conDevolucion && (
          <label className="flex items-center gap-2">
            <input type="checkbox" checked={baja} onChange={(e) => setBaja(e.target.checked)} />
            Dar de baja la domiciliación de la anterior
          </label>
        )}
        <button type="submit" className="rounded border border-stone-300 px-3 py-1" disabled={busy}>
          {busy ? 'Calculando…' : 'Recalcular'}
        </button>
      </form>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      {r && (
        <>
          <div role="status">
            <p className="font-medium">{CAUCE[r.cauce]}</p>
            <p>{r.cauce_texto}</p>
          </div>
          <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-0.5">
            <dt>Resultado con los datos de hoy</dt>
            <dd className="tabular-nums">{euros(r.total)}</dd>
            <dt>Ingresado con la anterior</dt>
            <dd className="tabular-nums">{euros(r.ingresado_antes)}</dd>
            {r.devuelto_antes !== 0 && (
              <>
                <dt>Ya devuelto</dt>
                <dd className="tabular-nums">{euros(r.devuelto_antes)}</dd>
              </>
            )}
            <dt>
              Resultado de la {r.cauce === 'complementaria' ? 'complementaria' : 'rectificativa'}
            </dt>
            <dd className="tabular-nums">{euros(r.resultado)}</dd>
            {r.a_ingresar > 0 && (
              <>
                <dt>A ingresar más</dt>
                <dd className="font-semibold tabular-nums">{euros(r.a_ingresar)}</dd>
              </>
            )}
            {r.a_devolver > 0 && (
              <>
                <dt>A devolver (ingresado de más)</dt>
                <dd className="font-semibold tabular-nums">{euros(r.a_devolver)}</dd>
                <dt>Intereses de demora estimados</dt>
                <dd className="tabular-nums">{euros(r.intereses_devolucion)}</dd>
              </>
            )}
            {r.a_compensar > 0 && (
              <>
                <dt>Además, a compensar o devolver</dt>
                <dd className="tabular-nums">{euros(r.a_compensar)}</dd>
              </>
            )}
            <dt>Último día del plazo</dt>
            <dd>{r.plazo ? formatDate(r.plazo) : 'no se sabe'}</dd>
            {r.recargo && (
              <>
                <dt>Recargo (art. 27 LGT)</dt>
                <dd className="tabular-nums">
                  {r.recargo.porcentaje} % · {euros(r.recargo.importe)} ({r.recargo.meses}{' '}
                  {r.recargo.meses === 1 ? 'mes completo' : 'meses completos'})
                  {r.recargo.intereses > 0 && ` + ${euros(r.recargo.intereses)} de intereses`}
                </dd>
                <dt>Total a pagar</dt>
                <dd className="font-semibold tabular-nums">{euros(r.total_a_pagar)}</dd>
              </>
            )}
          </dl>
          {r.avisos.length > 0 && (
            <ul className="list-disc space-y-1 pl-5 text-amber-900">
              {r.avisos.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          )}
          {r.filas.length > 0 && (
            <table className="w-full">
              <caption className="sr-only">
                Casillas de lo presentado y de la {CAUCE[r.cauce].toLowerCase()}
              </caption>
              <thead>
                <tr className="text-left text-stone-500">
                  <th className="w-16 py-1">Casilla</th>
                  <th>Concepto</th>
                  <th className="text-right">Presentado</th>
                  <th className="text-right">Ahora</th>
                  <th className="text-right">Diferencia</th>
                </tr>
              </thead>
              <tbody>
                {r.filas.map((f) => (
                  <tr
                    key={f.casilla}
                    className={`border-t border-amber-200 ${f.diferencia !== 0 ? 'font-medium' : ''}`}
                  >
                    <td className="py-1 tabular-nums">[{String(f.casilla).padStart(2, '0')}]</td>
                    <td>{label(f.casilla)}</td>
                    <td className="text-right tabular-nums">{euros(f.presentado)}</td>
                    <td className="text-right tabular-nums">{euros(f.ahora)}</td>
                    <td className="text-right tabular-nums">
                      {f.diferencia === 0 ? '' : euros(f.diferencia)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {r.calendario && (
            <p className="text-xs text-stone-600">
              {r.calendario[0]}. {r.calendario[1]}
            </p>
          )}
          {r.file && (
            <button
              type="button"
              className="rounded btn-primary px-3 py-1.5"
              onClick={async () => {
                const o = opciones();
                if (!o) return;
                const dir = await pickFolder();
                if (!dir) return;
                try {
                  const path = await rectificativaExportar(model, year, quarter, o, dir);
                  setMsg(`Fichero guardado en ${path}. Nada se ha presentado.`);
                } catch (e) {
                  setMsg(errorMessage(e));
                }
              }}
            >
              Guardar el fichero de la{' '}
              {r.cauce === 'complementaria' ? 'complementaria' : 'rectificativa'}…
            </button>
          )}
          {msg && <p role="status">{msg}</p>}
        </>
      )}
    </section>
  );
}

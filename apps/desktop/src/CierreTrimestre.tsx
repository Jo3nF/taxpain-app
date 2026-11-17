import { CircleAlert, CircleCheck, CircleDashed, CircleMinus } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { errorMessage, euros, formatDate } from './api';
import {
  type CierreTrimestre as Cierre,
  cierreTrimestre,
  cierreTrimestreMarcar,
  cierreTrimestreResumen,
  type EstadoPasoTrimestre,
  type MarcaTrimestre,
  type PasoTrimestre,
  type ResumenTrimestre,
} from './trimestreApi';

/** Abre una sección de la app (y, en Contabilidad, una vista). */
export type Navegar = (tab: string, view?: string) => void;

const ESTADO: Record<EstadoPasoTrimestre, string> = {
  ok: 'hecho',
  pendiente: 'pendiente',
  aviso: 'revisar',
  no_aplica: 'no aplica',
};

function Icono({ estado }: { estado: EstadoPasoTrimestre }) {
  const cls = 'mt-0.5 shrink-0';
  if (estado === 'ok')
    return <CircleCheck size={16} className={`${cls} text-emerald-600`} aria-hidden="true" />;
  if (estado === 'pendiente')
    return <CircleAlert size={16} className={`${cls} text-red-600`} aria-hidden="true" />;
  if (estado === 'aviso')
    return <CircleDashed size={16} className={`${cls} text-amber-600`} aria-hidden="true" />;
  return <CircleMinus size={16} className={`${cls} text-stone-400`} aria-hidden="true" />;
}

function etiquetaTrimestre(r: ResumenTrimestre): string {
  if (r.futuro) return 'aún no empieza';
  if (r.presentado) return 'presentado';
  if (r.listo) return 'listo para presentar';
  return r.pendientes === 1 ? '1 cosa pendiente' : `${r.pendientes} cosas pendientes`;
}

/** Trimestre que toca cerrar el día `now`: en el primer mes de un trimestre (cuando vencen los
 * modelos), el anterior; si no, el que está en curso. */
export function trimestrePorCerrar(now: Date): { year: number; quarter: number } {
  const m = now.getMonth() + 1;
  const q = Math.floor((m - 1) / 3) + 1;
  const primerMes = (q - 1) * 3 + 1;
  if (m === primerMes) {
    return q === 1
      ? { year: now.getFullYear() - 1, quarter: 4 }
      : { year: now.getFullYear(), quarter: q - 1 };
  }
  return { year: now.getFullYear(), quarter: q };
}

function Paso({
  p,
  onGo,
  onMarcar,
}: {
  p: PasoTrimestre;
  onGo?: Navegar | undefined;
  onMarcar: (paso: string, m: MarcaTrimestre | null) => void;
}) {
  return (
    <li className="flex items-start gap-2">
      <Icono estado={p.estado} />
      <span className="flex-1">
        <span className="font-medium">{p.label}</span>
        <span className="sr-only"> ({ESTADO[p.estado]})</span>
        <span className="block text-stone-600">
          {p.marca ? `Marcado a mano: ${p.marca === 'hecho' ? 'hecho' : 'no aplica'}. ` : ''}
          {p.detalle}
        </span>
        <span className="mt-1 flex flex-wrap gap-3 text-xs">
          {p.destino && onGo && (
            <button
              type="button"
              className="text-link underline"
              onClick={() => onGo(p.destino?.tab ?? '', p.destino?.view ?? undefined)}
            >
              Ir: {p.label}
            </button>
          )}
          {p.manual &&
            (p.marca ? (
              <button
                type="button"
                className="text-link underline"
                onClick={() => onMarcar(p.id, null)}
              >
                Quitar la marca: {p.label}
              </button>
            ) : (
              <>
                {p.estado !== 'ok' && (
                  <button
                    type="button"
                    className="text-link underline"
                    onClick={() => onMarcar(p.id, 'hecho')}
                  >
                    Marcar como hecho: {p.label}
                  </button>
                )}
                {p.estado !== 'no_aplica' && (
                  <button
                    type="button"
                    className="text-link underline"
                    onClick={() => onMarcar(p.id, 'no_aplica')}
                  >
                    No aplica: {p.label}
                  </button>
                )}
              </>
            ))}
        </span>
      </span>
    </li>
  );
}

/**
 * Cerrar el trimestre: lo que falta antes de presentar (recibidas, banco, nóminas…), los modelos
 * que tocan según el perfil con su cuadre con el diario, lo que sale a pagar y la liquidación
 * del IVA. No presenta ni contabiliza nada: cada paso lleva a donde se hace.
 */
export function CierreTrimestre({ onGo, now = new Date() }: { onGo?: Navegar; now?: Date }) {
  const inicial = trimestrePorCerrar(now);
  const [year, setYear] = useState(inicial.year);
  const [quarter, setQuarter] = useState(inicial.quarter);
  const [resumen, setResumen] = useState<ResumenTrimestre[] | null>(null);
  const [data, setData] = useState<Cierre | null>(null);
  const [error, setError] = useState<string | null>(null);

  const cargar = useCallback(() => {
    setError(null);
    cierreTrimestre(year, quarter)
      .then(setData)
      .catch((e) => setError(errorMessage(e)));
    cierreTrimestreResumen(year)
      .then(setResumen)
      .catch(() => setResumen(null));
  }, [year, quarter]);
  useEffect(cargar, [cargar]);

  const marcar = (paso: string, m: MarcaTrimestre | null) => {
    cierreTrimestreMarcar(year, quarter, paso, m)
      .then(cargar)
      .catch((e) => setError(errorMessage(e)));
  };

  const previos = data?.pasos.filter((p) => p.id !== 'liquidacion_iva') ?? [];
  const liquidacion = data?.pasos.find((p) => p.id === 'liquidacion_iva');

  return (
    <section aria-labelledby="cierre-trimestre-t" className="card mt-6 p-5 text-sm">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 id="cierre-trimestre-t" className="text-lg font-semibold">
          Cerrar el trimestre
        </h2>
        <label className="flex items-center gap-2">
          Año
          <select
            value={year}
            onChange={(e) => setYear(Number(e.target.value))}
            className="rounded border border-stone-300 px-2 py-1"
          >
            {[inicial.year - 1, inicial.year, inicial.year + 1].map((y) => (
              <option key={y} value={y}>
                {y}
              </option>
            ))}
          </select>
        </label>
      </div>
      <p className="mt-1 text-stone-600">
        Lo que conviene tener hecho antes de presentar los modelos del trimestre. Nada se presenta
        ni se contabiliza desde aquí.
      </p>
      <fieldset className="mt-3 flex flex-wrap gap-2 border-0 p-0">
        <legend className="sr-only">Trimestres</legend>
        {[1, 2, 3, 4].map((q) => {
          const r = resumen?.find((x) => x.quarter === q);
          return (
            <button
              key={q}
              type="button"
              aria-pressed={q === quarter}
              onClick={() => setQuarter(q)}
              className={`rounded-lg border px-3 py-1.5 text-left ${q === quarter ? 'border-stone-800 bg-stone-100' : 'border-stone-300'}`}
            >
              <span className="font-semibold">{q}T</span>
              {r && <span className="block text-xs text-stone-600">{etiquetaTrimestre(r)}</span>}
            </button>
          );
        })}
      </fieldset>
      {error && (
        <p role="alert" className="mt-3 rounded-md bg-red-50 p-3 text-red-900">
          {error}
        </p>
      )}
      {data && (
        <>
          <p
            role="status"
            className={`mt-4 rounded-md p-3 ${data.presentado ? 'bg-emerald-50 text-emerald-900' : data.listo ? 'bg-emerald-50 text-emerald-900' : 'bg-amber-50 text-amber-950'}`}
          >
            {data.presentado
              ? `El ${data.quarter}T de ${data.year} está presentado.`
              : data.listo
                ? `El ${data.quarter}T de ${data.year} está listo para presentar.`
                : `El ${data.quarter}T de ${data.year} aún no está listo: mira lo marcado en rojo.`}
            {data.sin_perfil &&
              ' Sin perfil fiscal: los modelos salen del perfil tipo (configúralo en el calendario).'}
          </p>
          <h3 className="mt-4 font-semibold">Antes de los modelos</h3>
          <ol className="mt-2 space-y-3">
            {previos.map((p) => (
              <Paso key={p.id} p={p} onGo={onGo} onMarcar={marcar} />
            ))}
          </ol>
          <h3 className="mt-5 font-semibold">Modelos del trimestre</h3>
          {data.modelos.length === 0 ? (
            <p className="mt-2 text-stone-600">Tu perfil no tiene modelos este trimestre.</p>
          ) : (
            <ul className="mt-2 space-y-3">
              {data.modelos.map((m) => (
                <li key={m.model} className="rounded-lg border border-stone-200 p-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <span className="flex items-center gap-2">
                      <span className="chip">{m.model}</span> {m.period}
                      {m.presentado && <span className="pill pill-stone">presentado</span>}
                    </span>
                    <span className="tabular-nums">
                      {m.error
                        ? 'Sin borrador'
                        : m.result >= 0
                          ? euros(m.result)
                          : `${euros(-m.result)} a tu favor`}
                      {m.plazo && ` · hasta el ${formatDate(m.plazo)}`}
                    </span>
                  </div>
                  {m.error && <p className="mt-1 text-stone-600">{m.error}</p>}
                  {m.cuadres.length > 0 && (
                    <ul className="mt-2 space-y-1">
                      {m.cuadres.map((c) => (
                        <li key={c.concepto} className="flex items-start gap-2">
                          <Icono estado={c.ok ? 'ok' : 'pendiente'} />
                          <span>
                            {c.concepto}: {euros(c.modelo)} en el modelo y {euros(c.diario)} en el
                            diario
                            {c.ok ? ' (cuadra)' : ` (diferencia de ${euros(c.modelo - c.diario)})`}
                          </span>
                        </li>
                      ))}
                    </ul>
                  )}
                  {m.avisos.length > 0 && (
                    <ul className="mt-2 list-disc pl-5 text-stone-600">
                      {m.avisos.map((a) => (
                        <li key={a}>{a}</li>
                      ))}
                    </ul>
                  )}
                </li>
              ))}
            </ul>
          )}
          {!data.presentado && data.modelos.length > 0 && (
            <p className="mt-3">
              A ingresar: <strong>{euros(data.a_ingresar)}</strong>
              {data.domiciliar_hasta &&
                ` · para domiciliarlo, preséntalo hasta el ${formatDate(data.domiciliar_hasta)}`}
              .{' '}
              {onGo && (
                <button
                  type="button"
                  className="text-link underline"
                  onClick={() => onGo('modelos')}
                >
                  Abrir los borradores en Modelos
                </button>
              )}
            </p>
          )}
          {liquidacion && (
            <>
              <h3 className="mt-5 font-semibold">Después</h3>
              <ol className="mt-2 space-y-3">
                <Paso p={liquidacion} onGo={onGo} onMarcar={marcar} />
              </ol>
            </>
          )}
        </>
      )}
    </section>
  );
}

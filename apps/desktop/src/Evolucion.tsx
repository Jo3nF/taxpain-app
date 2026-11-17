import { useEffect, useMemo, useState } from 'react';
import { useAskAi } from './AssistantPanel';
import {
  companyEvolution,
  companyReview,
  type Evolucion,
  errorMessage,
  euros,
  type FilaEvolucion,
  type FormatoInforme,
  findingTopic,
  folderReviewSave,
  type Gravedad,
  type Hallazgo,
  type Revision,
  type ValorEvolucion,
} from './api';
import { HallazgoItem, QuePedir, Tabla } from './Revision';
import { BloqueUrgente } from './Urgentes';

/** Una cifra: euros o un recuento. */
function cifra(f: FilaEvolucion, v: number): string {
  return f.recuento ? v.toLocaleString('es-ES') : euros(v);
}

/** «+50,0 %» o, sin porcentaje (el año anterior era cero), la diferencia. */
function cambio(f: FilaEvolucion, v: ValorEvolucion): string | null {
  if (v.diferencia === null || v.diferencia === 0) return v.diferencia === 0 ? 'igual' : null;
  const signo = v.diferencia > 0 ? '+' : '−';
  if (v.variacion !== null) {
    return `${signo}${(Math.abs(v.variacion) / 10).toLocaleString('es-ES', {
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
    })} %`;
  }
  return `${signo}${cifra(f, Math.abs(v.diferencia))}`;
}

/** Barras pequeñas con la tendencia (decorativas: las cifras están en la tabla). */
function Tendencia({ f }: { f: FilaEvolucion }) {
  const vals = f.valores.map((v) => v.valor);
  const max = Math.max(1, ...vals.map((v) => Math.abs(v ?? 0)));
  const w = 8;
  const gap = 3;
  const h = 28;
  const mid = vals.some((v) => (v ?? 0) < 0) ? h / 2 : h;
  return (
    <svg
      aria-hidden="true"
      width={vals.length * (w + gap)}
      height={h}
      className="text-brand-600"
      focusable="false"
    >
      {vals.map((v, i) => {
        const len = v === null ? 0 : (Math.abs(v) / max) * (mid === h ? h : h / 2);
        const y = v !== null && v < 0 ? mid : mid - len;
        return (
          <rect
            // biome-ignore lint/suspicious/noArrayIndexKey: una barra por año, en orden
            key={i}
            x={i * (w + gap)}
            y={y}
            width={w}
            height={Math.max(v === null ? 0 : 1, len)}
            className={`${v !== null && v < 0 ? 'fill-red-600' : 'fill-current'}${f.valores[i]?.avance ? ' opacity-40' : ''}`}
          />
        );
      })}
    </svg>
  );
}

function TablaEvolucion({
  titulo,
  filas,
  años,
}: {
  titulo: string;
  filas: FilaEvolucion[];
  años: number[];
}) {
  return (
    <section className="card p-4">
      <h4 className="font-semibold">{titulo}</h4>
      <div className="mt-2 overflow-x-auto">
        <table className="w-full text-left text-sm" aria-label={titulo}>
          <thead>
            <tr className="border-b border-stone-200 text-stone-600">
              <th scope="col" className="py-1.5 pr-3 font-medium">
                Cifra
              </th>
              {años.map((y) => (
                <th key={y} scope="col" className="px-2 py-1.5 text-right font-medium">
                  {y}
                </th>
              ))}
              <th scope="col" className="px-2 py-1.5 font-medium">
                Tendencia
              </th>
            </tr>
          </thead>
          <tbody>
            {filas.map((f) => (
              <tr key={f.id} className="border-b border-stone-100 align-top">
                <th scope="row" className="py-1.5 pr-3 font-normal">
                  {f.titulo}
                  {f.ayuda && <span className="block text-xs text-stone-500">{f.ayuda}</span>}
                </th>
                {f.valores.map((v) => {
                  const c = cambio(f, v);
                  return (
                    <td key={v.año} className="px-2 py-1.5 text-right tabular-nums">
                      {v.valor === null ? (
                        <span className="text-stone-500">sin dato</span>
                      ) : (
                        <>
                          <span className={v.valor < 0 ? 'text-red-700' : ''}>
                            {cifra(f, v.valor)}
                          </span>
                          {v.avance && (
                            <span className="pill pill-amber ml-1 align-middle">a medias</span>
                          )}
                          {c && (
                            <span className="block text-xs text-stone-600">
                              {c}
                              <span className="sr-only"> frente a {v.año - 1}</span>
                            </span>
                          )}
                          {v.fuente && (
                            <span className="block text-xs text-stone-500">{v.fuente}</span>
                          )}
                        </>
                      )}
                    </td>
                  );
                })}
                <td className="px-2 py-1.5">
                  <Tendencia f={f} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

const GRAVEDADES: { id: Gravedad | 'todas'; titulo: string }[] = [
  { id: 'todas', titulo: 'Todas' },
  { id: 'error', titulo: 'Errores' },
  { id: 'aviso', titulo: 'Avisos' },
  { id: 'info', titulo: 'Para saber' },
];

/**
 * «Evolución»: la empresa año a año con lo que tiene la AEAT (lo de «Comprobar lo presentado» en
 * Modelos) y, debajo, lo que hay que revisar: lo que falta, lo presentado fuera de plazo y lo que
 * no cuadra entre modelos, con quién debe corregirlo y hasta cuándo.
 */
export function EvolucionPage({ onNavigate }: { onNavigate?: (t: 'modelos') => void }) {
  const [evo, setEvo] = useState<Evolucion | null>(null);
  const [review, setReview] = useState<Revision | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [gravedad, setGravedad] = useState<Gravedad | 'todas'>('todas');
  const [ejercicio, setEjercicio] = useState<number | 'todos'>('todos');
  const ia = useAskAi();
  const preguntar = (h: Hallazgo, from: HTMLButtonElement) =>
    ia.open({ topic: findingTopic(h, 'aeat'), title: h.titulo, conversationId: null }, from);

  const cargar = () => {
    setBusy(true);
    setError(null);
    Promise.all([companyEvolution(), companyReview()])
      .then(([e, r]) => {
        setEvo(e);
        setReview(r);
      })
      .catch((e) => setError(errorMessage(e)))
      .finally(() => setBusy(false));
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: sólo al abrir
  useEffect(() => {
    cargar();
  }, []);

  const hallazgos = useMemo(
    () =>
      (review?.hallazgos ?? []).filter(
        (h) =>
          (gravedad === 'todas' || h.gravedad === gravedad) &&
          (ejercicio === 'todos' || h.ejercicio === ejercicio),
      ),
    [review, gravedad, ejercicio],
  );
  const ejercicios = useMemo(
    () =>
      [...new Set((review?.hallazgos ?? []).flatMap((h) => (h.ejercicio ? [h.ejercicio] : [])))]
        .sort()
        .reverse(),
    [review],
  );

  const guardar = async (formato: FormatoInforme) => {
    if (!review) return;
    setMsg(null);
    try {
      if (await folderReviewSave(review, formato, 'aeat')) setMsg('Informe guardado.');
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <div className="space-y-8">
      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}
      <section aria-labelledby="evo-h" className="space-y-3">
        <div className="flex flex-wrap items-baseline justify-between gap-3">
          <h3 id="evo-h" className="text-base font-semibold">
            Evolución año a año
          </h3>
          <button
            type="button"
            className="text-link"
            disabled={busy}
            onClick={cargar}
            aria-busy={busy}
          >
            {busy ? 'Calculando…' : 'Volver a calcular'}
          </button>
        </div>
        <p className="text-sm text-stone-600">
          Las cifras clave de cada ejercicio según las declaraciones que tiene la AEAT (también las
          de la gestoría), cada una con el modelo y la casilla de donde sale y lo que cambia frente
          al año anterior. Si TaxPain lleva la contabilidad de un año, sus cifras salen al lado.
        </p>
        {evo?.sin_comprobar && (
          <div className="card space-y-2 p-4 text-sm">
            <p>
              Aún no hay declaraciones de la AEAT guardadas. En Modelos, pulsa «Comprobar varios
              años…» para traer lo presentado de los ejercicios que quieras, con tu certificado.
            </p>
            {onNavigate && (
              <button type="button" className="btn-primary" onClick={() => onNavigate('modelos')}>
                Ir a Modelos
              </button>
            )}
          </div>
        )}
        {evo && evo.sin_casillas > 0 && (
          <p className="text-sm text-amber-800">
            {evo.sin_casillas === 1
              ? '1 declaración no tiene sus casillas leídas'
              : `${evo.sin_casillas} declaraciones no tienen sus casillas leídas`}
            : sus cifras salen como «sin dato». Vuelve a comprobar esos años en Modelos.
          </p>
        )}
        {evo?.grupos.map((g) => (
          <TablaEvolucion key={g.titulo} titulo={g.titulo} filas={g.filas} años={evo.años} />
        ))}
      </section>

      <section aria-labelledby="revisar-h" className="space-y-3">
        <h3 id="revisar-h" className="text-base font-semibold">
          Qué revisar
        </h3>
        <p className="text-sm text-stone-600">
          Lo que falta, lo presentado fuera de plazo y lo que no cuadra entre los modelos (el 390
          con los 303, los resúmenes anuales con sus trimestres, el 349 con el 303, el 200 con los
          202) y en el Impuesto sobre Sociedades. Cada aviso dice por cuánto, quién debe corregirlo
          y hasta cuándo se puede rectificar. La revisión de la carpeta (Importar → Revisión) añade
          lo que sale de las facturas y los libros.
        </p>
        {review && (
          <>
            <BloqueUrgente hallazgos={review.hallazgos} id="revisar-urgente" />
            <dl className="grid grid-cols-2 gap-3 sm:grid-cols-4">
              {[
                ['Errores', review.resumen.errores, 'text-red-700'],
                ['Avisos', review.resumen.avisos, 'text-amber-800'],
                ['Para saber', review.resumen.info, 'text-stone-700'],
              ].map(([t, n, c]) => (
                <div key={t as string} className="card p-4">
                  <dt className="text-xs text-stone-600">{t}</dt>
                  <dd className={`mt-1 text-2xl font-semibold tabular-nums ${c}`}>{n}</dd>
                </div>
              ))}
              <div className="card p-4">
                <dt className="text-xs text-stone-600">Recuperable estimado</dt>
                <dd className="mt-1 text-2xl font-semibold tabular-nums text-emerald-800">
                  {euros(review.resumen.recuperable)}
                </dd>
              </div>
            </dl>
            <div className="flex flex-wrap items-end gap-3 text-sm">
              <label className="flex flex-col">
                <span className="text-stone-600">Importancia</span>
                <select
                  value={gravedad}
                  onChange={(e) => setGravedad(e.target.value as Gravedad | 'todas')}
                  className="rounded border border-stone-300 px-2 py-1"
                >
                  {GRAVEDADES.map((g) => (
                    <option key={g.id} value={g.id}>
                      {g.titulo}
                    </option>
                  ))}
                </select>
              </label>
              <label className="flex flex-col">
                <span className="text-stone-600">Ejercicio</span>
                <select
                  value={String(ejercicio)}
                  onChange={(e) =>
                    setEjercicio(e.target.value === 'todos' ? 'todos' : Number(e.target.value))
                  }
                  className="rounded border border-stone-300 px-2 py-1"
                >
                  <option value="todos">Todos</option>
                  {ejercicios.map((y) => (
                    <option key={y} value={y}>
                      {y}
                    </option>
                  ))}
                </select>
              </label>
              <button type="button" className="text-link" onClick={() => guardar('pdf')}>
                Guardar el informe en PDF…
              </button>
              <button type="button" className="text-link" onClick={() => guardar('xlsx')}>
                En Excel…
              </button>
            </div>
            {msg && (
              <p role="status" className="text-sm">
                {msg}
              </p>
            )}
            {review.hallazgos.length === 0 ? (
              <p className="text-sm text-stone-600">
                No hay nada que revisar con lo que tiene la AEAT.
              </p>
            ) : hallazgos.length === 0 ? (
              <p className="text-sm text-stone-600">Nada con estos filtros.</p>
            ) : (
              <ul className="space-y-3">
                {hallazgos.map((h) => (
                  <HallazgoItem
                    key={`${h.codigo}-${h.ejercicio}-${h.titulo}`}
                    h={h}
                    questions={ia.questions(findingTopic(h, 'aeat'))}
                    onAsk={preguntar}
                  />
                ))}
              </ul>
            )}
            {review.pedir.length > 0 && <QuePedir pedir={review.pedir} />}
            {review.tablas.length > 0 && (
              <div className="space-y-2">
                <h4 className="font-semibold">Tablas</h4>
                {review.tablas.map((t) => (
                  <Tabla key={t.titulo} t={t} />
                ))}
              </div>
            )}
          </>
        )}
      </section>
      {ia.element}
    </div>
  );
}

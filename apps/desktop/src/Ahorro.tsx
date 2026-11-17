import { useEffect, useMemo, useState } from 'react';
import { AskAiButton, useAskAi } from './AssistantPanel';
import {
  type Ahorro,
  type AreaAhorro,
  type AssistantTopic,
  type EncajeAhorro,
  type EstadoAhorro,
  errorMessage,
  euros,
  formatDate,
  type MarcaAhorro,
  openSede,
  type Recomendacion,
  taxSavings,
  taxSavingsMark,
} from './api';

/** Pantallas de TaxPain a las que lleva una recomendación. */
export type AhorroDestino =
  | { tab: 'modelos' | 'recibidas' | 'cripto' | 'evolucion' }
  | { tab: 'contabilidad'; view?: 'cartera' | 'inmovilizado' };

const DESTINOS: Record<string, { label: string; to: AhorroDestino }> = {
  modelos: { label: 'Abrir Modelos', to: { tab: 'modelos' } },
  evolucion: { label: 'Abrir Evolución', to: { tab: 'evolucion' } },
  cartera: { label: 'Abrir Cobros y pagos', to: { tab: 'contabilidad', view: 'cartera' } },
  cripto: { label: 'Abrir Criptomonedas', to: { tab: 'cripto' } },
  contabilidad: { label: 'Abrir Contabilidad', to: { tab: 'contabilidad' } },
  inmovilizado: {
    label: 'Abrir Inmovilizado',
    to: { tab: 'contabilidad', view: 'inmovilizado' },
  },
  recibidas: { label: 'Abrir Recibidas', to: { tab: 'recibidas' } },
};

const AREAS: Record<AreaAhorro, string> = {
  beneficio: 'Impuesto sobre el beneficio',
  inversion: 'Inversiones que desgravan',
  gastos: 'Gastos y amortizaciones',
  personas: 'Sueldos y pensiones',
  iva: 'IVA',
  donativos: 'Donativos',
  cripto: 'Criptomonedas',
};

const ENCAJES: Record<EncajeAhorro, { label: string; className: string }> = {
  conviene: { label: 'Te conviene', className: 'bg-emerald-100 text-emerald-900' },
  valorar: { label: 'Puede interesarte', className: 'bg-brand-50 text-brand-700' },
  aprovechado: { label: 'Ya lo aprovechas', className: 'bg-stone-100 text-stone-700' },
  no_aplica: { label: 'No te aplica', className: 'bg-stone-100 text-stone-600' },
};

const ESTADOS: Record<EstadoAhorro, string> = {
  pendiente: 'Pendiente',
  en_marcha: 'En marcha',
  hecho: 'Hecho',
  descartado: 'No me interesa',
};

type Filtro = 'para_ti' | 'en_marcha' | 'hechas' | 'descartadas' | 'todas';

function filtrar(r: Recomendacion, f: Filtro): boolean {
  const e = r.marca.estado;
  switch (f) {
    case 'para_ti':
      return (
        (r.encaje === 'conviene' || r.encaje === 'valorar') &&
        (e === 'pendiente' || e === 'en_marcha')
      );
    case 'en_marcha':
      return e === 'en_marcha' && r.encaje !== 'aprovechado';
    case 'hechas':
      return e === 'hecho' || (r.encaje === 'aprovechado' && e !== 'descartado');
    case 'descartadas':
      return e === 'descartado';
    case 'todas':
      return true;
  }
}

/** Los totales como en `taxpain_app::ahorro::ahorro`: lo que conviene y no está hecho ni descartado. */
export function conTotales(d: Ahorro): Ahorro {
  const suma = (aplazado: boolean) =>
    d.recomendaciones
      .filter(
        (r) =>
          r.encaje === 'conviene' &&
          r.marca.estado !== 'hecho' &&
          r.marca.estado !== 'descartado' &&
          r.aplazado === aplazado,
      )
      .reduce((t, r) => t + (r.ahorro ?? 0), 0);
  return { ...d, ahorro_posible: suma(false), aplazado_posible: suma(true) };
}

const FILTROS: { id: Filtro; label: string }[] = [
  { id: 'para_ti', label: 'Para ti' },
  { id: 'en_marcha', label: 'En marcha' },
  { id: 'hechas', label: 'Hechas o aprovechadas' },
  { id: 'descartadas', label: 'Descartadas' },
  { id: 'todas', label: 'Todas' },
];

function Lista({ titulo, items }: { titulo: string; items: string[] }) {
  if (items.length === 0) return null;
  return (
    <div>
      <h5 className="text-sm font-medium">{titulo}</h5>
      <ul className="mt-1 list-disc space-y-1 pl-5 text-sm text-stone-700">
        {items.map((t) => (
          <li key={t}>{t}</li>
        ))}
      </ul>
    </div>
  );
}

/**
 * Una acción sobre una tarjeta: la marca nueva, lo que se anuncia al lector de pantalla y si el
 * foco tiene que moverse (el botón pulsado desaparece al cambiar de estado; una casilla no).
 */
type Accion = { marca: MarcaAhorro; aviso: string; moverFoco: boolean };

function Tarjeta({
  r,
  cierre,
  onMarca,
  onNavigate,
  busy,
  questions,
  onAsk,
}: {
  r: Recomendacion;
  /** Último día del ejercicio en curso. */
  cierre: string;
  onMarca: (a: Accion) => void;
  onNavigate?: (to: AhorroDestino) => void;
  busy: boolean;
  /** Preguntas de su conversación con la IA (0 si no tiene). */
  questions: number;
  onAsk: (b: HTMLButtonElement) => void;
}) {
  const enc = ENCAJES[r.encaje];
  const hechos = new Set(r.marca.pasos);
  const destino = r.pantalla ? DESTINOS[r.pantalla] : undefined;
  const titleId = `ahorro-${r.id}`;
  // Mientras se guarda, los controles no se desactivan (perderían el foco): se ignora el clic.
  const paso = (i: number, on: boolean) => {
    if (busy) return;
    const pasos = new Set(hechos);
    if (on) pasos.add(i);
    else pasos.delete(i);
    // Lo que ya se aprovecha no pasa a «en marcha» por marcar un paso.
    const estado: EstadoAhorro =
      r.encaje !== 'aprovechado' && r.marca.estado === 'pendiente' && pasos.size > 0
        ? 'en_marcha'
        : r.marca.estado;
    onMarca({
      marca: { estado, pasos: [...pasos].sort((a, b) => a - b) },
      aviso: `Paso ${i + 1} ${on ? 'hecho' : 'sin hacer'}.`,
      moverFoco: false,
    });
  };
  const estado = (e: EstadoAhorro, pasos: number[] = r.marca.pasos) => {
    if (busy) return;
    onMarca({
      marca: { estado: e, pasos },
      aviso: `«${r.titulo}»: ${ESTADOS[e].toLowerCase()}.`,
      moverFoco: true,
    });
  };

  return (
    <article className="card space-y-3 p-4" aria-labelledby={titleId}>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <p className="flex flex-wrap items-center gap-2 text-xs">
            <span className={`rounded px-1.5 py-0.5 font-medium ${enc.className}`}>
              {enc.label}
            </span>
            <span className="text-stone-600">{AREAS[r.area]}</span>
            {r.marca.estado !== 'pendiente' && (
              <span className="rounded border border-stone-300 px-1.5 py-0.5 text-stone-700">
                {ESTADOS[r.marca.estado]}
              </span>
            )}
          </p>
          <h4 id={titleId} tabIndex={-1} className="mt-1.5 font-semibold">
            {r.titulo}
          </h4>
        </div>
        {r.ahorro !== null && (
          <p className="text-right">
            <span className="block text-xs text-stone-600">
              {r.aplazado ? 'Impuesto aplazado' : 'Ahorro estimado'}
            </span>
            <span className="text-lg font-semibold tabular-nums text-emerald-800">
              {euros(r.ahorro)}
            </span>
          </p>
        )}
      </div>
      <p className="text-sm">{r.motivo}</p>
      {r.plazo && (
        <p className="text-sm font-medium text-amber-900">
          {r.plazo === cierre
            ? `Cuenta lo que hagas hasta el cierre del ejercicio (${formatDate(r.plazo)}).`
            : `Fecha límite: ${formatDate(r.plazo)}, incluido.`}
        </p>
      )}
      <details className="group">
        <summary className="cursor-pointer text-sm font-medium text-brand-700">
          Qué es y cómo conseguirlo<span className="sr-only">: {r.titulo}</span>
        </summary>
        <div className="mt-3 space-y-3">
          <p className="text-sm text-stone-700">{r.resumen}</p>
          <Lista titulo="Requisitos" items={r.requisitos} />
          <fieldset>
            <legend className="text-sm font-medium">Pasos</legend>
            <ol className="mt-1 space-y-1.5 text-sm">
              {r.pasos.map((p, i) => (
                <li key={p} className="flex items-start gap-2">
                  <input
                    id={`${titleId}-p${i}`}
                    type="checkbox"
                    className="mt-0.5"
                    checked={hechos.has(i)}
                    aria-disabled={busy || undefined}
                    onChange={(e) => paso(i, e.target.checked)}
                  />
                  <label
                    htmlFor={`${titleId}-p${i}`}
                    className={hechos.has(i) ? 'line-through text-stone-600' : ''}
                  >
                    <span className="tabular-nums">{i + 1}.</span> {p}
                  </label>
                </li>
              ))}
            </ol>
          </fieldset>
          <Lista titulo="Ten en cuenta" items={r.riesgos} />
          {r.calculo && <p className="text-xs text-stone-600">Cálculo: {r.calculo}.</p>}
          <p className="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
            <button
              type="button"
              className="text-link"
              onClick={() => openSede(r.enlace).catch(() => undefined)}
            >
              {r.norma}
            </button>
            {destino && onNavigate && (
              <button type="button" className="text-link" onClick={() => onNavigate(destino.to)}>
                {destino.label}
              </button>
            )}
          </p>
        </div>
      </details>
      <div className="flex flex-wrap gap-2 border-t border-stone-100 pt-3">
        {r.encaje !== 'aprovechado' &&
          r.marca.estado !== 'en_marcha' &&
          r.marca.estado !== 'hecho' && (
            <button
              type="button"
              className="border px-3 py-1.5 text-sm"
              aria-disabled={busy || undefined}
              onClick={() => estado('en_marcha')}
            >
              Lo estoy haciendo
              <span className="sr-only">: {r.titulo}</span>
            </button>
          )}
        {r.encaje !== 'aprovechado' && r.marca.estado !== 'hecho' && (
          <button
            type="button"
            className="border px-3 py-1.5 text-sm"
            aria-disabled={busy || undefined}
            onClick={() => estado('hecho')}
          >
            Hecho
            <span className="sr-only">: {r.titulo}</span>
          </button>
        )}
        {r.marca.estado !== 'descartado' && r.marca.estado !== 'hecho' && (
          <button
            type="button"
            className="border px-3 py-1.5 text-sm"
            aria-disabled={busy || undefined}
            onClick={() => estado('descartado')}
          >
            No me interesa
            <span className="sr-only">: {r.titulo}</span>
          </button>
        )}
        {r.marca.estado !== 'pendiente' && (
          <button
            type="button"
            className="text-link"
            aria-disabled={busy || undefined}
            onClick={() => estado('pendiente', [])}
          >
            Volver a pendiente
            <span className="sr-only">: {r.titulo}</span>
          </button>
        )}
        <AskAiButton
          what={r.titulo}
          questions={questions}
          className="btn-ghost ml-auto"
          onOpen={onAsk}
        />
      </div>
    </article>
  );
}

export function AhorroPage({ onNavigate }: { onNavigate?: (to: AhorroDestino) => void }) {
  const [data, setData] = useState<Ahorro | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [filtro, setFiltro] = useState<Filtro>('para_ti');
  // Lo que se anuncia tras cada acción y dónde poner el foco cuando se vuelve a pintar.
  const [aviso, setAviso] = useState('');
  const [enfocar, setEnfocar] = useState<string | null>(null);
  useEffect(() => {
    if (!enfocar) return;
    document.getElementById(enfocar)?.focus();
    setEnfocar(null);
  }, [enfocar]);

  const cargar = () => {
    setError(null);
    taxSavings()
      .then(setData)
      .catch((e) => setError(errorMessage(e)));
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: sólo al abrir
  useEffect(() => {
    cargar();
  }, []);

  // Las conversaciones con la IA guardadas (si no se pueden leer, los botones salen igual).
  const ia = useAskAi();
  const preguntas = ia.questions;

  // Guarda la marca y cambia sólo esa tarjeta y los totales (recalcularlo todo vuelve a leer el
  // Impuesto sobre Sociedades, la cartera y la revisión). Si la tarjeta sale de la lista que se
  // ve, se dice dónde está ahora y el foco pasa a la siguiente.
  const marcar = (r: Recomendacion, a: Accion) => {
    setBusy(true);
    setError(null);
    const despues = { ...r, marca: a.marca };
    const sigue = filtrar(despues, filtro);
    const i = visibles.findIndex((x) => x.id === r.id);
    const otra = visibles[i + 1] ?? visibles[i - 1];
    const ahora = FILTROS.find((f) => f.id !== 'todas' && filtrar(despues, f.id));
    taxSavingsMark(r.id, a.marca)
      .then(() => {
        setData((d) =>
          d
            ? conTotales({
                ...d,
                recomendaciones: d.recomendaciones.map((x) => (x.id === r.id ? despues : x)),
              })
            : d,
        );
        setAviso(sigue || !ahora ? a.aviso : `${a.aviso} Ahora está en «${ahora.label}».`);
        // El total exacto lo calcula el motor (las rebajas juntas no suman lo mismo que por
        // separado); mientras, se ve el aproximado.
        taxSavings()
          .then((d) =>
            setData((x) =>
              x
                ? { ...x, ahorro_posible: d.ahorro_posible, aplazado_posible: d.aplazado_posible }
                : x,
            ),
          )
          .catch(() => undefined);
        if (a.moverFoco) {
          setEnfocar(sigue ? `ahorro-${r.id}` : otra ? `ahorro-${otra.id}` : 'ahorro-lista-h');
        }
      })
      .catch((e) => setError(errorMessage(e)))
      .finally(() => setBusy(false));
  };

  const cuenta = useMemo(() => {
    const out = {} as Record<Filtro, number>;
    for (const f of FILTROS) {
      out[f.id] = (data?.recomendaciones ?? []).filter((r) => filtrar(r, f.id)).length;
    }
    return out;
  }, [data]);
  const visibles = (data?.recomendaciones ?? []).filter((r) => filtrar(r, filtro));
  const s = data?.situacion;

  return (
    <div className="space-y-6">
      <p role="status" className="sr-only">
        {aviso}
      </p>
      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}
      {!data && !error && <p className="text-sm text-stone-600">Calculando…</p>}
      {data && s && (
        <section aria-labelledby="ahorro-resumen-h" className="card space-y-3 p-4">
          <h3 id="ahorro-resumen-h" className="text-base font-semibold">
            {data.ahorro_posible > 0
              ? `En ${s.año} podrías ahorrarte unos ${euros(data.ahorro_posible)}`
              : `Lo que puedes aprovechar en ${s.año}`}
          </h3>
          {data.aplazado_posible > 0 && (
            <p className="text-sm">
              Y aplazar unos {euros(data.aplazado_posible)} de impuesto a años siguientes.
            </p>
          )}
          <dl className="grid gap-x-6 gap-y-2 text-sm sm:grid-cols-2">
            {s.regimen && (
              <div>
                <dt className="text-stone-600">Régimen</dt>
                <dd>{s.regimen}</dd>
              </div>
            )}
            {s.cifra_negocios && (
              <div>
                <dt className="text-stone-600">Cifra de negocios</dt>
                <dd>
                  <span className="tabular-nums">{euros(s.cifra_negocios.valor)}</span>{' '}
                  <span className="text-xs text-stone-600">({s.cifra_negocios.fuente})</span>
                </dd>
              </div>
            )}
            {s.base && (
              <div>
                <dt className="text-stone-600">
                  {s.persona_fisica ? 'Rendimiento neto' : 'Base imponible'}
                </dt>
                <dd>
                  <span className="tabular-nums">{euros(s.base.valor)}</span>{' '}
                  <span className="text-xs text-stone-600">({s.base.fuente})</span>
                </dd>
              </div>
            )}
            {s.tipo !== null && (
              <div>
                <dt className="text-stone-600">Tipo con el que se calcula el ahorro</dt>
                <dd>
                  {s.tipo} %{s.persona_fisica ? ' (marginal aproximado de la Renta)' : ''}
                </dd>
              </div>
            )}
          </dl>
          {s.notas.length > 0 && (
            <ul className="list-disc space-y-1 pl-5 text-sm text-stone-700">
              {s.notas.map((n) => (
                <li key={n}>{n}</li>
              ))}
            </ul>
          )}
          <AskAiButton
            what={`tu plan de ahorro de ${s.año}`}
            questions={preguntas({ kind: 'ahorro_plan', year: s.año })}
            className="btn-ghost -ml-3"
            onOpen={(b) =>
              ia.open(
                {
                  topic: { kind: 'ahorro_plan', year: s.año },
                  title: `Tu plan de ahorro de ${s.año}`,
                  conversationId: null,
                },
                b,
              )
            }
          />
          <p className="text-xs text-stone-600">
            Son orientaciones calculadas con tus datos y la ley de {s.año}. Antes de firmar,
            invertir o presentar nada, confírmalo con tu asesor.
          </p>
        </section>
      )}
      {data && (
        <section aria-labelledby="ahorro-lista-h" className="space-y-3">
          <h3 id="ahorro-lista-h" tabIndex={-1} className="sr-only">
            Recomendaciones
          </h3>
          <fieldset>
            <legend className="sr-only">Qué recomendaciones ver</legend>
            <div className="flex flex-wrap gap-2">
              {FILTROS.map((f) => (
                <button
                  key={f.id}
                  type="button"
                  aria-pressed={filtro === f.id}
                  className={filtro === f.id ? 'btn-primary' : 'border px-3 py-1.5 text-sm'}
                  onClick={() => setFiltro(f.id)}
                >
                  {f.label} ({cuenta[f.id]})
                </button>
              ))}
            </div>
          </fieldset>
          {visibles.length === 0 && (
            <p className="text-sm text-stone-600">No hay ninguna en este apartado.</p>
          )}
          {visibles.map((r) => {
            const topic: AssistantTopic = { kind: 'ahorro', id: r.id, year: data.situacion.año };
            return (
              <Tarjeta
                key={r.id}
                r={r}
                cierre={data.situacion.cierre}
                busy={busy}
                onMarca={(a) => marcar(r, a)}
                questions={preguntas(topic)}
                onAsk={(b) => ia.open({ topic, title: r.titulo, conversationId: null }, b)}
                {...(onNavigate ? { onNavigate } : {})}
              />
            );
          })}
        </section>
      )}
      {ia.element}
    </div>
  );
}

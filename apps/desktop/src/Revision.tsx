import { ClipboardCheck, Copy, FileDown, LoaderCircle } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { AskAiButton, useAskAi } from './AssistantPanel';
import {
  type AreaRevision,
  type Destinatario,
  type DevolucionPendiente,
  errorMessage,
  euros,
  type FormatoInforme,
  findingTopic,
  folderGet,
  folderReview,
  folderReviewLast,
  folderReviewSave,
  formatDate,
  type Gravedad,
  type Hallazgo,
  type Peticion,
  pickFolder,
  plural,
  type Responsable,
  type ResumenRevision,
  type Revision,
  type TablaRevision,
} from './api';
import { BloqueUrgente, textoUrgente } from './Urgentes';

const GRAVEDAD: Record<Gravedad, { titulo: string; varios: string; pill: string; borde: string }> =
  {
    error: { titulo: 'Error', varios: 'Errores', pill: 'pill-red', borde: 'border-l-red-600' },
    aviso: { titulo: 'Aviso', varios: 'Avisos', pill: 'pill-amber', borde: 'border-l-amber-500' },
    info: {
      titulo: 'Para saber',
      varios: 'Para saber',
      pill: 'pill-stone',
      borde: 'border-l-stone-300',
    },
  };

const AREA: Record<AreaRevision, string> = {
  ficheros: 'Ficheros',
  facturas: 'Facturas',
  emitidas: 'Facturas emitidas',
  libros: 'Libros registro',
  modelos: 'Modelos presentados',
  sociedades: 'Impuesto sobre Sociedades',
  bancos: 'Bancos',
  plazos: 'Plazos y Registro Mercantil',
  aeat: 'AEAT: certificados, requerimientos y devoluciones',
  socios: 'Socios y administradores',
};

const RESPONSABLE: Record<Responsable, string> = {
  empresa: 'Empresa',
  gestoria: 'Gestoría',
  documentacion: 'Documentación',
};

const DESTINATARIO: Record<Destinatario, string> = {
  gestoria: 'A la gestoría',
  registro: 'Al Registro Mercantil',
  aeat: 'En la Sede de la AEAT',
  empresa: 'En la empresa',
};

const FORMATOS: { id: FormatoInforme; titulo: string }[] = [
  { id: 'pdf', titulo: 'PDF' },
  { id: 'xlsx', titulo: 'Excel' },
  { id: 'md', titulo: 'Markdown' },
];

/** Los valores distintos de una lista, en el orden en que salen. */
function distintos<T>(xs: T[]): T[] {
  return [...new Set(xs)];
}

/** Lo que hay que pedir, como texto para pegar en un correo. */
export function textoPedir(pedir: Peticion[]): string {
  return distintos(pedir.map((p) => p.a_quien))
    .map(
      (q) =>
        `${DESTINATARIO[q]}:\n${pedir
          .filter((p) => p.a_quien === q)
          .map((p) => `- ${p.que}`)
          .join('\n')}`,
    )
    .join('\n\n');
}

function Cifra({ titulo, valor, color }: { titulo: string; valor: string; color: string }) {
  return (
    <div className="card p-4">
      <dt className="text-xs text-stone-600">{titulo}</dt>
      <dd className={`mt-1 text-2xl font-semibold tabular-nums ${color}`}>{valor}</dd>
    </div>
  );
}

/** Hallazgos cuyo importe es para el próximo 200, no corrigiendo modelos (como en Rust). */
const PROXIMO_200 = ['sociedades.retencion_dividendos_grupo', 'sociedades.dividendos_confirmados'];

/** Lo que se recupera en el próximo 200 y lo que se pierde sin la exención, o `null`. */
export function textoProximo200(r: ResumenRevision): string | null {
  const ret = r.retenciones_proximo_200 ?? 0;
  const exencion = r.exencion_proximo_200 ?? 0;
  const partes: string[] = [];
  if (ret > 0)
    partes.push(
      `En el próximo 200 se recuperan unos ${euros(r.a_favor_proximo_200 ?? 0)}: ${euros(ret)} de retenciones soportadas, que se restan de la cuota, menos ${euros(r.impuesto_dividendos_proximo_200 ?? 0)} de impuesto que corresponde a esos dividendos con la exención (estimación).`,
    );
  if (exencion > 0)
    partes.push(
      `Si no se aplica la exención de los dividendos (casilla [00370]), se pierden unos ${euros(exencion)}: compruébalo en el borrador antes de presentarlo.`,
    );
  return partes.length ? partes.join(' ') : null;
}

/** «El 200 de 2025», «El 303 del 4T 2025». */
function modeloYPeriodo(d: DevolucionPendiente): string {
  return d.periodo === '0A' || d.periodo === ''
    ? `El ${d.modelo} de ${d.ejercicio}`
    : `El ${d.modelo} del ${d.periodo} ${d.ejercicio}`;
}

/** Una línea por devolución pedida: modelo y año, importe, cómo se pidió y estado. */
export function lineaDevolucion(d: DevolucionPendiente): string {
  const cuando = d.presentada ? ` el ${formatDate(d.presentada)}` : '';
  return `${modeloYPeriodo(d)}: ${euros(d.importe)}${d.estimado ? ' (estimación)' : ''}, pedida por ${d.via}${cuando}; ${d.estado}.`;
}

/** Devoluciones ya pedidas a Hacienda que no se ven cobradas: aparte de lo recuperable. */
function DevolucionesPendientes({ r }: { r: Revision }) {
  const lista = r.devoluciones_pendientes ?? [];
  if (lista.length === 0) return null;
  const total = r.resumen.pendiente_cobro ?? lista.reduce((a, d) => a + d.importe, 0);
  return (
    <section
      aria-labelledby="rev-devoluciones"
      className="card border-l-4 border-sky-600 p-3 text-sm text-stone-800"
    >
      <h4 id="rev-devoluciones" className="font-semibold">
        Devoluciones pedidas, pendientes de cobrar: {euros(total)}
      </h4>
      <ul className="mt-1 list-disc space-y-0.5 pl-5">
        {lista.map((d) => (
          <li key={`${d.modelo}-${d.ejercicio}-${d.periodo}-${d.via}`}>{lineaDevolucion(d)}</li>
        ))}
      </ul>
    </section>
  );
}

function Importe({ cents, codigo }: { cents: number; codigo: string }) {
  if (cents === 0) return null;
  const proximo = PROXIMO_200.includes(codigo);
  return (
    <span className={`pill ${cents > 0 ? 'pill-green' : 'pill-red'}`}>
      {cents < 0 ? 'A pagar' : proximo ? 'A favor en el próximo 200' : 'Recuperable'}{' '}
      {euros(Math.abs(cents))}
    </span>
  );
}

export function HallazgoItem({
  h,
  questions = 0,
  onAsk,
}: {
  h: Hallazgo;
  /** Preguntas de su conversación con la IA (si la hay). */
  questions?: number;
  /** Abre la conversación con la IA de este hallazgo. */
  onAsk?: (h: Hallazgo, from: HTMLButtonElement) => void;
}) {
  const g = GRAVEDAD[h.gravedad];
  return (
    <li className={`card border-l-4 p-4 ${g.borde}`}>
      <div className="flex flex-wrap items-start justify-between gap-2">
        <h4 className="font-semibold">{h.titulo}</h4>
        {h.importe !== null && <Importe cents={h.importe} codigo={h.codigo} />}
      </div>
      <p className="mt-1.5 flex flex-wrap gap-1.5">
        {h.urgente && <span className="pill pill-red">Urgente: puede haber deuda</span>}
        <span className={`pill ${g.pill}`}>{g.titulo}</span>
        <span className="pill pill-stone">{RESPONSABLE[h.responsable]}</span>
        <span className="pill pill-stone">{AREA[h.area]}</span>
        {h.ejercicio !== null && <span className="pill pill-stone">{h.ejercicio}</span>}
      </p>
      {h.urgente && (
        <p className="mt-2 text-sm font-medium text-red-800">{textoUrgente(h.urgente)}</p>
      )}
      <p className="mt-2 text-sm text-stone-700">{h.detalle}</p>
      {h.rectificable_hasta && (
        <p className="mt-1 text-sm text-stone-600">
          Se puede rectificar hasta el {formatDate(h.rectificable_hasta)}
          {h.rectificable_aprox ? ' aprox.' : ''} (cuatro años desde el fin del plazo, art. 66 LGT
          {h.rectificable_aprox
            ? '; sin los festivos de ese año, el día exacto puede ser algo después'
            : ''}
          ).
        </p>
      )}
      {h.ficheros.length > 0 && (
        <details className="mt-2 text-sm">
          <summary className="cursor-pointer text-stone-600">
            {plural(h.ficheros.length, 'fichero')}
          </summary>
          <ul className="mt-1 max-h-48 space-y-0.5 overflow-y-auto">
            {h.ficheros.map((f) => (
              <li key={f} className="truncate font-mono text-xs text-stone-700" title={f}>
                {f}
              </li>
            ))}
          </ul>
        </details>
      )}
      {onAsk && (
        <div className="mt-2 flex justify-end">
          <AskAiButton what={h.titulo} questions={questions} onOpen={(b) => onAsk(h, b)} />
        </div>
      )}
    </li>
  );
}

export function Tabla({ t }: { t: TablaRevision }) {
  const filas = t.filas.map((celdas, n) => ({ id: n, celdas }));
  return (
    <details className="card p-4">
      <summary className="cursor-pointer font-medium">
        {t.titulo}{' '}
        <span className="font-normal text-stone-600">· {plural(t.filas.length, 'fila')}</span>
      </summary>
      <div className="mt-3 max-h-96 overflow-auto">
        <table className="w-full text-left text-sm">
          <thead>
            <tr className="border-b border-stone-200">
              {t.columnas.map((c) => (
                <th key={c} scope="col" className="px-2 py-1.5 font-medium text-stone-700">
                  {c}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {filas.map((f) => (
              <tr key={f.id} className="border-b border-stone-100 align-top">
                {t.columnas.map((col, j) => (
                  <td key={col} className="whitespace-pre-line px-2 py-1.5">
                    {f.celdas[j]}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </details>
  );
}

/** Pestaña «Revisión» de Importar: revisa la carpeta de la empresa y guarda el informe. */
export function FolderReview() {
  const [root, setRoot] = useState<string | null>(null);
  const [review, setReview] = useState<Revision | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [gravedad, setGravedad] = useState<Gravedad | 'todas'>('todas');
  const [area, setArea] = useState<AreaRevision | 'todas'>('todas');
  const [responsable, setResponsable] = useState<Responsable | 'todos'>('todos');
  const [ejercicio, setEjercicio] = useState<number | 'todos'>('todos');
  const ia = useAskAi();
  const preguntar = (h: Hallazgo, from: HTMLButtonElement) =>
    ia.open({ topic: findingTopic(h, 'carpeta'), title: h.titulo, conversationId: null }, from);

  useEffect(() => {
    folderGet()
      .then(setRoot)
      .catch(() => setRoot(null));
    // La última revisión, para no esperar otra vez al volver a la pestaña.
    folderReviewLast()
      .then((r) => {
        if (r) setReview((actual) => actual ?? r);
      })
      .catch((e) => console.warn('Última revisión no disponible:', errorMessage(e)));
  }, []);

  async function revisar(dir: string) {
    setBusy(true);
    setError(null);
    setMsg(null);
    try {
      setReview(await folderReview(dir));
      setRoot(dir);
      setGravedad('todas');
      setArea('todas');
      setResponsable('todos');
      setEjercicio('todos');
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function guardar(f: FormatoInforme) {
    if (!review) return;
    setError(null);
    setMsg(null);
    try {
      if (await folderReviewSave(review, f)) setMsg('Informe guardado.');
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  const hallazgos = review?.hallazgos ?? [];
  const areas = useMemo(() => distintos(hallazgos.map((h) => h.area)), [hallazgos]);
  const responsables = useMemo(() => distintos(hallazgos.map((h) => h.responsable)), [hallazgos]);
  const ejercicios = useMemo(
    () =>
      distintos(hallazgos.map((h) => h.ejercicio).filter((y): y is number => y !== null)).sort(
        (a, b) => b - a,
      ),
    [hallazgos],
  );
  const visibles = hallazgos.filter(
    (h) =>
      (gravedad === 'todas' || h.gravedad === gravedad) &&
      (area === 'todas' || h.area === area) &&
      (responsable === 'todos' || h.responsable === responsable) &&
      (ejercicio === 'todos' || h.ejercicio === ejercicio),
  );

  return (
    <div className="mt-5 space-y-5">
      <div className="card p-5">
        <div className="flex flex-wrap items-center gap-4">
          <span className="grid h-11 w-11 place-items-center rounded-xl bg-brand-50 text-brand-700">
            <ClipboardCheck size={22} aria-hidden="true" />
          </span>
          <div className="min-w-0 flex-1">
            <h3 className="font-semibold">Revisión de la carpeta</h3>
            <p className="truncate text-sm text-stone-600">
              {root ?? 'Elige la carpeta donde tienes lo que te da la gestoría.'}
            </p>
          </div>
          <div className="flex gap-2">
            {root && (
              <button
                type="button"
                className="btn-primary rounded px-3 py-1.5 text-sm"
                disabled={busy}
                onClick={() => revisar(root)}
              >
                Revisar la carpeta
              </button>
            )}
            <button
              type="button"
              className={
                root
                  ? 'rounded border px-3 py-1.5 text-sm'
                  : 'btn-primary rounded px-3 py-1.5 text-sm'
              }
              disabled={busy}
              onClick={async () => {
                try {
                  const dir = await pickFolder();
                  if (dir) await revisar(dir);
                } catch (e) {
                  setError(errorMessage(e));
                }
              }}
            >
              {root ? 'Otra carpeta…' : 'Elegir carpeta…'}
            </button>
          </div>
        </div>
        <p className="mt-3 text-xs text-stone-600">
          TaxPain mira todo lo que hay en la carpeta con lo que ya ha leído (también lo leído con IA
          o con Claude Code) y te dice lo que no cuadra: facturas repetidas o mal hechas, saltos en
          la numeración, el libro frente a los PDF, los modelos que faltan, el 303 frente a las
          facturas y el Impuesto sobre Sociedades. No cambia nada ni envía nada (salvo lo que
          preguntes a la IA). Es orientativa: no sustituye a un asesor, y las cifras de impuestos
          son estimaciones para confirmar con la gestoría.
        </p>
      </div>

      {busy && (
        <p role="status" className="flex items-center gap-2 text-sm text-stone-600">
          <LoaderCircle size={16} className="animate-spin" aria-hidden="true" />
          Revisando la carpeta… (con muchas facturas en PDF puede tardar un poco)
        </p>
      )}
      {error && (
        <p role="alert" className="rounded-md bg-red-50 p-3 text-sm text-red-800">
          {error}
        </p>
      )}

      {review && !busy && (
        <>
          <BloqueUrgente hallazgos={review.hallazgos} id="rev-urgente" />
          <section aria-labelledby="rev-resumen" className="space-y-3">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <h3 id="rev-resumen" className="font-semibold">
                Resumen
                <span className="ml-2 text-sm font-normal text-stone-600">
                  {review.empresa || review.nif
                    ? `${review.empresa}${review.nif ? ` (${review.nif})` : ''} · `
                    : ''}
                  revisada el {formatDate(review.generada)}
                </span>
              </h3>
              <fieldset className="flex flex-wrap items-center gap-2 text-sm">
                <legend className="sr-only">Guardar el informe</legend>
                <span className="text-stone-600">Guardar el informe:</span>
                {FORMATOS.map((f) => (
                  <button
                    key={f.id}
                    type="button"
                    className="btn-ghost"
                    aria-label={`Guardar el informe en ${f.titulo}`}
                    onClick={() => guardar(f.id)}
                  >
                    <FileDown size={14} aria-hidden="true" />
                    {f.titulo}
                  </button>
                ))}
              </fieldset>
            </div>
            {msg && (
              <p role="status" className="text-sm text-stone-700">
                {msg}
              </p>
            )}
            <dl className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
              <Cifra
                titulo="Errores"
                valor={String(review.resumen.errores)}
                color={review.resumen.errores > 0 ? 'text-red-700' : 'text-stone-900'}
              />
              <Cifra
                titulo="Avisos"
                valor={String(review.resumen.avisos)}
                color={review.resumen.avisos > 0 ? 'text-amber-800' : 'text-stone-900'}
              />
              <Cifra
                titulo="Para saber"
                valor={String(review.resumen.info)}
                color="text-stone-900"
              />
              <Cifra
                titulo="Recuperable estimado"
                valor={euros(Math.max(review.resumen.recuperable, 0))}
                color={review.resumen.recuperable > 0 ? 'text-emerald-700' : 'text-stone-900'}
              />
            </dl>
            {textoProximo200(review.resumen) && (
              <p className="card border-l-4 border-emerald-600 p-3 text-sm text-stone-800">
                {textoProximo200(review.resumen)}
              </p>
            )}
            <DevolucionesPendientes r={review} />
            <p className="text-sm text-stone-600">
              {plural(review.resumen.ficheros, 'fichero')}: {review.resumen.con_detalle} con datos,{' '}
              {review.resumen.sin_leer} sin leer y {review.resumen.no_admitidos} en formatos que no
              se leen.
            </p>
          </section>

          <section aria-labelledby="rev-hallazgos" className="space-y-3">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <h3 id="rev-hallazgos" className="font-semibold">
                Lo que se ha encontrado
                {visibles.length !== hallazgos.length && (
                  <span className="ml-2 text-sm font-normal text-stone-600">
                    {visibles.length} de {hallazgos.length}
                  </span>
                )}
              </h3>
              {hallazgos.length > 0 && (
                <div className="flex flex-wrap gap-3 text-sm">
                  <label className="field-inline">
                    Gravedad
                    <select
                      value={gravedad}
                      onChange={(e) => setGravedad(e.target.value as Gravedad | 'todas')}
                    >
                      <option value="todas">Todas</option>
                      {(['error', 'aviso', 'info'] as const).map((g) => (
                        <option key={g} value={g}>
                          {GRAVEDAD[g].varios}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="field-inline">
                    Área
                    <select
                      value={area}
                      onChange={(e) => setArea(e.target.value as AreaRevision | 'todas')}
                    >
                      <option value="todas">Todas</option>
                      {areas.map((a) => (
                        <option key={a} value={a}>
                          {AREA[a]}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="field-inline">
                    Responsable
                    <select
                      value={responsable}
                      onChange={(e) => setResponsable(e.target.value as Responsable | 'todos')}
                    >
                      <option value="todos">Todos</option>
                      {responsables.map((r) => (
                        <option key={r} value={r}>
                          {RESPONSABLE[r]}
                        </option>
                      ))}
                    </select>
                  </label>
                  {ejercicios.length > 0 && (
                    <label className="field-inline">
                      Ejercicio
                      <select
                        value={ejercicio}
                        onChange={(e) =>
                          setEjercicio(
                            e.target.value === 'todos' ? 'todos' : Number(e.target.value),
                          )
                        }
                      >
                        <option value="todos">Todos</option>
                        {ejercicios.map((y) => (
                          <option key={y} value={y}>
                            {y}
                          </option>
                        ))}
                      </select>
                    </label>
                  )}
                </div>
              )}
            </div>
            {hallazgos.length === 0 ? (
              <p role="status" className="card p-4 text-sm text-stone-700">
                No se ha encontrado nada que no cuadre en la carpeta.
              </p>
            ) : visibles.length === 0 ? (
              <p className="text-sm text-stone-600">Nada con estos filtros.</p>
            ) : (
              <ul className="space-y-3">
                {visibles.map((h) => (
                  <HallazgoItem
                    key={`${h.codigo}-${h.ejercicio}-${h.titulo}`}
                    h={h}
                    questions={ia.questions(findingTopic(h, 'carpeta'))}
                    onAsk={preguntar}
                  />
                ))}
              </ul>
            )}
          </section>

          {review.pedir.length > 0 && <QuePedir pedir={review.pedir} />}

          {review.tablas.length > 0 && (
            <section aria-labelledby="rev-tablas" className="space-y-3">
              <h3 id="rev-tablas" className="font-semibold">
                Tablas
              </h3>
              {review.tablas.map((t) => (
                <Tabla key={t.titulo} t={t} />
              ))}
            </section>
          )}
        </>
      )}
      {ia.element}
    </div>
  );
}

export function QuePedir({ pedir }: { pedir: Peticion[] }) {
  const [copiado, setCopiado] = useState(false);
  return (
    <section aria-labelledby="rev-pedir" className="card p-5 text-sm">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h3 id="rev-pedir" className="font-semibold">
          Qué pedir
        </h3>
        <button
          type="button"
          className="btn-ghost"
          onClick={() => {
            navigator.clipboard
              ?.writeText(textoPedir(pedir))
              .then(() => setCopiado(true))
              .catch(() => setCopiado(false));
          }}
        >
          <Copy size={14} aria-hidden="true" />
          {copiado ? 'Copiado' : 'Copiar la lista'}
        </button>
      </div>
      {distintos(pedir.map((p) => p.a_quien)).map((q) => (
        <div key={q} className="mt-3">
          <h4 className="font-medium">{DESTINATARIO[q]}</h4>
          <ul className="mt-1 list-disc space-y-1 pl-5 text-stone-700">
            {pedir
              .filter((p) => p.a_quien === q)
              .map((p) => (
                <li key={p.que}>{p.que}</li>
              ))}
          </ul>
        </div>
      ))}
    </section>
  );
}

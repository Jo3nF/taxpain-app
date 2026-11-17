import {
  Bitcoin,
  BookOpen,
  Building2,
  ChartCandlestick,
  CircleAlert,
  CircleCheck,
  Download,
  FileQuestion,
  FileText,
  FolderOpen,
  Inbox,
  Landmark,
  LoaderCircle,
  type LucideIcon,
  Sparkles,
  Wallet,
} from 'lucide-react';
import {
  type FormEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import {
  type AeatCheckTarget,
  type AeatCsvItem,
  aeatCheckTarget,
  aeatCsvDownload,
  aeatCsvRunning,
  type BookBatch,
  type CompanyDoc,
  type CompanyFacts,
  type CompanySuggestion,
  type Comparison,
  companyApplySuggestion,
  companyDocAi,
  companyDocDelete,
  companyDocOpen,
  companyDocsList,
  companySuggestions,
  errorMessage,
  euros,
  type FiledModel,
  type FolderFound,
  type FolderInventory,
  type FolderItem,
  type FolderOutcome,
  filedCompare,
  filedDelete,
  filedList,
  filedSaveManual,
  folderAiRead,
  folderApply,
  folderBookImports,
  folderBookUndo,
  folderGet,
  folderScan,
  formatDate,
  type LecturaIaSettings,
  lecturaIaGet,
  lecturaIaSave,
  onAeatCsvDone,
  onAeatCsvProgress,
  onFolderAiProgress,
  parseEuros,
  pickFolder,
  plural,
  receivedAiFillPending,
  type WatchedPending,
  watchedDismiss,
  watchedPending,
} from './api';
import { CertificateSelect } from './CertificateSelect';
import { ConfirmAction } from './ConfirmAction';
import { YearStepper } from './Controls';
import { useInlineDialog, useReturnFocus } from './dialogs';
import { BOX_LABELS } from './Models';
import { FolderReview } from './Revision';

export type ImportTarget = 'recibidas' | 'contabilidad' | 'modelos';

type Group =
  | 'documento'
  | 'modelo'
  | 'libro'
  | 'diario'
  | 'banco'
  | 'cripto'
  | 'valores'
  | 'factura'
  | 'otro';

const GROUPS: { id: Group; title: string; icon: LucideIcon; hint: string }[] = [
  {
    id: 'documento',
    title: 'Documentos de la empresa',
    icon: Building2,
    hint: 'Escrituras, tarjeta NIF, alta en el ROI (NIF-IVA), 036/037, certificados, nota simple… Se guardan cifrados y sus datos sirven para completar la ficha de la empresa.',
  },
  {
    id: 'libro',
    title: 'Libros de facturas',
    icon: BookOpen,
    hint: 'Libros registro de IVA de emitidas y recibidas: alimentan el 303, 390, 347 y 349.',
  },
  {
    id: 'diario',
    title: 'Contabilidad',
    icon: Landmark,
    hint: 'Diario de la gestoría (SUENLACE de a3, Excel o CSV): alimenta el balance, el 200 y el 111.',
  },
  {
    id: 'modelo',
    title: 'Modelos presentados',
    icon: FileText,
    hint: 'Las declaraciones del gestor: se guardan para compararlas con las que calcula TaxPain.',
  },
  {
    id: 'banco',
    title: 'Extractos del banco',
    icon: Wallet,
    hint: 'De cualquier banco y formato (PDF, Excel, CSV, Norma 43…), comprobados con sus saldos: movimientos para conciliar.',
  },
  {
    id: 'cripto',
    title: 'Criptomonedas',
    icon: Bitcoin,
    hint: 'Trade List de CoinTracking, ledgers de Kraken, hojas de operaciones o de saldos por fecha, también dentro de un zip: operaciones y precios de cierre para Criptomonedas (coste, ganancias y 721).',
  },
  {
    id: 'valores',
    title: 'Inversiones en valores',
    icon: ChartCandlestick,
    hint: 'Informe de cuenta de XTB en PDF o Excel (uno por cuenta) y Activity Flex de Interactive Brokers (XML o CSV): operaciones, dividendos, retenciones y precios para Inversiones en valores, sin duplicar lo ya importado.',
  },
  {
    id: 'factura',
    title: 'Facturas sueltas',
    icon: Inbox,
    hint: 'Las recibidas que ya están en el libro se le adjuntan; las demás quedan en Recibidas para revisar.',
  },
  {
    id: 'otro',
    title: 'Sin reconocer',
    icon: FileQuestion,
    hint: 'No se importan. Si es algo útil, dime qué es y lo añadimos.',
  },
];

const IA_GROUP: Record<string, Group> = {
  diario: 'diario',
  libro_emitidas: 'libro',
  libro_recibidas: 'libro',
  factura_recibida: 'factura',
  factura_emitida: 'factura',
  modelo_aeat: 'modelo',
  extracto_bancario: 'banco',
  sumas_saldos: 'otro',
  nomina: 'otro',
  otro: 'otro',
};

function groupOf(f: FolderFound): Group {
  if (f.kind === 'ia') return IA_GROUP[f.tipo] ?? 'documento';
  switch (f.kind) {
    case 'factura_recibida':
    case 'factura_emitida':
      return 'factura';
    default:
      return f.kind;
  }
}

function range(first: string | null, last: string | null): string {
  if (!first || !last) return '';
  return first === last ? formatDate(first) : `${formatDate(first)} – ${formatDate(last)}`;
}

/** Lo que se ha visto en un fichero, en una línea. */
/** Los datos principales leídos de un documento, en una línea. */
export function factsLine(f: CompanyFacts): string {
  return [
    f.name,
    f.nif ? `NIF ${f.nif}` : null,
    f.vat_number ? `NIF-IVA ${f.vat_number}` : null,
    f.constitution_date ? `constituida el ${formatDate(f.constitution_date)}` : null,
    f.capital !== null ? `capital ${euros(f.capital)}` : null,
    f.cnae ? `CNAE ${f.cnae}` : null,
    f.iae ? `IAE ${f.iae}` : null,
    f.administrator ? `administración: ${f.administrator}` : null,
  ]
    .filter(Boolean)
    .join(' · ');
}

/** Sin tildes y en minúsculas, para buscar. */
function plain(s: string): string {
  return s
    .normalize('NFD')
    .replace(/\p{Diacritic}/gu, '')
    .toLowerCase();
}

export function describe(f: FolderFound): string {
  switch (f.kind) {
    case 'ia':
      return [f.titulo, f.detalle, f.descripcion].filter(Boolean).join(' · ');
    case 'documento': {
      const facts = factsLine(f.facts);
      return f.by_name
        ? `${f.title} (por el nombre del fichero)`
        : facts
          ? `${f.title} · ${facts}`
          : f.title;
    }
    case 'diario':
      return `${plural(f.entries, 'asiento')} · ${range(f.first, f.last)}${f.already ? ' · ya importado' : ''}${
        f.overlap
          ? ` · ${plural(f.overlap, 'asiento propio', 'asientos propios')} en esas fechas`
          : ''
      }${f.imported ? ` · ${plural(f.imported, 'asiento', 'asientos')} de otro lote importado en esas fechas` : ''}`;
    case 'libro': {
      // La base y el IVA son los de lo que entra: sin la autorrepercusión de compras.
      const selfAssessed = f.self_assessed ?? 0;
      const overlap = f.overlap
        ? ` · ${plural(f.overlap, 'factura propia', 'facturas propias')} en esas fechas`
        : '';
      if (selfAssessed > 0 && selfAssessed >= f.invoices) {
        return `Libro de ${f.book} · ${plural(f.invoices, 'línea')} · autorrepercusión de compras, no ventas · ${range(f.first, f.last)}${overlap}`;
      }
      return `Libro de ${f.book} · ${plural(f.invoices - selfAssessed, 'factura')} · base ${euros(f.base)} · IVA ${euros(f.vat)} · ${range(f.first, f.last)}${overlap}${
        selfAssessed > 0
          ? ` · y ${plural(selfAssessed, 'línea', 'líneas')} de autorrepercusión de compras, que no ${selfAssessed === 1 ? 'entra' : 'entran'}`
          : ''
      }`;
    }
    case 'banco':
      return [
        'Extracto bancario',
        f.resumen,
        f.cuadra === false ? 'no cuadra con sus saldos' : null,
      ]
        .filter(Boolean)
        .join(' · ');
    case 'factura_recibida':
      return [
        'Recibida',
        f.supplier,
        f.number,
        f.date ? formatDate(f.date) : null,
        f.total !== null ? euros(f.total) : null,
        f.in_book ? 'ya en el libro: se adjunta' : null,
        f.historical ? 'de un periodo ya declarado' : null,
      ]
        .filter(Boolean)
        .join(' · ');
    case 'factura_emitida':
      return [
        'Emitida por la empresa (entra con su libro)',
        f.number,
        f.date ? formatDate(f.date) : null,
        f.total !== null ? euros(f.total) : null,
      ]
        .filter(Boolean)
        .join(' · ');
    case 'modelo':
      if (f.source === 'registros') {
        return `Modelo ${f.model} · ${f.year} ${f.period} · ${f.model === '349' ? plural(f.boxes, 'operador leído', 'operadores leídos') : plural(f.boxes, 'tercero leído', 'terceros leídos')} del fichero (se cruza en la revisión de la carpeta)`;
      }
      if (f.source === 'justificante') {
        return f.boxes_read === false
          ? `Justificante de la AEAT del modelo ${f.model} · ${f.year} ${f.period} · casillas sin leer (cuenta como presentado)`
          : `Justificante de la AEAT del modelo ${f.model} · ${f.year} ${f.period} · ${plural(f.boxes, 'casilla leída', 'casillas leídas')}`;
      }
      return `Modelo ${f.model} · ${f.year} ${f.period} · ${plural(f.boxes, 'casilla leída', 'casillas leídas')} del ${f.source === 'pdf' ? 'PDF' : 'fichero'}`;
    case 'cripto':
      return [
        'Criptomonedas',
        f.resumen,
        f.duplicates > 0 && f.duplicates >= f.ops && f.prices === 0 ? 'ya importadas' : null,
        f.unread > 0 ? `${plural(f.unread, 'fichero')} del zip sin leer` : null,
      ]
        .filter(Boolean)
        .join(' · ');
    case 'valores':
      return [
        `Bróker: ${f.formato}`,
        f.cuenta ? `cuenta ${f.cuenta}` : null,
        f.resumen,
        f.duplicates > 0 && f.duplicates >= f.ops && f.prices === 0 ? 'ya importado' : null,
      ]
        .filter(Boolean)
        .join(' · ');
    case 'otro':
      return f.reason;
  }
}

/** Ficheros que las reglas no entienden del todo (los que tiene sentido leer con la IA). */
export function needsAi(i: FolderItem): boolean {
  const f = i.found;
  switch (f.kind) {
    case 'otro':
      return true;
    case 'factura_recibida':
      return !f.supplier || !f.number || !f.date || f.total === null;
    case 'factura_emitida':
      return true;
    case 'documento':
      return f.by_name || Object.values(f.facts).every((x) => x === null);
    case 'modelo':
      return f.source === 'pdf' || (f.source === 'justificante' && f.boxes_read === false);
    case 'banco':
      // Un extracto que no cuadra puede ser que no se haya leído bien.
      return f.cuadra === false;
    default:
      return false;
  }
}

function FullIa({
  s,
  onChange,
}: {
  s: LecturaIaSettings;
  onChange: (s: LecturaIaSettings) => void;
}) {
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const askRef = useRef<HTMLButtonElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const wasAsking = useRef(false);
  // Al abrir el aviso, el foco va a «Sí, activar»; al cerrarlo, vuelve a «Activar Full IA…».
  useEffect(() => {
    if (asking) confirmRef.current?.focus();
    else if (wasAsking.current) askRef.current?.focus();
    wasAsking.current = asking;
  }, [asking]);
  /** Guarda y devuelve si ha ido bien (el error se muestra aquí). */
  const save = async (n: LecturaIaSettings) => {
    setError(null);
    try {
      onChange(await lecturaIaSave(n));
      return true;
    } catch (e) {
      setError(errorMessage(e));
      return false;
    }
  };
  return (
    <div className="card p-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex min-w-0 items-start gap-3">
          <span className="icon-tile" aria-hidden="true">
            <Sparkles size={18} />
          </span>
          <div className="min-w-0">
            <h3 className="font-semibold">
              Full IA {s.full && <span className="pill pill-green ml-1">activada</span>}
            </h3>
            <p className="text-sm text-stone-600">
              {s.full
                ? 'El asistente puede leer todo: Excel, PDF, escaneados y fotos. Las hojas salen anonimizadas; los PDF y las imágenes, tal cual, al proveedor de IA de Ajustes del asistente. Los importes de libros y diario se siguen leyendo de las celdas.'
                : 'Desactivada: TaxPain reconoce los ficheros con sus reglas y el reconocimiento de texto del Mac, y no envía nada fuera. Sin pagar API: en Claude Code, con tu suscripción, escribe /leer-carpeta y la ruta de esta carpeta (lo que lee va a Anthropic); luego pulsa «Volver a leer» y lo leído sale con la etiqueta «IA».'}
            </p>
          </div>
        </div>
        {s.full ? (
          <button
            ref={askRef}
            type="button"
            className="rounded border px-3"
            onClick={() => save({ ...s, full: false })}
          >
            Desactivar
          </button>
        ) : (
          <button
            ref={askRef}
            type="button"
            className="btn-primary rounded px-3"
            onClick={() => setAsking(true)}
          >
            Activar Full IA…
          </button>
        )}
      </div>
      {asking && !s.full && (
        <div
          role="alertdialog"
          aria-label="Activar Full IA"
          className="mt-4 rounded-lg border border-amber-300 bg-amber-50 p-4 text-sm text-amber-900"
          onKeyDown={(e) => {
            if (e.key === 'Escape') setAsking(false);
          }}
        >
          <p className="font-medium">Antes de activarlo</p>
          <ul className="mt-1 list-disc space-y-1 pl-5">
            <li>
              Los PDF y las imágenes se envían <strong>completos</strong> al proveedor de IA (con
              nombres, NIF, direcciones e importes): no se pueden anonimizar.
            </li>
            <li>Cada fichero leído cuesta una consulta a tu cuenta del proveedor.</li>
            <li>Nada se importa sin que lo revises; puedes desactivarlo cuando quieras.</li>
          </ul>
          <div className="mt-3 flex gap-2">
            <button
              ref={confirmRef}
              type="button"
              className="btn-primary rounded px-3"
              onClick={async () => {
                if (await save({ ...s, full: true })) setAsking(false);
              }}
            >
              Sí, activar Full IA
            </button>
            <button type="button" className="rounded border px-3" onClick={() => setAsking(false)}>
              Cancelar
            </button>
          </div>
        </div>
      )}
      {s.full && (
        <div className="form-grid mt-4">
          <label className="span-2">
            Modelo para leer documentos
            <select
              value={s.model ?? ''}
              onChange={(e) => save({ ...s, model: e.target.value || null })}
            >
              <option value="">El del asistente</option>
              <option value="claude-sonnet-5-5">Claude Sonnet 5.5 (más barato)</option>
              <option value="claude-haiku-5-5">Claude Haiku 5.5 (el más barato)</option>
              <option value="claude-opus-5-5">Claude Opus 5.5 (el mejor)</option>
            </select>
          </label>
        </div>
      )}
      {error && (
        <p role="alert" className="mt-3 text-sm text-red-700">
          No se ha podido guardar: {error}
        </p>
      )}
    </div>
  );
}

/** Lo que se dice de cada CSV en la lista de resultados. */
export function csvLine(r: AeatCsvItem): string {
  switch (r.state) {
    case 'guardado':
      return `Guardado en ${r.path}.`;
    case 'ya_estaba':
      return `Ya estaba en la carpeta: ${r.path}.`;
    case 'no_encontrado':
      return 'La AEAT no tiene ningún documento con este código. Revisa que esté bien copiado.';
    case 'anulado':
      return r.replacement
        ? 'La AEAT lo tiene como anulado: hay otro documento que lo sustituye.'
        : 'La AEAT lo tiene como anulado.';
    case 'no_disponible':
      return r.message;
    case 'error':
      return r.message;
  }
}

/** Resumen de una descarga, para leerlo de un vistazo (y para el lector de pantalla). */
function csvSummary(list: AeatCsvItem[]): string {
  const saved = list.filter((r) => r.state === 'guardado').length;
  const already = list.filter((r) => r.state === 'ya_estaba').length;
  const other = list.length - saved - already;
  const parts = [
    saved > 0 && plural(saved, 'documento guardado', 'documentos guardados'),
    already > 0 && plural(already, 'ya estaba', 'ya estaban'),
    other > 0 && plural(other, 'no se ha podido descargar', 'no se han podido descargar'),
  ].filter(Boolean);
  return `${parts.join(' · ')}.`;
}

/**
 * Cambia la línea del CSV anulado por lo que dio el que lo sustituye (sin repetir códigos); el
 * resto de la lista se queda como estaba.
 */
export function mergeCsvResults(
  prev: AeatCsvItem[] | null,
  annulled: string,
  fresh: AeatCsvItem[],
): AeatCsvItem[] {
  const codes = new Set(fresh.map((x) => x.csv));
  const base = (prev ?? []).filter((x) => x.csv === annulled || !codes.has(x.csv));
  const i = base.findIndex((x) => x.csv === annulled);
  return i < 0 ? [...base, ...fresh] : [...base.slice(0, i), ...fresh, ...base.slice(i + 1)];
}

/**
 * Una descarga por CSV en marcha, o acabada sin que nadie haya visto el resultado. Vive fuera del
 * recuadro: si el usuario sale de Importar a mitad, el proceso nativo sigue descargando y, al
 * volver, el recuadro la sigue esperando y enseña el resultado.
 */
type CsvJob = {
  root: string;
  /** Si se descarga el sustituto de un anulado: el CSV anulado (su línea se cambia). */
  annulled: string | null;
  promise: Promise<AeatCsvItem[]>;
  seen: boolean;
};
let csvJob: CsvJob | null = null;

/** Olvida la descarga guardada (para las pruebas). */
export function forgetCsvJob() {
  csvJob = null;
}

/**
 * «Descargar de la AEAT por CSV»: el usuario pega los códigos que ve en la Sede y TaxPain baja
 * cada documento a la subcarpeta «Documentos AEAT» de la carpeta de la empresa. Después se
 * vuelve a leer la carpeta para que salgan con su lectura; leerlos con IA es otro paso (envía
 * los ficheros al proveedor).
 */
function CsvDownload({
  root,
  iaFull,
  blocked,
  onRescan,
  onBusy,
}: {
  root: string | null;
  iaFull: boolean;
  blocked: boolean;
  /** Vuelve a leer la carpeta; `null` si ya no es la carpeta elegida. */
  onRescan: (dir: string) => Promise<FolderInventory | null>;
  /** Avisa de si el recuadro está ocupado (para bloquear la carpeta mientras tanto). */
  onBusy: (busy: boolean) => void;
}) {
  const [text, setText] = useState('');
  const [cert, setCert] = useState('');
  const [busy, setBusy] = useState<'download' | 'replace' | 'ai' | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [results, setResults] = useState<AeatCsvItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [toRead, setToRead] = useState<string[]>([]);
  const [aiMsg, setAiMsg] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<{
    codes: string;
    annulled: string | null;
    target: AeatCheckTarget;
  } | null>(null);
  // El certificado de otro titular ya confirmado (para no preguntar en cada sustituto).
  const confirmedFor = useRef<string | null>(null);
  const mounted = useRef(true);
  const rootRef = useRef(root);
  rootRef.current = root;
  const downloadBtn = useRef<HTMLButtonElement>(null);
  const summaryRef = useRef<HTMLParagraphElement>(null);
  const aiMsgRef = useRef<HTMLParagraphElement>(null);
  // Adónde va el foco al acabar (el botón pulsado desaparece).
  const focusNext = useRef<'summary' | 'ai' | null>(null);
  const dialog = useInlineDialog(confirm !== null, false, () => setConfirm(null));
  useReturnFocus(confirm !== null, downloadBtn);

  useEffect(() => {
    onBusy(busy !== null);
  }, [busy, onBusy]);

  useEffect(() => {
    if (busy !== null) return;
    const target = focusNext.current;
    focusNext.current = null;
    if (target === 'summary') summaryRef.current?.focus();
    if (target === 'ai') aiMsgRef.current?.focus();
  }, [busy]);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;
    void onAeatCsvProgress(setProgress).then((f) => {
      if (cancelled) f();
      else stop = f;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  // Al volver a Importar con una descarga en marcha (o acabada sin ver), se sigue esperando.
  // biome-ignore lint/correctness/useExhaustiveDependencies: sólo al abrir
  useEffect(() => {
    mounted.current = true;
    let alive = true;
    let stop: (() => void) | undefined;
    const job = csvJob;
    if (job && !job.seen) {
      void follow(job);
    } else {
      // Una descarga que esta ventana no conoce (por ejemplo, tras recargarla).
      Promise.resolve()
        .then(() => aeatCsvRunning())
        .then(async (running) => {
          if (!alive || !running) return;
          setBusy('download');
          const done = () => {
            setBusy(null);
            setProgress(null);
            const dir = rootRef.current;
            if (dir) onRescan(dir).catch(() => {});
          };
          stop = await onAeatCsvDone(done);
          if (!alive) stop();
          else if (!(await aeatCsvRunning())) done();
        })
        .catch(() => {});
    }
    return () => {
      alive = false;
      mounted.current = false;
      stop?.();
    };
  }, []);

  async function follow(job: CsvJob) {
    setBusy(job.annulled ? 'replace' : 'download');
    let r: AeatCsvItem[];
    try {
      r = await job.promise;
    } catch (e) {
      if (!mounted.current) return;
      job.seen = true;
      setError(errorMessage(e));
      setBusy(null);
      setProgress(null);
      return;
    }
    if (!mounted.current) return;
    job.seen = true;
    const annulled = job.annulled;
    setResults((prev) => (annulled ? mergeCsvResults(prev, annulled, r) : r));
    const saved = new Set(r.flatMap((x) => (x.state === 'guardado' ? [x.path] : [])));
    let fresh: string[] = [];
    if (saved.size > 0) {
      try {
        const inv = await onRescan(job.root);
        fresh = (inv?.items ?? [])
          .filter((i) => saved.has(i.path) && needsAi(i))
          .map((i) => i.path);
      } catch (e) {
        if (mounted.current) setError(errorMessage(e));
      }
    }
    if (!mounted.current) return;
    setToRead((prev) => [...new Set([...(annulled ? prev : []), ...fresh])]);
    if (annulled) focusNext.current = 'summary';
    setBusy(null);
    setProgress(null);
  }

  function run(codes: string, annulled: string | null, otherHolder: boolean) {
    if (!root) return;
    if (!annulled) {
      // Una descarga nueva: lo de la anterior ya no vale.
      setResults(null);
      setToRead([]);
    }
    setProgress(null);
    const job: CsvJob = {
      root,
      annulled,
      seen: false,
      promise: aeatCsvDownload(codes, cert || null, otherHolder),
    };
    csvJob = job;
    void follow(job);
  }

  /** Con certificado, primero se mira de quién es (como en «Comprobar lo presentado»). */
  async function download(codes: string, annulled: string | null) {
    if (!root) return;
    setError(null);
    setAiMsg(null);
    if (!cert) {
      run(codes, annulled, false);
      return;
    }
    try {
      const t = await aeatCheckTarget(cert);
      if (!t.company_nif) {
        setError(
          'Pon el NIF de la empresa (en Facturación o en la ficha de la empresa) antes de descargar con el certificado.',
        );
        return;
      }
      if (t.other_entity) {
        setError(
          `El certificado es de representante de ${t.cert_nif} y la empresa abierta es ${t.company_nif}: elige el de esta empresa, o descarga sin certificado.`,
        );
        return;
      }
      if (!t.same && confirmedFor.current !== cert) {
        setConfirm({ codes, annulled, target: t });
        return;
      }
      run(codes, annulled, !t.same);
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  async function readWithAi() {
    if (!root || toRead.length === 0) return;
    setBusy('ai');
    setError(null);
    try {
      const r = await folderAiRead(root, toRead, false);
      setAiMsg(
        `Leídos con IA: ${r.read}${r.reused ? ` (${r.reused} ya estaban leídos)` : ''}${
          r.failed.length
            ? `. No se pudieron leer ${r.failed.length}: ${r.failed[0]?.[0]} (${r.failed[0]?.[1]}). Puedes volver a intentarlo`
            : ''
        }.`,
      );
      // Los que fallaron se quedan, para reintentarlos desde aquí.
      const failed = new Set(r.failed.map((f) => f[0]));
      setToRead(toRead.filter((p) => failed.has(p)));
      focusNext.current = 'ai';
      await onRescan(root);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="card p-5" aria-labelledby="csv-title">
      <div className="flex items-start gap-3">
        <span className="icon-tile" aria-hidden="true">
          <Download size={18} />
        </span>
        <div>
          <h3 id="csv-title" className="font-semibold">
            Descargar de la AEAT por CSV
          </h3>
          <p className="text-sm text-stone-600">
            Pega los códigos CSV que ves en la Sede (en «Mis expedientes», en los asientos
            registrales, en un justificante…) y TaxPain baja cada documento en PDF a la subcarpeta
            «Documentos AEAT» de la carpeta de la empresa. Sólo descarga: no abre notificaciones ni
            cambia nada en la AEAT.
          </p>
        </div>
      </div>
      {!root && (
        <p className="mt-3 text-sm text-amber-800">
          Elige primero la carpeta de la empresa (arriba): ahí se guardan los documentos.
        </p>
      )}
      <label htmlFor="csv-codes" className="mt-3 block text-sm font-medium">
        Códigos CSV
      </label>
      <textarea
        id="csv-codes"
        className="mt-1 block w-full rounded border border-stone-300 p-2 font-mono text-sm"
        rows={4}
        value={text}
        aria-describedby="csv-hint"
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
      />
      <p id="csv-hint" className="mt-1 text-xs text-stone-500">
        Uno por línea, o pega la tabla de la Sede tal cual: TaxPain busca los códigos de 16 letras y
        cifras. Hasta 50 de una vez. Sale del Mac sólo cada código, hacia la AEAT. Con certificado
        se usa primero el servicio oficial (te pedirá Touch ID); sin él, la página pública de la
        Sede. Lo que ya está en la carpeta no se vuelve a pedir.
      </p>
      <div className="mt-3 flex flex-wrap items-center gap-3">
        <CertificateSelect value={cert} onChange={setCert} optional />
        <button
          ref={downloadBtn}
          type="button"
          className="btn-primary rounded px-3 py-1.5 text-sm"
          disabled={!root || !text.trim() || busy !== null || blocked || confirm !== null}
          onClick={() => download(text, null)}
        >
          <Download size={16} aria-hidden="true" />
          {busy === 'download' ? 'Descargando…' : 'Descargar'}
        </button>
      </div>
      {confirm && (
        <div
          ref={dialog.ref}
          onKeyDown={dialog.onKeyDown}
          role="dialog"
          aria-label="Descargar con este certificado"
          className="mt-3 space-y-2 rounded-lg border border-amber-300 bg-amber-50 p-3 text-sm"
        >
          <p>
            El certificado es de {confirm.target.cert_name || confirm.target.cert_nif} (
            {confirm.target.cert_nif}), no de la empresa ({confirm.target.company_nif}). Úsalo sólo
            si está apoderado o es colaborador social de la empresa en la AEAT: las consultas quedan
            a su nombre.
          </p>
          <div className="flex gap-3">
            <button
              type="button"
              className="btn-primary rounded px-3 py-1.5"
              onClick={() => {
                const c = confirm;
                setConfirm(null);
                confirmedFor.current = cert;
                run(c.codes, c.annulled, true);
              }}
            >
              Descargar con este certificado
            </button>
            <button type="button" className="text-link" onClick={() => setConfirm(null)}>
              Cancelar
            </button>
          </div>
        </div>
      )}

      {/* Sólo el progreso y el resumen se leen en voz alta; la lista, al recorrerla. */}
      <div role="status" className="mt-3">
        {busy === 'download' && (
          <p className="flex items-center gap-2 text-sm text-stone-600">
            <LoaderCircle size={16} className="animate-spin" aria-hidden="true" />
            {progress && progress.total > 0
              ? `Descargando ${Math.min(progress.done + 1, progress.total)} de ${progress.total}…`
              : 'Descargando de la AEAT…'}
          </p>
        )}
        {busy === 'replace' && (
          <p className="flex items-center gap-2 text-sm text-stone-600">
            <LoaderCircle size={16} className="animate-spin" aria-hidden="true" />
            Descargando el documento que lo sustituye…
          </p>
        )}
        {results && busy !== 'download' && (
          <p ref={summaryRef} tabIndex={-1} className="text-sm font-medium">
            {csvSummary(results)}
          </p>
        )}
      </div>
      {busy === 'download' && progress && progress.total > 0 && (
        <div
          className="mt-1 h-2 overflow-hidden rounded-full bg-stone-100"
          role="progressbar"
          aria-label="Descarga de la AEAT"
          aria-valuemin={0}
          aria-valuemax={progress.total}
          aria-valuenow={progress.done}
        >
          <div
            className="h-full rounded-full bg-brand-600 transition-all"
            style={{ width: `${(progress.done / progress.total) * 100}%` }}
          />
        </div>
      )}
      {results && busy !== 'download' && (
        <ul className="mt-2 space-y-1.5 text-sm" aria-label="Resultado de cada código">
          {results.map((r) => (
            <li key={r.csv} className="flex items-start gap-2">
              {r.state === 'guardado' || r.state === 'ya_estaba' ? (
                <CircleCheck size={16} className="mt-0.5 text-emerald-600" aria-hidden="true" />
              ) : (
                <CircleAlert size={16} className="mt-0.5 text-amber-700" aria-hidden="true" />
              )}
              <span className="min-w-0">
                <span className="font-mono">{r.csv}</span>: {csvLine(r)}
                {r.state === 'anulado' && r.replacement && (
                  <button
                    type="button"
                    className="ml-2 rounded border px-2 py-0.5 text-xs"
                    disabled={busy !== null || blocked}
                    onClick={() => download(r.replacement ?? '', r.csv)}
                  >
                    Descargar el que lo sustituye ({r.replacement})
                  </button>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}
      {toRead.length > 0 && busy !== 'download' && (
        <div className="mt-3 rounded-lg bg-brand-50 p-3 text-sm text-stone-800">
          {iaFull ? (
            <div className="flex flex-wrap items-center justify-between gap-3">
              <p>
                {plural(toRead.length, 'documento descargado', 'documentos descargados')} que las
                reglas no entienden del todo. Leerlos con IA envía los PDF al proveedor de IA.
              </p>
              <button
                type="button"
                className="btn-primary rounded px-3 py-1.5 text-sm"
                disabled={busy !== null || blocked}
                onClick={readWithAi}
              >
                <Sparkles size={16} aria-hidden="true" />
                {busy === 'ai' ? 'Leyendo…' : 'Leer con IA los descargados'}
              </button>
            </div>
          ) : (
            <p>
              {plural(toRead.length, 'documento descargado', 'documentos descargados')} que las
              reglas no entienden del todo. Para leerlos con IA: en Claude Code escribe
              /leer-carpeta y la ruta de esta carpeta, o activa Full IA (arriba).
            </p>
          )}
        </div>
      )}
      <div role="status" className="mt-3">
        {aiMsg && busy === null && (
          <p ref={aiMsgRef} tabIndex={-1} className="text-sm">
            {aiMsg}
          </p>
        )}
      </div>
      {error && (
        <p role="alert" className="mt-3 rounded-md bg-red-50 p-3 text-sm text-red-800">
          {error}
        </p>
      )}
    </section>
  );
}

/**
 * Avisos de las carpetas vigiladas: los documentos nuevos que no se importan solos (extractos,
 * modelos…) y, aparte, los ficheros que TaxPain no ha podido leer.
 */
function WatchedNotice({
  pending,
  locked,
  onReview,
  onDismiss,
}: {
  pending: WatchedPending[];
  locked: boolean;
  onReview: (folder: string) => void;
  onDismiss: (folder: string) => void;
}) {
  const docs = pending.filter((p) => !p.error);
  const unreadable = pending.filter((p) => p.error);
  if (pending.length === 0) return null;
  return (
    <>
      {docs.length > 0 && (
        <div role="status" className="mt-3 rounded border border-amber-300 bg-amber-50 p-3 text-sm">
          <p className="font-medium">
            {docs.length === 1
              ? 'Hay 1 documento nuevo en las carpetas vigiladas'
              : `Hay ${docs.length} documentos nuevos en las carpetas vigiladas`}
          </p>
          <p className="text-xs text-stone-600">
            No son facturas recibidas, así que no se han importado solos: revísalos y elige cuáles
            entran.
          </p>
          <WatchedFolders
            items={docs}
            locked={locked}
            onReview={onReview}
            onDismiss={onDismiss}
            line={(i) => (
              <>
                {i.kind}: <span className="break-all">{i.path}</span>
              </>
            )}
          />
        </div>
      )}
      {unreadable.length > 0 && (
        <div role="status" className="mt-3 rounded border border-red-300 bg-red-50 p-3 text-sm">
          <p className="font-medium">Ficheros que TaxPain no ha podido leer</p>
          <p className="text-xs text-stone-600">
            Están en las carpetas vigiladas, pero no se han podido abrir (dañados, protegidos o
            demasiado grandes). Ábrelos tú para ver qué son.
          </p>
          <WatchedFolders
            items={unreadable}
            locked={locked}
            onDismiss={onDismiss}
            line={(i) => (
              <>
                <span className="break-all">{i.path}</span>: {i.error}
              </>
            )}
          />
        </div>
      )}
    </>
  );
}

function WatchedFolders({
  items,
  locked,
  onReview,
  onDismiss,
  line,
}: {
  items: WatchedPending[];
  locked: boolean;
  onReview?: (folder: string) => void;
  onDismiss: (folder: string) => void;
  line: (i: WatchedPending) => ReactNode;
}) {
  const byFolder = new Map<string, WatchedPending[]>();
  for (const p of items) byFolder.set(p.folder, [...(byFolder.get(p.folder) ?? []), p]);
  return (
    <>
      {[...byFolder].map(([folder, list]) => (
        <div key={folder} className="mt-2">
          <p className="break-all text-xs font-medium">{folder}</p>
          <ul className="ml-4 list-disc text-xs">
            {list.slice(0, 5).map((i) => (
              <li key={i.path}>{line(i)}</li>
            ))}
            {list.length > 5 && <li>y {list.length - 5} más</li>}
          </ul>
          <div className="mt-1 flex gap-2">
            {onReview && (
              <button
                type="button"
                className="rounded border px-2 py-1 text-xs"
                disabled={locked}
                onClick={() => onReview(folder)}
              >
                Revisar esta carpeta
              </button>
            )}
            <button
              type="button"
              className="rounded px-2 py-1 text-xs underline"
              disabled={locked}
              onClick={() => onDismiss(folder)}
            >
              Quitar aviso
            </button>
          </div>
        </div>
      ))}
    </>
  );
}

function FolderImport({
  onNavigate,
  onApplied,
}: {
  onNavigate: (t: ImportTarget) => void;
  onApplied?: () => void;
}) {
  const [root, setRoot] = useState<string | null>(null);
  const [inv, setInv] = useState<FolderInventory | null>(null);
  const [busy, setBusy] = useState<'scan' | 'apply' | 'ai' | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [year, setYear] = useState<'todos' | number>('todos');
  const [query, setQuery] = useState('');
  const [outcomes, setOutcomes] = useState<FolderOutcome[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filling, setFilling] = useState(false);
  const [aiMsg, setAiMsg] = useState<string | null>(null);
  const [ia, setIa] = useState<LecturaIaSettings | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number; path: string } | null>(
    null,
  );
  const [allAi, setAllAi] = useState(false);
  const [watched, setWatched] = useState<WatchedPending[]>([]);
  const loadWatched = useCallback(() => {
    watchedPending()
      .then(setWatched)
      .catch(() => setWatched([]));
  }, []);
  // Una descarga por CSV (o su lectura con IA) en marcha: la carpeta no se cambia mientras tanto.
  const [csvBusy, setCsvBusy] = useState(false);
  const rootRef = useRef(root);
  rootRef.current = root;
  const locked = busy !== null || csvBusy;

  useEffect(() => {
    lecturaIaGet()
      .then(setIa)
      .catch(() => setIa(null));
    let stop: (() => void) | undefined;
    let cancelled = false;
    void onFolderAiProgress(setProgress).then((f) => {
      if (cancelled) f();
      else stop = f;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  useEffect(() => {
    folderGet()
      .then(setRoot)
      .catch(() => setRoot(null));
    loadWatched();
  }, [loadWatched]);

  const scan = useCallback(
    async (dir: string) => {
      setBusy('scan');
      setError(null);
      setOutcomes(null);
      try {
        const r = await folderScan(dir);
        setInv(r);
        setRoot(dir);
        setSelected(new Set(r.items.filter((i) => i.importable).map((i) => i.path)));
      } catch (e) {
        setError(errorMessage(e));
      } finally {
        setBusy(null);
        loadWatched();
      }
    },
    [loadWatched],
  );

  const years = useMemo(
    () =>
      [...new Set((inv?.items ?? []).map((i) => i.year).filter((y): y is number => y !== null))]
        .sort()
        .reverse(),
    [inv],
  );
  const words = plain(query).split(/\s+/).filter(Boolean);
  const visible = (inv?.items ?? []).filter(
    (i) =>
      (year === 'todos' || i.year === year || i.year === null) &&
      (words.length === 0 ||
        ((t) => words.every((w) => t.includes(w)))(
          plain([i.path, describe(i.found), ...i.warnings].join(' ')),
        )),
  );
  const canPick = (i: FolderItem) => i.importable || i.selectable === true;
  const chosen = visible.filter((i) => canPick(i) && selected.has(i.path));
  const pickable = visible.filter(canPick);

  /** Marca o desmarca de golpe los que se ven (con el buscador o el ejercicio). */
  function toggleVisible(on: boolean) {
    const s = new Set(selected);
    for (const i of pickable) {
      if (on) s.add(i.path);
      else s.delete(i.path);
    }
    setSelected(s);
  }

  /**
   * Vuelve a leer la carpeta respetando lo que se había desmarcado: sólo se marcan los que antes
   * no se podían importar. Si entretanto se ha elegido otra carpeta, no hace nada (`null`): no se
   * vuelve sola a la anterior.
   */
  async function rescan(dir: string): Promise<FolderInventory | null> {
    if (rootRef.current !== dir) return null;
    const before = new Set((inv?.items ?? []).filter((i) => i.importable).map((i) => i.path));
    const inv2 = await folderScan(dir);
    if (rootRef.current !== dir) return null;
    setInv(inv2);
    setRoot(dir);
    setSelected(
      (prev) =>
        new Set(
          inv2.items
            .filter(
              (i) => (prev.has(i.path) && canPick(i)) || (i.importable && !before.has(i.path)),
            )
            .map((i) => i.path),
        ),
    );
    return inv2;
  }

  function toggle(i: FolderItem, on: boolean) {
    const s = new Set(selected);
    if (on) s.add(i.path);
    else s.delete(i.path);
    setSelected(s);
  }

  async function apply() {
    if (!root) return;
    setBusy('apply');
    setError(null);
    try {
      setOutcomes(
        await folderApply(
          root,
          chosen.map((i) => i.path),
        ),
      );
      onApplied?.();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  const toReview = (outcomes ?? []).filter((o) => o.ok && o.message.startsWith('a Recibidas'));

  return (
    <div className="mt-5 space-y-5">
      <div className="card p-5">
        <div className="flex flex-wrap items-center gap-4">
          <span className="grid h-11 w-11 place-items-center rounded-xl bg-brand-50 text-brand-700">
            <FolderOpen size={22} aria-hidden="true" />
          </span>
          <div className="min-w-0 flex-1">
            <h3 className="font-semibold">Carpeta de la empresa</h3>
            <p className="truncate text-sm text-stone-600">
              {root ?? 'Elige la carpeta donde tienes lo que te da la gestoría.'}
            </p>
          </div>
          <div className="flex gap-2">
            {root && (
              <button
                type="button"
                className="rounded border px-3 py-1.5 text-sm"
                disabled={locked}
                onClick={() => scan(root)}
              >
                Volver a leer
              </button>
            )}
            <button
              type="button"
              className="btn-primary rounded px-3 py-1.5 text-sm"
              disabled={locked}
              onClick={async () => {
                try {
                  const dir = await pickFolder();
                  if (dir) await scan(dir);
                } catch (e) {
                  setError(errorMessage(e));
                }
              }}
            >
              Elegir carpeta…
            </button>
          </div>
        </div>
        <WatchedNotice
          pending={watched}
          locked={locked}
          onReview={(f) => void scan(f)}
          onDismiss={async (f) => {
            try {
              await watchedDismiss(f);
            } catch (e) {
              setError(errorMessage(e));
            }
            loadWatched();
          }}
        />
        <p className="mt-3 text-xs text-stone-500">
          TaxPain lee la carpeta (y sus subcarpetas) en este Mac y te dice qué es cada fichero. Leer
          no cambia nada: tú eliges qué se importa.{' '}
          {ia?.full
            ? 'Leer la carpeta no envía nada; con Full IA, «Leer con IA» envía los ficheros al proveedor de IA (los PDF y las imágenes, completos).'
            : 'Nada sale del ordenador.'}{' '}
          Reconoce libros registro de IVA, el diario (a3, Excel, CSV), modelos presentados (fichero
          de la AEAT o PDF), extractos del banco y facturas en PDF o Facturae.
        </p>
      </div>

      {ia && <FullIa s={ia} onChange={setIa} />}

      <CsvDownload
        root={root}
        iaFull={ia?.full === true}
        blocked={busy !== null}
        onRescan={rescan}
        onBusy={setCsvBusy}
      />

      {busy === 'scan' && (
        <p role="status" className="flex items-center gap-2 text-sm text-stone-600">
          <LoaderCircle size={16} className="animate-spin" aria-hidden="true" />
          Leyendo la carpeta… (con muchas facturas en PDF puede tardar un poco)
        </p>
      )}
      {error && (
        <p role="alert" className="rounded-md bg-red-50 p-3 text-sm text-red-800">
          {error}
        </p>
      )}

      {inv && busy !== 'scan' && (
        <>
          {inv.claude_code && (
            <p role="status" className="rounded-lg bg-brand-50 p-3 text-sm text-stone-800">
              <Sparkles size={15} aria-hidden="true" className="mr-1 inline text-brand-700" />
              Claude Code ya ha leído {plural(inv.claude_code.loaded, 'fichero')} de esta carpeta
              (con tu suscripción, sin coste de API): salen con la etiqueta «IA».
              {inv.claude_code.missing.length === 1 &&
                ' 1 lectura es de un fichero que ya no está.'}
              {inv.claude_code.missing.length > 1 &&
                ` ${inv.claude_code.missing.length} lecturas son de ficheros que ya no están.`}
              {(inv.claude_code.kept ?? 0) > 0 &&
                ` ${plural(inv.claude_code.kept ?? 0, 'lectura no se usa', 'lecturas no se usan')}: el asistente de la app leyó esos ficheros después.`}
              {inv.claude_code.invalid > 0 &&
                ` ${plural(inv.claude_code.invalid, 'lectura mal escrita se salta', 'lecturas mal escritas se saltan')}.`}
              {(inv.claude_code.merged ?? 0) > 0 &&
                ` ${plural(inv.claude_code.merged ?? 0, 'lectura es de una copia', 'lecturas son de copias')} de otro fichero con el mismo contenido: se juntan con la suya, sin perder datos.`}
            </p>
          )}
          {inv.claude_code?.error && (
            <p role="alert" className="rounded-lg bg-amber-50 p-3 text-sm text-amber-950">
              No se pudo leer taxpain-lecturas.json ({inv.claude_code.error}). La carpeta se lee
              igual; si Claude Code aún está trabajando, espera a que termine y pulsa «Volver a
              leer».
            </p>
          )}
          {((inv.over_limit ?? 0) > 0 || (inv.too_deep ?? 0) > 0) && (
            <p role="alert" className="rounded-lg bg-amber-50 p-3 text-sm text-amber-950">
              La carpeta está incompleta:
              {(inv.over_limit ?? 0) > 0 &&
                ` tiene ${plural((inv.over_limit ?? 0) + inv.items.length, 'fichero')} y sólo se han leído los ${inv.items.length} primeros (divídela por años o por empresas).`}
              {(inv.too_deep ?? 0) > 0 &&
                ` ${plural(inv.too_deep ?? 0, 'subcarpeta', 'subcarpetas')} a más de ocho niveles no se ${inv.too_deep === 1 ? 'mira' : 'miran'}.`}
            </p>
          )}
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="text-sm text-stone-600">
              {plural(inv.items.length, 'fichero leído', 'ficheros leídos')}
              {inv.skipped === 1 && ' · 1 de otro tipo no se mira'}
              {inv.skipped > 1 && ` · ${inv.skipped} de otros tipos no se miran`}
            </p>
            <label className="field-inline min-w-0 flex-1 sm:max-w-sm">
              Buscar
              <input
                type="search"
                className="min-w-0 flex-1"
                value={query}
                placeholder="Nombre, carpeta, banco, proveedor…"
                aria-label="Buscar en los ficheros de la carpeta"
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
            <label className="field-inline">
              Ejercicio
              <select
                value={year}
                onChange={(e) =>
                  setYear(e.target.value === 'todos' ? 'todos' : Number(e.target.value))
                }
              >
                <option value="todos">Todos</option>
                {years.map((y) => (
                  <option key={y} value={y}>
                    {y}
                  </option>
                ))}
              </select>
            </label>
          </div>
          {words.length > 0 && (
            <div className="flex flex-wrap items-center gap-3 text-sm text-stone-700">
              <p role="status">
                {visible.length === 0
                  ? `Ningún fichero tiene «${query.trim()}».`
                  : `${plural(visible.length, 'fichero tiene', 'ficheros tienen')} «${query.trim()}». Sólo se importan los marcados que se ven.`}
              </p>
              {pickable.length > 0 && (
                <>
                  <button type="button" className="btn-ghost" onClick={() => toggleVisible(true)}>
                    {pickable.length === 1
                      ? 'Marcar el que se ve'
                      : `Marcar los ${pickable.length} que se ven`}
                  </button>
                  <button type="button" className="btn-ghost" onClick={() => toggleVisible(false)}>
                    Desmarcarlos
                  </button>
                </>
              )}
              <button
                type="button"
                className="text-sm text-brand-700 underline"
                onClick={() => setQuery('')}
              >
                Quitar la búsqueda
              </button>
            </div>
          )}

          {ia?.full && (
            <div className="card flex flex-wrap items-center justify-between gap-3 p-5">
              <div className="text-sm">
                <p className="font-medium">Leer con IA</p>
                <p className="text-stone-600">
                  {allAi
                    ? plural(visible.length, 'fichero')
                    : `${plural(visible.filter(needsAi).length, 'fichero')} que las reglas no entienden del todo`}
                  . Lo leído se guarda: no se vuelve a pagar.
                </p>
                <label className="mt-2">
                  <input
                    type="checkbox"
                    checked={allAi}
                    onChange={(e) => setAllAi(e.target.checked)}
                  />
                  También los ya reconocidos
                </label>
              </div>
              <button
                type="button"
                className="btn-primary rounded px-3"
                disabled={locked || !root}
                onClick={async () => {
                  if (!root) return;
                  setBusy('ai');
                  setError(null);
                  try {
                    const list = (allAi ? visible : visible.filter(needsAi)).map((i) => i.path);
                    const r = await folderAiRead(root, list, false);
                    setAiMsg(
                      `Leídos con IA: ${r.read}${r.reused ? ` (${r.reused} ya estaban leídos)` : ''}${
                        r.failed.length
                          ? `. No se pudieron leer ${r.failed.length}: ${r.failed[0]?.[0]} (${r.failed[0]?.[1]})`
                          : ''
                      }.`,
                    );
                    await rescan(root);
                  } catch (e) {
                    setError(errorMessage(e));
                  } finally {
                    setBusy(null);
                    setProgress(null);
                  }
                }}
              >
                <Sparkles size={16} aria-hidden="true" />
                {busy === 'ai' ? 'Leyendo…' : 'Leer con IA'}
              </button>
              {busy === 'ai' && progress && (
                <div className="w-full">
                  <div
                    className="h-2 overflow-hidden rounded-full bg-stone-100"
                    role="progressbar"
                    aria-label="Lectura con IA"
                    aria-valuemin={0}
                    aria-valuemax={progress.total}
                    aria-valuenow={progress.done}
                  >
                    <div
                      className="h-full rounded-full bg-brand-600 transition-all"
                      style={{
                        width: `${progress.total ? (progress.done / progress.total) * 100 : 0}%`,
                      }}
                    />
                  </div>
                  <p className="mt-1 truncate text-xs text-stone-500">
                    {progress.done} de {progress.total} · {progress.path}
                  </p>
                </div>
              )}
              {aiMsg && busy !== 'ai' && (
                <p role="status" className="w-full text-sm">
                  {aiMsg}
                </p>
              )}
            </div>
          )}

          {GROUPS.map((g) => {
            const items = visible.filter((i) => groupOf(i.found) === g.id);
            if (items.length === 0) return null;
            const Icon = g.icon;
            return (
              <section key={g.id} className="card p-5" aria-labelledby={`g-${g.id}`}>
                <div className="flex items-start gap-3">
                  <span className="icon-tile" aria-hidden="true">
                    <Icon size={18} />
                  </span>
                  <div>
                    <h3 id={`g-${g.id}`} className="font-semibold">
                      {g.title} <span className="pill pill-stone ml-1">{items.length}</span>
                    </h3>
                    <p className="text-sm text-stone-600">{g.hint}</p>
                  </div>
                </div>
                <ul className="mt-3 divide-y divide-stone-200">
                  {items.map((i) => (
                    <li key={i.path} className="flex items-start gap-3 py-2.5 text-sm">
                      {canPick(i) ? (
                        <input
                          type="checkbox"
                          className="mt-1"
                          aria-label={`Importar ${i.path}`}
                          checked={selected.has(i.path)}
                          onChange={(e) => toggle(i, e.target.checked)}
                        />
                      ) : (
                        <span className="w-[13px]" aria-hidden="true" />
                      )}
                      <div className="min-w-0 flex-1">
                        <p className="truncate font-medium">
                          {i.path}
                          {i.found.kind === 'ia' && (
                            <span className="pill pill-green ml-2">IA</span>
                          )}
                        </p>
                        <p className="text-stone-600">{describe(i.found)}</p>
                        {i.warnings.map((w) => (
                          <p key={w} className="text-xs text-amber-800">
                            {w}
                          </p>
                        ))}
                      </div>
                    </li>
                  ))}
                </ul>
              </section>
            );
          })}

          <div className="card flex flex-wrap items-center justify-between gap-3 p-5">
            <p className="text-sm text-stone-700">
              {chosen.length === 0
                ? 'No hay nada marcado para importar.'
                : `${plural(chosen.length, 'fichero marcado', 'ficheros marcados')}. Los asientos entran en un lote que se puede deshacer (Contabilidad → IVA y libros → Importar asientos); las facturas de los libros, sin asiento ni VERI*FACTU.`}
            </p>
            {chosen.length > 0 && (
              <ConfirmAction
                label={
                  busy === 'apply' ? 'Importando…' : `Importar ${plural(chosen.length, 'fichero')}`
                }
                className="btn-primary rounded px-4 py-2 text-sm"
                danger={false}
                question={`${chosen.length === 1 ? 'Se importará 1 fichero' : `Se importarán ${chosen.length} ficheros`} a la empresa abierta. Lo que ya estaba no se duplica.`}
                confirmLabel="Sí, importar"
                disabled={locked}
                onConfirm={apply}
              />
            )}
          </div>
        </>
      )}

      {outcomes && (
        <section className="card p-5" aria-labelledby="resultado">
          <h3 id="resultado" className="font-semibold">
            Resultado
          </h3>
          <ul className="mt-2 space-y-1.5 text-sm">
            {outcomes.map((o) => (
              <li key={o.path} className="flex items-start gap-2">
                {o.ok ? (
                  <CircleCheck size={16} className="mt-0.5 text-emerald-600" aria-hidden="true" />
                ) : (
                  <CircleAlert size={16} className="mt-0.5 text-red-700" aria-hidden="true" />
                )}
                <span>
                  <span className="font-medium">{o.path}</span>: {o.message}
                </span>
              </li>
            ))}
          </ul>
          <div className="mt-4 flex flex-wrap gap-2">
            {toReview.length > 0 && (
              <button
                type="button"
                className="rounded border px-3 py-1.5 text-sm"
                onClick={() => onNavigate('recibidas')}
              >
                Revisar {plural(toReview.length, 'factura')} en Recibidas
              </button>
            )}
            {toReview.length > 0 && (
              <button
                type="button"
                className="flex items-center gap-1.5 rounded border px-3 py-1.5 text-sm"
                disabled={filling}
                onClick={async () => {
                  setFilling(true);
                  try {
                    const r = await receivedAiFillPending(toReview.length);
                    setAiMsg(
                      `El asistente ha completado ${plural(r.filled, 'factura')}${
                        r.failed.length > 0 ? `; ${r.failed.length} no: ${r.failed[0]}` : ''
                      }. Revísalas en Recibidas antes de confirmarlas.`,
                    );
                  } catch (e) {
                    setAiMsg(errorMessage(e));
                  } finally {
                    setFilling(false);
                  }
                }}
              >
                <Sparkles size={15} aria-hidden="true" />
                {filling ? 'Completando…' : 'Completar con el asistente'}
              </button>
            )}
            <button
              type="button"
              className="rounded border px-3 py-1.5 text-sm"
              onClick={() => onNavigate('contabilidad')}
            >
              Ver la contabilidad
            </button>
          </div>
          {aiMsg && (
            <p role="status" className="mt-3 text-sm">
              {aiMsg}
            </p>
          )}
          {toReview.length > 0 && (
            <p className="mt-3 text-xs text-stone-500">
              El asistente sólo lee las facturas a las que les faltan datos. Su texto sale
              anonimizado (NIF, nombres, direcciones y cuentas como marcadores) hacia el proveedor
              de IA que elegiste en Ajustes del asistente, o se queda en el Mac si usas un modelo
              local.
            </p>
          )}
        </section>
      )}
    </div>
  );
}

/** Trimestres y, para los que pueden ser mensuales (gran empresa, REDEME, 349), los meses. */
const TRIMESTRES = ['1T', '2T', '3T', '4T'];
const CON_MESES = [
  ...TRIMESTRES,
  ...Array.from({ length: 12 }, (_, i) => String(i + 1).padStart(2, '0')),
];
const COMPARABLE: { id: string; periods: string[] }[] = [
  { id: '303', periods: CON_MESES },
  { id: '390', periods: ['0A'] },
  { id: '111', periods: CON_MESES },
  { id: '115', periods: CON_MESES },
  { id: '123', periods: CON_MESES },
  { id: '130', periods: TRIMESTRES },
  { id: '202', periods: ['1P', '2P', '3P'] },
  { id: '349', periods: CON_MESES },
  // El 200 sólo a mano: su [00599] es la base de los 202 del año siguiente.
  { id: '200', periods: ['0A'] },
  // Informativas: [02] importe total y [03] retenciones (sin el fichero no hay perceptores).
  { id: '347', periods: ['0A'] },
  { id: '190', periods: ['0A'] },
  { id: '180', periods: ['0A'] },
  { id: '193', periods: ['0A'] },
];

const SOURCE: Record<FiledModel['source'], string> = {
  fichero: 'fichero AEAT',
  pdf: 'leído del PDF',
  manual: 'tecleado',
};

function ComparisonView({ c }: { c: Comparison }) {
  const labels = BOX_LABELS[c.filed.model] ?? {};
  return (
    <div className="mt-4">
      {c.mismatches === 0 ? (
        <p
          role="status"
          className="flex items-center gap-2 rounded-lg bg-emerald-50 p-3 text-sm text-emerald-900"
        >
          <CircleCheck size={18} aria-hidden="true" />
          Cuadran todas las casillas del modelo {c.filed.model} {c.filed.period} {c.filed.year}.
        </p>
      ) : (
        <p
          role="status"
          className="flex items-center gap-2 rounded-lg bg-amber-50 p-3 text-sm text-amber-900"
        >
          <CircleAlert size={18} aria-hidden="true" />
          {c.mismatches}{' '}
          {(c.lines?.length ?? 0) > 0
            ? c.mismatches === 1
              ? 'diferencia'
              : 'diferencias'
            : c.mismatches === 1
              ? 'casilla no cuadra'
              : 'casillas no cuadran'}{' '}
          con lo que presentó la gestoría.
        </p>
      )}
      {c.warnings.map((w) => (
        <p key={w} className="mt-2 text-xs text-amber-800">
          {w}
        </p>
      ))}
      <table className="mt-3 w-full text-sm">
        <thead>
          <tr>
            <th className="text-left">Casilla</th>
            <th className="text-left">Concepto</th>
            <th className="text-right">TaxPain</th>
            <th className="text-right">Gestoría</th>
            <th className="text-right">Diferencia</th>
          </tr>
        </thead>
        <tbody>
          {c.rows.map((r) => {
            const off = Math.abs(r.diff) > 1;
            return (
              <tr key={r.box} className={off ? 'bg-amber-50' : ''}>
                <td className="font-mono">[{String(r.box).padStart(2, '0')}]</td>
                <td>
                  {labels[r.box] || r.label || ''}
                  {off && r.explanation && (
                    <span className="block text-xs text-amber-900">{r.explanation}</span>
                  )}
                </td>
                <td className="text-right tabular-nums">{euros(r.ours)}</td>
                <td className="text-right tabular-nums">
                  {r.theirs === null ? '—' : euros(r.theirs)}
                </td>
                <td
                  className={`text-right tabular-nums ${off ? 'font-semibold text-amber-800' : 'text-stone-500'}`}
                >
                  {off ? euros(r.diff) : '✓'}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {(c.lines?.length ?? 0) > 0 && (
        <table className="mt-4 w-full text-sm">
          <caption className="text-left font-medium">
            {c.filed.model === '347' ? 'Tercero a tercero' : 'Perceptor a perceptor'}
          </caption>
          <thead>
            <tr>
              <th className="text-left">NIF</th>
              <th className="text-left">Nombre y clave</th>
              <th className="text-right">TaxPain</th>
              <th className="text-right">Gestoría</th>
              <th className="text-right">Retención TaxPain</th>
              <th className="text-right">Retención gestoría</th>
            </tr>
          </thead>
          <tbody>
            {c.lines?.map((l) => (
              <tr key={`${l.nif}-${l.key}`} className={l.explanation ? 'bg-amber-50' : ''}>
                <td className="font-mono">{l.nif}</td>
                <td>
                  {l.name}
                  {l.key ? ` · ${l.key}` : ''}
                  {l.explanation && (
                    <span className="block text-xs text-amber-900">{l.explanation}</span>
                  )}
                </td>
                <td className="text-right tabular-nums">{l.ours === null ? '—' : euros(l.ours)}</td>
                <td className="text-right tabular-nums">
                  {l.theirs === null ? '—' : euros(l.theirs)}
                </td>
                <td className="text-right tabular-nums">
                  {l.ours_withheld === null ? '—' : euros(l.ours_withheld)}
                </td>
                <td className="text-right tabular-nums">
                  {l.theirs_withheld === null ? '—' : euros(l.theirs_withheld)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function ManualEntry({ year, onSaved }: { year: number; onSaved: (id: number) => void }) {
  const [model, setModel] = useState('303');
  const [period, setPeriod] = useState('1T');
  const [rows, setRows] = useState<{ box: string; amount: string }[]>([{ box: '', amount: '' }]);
  const [msg, setMsg] = useState<string | null>(null);
  const periods = COMPARABLE.find((m) => m.id === model)?.periods ?? ['1T'];

  async function save(e: FormEvent) {
    e.preventDefault();
    const boxes: Record<number, number> = {};
    for (const r of rows) {
      const box = r.box.trim();
      if (!box && !r.amount.trim()) continue;
      const n = Number(box);
      if (!/^\d+$/.test(box) || n < 1 || n > 65535) {
        setMsg(`«${r.box}» no es un número de casilla válido.`);
        return;
      }
      if (n in boxes) {
        setMsg(`La casilla ${n} está repetida.`);
        return;
      }
      const cents = parseEuros(r.amount);
      if (cents === null) {
        setMsg(`Revisa el importe de la casilla ${n}: «${r.amount}».`);
        return;
      }
      boxes[n] = cents;
    }
    if (Object.keys(boxes).length === 0) {
      setMsg('Teclea al menos una casilla con su importe.');
      return;
    }
    try {
      onSaved(await filedSaveManual(model, year, period, boxes));
      setMsg(null);
    } catch (err) {
      setMsg(errorMessage(err));
    }
  }

  return (
    <form
      onSubmit={save}
      className="mt-3 space-y-3 rounded-lg border border-stone-200 p-4"
      aria-label="Teclear un modelo del gestor"
    >
      <div className="flex flex-wrap items-end gap-3">
        <label className="flex w-36 flex-col">
          Modelo
          <select
            value={model}
            onChange={(e) => {
              setModel(e.target.value);
              setPeriod(COMPARABLE.find((m) => m.id === e.target.value)?.periods[0] ?? '1T');
            }}
          >
            {COMPARABLE.map((m) => (
              <option key={m.id} value={m.id}>
                {m.id}
              </option>
            ))}
          </select>
        </label>
        <label className="flex w-36 flex-col">
          Periodo
          <select value={period} onChange={(e) => setPeriod(e.target.value)}>
            {periods.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
        </label>
      </div>
      {rows.map((r, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: filas que sólo crecen
        <div key={i} className="flex flex-wrap items-end gap-3">
          <label className="flex w-36 flex-col">
            Casilla
            <input
              inputMode="numeric"
              value={r.box}
              onChange={(e) =>
                setRows(rows.map((x, j) => (j === i ? { ...x, box: e.target.value } : x)))
              }
            />
          </label>
          <label className="flex w-36 flex-col">
            Importe (€)
            <input
              className="text-right"
              inputMode="decimal"
              value={r.amount}
              onChange={(e) =>
                setRows(rows.map((x, j) => (j === i ? { ...x, amount: e.target.value } : x)))
              }
            />
          </label>
          <span className="pb-2 text-sm text-stone-500">
            {BOX_LABELS[model]?.[Number(r.box)] ?? ''}
          </span>
        </div>
      ))}
      <div className="form-actions">
        <button
          type="button"
          className="rounded border px-3 py-1.5 text-sm"
          onClick={() => setRows([...rows, { box: '', amount: '' }])}
        >
          Otra casilla
        </button>
        <button type="submit" className="btn-primary rounded px-3 py-1.5 text-sm">
          Guardar y comparar
        </button>
      </div>
      {msg && (
        <p role="alert" className="text-sm text-red-700">
          {msg}
        </p>
      )}
    </form>
  );
}

function CompareWithAdvisor() {
  const [year, setYear] = useState(new Date().getFullYear());
  const [list, setList] = useState<FiledModel[]>([]);
  const [open, setOpen] = useState<number | null>(null);
  const [cmp, setCmp] = useState<Comparison | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [typing, setTyping] = useState(false);

  const load = useCallback(() => {
    setError(null);
    filedList(year)
      .then(setList)
      .catch((e) => {
        setList([]);
        setError(errorMessage(e));
      });
  }, [year]);
  useEffect(load, [load]);
  // Se abre en el último año con modelos de la gestoría (si no hay, en el actual).
  useEffect(() => {
    filedList(null)
      .then((all) => {
        if (all.length > 0) setYear(Math.max(...all.map((f) => f.year)));
      })
      .catch(() => {});
  }, []);

  async function compare(id: number) {
    setOpen(id);
    setCmp(null);
    setError(null);
    try {
      setCmp(await filedCompare(id));
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  return (
    <div className="mt-5 space-y-5">
      <div className="card p-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h3 className="font-semibold">Modelos de la gestoría</h3>
            <p className="text-sm text-stone-600">
              TaxPain calcula cada modelo con tus datos y lo compara casilla a casilla con el que
              presentó la gestoría.
            </p>
          </div>
          <YearStepper year={year} onChange={setYear} />
        </div>
        {list.length === 0 ? (
          <p className="mt-4 text-sm text-stone-500">
            No hay modelos de {year}. Impórtalos desde la carpeta (fichero de la AEAT o PDF) o
            teclea las casillas a mano.
          </p>
        ) : (
          <ul className="mt-4 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
            {list.map((f) => (
              <li key={f.id}>
                <button
                  type="button"
                  aria-pressed={open === f.id}
                  className="task-row flex w-full items-center justify-between gap-2 rounded-lg border border-stone-200 px-3 py-2.5 text-left text-sm"
                  onClick={() => compare(f.id)}
                >
                  <span className="flex items-center gap-2">
                    <span className="chip">{f.model}</span>
                    {f.period}
                  </span>
                  <span className="pill pill-stone">{SOURCE[f.source]}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
        <button
          type="button"
          className="mt-4 text-sm"
          aria-expanded={typing}
          onClick={() => setTyping(!typing)}
        >
          {typing ? 'Cerrar' : 'Teclear las casillas de un modelo…'}
        </button>
        {typing && (
          <ManualEntry
            year={year}
            onSaved={(id) => {
              setTyping(false);
              load();
              compare(id);
            }}
          />
        )}
      </div>
      {error && (
        <p role="alert" className="rounded-md bg-red-50 p-3 text-sm text-red-800">
          {error}
        </p>
      )}
      {cmp && (
        <section className="card p-5" aria-labelledby="cmp-title">
          <div className="flex items-center justify-between">
            <h3 id="cmp-title" className="font-semibold">
              Modelo {cmp.filed.model} · {cmp.filed.period} {cmp.filed.year}
            </h3>
            <ConfirmAction
              label="Quitar"
              question="¿Quitar este modelo de la comparación? (no afecta a nada más)"
              confirmLabel="Sí, quitar"
              onConfirm={async () => {
                await filedDelete(cmp.filed.id);
                setCmp(null);
                load();
              }}
            />
          </div>
          <p className="text-xs text-stone-500">{cmp.filed.filename}</p>
          <ComparisonView c={cmp} />
        </section>
      )}
    </div>
  );
}

const FACT_LABELS: [keyof CompanyFacts, string][] = [
  ['name', 'Razón social'],
  ['nif', 'NIF'],
  ['vat_number', 'NIF-IVA'],
  ['street', 'Domicilio'],
  ['post_code', 'Código postal'],
  ['town', 'Población'],
  ['constitution_date', 'Constitución'],
  ['capital', 'Capital social'],
  ['cnae', 'CNAE'],
  ['iae', 'Epígrafe IAE'],
  ['administrator', 'Administración'],
  ['notary', 'Notario'],
  ['protocol', 'Protocolo'],
  ['registry', 'Registro Mercantil'],
  ['date', 'Fecha del documento'],
];

function factValue(k: keyof CompanyFacts, v: string | number): string {
  if (k === 'capital' && typeof v === 'number') return euros(v);
  if ((k === 'constitution_date' || k === 'date') && typeof v === 'string') return formatDate(v);
  return String(v);
}

function CompanyDocuments() {
  const [docs, setDocs] = useState<CompanyDoc[]>([]);
  const [sugg, setSugg] = useState<CompanySuggestion[]>([]);
  const [open, setOpen] = useState<number | null>(null);
  const [busy, setBusy] = useState<number | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [suggError, setSuggError] = useState<string | null>(null);

  const load = useCallback(() => {
    companyDocsList()
      .then(setDocs)
      .catch((e) => setMsg(errorMessage(e)));
    setSuggError(null);
    companySuggestions()
      .then(setSugg)
      .catch((e) => {
        setSugg([]);
        setSuggError(errorMessage(e));
      });
  }, []);
  useEffect(load, [load]);

  return (
    <div className="mt-5 space-y-5">
      <section className="card p-5" aria-labelledby="sugg-title">
        <h3 id="sugg-title" className="font-semibold">
          Datos de la empresa según los documentos
        </h3>
        {suggError ? (
          <p role="alert" className="mt-2 text-sm text-red-700">
            No se han podido comparar los datos de la app con los documentos: {suggError}
          </p>
        ) : sugg.length === 0 ? (
          <p className="mt-2 flex items-center gap-2 text-sm text-stone-600">
            <CircleCheck size={16} className="text-emerald-600" aria-hidden="true" />
            {docs.length === 0
              ? 'Importa las escrituras, la tarjeta NIF o el 036 desde la carpeta y aquí verás qué datos proponen.'
              : 'Los datos de la app coinciden con los de los documentos.'}
          </p>
        ) : (
          <ul className="mt-3 space-y-2 text-sm">
            {sugg.map((g) => (
              <li
                key={g.field}
                className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-stone-200 px-3 py-2.5"
              >
                <div className="min-w-0">
                  <p className="font-medium">{g.label}</p>
                  <p className="text-stone-600">
                    {g.current ? (
                      <>
                        <span className="line-through decoration-stone-400">{g.current}</span> →{' '}
                      </>
                    ) : (
                      'Vacío → '
                    )}
                    <span className="font-medium text-stone-900">{g.proposed}</span>
                  </p>
                  {g.source && <p className="text-xs text-stone-500">Según: {g.source}</p>}
                </div>
                <button
                  type="button"
                  className="btn-primary rounded px-3 py-1.5 text-sm"
                  onClick={async () => {
                    try {
                      await companyApplySuggestion(g.field);
                      setMsg(`${g.label}: actualizado.`);
                      load();
                    } catch (e) {
                      setMsg(errorMessage(e));
                    }
                  }}
                >
                  Aplicar
                </button>
              </li>
            ))}
          </ul>
        )}
        {msg && (
          <p role="status" className="mt-3 text-sm">
            {msg}
          </p>
        )}
      </section>

      <section className="card p-5" aria-labelledby="docs-title">
        <h3 id="docs-title" className="font-semibold">
          Documentos guardados <span className="pill pill-stone ml-1">{docs.length}</span>
        </h3>
        <p className="text-sm text-stone-600">Se guardan cifrados en este Mac, con la empresa.</p>
        <ul className="mt-3 divide-y divide-stone-200">
          {docs.map((d) => {
            const facts = FACT_LABELS.filter(([k]) => d.facts[k] !== null);
            return (
              <li key={d.id} className="py-3 text-sm">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <button
                    type="button"
                    className="flex min-w-0 items-center gap-3 text-left"
                    aria-expanded={open === d.id}
                    onClick={() => setOpen(open === d.id ? null : d.id)}
                  >
                    <span className="icon-tile" aria-hidden="true">
                      <Building2 size={18} />
                    </span>
                    <span className="min-w-0">
                      <span className="block font-medium text-stone-900">{d.title}</span>
                      <span className="block truncate text-stone-500">
                        {d.filename}
                        {d.date && ` · ${formatDate(d.date)}`} ·{' '}
                        {plural(facts.length, 'dato leído', 'datos leídos')}
                      </span>
                    </span>
                  </button>
                  <div className="flex items-center gap-3">
                    <button
                      type="button"
                      onClick={() => companyDocOpen(d.id).catch((e) => setMsg(errorMessage(e)))}
                    >
                      Abrir
                    </button>
                    <button
                      type="button"
                      className="flex items-center gap-1"
                      disabled={busy === d.id}
                      onClick={async () => {
                        setBusy(d.id);
                        try {
                          await companyDocAi(d.id);
                          setMsg(`${d.title}: completado con el asistente. Revisa los datos.`);
                          load();
                        } catch (e) {
                          setMsg(errorMessage(e));
                        } finally {
                          setBusy(null);
                        }
                      }}
                    >
                      <Sparkles size={14} aria-hidden="true" />
                      {busy === d.id ? 'Leyendo…' : 'Completar con el asistente'}
                    </button>
                    <ConfirmAction
                      label="Quitar"
                      question={`¿Quitar «${d.filename}» de los documentos de la empresa?`}
                      confirmLabel="Sí, quitar"
                      onConfirm={async () => {
                        await companyDocDelete(d.id);
                        load();
                      }}
                    />
                  </div>
                </div>
                {open === d.id && (
                  <dl className="mt-3 grid gap-x-6 gap-y-1 rounded-lg bg-stone-50 p-3 sm:grid-cols-2">
                    {facts.length === 0 && (
                      <p className="text-stone-500">No se han leído datos de este documento.</p>
                    )}
                    {facts.map(([k, label]) => (
                      <div key={k} className="flex justify-between gap-3">
                        <dt className="text-stone-500">{label}</dt>
                        <dd className="text-right font-medium">
                          {factValue(k, d.facts[k] as string | number)}
                        </dd>
                      </div>
                    ))}
                  </dl>
                )}
              </li>
            );
          })}
        </ul>
      </section>
    </div>
  );
}

/** Libros importados (y facturas leídas con IA): cada importación se puede deshacer entera. */
function BookImports() {
  const [list, setList] = useState<BookBatch[] | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(() => {
    folderBookImports()
      .then(setList)
      .catch(() => setList([]));
  }, []);
  useEffect(load, [load]);
  if (!list || list.length === 0) return null;
  return (
    <section aria-labelledby="book-imports" className="card mt-5 p-5 text-sm">
      <h3 id="book-imports" className="font-semibold">
        Facturas importadas
      </h3>
      <p className="mt-1 text-stone-600">
        Cada libro (o lote de facturas leídas con IA) se puede deshacer entero si algo salió mal. No
        se deshace si alguna factura ya tiene cobros, pagos o una rectificativa.
      </p>
      <ul className="mt-3 divide-y divide-stone-100">
        {list.map((b) => (
          <li key={b.id} className="flex flex-wrap items-center justify-between gap-3 py-2">
            <span className="min-w-0">
              <span className="block truncate font-medium">{b.filename}</span>
              <span className="text-stone-500">
                {b.source === 'lectura' ? 'Leídas con IA' : 'Libro'} de {b.kind} ·{' '}
                {plural(b.invoices, 'factura')} ·{' '}
                {new Date(b.created_at).toLocaleDateString('es-ES')}
              </span>
            </span>
            <ConfirmAction
              label="Deshacer"
              question={`¿Quitar las ${plural(b.invoices, 'factura')} de «${b.filename}»?`}
              confirmLabel="Sí, quitarlas"
              onConfirm={async () => {
                const n = await folderBookUndo(b.id);
                setMsg(`${plural(n, 'factura quitada', 'facturas quitadas')}.`);
                load();
              }}
            />
          </li>
        ))}
      </ul>
      {msg && (
        <p role="status" className="mt-2 text-stone-700">
          {msg}
        </p>
      )}
    </section>
  );
}

export function Importar({ onNavigate }: { onNavigate: (t: ImportTarget) => void }) {
  const [view, setView] = useState<'carpeta' | 'documentos' | 'comparar' | 'revision'>('carpeta');
  const [applied, setApplied] = useState(0);
  return (
    <section className="mt-5" aria-labelledby="importar-title">
      <h2 id="importar-title" className="sr-only">
        Importar
      </h2>
      <nav aria-label="Importar, comparar o revisar">
        <button
          type="button"
          aria-current={view === 'carpeta' ? 'page' : undefined}
          onClick={() => setView('carpeta')}
        >
          Carpeta de la empresa
        </button>
        <button
          type="button"
          aria-current={view === 'documentos' ? 'page' : undefined}
          onClick={() => setView('documentos')}
        >
          Documentos y datos
        </button>
        <button
          type="button"
          aria-current={view === 'comparar' ? 'page' : undefined}
          onClick={() => setView('comparar')}
        >
          Comparar con la gestoría
        </button>
        <button
          type="button"
          aria-current={view === 'revision' ? 'page' : undefined}
          onClick={() => setView('revision')}
        >
          Revisión
        </button>
      </nav>
      {view === 'carpeta' && (
        <>
          <FolderImport onNavigate={onNavigate} onApplied={() => setApplied((n) => n + 1)} />
          <BookImports key={applied} />
        </>
      )}
      {view === 'documentos' && <CompanyDocuments />}
      {view === 'comparar' && <CompareWithAdvisor />}
      {view === 'revision' && <FolderReview />}
    </section>
  );
}

import { MessageSquare, Sparkles, X } from 'lucide-react';
import {
  Fragment,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import { AI_NOTICE_SHORT, AssistantSetup, providerLabel, ReplyNotes } from './AssistantParts';
import { useAnnounce } from './announce';
import {
  type AssistantStatus,
  type AssistantTopic,
  assistantConversationDelete,
  assistantConversationOpen,
  assistantConversations,
  assistantFactAdd,
  assistantFileAdd,
  assistantMaterialDelete,
  assistantStatus,
  assistantTopicAsk,
  assistantTopicOpen,
  assistantTopicPreview,
  type Conversation,
  type ConversationMaterial,
  type ConversationSummary,
  errorMessage,
  pickAssistantFile,
  type StoredMessage,
  type TopicOpened,
  type TopicOutgoing,
  topicKey,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { LayersContext, useInlineDialog, useReturnFocus } from './dialogs';
import { Markdown } from './Markdown';

/** De qué se habla: el tema, su título (mientras carga) y la conversación guardada, si se sabe. */
export type AssistantTarget = {
  topic: AssistantTopic;
  title: string;
  conversationId: number | null;
};

/**
 * Preguntas que esperan su respuesta. Sobreviven a cerrar el panel o cambiar de pantalla: al
 * volver a abrirlo se ve la pregunta con «Pensando…» y se espera la misma respuesta (que se
 * guarda en la conversación aunque el panel esté cerrado).
 */
const pending = new Map<string, { question: string; promise: Promise<TopicOpened> }>();

/**
 * Preguntas que han fallado sin nadie mirando (el panel se cerró mientras pensaba): al volver a
 * abrirlo, la pregunta vuelve al cuadro con su error. No se guardan en ningún sitio más.
 */
const failed = new Map<string, { question: string; error: string }>();

function pendingKey(topic: AssistantTopic, conversationId: number | null): string {
  return topic.kind === 'general' ? `general#${conversationId ?? 'nueva'}` : topicKey(topic);
}

/**
 * Quien se entera de las preguntas que empiezan o dejan de esperar (con la conversación ya
 * guardada, si ha llegado la respuesta) y de lo que se termina de añadir con el panel ya cerrado.
 */
type Watcher = (saved: Conversation | null) => void;
const watchers = new Set<Watcher>();
function tell(saved: Conversation | null = null) {
  for (const w of watchers) w(saved);
}

/**
 * Las preguntas que esperan su respuesta, de cualquier panel o pantalla: la pantalla se vuelve a
 * pintar cuando empiezan o terminan. `onSaved` llega con cada conversación que se guarda al
 * llegar su respuesta, también si se preguntó en un panel ya cerrado o en la pantalla de antes, y
 * con la que guarda un dato o un documento que se termina de añadir con el panel ya cerrado (un
 * PDF escaneado tarda en leerse: puede crear la conversación).
 */
export function usePendingQuestions(onSaved?: (c: Conversation) => void): {
  waiting: (topic: AssistantTopic, conversationId: number | null) => boolean;
  any: boolean;
} {
  const [, setTick] = useState(0);
  const latest = useRef(onSaved);
  latest.current = onSaved;
  useEffect(() => {
    const w: Watcher = (c) => {
      setTick((t) => t + 1);
      if (c) latest.current?.(c);
    };
    watchers.add(w);
    return () => {
      watchers.delete(w);
    };
  }, []);
  return {
    waiting: (topic, conversationId) => pending.has(pendingKey(topic, conversationId)),
    any: pending.size > 0,
  };
}

/** El resumen de una conversación, como lo da la lista de guardadas. */
export function summaryOf(c: Conversation): ConversationSummary {
  return {
    id: c.id,
    topic: c.topic,
    title: c.title,
    updated_at: c.updated_at,
    questions: c.messages.filter((m) => m.role === 'user').length,
    materials: c.materials.length,
  };
}

const MESES = [
  'enero',
  'febrero',
  'marzo',
  'abril',
  'mayo',
  'junio',
  'julio',
  'agosto',
  'septiembre',
  'octubre',
  'noviembre',
  'diciembre',
];

/** «Hoy», «Ayer» o «31 de octubre de 2026» (en la hora del Mac). */
export function dayLabel(iso: string, now = new Date()): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  const day = (x: Date) => `${x.getFullYear()}-${x.getMonth()}-${x.getDate()}`;
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (day(d) === day(now)) return 'Hoy';
  if (day(d) === day(yesterday)) return 'Ayer';
  return `${d.getDate()} de ${MESES[d.getMonth()]} de ${d.getFullYear()}`;
}

/** Las primeras palabras de un dato (para el botón «Quitar»). */
function firstWords(t: string): string {
  const words = t.trim().split(/\s+/);
  return words.length > 6 ? `${words.slice(0, 6).join(' ')}…` : words.join(' ');
}

/** «Antes de enviar se sustituyeron 2 datos personales…» (o «se sustituyó 1 dato personal…»). */
function replacedNote(n: number): string {
  return n === 1
    ? 'Antes de enviar se sustituyó 1 dato personal por un marcador.'
    : `Antes de enviar se sustituyeron ${n} datos personales por marcadores.`;
}

/** «2 datos sustituidos» (o «1 dato sustituido»). */
function replacedCount(n: number): string {
  return n === 1 ? '1 dato sustituido' : `${n} datos sustituidos`;
}

/** Una pregunta que falló vuelve al cuadro: delante de lo que se haya escrito mientras. */
function withQuestion(text: string, question: string): string {
  return text.trim() && text.trim() !== question ? `${question}\n\n${text}` : question;
}

const CHANGED =
  'Los datos de este tema han cambiado desde tu última pregunta. La IA usará los de ahora.';
const SLOW = {
  panel: 'Sigue pensando… Puedes cerrar el panel: la respuesta se guardará en la conversación.',
  page: 'Sigue pensando… Puedes ir a otra pantalla: la respuesta se guardará en la conversación.',
};
const FULL =
  'Esta conversación ha llegado a 100 preguntas: bórrala para empezar otra (copia antes lo que quieras guardar).';

/** Botón que abre la conversación con la IA de un tema («Seguir con la IA» si ya tiene). */
export function AskAiButton({
  what,
  questions,
  about,
  className = 'btn-ghost',
  onOpen,
}: {
  /** De qué (para el lector de pantalla): el título de la recomendación… */
  what: string;
  questions: number;
  /** Lo que sigue en el botón a «Preguntar a la IA» («sobre este borrador»). */
  about?: string;
  className?: string;
  onOpen: (b: HTMLButtonElement) => void;
}) {
  const visible = `${questions > 0 ? 'Seguir con la IA' : 'Preguntar a la IA'}${about ? ` ${about}` : ''}`;
  // El nombre entero va en aria-label: un texto oculto al lado del visible no se lee igual en
  // todos los navegadores («Preguntar a la IA : …»).
  const label =
    questions > 0
      ? `${visible}: ${what}, ${questions} ${questions === 1 ? 'pregunta' : 'preguntas'}`
      : `${visible}: ${what}`;
  return (
    <button
      type="button"
      className={className}
      aria-label={label}
      onClick={(e) => onOpen(e.currentTarget)}
    >
      {questions > 0 ? (
        <>
          <MessageSquare size={16} aria-hidden="true" />
          {visible}
          <span className="count" aria-hidden="true">
            {questions}
          </span>
        </>
      ) : (
        <>
          <Sparkles size={16} aria-hidden="true" />
          {visible}
        </>
      )}
    </button>
  );
}

/**
 * El panel de la conversación de una pantalla: `open(objetivo, botón)` lo abre y `element` se
 * pinta como último hijo de la página (dentro de `<main>`, con sus estilos). Al cerrarlo, el foco
 * vuelve al botón. `onChange` llega con el resumen nuevo (o `null` si se ha borrado), también si
 * la respuesta llega con el panel ya cerrado.
 */
export function useAssistantPanel(
  onChange?: (s: ConversationSummary | null, t: AssistantTarget) => void,
): { open: (t: AssistantTarget, from: HTMLButtonElement) => void; element: ReactNode } {
  const [target, setTarget] = useState<AssistantTarget | null>(null);
  const opener = useRef<HTMLButtonElement | null>(null);
  useReturnFocus(target !== null, opener);
  const open = (t: AssistantTarget, from: HTMLButtonElement) => {
    opener.current = from;
    setTarget(t);
  };
  const element = target ? (
    <AssistantPanel
      key={pendingKey(target.topic, target.conversationId)}
      target={target}
      onClose={() => setTarget(null)}
      onChange={(s) => onChange?.(s, target)}
    />
  ) : null;
  return { open, element };
}

/**
 * «Preguntar a la IA» en una pantalla con temas: cuántas preguntas tiene la conversación de cada
 * tema (para «Seguir con la IA») y el panel. Las conversaciones se leen al abrir la pantalla y
 * cada vez que se guarda una respuesta (también la de un panel que se cerró antes de cambiar de
 * pantalla); si no se puede, los botones dicen «Preguntar a la IA».
 */
export function useAskAi(): {
  questions: (t: AssistantTopic) => number;
  open: (t: AssistantTarget, from: HTMLButtonElement) => void;
  element: ReactNode;
} {
  const [convs, setConvs] = useState<ConversationSummary[]>([]);
  // Sólo vale la última lectura: una anterior puede llegar después con lo de antes.
  const reads = useRef(0);
  const load = useCallback(() => {
    const n = ++reads.current;
    assistantConversations()
      .then((cs) => {
        if (n === reads.current) setConvs(cs);
      })
      .catch(() => undefined);
  }, []);
  useEffect(() => load(), [load]);
  usePendingQuestions(load);
  const { open, element } = useAssistantPanel((s, t) =>
    setConvs((cs) => {
      const k = topicKey(t.topic);
      const rest = cs.filter((c) => topicKey(c.topic) !== k);
      return s ? [s, ...rest] : rest;
    }),
  );
  const questions = (t: AssistantTopic) =>
    convs.find((c) => topicKey(c.topic) === topicKey(t))?.questions ?? 0;
  return { questions, open, element };
}

/** Lo que puede tener el foco dentro del panel. */
const FOCUSABLE = 'textarea, input, select, button:not([disabled]), summary, [tabindex="0"]';

/**
 * Sin `showModal` (macOS anteriores a 12.3, jsdom), el panel hace de diálogo modal a mano: lo de
 * detrás no se lee ni se usa (`aria-hidden` e `inert`), el foco no sale del panel y Esc lo
 * cierra esté donde esté el foco. Devuelve cómo deshacerlo.
 */
function modalFallback(d: HTMLElement, closeTop: () => void): () => void {
  d.setAttribute('aria-modal', 'true');
  const changed: [Element, string | null, boolean][] = [];
  for (let el: Element | null = d; el && el !== document.body; el = el.parentElement) {
    for (const sib of Array.from(el.parentElement?.children ?? [])) {
      if (sib === el || sib.classList.contains('ai-panel-backdrop')) continue;
      changed.push([sib, sib.getAttribute('aria-hidden'), sib.hasAttribute('inert')]);
      sib.setAttribute('aria-hidden', 'true');
      sib.setAttribute('inert', '');
    }
  }
  let last: HTMLElement | null = null;
  const keep = (e: FocusEvent) => {
    const t = e.target;
    if (!(t instanceof HTMLElement)) return;
    if (d.contains(t)) {
      last = t;
      return;
    }
    (last?.isConnected ? last : d.querySelector<HTMLElement>(FOCUSABLE))?.focus();
  };
  const key = (e: globalThis.KeyboardEvent) => {
    // Lo de dentro del panel ya lo ha atendido (y marcado) el propio panel.
    if (e.key === 'Escape' && !e.defaultPrevented) {
      e.preventDefault();
      closeTop();
    }
  };
  document.addEventListener('focusin', keep);
  document.addEventListener('keydown', key);
  return () => {
    document.removeEventListener('focusin', keep);
    document.removeEventListener('keydown', key);
    for (const [el, hidden, inert] of changed) {
      if (hidden === null) el.removeAttribute('aria-hidden');
      else el.setAttribute('aria-hidden', hidden);
      if (!inert) el.removeAttribute('inert');
    }
  };
}

/** La conversación en un diálogo modal a la derecha. */
export function AssistantPanel({
  target,
  onClose,
  onChange,
}: {
  target: AssistantTarget;
  onClose: () => void;
  onChange?: (s: ConversationSummary | null) => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const noticeId = useId();
  const layers = useRef<(() => void)[]>([]);
  const [fallback, setFallback] = useState(false);
  // Esc (u otra petición de cerrar) cierra antes la capa de dentro que se abrió la última (la
  // confirmación de borrar, «Añadir un dato»), esté donde esté el foco; sin ninguna, el panel.
  const closeTop = () => {
    const top = layers.current.at(-1);
    if (top) top();
    else onClose();
  };
  const latestClose = useRef(closeTop);
  latestClose.current = closeTop;
  useLayoutEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (typeof d.showModal === 'function') {
      if (!d.open) d.showModal();
      return;
    }
    d.setAttribute('open', '');
    d.setAttribute('data-fallback', '');
    setFallback(true);
    return modalFallback(d, () => latestClose.current());
  }, []);
  return (
    <LayersContext.Provider value={layers}>
      {fallback && <div className="ai-panel-backdrop" aria-hidden="true" onMouseDown={onClose} />}
      <dialog
        ref={ref}
        className="ai-panel"
        aria-labelledby={titleId}
        aria-describedby={noticeId}
        onCancel={(e) => {
          e.preventDefault();
          closeTop();
        }}
        onKeyDown={(e: KeyboardEvent) => {
          // Con el foco dentro de una capa (confirmación, «Añadir un dato»), ella se cierra antes
          // y no llega aquí.
          if (e.key === 'Escape') {
            e.preventDefault();
            closeTop();
          }
        }}
        onMouseDown={(e) => {
          // Sólo un clic en el fondo, fuera del panel (no en su barra de desplazamiento).
          if (e.target !== e.currentTarget) return;
          const r = e.currentTarget.getBoundingClientRect();
          if (
            e.clientX < r.left ||
            e.clientX > r.right ||
            e.clientY < r.top ||
            e.clientY > r.bottom
          )
            onClose();
        }}
      >
        <ConversationView
          target={target}
          layout="panel"
          titleId={titleId}
          noticeId={noticeId}
          onClose={onClose}
          {...(onChange ? { onChange } : {})}
        />
      </dialog>
    </LayersContext.Provider>
  );
}

/** «Añadir un dato»: se cierra con Esc (sin cerrar el panel) y añade con ⌘+Intro. */
function FactForm({
  busy,
  blocked,
  error,
  onAdd,
  onCancel,
}: {
  busy: boolean;
  /** Ahora no se puede añadir (se espera la primera respuesta), aunque sí cerrar. */
  blocked: boolean;
  error: string | null;
  onAdd: (text: string) => void;
  onCancel: () => void;
}) {
  const [text, setText] = useState('');
  const { ref, onKeyDown } = useInlineDialog(true, busy, onCancel);
  const box = useRef<HTMLTextAreaElement>(null);
  const id = useId();
  const helpId = useId();
  // El foco, al cuadro (el diálogo en línea lo pondría en el primer botón).
  useEffect(() => {
    box.current?.focus();
  }, []);
  const add = () => {
    if (!busy && !blocked && text.trim()) onAdd(text);
  };
  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: Esc cierra esta capa
    <div
      ref={ref}
      onKeyDown={onKeyDown}
      className="space-y-2 rounded-md border border-stone-200 bg-stone-50 p-3"
    >
      <label htmlFor={id} className="block text-sm font-medium">
        Dato para la IA
      </label>
      <textarea
        id={id}
        ref={box}
        className="min-h-16 w-full rounded border border-stone-300 px-2 py-1"
        aria-describedby={helpId}
        placeholder="p. ej.: En 2025 dejamos 30.000 € del beneficio en reservas voluntarias."
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            add();
          }
        }}
      />
      <p id={helpId} className="text-xs text-stone-600">
        Se guarda en esta conversación y va con cada pregunta, anonimizado. TaxPain no lo comprueba.
        Con ⌘ + Intro se añade.
      </p>
      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <button
          type="button"
          className="btn-primary aria-disabled:opacity-50"
          aria-disabled={busy || blocked || !text.trim() || undefined}
          onClick={add}
        >
          Añadir
        </button>
        <button type="button" className="border" onClick={onCancel}>
          Cancelar
        </button>
      </div>
    </div>
  );
}

/** Lo que ha añadido el usuario, con «Quitar» en cada cosa. */
function Materials({
  materials,
  open,
  onToggle,
  summaryRef,
  onRemove,
}: {
  materials: ConversationMaterial[];
  open: boolean;
  onToggle: (open: boolean) => void;
  summaryRef: RefObject<HTMLElement | null>;
  onRemove: (m: ConversationMaterial) => void;
}) {
  return (
    <details open={open} onToggle={(e) => onToggle(e.currentTarget.open)}>
      <summary ref={summaryRef} className="text-sm">
        Lo que has añadido tú ({materials.length})
      </summary>
      {materials.length === 0 ? (
        <p className="mt-1 text-sm text-stone-600">
          Nada todavía. Añade cifras, decisiones o documentos que TaxPain no tenga.
        </p>
      ) : (
        <ul className="mt-2 max-h-[25vh] space-y-2 overflow-y-auto text-sm">
          {materials.map((m) => (
            <li key={m.id} className="rounded-md border border-stone-200 p-2">
              {m.kind === 'dato' ? (
                <p className="whitespace-pre-wrap">{m.content}</p>
              ) : (
                <>
                  <p className="font-medium break-all">{m.filename}</p>
                  <p className="text-xs text-stone-600">
                    {m.read_by === 'ia' ? 'leído por la IA' : 'leído en tu Mac'} ·{' '}
                    {m.chars.toLocaleString('es-ES')} caracteres
                    {m.truncated ? ' · recortado: no va entero' : ''}
                  </p>
                  <details className="mt-1">
                    <summary className="text-xs" aria-label={`Ver el texto: ${m.filename}`}>
                      Ver el texto
                    </summary>
                    <pre className="mt-1 whitespace-pre-wrap text-xs">{m.content}</pre>
                  </details>
                </>
              )}
              <button
                type="button"
                className="text-link text-xs"
                data-quitar={m.id}
                aria-label={`Quitar: ${m.kind === 'dato' ? firstWords(m.content) : (m.filename ?? '')}`}
                onClick={() => onRemove(m)}
              >
                Quitar
              </button>
            </li>
          ))}
        </ul>
      )}
    </details>
  );
}

/** Lo que se envía, exactamente (anonimizado). */
function Preview({ p }: { p: TopicOutgoing }) {
  return (
    <section
      aria-label="Lo que se envía"
      className="max-h-[40vh] overflow-y-auto rounded-md border border-stone-200 bg-stone-50 p-3 text-xs"
    >
      <details open>
        <summary className="font-medium">
          Esto es exactamente lo que se envía ({replacedCount(p.replaced)})
        </summary>
        <pre className="mt-2 whitespace-pre-wrap">{p.system}</pre>
        {p.messages.map((m, i) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: vista estática
          <pre key={i} className="mt-2 whitespace-pre-wrap">
            {m.role === 'user' ? 'Tú' : 'IA'}: {m.content}
          </pre>
        ))}
        {p.omitted > 0 && (
          <p className="mt-2">
            {p.omitted === 1
              ? 'No va la intervención más antigua de esta conversación: se queda en tu Mac.'
              : `No van las ${p.omitted} intervenciones más antiguas de esta conversación: se quedan en tu Mac.`}{' '}
            Lo que has añadido va siempre.
          </p>
        )}
        <p className="mt-2">
          Además, la IA puede consultar, sólo leyendo, tus plazos, borradores de modelos, facturas,
          saldos y el BOE.
        </p>
      </details>
    </section>
  );
}

/**
 * Un mensaje del registro: la pregunta, la respuesta (con sus notas) o la que espera
 * (`pending`). La que espera es el mismo elemento que la guardada, con la misma clave: al llegar
 * la respuesta, el lector de pantalla sólo lee lo nuevo.
 */
function MessageItem({
  m,
  pending = false,
  onCopy,
}: {
  m: StoredMessage;
  pending?: boolean;
  onCopy: (t: string) => void;
}) {
  const mine = m.role === 'user';
  return (
    <li className={mine ? 'ml-10' : 'mr-6'}>
      <p className={`text-xs font-medium text-stone-600 ${mine ? 'text-right' : ''}`}>
        {mine ? 'Tú' : 'IA'}
      </p>
      <div
        className={`rounded-lg p-3 text-sm ${
          mine ? 'bubble-me whitespace-pre-wrap' : 'border border-stone-200 bg-white'
        }`}
      >
        {mine ? m.content : <Markdown text={m.content} />}
        {!mine && m.notes && <ReplyNotes r={m.notes} />}
      </div>
      {pending && (
        <p aria-hidden="true" className="mt-2 text-sm text-stone-600">
          Pensando…
        </p>
      )}
      {!mine && (
        <div className="mt-1 flex flex-wrap items-center gap-x-3 text-xs text-stone-500">
          {m.notes && m.notes.replaced > 0 && <span>{replacedNote(m.notes.replaced)}</span>}
          <button type="button" className="text-link" onClick={() => onCopy(m.content)}>
            Copiar la respuesta
          </button>
        </div>
      )}
    </li>
  );
}

/**
 * Una conversación con la IA: cabecera (de qué va, lo que sabe TaxPain y lo añadido), registro
 * y cuadro para preguntar. En el panel (`layout="panel"`) y en la página del Asistente.
 */
export function ConversationView({
  target,
  layout,
  titleId,
  noticeId: noticeIdProp,
  onChange,
  onCreated,
  onClose,
}: {
  target: AssistantTarget;
  layout: 'panel' | 'page';
  titleId: string;
  noticeId?: string;
  onChange?: (s: ConversationSummary | null) => void;
  /** Una general nueva ya tiene id (tras su primera respuesta o al añadirle algo). */
  onCreated?: (id: number) => void;
  onClose?: () => void;
}) {
  const ownNoticeId = useId();
  const noticeId = noticeIdProp ?? ownNoticeId;
  const boxId = useId();
  const helpId = useId();
  const docHelpId = useId();
  const [status, setStatus] = useState<AssistantStatus | null>(null);
  const [opened, setOpened] = useState<TopicOpened | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [text, setText] = useState('');
  const [waiting, setWaiting] = useState<string | null>(null);
  const [slow, setSlow] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [announce, setAnnounce] = useAnnounce();
  const [factOpen, setFactOpen] = useState(false);
  const [factBusy, setFactBusy] = useState(false);
  const [factError, setFactError] = useState<string | null>(null);
  const [reading, setReading] = useState<string | null>(null);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [preview, setPreview] = useState<TopicOutgoing | null>(null);
  const [materialsOpen, setMaterialsOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const log = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLTextAreaElement>(null);
  const factBtn = useRef<HTMLButtonElement>(null);
  const materialsSummary = useRef<HTMLElement>(null);
  const alive = useRef(true);
  // La respuesta ha llegado antes que la carga: lo cargado ya es viejo.
  const answered = useRef(false);
  // «Ver qué se envía»: sólo vale lo que se pidió la última vez.
  const previews = useRef(0);
  useReturnFocus(factOpen, factBtn);

  const conversation = opened?.conversation ?? null;
  const conversationId = conversation?.id ?? target.conversationId;
  // El id de ahora, para lo que se termina después de esperar (el diálogo de elegir fichero).
  const currentId = useRef(conversationId);
  currentId.current = conversationId;
  const setup = status?.remote === true && (!status.settings.consent || !status.has_api_key);
  // Una general nueva aún no tiene id: hasta que se guarde lo primero (su primera respuesta o lo
  // primero que se añade), no se hace otra cosa a la vez; si no, acabarían en dos conversaciones
  // (las de un tema se encuentran por el tema).
  const unsaved = target.topic.kind === 'general' && conversationId === null;
  const addBlocked = unsaved && (waiting !== null || factBusy || reading !== null);
  const askBlocked = unsaved && (factBusy || reading !== null);

  /** Lo que enseña «Ver qué se envía» ya no es lo que se enviaría: se cierra. */
  const closePreview = () => {
    previews.current += 1;
    setPreviewOpen(false);
    setPreview(null);
  };

  const update = (c: Conversation) => {
    setOpened((o) => (o ? { ...o, conversation: c } : o));
    if (c.topic.kind === 'general' && target.conversationId === null) onCreated?.(c.id);
    onChange?.(summaryOf(c));
  };

  /** Sigue una pregunta en marcha (de este panel o de uno que se cerró). */
  const follow = (promise: Promise<TopicOpened>, question: string, key: string) => {
    promise
      .then((o) => {
        if (!alive.current) return;
        answered.current = true;
        setOpened(o);
        setWaiting(null);
        // Lo que enseñaba «Ver qué se envía» no lleva esta pregunta ni su respuesta.
        closePreview();
        if (o.conversation) {
          // También si la preguntó un panel que ya se cerró: la página se entera.
          onChange?.(summaryOf(o.conversation));
          if (o.conversation.topic.kind === 'general' && target.conversationId === null)
            onCreated?.(o.conversation.id);
        }
      })
      .catch((e) => {
        // Sin nadie mirando, se queda en `failed` hasta que se vuelva a abrir.
        if (!alive.current) return;
        failed.delete(key);
        setWaiting(null);
        setText((t) => withQuestion(t, question));
        closePreview();
        setError(errorMessage(e));
      });
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: sólo al abrir
  useEffect(() => {
    alive.current = true;
    const key = pendingKey(target.topic, target.conversationId);
    // La pregunta que falló con el panel cerrado vuelve al cuadro, con su error.
    const f = failed.get(key);
    if (f) {
      failed.delete(key);
      setText(f.question);
      setError(f.error);
    }
    const p = pending.get(key);
    if (p) {
      setWaiting(p.question);
      follow(p.promise, p.question, key);
    }
    Promise.all([
      assistantStatus(),
      target.conversationId !== null
        ? assistantConversationOpen(target.conversationId)
        : assistantTopicOpen(target.topic),
    ])
      .then(([s, o]) => {
        if (!alive.current) return;
        setStatus(s);
        if (!answered.current) setOpened(o);
      })
      .catch((e) => {
        if (alive.current) setLoadError(errorMessage(e));
      });
    return () => {
      alive.current = false;
    };
  }, []);

  // En el panel, el foco va a «Tu pregunta» (o al primer campo de los ajustes) al cargar.
  const loaded = status !== null && opened !== null;
  // biome-ignore lint/correctness/useExhaustiveDependencies: sólo al terminar de cargar
  useEffect(() => {
    if (!loaded || layout !== 'panel') return;
    (box.current ?? root.current?.querySelector<HTMLElement>('form select, form input'))?.focus();
  }, [loaded]);
  // Al guardar los ajustes, el formulario (con el foco en «Guardar») deja paso al cuadro.
  const wasSetup = useRef(false);
  useEffect(() => {
    if (wasSetup.current && !setup) box.current?.focus();
    wasSetup.current = setup;
  }, [setup]);

  // El registro baja hasta lo último (al abrir, al preguntar y al llegar la respuesta).
  const count = opened?.conversation?.messages.length ?? 0;
  // biome-ignore lint/correctness/useExhaustiveDependencies: se dispara al cambiar la conversación
  useEffect(() => {
    const l = log.current;
    if (l) l.scrollTop = l.scrollHeight;
  }, [count, waiting, loaded]);

  useEffect(() => {
    if (waiting === null) {
      setSlow(false);
      return;
    }
    const t = setTimeout(() => setSlow(true), 20_000);
    return () => clearTimeout(t);
  }, [waiting]);

  const send = () => {
    const q = text.trim();
    if (!q || waiting !== null || askBlocked) return;
    const key = pendingKey(target.topic, conversationId);
    failed.delete(key);
    // Quien la siga (este panel u otro que se abra) se entera después de esto: si falla con el
    // panel cerrado, ya está en `failed`.
    const promise = assistantTopicAsk(target.topic, conversationId, q)
      .catch((e: unknown) => {
        failed.set(key, { question: q, error: errorMessage(e) });
        throw e;
      })
      .finally(() => pending.delete(key));
    pending.set(key, { question: q, promise });
    tell();
    // Aunque se cierre el panel, la página se entera (el botón pasa a «Seguir con la IA»), y
    // también las pantallas que miran lo que espera (otra instancia de la página, el Asistente).
    promise
      .then((o) => {
        if (o.conversation) onChange?.(summaryOf(o.conversation));
        tell(o.conversation);
      })
      .catch(() => tell());
    setWaiting(q);
    setText('');
    setError(null);
    setAnnounce('');
    closePreview();
    follow(promise, q, key);
    box.current?.focus();
  };

  const addFact = (t: string) => {
    if (addBlocked) return;
    setFactBusy(true);
    setFactError(null);
    assistantFactAdd(target.topic, conversationId, t)
      .then((c) => {
        // Con el panel ya cerrado, que se enteren las pantallas que miran (la lista del Asistente).
        if (!alive.current) {
          tell(c);
          return;
        }
        update(c);
        closePreview();
        setFactOpen(false);
        setMaterialsOpen(true);
        setAnnounce('Dato añadido. Va con la próxima pregunta.');
      })
      .catch((e) => alive.current && setFactError(errorMessage(e)))
      .finally(() => alive.current && setFactBusy(false));
  };

  const addFile = async () => {
    if (reading !== null || addBlocked) return;
    setError(null);
    let path: string | null;
    try {
      path = await pickAssistantFile();
    } catch (e) {
      setError(errorMessage(e));
      return;
    }
    if (!path) return;
    const name = path.split(/[\\/]/).pop() ?? path;
    setReading(name);
    setAnnounce(`Leyendo «${name}» en tu Mac…`);
    try {
      // La de ahora: mientras se elegía el fichero ha podido guardarse lo primero de una nueva.
      const c = await assistantFileAdd(target.topic, currentId.current, path);
      // Con el panel ya cerrado, que se enteren las pantallas que miran (la lista del Asistente).
      if (!alive.current) {
        tell(c);
        return;
      }
      update(c);
      closePreview();
      setMaterialsOpen(true);
      setAnnounce(`Documento añadido: ${name}.`);
    } catch (e) {
      if (!alive.current) return;
      setAnnounce('');
      setError(errorMessage(e));
    } finally {
      if (alive.current) setReading(null);
    }
  };

  const remove = (m: ConversationMaterial) => {
    if (!conversation) return;
    const list = conversation.materials;
    const i = list.findIndex((x) => x.id === m.id);
    const next = list[i + 1] ?? list[i - 1];
    assistantMaterialDelete(conversation.id, m.id)
      .then((c) => {
        if (!alive.current) return;
        update(c);
        closePreview();
        setAnnounce(m.kind === 'dato' ? 'Dato quitado.' : `Documento quitado: ${m.filename}.`);
        // El foco, al siguiente «Quitar» (o a la lista, si ya no queda nada).
        requestAnimationFrame(() => {
          const b = next
            ? root.current?.querySelector<HTMLElement>(`[data-quitar="${next.id}"]`)
            : null;
          (b ?? materialsSummary.current)?.focus();
        });
      })
      .catch((e) => alive.current && setError(errorMessage(e)));
  };

  const togglePreview = () => {
    if (previewOpen) {
      closePreview();
      return;
    }
    const n = ++previews.current;
    setPreviewOpen(true);
    setPreview(null);
    assistantTopicPreview(target.topic, conversationId, text.trim() || null)
      .then((p) => {
        if (alive.current && n === previews.current) setPreview(p);
      })
      .catch((e) => {
        if (!alive.current || n !== previews.current) return;
        setPreviewOpen(false);
        setError(errorMessage(e));
      });
  };

  /** Otra pregunta en el cuadro: lo que enseñaba «Ver qué se envía» ya no vale. */
  const changeText = (t: string) => {
    setText(t);
    if (previewOpen) closePreview();
  };

  const copy = (t: string) => {
    const clip = navigator.clipboard;
    if (!clip) {
      setAnnounce('No se ha podido copiar.');
      return;
    }
    clip
      .writeText(t)
      .then(() => setAnnounce('Copiada.'))
      .catch(() => setAnnounce('No se ha podido copiar.'));
  };

  const header = opened?.header;
  const Title = layout === 'panel' ? 'h2' : 'h3';
  const messages = conversation?.messages ?? [];
  // La pregunta que espera es el mismo elemento, con la misma clave, que tendrá al guardarse (y
  // los días van por su nombre): al llegar la respuesta, el registro sólo lee la respuesta.
  const nextSeq = (messages[messages.length - 1]?.seq ?? 0) + 1;
  const items: ReactNode[] = [];
  let lastDay = '';
  const dayItem = (day: string) => {
    items.push(
      <li key={`d-${day}`} className="text-center text-xs text-stone-500">
        {day}
      </li>,
    );
    lastDay = day;
  };
  for (const m of messages) {
    const day = dayLabel(m.created_at);
    if (day && day !== lastDay) dayItem(day);
    items.push(<MessageItem key={`m-${m.seq}`} m={m} onCopy={copy} />);
  }
  if (waiting !== null) {
    if (lastDay !== 'Hoy') dayItem('Hoy');
    items.push(
      <MessageItem
        key={`m-${nextSeq}`}
        m={{ seq: nextSeq, role: 'user', content: waiting, created_at: '', notes: null }}
        pending
        onCopy={copy}
      />,
    );
  }
  const slowText = SLOW[layout];
  const statusText = waiting !== null ? (slow ? slowText : 'Pensando…') : announce;

  return (
    <div
      ref={root}
      className={`flex flex-col ${layout === 'panel' ? 'h-full' : 'min-h-[28rem] rounded-lg border border-stone-200'}`}
    >
      <header className="shrink-0 space-y-2 border-b border-stone-200 p-4">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <p className="text-xs font-semibold uppercase tracking-wide text-brand-700">
              {header?.group ?? (target.topic.kind === 'general' ? 'Asistente' : '')}
            </p>
            <Title id={titleId} className="mt-0.5 text-base font-semibold">
              <span className="sr-only">Conversación con la IA:</span>{' '}
              {header?.title ?? target.title}
            </Title>
          </div>
          {layout === 'panel' && onClose && (
            // El «esc» va fuera del botón: su nombre («Cerrar») tiene que llevar lo que se ve.
            <div className="flex shrink-0 items-center gap-1">
              <span aria-hidden="true" className="text-xs text-stone-500">
                esc
              </span>
              <button type="button" aria-label="Cerrar" className="btn-ghost" onClick={onClose}>
                <X size={16} aria-hidden="true" />
              </button>
            </div>
          )}
        </div>
        <p id={noticeId} className="text-xs text-stone-600">
          {AI_NOTICE_SHORT}
        </p>
        {header?.notice && (
          <p className="rounded-md bg-amber-50 p-2 text-xs text-amber-900">{header.notice}</p>
        )}
        {header && header.facts.length > 0 && (
          <details>
            <summary className="text-sm">Lo que sabe TaxPain de esto</summary>
            <dl className="mt-2 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-sm">
              {header.facts.map(([k, v]) => (
                <Fragment key={k}>
                  <dt className="text-stone-600">{k}</dt>
                  <dd>{v}</dd>
                </Fragment>
              ))}
            </dl>
          </details>
        )}
        {opened && (
          <Materials
            materials={conversation?.materials ?? []}
            open={materialsOpen}
            onToggle={setMaterialsOpen}
            summaryRef={materialsSummary}
            onRemove={remove}
          />
        )}
      </header>
      {loadError ? (
        <p role="alert" className="m-4 text-sm text-red-700">
          {loadError}
        </p>
      ) : !opened || !header ? (
        <p className="m-4 text-sm text-stone-600">Cargando la conversación…</p>
      ) : (
        <div
          ref={log}
          role="log"
          aria-label="Conversación"
          // biome-ignore lint/a11y/noNoninteractiveTabindex: se desplaza con el teclado
          tabIndex={0}
          // En la página, el registro no pasa de media pantalla y el cuadro va debajo, entero.
          className={`min-h-32 flex-1 overflow-y-auto p-4 ${layout === 'page' ? 'max-h-[55vh]' : ''}`}
        >
          <ol className="space-y-3">
            {opened.changed && (
              <li className="rounded-md bg-amber-50 p-2 text-sm text-amber-900">{CHANGED}</li>
            )}
            <li className="mr-6">
              <p className="text-xs font-medium text-stone-600">TaxPain</p>
              <div className="rounded-lg border border-dashed border-stone-300 bg-stone-50 p-3 text-sm">
                <p className="whitespace-pre-wrap">{header.intro}</p>
              </div>
            </li>
            {items}
          </ol>
        </div>
      )}
      <div className="shrink-0 space-y-2 border-t border-stone-200 p-4 text-sm">
        <p role="status" className="sr-only">
          {statusText}
        </p>
        {slow && waiting !== null && (
          <p aria-hidden="true" className="text-xs text-stone-600">
            {slowText}
          </p>
        )}
        {error && (
          <p role="alert" className="text-red-700">
            {error}
          </p>
        )}
        {!loaded ? null : conversation?.full ? (
          <>
            <p>{FULL}</p>
            <DeleteConversation
              conversation={conversation}
              disabled={waiting !== null}
              onDeleted={() => {
                setOpened((o) => (o ? { ...o, conversation: null, changed: false } : o));
                closePreview();
                setAnnounce('Conversación borrada.');
                onChange?.(null);
                // La confirmación (con el foco) deja paso al cuadro, que aún no está pintado.
                if (layout === 'panel') requestAnimationFrame(() => box.current?.focus());
              }}
            />
          </>
        ) : setup && status ? (
          <AssistantSetup
            status={status}
            intro="Para hablar con la IA, elige el proveedor y acepta qué se envía. Sólo hace falta una vez por empresa."
            onSaved={() => {
              assistantStatus()
                .then((s) => alive.current && setStatus(s))
                .catch((e) => alive.current && setError(errorMessage(e)));
            }}
          />
        ) : (
          <>
            {factOpen && (
              <FactForm
                busy={factBusy}
                blocked={addBlocked}
                error={factError}
                onAdd={addFact}
                onCancel={() => {
                  setFactOpen(false);
                  setFactError(null);
                }}
              />
            )}
            {/* Fuera del registro: al volver a salir (tras borrar o si falla la primera
                pregunta) no se leen como algo nuevo de la conversación. */}
            {header &&
              messages.length === 0 &&
              waiting === null &&
              header.suggestions.length > 0 && (
                <fieldset className="flex flex-wrap gap-2">
                  <legend className="sr-only">Preguntas para empezar</legend>
                  {header.suggestions.map((s) => (
                    <button
                      key={s}
                      type="button"
                      className="border px-2 py-1 text-left text-sm whitespace-normal"
                      onClick={() => {
                        changeText(s);
                        box.current?.focus();
                      }}
                    >
                      {s}
                    </button>
                  ))}
                </fieldset>
              )}
            <label htmlFor={boxId} className="block font-medium">
              Tu pregunta
            </label>
            <textarea
              id={boxId}
              ref={box}
              className="min-h-20 w-full rounded border border-stone-300 px-2 py-1"
              aria-describedby={helpId}
              placeholder="Escribe tu pregunta"
              value={text}
              onChange={(e) => changeText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault();
                  send();
                }
              }}
            />
            <p id={helpId} className="text-xs text-stone-500">
              Con ⌘ + Intro se envía. Evita nombres de personas si no hacen falta: se tapan los que
              se detectan, no todos.
            </p>
            <div className="flex flex-wrap items-center gap-2">
              <button
                type="button"
                className="btn-primary aria-disabled:opacity-50"
                aria-disabled={waiting !== null || askBlocked || !text.trim() || undefined}
                onClick={send}
              >
                Preguntar
              </button>
              <button
                ref={factBtn}
                type="button"
                className="border aria-disabled:opacity-50"
                aria-expanded={factOpen}
                aria-disabled={addBlocked || undefined}
                onClick={() => {
                  if (!addBlocked) setFactOpen(true);
                }}
              >
                Añadir un dato
              </button>
              <button
                type="button"
                className="border aria-disabled:opacity-50"
                aria-describedby={docHelpId}
                aria-disabled={reading !== null || addBlocked || undefined}
                onClick={() => void addFile()}
              >
                Añadir un documento…
              </button>
            </div>
            <p id={docHelpId} className="text-xs text-stone-500">
              Se lee en tu Mac y con cada pregunta va su texto, anonimizado. El fichero no sale de
              tu Mac.
            </p>
            {reading !== null && (
              <p aria-hidden="true" className="text-xs text-stone-600">
                Leyendo «{reading}» en tu Mac…
              </p>
            )}
            <p className="text-xs text-stone-500">
              {status?.remote
                ? `Se envía a ${providerLabel(status.settings.provider)} con nombres, NIF y cuentas cambiados por marcadores.`
                : 'Modelo local: nada sale de tu Mac.'}
            </p>
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
              <button
                type="button"
                className="text-link"
                aria-expanded={previewOpen}
                onClick={togglePreview}
              >
                {previewOpen ? 'Ocultar lo que se envía' : 'Ver qué se envía'}
              </button>
              <DeleteConversation
                conversation={conversation}
                disabled={waiting !== null}
                onDeleted={() => {
                  setOpened((o) =>
                    o
                      ? {
                          ...o,
                          conversation: null,
                          changed: false,
                          header:
                            target.topic.kind === 'general'
                              ? { ...o.header, title: 'Nueva conversación' }
                              : o.header,
                        }
                      : o,
                  );
                  setText('');
                  closePreview();
                  setAnnounce('Conversación borrada.');
                  onChange?.(null);
                  if (layout === 'panel') box.current?.focus();
                }}
              />
            </div>
            {previewOpen && preview && <Preview p={preview} />}
          </>
        )}
      </div>
    </div>
  );
}

/** «Borrar la conversación», con su confirmación. */
function DeleteConversation({
  conversation,
  disabled,
  onDeleted,
}: {
  conversation: Conversation | null;
  disabled: boolean;
  onDeleted: () => void;
}) {
  return (
    <ConfirmAction
      label="Borrar la conversación"
      question="Se borran las preguntas, las respuestas y lo que has añadido. No se puede deshacer; las copias de seguridad que ya has hecho las conservan."
      confirmLabel="Borrar"
      className="btn-ghost text-red-700"
      disabled={disabled || conversation === null}
      onConfirm={async () => {
        if (!conversation) return;
        await assistantConversationDelete(conversation.id);
        onDeleted();
      }}
    />
  );
}

import { useEffect, useId, useRef, useState } from 'react';
import {
  type AssistantTarget,
  ConversationView,
  dayLabel,
  usePendingQuestions,
} from './AssistantPanel';
import { AssistantSetup } from './AssistantParts';
import { useAnnounce } from './announce';
import {
  type AssistantStatus,
  type AssistantTopic,
  assistantConversationDelete,
  assistantConversations,
  assistantConversationsDeleteAll,
  assistantStatus,
  type ConversationSummary,
  errorMessage,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const NEW: AssistantTarget = {
  topic: { kind: 'general' },
  title: 'Nueva conversación',
  conversationId: null,
};

/** De qué es una conversación guardada, en corto. */
function kindLabel(t: AssistantTopic): string {
  switch (t.kind) {
    case 'general':
      return 'General';
    case 'ahorro':
      return `Ahorrar impuestos ${t.year}`;
    case 'ahorro_plan':
      return `Tu plan de ahorro ${t.year}`;
    case 'hallazgo':
      return `${t.origin === 'aeat' ? 'Qué revisar' : 'Revisión de la carpeta'}${t.year ? ` ${t.year}` : ''}`;
    case 'modelo':
      return `Modelos ${t.year}`;
  }
}

/** «hoy», «ayer» o «31/10/2026». */
function whenLabel(iso: string): string {
  const d = dayLabel(iso);
  if (d === 'Hoy' || d === 'Ayer') return d.toLowerCase();
  const x = new Date(iso);
  if (Number.isNaN(x.getTime())) return '';
  const p = (n: number) => String(n).padStart(2, '0');
  return `${p(x.getDate())}/${p(x.getMonth() + 1)}/${x.getFullYear()}`;
}

export function Assistant() {
  const [status, setStatus] = useState<AssistantStatus | null>(null);
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [list, setList] = useState<ConversationSummary[]>([]);
  const [selected, setSelected] = useState<AssistantTarget>(NEW);
  // Cambia al elegir otra conversación (o una nueva): la vista empieza de cero.
  const [view, setView] = useState(0);
  const [announce, setAnnounce] = useAnnounce();
  const listHeading = useRef<HTMLHeadingElement>(null);
  const titleId = useId();

  const load = () =>
    assistantStatus()
      .then((s) => {
        setStatus(s);
        setEditing(!(s.settings.consent || !s.remote) || (s.remote && !s.has_api_key));
      })
      .catch((e) => setError(errorMessage(e)));
  // Sólo vale la última lectura de la lista: una anterior puede llegar después con lo de antes.
  const reads = useRef(0);
  const refresh = () => {
    const n = ++reads.current;
    return assistantConversations()
      .then((l) => {
        if (n === reads.current) setList(l);
      })
      .catch(() => {
        if (n === reads.current) setList([]);
      });
  };
  // Lo que espera su respuesta no se borra; y la que llega (también la de un panel de otra
  // pantalla que se cerró mientras pensaba) entra en la lista.
  const pend = usePendingQuestions(() => void refresh());
  // biome-ignore lint/correctness/useExhaustiveDependencies: sólo al montar
  useEffect(() => {
    void load();
    void refresh();
  }, []);

  if (!status) {
    return error ? (
      <p role="alert" className="mt-6 text-sm text-red-700">
        {error}
      </p>
    ) : null;
  }

  const open = (t: AssistantTarget) => {
    setSelected(t);
    setView((v) => v + 1);
  };
  /** Tras borrar la que estaba abierta (o todas): una nueva y el foco en la lista. */
  const startAgain = (message: string) => {
    open(NEW);
    setAnnounce(message);
    requestAnimationFrame(() => listHeading.current?.focus());
  };

  return (
    <section className="mt-6" aria-labelledby="asistente-h">
      <div className="flex items-baseline justify-between">
        <h2 id="asistente-h" className="text-lg font-semibold">
          Asistente
        </h2>
        {!editing && (
          <button type="button" className="text-sm underline" onClick={() => setEditing(true)}>
            Ajustes del asistente
          </button>
        )}
      </div>
      <p className="mt-1 text-sm text-stone-600">
        Explica, prepara y comprueba con tus datos de TaxPain. No es asesoramiento fiscal y no
        presenta nada: cada presentación la revisas y confirmas tú.
      </p>
      <p className="mt-1 text-xs text-stone-500">
        Estás hablando con un sistema de inteligencia artificial (Reglamento (UE) 2024/1689, art.
        50.1): sus respuestas las genera un modelo de lenguaje y pueden contener errores. Antes de
        enviar nada se sustituyen tu nombre, NIF, domicilio, cuentas y otros datos personales por
        marcadores.
      </p>
      <p role="status" className="sr-only">
        {announce}
      </p>
      {editing ? (
        <AssistantSetup
          status={status}
          onSaved={() => {
            setEditing(false);
            void load();
          }}
        />
      ) : (
        <div className="mt-4 grid gap-6 lg:grid-cols-[17rem_minmax(0,1fr)]">
          <section aria-labelledby="ia-guardadas-h" className="space-y-3 text-sm">
            <h3
              id="ia-guardadas-h"
              ref={listHeading}
              tabIndex={-1}
              className="text-base font-semibold"
            >
              Conversaciones guardadas
            </h3>
            <button
              type="button"
              className="border"
              onClick={() => {
                open(NEW);
                setAnnounce('Nueva conversación.');
              }}
            >
              Nueva conversación
            </button>
            <p className="text-xs text-stone-600">
              Se guardan cifradas en tu Mac, con esta empresa, y van en sus copias de seguridad.
            </p>
            {list.length === 0 ? (
              <p className="text-stone-600">
                Aún no hay ninguna. Empieza una aquí o con «Preguntar a la IA» en Ahorrar impuestos,
                en Qué revisar, en Importar → Revisión o en Modelos.
              </p>
            ) : (
              <ul className="space-y-2">
                {list.map((c) => (
                  <li key={c.id} className="rounded-md border border-stone-200 p-2">
                    <button
                      type="button"
                      className="text-left font-medium"
                      aria-current={selected.conversationId === c.id ? 'true' : undefined}
                      onClick={() => {
                        open({ topic: c.topic, title: c.title, conversationId: c.id });
                        setAnnounce(`Abierta: «${c.title}».`);
                      }}
                    >
                      {c.title}
                    </button>
                    <p className="text-xs text-stone-600">
                      {kindLabel(c.topic)} · {c.questions}{' '}
                      {c.questions === 1 ? 'pregunta' : 'preguntas'} · {whenLabel(c.updated_at)}
                    </p>
                    <ConfirmAction
                      label="Borrar"
                      ariaLabel={`Borrar: ${c.title}`}
                      question="Se borran las preguntas, las respuestas y lo que has añadido. No se puede deshacer; las copias de seguridad que ya has hecho las conservan."
                      confirmLabel="Borrar"
                      className="btn-ghost text-red-700"
                      disabled={pend.waiting(c.topic, c.id)}
                      onConfirm={async () => {
                        await assistantConversationDelete(c.id);
                        setList((l) => l.filter((x) => x.id !== c.id));
                        if (selected.conversationId === c.id) startAgain('Conversación borrada.');
                        else {
                          setAnnounce('Conversación borrada.');
                          requestAnimationFrame(() => listHeading.current?.focus());
                        }
                      }}
                    />
                  </li>
                ))}
              </ul>
            )}
            {list.length >= 2 && (
              <ConfirmAction
                label="Borrar todas"
                question={`Se borran las ${list.length} conversaciones guardadas de esta empresa, con lo que has añadido. No se puede deshacer; las copias de seguridad que ya has hecho las conservan.`}
                confirmLabel="Borrar todas"
                className="btn-ghost text-red-700"
                disabled={pend.any}
                onConfirm={async () => {
                  const n = await assistantConversationsDeleteAll();
                  setList([]);
                  startAgain(`Se han borrado las ${n} conversaciones.`);
                }}
              />
            )}
          </section>
          <ConversationView
            key={view}
            target={selected}
            layout="page"
            titleId={titleId}
            onCreated={(id) => setSelected((s) => ({ ...s, conversationId: id }))}
            onChange={(s) => {
              if (s) setList((l) => [s, ...l.filter((x) => x.id !== s.id)]);
              else {
                void refresh();
                startAgain('Conversación borrada.');
              }
            }}
          />
        </div>
      )}
    </section>
  );
}

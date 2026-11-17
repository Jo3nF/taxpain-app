import { type FormEvent, useState } from 'react';
import {
  type AssistantProvider,
  type AssistantStatus,
  assistantSave,
  errorMessage,
  type MessageNotes,
} from './api';

export type Choice = 'anthropic' | 'openai' | 'mistral' | 'local';

export const PRESETS: Record<Choice, { label: string; provider: AssistantProvider; note: string }> =
  {
    anthropic: {
      label: 'Claude (Anthropic)',
      provider: { kind: 'Anthropic', model: 'claude-opus-5-5' },
      note: 'Necesita tu clave de API de Anthropic.',
    },
    openai: {
      label: 'OpenAI',
      provider: { kind: 'OpenAiCompatible', base_url: 'https://api.openai.com/v1', model: '' },
      note: 'Necesita tu clave de API de OpenAI.',
    },
    mistral: {
      label: 'Mistral (servidores en la UE)',
      provider: {
        kind: 'OpenAiCompatible',
        base_url: 'https://api.mistral.ai/v1',
        model: 'mistral-large-latest',
      },
      note: 'Necesita tu clave de API de Mistral.',
    },
    local: {
      label: 'Modelo local (Ollama, LM Studio…)',
      provider: { kind: 'OpenAiCompatible', base_url: 'http://localhost:11434/v1', model: '' },
      note: 'Nada sale de tu Mac. Arranca el modelo antes (p. ej. «ollama serve»).',
    },
  };

export function choiceOf(p: AssistantProvider): Choice {
  if (p.kind === 'Anthropic') return 'anthropic';
  if (p.base_url.includes('api.openai.com')) return 'openai';
  if (p.base_url.includes('api.mistral.ai')) return 'mistral';
  return 'local';
}

/** Nombre del proveedor para los textos («Claude (Anthropic)», «OpenAI»…). */
export function providerLabel(p: AssistantProvider): string {
  return PRESETS[choiceOf(p)].label;
}

/** Aviso corto de que se habla con una IA (Reglamento (UE) 2024/1689, art. 50.1). */
export const AI_NOTICE_SHORT =
  'Hablas con una IA (Reglamento (UE) 2024/1689, art. 50.1): puede equivocarse. Orienta, no decide: antes de firmar, invertir o presentar nada, confírmalo con tu asesor.';

const input = 'w-full rounded border border-stone-300 px-2 py-1';

/** Proveedor, modelo, clave y consentimiento del asistente (de la empresa abierta). */
export function AssistantSetup({
  status,
  onSaved,
  intro,
}: {
  status: AssistantStatus;
  onSaved: () => void;
  /** Por qué se pide (dentro del panel de una conversación). */
  intro?: string;
}) {
  const [choice, setChoice] = useState<Choice>(choiceOf(status.settings.provider));
  const [provider, setProvider] = useState<AssistantProvider>(status.settings.provider);
  const [key, setKey] = useState('');
  const [consent, setConsent] = useState(status.settings.consent);
  const [error, setError] = useState<string | null>(null);
  const remote = choice !== 'local';

  async function save(e: FormEvent) {
    e.preventDefault();
    setError(null);
    if (!provider.model.trim()) return setError('Indica el modelo.');
    if (remote && !consent) return setError('Para usar un proveedor externo, acepta el envío.');
    try {
      // Proveedor y clave juntos: si se cancela Touch ID, no se queda el proveedor nuevo sin
      // clave.
      await assistantSave(
        { provider, consent: remote ? consent : false },
        remote && key.trim() ? key : null,
      );
      setKey('');
      onSaved();
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  return (
    <form
      onSubmit={save}
      className="mt-4 space-y-4 rounded-lg border border-stone-200 bg-white p-5 text-sm"
    >
      {intro && <p>{intro}</p>}
      <div className="max-w-2xl space-y-4">
        <label className="flex flex-col">
          Proveedor
          <select
            className={input}
            value={choice}
            onChange={(e) => {
              const c = e.target.value as Choice;
              setChoice(c);
              setProvider(PRESETS[c].provider);
              // Lo aceptado vale para el proveedor al que se aceptó: con otro, hay que volver a
              // aceptarlo.
              setConsent(
                c === choiceOf(status.settings.provider) ? status.settings.consent : false,
              );
            }}
          >
            {(Object.keys(PRESETS) as Choice[]).map((c) => (
              <option key={c} value={c}>
                {PRESETS[c].label}
              </option>
            ))}
          </select>
          <span className="mt-1 text-xs text-stone-500">{PRESETS[choice].note}</span>
        </label>
        {provider.kind === 'OpenAiCompatible' && choice === 'local' && (
          <label className="flex flex-col">
            Dirección del servidor local
            <input
              className={input}
              value={provider.base_url}
              onChange={(e) => setProvider({ ...provider, base_url: e.target.value })}
            />
          </label>
        )}
        <label className="flex flex-col">
          Modelo
          <input
            className={input}
            value={provider.model}
            onChange={(e) => setProvider({ ...provider, model: e.target.value })}
            placeholder={choice === 'local' ? 'p. ej. llama3.1' : ''}
          />
        </label>
        {remote && (
          <label className="flex flex-col">
            Clave de API {status.has_api_key && '(ya guardada; escribe otra para cambiarla)'}
            <input
              className={input}
              type="password"
              autoComplete="off"
              value={key}
              onChange={(e) => setKey(e.target.value)}
            />
            <span className="mt-1 text-xs text-stone-500">
              Se guarda cifrada en tu Mac y sólo se usa para hablar con el proveedor.
            </span>
          </label>
        )}
      </div>
      {remote && (
        <label>
          <input type="checkbox" checked={consent} onChange={(e) => setConsent(e.target.checked)} />
          <span>
            Entiendo que mis preguntas, los datos del tema del que pregunte, lo que añada a la
            conversación (también el texto de los documentos) y un resumen de mis cifras (sin
            nombres; NIF, IBAN, emails y teléfonos sustituidos por marcadores) se envían a{' '}
            {PRESETS[choice].label}. Nunca se envían el certificado, contraseñas ni claves.
          </span>
        </label>
      )}
      <div className="form-actions">
        <button type="submit" className="rounded-md btn-primary px-3 py-1.5">
          Guardar
        </button>
      </div>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
    </form>
  );
}

export const TOOL_NAMES: Record<string, string> = {
  plazos_proximos: 'plazos',
  borrador_modelo: 'borrador de un modelo',
  buscar_facturas: 'facturas',
  saldo_cuenta: 'saldo de una cuenta',
  buscar_ley: 'búsqueda en el BOE',
  leer_articulo: 'artículo del BOE',
};

/** Qué consultó la IA (sólo lectura), qué normas leyó y citas que no se pudieron comprobar. */
export function ReplyNotes({ r }: { r: Pick<MessageNotes, 'used' | 'sources' | 'unverified'> }) {
  const used = [...new Set(r.used ?? [])].map((u) => TOOL_NAMES[u] ?? u);
  const sources = r.sources ?? [];
  const unverified = r.unverified ?? [];
  if (used.length === 0 && sources.length === 0 && unverified.length === 0) return null;
  return (
    <div className="mt-2 space-y-1 border-t border-stone-100 pt-2 text-xs text-stone-500">
      {used.length > 0 && <p>Ha consultado (sólo lectura): {used.join(', ')}.</p>}
      {sources.length > 0 && <p>Normas leídas en el BOE: {sources.join('; ')}.</p>}
      {unverified.length > 0 && (
        <p className="text-amber-800">
          Estas citas no aparecen en lo que ha leído del BOE; no te fíes de ellas sin comprobarlas:{' '}
          {unverified.map((q) => `«${q}»`).join(' · ')}
        </p>
      )}
    </div>
  );
}

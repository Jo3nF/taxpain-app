import { Mail, Paperclip } from 'lucide-react';
import { type FormEvent, useEffect, useRef, useState } from 'react';
import { type EmailDraft, errorMessage, euros, invoiceEmailCompose, invoiceEmailOpen } from './api';

/** Correo de una factura o de un recordatorio de pago. */
export function InvoiceEmailPanel({
  invoiceId,
  number,
  reminder,
  onClose,
  onDone,
}: {
  invoiceId: number;
  number: string;
  reminder: boolean;
  onClose: () => void;
  onDone: (message: string) => void;
}) {
  return (
    <EmailPanel
      title={reminder ? `Recordatorio de pago · ${number}` : `Enviar ${number} por correo`}
      compose={() => invoiceEmailCompose(invoiceId, reminder)}
      open={(to, subject, body) => invoiceEmailOpen(invoiceId, reminder, to, subject, body)}
      doneMessage={
        reminder
          ? `Recordatorio de ${number} abierto en Mail: revísalo y envíalo.`
          : `Correo de ${number} abierto en Mail: revísalo y envíalo.`
      }
      onClose={onClose}
      onDone={onDone}
    />
  );
}

function when(at: string): string {
  const d = new Date(at);
  return Number.isNaN(d.getTime()) ? at : d.toLocaleDateString('es-ES');
}

/**
 * Correo de una factura o recordatorio de pago: se propone el texto, el usuario lo revisa y se
 * abre en Mail como mensaje nuevo con el PDF adjunto. TaxPain no envía nada.
 */
export function EmailPanel({
  title,
  compose,
  open,
  doneMessage,
  onClose,
  onDone,
}: {
  title: string;
  compose: () => Promise<EmailDraft>;
  open: (to: string | null, subject: string, body: string) => Promise<void>;
  doneMessage: string;
  onClose: () => void;
  onDone: (message: string) => void;
}) {
  const [draft, setDraft] = useState<EmailDraft | null>(null);
  const [to, setTo] = useState('');
  const [subject, setSubject] = useState('');
  const [body, setBody] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const first = useRef<HTMLInputElement>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: se propone una vez, al abrir
  useEffect(() => {
    let cancelled = false;
    compose()
      .then((d) => {
        if (cancelled) return;
        setDraft(d);
        setTo(d.to ?? '');
        setSubject(d.subject);
        setBody(d.body);
      })
      .catch((e) => {
        if (!cancelled) setError(errorMessage(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (draft) first.current?.focus();
  }, [draft]);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await open(to.trim() || null, subject, body);
      onDone(doneMessage);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section
      aria-label={title}
      className="card mt-4 p-5"
      onKeyDown={(e) => {
        if (e.key === 'Escape' && !busy) onClose();
      }}
    >
      <h3 className="flex items-center gap-2 font-semibold">
        <Mail size={16} aria-hidden="true" className="text-stone-500" />
        {title}
      </h3>
      {!draft && !error && <p className="mt-3 text-sm text-stone-500">Preparando…</p>}
      {error && !draft && (
        <div className="mt-3 space-y-2 text-sm">
          <p role="alert" className="text-red-700">
            {error}
          </p>
          <button type="button" className="btn-ghost" onClick={onClose}>
            Cerrar
          </button>
        </div>
      )}
      {draft && (
        <form onSubmit={submit} className="mt-3 max-w-3xl space-y-3 text-sm">
          {draft.reminder !== null && (
            <p className="rounded-md bg-amber-50 p-3 text-amber-950">
              {draft.overdue_days > 0
                ? `Vencida hace ${draft.overdue_days} ${draft.overdue_days === 1 ? 'día' : 'días'}`
                : 'Aún no ha vencido'}{' '}
              ·{' '}
              {draft.overdue > 0 && draft.overdue !== draft.pending
                ? `vencido ${euros(draft.overdue)} de ${euros(draft.pending)} pendiente`
                : `pendiente ${euros(draft.pending)}`}{' '}
              · {draft.reminder}.º recordatorio
              {draft.last_reminder && ` (el anterior, el ${when(draft.last_reminder)})`}
            </p>
          )}
          <label className="block">
            Para
            <input
              ref={first}
              type="email"
              multiple
              className="mt-1 block w-full rounded-md border border-stone-300 px-3 py-2"
              placeholder="Si lo dejas vacío, lo pones en Mail"
              value={to}
              onChange={(e) => setTo(e.target.value)}
            />
          </label>
          <label className="block">
            Asunto
            <input
              className="mt-1 block w-full rounded-md border border-stone-300 px-3 py-2"
              value={subject}
              onChange={(e) => setSubject(e.target.value)}
            />
          </label>
          <label className="block">
            Mensaje
            <textarea
              rows={9}
              className="mt-1 block w-full rounded-md border border-stone-300 px-3 py-2"
              value={body}
              onChange={(e) => setBody(e.target.value)}
            />
          </label>
          <p className="flex items-center gap-1.5 text-stone-600">
            <Paperclip size={14} aria-hidden="true" />
            {draft.attachments.join(', ')}
          </p>
          <p className="text-stone-600">
            Se abre en Mail como mensaje nuevo para que lo revises y lo envíes tú: TaxPain no envía
            nada ni guarda contraseñas.
            {draft.language === 'En' &&
              ' El texto va en inglés (cliente de fuera o ficha en inglés).'}
          </p>
          {error && (
            <p role="alert" className="text-red-700">
              {error}
            </p>
          )}
          <div className="form-actions">
            <button
              type="submit"
              disabled={busy}
              className="btn-primary rounded-md px-4 py-2 disabled:opacity-50"
            >
              {busy ? 'Abriendo…' : 'Abrir en Mail'}
            </button>
            <button type="button" className="btn-ghost" onClick={onClose}>
              Cancelar
            </button>
          </div>
        </form>
      )}
    </section>
  );
}

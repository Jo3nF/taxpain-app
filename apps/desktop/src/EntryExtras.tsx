import { useCallback, useEffect, useState } from 'react';
import {
  type EntryDocument,
  entryAttach,
  entryDetach,
  entryDocumentOpen,
  entryDocuments,
  entryNotesSet,
  errorMessage,
  pickAnyFile,
  type StoredEntry,
} from './api';

function size(n: number): string {
  return n < 1024 * 1024 ? `${Math.ceil(n / 1024)} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/** Observaciones y documentos adjuntos (justificantes, contratos…) de un asiento. */
export function EntryExtras({ s, onChange }: { s: StoredEntry; onChange: () => void }) {
  const [notes, setNotes] = useState(s.notes ?? '');
  const [docs, setDocs] = useState<EntryDocument[]>([]);
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(() => {
    entryDocuments(s.id)
      .then(setDocs)
      .catch((e) => setMsg(errorMessage(e)));
  }, [s.id]);
  useEffect(load, [load]);
  const run = async (f: () => Promise<unknown>, ok: string) => {
    try {
      await f();
      setMsg(ok);
      load();
      onChange();
    } catch (e) {
      setMsg(errorMessage(e));
    }
  };
  return (
    <div className="my-1 space-y-3 rounded-md bg-stone-50 p-3 text-sm">
      <label className="block">
        Observaciones del asiento {s.number}
        <textarea
          className="mt-1 block w-full rounded border px-2 py-1"
          rows={2}
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
        />
      </label>
      <div className="form-actions">
        <button
          type="button"
          className="rounded border border-stone-300 px-2 py-0.5"
          onClick={() => run(() => entryNotesSet(s.id, notes), 'Observaciones guardadas.')}
        >
          Guardar observaciones
        </button>
        <button
          type="button"
          className="rounded border border-stone-300 px-2 py-0.5"
          onClick={async () => {
            const path = await pickAnyFile();
            if (path) await run(() => entryAttach(s.id, path), 'Documento adjuntado.');
          }}
        >
          Adjuntar documento…
        </button>
      </div>
      {docs.length > 0 && (
        <ul>
          {docs.map((d) => (
            <li key={d.id} className="flex items-center gap-2">
              <button
                type="button"
                className="text-link underline"
                onClick={() => entryDocumentOpen(d.id).catch((e) => setMsg(errorMessage(e)))}
              >
                {d.filename}
              </button>
              <span className="text-stone-500">{size(d.size)}</span>
              <button
                type="button"
                className="text-red-700"
                aria-label={`Quitar ${d.filename}`}
                onClick={() => run(() => entryDetach(s.id, d.id), 'Documento quitado.')}
              >
                Quitar
              </button>
            </li>
          ))}
        </ul>
      )}
      {msg && <p role="status">{msg}</p>}
    </div>
  );
}

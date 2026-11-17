import { ScrollText, ShieldCheck } from 'lucide-react';
import { type FormEvent, useCallback, useEffect, useState } from 'react';
import {
  type AuditEntry,
  auditoriaAcciones,
  auditoriaConsultar,
  auditoriaVerificar,
  errorMessage,
  type FiltroAuditoria,
  type VerificacionAuditoria,
} from './api';

const INPUT = 'mt-1 block w-full rounded-md border border-stone-300 px-2 py-1.5 text-sm';
const BUTTON = 'rounded-md border border-stone-300 bg-white px-3 py-1.5 text-sm';

const ACTORES: Record<string, string> = {
  user: 'Tú',
  system: 'TaxPain',
  assistant: 'Asistente',
};

/** «invoice.issue» → familia «invoice». */
function familias(acciones: [string, number][]): string[] {
  return [...new Set(acciones.map(([a]) => a.split('.')[0] ?? a))].sort();
}

function cuando(at: string): string {
  const d = new Date(at);
  return Number.isNaN(d.getTime()) ? at : d.toLocaleString('es-ES');
}

/** Registro de auditoría: consulta de sólo lectura con filtros y verificación de la cadena. */
export function Auditoria() {
  const [acciones, setAcciones] = useState<[string, number][]>([]);
  const [filtro, setFiltro] = useState<FiltroAuditoria>({});
  const [aplicado, setAplicado] = useState<FiltroAuditoria>({});
  const [entradas, setEntradas] = useState<AuditEntry[]>([]);
  const [total, setTotal] = useState(0);
  const [siguiente, setSiguiente] = useState<number | null>(null);
  const [verif, setVerif] = useState<VerificacionAuditoria | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [abierta, setAbierta] = useState<number | null>(null);

  const cargar = useCallback(async (f: FiltroAuditoria, mas: number | null) => {
    setError(null);
    try {
      const p = await auditoriaConsultar({ ...f, antes_de: mas, limite: 100 });
      setEntradas((prev) => (mas ? [...prev, ...p.entradas] : p.entradas));
      setTotal(p.total);
      setSiguiente(p.siguiente);
    } catch (e) {
      setError(errorMessage(e));
    }
  }, []);

  useEffect(() => {
    auditoriaAcciones()
      .then(setAcciones)
      .catch(() => setAcciones([]));
    cargar({}, null);
  }, [cargar]);

  function buscar(e: FormEvent) {
    e.preventDefault();
    setAplicado(filtro);
    cargar(filtro, null);
  }

  async function verificar() {
    setBusy(true);
    setVerif(null);
    try {
      setVerif(await auditoriaVerificar());
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const set = (k: keyof FiltroAuditoria) => (v: string) =>
    setFiltro((f) => ({ ...f, [k]: v === '' ? null : v }));

  return (
    <section className="mt-8" aria-labelledby="auditoria-title">
      <div className="flex items-center gap-3">
        <span className="icon-tile" aria-hidden="true">
          <ScrollText size={18} />
        </span>
        <h2 id="auditoria-title" className="text-lg font-semibold">
          Registro de auditoría
        </h2>
      </div>
      <p className="mt-1 text-sm text-stone-600">
        Todo lo que se ha hecho con efecto (emitir, registrar, borrar, presentar, exportar…), en
        orden y encadenado con huellas SHA-256: ninguna entrada se puede cambiar ni borrar sin que
        se note. Sólo se consulta: aquí no se cambia nada.
      </p>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <button type="button" className={BUTTON} disabled={busy} onClick={verificar}>
          <ShieldCheck size={14} aria-hidden="true" className="mr-1 inline" />
          {busy ? 'Comprobando…' : 'Verificar la cadena de huellas'}
        </button>
        {verif && (
          <p
            role={verif.ok ? 'status' : 'alert'}
            className={verif.ok ? 'text-sm text-emerald-800' : 'text-sm text-red-800'}
          >
            {verif.mensaje}
          </p>
        )}
      </div>
      <form onSubmit={buscar} className="mt-4 grid gap-3 sm:grid-cols-5" aria-label="Filtro">
        <label className="text-sm">
          Desde
          <input
            type="date"
            className={INPUT}
            value={filtro.desde ?? ''}
            onChange={(e) => set('desde')(e.target.value)}
          />
        </label>
        <label className="text-sm">
          Hasta
          <input
            type="date"
            className={INPUT}
            value={filtro.hasta ?? ''}
            onChange={(e) => set('hasta')(e.target.value)}
          />
        </label>
        <label className="text-sm">
          Tipo de acción
          <select
            className={INPUT}
            value={filtro.accion ?? ''}
            onChange={(e) => set('accion')(e.target.value)}
          >
            <option value="">Todas</option>
            {familias(acciones).map((f) => (
              <optgroup key={f} label={f}>
                <option value={f}>{f} (todas)</option>
                {acciones
                  .filter(([a]) => a.startsWith(`${f}.`))
                  .map(([a, n]) => (
                    <option key={a} value={a}>
                      {a} ({n})
                    </option>
                  ))}
              </optgroup>
            ))}
          </select>
        </label>
        <label className="text-sm">
          Quién
          <select
            className={INPUT}
            value={filtro.actor ?? ''}
            onChange={(e) => set('actor')(e.target.value)}
          >
            <option value="">Todos</option>
            {Object.entries(ACTORES).map(([k, v]) => (
              <option key={k} value={k}>
                {v}
              </option>
            ))}
          </select>
        </label>
        <label className="text-sm">
          Texto
          <input
            className={INPUT}
            value={filtro.texto ?? ''}
            onChange={(e) => set('texto')(e.target.value)}
            placeholder="Nº de factura, NIF…"
          />
        </label>
        <div className="sm:col-span-5">
          <button type="submit" className={BUTTON}>
            Buscar
          </button>
        </div>
      </form>
      {error && (
        <p role="alert" className="mt-3 text-sm text-red-800">
          {error}
        </p>
      )}
      <p className="mt-3 text-sm text-stone-500" aria-live="polite">
        {total} entradas
        {aplicado.accion || aplicado.desde || aplicado.texto ? ' con el filtro' : ''}.
      </p>
      <table className="mt-2 w-full text-left text-sm">
        <thead>
          <tr className="border-b border-stone-200 text-stone-500">
            <th scope="col" className="py-1 pr-2">
              Nº
            </th>
            <th scope="col" className="py-1 pr-2">
              Cuándo
            </th>
            <th scope="col" className="py-1 pr-2">
              Quién
            </th>
            <th scope="col" className="py-1 pr-2">
              Acción
            </th>
            <th scope="col" className="py-1">
              Sobre
            </th>
          </tr>
        </thead>
        <tbody>
          {entradas.map((e) => (
            <tr key={e.seq} className="border-b border-stone-100 align-top">
              <td className="py-1 pr-2 tabular-nums">{e.seq}</td>
              <td className="py-1 pr-2 whitespace-nowrap">{cuando(e.at)}</td>
              <td className="py-1 pr-2">{ACTORES[e.actor] ?? e.actor}</td>
              <td className="py-1 pr-2 font-mono text-xs">{e.action}</td>
              <td className="py-1">
                <button
                  type="button"
                  className="text-left underline-offset-2 hover:underline"
                  aria-expanded={abierta === e.seq}
                  onClick={() => setAbierta(abierta === e.seq ? null : e.seq)}
                >
                  {e.subject || '—'}
                </button>
                {abierta === e.seq && (
                  <div className="mt-1 space-y-1 text-xs text-stone-600">
                    <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded bg-stone-50 p-2">
                      {e.details}
                    </pre>
                    <p className="break-all font-mono">Huella: {e.hash}</p>
                    <p className="break-all font-mono">Anterior: {e.prev_hash}</p>
                  </div>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {siguiente !== null && (
        <button
          type="button"
          className={`${BUTTON} mt-3`}
          onClick={() => cargar(aplicado, siguiente)}
        >
          Ver más antiguas
        </button>
      )}
    </section>
  );
}

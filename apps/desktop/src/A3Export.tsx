import { useEffect, useState } from 'react';
import { type A3Options, a3Options, errorMessage, exportA3, pickFolder } from './api';

/** Exporta el diario del ejercicio a `SUENLACE.DAT` para importarlo en a3ASESOR | eco / | con. */
export function A3Export({ year }: { year: number }) {
  const [o, setO] = useState<A3Options>({ company: 1, digits: 8, format: 'F5', invoices: true });
  const [msg, setMsg] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);

  useEffect(() => {
    a3Options()
      .then(setO)
      .catch((e) => console.warn('Opciones de A3 no disponibles:', errorMessage(e)));
  }, []);

  async function run() {
    const dir = await pickFolder();
    if (!dir) return;
    try {
      const r = await exportA3(year, o, dir);
      setMsg(
        `${r.entries} asientos (${r.invoices} como facturas con IVA) y ${r.accounts} clientes/proveedores guardados en ${r.path}. En a3: Utilidades → Importar/Exportar → Enlace contable.`,
      );
      setWarnings(r.warnings);
    } catch (e) {
      setMsg(errorMessage(e));
      setWarnings([]);
    }
  }

  return (
    <section aria-label="Exportar a a3">
      <p className="font-medium">Para la gestoría con a3 (SUENLACE.DAT)</p>
      <div className="form-grid mt-2 text-sm">
        <label>
          Código de empresa en a3
          <input
            type="number"
            min={1}
            max={99999}
            value={o.company}
            onChange={(e) => setO({ ...o, company: Number(e.target.value) })}
          />
        </label>
        <label>
          Dígitos de las cuentas
          <input
            type="number"
            min={6}
            max={12}
            value={o.digits}
            onChange={(e) => setO({ ...o, digits: Number(e.target.value) })}
          />
        </label>
        <label className="span-2">
          Formato
          <select
            value={o.format ?? 'F5'}
            onChange={(e) => setO({ ...o, format: e.target.value as 'F4' | 'F5' })}
          >
            <option value="F5">5 (512 bytes, SII; el actual)</option>
            <option value="F4">4 (256 bytes, versiones antiguas)</option>
          </select>
        </label>
        <label className="span-full flex gap-2">
          <input
            type="checkbox"
            checked={o.invoices ?? true}
            onChange={(e) => setO({ ...o, invoices: e.target.checked })}
          />
          Facturas con IVA (rellena los libros de IVA de a3)
        </label>
        <div className="span-full form-actions">
          <button type="button" className="rounded border border-stone-300 px-3 py-1" onClick={run}>
            Exportar diario {year} para a3…
          </button>
        </div>
      </div>
      {msg && (
        <p role="status" className="mt-2 text-sm">
          {msg}
        </p>
      )}
      {warnings.length > 0 && (
        <ul className="mt-1 list-disc pl-5 text-sm text-amber-900">
          {warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
    </section>
  );
}

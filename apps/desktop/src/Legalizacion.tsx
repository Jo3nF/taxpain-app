import { useState } from 'react';
import {
  errorMessage,
  formatDate,
  type LegalizationRequest,
  type LegalizationSaved,
  legalizationBuild,
  pickFolder,
} from './api';

/** Paquete para legalizar el Diario y el de Inventarios y Cuentas anuales en el Registro. */
export function Legalizacion({ year }: { year: number }) {
  const [r, setR] = useState<LegalizationRequest>({
    year,
    diary_number: 1,
    inventory_number: 1,
    summarized: false,
    address: null,
    town: null,
    postal_code: null,
    province_code: null,
    registry_code: null,
    tomo: null,
    folio: null,
    hoja: null,
    presenter_name: null,
    presenter_nif: null,
    presenter_email: null,
    presenter_phone: null,
    minutes_number: null,
    partners_number: null,
    contracts_number: null,
  });
  const [done, setDone] = useState<LegalizationSaved | null>(null);
  const [error, setError] = useState<string | null>(null);
  const text = (key: keyof LegalizationRequest, label: string, className?: string) => (
    <label className={className}>
      {label}
      <input
        value={(r[key] as string | null) ?? ''}
        onChange={(e) => setR({ ...r, [key]: e.target.value || null })}
      />
    </label>
  );

  async function run() {
    setError(null);
    const dir = await pickFolder();
    if (!dir) return;
    try {
      setDone(await legalizationBuild({ ...r, year }, dir));
    } catch (e) {
      setDone(null);
      setError(errorMessage(e));
    }
  }

  return (
    <section aria-label="Legalización de libros" className="text-sm">
      <p className="font-medium">Legalización de libros en el Registro Mercantil ({year})</p>
      <p className="text-stone-600">
        Prepara el ZIP con el Diario y el libro de Inventarios y Cuentas anuales (balance inicial,
        sumas y saldos trimestrales, inventario de cierre, balance y pérdidas y ganancias) y, si los
        marcas, el libro de actas, el de socios y el de contratos del socio único, con NOMBRES.TXT y
        DATOS.TXT. Se presenta con certificado en registradores.org en los cuatro meses siguientes
        al cierre. TaxPain no lo envía.
      </p>
      <div className="form-grid mt-3">
        <label>
          Nº del Diario
          <input
            type="number"
            min={1}
            value={r.diary_number}
            onChange={(e) => setR({ ...r, diary_number: Number(e.target.value) })}
          />
        </label>
        <label>
          Nº de Inventarios
          <input
            type="number"
            min={1}
            value={r.inventory_number}
            onChange={(e) => setR({ ...r, inventory_number: Number(e.target.value) })}
          />
        </label>
        {text('registry_code', 'Código del Registro')}
        {text('tomo', 'Tomo')}
        {text('folio', 'Folio')}
        {text('hoja', 'Hoja')}
        <label className="span-2 flex gap-2">
          <input
            type="checkbox"
            checked={r.summarized}
            onChange={(e) => setR({ ...r, summarized: e.target.checked })}
          />
          Diario resumido por meses
        </label>
      </div>
      <fieldset className="mt-3">
        <legend className="text-stone-700">
          Libros societarios (en PDF con folios, de Contabilidad → Libros societarios)
        </legend>
        <div className="form-grid mt-1">
          {(
            [
              ['minutes_number', 'Libro de actas (aprobadas del ejercicio)'],
              ['partners_number', 'Libro registro de socios'],
              ['contracts_number', 'Libro de contratos del socio único'],
            ] as const
          ).map(([key, label]) => (
            <div key={key} className="flex flex-wrap items-end gap-2">
              <label className="flex gap-2">
                <input
                  type="checkbox"
                  checked={r[key] != null}
                  onChange={(e) => setR({ ...r, [key]: e.target.checked ? 1 : null })}
                />
                {label}
              </label>
              {r[key] != null && (
                <label>
                  Nº
                  <input
                    type="number"
                    min={1}
                    className="w-20"
                    value={r[key] ?? 1}
                    onChange={(e) => setR({ ...r, [key]: Number(e.target.value) })}
                  />
                </label>
              )}
            </div>
          ))}
        </div>
      </fieldset>
      <details className="mt-3">
        <summary className="cursor-pointer text-stone-700">
          Domicilio y presentante (si no son los de los datos de facturación)
        </summary>
        <div className="form-grid mt-3">
          {text('address', 'Domicilio', 'span-2')}
          {text('town', 'Ciudad')}
          {text('postal_code', 'Código postal')}
          {text('province_code', 'Provincia (código)')}
          {text('presenter_name', 'Presentante', 'span-2')}
          {text('presenter_nif', 'NIF del presentante')}
          {text('presenter_email', 'Correo')}
          {text('presenter_phone', 'Teléfono')}
        </div>
      </details>
      <button
        type="button"
        className="mt-4 rounded border border-stone-300 px-3 py-1"
        onClick={run}
      >
        Preparar legalización {year}…
      </button>
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
      {done && (
        <div role="status" className="mt-2">
          <p>
            Guardado en {done.path}. Plazo: hasta el {formatDate(done.deadline)}.
          </p>
          <ul className="mt-1 list-disc pl-5">
            {done.books.map((b) => (
              <li key={b.file}>
                {b.file}: {b.description}, libro nº {b.number}
              </li>
            ))}
          </ul>
          {done.warnings.length > 0 && (
            <ul className="mt-1 list-disc pl-5 text-amber-900">
              {done.warnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          )}
        </div>
      )}
    </section>
  );
}

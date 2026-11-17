import { useState } from 'react';
import {
  type AccountsModel,
  type DepositRequest,
  type DepositSaved,
  depositBuild,
  errorMessage,
  pickFolder,
  pickPdf,
} from './api';

/** Depósito de cuentas: ZIP para importar en el programa D2 del Registro Mercantil. */
export function Deposito({ year }: { year: number }) {
  const [r, setR] = useState<DepositRequest>({
    year,
    model: 'Pymes',
    registry_code: '',
    legal_form: 'Sl',
    activity: '',
    cnae: '',
    address: null,
    town: null,
    postal_code: null,
    staff_permanent: null,
    staff_temporary: null,
    micro_criteria: false,
    signers: [],
    memoria_taxpain: false,
  });
  const [signers, setSigners] = useState('');
  const [memoria, setMemoria] = useState<string | null>(null);
  const [done, setDone] = useState<DepositSaved | null>(null);
  const [error, setError] = useState<string | null>(null);
  const text = (key: keyof DepositRequest, label: string, className?: string) => (
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
      setDone(
        await depositBuild(
          {
            ...r,
            year,
            signers: signers
              .split('\n')
              .map((s) => s.trim())
              .filter(Boolean),
          },
          memoria,
          dir,
        ),
      );
    } catch (e) {
      setDone(null);
      setError(errorMessage(e));
    }
  }

  return (
    <section aria-label="Depósito de cuentas" className="mt-8 text-sm">
      <p className="font-medium">Depósito de cuentas {year} (Registro Mercantil)</p>
      <p className="text-stone-600">
        Genera un ZIP con el balance, la cuenta de pérdidas y ganancias (de este año y del anterior)
        y los datos de la empresa para <strong>importarlo en el programa D2</strong> del Colegio de
        Registradores. El D2 lo valida, te deja completar lo que falte (titular real, aplicación del
        resultado…) y genera el fichero definitivo y la huella para la certificación. TaxPain no
        presenta nada.
      </p>
      <div className="form-grid mt-3">
        <label>
          Modelo
          <select
            value={r.model}
            onChange={(e) => setR({ ...r, model: e.target.value as AccountsModel })}
          >
            <option value="Pymes">PYMES</option>
            <option value="Abreviado">Abreviado</option>
            <option value="Normal">Normal</option>
          </select>
        </label>
        <label>
          Forma jurídica
          <select
            value={r.legal_form}
            onChange={(e) =>
              setR({ ...r, legal_form: e.target.value as DepositRequest['legal_form'] })
            }
          >
            <option value="Sl">SL</option>
            <option value="Sa">SA</option>
            <option value="Otra">Otra</option>
          </select>
        </label>
        {text('registry_code', 'Código del Registro (5 dígitos)')}
        {text('cnae', 'CNAE-2025')}
        {text('activity', 'Actividad', 'span-2')}
        {text('staff_permanent', 'Personal medio fijo')}
        {text('staff_temporary', 'No fijo')}
        <label className="flex gap-2">
          <input
            type="checkbox"
            checked={r.micro_criteria}
            onChange={(e) => setR({ ...r, micro_criteria: e.target.checked })}
          />
          Criterios de microempresa
        </label>
      </div>
      <details className="mt-3">
        <summary className="cursor-pointer text-stone-700">
          Domicilio (si no es el de los datos de facturación)
        </summary>
        <div className="form-grid mt-3">
          {text('address', 'Domicilio', 'span-2')}
          {text('town', 'Municipio')}
          {text('postal_code', 'Código postal')}
        </div>
      </details>
      <label className="mt-3 block max-w-xl">
        Administradores que firman (uno por línea)
        <textarea
          className="mt-1 block w-full"
          rows={2}
          value={signers}
          onChange={(e) => setSigners(e.target.value)}
        />
      </label>
      <div className="form-actions mt-4">
        <button
          type="button"
          className="rounded border border-stone-300 px-3 py-1"
          onClick={() => pickPdf().then((p) => p && setMemoria(p))}
        >
          {memoria ? 'Cambiar memoria (PDF)…' : 'Adjuntar memoria (PDF)…'}
        </button>
        {memoria && <span className="text-stone-600">{memoria.split(/[\\/]/).pop()}</span>}
        {!memoria && (
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={r.memoria_taxpain ?? false}
              onChange={(e) => setR({ ...r, memoria_taxpain: e.target.checked })}
            />
            Incluir el borrador de memoria de TaxPain
          </label>
        )}
        <button type="button" className="rounded btn-primary px-3 py-1" onClick={run}>
          Preparar depósito {year}…
        </button>
      </div>
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
      {done && (
        <div role="status" className="mt-2">
          <p>
            Guardado en {done.path} ({done.claves} datos). Ábrelo en el D2 con «Importar» y valida.
          </p>
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

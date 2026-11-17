import { useCallback, useEffect, useState } from 'react';
import {
  errorMessage,
  formatDate,
  type InmuebleArrendado,
  inmuebleGuardar,
  inmuebleQuitar,
  inmueblesList,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const SITUACIONES: [number, string][] = [
  [1, '1 · Con referencia catastral (territorio común)'],
  [2, '2 · País Vasco'],
  [3, '3 · Navarra'],
  [4, '4 · Sin referencia catastral'],
];

const vacio = (nif = ''): InmuebleArrendado => ({
  nif,
  situacion: 1,
  ref_catastral: '',
  via: '',
  numero: '',
  municipio: '',
  provincia: '',
  codigo_postal: '',
  exonerado_hasta: null,
});

/** Arrendadores del borrador del 180 («Nombre · NIF»). */
export function arrendadoresDe(details: [string, number][]): { nif: string; name: string }[] {
  return details.map(([label]) => {
    const i = label.lastIndexOf(' · ');
    return i < 0
      ? { nif: label.trim(), name: '' }
      : { nif: label.slice(i + 3).trim(), name: label.slice(0, i).trim() };
  });
}

/**
 * Inmueble arrendado de cada arrendador (situación, referencia catastral y dirección), que usan
 * el fichero del 180 y su certificado de retenciones, y la acreditación para no retener que
 * mira el 115.
 */
export function InmueblesArrendados({
  arrendadores = [],
  onChange,
}: {
  arrendadores?: { nif: string; name: string }[];
  onChange?: () => void;
}) {
  const [list, setList] = useState<InmuebleArrendado[]>([]);
  const [form, setForm] = useState<InmuebleArrendado | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const load = useCallback(() => {
    inmueblesList()
      .then(setList)
      .catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(load, [load]);

  const sin = arrendadores.filter((a) => !list.some((i) => i.nif === a.nif.toUpperCase()));
  const set = (k: keyof InmuebleArrendado, v: string | number | null) =>
    setForm((f) => (f ? { ...f, [k]: v } : f));

  return (
    <section aria-labelledby="inmuebles-h" className="mt-6">
      <h3 id="inmuebles-h" className="text-base font-semibold">
        Inmuebles arrendados
      </h3>
      <p className="mt-1 text-sm text-stone-600">
        El 180 pide la situación, la referencia catastral y la dirección del inmueble de cada
        arrendador. Con todos guardados, TaxPain genera el fichero y los certificados los llevan.
      </p>
      {error && (
        <p role="alert" className="mt-2 text-sm text-red-700">
          {error}
        </p>
      )}
      {sin.length > 0 && (
        <ul className="mt-2 space-y-1 text-sm">
          {sin.map((a) => (
            <li key={a.nif} className="flex flex-wrap items-center gap-2">
              <span>
                {a.name || a.nif} ({a.nif}): sin inmueble guardado.
              </span>
              <button
                type="button"
                className="btn-ghost rounded px-2 py-0.5"
                onClick={() => setForm(vacio(a.nif))}
              >
                Añadir su inmueble
              </button>
            </li>
          ))}
        </ul>
      )}
      {list.length > 0 && (
        <table className="mt-3 w-full text-sm" aria-label="Inmuebles arrendados guardados">
          <thead>
            <tr className="text-left text-stone-600">
              <th className="py-1">Arrendador</th>
              <th>Inmueble</th>
              <th>Ref. catastral</th>
              <th>Sin retención hasta</th>
              <th>
                <span className="sr-only">Acciones</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {list.map((i) => (
              <tr key={i.nif} className="border-t border-stone-200">
                <td className="py-1">{i.nif}</td>
                <td>
                  {i.via} {i.numero}, {i.codigo_postal} {i.municipio}
                </td>
                <td>{i.ref_catastral || '—'}</td>
                <td>{i.exonerado_hasta ? formatDate(i.exonerado_hasta) : '—'}</td>
                <td className="text-right">
                  <button
                    type="button"
                    className="btn-ghost rounded px-2 py-0.5"
                    aria-label={`Editar el inmueble de ${i.nif}`}
                    onClick={() => setForm({ ...i })}
                  >
                    Editar
                  </button>
                  <ConfirmAction
                    label="Quitar"
                    ariaLabel={`Quitar el inmueble de ${i.nif}`}
                    question={`¿Quitar el inmueble de ${i.nif}? El 180 volverá a salir sin fichero.`}
                    confirmLabel="Quitar"
                    onConfirm={async () => {
                      await inmuebleQuitar(i.nif, true);
                      load();
                      onChange?.();
                    }}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {!form && (
        <button
          type="button"
          className="btn-ghost mt-2 rounded px-3 py-1 text-sm"
          onClick={() => setForm(vacio())}
        >
          Añadir un inmueble
        </button>
      )}
      {form && (
        <form
          aria-label="Datos del inmueble"
          className="mt-3 grid gap-2 rounded border border-stone-200 p-3 text-sm sm:grid-cols-2"
          onSubmit={async (e) => {
            e.preventDefault();
            setError(null);
            try {
              await inmuebleGuardar(form);
              setMsg(`Inmueble de ${form.nif.toUpperCase()} guardado.`);
              setForm(null);
              load();
              onChange?.();
            } catch (err) {
              setError(errorMessage(err));
            }
          }}
        >
          <label className="flex flex-col">
            NIF del arrendador
            <input value={form.nif} onChange={(e) => set('nif', e.target.value)} required />
          </label>
          <label className="flex flex-col">
            Situación
            <select
              value={form.situacion}
              onChange={(e) => set('situacion', Number(e.target.value))}
            >
              {SITUACIONES.map(([v, l]) => (
                <option key={v} value={v}>
                  {l}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col">
            Referencia catastral (20 caracteres)
            <input
              value={form.ref_catastral}
              maxLength={20}
              onChange={(e) => set('ref_catastral', e.target.value)}
            />
          </label>
          <label className="flex flex-col">
            Vía
            <input value={form.via} onChange={(e) => set('via', e.target.value)} />
          </label>
          <label className="flex flex-col">
            Número
            <input value={form.numero} onChange={(e) => set('numero', e.target.value)} />
          </label>
          <label className="flex flex-col">
            Municipio
            <input value={form.municipio} onChange={(e) => set('municipio', e.target.value)} />
          </label>
          <label className="flex flex-col">
            Código de provincia
            <input
              value={form.provincia}
              maxLength={2}
              inputMode="numeric"
              onChange={(e) => set('provincia', e.target.value)}
            />
          </label>
          <label className="flex flex-col">
            Código postal
            <input
              value={form.codigo_postal}
              maxLength={5}
              inputMode="numeric"
              onChange={(e) => set('codigo_postal', e.target.value)}
            />
          </label>
          <label className="flex flex-col sm:col-span-2">
            Acreditación del arrendador para no retener válida hasta (art. 75.3.g RIRPF; vacío si no
            la hay)
            <input
              type="date"
              value={form.exonerado_hasta ?? ''}
              onChange={(e) => set('exonerado_hasta', e.target.value || null)}
            />
          </label>
          <div className="form-actions sm:col-span-2">
            <button type="submit" className="btn-primary rounded px-3 py-1.5">
              Guardar inmueble
            </button>
            <button
              type="button"
              className="btn-ghost rounded px-3 py-1.5"
              onClick={() => setForm(null)}
            >
              Cancelar
            </button>
          </div>
        </form>
      )}
      {msg && (
        <p role="status" className="mt-2 text-sm">
          {msg}
        </p>
      )}
    </section>
  );
}

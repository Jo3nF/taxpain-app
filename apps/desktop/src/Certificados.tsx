import { useState } from 'react';
import {
  type CertificateOptions,
  type CertificatesPrep,
  errorMessage,
  euros,
  modelCertificateEmail,
  modelCertificatesPrepare,
  modelCertificatesSave,
  pickFolder,
} from './api';

const TITLES: Record<string, string> = {
  '190': 'trabajadores, administradores y profesionales',
  '180': 'arrendadores',
  '193': 'socios que cobran dividendos',
  '296': 'no residentes (rentas sin establecimiento permanente)',
};

function todayIso(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/**
 * Certificados de retenciones para los perceptores del 190, 180 o 193 (art. 108.3 RIRPF): una
 * hoja por perceptor con las cifras del borrador, guardados en una carpeta o preparados como
 * correo sin enviar.
 */
export function Certificados({ model, year }: { model: string; year: number }) {
  const [prep, setPrep] = useState<CertificatesPrep | null>(null);
  const [open, setOpen] = useState(false);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [place, setPlace] = useState('');
  const [date, setDate] = useState(todayIso());
  const [signer, setSigner] = useState('');
  const [role, setRole] = useState('');
  const [properties, setProperties] = useState<Record<string, string>>({});
  const [each, setEach] = useState(true);
  const [combined, setCombined] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = async () => {
    setMsg(null);
    try {
      const p = await modelCertificatesPrepare(model, year);
      setPrep(p);
      setChosen(new Set(p.payees.map((x) => x.nif)));
      setPlace(p.place);
      setSigner(p.signer);
      setRole(p.signer_role);
      setOpen(true);
    } catch (e) {
      setMsg(errorMessage(e));
    }
  };

  const options = (nifs: string[]): CertificateOptions => ({
    place,
    date,
    signer,
    signer_role: role,
    nifs,
    properties,
  });

  const toggle = (nif: string) =>
    setChosen((s) => {
      const n = new Set(s);
      if (n.has(nif)) n.delete(nif);
      else n.add(nif);
      return n;
    });

  const save = async () => {
    if (chosen.size === 0) {
      setMsg('Elige al menos un perceptor.');
      return;
    }
    const dir = await pickFolder();
    if (!dir) return;
    setBusy(true);
    try {
      const paths = await modelCertificatesSave(
        model,
        year,
        options([...chosen]),
        dir,
        each,
        combined,
      );
      setMsg(
        paths.length === 1 ? `Guardado en ${paths[0]}` : `${paths.length} PDF guardados en ${dir}`,
      );
    } catch (e) {
      setMsg(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const email = async (nif: string, to: string | null) => {
    try {
      await modelCertificateEmail(model, year, options([nif]), nif, to);
      setMsg('Correo preparado en tu programa de correo, sin enviar: revísalo y envíalo tú.');
    } catch (e) {
      setMsg(errorMessage(e));
    }
  };

  return (
    <section
      aria-label="Certificados para los perceptores"
      className="mt-4 rounded-lg border bg-white p-3 text-sm"
    >
      <h3 className="font-medium">Certificados de retenciones</h3>
      <p className="text-stone-600">
        Un certificado por perceptor ({TITLES[model] ?? 'perceptores'}) con lo que declara este
        borrador, para que lo usen en su declaración. Hay que entregarlo antes de que empiece la
        campaña de la Renta (art. 108.3 del Reglamento del IRPF).
      </p>
      {!open && (
        <button type="button" className="mt-2 rounded border border-stone-300 px-3" onClick={load}>
          Certificados para los perceptores…
        </button>
      )}
      {open && prep && (
        <div className="mt-3 space-y-3">
          {prep.notes.map((n) => (
            <p key={n} className="text-amber-800">
              {n}
            </p>
          ))}
          {prep.payees.length === 0 ? (
            <p>El borrador no tiene perceptores.</p>
          ) : (
            <table className="w-full text-left">
              <thead>
                <tr className="text-stone-500">
                  <th scope="col">Incluir</th>
                  <th scope="col">Perceptor</th>
                  <th scope="col">Concepto</th>
                  <th scope="col" className="text-right">
                    Íntegro
                  </th>
                  <th scope="col" className="text-right">
                    Retenido
                  </th>
                  {model === '180' && <th scope="col">Inmueble</th>}
                  <th scope="col">Correo</th>
                </tr>
              </thead>
              <tbody>
                {prep.payees.map((p) => (
                  <tr key={p.nif} className="border-t">
                    <td>
                      <input
                        type="checkbox"
                        aria-label={`Incluir a ${p.name}`}
                        checked={chosen.has(p.nif)}
                        onChange={() => toggle(p.nif)}
                      />
                    </td>
                    <td>
                      {p.name} <span className="text-stone-500">{p.nif}</span>
                    </td>
                    <td>{p.concept}</td>
                    <td className="text-right tabular-nums">{euros(p.gross)}</td>
                    <td className="text-right tabular-nums">{euros(p.withheld)}</td>
                    {model === '180' && (
                      <td>
                        <input
                          className="w-full"
                          aria-label={`Inmueble de ${p.name}`}
                          placeholder="Dirección y referencia catastral"
                          value={properties[p.nif] ?? ''}
                          onChange={(e) =>
                            setProperties((m) => ({ ...m, [p.nif]: e.target.value }))
                          }
                        />
                      </td>
                    )}
                    <td>
                      <button
                        type="button"
                        className="rounded border border-stone-300 px-2"
                        aria-label={`Preparar correo a ${p.name}`}
                        onClick={() => email(p.nif, p.email)}
                      >
                        Preparar correo
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <div className="flex flex-wrap items-end gap-3">
            <label>
              Lugar de expedición
              <input
                className="mt-1 block w-48"
                value={place}
                onChange={(e) => setPlace(e.target.value)}
              />
            </label>
            <label>
              Fecha de expedición
              <input
                type="date"
                className="mt-1 block"
                value={date}
                onChange={(e) => setDate(e.target.value)}
              />
            </label>
            <label>
              Firma (nombre)
              <input
                className="mt-1 block w-64"
                value={signer}
                onChange={(e) => setSigner(e.target.value)}
              />
            </label>
            <label>
              Cargo
              <input
                className="mt-1 block w-48"
                value={role}
                onChange={(e) => setRole(e.target.value)}
              />
            </label>
          </div>
          <div className="flex flex-wrap items-center gap-4">
            <label className="flex items-center gap-1">
              <input type="checkbox" checked={each} onChange={(e) => setEach(e.target.checked)} />
              Un PDF por perceptor
            </label>
            <label className="flex items-center gap-1">
              <input
                type="checkbox"
                checked={combined}
                onChange={(e) => setCombined(e.target.checked)}
              />
              Un PDF con todos
            </label>
            <button
              type="button"
              className="rounded border border-stone-300 px-3"
              disabled={busy || prep.payees.length === 0 || (!each && !combined)}
              onClick={save}
            >
              Guardar los certificados…
            </button>
          </div>
        </div>
      )}
      {msg && <p role="status">{msg}</p>}
    </section>
  );
}

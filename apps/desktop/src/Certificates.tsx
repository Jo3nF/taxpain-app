import { IdCard } from 'lucide-react';
import { type FormEvent, useCallback, useEffect, useState } from 'react';
import {
  type CertificateInfo,
  daysUntil,
  deleteCertificate,
  errorMessage,
  formatDate,
  importCertificate,
  isTauri,
  listCertificates,
  pickCertificateFile,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const KIND_LABEL: Record<CertificateInfo['kind'], string> = {
  persona_fisica: 'Persona física',
  representante_pj: 'Representante de persona jurídica',
  otro: 'Otro',
};

function expiry(days: number): string {
  if (days < 0) return 'caducado';
  if (days === 0) return 'queda menos de un día';
  return days === 1 ? 'queda 1 día' : `quedan ${days} días`;
}

function CertificateCard({
  cert,
  onDelete,
}: {
  cert: CertificateInfo;
  onDelete: (fp: string) => void;
}) {
  const days = daysUntil(cert.valid_to);
  return (
    <li className="rounded-lg border border-stone-200 bg-white p-4">
      <div className="flex items-start justify-between gap-4">
        <div>
          <p className="font-medium">{cert.holder_name ?? cert.holder_nif}</p>
          <p className="text-sm text-stone-600">
            {KIND_LABEL[cert.kind]} · NIF {cert.holder_nif}
            {cert.entity_nif && ` · ${cert.entity_name ?? ''} (${cert.entity_nif})`}
          </p>
          <p className={`mt-1 text-sm ${days < 30 ? 'text-amber-700' : 'text-stone-500'}`}>
            Caduca el {formatDate(cert.valid_to.slice(0, 10))}
            {days < 30 && ` (${expiry(days)})`} ·{' '}
            {cert.issuer_kind === 'fnmt' ? 'FNMT' : 'Otra autoridad'} · {cert.key_algorithm}
          </p>
        </div>
        <ConfirmAction
          className="text-sm text-red-700 hover:underline"
          label="Eliminar"
          question="¿Eliminar este certificado de TaxPain? Para volver a usarlo necesitarás el fichero .p12 y su contraseña."
          confirmLabel="Sí, eliminar"
          onConfirm={() => onDelete(cert.fingerprint_sha256)}
        />
      </div>
    </li>
  );
}

export function Certificates() {
  const [certs, setCerts] = useState<CertificateInfo[]>([]);
  const [path, setPath] = useState<string | null>(null);
  const [passphrase, setPassphrase] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const available = isTauri();

  const refresh = useCallback(() => {
    listCertificates()
      .then(setCerts)
      .catch((e) => setError(errorMessage(e)));
  }, []);

  useEffect(refresh, [refresh]);

  async function choose() {
    setError(null);
    try {
      setPath(await pickCertificateFile());
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!path) return;
    setBusy(true);
    setError(null);
    try {
      await importCertificate(path, passphrase);
      setPath(null);
      refresh();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setPassphrase('');
      setBusy(false);
    }
  }

  async function remove(fingerprint: string) {
    setError(null);
    try {
      await deleteCertificate(fingerprint);
      refresh();
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  return (
    <section aria-labelledby="certs-title" className="mt-8">
      <div className="flex items-start gap-3">
        <span className="icon-tile" aria-hidden="true">
          <IdCard size={18} />
        </span>
        <div>
          <h2 id="certs-title" className="text-lg font-semibold">
            Certificado electrónico
          </h2>
          <p className="text-sm text-stone-600">
            Tu certificado se guarda cifrado en este Mac y nunca sale de él. Cada uso te pedirá
            Touch ID.
          </p>
        </div>
      </div>

      {certs.length > 0 && (
        <ul className="mt-4 space-y-3">
          {certs.map((c) => (
            <CertificateCard key={c.fingerprint_sha256} cert={c} onDelete={remove} />
          ))}
        </ul>
      )}

      {!available ? (
        <p className="mt-4 text-sm text-stone-500">Disponible en la app de escritorio.</p>
      ) : path === null ? (
        <button
          type="button"
          onClick={choose}
          className="mt-4 rounded-md btn-primary px-4 py-2 text-sm font-medium"
        >
          Importar certificado (.p12 / .pfx)
        </button>
      ) : (
        <form onSubmit={submit} className="mt-4 space-y-3 rounded-lg border border-stone-200 p-4">
          <p className="text-sm text-stone-700">{path.split(/[\\/]/).pop()}</p>
          <div className="form-grid">
            <label className="span-2">
              Contraseña del certificado
              <input
                type="password"
                autoComplete="off"
                value={passphrase}
                onChange={(e) => setPassphrase(e.target.value)}
              />
            </label>
          </div>
          <div className="form-actions">
            <button
              type="submit"
              disabled={busy || passphrase.length === 0}
              className="rounded-md btn-primary px-4 py-2 text-sm font-medium disabled:opacity-50"
            >
              {busy ? 'Comprobando…' : 'Importar'}
            </button>
            <button type="button" onClick={() => setPath(null)} className="btn-ghost">
              Cancelar
            </button>
          </div>
        </form>
      )}

      {error && (
        <p role="alert" className="mt-3 text-sm text-red-700">
          {error}
        </p>
      )}
    </section>
  );
}

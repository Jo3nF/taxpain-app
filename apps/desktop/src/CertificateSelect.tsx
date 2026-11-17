import { useEffect, useState } from 'react';
import { type CertificateInfo, listCertificates } from './api';

/**
 * Cómo se nombra un certificado en los selectores: primero la entidad representada (con su NIF),
 * para distinguir el de la sociedad del personal del administrador.
 */
export function certLabel(c: CertificateInfo): string {
  return `${c.entity_name ?? c.holder_name ?? c.subject_dn} · ${c.entity_nif ?? c.holder_nif}`;
}

/**
 * Selector del certificado con el que se actúa ante la AEAT. Con varios certificados (persona
 * física y representante de una sociedad), cada acción debe poder elegir cuál usar.
 * Devuelve la huella elegida (o '' si no hay ninguno) por `onChange`.
 *
 * Con `optional`, el certificado no hace falta: se puede elegir «Sin certificado» (la opción de
 * entrada, valor '') y el selector sale aunque sólo haya uno; sin certificados, no sale nada.
 */
export function CertificateSelect({
  value,
  onChange,
  optional = false,
}: {
  value: string;
  onChange: (fingerprint: string) => void;
  optional?: boolean;
}) {
  const [certs, setCerts] = useState<CertificateInfo[] | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: sólo al montar (elige el primero)
  useEffect(() => {
    Promise.resolve()
      .then(listCertificates)
      .then((c = []) => {
        setCerts(c);
        if (c[0] && !optional) onChange(c[0].fingerprint_sha256);
      })
      .catch(() => setCerts([]));
  }, []);
  if (certs === null) return null;
  if (certs.length === 0) {
    if (optional) return null;
    return <p className="text-sm text-amber-800">Importa primero tu certificado en Ajustes.</p>;
  }
  if (certs.length === 1 && !optional) return null;
  return (
    <label className="field-inline text-sm">
      {optional ? 'Certificado (opcional)' : 'Certificado'}
      <select value={value} onChange={(e) => onChange(e.target.value)}>
        {optional && <option value="">Sin certificado (sólo la Sede pública)</option>}
        {certs.map((c) => (
          <option key={c.fingerprint_sha256} value={c.fingerprint_sha256}>
            {certLabel(c)}
          </option>
        ))}
      </select>
    </label>
  );
}

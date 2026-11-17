import { CircleCheck, CircleMinus, Copy } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { type CertificateInfo, errorMessage, listCertificates, openSede } from './api';
import type { Navegar } from './CierreTrimestre';
import {
  type EstadoPasoGuia,
  type Guia,
  type PasoGuia,
  puestaMarcha,
  puestaMarchaMarcar,
} from './trimestreApi';

function abrir(onGo: Navegar | undefined, destino: string) {
  const [tab, view] = destino.split(':');
  onGo?.(tab ?? destino, view);
}

function Correo({ guia }: { guia: Guia }) {
  const [copiado, setCopiado] = useState(false);
  const texto = `Asunto: ${guia.correo.asunto}\n\n${guia.correo.cuerpo}`;
  return (
    <div className="mt-2 rounded-lg border border-stone-200 bg-stone-50 p-3">
      <p className="font-medium">{guia.correo.asunto}</p>
      <textarea
        aria-label="Texto del correo a la gestoría"
        readOnly
        rows={12}
        value={guia.correo.cuerpo}
        className="mt-2 w-full rounded border border-stone-300 bg-white p-2 font-mono text-xs"
      />
      <button
        type="button"
        className="mt-2 inline-flex items-center gap-1 rounded border border-stone-300 px-3 py-1"
        onClick={() =>
          navigator.clipboard
            ?.writeText(texto)
            .then(() => setCopiado(true))
            .catch(() => setCopiado(false))
        }
      >
        <Copy size={14} aria-hidden="true" /> Copiar el correo
      </button>
      {copiado && (
        <span role="status" className="ml-2 text-emerald-700">
          Copiado. Pégalo en tu correo y envíalo tú.
        </span>
      )}
    </div>
  );
}

function Paso({
  n,
  p,
  guia,
  certificados,
  onGo,
  onGuardar,
}: {
  n: number;
  p: PasoGuia;
  guia: Guia;
  certificados: CertificateInfo[] | null;
  onGo?: Navegar | undefined;
  onGuardar: (p: PasoGuia, estado: EstadoPasoGuia, fecha: string | null) => void;
}) {
  const detectado =
    p.id === 'certificado' && certificados && certificados.length > 0
      ? `Hay ${certificados.length === 1 ? 'un certificado instalado' : `${certificados.length} certificados instalados`}.`
      : p.detectado;
  const [fecha, setFecha] = useState(p.fecha ?? '');
  return (
    <li className="card p-4">
      <div className="flex items-start gap-2">
        {p.estado === 'hecho' ? (
          <CircleCheck size={18} className="mt-0.5 text-emerald-600" aria-hidden="true" />
        ) : p.estado === 'no_aplica' ? (
          <CircleMinus size={18} className="mt-0.5 text-stone-400" aria-hidden="true" />
        ) : (
          <span className="count mt-0.5" aria-hidden="true">
            {n}
          </span>
        )}
        <div className="flex-1">
          <h3 className="font-semibold">
            {p.titulo}
            <span className="sr-only">
              {' '}
              (
              {p.estado === 'hecho'
                ? 'hecho'
                : p.estado === 'no_aplica'
                  ? 'no aplica'
                  : 'pendiente'}
              )
            </span>
          </h3>
          <p className="mt-1 text-stone-700">{p.texto}</p>
          {detectado && p.estado === 'pendiente' && (
            <p className="mt-1 text-emerald-800">TaxPain ve que: {detectado}</p>
          )}
          {p.id === 'pedir' && <Correo guia={guia} />}
          {p.id === 'saldos' && (
            <label className="mt-2 flex items-center gap-2">
              Fecha de corte
              <input
                type="date"
                value={fecha}
                onChange={(e) => setFecha(e.target.value)}
                onBlur={() => fecha !== (p.fecha ?? '') && onGuardar(p, p.estado, fecha || null)}
                className="rounded border border-stone-300 px-2 py-1"
              />
            </label>
          )}
          <div className="mt-2 flex flex-wrap gap-3">
            {p.destino && onGo && (
              <button
                type="button"
                className="text-link underline"
                onClick={() => abrir(onGo, p.destino ?? '')}
              >
                Ir: {p.titulo}
              </button>
            )}
            {p.enlaces.map((e) => (
              <button
                key={e.url}
                type="button"
                className="text-link underline"
                onClick={() => openSede(e.url).catch(() => undefined)}
              >
                {e.label}
              </button>
            ))}
          </div>
          <fieldset className="mt-3 flex gap-2 border-0 p-0">
            <legend className="sr-only">Estado: {p.titulo}</legend>
            {(
              [
                ['pendiente', 'Pendiente'],
                ['hecho', 'Hecho'],
                ['no_aplica', 'No aplica'],
              ] as [EstadoPasoGuia, string][]
            ).map(([e, label]) => (
              <button
                key={e}
                type="button"
                aria-pressed={p.estado === e}
                className={`rounded border px-2 py-0.5 text-xs ${p.estado === e ? 'border-stone-800 bg-stone-100' : 'border-stone-300'}`}
                onClick={() => onGuardar(p, e, fecha || null)}
              >
                {label}
              </button>
            ))}
          </fieldset>
        </div>
      </div>
    </li>
  );
}

/**
 * Guía «Dejar la gestoría»: los pasos para empezar a llevar tú la empresa, con su estado
 * guardado. Ordena lo que ya hay en la app y explica lo que se hace fuera (la Sede, el correo a
 * la gestoría). No envía ni revoca nada.
 */
export function PuestaEnMarcha({ onGo }: { onGo?: Navegar }) {
  const [guia, setGuia] = useState<Guia | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [certificados, setCertificados] = useState<CertificateInfo[] | null>(null);

  const cargar = useCallback(() => {
    puestaMarcha()
      .then(setGuia)
      .catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(() => {
    cargar();
    listCertificates()
      .then(setCertificados)
      .catch(() => setCertificados(null));
  }, [cargar]);

  const guardar = (p: PasoGuia, estado: EstadoPasoGuia, fecha: string | null) => {
    setError(null);
    puestaMarchaMarcar(p.id, estado, fecha, p.nota)
      .then(cargar)
      .catch((e) => setError(errorMessage(e)));
  };

  if (error && !guia)
    return (
      <p role="alert" className="mt-6 text-sm text-red-700">
        {error}
      </p>
    );
  if (!guia) return null;
  const total = guia.pasos.length;
  return (
    <section aria-labelledby="guia-t" className="mt-6 text-sm">
      <h2 id="guia-t" className="text-lg font-semibold">
        Dejar la gestoría: puesta en marcha
      </h2>
      <p className="mt-1 text-stone-600">
        Si vienes de una gestoría, sigue estos pasos en orden. Marca cada uno cuando lo tengas (o si
        no te aplica): se guarda y puedes seguir otro día.
      </p>
      <p className="mt-3 font-medium" id="guia-progreso">
        {guia.resueltos} de {total} pasos resueltos
      </p>
      <progress
        aria-labelledby="guia-progreso"
        max={total}
        value={guia.resueltos}
        className="mt-1 w-full"
      />
      {error && (
        <p role="alert" className="mt-3 rounded-md bg-red-50 p-3 text-red-900">
          {error}
        </p>
      )}
      <ol className="mt-4 space-y-3">
        {guia.pasos.map((p, i) => (
          <Paso
            key={`${p.id}-${p.estado}-${p.fecha ?? ''}`}
            n={i + 1}
            p={p}
            guia={guia}
            certificados={certificados}
            onGo={onGo}
            onGuardar={guardar}
          />
        ))}
      </ol>
    </section>
  );
}

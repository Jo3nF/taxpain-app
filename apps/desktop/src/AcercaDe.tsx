import { useEffect, useState } from 'react';
import {
  type DeclaracionResponsable,
  declaracionResponsable,
  declaracionResponsableSave,
  errorMessage,
  hayFacturacion,
} from './api';

/** Ajustes › Acerca de TaxPain: la declaración responsable del sistema informático de
 * facturación de esta versión, que tiene que verse en el propio programa (RD 1007/2023 art.
 * 13.2; Orden HAC/1177/2024 art. 15). */
export function AcercaDe() {
  const [d, setD] = useState<DeclaracionResponsable | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  useEffect(() => {
    declaracionResponsable()
      .then(setD)
      .catch((e) => setMessage(errorMessage(e)));
  }, []);
  return (
    <section className="mt-8" aria-labelledby="acerca-h">
      <h2 id="acerca-h" className="text-lg font-semibold">
        Acerca de TaxPain
      </h2>
      {!hayFacturacion() && (
        <p className="mt-1 text-sm text-stone-600">
          Esta versión de TaxPain no emite facturas, así que no es un sistema de facturación y no
          lleva declaración responsable de VERI*FACTU. Quien compila TaxPain desde su código con la
          facturación activada es su productor y firma la suya.
        </p>
      )}
      {d && hayFacturacion() && (
        <>
          <p className="mt-1 text-sm text-stone-600">
            TaxPain {d.version}. Como programa de facturación, su productor declara que cumple el
            reglamento de sistemas de facturación (VERI*FACTU). Ésta es la declaración de esta
            versión:
          </p>
          <article
            aria-labelledby="declaracion-h"
            className="mt-3 rounded-md border border-stone-300 bg-white p-4 text-sm"
          >
            <h3 id="declaracion-h" className="font-semibold">
              {d.titulo}
            </h3>
            <dl className="mt-2 space-y-2">
              {d.apartados.map((a) => (
                <div key={a.letra}>
                  <dt className="font-medium">
                    {a.letra}) {a.etiqueta}
                  </dt>
                  <dd className="text-stone-700">{a.valor}</dd>
                </div>
              ))}
            </dl>
            {d.anexo.length > 0 && (
              <div className="mt-3 text-stone-600">
                {d.anexo.map((t) => (
                  <p key={t} className="mt-1">
                    {t}
                  </p>
                ))}
              </div>
            )}
          </article>
          {d.faltan.length > 0 && (
            <p className="mt-2 text-sm text-amber-800">
              Pendiente de firmar: falta {d.faltan.join(', ')}.
            </p>
          )}
          <button
            type="button"
            className="mt-2 rounded border border-stone-300 px-2 py-1 text-sm"
            onClick={async () => {
              try {
                const p = await declaracionResponsableSave(d.version);
                if (p) setMessage('Declaración guardada en PDF.');
              } catch (e) {
                setMessage(errorMessage(e));
              }
            }}
          >
            Guardar en PDF…
          </button>
        </>
      )}
      {message && (
        <p role="status" className="mt-2 text-sm text-stone-700">
          {message}
        </p>
      )}
    </section>
  );
}

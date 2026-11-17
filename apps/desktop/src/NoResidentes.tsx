import { useCallback, useEffect, useState } from 'react';
import {
  type CandidataIrnr,
  type CertificadoResidencia,
  type ExencionIrnr,
  errorMessage,
  euros,
  formatDate,
  irnrApuntar,
  irnrGuardarCertificado,
  irnrProponer,
  irnrQuitarCertificado,
  irnrQuitarRenta,
  irnrResumen,
  type NaturalezaIrnr,
  type NuevaRentaIrnr,
  type PropuestaIrnr,
  parseEuros,
  type ResumenIrnr,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const NATURALEZAS: [NaturalezaIrnr, string][] = [
  ['interes', 'Intereses'],
  ['canon', 'Cánones (licencias, software con cesión de derechos, know-how)'],
  ['servicio', 'Prestaciones de servicios utilizadas en España'],
  ['otros', 'Otras rentas'],
];

const NATURALEZA: Record<NaturalezaIrnr, string> = {
  dividendo: 'Dividendos',
  interes: 'Intereses',
  canon: 'Cánones',
  servicio: 'Servicios',
  otros: 'Otras',
};

const EXENCIONES: [ExencionIrnr | '', string][] = [
  ['', 'Ninguna'],
  ['art14_1_c', 'Intereses de residentes en la UE (art. 14.1.c)'],
  ['art14_1_h', 'Dividendos a la matriz de la UE (art. 14.1.h)'],
  ['art14_1_m', 'Cánones a sociedad asociada de la UE (art. 14.1.m)'],
  ['otra', 'Otra del art. 14 TRLIRNR'],
];

const REGIMEN: Record<string, string> = {
  general: 'Tipo general',
  convenio: 'Convenio',
  exenta: 'Exenta',
};

function pct(bp: number): string {
  return `${(bp / 100).toLocaleString('es-ES', { maximumFractionDigits: 2 })} %`;
}

function hoy(): string {
  return new Date().toISOString().slice(0, 10);
}

const certVacio = (): CertificadoResidencia => ({
  id: 0,
  nif: '',
  nombre: '',
  pais: '',
  expedido: hoy(),
  valido_hasta: null,
  convenio: true,
  documento: '',
  notas: '',
});

const rentaVacia = (): NuevaRentaIrnr => ({
  fecha: hoy(),
  nif: '',
  nombre: '',
  pais: '',
  naturaleza: 'canon',
  bruto: 0,
  tipo_convenio: null,
  exencion: null,
  concepto: '',
  recibida_id: null,
});

/**
 * Rentas pagadas a no residentes sin establecimiento permanente (216 y 296): certificados de
 * residencia, rentas apuntadas con el tipo propuesto, dividendos a socios no residentes y
 * facturas de proveedores extranjeros por revisar.
 */
export function NoResidentes({ year, onChange }: { year: number; onChange?: () => void }) {
  const [r, setR] = useState<ResumenIrnr | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [cert, setCert] = useState<CertificadoResidencia | null>(null);
  const [renta, setRenta] = useState<NuevaRentaIrnr | null>(null);
  const [bruto, setBruto] = useState('');
  const [tipo, setTipo] = useState('');
  const [propuesta, setPropuesta] = useState<PropuestaIrnr | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const load = useCallback(() => {
    irnrResumen(year)
      .then(setR)
      .catch((e) => setError(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);

  const changed = () => {
    load();
    onChange?.();
  };

  const nueva = (): NuevaRentaIrnr | null => {
    if (!renta) return null;
    const b = parseEuros(bruto);
    const t = tipo.trim() ? parseEuros(tipo.replace('%', '')) : null;
    return { ...renta, bruto: b ?? 0, tipo_convenio: t };
  };

  const desde = (c: CandidataIrnr) => {
    setRenta({
      ...rentaVacia(),
      fecha: c.fecha,
      nif: c.nif,
      nombre: c.nombre,
      pais: c.pais,
      concepto: `Factura ${c.numero}`,
      recibida_id: c.recibida_id,
    });
    setBruto(euros(c.base).replace(' €', ''));
    setTipo('');
    setPropuesta(null);
  };

  return (
    <section aria-labelledby="irnr-h" className="mt-6 space-y-5">
      <div>
        <h3 id="irnr-h" className="text-base font-semibold">
          Rentas pagadas a no residentes ({year})
        </h3>
        <p className="mt-1 text-sm text-stone-600">
          Intereses, cánones y servicios pagados a quien no reside en España y no tiene aquí
          establecimiento permanente. El tipo general es el 24 % (19 % en la UE y el EEE) o el 19 %
          en dividendos e intereses; con certificado de residencia vigente se aplica el tipo del
          convenio o la exención. Los dividendos a socios no residentes salen de Socios.
        </p>
        {error && (
          <p role="alert" className="mt-2 text-sm text-red-700">
            {error}
          </p>
        )}
        {msg && (
          <p role="status" className="mt-2 text-sm">
            {msg}
          </p>
        )}
      </div>

      {r && (
        <div>
          <h4 className="font-medium">Certificados de residencia fiscal</h4>
          {r.certificados.length === 0 ? (
            <p className="text-sm text-stone-600">Ninguno guardado.</p>
          ) : (
            <table className="mt-1 w-full text-sm" aria-label="Certificados de residencia">
              <thead>
                <tr className="text-left text-stone-600">
                  <th className="py-1">Perceptor</th>
                  <th>País</th>
                  <th>Expedido</th>
                  <th>Válido hasta</th>
                  <th>Estado</th>
                  <th>
                    <span className="sr-only">Acciones</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {r.certificados.map((c) => (
                  <tr key={c.id} className="border-t border-stone-200">
                    <td className="py-1">
                      {c.nombre} · {c.nif}
                    </td>
                    <td>{c.pais}</td>
                    <td>{formatDate(c.expedido)}</td>
                    <td>{formatDate(c.valido_hasta)}</td>
                    <td>
                      {c.vigente ? (c.caduca_pronto ? 'Caduca pronto' : 'Vigente') : 'Caducado'}
                    </td>
                    <td className="text-right">
                      <ConfirmAction
                        label="Quitar"
                        ariaLabel={`Quitar el certificado de ${c.nombre}`}
                        question={`¿Quitar el certificado de residencia de ${c.nombre}? Las rentas nuevas irán al tipo general.`}
                        confirmLabel="Quitar"
                        onConfirm={async () => {
                          await irnrQuitarCertificado(c.id, true);
                          changed();
                        }}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {!cert ? (
            <button
              type="button"
              className="btn-ghost mt-2 rounded px-3 py-1 text-sm"
              onClick={() => setCert(certVacio())}
            >
              Añadir un certificado
            </button>
          ) : (
            <form
              aria-label="Certificado de residencia"
              className="mt-2 grid gap-2 rounded border border-stone-200 p-3 text-sm sm:grid-cols-2"
              onSubmit={async (e) => {
                e.preventDefault();
                setError(null);
                try {
                  await irnrGuardarCertificado(cert);
                  setMsg(`Certificado de ${cert.nombre} guardado.`);
                  setCert(null);
                  changed();
                } catch (err) {
                  setError(errorMessage(err));
                }
              }}
            >
              <label className="flex flex-col">
                Identificador fiscal del perceptor
                <input
                  value={cert.nif}
                  required
                  onChange={(e) => setCert({ ...cert, nif: e.target.value })}
                />
              </label>
              <label className="flex flex-col">
                Nombre o razón social
                <input
                  value={cert.nombre}
                  required
                  onChange={(e) => setCert({ ...cert, nombre: e.target.value })}
                />
              </label>
              <label className="flex flex-col">
                País de residencia (código de dos letras)
                <input
                  value={cert.pais}
                  maxLength={2}
                  required
                  onChange={(e) => setCert({ ...cert, pais: e.target.value.toUpperCase() })}
                />
              </label>
              <label className="flex flex-col">
                Fecha de expedición
                <input
                  type="date"
                  value={cert.expedido}
                  required
                  onChange={(e) => setCert({ ...cert, expedido: e.target.value })}
                />
              </label>
              <label className="flex flex-col">
                Válido hasta (vacío: un año)
                <input
                  type="date"
                  value={cert.valido_hasta ?? ''}
                  onChange={(e) => setCert({ ...cert, valido_hasta: e.target.value || null })}
                />
              </label>
              <label className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={cert.convenio}
                  onChange={(e) => setCert({ ...cert, convenio: e.target.checked })}
                />
                Expedido «a efectos del convenio»
              </label>
              <label className="flex flex-col sm:col-span-2">
                Documento (nombre del fichero o referencia)
                <input
                  value={cert.documento}
                  onChange={(e) => setCert({ ...cert, documento: e.target.value })}
                />
              </label>
              <div className="form-actions sm:col-span-2">
                <button type="submit" className="btn-primary rounded px-3 py-1.5">
                  Guardar certificado
                </button>
                <button
                  type="button"
                  className="btn-ghost rounded px-3 py-1.5"
                  onClick={() => setCert(null)}
                >
                  Cancelar
                </button>
              </div>
            </form>
          )}
        </div>
      )}

      {r && (
        <div>
          <h4 className="font-medium">Rentas apuntadas</h4>
          {r.rentas.length === 0 && r.dividendos.length === 0 ? (
            <p className="text-sm text-stone-600">Ninguna renta a no residentes en {year}.</p>
          ) : (
            <table className="mt-1 w-full text-sm" aria-label="Rentas a no residentes">
              <thead>
                <tr className="text-left text-stone-600">
                  <th className="py-1">Fecha</th>
                  <th>Perceptor</th>
                  <th>Naturaleza</th>
                  <th>Régimen</th>
                  <th className="text-right">Íntegro</th>
                  <th className="text-right">Tipo</th>
                  <th className="text-right">Retención</th>
                  <th>
                    <span className="sr-only">Acciones</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {r.dividendos.map((d) => (
                  <tr key={`div-${d.fecha}-${d.nif}`} className="border-t border-stone-200">
                    <td className="py-1">{formatDate(d.fecha)}</td>
                    <td>
                      {d.nombre} · {d.pais || '—'}
                    </td>
                    <td>{NATURALEZA[d.naturaleza]} (Socios)</td>
                    <td>{REGIMEN[d.regimen]}</td>
                    <td className="text-right tabular-nums">{euros(d.bruto)}</td>
                    <td className="text-right">{pct(d.tipo)}</td>
                    <td className="text-right tabular-nums">{euros(d.retencion)}</td>
                    <td />
                  </tr>
                ))}
                {r.rentas.map((x) => (
                  <tr key={x.id} className="border-t border-stone-200">
                    <td className="py-1">{formatDate(x.fecha)}</td>
                    <td>
                      {x.nombre} · {x.pais}
                    </td>
                    <td>{NATURALEZA[x.naturaleza]}</td>
                    <td>{REGIMEN[x.regimen]}</td>
                    <td className="text-right tabular-nums">{euros(x.bruto)}</td>
                    <td className="text-right">{pct(x.tipo)}</td>
                    <td className="text-right tabular-nums">{euros(x.retencion)}</td>
                    <td className="text-right">
                      <ConfirmAction
                        label="Quitar"
                        ariaLabel={`Quitar la renta de ${x.nombre} del ${formatDate(x.fecha)}`}
                        question={`¿Quitar la renta de ${x.nombre} del ${formatDate(x.fecha)}? Saldrá del 216 y del 296.`}
                        confirmLabel="Quitar"
                        onConfirm={async () => {
                          await irnrQuitarRenta(x.id, true);
                          changed();
                        }}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {r.candidatas.length > 0 && (
            <div className="mt-3">
              <p className="text-sm">
                Facturas de proveedores de otro país sin revisar aquí (la mayoría no llevan
                retención: compras de bienes o servicios con convenio). Apunta sólo las sujetas:
              </p>
              <ul className="mt-1 space-y-1 text-sm">
                {r.candidatas.map((c) => (
                  <li key={c.recibida_id} className="flex flex-wrap items-center gap-2">
                    <span>
                      {formatDate(c.fecha)} · {c.nombre} ({c.pais}) · {c.numero} · {euros(c.base)}
                    </span>
                    <button
                      type="button"
                      className="btn-ghost rounded px-2 py-0.5"
                      aria-label={`Apuntar como renta la factura ${c.numero}`}
                      onClick={() => desde(c)}
                    >
                      Apuntar como renta
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          )}
          {!renta ? (
            <button
              type="button"
              className="btn-ghost mt-2 rounded px-3 py-1 text-sm"
              onClick={() => {
                setRenta(rentaVacia());
                setBruto('');
                setTipo('');
                setPropuesta(null);
              }}
            >
              Apuntar una renta
            </button>
          ) : (
            <form
              aria-label="Renta a un no residente"
              className="mt-2 grid gap-2 rounded border border-stone-200 p-3 text-sm sm:grid-cols-2"
              onSubmit={async (e) => {
                e.preventDefault();
                const n = nueva();
                if (!n) return;
                setError(null);
                try {
                  setPropuesta(await irnrProponer(n));
                } catch (err) {
                  setError(errorMessage(err));
                }
              }}
            >
              <label className="flex flex-col">
                Fecha de devengo
                <input
                  type="date"
                  value={renta.fecha}
                  required
                  onChange={(e) => setRenta({ ...renta, fecha: e.target.value })}
                />
              </label>
              <label className="flex flex-col">
                Naturaleza
                <select
                  value={renta.naturaleza}
                  onChange={(e) =>
                    setRenta({ ...renta, naturaleza: e.target.value as NaturalezaIrnr })
                  }
                >
                  {NATURALEZAS.map(([v, l]) => (
                    <option key={v} value={v}>
                      {l}
                    </option>
                  ))}
                </select>
              </label>
              <label className="flex flex-col">
                Identificador fiscal del perceptor
                <input
                  value={renta.nif}
                  required
                  onChange={(e) => setRenta({ ...renta, nif: e.target.value })}
                />
              </label>
              <label className="flex flex-col">
                Nombre del perceptor
                <input
                  value={renta.nombre}
                  required
                  onChange={(e) => setRenta({ ...renta, nombre: e.target.value })}
                />
              </label>
              <label className="flex flex-col">
                País de residencia
                <input
                  value={renta.pais}
                  maxLength={2}
                  required
                  onChange={(e) => setRenta({ ...renta, pais: e.target.value.toUpperCase() })}
                />
              </label>
              <label className="flex flex-col">
                Importe íntegro
                <input
                  value={bruto}
                  inputMode="decimal"
                  required
                  onChange={(e) => setBruto(e.target.value)}
                />
              </label>
              <label className="flex flex-col">
                Tipo del convenio (vacío: el de la tabla)
                <input
                  value={tipo}
                  inputMode="decimal"
                  placeholder="p. ej. 10"
                  onChange={(e) => setTipo(e.target.value)}
                />
              </label>
              <label className="flex flex-col">
                Exención del art. 14 TRLIRNR
                <select
                  value={renta.exencion ?? ''}
                  onChange={(e) =>
                    setRenta({
                      ...renta,
                      exencion: (e.target.value || null) as ExencionIrnr | null,
                    })
                  }
                >
                  {EXENCIONES.map(([v, l]) => (
                    <option key={v} value={v}>
                      {l}
                    </option>
                  ))}
                </select>
              </label>
              <label className="flex flex-col sm:col-span-2">
                Concepto
                <input
                  value={renta.concepto}
                  onChange={(e) => setRenta({ ...renta, concepto: e.target.value })}
                />
              </label>
              <div className="form-actions sm:col-span-2">
                <button type="submit" className="btn-primary rounded px-3 py-1.5">
                  Calcular el tipo
                </button>
                <button
                  type="button"
                  className="btn-ghost rounded px-3 py-1.5"
                  onClick={() => {
                    setRenta(null);
                    setPropuesta(null);
                  }}
                >
                  Cancelar
                </button>
              </div>
              {propuesta && (
                <div className="sm:col-span-2" aria-live="polite">
                  <p>
                    {REGIMEN[propuesta.regimen]}: {pct(propuesta.tipo)} ({propuesta.motivo}). Tipo
                    general: {pct(propuesta.tipo_general)}.
                  </p>
                  {propuesta.avisos.length > 0 && (
                    <ul className="mt-1 list-disc pl-5 text-amber-800">
                      {propuesta.avisos.map((a) => (
                        <li key={a}>{a}</li>
                      ))}
                    </ul>
                  )}
                  <ConfirmAction
                    label="Apuntar la renta"
                    question={`¿Apuntar la renta de ${renta.nombre} al ${pct(propuesta.tipo)}? Entra en el 216 del trimestre y en el 296. No hace asientos: la retención se contabiliza con el pago.`}
                    confirmLabel="Apuntar"
                    danger={false}
                    className="btn-primary mt-2 rounded px-3 py-1.5"
                    onConfirm={async () => {
                      const n = nueva();
                      if (!n) return;
                      await irnrApuntar(n, true);
                      setMsg(`Renta de ${n.nombre} apuntada.`);
                      setRenta(null);
                      setPropuesta(null);
                      changed();
                    }}
                  />
                </div>
              )}
            </form>
          )}
        </div>
      )}

      {r && (
        <details>
          <summary className="cursor-pointer font-medium">
            Tipos de convenio que conoce TaxPain
          </summary>
          <p className="mt-1 text-sm text-stone-600">
            Límites generales, sin las condiciones especiales (participaciones significativas,
            fondos de pensiones…). Compruébalos con el convenio; para otros países, teclea el tipo.
          </p>
          <table className="mt-1 w-full text-sm" aria-label="Tipos de convenio">
            <thead>
              <tr className="text-left text-stone-600">
                <th className="py-1">País</th>
                <th className="text-right">Dividendos</th>
                <th className="text-right">Intereses</th>
                <th className="text-right">Cánones</th>
                <th>Fuente</th>
              </tr>
            </thead>
            <tbody>
              {r.convenios.map((c) => (
                <tr key={c.pais} className="border-t border-stone-200">
                  <td className="py-1">
                    {c.nombre} ({c.pais})
                  </td>
                  <td className="text-right">{pct(c.dividendos_bp)}</td>
                  <td className="text-right">{pct(c.intereses_bp)}</td>
                  <td className="text-right">{pct(c.canones_bp)}</td>
                  <td>{c.fuente}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
      )}
    </section>
  );
}

import { useEffect, useState } from 'react';
import {
  errorMessage,
  euros,
  formatDate,
  type InformeSocio,
  type PersonaSocio,
  parseEuros,
  rentaSocio,
  rentaSocioPersonas,
  rentaSocioSave,
  type Semaforo,
} from './api';

const SEMAFORO: Record<Semaforo, { texto: string; clase: string }> = {
  cumple: { texto: 'Parece que se cumple', clase: 'text-emerald-800' },
  dudoso: { texto: 'Dudoso: revísalo', clase: 'text-amber-800' },
  no_cumple: { texto: 'Parece que no se cumple', clase: 'text-red-700' },
  sin_datos: { texto: 'Faltan datos', clase: 'text-stone-600' },
};

const pct = (millonesimas: number) =>
  `${(millonesimas / 10_000).toLocaleString('es-ES', { maximumFractionDigits: 2 })} %`;

/**
 * Cierre → Datos para la Renta y el Patrimonio del socio o administrador: lo que la sociedad le
 * ha pagado (con retenciones y casillas), el valor de sus participaciones y el semáforo de la
 * exención de empresa familiar. No calcula su IRPF ni le aconseja.
 */
export function RentaSocio({ year }: { year: number }) {
  const [personas, setPersonas] = useState<PersonaSocio[]>([]);
  const [nif, setNif] = useState('');
  const [otros, setOtros] = useState('');
  const [i, setI] = useState<InformeSocio | null>(null);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    rentaSocioPersonas()
      .then((p) => {
        setPersonas(p);
        setNif((n) => n || (p[0]?.nif ?? ''));
      })
      .catch((e) => setMsg({ ok: false, text: errorMessage(e) }));
  }, []);

  const otrosCents = otros.trim() ? parseEuros(otros) : null;
  useEffect(() => {
    if (!nif) return;
    rentaSocio(year, nif, otrosCents)
      .then((x) => {
        setI(x);
        setMsg(null);
      })
      .catch((e) => {
        setI(null);
        setMsg({ ok: false, text: errorMessage(e) });
      });
  }, [year, nif, otrosCents]);

  if (personas.length === 0) {
    return (
      <section aria-label="Datos para la Renta y el Patrimonio" className="mt-4 text-sm">
        <h3 className="font-semibold">Datos para la Renta y el Patrimonio del socio</h3>
        <p className="text-stone-600">
          Anota a los socios personas físicas en «Socios y dividendos» o marca al administrador en
          su ficha de Nóminas.
        </p>
      </section>
    );
  }
  const save = () =>
    rentaSocioSave(year, nif, otrosCents)
      .then((p) => p && setMsg({ ok: true, text: `Informe guardado en ${p}.` }))
      .catch((e) => setMsg({ ok: false, text: errorMessage(e) }));
  return (
    <section aria-label="Datos para la Renta y el Patrimonio" className="mt-4 text-sm">
      <h3 className="font-semibold">Datos para la Renta y el Patrimonio del socio de {year}</h3>
      <div className="mt-2 flex flex-wrap items-end gap-3">
        <label>
          Persona{' '}
          <select value={nif} onChange={(e) => setNif(e.target.value)}>
            {personas.map((p) => (
              <option key={p.nif} value={p.nif}>
                {p.nombre} ({p.nif})
              </option>
            ))}
          </select>
        </label>
        <label>
          Otros rendimientos del trabajo y de actividades (fuera de la sociedad){' '}
          <input
            className="text-right"
            inputMode="decimal"
            value={otros}
            onChange={(e) => setOtros(e.target.value)}
          />
        </label>
        <button type="button" className="rounded border px-3 py-1" onClick={save} disabled={!i}>
          Guardar en PDF
        </button>
      </div>
      {msg && (
        <p
          role={msg.ok ? 'status' : 'alert'}
          className={msg.ok ? 'text-emerald-800' : 'text-red-700'}
        >
          {msg.text}
        </p>
      )}
      {i && (
        <>
          <h4 className="mt-3 font-medium">Lo que la sociedad le ha pagado</h4>
          {i.conceptos.length === 0 ? (
            <p>Nada en {year}.</p>
          ) : (
            <table className="mt-1 w-full" aria-label="Conceptos para la Renta">
              <thead>
                <tr className="text-left">
                  <th>Concepto</th>
                  <th>Casilla</th>
                  <th className="text-right">Importe</th>
                  <th className="text-right">Retención</th>
                  <th>Origen</th>
                </tr>
              </thead>
              <tbody>
                {i.conceptos.map((c) => (
                  <tr key={c.concepto} className="align-top">
                    <td>
                      {c.concepto}
                      {c.estimado && <span className="text-amber-800"> (estimado)</span>}
                      <span className="block text-xs text-stone-600">{c.apartado}</span>
                      {c.nota && <span className="block text-xs text-stone-600">{c.nota}</span>}
                    </td>
                    <td className="font-mono">{c.casilla}</td>
                    <td className="text-right tabular-nums">{euros(c.importe)}</td>
                    <td className="text-right tabular-nums">
                      {c.retencion ? euros(c.retencion) : ''}
                    </td>
                    <td className="text-stone-600">{c.fuente}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <h4 className="mt-3 font-medium">Impuesto sobre el Patrimonio: sus participaciones</h4>
          <table className="mt-1" aria-label="Valor de las participaciones">
            <tbody>
              <tr>
                <td className="pr-4">
                  Patrimonio neto del balance de {i.patrimonio.ejercicio_balance}
                  {i.patrimonio.fecha_balance && ` (${formatDate(i.patrimonio.fecha_balance)})`}
                </td>
                <td className="text-right tabular-nums">
                  {i.patrimonio.patrimonio_neto == null
                    ? 'sin contabilidad'
                    : euros(i.patrimonio.patrimonio_neto)}
                </td>
              </tr>
              <tr>
                <td className="pr-4">Participación</td>
                <td className="text-right tabular-nums">{pct(i.patrimonio.pct)}</td>
              </tr>
              {i.patrimonio.valor_teorico != null && (
                <tr>
                  <td className="pr-4">Valor teórico de las participaciones</td>
                  <td className="text-right tabular-nums">{euros(i.patrimonio.valor_teorico)}</td>
                </tr>
              )}
              {i.patrimonio.valor_exento_orientativo != null && (
                <tr>
                  <td className="pr-4">Parte exenta orientativa (activo afecto medio)</td>
                  <td className="text-right tabular-nums">
                    {euros(i.patrimonio.valor_exento_orientativo)}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
          <p className="mt-2">
            Exención de empresa familiar (art. 4.Ocho.Dos LIP):{' '}
            <strong className={SEMAFORO[i.patrimonio.exencion].clase}>
              {SEMAFORO[i.patrimonio.exencion].texto}
            </strong>
          </p>
          <ul aria-label="Requisitos de la exención" className="mt-1 space-y-1">
            {i.patrimonio.requisitos.map((r) => (
              <li key={r.titulo}>
                <span className={SEMAFORO[r.estado].clase}>{SEMAFORO[r.estado].texto}</span> ·{' '}
                {r.titulo} ({r.norma}). <span className="text-stone-600">{r.detalle}</span>
              </li>
            ))}
          </ul>
          {i.avisos.length > 0 && (
            <ul aria-label="Avisos para la Renta" className="mt-2 list-disc pl-5 text-amber-800">
              {i.avisos.map((a) => (
                <li key={a}>{a}</li>
              ))}
            </ul>
          )}
          <ul className="mt-2 list-disc pl-5 text-stone-600">
            {[...i.patrimonio.notas, ...i.notas].map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

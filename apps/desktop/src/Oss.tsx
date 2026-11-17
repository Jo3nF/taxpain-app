// Ventanilla única (OSS, modelos 035 y 369): umbral de 10.000 € de ventas a distancia y
// servicios electrónicos o de telecomunicaciones a particulares de otros países de la UE (art. 73
// de la Ley del IVA), clasificación de esas facturas, tipos de IVA por país, libro y borrador
// del 369 trimestral. Nada se presenta: el 035 y el 369 se presentan en la Sede.

import { useCallback, useEffect, useState } from 'react';
import {
  type AjustesOss,
  type Borrador369,
  type ClaseOss,
  type EstadoOss,
  errorMessage,
  euros,
  formatDate,
  oss369,
  oss369Hoja,
  ossAjustesGuardar,
  ossClasificar,
  ossConfirmarTipo,
  ossEstado,
  ossTipos,
  ossVentas,
  parseEuros,
  pickFolder,
  type TiposOss,
  type VentaUe,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { pctText, SUPPLY_LABEL } from './OssFactura';

const input = 'mt-1 block w-full rounded-md border border-stone-300 px-2 py-1';

const CLASES: { value: ClaseOss | ''; label: string }[] = [
  { value: '', label: 'Sin clasificar' },
  { value: 'servicios', label: 'Servicio electrónico o de telecomunicaciones' },
  { value: 'bienes', label: 'Venta a distancia de bienes' },
  { value: 'otro', label: 'Otra operación (no cuenta)' },
];

function Umbral({ e }: { e: EstadoOss }) {
  const pct = Math.min(100, Math.round((e.umbral.en_curso / e.limite) * 100));
  return (
    <div>
      <dl className="grid grid-cols-3 gap-3">
        <div>
          <dt className="text-stone-500">Año anterior ({e.umbral.year - 1})</dt>
          <dd className="font-medium">{euros(e.umbral.anterior)}</dd>
        </div>
        <div>
          <dt className="text-stone-500">Año {e.umbral.year}</dt>
          <dd className="font-medium">{euros(e.umbral.en_curso)}</dd>
        </div>
        <div>
          <dt className="text-stone-500">Umbral</dt>
          <dd className="font-medium">{euros(e.limite)}</dd>
        </div>
      </dl>
      <meter
        className="mt-2 block h-2 w-full"
        aria-label="Ventas del año sobre el umbral de 10.000 €"
        min={0}
        max={100}
        high={80}
        value={pct}
      />
      <p className="mt-2">
        {e.tributa_en_destino
          ? `Tributas en el país del cliente desde el ${formatDate(e.destino_desde ?? '')}.`
          : 'Por debajo del umbral: estas ventas llevan IVA español (puedes optar por el del país del cliente).'}
      </p>
      {e.avisos.length > 0 && (
        <div role="alert">
          <ul className="mt-2 list-disc space-y-1 pl-5 text-amber-800">
            {e.avisos.map((a) => (
              <li key={a}>{a}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function Ajustes({
  e,
  year,
  onSaved,
}: {
  e: EstadoOss;
  year: number;
  onSaved: (m: string) => void;
}) {
  const a = e.ajustes;
  const [alta, setAlta] = useState(a.alta_035 ?? '');
  const [opcion, setOpcion] = useState(a.opcion_destino);
  const euro = (c: number | undefined) => (c ? (c / 100).toFixed(2).replace('.', ',') : '');
  const [ant, setAnt] = useState(euro(a.otras_ventas[String(year - 1)]));
  const [cur, setCur] = useState(euro(a.otras_ventas[String(year)]));
  const [error, setError] = useState<string | null>(null);
  async function save() {
    const otras: Record<string, number> = { ...a.otras_ventas };
    for (const [y, t] of [
      [year - 1, ant],
      [year, cur],
    ] as const) {
      if (!t.trim()) {
        delete otras[String(y)];
        continue;
      }
      const c = parseEuros(t);
      if (c === null || c < 0) {
        setError(`Revisa el importe de ${y}`);
        return;
      }
      otras[String(y)] = c;
    }
    const next: AjustesOss = {
      alta_035: alta || null,
      opcion_destino: opcion,
      otras_ventas: otras,
    };
    try {
      await ossAjustesGuardar(next);
      setError(null);
      onSaved('Ajustes de la ventanilla única guardados.');
    } catch (err) {
      setError(errorMessage(err));
    }
  }
  return (
    <div className="grid grid-cols-2 gap-3">
      <label>
        Alta en el régimen de la Unión (modelo 035), fecha de efectos
        <input
          type="date"
          className={input}
          value={alta}
          onChange={(x) => setAlta(x.target.value)}
        />
      </label>
      <label className="flex items-center gap-2 self-end">
        <input type="checkbox" checked={opcion} onChange={(x) => setOpcion(x.target.checked)} />
        Opto por tributar en el país del cliente aunque no llegue al umbral (al menos dos años)
      </label>
      <label>
        Ventas de {year - 1} fuera de TaxPain (sin IVA)
        <input className={input} value={ant} onChange={(x) => setAnt(x.target.value)} />
      </label>
      <label>
        Ventas de {year} fuera de TaxPain (sin IVA)
        <input className={input} value={cur} onChange={(x) => setCur(x.target.value)} />
      </label>
      <div className="col-span-2">
        <button
          type="button"
          className="rounded border border-stone-300 px-3 py-1"
          onClick={() => void save()}
        >
          Guardar ajustes
        </button>
        {error && <span className="ml-2 text-red-700">{error}</span>}
      </div>
    </div>
  );
}

function Ventas({ ventas, onSaved }: { ventas: VentaUe[]; onSaved: (m: string) => void }) {
  const [error, setError] = useState<string | null>(null);
  if (ventas.length === 0)
    return (
      <p className="text-stone-500">No hay facturas a particulares de otros países de la UE.</p>
    );
  return (
    <>
      <table className="w-full">
        <thead>
          <tr className="text-left text-stone-500">
            <th>Factura</th>
            <th>Fecha</th>
            <th>Cliente</th>
            <th>País</th>
            <th className="text-right">Base</th>
            <th className="text-right">IVA</th>
            <th>Clase</th>
          </tr>
        </thead>
        <tbody>
          {ventas.map((x) => (
            <tr key={x.invoice_id} className="border-t border-stone-100">
              <td>{x.number}</td>
              <td>{formatDate(x.date)}</td>
              <td>{x.cliente}</td>
              <td>{x.pais_nombre}</td>
              <td className="text-right">{euros(x.base)}</td>
              <td className="text-right">
                {x.oss
                  ? `${euros(x.oss.vat)} (${x.oss.country} ${pctText(x.oss.rate)})`
                  : `${euros(x.iva_espanol)} (ES)`}
              </td>
              <td>
                {x.clase_fija && x.oss ? (
                  SUPPLY_LABEL[x.oss.supply]
                ) : (
                  <select
                    aria-label={`Clase de la factura ${x.number}`}
                    className="rounded-md border border-stone-300 px-1 py-0.5"
                    value={x.clase ?? ''}
                    onChange={async (e) => {
                      const v = e.target.value as ClaseOss | '';
                      try {
                        await ossClasificar(x.invoice_id, v || null);
                        setError(null);
                        onSaved(`Factura ${x.number} clasificada.`);
                      } catch (err) {
                        setError(errorMessage(err));
                      }
                    }}
                  >
                    {CLASES.map((c) => (
                      <option key={c.value} value={c.value}>
                        {c.label}
                      </option>
                    ))}
                  </select>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {error && <p className="text-red-700">{error}</p>}
    </>
  );
}

function Tipos({ t, onSaved }: { t: TiposOss; onSaved: (m: string) => void }) {
  return (
    <details>
      <summary className="cursor-pointer">
        Tipos generales de IVA por país (tabla de TaxPain a {formatDate(t.fecha)})
      </summary>
      <p className="mt-1 text-xs text-stone-500">
        Fuente: {t.fuente}. Los tipos cambian: compruébalos en la fuente oficial y confirma el de
        cada país antes de emitir la primera factura con su IVA.
      </p>
      <table className="mt-2 w-full">
        <thead>
          <tr className="text-left text-stone-500">
            <th>País</th>
            <th className="text-right">Tabla</th>
            <th className="text-right">Confirmado</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {t.paises.map((p) => (
            <tr key={p.pais} className="border-t border-stone-100">
              <td>
                {p.nombre} ({p.pais})
              </td>
              <td className="text-right">{pctText(p.tabla)}</td>
              <td className="text-right">{p.confirmado !== null ? pctText(p.confirmado) : '—'}</td>
              <td className="text-right">
                {p.confirmado === null && (
                  <ConfirmAction
                    label="Confirmar"
                    ariaLabel={`Confirmar el tipo de ${p.nombre}`}
                    question={`¿Has comprobado que el tipo general de IVA de ${p.nombre} es el ${pctText(p.tabla)}? Quedará en la auditoría.`}
                    confirmLabel="Sí, confirmar"
                    danger={false}
                    className="text-sky-800 underline"
                    onConfirm={async () => {
                      await ossConfirmarTipo(p.pais, p.tabla, true);
                      onSaved(`Tipo de ${p.nombre} confirmado.`);
                    }}
                  />
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </details>
  );
}

function Modelo369({ year, onMsg }: { year: number; onMsg: (m: string) => void }) {
  const [q, setQ] = useState(1);
  const [b, setB] = useState<Borrador369 | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    oss369(year, q)
      .then((r) => {
        setB(r);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
  }, [year, q]);
  return (
    <div>
      <label className="inline-flex items-center gap-2">
        Trimestre
        <select
          className="rounded-md border border-stone-300 px-1 py-0.5"
          value={q}
          onChange={(e) => setQ(Number(e.target.value))}
        >
          {[1, 2, 3, 4].map((n) => (
            <option key={n} value={n}>
              {n}T
            </option>
          ))}
        </select>
      </label>
      {error && <p className="text-red-700">{error}</p>}
      {b && (
        <>
          <p className="mt-2 text-stone-600">
            Plazo: hasta el {b.vencimiento ? formatDate(b.vencimiento) : '—'} (no se traslada si es
            festivo).
          </p>
          {b.lineas.length === 0 && b.correcciones.length === 0 ? (
            <p className="mt-2 text-stone-500">
              Sin ventas con IVA de otros países en el {q}T {year}.
            </p>
          ) : (
            <table className="mt-2 w-full">
              <thead>
                <tr className="text-left text-stone-500">
                  <th>Estado miembro</th>
                  <th>Operación</th>
                  <th className="text-right">Tipo</th>
                  <th className="text-right">Base</th>
                  <th className="text-right">Cuota</th>
                </tr>
              </thead>
              <tbody>
                {b.lineas.map((l) => (
                  <tr key={`${l.pais}-${l.supply}-${l.rate}`} className="border-t border-stone-100">
                    <td>
                      {l.pais_nombre} ({l.pais})
                    </td>
                    <td>{SUPPLY_LABEL[l.supply]}</td>
                    <td className="text-right">{pctText(l.rate)}</td>
                    <td className="text-right">{euros(l.base)}</td>
                    <td className="text-right">{euros(l.cuota)}</td>
                  </tr>
                ))}
                {b.correcciones.map((c) => (
                  <tr
                    key={`c-${c.pais}-${c.year}-${c.quarter}`}
                    className="border-t border-stone-100"
                  >
                    <td>
                      {c.pais_nombre} ({c.pais})
                    </td>
                    <td colSpan={3}>
                      Corrección del {c.quarter}T {c.year}
                    </td>
                    <td className="text-right">{euros(c.cuota)}</td>
                  </tr>
                ))}
                <tr className="border-t border-stone-300 font-semibold">
                  <td colSpan={4}>Total a ingresar</td>
                  <td className="text-right">{euros(b.total)}</td>
                </tr>
              </tbody>
            </table>
          )}
          {b.asiento.length > 0 && (
            <div className="mt-3">
              <p className="font-medium">Asiento al pagarlo (para grabarlo a mano en el diario)</p>
              <table className="w-full">
                <tbody>
                  {b.asiento.map((l) => (
                    <tr key={l.cuenta}>
                      <td>{l.cuenta}</td>
                      <td>{l.concepto}</td>
                      <td className="text-right">{l.debe ? euros(l.debe) : ''}</td>
                      <td className="text-right">{l.haber ? euros(l.haber) : ''}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          <ul className="mt-2 list-disc pl-5 text-xs text-stone-600">
            {b.avisos.map((a) => (
              <li key={a}>{a}</li>
            ))}
          </ul>
          <button
            type="button"
            className="mt-2 rounded border border-stone-300 px-3 py-1"
            onClick={async () => {
              const dir = await pickFolder();
              if (!dir) return;
              try {
                onMsg(`Hoja del 369 guardada en ${await oss369Hoja(year, q, dir)}`);
              } catch (e) {
                setError(errorMessage(e));
              }
            }}
          >
            Guardar la hoja del 369 (XLSX)…
          </button>
        </>
      )}
    </div>
  );
}

export function Oss({ year }: { year: number }) {
  const [e, setE] = useState<EstadoOss | null>(null);
  const [ventas, setVentas] = useState<VentaUe[]>([]);
  const [tipos, setTipos] = useState<TiposOss | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(() => {
    Promise.all([ossEstado(year), ossVentas(year), ossTipos()])
      .then(([a, b, c]) => {
        setE(a);
        setVentas(b);
        setTipos(c);
        setError(null);
      })
      .catch((x) => setError(errorMessage(x)));
  }, [year]);
  useEffect(load, [load]);
  const onSaved = (m: string) => {
    setMsg(m);
    load();
  };
  if (error)
    return (
      <p role="alert" className="mt-4 text-red-700">
        No se pudo cargar la ventanilla única: {error}
      </p>
    );
  if (!e || !tipos) return <p className="mt-4 text-stone-500">Cargando la ventanilla única…</p>;
  return (
    <section className="mt-8 space-y-4 border-t border-stone-200 pt-4 text-sm">
      <h3 className="text-base font-semibold">
        Ventanilla única (OSS): ventas a particulares de la UE · modelos 035 y 369
      </h3>
      <p className="text-stone-600">
        Los servicios electrónicos, de telecomunicaciones o de radiodifusión y las ventas a
        distancia de bienes a particulares de otros países de la UE llevan IVA español hasta que
        pasan de 10.000 € al año en toda la UE (art. 73 de la Ley del IVA). Desde la venta que lo
        supera, llevan el IVA del país del cliente, que se declara en el modelo 369 y no en el 303.
      </p>
      {msg && <p role="status">{msg}</p>}
      <Umbral e={e} />
      <h4 className="font-medium">Ajustes</h4>
      <Ajustes key={JSON.stringify(e.ajustes)} e={e} year={year} onSaved={onSaved} />
      <h4 className="font-medium">Facturas a particulares de la UE en {year}</h4>
      <Ventas ventas={ventas} onSaved={onSaved} />
      <Tipos t={tipos} onSaved={onSaved} />
      <h4 className="font-medium">Modelo 369 (régimen de la Unión), borrador trimestral</h4>
      <Modelo369 year={year} onMsg={setMsg} />
    </section>
  );
}

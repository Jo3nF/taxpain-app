import { useCallback, useEffect, useId, useState } from 'react';
import { errorMessage, euros, formatDate, parseEuros } from './api';
import { ConfirmAction } from './ConfirmAction';
import {
  type Ajuste200,
  type Datos200,
  type DetalleArrastre,
  type DividendoParticipada,
  type Hoja200,
  type Linea200,
  modelo200AjusteGuardar,
  modelo200AjusteQuitar,
  modelo200ArrastreBorrar,
  modelo200ArrastreGuardar,
  modelo200DatosGuardar,
  modelo200Hoja,
  modelo200PasarSiguiente,
  ORIGEN_LABEL,
  type OrigenParticipacion,
  type ParticipadaVista,
  participadaDividendoBorrar,
  participadaDividendoGuardar,
  participadaFiscalGuardar,
  participadaFondosGuardar,
  REGIMEN_LABEL,
  type Reserva200,
  TIPO_ARRASTRE_LABEL,
  type TipoArrastre,
} from './modelo200Api';

const ESTADO: Record<Ajuste200['estado'], string> = {
  propuesta: 'Propuesta',
  sin_incluir: 'Sin incluir',
  editada: 'Corregida',
  descartada: 'Descartada',
  manual: 'A mano',
};

const ART21: Record<string, string> = {
  cumple: 'Cumple',
  condicionada: 'Condicionada',
  no_cumple: 'No cumple',
};

const casilla = (n: number | null) => (n === null ? '' : `[${String(n).padStart(5, '0')}]`);

function Lineas({ titulo, lineas }: { titulo: string; lineas: Linea200[] }) {
  if (lineas.length === 0) return null;
  return (
    <div className="mt-2 overflow-x-auto">
      <table className="w-full text-left">
        <caption className="sr-only">{titulo}</caption>
        <thead>
          <tr className="border-b border-stone-200">
            <th scope="col">Casilla</th>
            <th scope="col">Concepto</th>
            <th scope="col" className="text-right">
              Importe
            </th>
            <th scope="col">De dónde sale</th>
            <th scope="col">Norma</th>
          </tr>
        </thead>
        <tbody>
          {lineas.map((l) => (
            <tr key={`${l.casilla}-${l.concepto}`} className="border-b border-stone-100">
              <td>{casilla(l.casilla)}</td>
              <th scope="row" className="font-normal">
                {l.concepto}
              </th>
              <td className="text-right">
                {l.casilla === 558 ? `${(l.importe / 100).toFixed(2)} %` : euros(l.importe)}
              </td>
              <td className="text-stone-600">{l.origen}</td>
              <td className="text-stone-600">{l.norma}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Detalle({ titulo, filas }: { titulo: string; filas: DetalleArrastre[] }) {
  if (filas.length === 0) return null;
  return (
    <table className="mt-2 w-full text-left">
      <caption className="text-left font-medium">{titulo}</caption>
      <thead>
        <tr className="border-b border-stone-200">
          <th scope="col">Ejercicio de origen</th>
          <th scope="col" className="text-right">
            Al empezar
          </th>
          <th scope="col" className="text-right">
            Aplicado
          </th>
          <th scope="col" className="text-right">
            Queda
          </th>
        </tr>
      </thead>
      <tbody>
        {filas.map((f) => (
          <tr key={f.origen} className="border-b border-stone-100">
            <th scope="row" className="font-normal">
              {f.origen}
            </th>
            <td className="text-right">{euros(f.inicio)}</td>
            <td className="text-right">{euros(f.aplicado)}</td>
            <td className="text-right">{euros(f.pendiente)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Seccion({
  id,
  titulo,
  children,
}: {
  id: string;
  titulo: string;
  children: React.ReactNode;
}) {
  return (
    <section aria-labelledby={id} className="mt-4 border-t border-stone-200 pt-3">
      <h4 id={id} className="font-medium">
        {titulo}
      </h4>
      {children}
    </section>
  );
}

function Importe({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (s: string) => void;
}) {
  const id = useId();
  return (
    <span className="inline-flex items-center gap-1">
      <label htmlFor={id}>{label}</label>
      <input
        id={id}
        className="w-32 rounded border border-stone-300 px-1 text-right"
        inputMode="decimal"
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
    </span>
  );
}

const centsText = (c: number | null) => (c === null ? '' : (c / 100).toFixed(2).replace('.', ','));

/** Fila de una corrección, con su edición. */
function FilaAjuste({ a, year, onChange }: { a: Ajuste200; year: number; onChange: () => void }) {
  const [edit, setEdit] = useState(false);
  const [txt, setTxt] = useState(centsText(a.importe || a.propuesto));
  const [err, setErr] = useState<string | null>(null);
  const guardar = async (importe: number | null) => {
    setErr(null);
    try {
      await modelo200AjusteGuardar(year, {
        clave: a.clave,
        casilla: a.casilla,
        importe,
        concepto: a.concepto,
      });
      setEdit(false);
      onChange();
    } catch (e) {
      setErr(errorMessage(e));
    }
  };
  return (
    <tr className="border-b border-stone-100 align-top">
      <td>{casilla(a.casilla)}</td>
      <th scope="row" className="font-normal">
        {a.concepto}
        <span className="block text-xs text-stone-600">
          {a.aumento ? 'Aumento' : 'Disminución'} · {a.norma} · {a.origen}
        </span>
        {a.nota && <span className="block text-xs text-amber-800">{a.nota}</span>}
        {err && (
          <span role="alert" className="block text-xs text-red-700">
            {err}
          </span>
        )}
      </th>
      <td className="text-right">{a.propuesto === null ? '—' : euros(a.propuesto)}</td>
      <td className="text-right">
        {edit ? (
          <Importe label={`Importe de ${a.concepto}`} value={txt} onChange={setTxt} />
        ) : (
          euros(a.importe)
        )}
      </td>
      <td>{ESTADO[a.estado]}</td>
      <td className="whitespace-nowrap">
        {edit ? (
          <>
            <button
              type="button"
              className="rounded border border-stone-300 px-2"
              onClick={() => {
                const c = parseEuros(txt);
                if (c === null || c < 0) setErr('Importe no válido');
                else guardar(c);
              }}
            >
              Guardar
            </button>{' '}
            <button type="button" className="px-2" onClick={() => setEdit(false)}>
              Cancelar
            </button>
          </>
        ) : (
          <>
            <button
              type="button"
              className="rounded border border-stone-300 px-2"
              aria-label={`Cambiar: ${a.concepto}`}
              onClick={() => setEdit(true)}
            >
              Cambiar
            </button>{' '}
            {a.estado !== 'descartada' && a.estado !== 'manual' && a.propuesto !== null && (
              <button
                type="button"
                className="px-2"
                aria-label={`Descartar: ${a.concepto}`}
                onClick={() => guardar(null)}
              >
                Descartar
              </button>
            )}
            {(a.estado === 'editada' || a.estado === 'descartada' || a.estado === 'manual') && (
              <ConfirmAction
                label={a.estado === 'manual' ? 'Quitar' : 'Volver a la propuesta'}
                ariaLabel={`${a.estado === 'manual' ? 'Quitar' : 'Volver a la propuesta'}: ${a.concepto}`}
                question={
                  a.estado === 'manual'
                    ? `Se quita la corrección «${a.concepto}» del 200 de ${year}.`
                    : `Se olvida lo guardado y vuelve lo que propone TaxPain para «${a.concepto}».`
                }
                confirmLabel="Sí"
                onConfirm={async () => {
                  await modelo200AjusteQuitar(year, a.clave);
                  onChange();
                }}
              />
            )}
          </>
        )}
      </td>
    </tr>
  );
}

function NuevaCorreccion({ hoja, onChange }: { hoja: Hoja200; onChange: () => void }) {
  const [cas, setCas] = useState(hoja.catalogo[0]?.casilla ?? 413);
  const [txt, setTxt] = useState('');
  const [concepto, setConcepto] = useState('');
  const [err, setErr] = useState<string | null>(null);
  const id = useId();
  return (
    <fieldset className="mt-2 flex flex-wrap items-end gap-2">
      <legend className="font-medium">Añadir una corrección a mano</legend>
      <label htmlFor={`${id}-c`}>Casilla</label>
      <select
        id={`${id}-c`}
        className="max-w-md rounded border border-stone-300"
        value={cas}
        onChange={(e) => setCas(Number(e.target.value))}
      >
        {hoja.catalogo.map((c) => (
          <option key={c.casilla} value={c.casilla}>
            {casilla(c.casilla)} {c.aumento ? '+' : '−'} {c.concepto} ({c.norma})
          </option>
        ))}
      </select>
      <Importe label="Importe" value={txt} onChange={setTxt} />
      <label htmlFor={`${id}-t`}>Concepto</label>
      <input
        id={`${id}-t`}
        className="rounded border border-stone-300 px-1"
        value={concepto}
        onChange={(e) => setConcepto(e.target.value)}
      />
      <button
        type="button"
        className="rounded border border-stone-300 px-2"
        onClick={async () => {
          const c = parseEuros(txt);
          if (c === null || c <= 0) {
            setErr('Importe no válido');
            return;
          }
          try {
            await modelo200AjusteGuardar(hoja.year, {
              clave: '',
              casilla: cas,
              importe: c,
              concepto,
            });
            setTxt('');
            setConcepto('');
            setErr(null);
            onChange();
          } catch (e) {
            setErr(errorMessage(e));
          }
        }}
      >
        Añadir
      </button>
      {err && (
        <p role="alert" className="w-full text-red-700">
          {err}
        </p>
      )}
    </fieldset>
  );
}

function NuevoArrastre({ year, onChange }: { year: number; onChange: () => void }) {
  const [tipo, setTipo] = useState<TipoArrastre>('bin');
  const [origen, setOrigen] = useState(String(year - 1));
  const [txt, setTxt] = useState('');
  const [err, setErr] = useState<string | null>(null);
  const id = useId();
  return (
    <fieldset className="mt-2 flex flex-wrap items-end gap-2">
      <legend className="font-medium">
        Añadir un pendiente de otro ejercicio (al empezar {year})
      </legend>
      <label htmlFor={`${id}-t`}>Qué</label>
      <select
        id={`${id}-t`}
        className="rounded border border-stone-300"
        value={tipo}
        onChange={(e) => setTipo(e.target.value as TipoArrastre)}
      >
        {(Object.keys(TIPO_ARRASTRE_LABEL) as TipoArrastre[]).map((t) => (
          <option key={t} value={t}>
            {TIPO_ARRASTRE_LABEL[t]}
          </option>
        ))}
      </select>
      <label htmlFor={`${id}-o`}>Ejercicio de origen</label>
      <input
        id={`${id}-o`}
        className="w-20 rounded border border-stone-300 px-1"
        inputMode="numeric"
        value={origen}
        onChange={(e) => setOrigen(e.target.value)}
      />
      <Importe label="Pendiente" value={txt} onChange={setTxt} />
      <button
        type="button"
        className="rounded border border-stone-300 px-2"
        onClick={async () => {
          const c = parseEuros(txt);
          const o = Number(origen);
          if (c === null || c < 0 || !Number.isInteger(o)) {
            setErr('Revisa el ejercicio y el importe');
            return;
          }
          try {
            await modelo200ArrastreGuardar({
              tipo,
              origen: o,
              ejercicio: year,
              pendiente: c,
              nota: '',
            });
            setTxt('');
            setErr(null);
            onChange();
          } catch (e) {
            setErr(errorMessage(e));
          }
        }}
      >
        Guardar
      </button>
      {err && (
        <p role="alert" className="w-full text-red-700">
          {err}
        </p>
      )}
    </fieldset>
  );
}

function Reserva({ titulo, r }: { titulo: string; r: Reserva200 }) {
  return (
    <div className="mt-2">
      <Lineas titulo={titulo} lineas={r.lineas} />
      <Detalle titulo={`${titulo} por ejercicio`} filas={r.detalle} />
      {r.dotacion && (
        <p
          role="status"
          className={`mt-2 rounded border p-2 ${
            r.dotacion.estado === 'dotada'
              ? 'border-emerald-300 bg-emerald-50'
              : 'border-amber-300 bg-amber-50'
          }`}
        >
          {r.dotacion.texto}
        </p>
      )}
    </div>
  );
}

function DatosForm({ hoja, onChange }: { hoja: Hoja200; onChange: () => void }) {
  const d = hoja.datos;
  const [aplicar, setAplicar] = useState(d.aplicar_capitalizacion);
  const [incr, setIncr] = useState(centsText(d.incremento_fondos_propios));
  const [pct, setPct] = useState(d.porcentaje_capitalizacion?.toString() ?? '');
  const [niv, setNiv] = useState(centsText(d.nivelacion || null));
  const [nueva, setNueva] = useState(d.bin_nueva_creacion);
  const [di, setDi] = useState(centsText(d.doble_imposicion));
  const [cuota, setCuota] = useState(d.cuota);
  const [cas, setCas] = useState(583);
  const [cTxt, setCTxt] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  const id = useId();
  const manuales = hoja.catalogo_cuota.filter(
    (c) => c.grupo === 'bonificacion' || c.grupo === 'deduccion',
  );
  const opt = (s: string) => (s.trim() === '' ? null : parseEuros(s));
  return (
    <form
      className="mt-2 space-y-2"
      onSubmit={async (e) => {
        e.preventDefault();
        const datos: Datos200 = {
          ...d,
          aplicar_capitalizacion: aplicar,
          incremento_fondos_propios: opt(incr),
          porcentaje_capitalizacion: pct.trim() === '' ? null : Number(pct),
          nivelacion: opt(niv) ?? 0,
          bin_nueva_creacion: nueva,
          doble_imposicion: opt(di),
          cuota,
        };
        try {
          await modelo200DatosGuardar(hoja.year, datos);
          setMsg('Datos guardados.');
          onChange();
        } catch (err) {
          setMsg(errorMessage(err));
        }
      }}
    >
      <label className="flex items-center gap-2">
        <input type="checkbox" checked={aplicar} onChange={(e) => setAplicar(e.target.checked)} />
        Aplicar la reserva de capitalización (obliga a dotarla y a mantener el incremento cinco
        años, art. 25 LIS)
      </label>
      <div className="flex flex-wrap gap-3">
        <Importe
          label="Incremento de fondos propios (vacío: el del balance)"
          value={incr}
          onChange={setIncr}
        />
        <span className="inline-flex items-center gap-1">
          <label htmlFor={`${id}-p`}>Porcentaje (vacío: 20 %; 15 % en 2024 y 10 % antes)</label>
          <input
            id={`${id}-p`}
            className="w-16 rounded border border-stone-300 px-1"
            inputMode="numeric"
            value={pct}
            onChange={(e) => setPct(e.target.value)}
          />
        </span>
      </div>
      <Importe
        label="Minoración por reserva de nivelación (art. 105 LIS)"
        value={niv}
        onChange={setNiv}
      />
      <label className="flex items-center gap-2">
        <input type="checkbox" checked={nueva} onChange={(e) => setNueva(e.target.checked)} />
        Entidad de nueva creación en sus tres primeros periodos con base positiva: bases negativas
        sin límite (art. 26.3 LIS)
      </label>
      <Importe
        label="Doble imposición internacional (vacío: la calculada)"
        value={di}
        onChange={setDi}
      />
      <fieldset>
        <legend>
          Bonificaciones y otras deducciones (a mano; su desglose va en Sociedades WEB)
        </legend>
        <ul className="list-disc pl-5">
          {cuota.map((x) => (
            <li key={x.casilla}>
              {casilla(x.casilla)} {manuales.find((c) => c.casilla === x.casilla)?.concepto}:{' '}
              {euros(x.importe)}{' '}
              <button
                type="button"
                className="px-1 text-red-700"
                aria-label={`Quitar ${casilla(x.casilla)}`}
                onClick={() => setCuota(cuota.filter((y) => y.casilla !== x.casilla))}
              >
                Quitar
              </button>
            </li>
          ))}
        </ul>
        <div className="flex flex-wrap items-end gap-2">
          <label htmlFor={`${id}-c`}>Casilla</label>
          <select
            id={`${id}-c`}
            className="max-w-md rounded border border-stone-300"
            value={cas}
            onChange={(e) => setCas(Number(e.target.value))}
          >
            {manuales.map((c) => (
              <option key={c.casilla} value={c.casilla}>
                {casilla(c.casilla)} {c.concepto} ({c.norma})
              </option>
            ))}
          </select>
          <Importe label="Importe de la deducción" value={cTxt} onChange={setCTxt} />
          <button
            type="button"
            className="rounded border border-stone-300 px-2"
            onClick={() => {
              const c = parseEuros(cTxt);
              if (c !== null && c > 0) {
                const resto = cuota.filter((y) => y.casilla !== cas);
                const antes = cuota.find((y) => y.casilla === cas)?.importe ?? 0;
                setCuota([...resto, { casilla: cas, importe: antes + c }]);
                setCTxt('');
              }
            }}
          >
            Añadir a la lista
          </button>
        </div>
      </fieldset>
      <button type="submit" className="rounded border border-stone-300 px-3 py-1">
        Guardar los datos del 200
      </button>
      {msg && <p role="status">{msg}</p>}
    </form>
  );
}

function FichaParticipada({
  p,
  year,
  onChange,
}: {
  p: ParticipadaVista;
  year: number;
  onChange: () => void;
}) {
  const [nominal, setNominal] = useState(centsText(p.fiscal.nominal));
  const [vf, setVf] = useState(centsText(p.fiscal.valor_fiscal));
  const [origen, setOrigen] = useState<OrigenParticipacion>(p.fiscal.origen);
  const [trib, setTrib] = useState(p.fiscal.tributacion_minima);
  const f = p.fondos;
  const [capital, setCapital] = useState(centsText(f?.capital ?? null));
  const [reservas, setReservas] = useState(centsText(f?.reservas ?? null));
  const [otras, setOtras] = useState(centsText(f?.otras_pn ?? null));
  const [res, setRes] = useState(centsText(f?.resultado ?? null));
  const [acuerdo, setAcuerdo] = useState('');
  const [bruto, setBruto] = useState('');
  const [ret, setRet] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  const id = useId();
  const n = (s: string) => parseEuros(s) ?? 0;
  const run = async (fn: () => Promise<unknown>, ok: string) => {
    try {
      await fn();
      setMsg(ok);
      onChange();
    } catch (e) {
      setMsg(errorMessage(e));
    }
  };
  return (
    <article className="mt-3 rounded border border-stone-200 p-3" aria-label={p.participada.name}>
      <h5 className="font-medium">
        {p.participada.name} ({p.participada.nif}) ·{' '}
        {(p.participada.pct / 10_000).toLocaleString('es-ES')} % desde el{' '}
        {formatDate(p.participada.acquired)}
      </h5>
      <p className="text-stone-600">
        Coste contable {euros(p.participada.cost)} · valor fiscal {euros(p.valor_fiscal)} ·
        dividendos del ejercicio {euros(p.bruto)} (exento {euros(p.exento)}, retenido{' '}
        {euros(p.retenido)})
        {p.pn_proporcional !== null &&
          ` · su parte del patrimonio neto ${euros(p.pn_proporcional)}`}
      </p>
      <div className="mt-2 flex flex-wrap items-end gap-2">
        <Importe label="Valor nominal" value={nominal} onChange={setNominal} />
        <Importe label="Valor fiscal (vacío: el coste)" value={vf} onChange={setVf} />
        <label htmlFor={`${id}-o`}>Adquirida por</label>
        <select
          id={`${id}-o`}
          className="rounded border border-stone-300"
          value={origen}
          onChange={(e) => setOrigen(e.target.value as OrigenParticipacion)}
        >
          {(Object.keys(ORIGEN_LABEL) as OrigenParticipacion[]).map((o) => (
            <option key={o} value={o}>
              {ORIGEN_LABEL[o]}
            </option>
          ))}
        </select>
        {p.participada.country !== 'ES' && (
          <label className="flex items-center gap-1">
            <input type="checkbox" checked={trib} onChange={(e) => setTrib(e.target.checked)} />
            Tributa al 10 % nominal o más (art. 21.1.b)
          </label>
        )}
        <button
          type="button"
          className="rounded border border-stone-300 px-2"
          onClick={() =>
            run(
              () =>
                participadaFiscalGuardar({
                  participada_id: p.participada.id,
                  nominal: n(nominal),
                  valor_fiscal: vf.trim() === '' ? null : n(vf),
                  origen,
                  tributacion_minima: trib,
                }),
              'Datos fiscales guardados.',
            )
          }
        >
          Guardar datos fiscales
        </button>
      </div>
      <div className="mt-2 flex flex-wrap items-end gap-2">
        <span>Fondos propios de la participada en {year}:</span>
        <Importe label="Capital" value={capital} onChange={setCapital} />
        <Importe label="Reservas" value={reservas} onChange={setReservas} />
        <Importe label="Otras partidas" value={otras} onChange={setOtras} />
        <Importe label="Resultado" value={res} onChange={setRes} />
        <button
          type="button"
          className="rounded border border-stone-300 px-2"
          onClick={() =>
            run(
              () =>
                participadaFondosGuardar({
                  participada_id: p.participada.id,
                  year,
                  capital: n(capital),
                  reservas: n(reservas),
                  otras_pn: n(otras),
                  resultado: n(res),
                }),
              'Fondos propios guardados.',
            )
          }
        >
          Guardar fondos propios
        </button>
      </div>
      <table className="mt-2 w-full text-left">
        <caption className="text-left font-medium">Dividendos cobrados en {year}</caption>
        <thead>
          <tr className="border-b border-stone-200">
            <th scope="col">Acuerdo</th>
            <th scope="col" className="text-right">
              Bruto
            </th>
            <th scope="col" className="text-right">
              Retención
            </th>
            <th scope="col">Art. 21 LIS</th>
            <th scope="col" className="text-right">
              Exento
            </th>
            <th scope="col" className="text-right">
              Retención que procede
            </th>
            <th scope="col">
              <span className="sr-only">Acciones</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {p.dividendos.map((d) => (
            <tr key={d.dividendo.id} className="border-b border-stone-100 align-top">
              <th scope="row" className="font-normal">
                {formatDate(d.dividendo.fecha_acuerdo)}
              </th>
              <td className="text-right">{euros(d.dividendo.bruto)}</td>
              <td className="text-right">{euros(d.dividendo.retencion)}</td>
              <td>
                {ART21[d.art21.estado]}
                {d.art21.motivos.map((m) => (
                  <span key={m} className="block text-xs text-stone-600">
                    {m}
                  </span>
                ))}
              </td>
              <td className="text-right">{euros(d.exento)}</td>
              <td className="text-right">{euros(d.retencion_procede)}</td>
              <td>
                <ConfirmAction
                  label="Borrar"
                  ariaLabel={`Borrar el dividendo del ${formatDate(d.dividendo.fecha_acuerdo)}`}
                  question={`Se borra el dividendo de ${euros(d.dividendo.bruto)} del ${formatDate(d.dividendo.fecha_acuerdo)}.`}
                  confirmLabel="Borrar"
                  onConfirm={async () => {
                    await participadaDividendoBorrar(d.dividendo.id);
                    onChange();
                  }}
                />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="mt-2 flex flex-wrap items-end gap-2">
        <label htmlFor={`${id}-a`}>Fecha del acuerdo</label>
        <input
          id={`${id}-a`}
          type="date"
          className="rounded border border-stone-300 px-1"
          value={acuerdo}
          onChange={(e) => setAcuerdo(e.target.value)}
        />
        <Importe label="Bruto" value={bruto} onChange={setBruto} />
        <Importe label="Retención" value={ret} onChange={setRet} />
        <button
          type="button"
          className="rounded border border-stone-300 px-2"
          onClick={() => {
            const b = parseEuros(bruto);
            if (!acuerdo || b === null || b <= 0) {
              setMsg('Pon la fecha del acuerdo y el bruto');
              return;
            }
            const d: DividendoParticipada = {
              id: 0,
              participada_id: p.participada.id,
              fecha_acuerdo: acuerdo,
              fecha_cobro: null,
              bruto: b,
              retencion: parseEuros(ret) ?? 0,
              documento: '',
              notas: '',
            };
            run(() => participadaDividendoGuardar(d), 'Dividendo guardado.').then(() => {
              setBruto('');
              setRet('');
            });
          }}
        >
          Añadir dividendo
        </button>
      </div>
      {p.avisos.length > 0 && (
        <ul className="mt-2 list-disc pl-5 text-amber-800">
          {p.avisos.map((a) => (
            <li key={a}>{a}</li>
          ))}
        </ul>
      )}
      {msg && (
        <p role="status" className="mt-1">
          {msg}
        </p>
      )}
    </article>
  );
}

/**
 * Modelos → 200: liquidación completa por bloques (resultado contable, correcciones, base
 * previa, bases negativas, reservas, cuota, deducciones, retenciones y resultado), cada cifra con
 * su casilla, su origen y su norma; ficha fiscal de las participadas y paso al ejercicio
 * siguiente. Nada se presenta: el fichero se importa en Sociedades WEB.
 */
export function Modelo200({ year, onChange }: { year: number; onChange: () => void }) {
  const [hoja, setHoja] = useState<Hoja200 | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [version, setVersion] = useState(0);

  const load = useCallback(() => {
    setError(null);
    modelo200Hoja(year)
      .then((h) => {
        setHoja(h);
        setVersion((v) => v + 1);
      })
      .catch((e) => {
        setHoja(null);
        setError(errorMessage(e));
      });
  }, [year]);
  useEffect(load, [load]);
  const changed = () => {
    load();
    onChange();
  };

  if (error) {
    return (
      <p role="alert" className="mt-4 text-sm text-red-700">
        {error}
      </p>
    );
  }
  if (!hoja) return null;
  const h = hoja;
  const g = (b: number) => h.base.find((l) => l.casilla === b)?.importe ?? 0;
  return (
    <section
      aria-labelledby="m200-h"
      className="mt-4 rounded-lg border border-stone-200 bg-white p-4 text-sm"
    >
      <h3 id="m200-h" className="font-semibold">
        Impuesto sobre Sociedades {h.year} ({formatDate(h.inicio)} – {formatDate(h.fin)})
      </h3>
      <p className="mt-1">
        Se liquida con: <strong>{REGIMEN_LABEL[h.regimen_aplicado]}</strong>. {h.regimen_motivo}
        {h.cifra_negocios_anterior !== null &&
          ` Cifra de negocios del periodo anterior: ${euros(h.cifra_negocios_anterior)}.`}{' '}
        Cifra de negocios de {h.year}: {euros(h.cifra_negocios)}.
      </p>
      {h.patrimonial === true && (
        <p role="status" className="mt-2 rounded border border-amber-300 bg-amber-50 p-2">
          Entidad patrimonial según el test del ejercicio (art. 5.2 LIS): casilla [00066], tipo
          general y sin incentivos de reducida dimensión.
        </p>
      )}

      <Seccion id="m200-res" titulo="1. Resultado contable">
        <Lineas titulo="Resultado contable" lineas={h.resultado} />
      </Seccion>

      <Seccion id="m200-aj" titulo="2. Correcciones al resultado contable">
        <div className="mt-2 overflow-x-auto">
          <table className="w-full text-left">
            <caption className="sr-only">Correcciones al resultado contable</caption>
            <thead>
              <tr className="border-b border-stone-200">
                <th scope="col">Casilla</th>
                <th scope="col">Concepto</th>
                <th scope="col" className="text-right">
                  Propuesto
                </th>
                <th scope="col" className="text-right">
                  En el 200
                </th>
                <th scope="col">Estado</th>
                <th scope="col">
                  <span className="sr-only">Acciones</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {h.ajustes.map((a) => (
                <FilaAjuste key={`${a.clave}-${version}`} a={a} year={h.year} onChange={changed} />
              ))}
            </tbody>
          </table>
          {h.ajustes.length === 0 && (
            <p className="mt-1 text-stone-600">No hay correcciones propuestas ni tecleadas.</p>
          )}
        </div>
        <NuevaCorreccion key={`nc-${version}`} hoja={h} onChange={changed} />
      </Seccion>

      <Seccion id="m200-bp" titulo="3. Base imponible previa">
        <Lineas titulo="Base imponible previa" lineas={h.base_previa} />
      </Seccion>

      <Seccion id="m200-bin" titulo="4. Bases imponibles negativas por ejercicio de origen">
        {h.bins_origen === 'datos' && (
          <p role="status" className="mt-1 text-amber-800">
            Las bases pendientes están como un total en «Datos de los modelos»: añádelas por año.
          </p>
        )}
        <Detalle titulo="Compensación por ejercicio (página 15)" filas={h.bins} />
        {h.bin_generada > 0 && (
          <p className="mt-1">
            Base negativa generada en {h.year}: {euros(h.bin_generada)}.
          </p>
        )}
        {h.arrastres.length > 0 && (
          <ul className="mt-2 list-disc pl-5">
            {h.arrastres.map((a) => (
              <li key={`${a.tipo}-${a.origen}`}>
                {TIPO_ARRASTRE_LABEL[a.tipo]} de {a.origen}: {euros(a.pendiente)}{' '}
                <ConfirmAction
                  label="Borrar"
                  ariaLabel={`Borrar ${TIPO_ARRASTRE_LABEL[a.tipo]} de ${a.origen}`}
                  question={`Se borra el pendiente de ${a.origen} (${euros(a.pendiente)}) al empezar ${h.year}.`}
                  confirmLabel="Borrar"
                  onConfirm={async () => {
                    await modelo200ArrastreBorrar(a.tipo, a.origen, a.ejercicio);
                    changed();
                  }}
                />
              </li>
            ))}
          </ul>
        )}
        <NuevoArrastre key={`na-${version}`} year={h.year} onChange={changed} />
      </Seccion>

      <Seccion id="m200-base" titulo="5. Base imponible">
        <Lineas titulo="Base imponible" lineas={h.base} />
      </Seccion>

      <Seccion id="m200-res2" titulo="6. Reservas de capitalización y de nivelación">
        <details className="mt-2">
          <summary>
            Fondos propios computables: {euros(h.fondos_propios.inicio)} al empezar,{' '}
            {euros(h.fondos_propios.fin)} al cerrar (incremento {euros(h.fondos_propios.incremento)}
            )
          </summary>
          <table className="mt-1 w-full text-left">
            <caption className="sr-only">Partidas de fondos propios</caption>
            <thead>
              <tr className="border-b border-stone-200">
                <th scope="col">Cuenta</th>
                <th scope="col" className="text-right">
                  Al empezar
                </th>
                <th scope="col" className="text-right">
                  Al cerrar
                </th>
                <th scope="col">¿Computa?</th>
              </tr>
            </thead>
            <tbody>
              {h.fondos_propios.partidas.map((p) => (
                <tr key={p.cuenta} className="border-b border-stone-100">
                  <th scope="row" className="font-normal">
                    {p.cuenta} {p.nombre}
                  </th>
                  <td className="text-right">{euros(p.inicio)}</td>
                  <td className="text-right">{euros(p.fin)}</td>
                  <td>{p.computa ? 'Sí' : `No: ${p.motivo}`}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
        <Reserva titulo="Reserva de capitalización" r={h.capitalizacion} />
        <Reserva titulo="Reserva de nivelación" r={h.nivelacion} />
        <DatosForm key={`df-${version}`} hoja={h} onChange={changed} />
      </Seccion>

      <Seccion id="m200-cuota" titulo="7. Cuota, bonificaciones y deducciones">
        <Lineas titulo="Cuota" lineas={h.cuota} />
      </Seccion>

      <Seccion id="m200-ret" titulo="8. Retenciones y pagos fraccionados">
        <Lineas titulo="Retenciones" lineas={h.retenciones} />
      </Seccion>

      <Seccion id="m200-liq" titulo="9. Resultado">
        <Lineas titulo="Resultado de la autoliquidación" lineas={h.liquidacion} />
        {g(552) < 0 && <p className="mt-1">Base imponible negativa: no hay cuota.</p>}
      </Seccion>

      <Seccion id="m200-part" titulo="Participadas y exención del art. 21 LIS">
        {h.participadas.length === 0 ? (
          <p className="mt-1 text-stone-600">
            No hay participadas. Se dan de alta en Operaciones vinculadas → Participadas.
          </p>
        ) : (
          h.participadas.map((p) => (
            <FichaParticipada
              key={`${p.participada.id}-${version}`}
              p={p}
              year={h.year}
              onChange={changed}
            />
          ))
        )}
      </Seccion>

      <Seccion id="m200-sig" titulo={`Lo que pasa a ${h.year + 1}`}>
        {h.siguiente.length === 0 ? (
          <p className="mt-1 text-stone-600">
            No queda nada pendiente para el ejercicio siguiente.
          </p>
        ) : (
          <ul className="mt-1 list-disc pl-5">
            {h.siguiente.map((a) => (
              <li key={`${a.tipo}-${a.origen}`}>
                {TIPO_ARRASTRE_LABEL[a.tipo]} de {a.origen}: {euros(a.pendiente)}
                {a.nota && ` (${a.nota})`}
              </li>
            ))}
          </ul>
        )}
        <div className="mt-2">
          <ConfirmAction
            label={`Pasar a ${h.year + 1}`}
            className="rounded border border-stone-300 px-3 py-1"
            danger={false}
            question={`Se guardan como pendientes al empezar ${h.year + 1} las cifras de esta lista y se sustituyen las que hubiera de ese ejercicio. Hazlo cuando el 200 de ${h.year} esté presentado.`}
            confirmLabel="Pasar"
            onConfirm={async () => {
              const n = await modelo200PasarSiguiente(h.year);
              setMsg(`${n} pendientes guardados en ${h.year + 1}.`);
              changed();
            }}
          />
        </div>
        {msg && (
          <p role="status" className="mt-1">
            {msg}
          </p>
        )}
      </Seccion>

      {h.notas.length > 0 && (
        <ul className="mt-3 list-disc pl-5 text-stone-600">
          {h.notas.map((n) => (
            <li key={n}>{n}</li>
          ))}
        </ul>
      )}
      {h.avisos.length > 0 && (
        <ul className="mt-2 list-disc pl-5 text-amber-800" aria-label="Avisos del 200">
          {h.avisos.map((a) => (
            <li key={a}>{a}</li>
          ))}
        </ul>
      )}
    </section>
  );
}

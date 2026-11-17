import { useCallback, useEffect, useState } from 'react';
import {
  type AjustesPatrimonial,
  type ClasePatrimonial,
  errorMessage,
  euros,
  type InformePatrimonial,
  type NotaPatrimonial,
  type PartidaPatrimonial,
  parseEuros,
  patrimonialMark,
  patrimonialReport,
  patrimonialSaleAdd,
  patrimonialSaleDelete,
  patrimonialSettings,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const CLASES: { id: ClasePatrimonial; label: string }[] = [
  { id: 'afecto', label: 'Afecto a la actividad' },
  { id: 'no_afecto', label: 'No afecto' },
  { id: 'valores', label: 'Valores (computan)' },
  { id: 'participacion', label: 'Participación ≥ 5 % con medios (no computa)' },
  { id: 'tesoreria', label: 'Tesorería' },
];
const claseLabel = (c: ClasePatrimonial) => CLASES.find((x) => x.id === c)?.label ?? c;

/** Centésimas de punto → «52,3 %». */
export function pct(bp: number): string {
  const v = (bp / 100).toFixed(1).replace('.', ',');
  return `${v.endsWith(',0') ? v.slice(0, -2) : v} %`;
}
const dmy = (iso: string) => {
  const [y, m, d] = iso.split('-');
  return `${d}/${m}/${y}`;
};
const plain = (c: number) => (c / 100).toFixed(2).replace('.', ',');
const cell = 'px-2 py-1 text-right tabular-nums';

function Notas({ titulo, notas }: { titulo: string; notas: NotaPatrimonial[] }) {
  return (
    <details className="mt-3">
      <summary className="cursor-pointer font-medium">{titulo}</summary>
      <ul className="mt-1 list-disc pl-5">
        {notas.map((n) => (
          <li key={n.titulo} className="mt-1">
            <strong>{n.titulo}.</strong> {n.texto}
            {n.norma && <span className="text-stone-500"> ({n.norma})</span>}
          </li>
        ))}
      </ul>
    </details>
  );
}

function FilaPartida({
  p,
  year,
  onSaved,
}: {
  p: PartidaPatrimonial;
  year: number;
  onSaved: (msg: string) => void;
}) {
  const [clase, setClase] = useState<ClasePatrimonial>(p.clase);
  const [motivo, setMotivo] = useState(p.motivo);
  const [error, setError] = useState<string | null>(null);
  const cambiada = clase !== p.clase || motivo !== p.motivo;
  return (
    <tr className="border-t border-stone-100 align-top">
      <td className="pr-2">
        <span className="font-mono">{p.cuenta}</span> {p.nombre}
        {p.marcada && p.desde !== null && p.desde < year && (
          <span className="block text-xs text-stone-500">Marcada desde {p.desde}</span>
        )}
      </td>
      {p.saldos.map((s, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: un saldo por trimestre, en orden fijo
        <td key={i} className={cell}>
          {euros(s)}
        </td>
      ))}
      <td className="px-2">
        <select
          aria-label={`Clase de ${p.cuenta}`}
          value={clase}
          onChange={(e) => setClase(e.target.value as ClasePatrimonial)}
        >
          {CLASES.map((c) => (
            <option key={c.id} value={c.id}>
              {c.label}
              {c.id === p.propuesta ? ' (propuesta)' : ''}
            </option>
          ))}
        </select>
        {p.correctora && (
          <span className="block text-xs text-stone-500">
            Correctora: dale la misma clase que su elemento.
          </span>
        )}
      </td>
      <td className="px-2">
        <input
          aria-label={`Motivo de ${p.cuenta}`}
          className="w-48"
          value={motivo}
          onChange={(e) => setMotivo(e.target.value)}
        />
        {error && (
          <span role="alert" className="block text-red-700">
            {error}
          </span>
        )}
      </td>
      <td className="px-2">
        <button
          type="button"
          className="rounded border border-stone-300 px-2"
          disabled={!cambiada}
          aria-label={`Guardar la clase de ${p.cuenta}`}
          onClick={() =>
            patrimonialMark(year, p.cuenta, clase, motivo)
              .then(() => {
                setError(null);
                onSaved(`${p.cuenta}: ${claseLabel(clase)}.`);
              })
              .catch((e) => setError(errorMessage(e)))
          }
        >
          Guardar
        </button>
      </td>
    </tr>
  );
}

function AjustesForm({
  year,
  inf,
  onSaved,
}: {
  year: number;
  inf: InformePatrimonial;
  onSaved: (msg: string) => void;
}) {
  const a = inf.ajustes;
  const [teso, setTeso] = useState(plain(a.tesoreria_afecta));
  const [tesoMotivo, setTesoMotivo] = useState(a.tesoreria_motivo);
  const [benef, setBenef] = useState(
    a.beneficios_actividad === null ? '' : plain(a.beneficios_actividad),
  );
  const [benefMotivo, setBenefMotivo] = useState(a.beneficios_motivo);
  const [prestamos, setPrestamos] = useState(plain(a.prestamos_participativos));
  const [dominante, setDominante] = useState(a.dominante);
  const [error, setError] = useState<string | null>(null);
  const save = () => {
    const t = parseEuros(teso || '0');
    const p = parseEuros(prestamos || '0');
    const b = benef.trim() === '' ? null : parseEuros(benef);
    if (t === null || p === null || (benef.trim() !== '' && b === null)) {
      setError('Revisa los importes.');
      return;
    }
    const ajustes: AjustesPatrimonial = {
      tesoreria_afecta: t,
      tesoreria_motivo: tesoMotivo,
      beneficios_actividad: b,
      beneficios_motivo: benefMotivo,
      prestamos_participativos: p,
      dominante,
    };
    patrimonialSettings(year, ajustes)
      .then(() => {
        setError(null);
        onSaved('Ajustes guardados.');
      })
      .catch((e) => setError(errorMessage(e)));
  };
  return (
    <fieldset className="mt-4 border-t border-stone-200 pt-3">
      <legend className="font-medium">Lo que no sale de la contabilidad</legend>
      <div className="mt-2 flex flex-wrap gap-3">
        <label>
          Tesorería necesaria para la actividad
          <input
            className="mt-1 block w-36 text-right"
            value={teso}
            onChange={(e) => setTeso(e.target.value)}
          />
        </label>
        <label>
          Cómo se justifica
          <input
            className="mt-1 block w-64"
            value={tesoMotivo}
            onChange={(e) => setTesoMotivo(e.target.value)}
          />
        </label>
      </div>
      <div className="mt-2 flex flex-wrap gap-3">
        <label>
          Beneficios no distribuidos de la actividad (año y diez anteriores)
          <input
            className="mt-1 block w-36 text-right"
            value={benef}
            placeholder="Sin confirmar"
            onChange={(e) => setBenef(e.target.value)}
          />
        </label>
        <label>
          De dónde sale
          <input
            className="mt-1 block w-64"
            value={benefMotivo}
            onChange={(e) => setBenefMotivo(e.target.value)}
          />
        </label>
      </div>
      <p className="text-xs text-stone-500">
        Reservas y resultados del balance: {euros(inf.beneficios_propuestos)}. Quita lo que no venga
        de la actividad (dividendos, intereses, plusvalías de inversiones). Sólo se usa en el
        cálculo del Patrimonio del socio.
      </p>
      <div className="mt-2 flex flex-wrap items-end gap-3">
        <label>
          Préstamos participativos recibidos
          <input
            className="mt-1 block w-36 text-right"
            value={prestamos}
            onChange={(e) => setPrestamos(e.target.value)}
          />
        </label>
        <label className="flex items-center gap-1">
          <input
            type="checkbox"
            checked={dominante}
            onChange={(e) => setDominante(e.target.checked)}
          />
          Es dominante de un grupo (art. 42 del Código de Comercio)
        </label>
      </div>
      <button type="button" className="mt-2 rounded btn-primary px-3 py-1" onClick={save}>
        Guardar ajustes
      </button>
      {error && (
        <p role="alert" className="mt-1 text-red-700">
          {error}
        </p>
      )}
    </fieldset>
  );
}

function Ventas({ inf, onSaved }: { inf: InformePatrimonial; onSaved: (msg: string) => void }) {
  const [fecha, setFecha] = useState('');
  const [importe, setImporte] = useState('');
  const [motivo, setMotivo] = useState('');
  const [error, setError] = useState<string | null>(null);
  const add = () => {
    const c = parseEuros(importe);
    if (!/^\d{4}-\d{2}-\d{2}$/.test(fecha) || c === null || c <= 0) {
      setError('Pon la fecha y el importe de la venta.');
      return;
    }
    patrimonialSaleAdd(fecha, c, motivo)
      .then(() => {
        setError(null);
        setFecha('');
        setImporte('');
        setMotivo('');
        onSaved('Venta anotada.');
      })
      .catch((e) => setError(errorMessage(e)));
  };
  return (
    <fieldset className="mt-4 border-t border-stone-200 pt-3">
      <legend className="font-medium">
        Ventas de elementos afectos (este ejercicio y los dos anteriores)
      </legend>
      <p className="text-stone-600">
        El dinero o el crédito de la venta no computa desde la fecha de la venta (art. 5.2 LIS). Se
        resta primero de la tesorería y después de los créditos por la venta (253, 543).
      </p>
      {inf.ventas.length > 0 && (
        <ul className="mt-1">
          {inf.ventas.map((v) => (
            <li key={v.id} className="flex items-center gap-2">
              <span>
                {dmy(v.fecha)} · {euros(v.importe)} · {v.motivo}
              </span>
              <ConfirmAction
                label="Quitar"
                ariaLabel={`Quitar la venta del ${dmy(v.fecha)}`}
                question={<>¿Quitar la venta del {dmy(v.fecha)}? El cálculo se rehace sin ella.</>}
                confirmLabel="Sí, quitarla"
                onConfirm={async () => {
                  await patrimonialSaleDelete(v.id, true);
                  onSaved('Venta quitada.');
                }}
              />
            </li>
          ))}
        </ul>
      )}
      <div className="mt-2 flex flex-wrap items-end gap-3">
        <label>
          Fecha de la venta
          <input
            type="date"
            className="mt-1 block"
            value={fecha}
            onChange={(e) => setFecha(e.target.value)}
          />
        </label>
        <label>
          Importe
          <input
            className="mt-1 block w-32 text-right"
            value={importe}
            onChange={(e) => setImporte(e.target.value)}
          />
        </label>
        <label>
          Qué se vendió
          <input
            className="mt-1 block w-64"
            value={motivo}
            onChange={(e) => setMotivo(e.target.value)}
          />
        </label>
        <button type="button" className="rounded border border-stone-300 px-3" onClick={add}>
          Anotar venta
        </button>
      </div>
      {error && (
        <p role="alert" className="mt-1 text-red-700">
          {error}
        </p>
      )}
    </fieldset>
  );
}

/**
 * Entidad patrimonial (art. 5.2 LIS) con la media de los balances trimestrales y avisos del
 * patrimonio neto frente al capital (arts. 327 y 363.1.e LSC). Enseña el cálculo: no da un
 * veredicto cerrado.
 */
export function EntidadPatrimonial({ year }: { year: number }) {
  const [inf, setInf] = useState<InformePatrimonial | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [oculto, setOculto] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [version, setVersion] = useState(0);
  const load = useCallback(() => {
    patrimonialReport(year)
      .then((r) => {
        setInf(r);
        setError(null);
        setVersion((v) => v + 1);
      })
      .catch((e) => {
        const m = errorMessage(e);
        // Un autónomo: no es de su caso.
        if (m.includes('son de las sociedades')) setOculto(true);
        else setError(m);
      });
  }, [year]);
  useEffect(load, [load]);
  const saved = (m: string) => {
    setMsg(m);
    load();
  };
  if (oculto) return null;
  const titulo = `Entidad patrimonial y patrimonio neto ${year}`;
  if (!inf) {
    return (
      <section aria-label={titulo} className="mt-6 text-sm">
        <h3 className="font-semibold">{titulo}</h3>
        {error && (
          <p role="alert" className="text-red-700">
            No se pudo cargar: {error}
          </p>
        )}
      </section>
    );
  }
  const m = inf.media;
  const filas: { label: string; get: (i: number) => number; media?: number }[] = [
    { label: 'Activo', get: (i) => inf.trimestres[i]?.activo ?? 0, media: m.activo },
    { label: 'Afecto', get: (i) => inf.trimestres[i]?.afecto ?? 0 },
    {
      label: 'Valores que computan',
      get: (i) => inf.trimestres[i]?.valores ?? 0,
      media: m.valores,
    },
    {
      label: 'No afecto que computa (con la tesorería no justificada)',
      get: (i) => inf.trimestres[i]?.no_afecto ?? 0,
      media: m.no_afecto,
    },
    {
      label: 'Participaciones que no computan',
      get: (i) => inf.trimestres[i]?.participaciones ?? 0,
      media: m.participaciones,
    },
    {
      label: 'De ello, tesorería no justificada',
      get: (i) => inf.trimestres[i]?.tesoreria_no_justificada ?? 0,
    },
    {
      label: 'Tesorería justificada como afecta',
      get: (i) => inf.trimestres[i]?.tesoreria_justificada ?? 0,
    },
    {
      label: 'Dinero o créditos de ventas de elementos afectos',
      get: (i) => inf.trimestres[i]?.ventas_excluidas ?? 0,
    },
  ];
  const pn = inf.patrimonio;
  return (
    <section aria-label={titulo} className="mt-6 text-sm">
      <h3 className="font-semibold">{titulo}</h3>
      <p className="text-stone-600">
        Es entidad patrimonial la sociedad con más de la mitad de su activo en valores o en bienes
        no afectos a una actividad económica, con la media de los balances trimestrales del
        ejercicio (art. 5.2 de la Ley 27/2014). TaxPain toma el saldo de la contabilidad al último
        día de cada trimestre y propone una clase para cada partida; cámbiala con su motivo si no es
        la que corresponde. Es un cálculo para revisarlo, no un veredicto.
      </p>
      {inf.avisos.map((a) => (
        <p key={a} className="mt-1 text-amber-800">
          {a}
        </p>
      ))}
      <p className={`mt-2 font-medium ${inf.patrimonial ? 'text-red-700' : ''}`} role="status">
        Media de {m.trimestres} {m.trimestres === 1 ? 'trimestre' : 'trimestres'}: computa el{' '}
        {pct(m.pct)} del activo ({euros(m.computa)} de {euros(m.activo)}).{' '}
        {inf.patrimonial
          ? `Supera el 50 %: con lo marcado, la sociedad sería patrimonial (sobran ${euros(-m.margen)}).`
          : `No llega al 50 % (margen de ${euros(m.margen)} con el mismo activo).`}
      </p>
      {m.pct_con_participaciones !== m.pct && (
        <p className="text-stone-600">
          Contando también las participaciones marcadas como de la letra d):{' '}
          {pct(m.pct_con_participaciones)}.
        </p>
      )}
      {m.pct_sin_tesoreria !== m.pct && (
        <p className="text-stone-600">
          Sin la tesorería que no has justificado como afecta: {pct(m.pct_sin_tesoreria)}. La ley no
          dice cómo se trata la tesorería; indica abajo la que necesita la actividad.
        </p>
      )}
      <section
        className="mt-2 overflow-x-auto"
        // biome-ignore lint/a11y/noNoninteractiveTabindex: tabla desplazable; necesita foco para usarla con el teclado (WCAG 2.1.1)
        tabIndex={0}
        aria-label={`Activo por trimestres ${year}`}
      >
        <table>
          <caption className="sr-only">Activo por trimestres {year}</caption>
          <thead className="text-left text-stone-500">
            <tr>
              <th scope="col">Partida</th>
              {inf.trimestres.map((t) => (
                <th key={t.fecha} scope="col" className="px-2 text-right">
                  {dmy(t.fecha)}
                  {!t.cerrado && <span className="block text-xs">(sin terminar)</span>}
                </th>
              ))}
              <th scope="col" className="px-2 text-right">
                Media
              </th>
            </tr>
          </thead>
          <tbody>
            {filas.map((f) => (
              <tr key={f.label}>
                <th scope="row" className="pr-2 text-left font-normal">
                  {f.label}
                </th>
                {inf.trimestres.map((t, i) => (
                  <td key={t.fecha} className={cell}>
                    {euros(f.get(i))}
                  </td>
                ))}
                <td className={cell}>{f.media === undefined ? '' : euros(f.media)}</td>
              </tr>
            ))}
            <tr className="font-medium">
              <th scope="row" className="pr-2 text-left">
                % que computa
              </th>
              {inf.trimestres.map((t) => (
                <td key={t.fecha} className={cell}>
                  {pct(t.pct)}
                </td>
              ))}
              <td className={cell}>{pct(m.pct)}</td>
            </tr>
          </tbody>
        </table>
      </section>

      <p className="mt-4 font-medium">Partidas del activo</p>
      <section
        className="overflow-x-auto"
        // biome-ignore lint/a11y/noNoninteractiveTabindex: tabla desplazable; necesita foco para usarla con el teclado (WCAG 2.1.1)
        tabIndex={0}
        aria-label="Partidas del activo"
      >
        <table>
          <thead className="text-left text-stone-500">
            <tr>
              <th scope="col">Cuenta</th>
              {inf.fechas.map((f) => (
                <th key={f} scope="col" className="px-2 text-right">
                  {dmy(f)}
                </th>
              ))}
              <th scope="col" className="px-2">
                Clase
              </th>
              <th scope="col" className="px-2">
                Motivo
              </th>
              <th scope="col">
                <span className="sr-only">Guardar</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {inf.partidas.map((p) => (
              <FilaPartida key={`${p.cuenta}-${version}`} p={p} year={year} onSaved={saved} />
            ))}
          </tbody>
        </table>
      </section>
      <p className="text-xs text-stone-500">
        La marca vale desde {year} para los ejercicios siguientes, hasta que la cambies.
      </p>

      <AjustesForm key={`a-${version}`} year={year} inf={inf} onSaved={saved} />
      <Ventas inf={inf} onSaved={saved} />

      <p className="mt-3 text-stone-600">
        Patrimonio del socio (orientativo): restando los beneficios no distribuidos de la actividad,{' '}
        {inf.trimestres_lip} de {inf.trimestres.filter((t) => t.cerrado).length} trimestres
        terminados superan el 50 %. La exención del Impuesto sobre el Patrimonio mira si pasa
        durante más de 90 días del ejercicio (art. 4.Ocho.Dos de la Ley 19/1991).
      </p>
      <Notas
        titulo={
          inf.patrimonial
            ? 'Qué supone ser entidad patrimonial'
            : 'Qué supondría ser entidad patrimonial'
        }
        notas={inf.consecuencias}
      />
      <Notas titulo="Qué se suele revisar" notas={inf.que_revisar} />

      <p className="mt-5 font-medium">Patrimonio neto frente al capital social</p>
      <section
        className="overflow-x-auto"
        // biome-ignore lint/a11y/noNoninteractiveTabindex: tabla desplazable; necesita foco para usarla con el teclado (WCAG 2.1.1)
        tabIndex={0}
        aria-label="Patrimonio neto frente al capital social"
      >
        <table>
          <thead className="text-left text-stone-500">
            <tr>
              <th scope="col">Fecha</th>
              <th scope="col" className="px-2 text-right">
                Capital
              </th>
              <th scope="col" className="px-2 text-right">
                Patrimonio neto contable
              </th>
              <th scope="col" className="px-2 text-right">
                Ajustes del capital
              </th>
              <th scope="col" className="px-2 text-right">
                Préstamos participativos
              </th>
              <th scope="col" className="px-2 text-right">
                Patrimonio neto mercantil
              </th>
              <th scope="col" className="px-2 text-right">
                % del capital
              </th>
            </tr>
          </thead>
          <tbody>
            {[...(pn.anterior ? [pn.anterior] : []), ...pn.fechas].map((f) => (
              <tr key={f.fecha}>
                <th scope="row" className="pr-2 text-left font-normal">
                  {dmy(f.fecha)}
                  {!f.cerrado && <span className="text-xs"> (sin terminar)</span>}
                </th>
                <td className={cell}>{euros(f.capital)}</td>
                <td className={cell}>{euros(f.contable)}</td>
                <td className={cell}>{euros(f.capital_ajustes - f.coberturas)}</td>
                <td className={cell}>{euros(f.prestamos_participativos)}</td>
                <td className={cell}>{euros(f.mercantil)}</td>
                <td className={cell}>{f.capital > 0 ? pct(f.pct_capital) : '—'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
      {pn.avisos.length === 0 ? (
        <p className="mt-1">
          Sin avisos: el patrimonio neto no está por debajo del capital social.
        </p>
      ) : (
        <ul className="mt-2">
          {pn.avisos.map((a) => (
            <li
              key={a.codigo}
              role={a.gravedad === 'warning' ? 'alert' : undefined}
              className={`mt-2 rounded border p-2 ${a.gravedad === 'warning' ? 'border-red-300 bg-red-50' : 'border-stone-200'}`}
            >
              <strong>{a.titulo}.</strong> {a.detalle}{' '}
              <span className="text-stone-500">({a.norma})</span>
            </li>
          ))}
        </ul>
      )}
      <Notas titulo="Cómo se calcula el patrimonio neto" notas={pn.notas} />
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}
    </section>
  );
}

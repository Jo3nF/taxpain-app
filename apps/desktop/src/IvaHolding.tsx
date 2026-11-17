import { useCallback, useEffect, useState } from 'react';
import {
  type AmbitoRegla,
  type DatosBienIva,
  type DiagnosticoHolding,
  errorMessage,
  euros,
  type FilaBienInversion,
  ivaHoldingBienGuardar,
  ivaHoldingConfig,
  ivaHoldingInforme,
  ivaHoldingLibro,
  ivaHoldingReglaBorrar,
  ivaHoldingReglaGuardar,
  ivaHoldingSectorBorrar,
  ivaHoldingSectorGuardar,
  type LibroBienesInversion,
  type PapelSector,
  parseEuros,
  type ReglaAfectacion,
  type VentaIva,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const cell = 'px-2 py-1 text-right tabular-nums';
const dmy = (iso: string) => {
  const [y, m, d] = iso.split('-');
  return `${d}/${m}/${y}`;
};
const pctTxt = (p: number | null) => (p === null ? '—' : `${p} %`);

const PAPEL: Record<PapelSector, string> = {
  principal: 'Principal',
  mismo_sector: 'Mismo sector que la principal',
  accesoria: 'Posible accesoria (sigue a la principal si contribuye a ella)',
  diferenciado: 'Sector diferenciado',
  sin_cnae: 'Sin CNAE: no se puede comprobar',
};

const VENTAS: { id: VentaIva; label: string }[] = [
  { id: 'sujeta', label: 'Sujeta y no exenta (o exenta con derecho)' },
  { id: 'exenta', label: 'Exenta sin derecho a deducir' },
  { id: 'no_sujeta', label: 'No sujeta' },
  { id: 'transmision_empresa', label: 'Transmisión de la empresa (art. 7.1.º)' },
  { id: 'perdida', label: 'Pérdida o inutilización no imputable' },
];
const ventaLabel = (v: VentaIva | null) =>
  v === null ? 'Sin saber cómo tributó' : (VENTAS.find((x) => x.id === v)?.label ?? v);

function hoyIso() {
  const d = new Date();
  const m = Math.floor(d.getMonth() / 3) * 3 + 1;
  return `${d.getFullYear()}-${String(m).padStart(2, '0')}-01`;
}

function Diagnostico({ d }: { d: DiagnosticoHolding }) {
  return (
    <section aria-labelledby="iva-holding-diag">
      <h3 id="iva-holding-diag" className="font-medium">
        Holding, cartera y deducción del IVA ({d.year})
      </h3>
      <p className="mt-1">{d.perfil_texto}</p>
      {d.avisos.map((a) => (
        <p key={a} role="alert" className="mt-1 text-amber-800">
          {a}
        </p>
      ))}
      <table className="mt-2 text-sm">
        <caption className="sr-only">Lo que se ve en la contabilidad</caption>
        <tbody>
          {d.senales.map((s) => (
            <tr key={s.clave} className="border-t border-stone-100">
              <th scope="row" className="pr-3 text-left font-normal">
                {s.titulo}
              </th>
              <td className={cell}>{euros(s.importe)}</td>
              <td className="px-2 text-stone-600">{s.detalle}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <dl className="mt-3 grid grid-cols-[auto_auto] gap-x-4 gap-y-1">
        <dt>Prorrata con las facturas emitidas (art. 104.Dos)</dt>
        <dd className="tabular-nums">{pctTxt(d.prorrata_facturas)}</dd>
        <dt>Con los ingresos financieros, si son habituales</dt>
        <dd className="tabular-nums">{pctTxt(d.prorrata_con_financieros)}</dd>
        <dt>Proporción para gastos comunes (sujetas / total de ingresos, art. 93.Cinco)</dt>
        <dd className="tabular-nums">{pctTxt(d.proporcion_93)}</dd>
        <dt>Prorrata configurada en Ajustes de modelos</dt>
        <dd className="tabular-nums">
          {d.prorrata_configurada === null
            ? 'Sin prorrata'
            : `${d.prorrata_configurada} %${d.prorrata_especial ? ' (especial)' : ''}`}
        </dd>
      </dl>
      <p className="mt-1 text-xs text-stone-500">
        Son cálculos para orientarte, no una calificación: el criterio lo eliges tú y queda escrito
        en cada regla.
      </p>
      <details className="mt-2">
        <summary className="cursor-pointer font-medium">Cómo afecta a la deducción</summary>
        <ul className="mt-1 list-disc pl-5">
          {d.notas.map((n) => (
            <li key={n.titulo} className="mt-1">
              <strong>{n.titulo}.</strong> {n.texto}{' '}
              <span className="text-stone-500">({n.norma})</span>
              {n.por_cotejar && <span className="ml-1 text-amber-800">Por cotejar.</span>}
            </li>
          ))}
        </ul>
      </details>
    </section>
  );
}

const reglaVacia = (pct: number | null): ReglaAfectacion => ({
  id: 0,
  ambito: 'cuenta',
  clave: '',
  pct: pct ?? 100,
  sector_id: null,
  criterio: '',
  desde: hoyIso(),
});

function Reglas({ d, onSaved }: { d: DiagnosticoHolding; onSaved: (m: string) => void }) {
  const [r, setR] = useState<ReglaAfectacion>(reglaVacia(d.proporcion_93));
  const [error, setError] = useState<string | null>(null);
  const sectorNombre = (id: number | null) =>
    id === null ? '' : (d.sectores.find((s) => s.id === id)?.nombre ?? '');
  const save = () =>
    ivaHoldingReglaGuardar(r)
      .then(() => {
        setError(null);
        setR(reglaVacia(d.proporcion_93));
        onSaved(r.id ? 'Regla cambiada.' : 'Regla añadida.');
      })
      .catch((e) => setError(errorMessage(e)));
  return (
    <section aria-labelledby="iva-holding-reglas" className="mt-6">
      <h3 id="iva-holding-reglas" className="font-medium">
        Afectación de gastos comunes
      </h3>
      <p className="text-stone-600">
        Porcentaje de la cuota que se deduce de las recibidas de un proveedor o de una cuenta de
        gasto, desde una fecha. Se aplica en el 303 por la fecha de registro (si la factura ya tenía
        un porcentaje menor, se queda el menor); la diferencia con la 472 va a la 634 al liquidar el
        IVA.
      </p>
      {d.reglas.length > 0 && (
        <table className="mt-2 text-sm">
          <caption className="sr-only">Reglas de afectación</caption>
          <thead>
            <tr className="text-left">
              <th className="pr-2">Para</th>
              <th className="px-2 text-right">Deducible</th>
              <th className="px-2">Desde</th>
              <th className="px-2">Criterio</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {d.reglas.map((x) => (
              <tr key={x.id} className="border-t border-stone-100 align-top">
                <td className="pr-2">
                  {x.ambito === 'proveedor' ? 'Proveedor' : 'Cuenta'}{' '}
                  <span className="font-mono">{x.clave}</span>
                  {x.sector_id !== null && (
                    <span className="block text-xs text-stone-500">
                      Sector: {sectorNombre(x.sector_id)}
                    </span>
                  )}
                </td>
                <td className={cell}>{x.pct} %</td>
                <td className="px-2">{dmy(x.desde)}</td>
                <td className="max-w-md px-2">{x.criterio}</td>
                <td className="px-2 whitespace-nowrap">
                  <button
                    type="button"
                    className="text-link underline"
                    aria-label={`Cambiar la regla de ${x.clave}`}
                    onClick={() => setR(x)}
                  >
                    Cambiar
                  </button>{' '}
                  <ConfirmAction
                    label="Borrar"
                    ariaLabel={`Borrar la regla de ${x.clave}`}
                    question={
                      <>
                        ¿Borrar la regla de {x.clave}? Los 303 que calcules después deducirán el
                        porcentaje de cada factura; los ya presentados no cambian.
                      </>
                    }
                    confirmLabel="Sí, borrarla"
                    onConfirm={async () => {
                      await ivaHoldingReglaBorrar(x.id, true);
                      onSaved('Regla borrada.');
                    }}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <fieldset className="mt-3 border-t border-stone-200 pt-2">
        <legend className="font-medium">{r.id ? 'Cambiar la regla' : 'Nueva regla'}</legend>
        <div className="mt-1 flex flex-wrap items-end gap-3">
          <label>
            Para
            <select
              className="mt-1 block"
              value={r.ambito}
              onChange={(e) => setR({ ...r, ambito: e.target.value as AmbitoRegla })}
            >
              <option value="cuenta">Una cuenta de gasto</option>
              <option value="proveedor">Un proveedor</option>
            </select>
          </label>
          <label>
            {r.ambito === 'cuenta' ? 'Cuenta (6230, 629…)' : 'NIF del proveedor'}
            <input
              className="mt-1 block w-32"
              value={r.clave}
              onChange={(e) => setR({ ...r, clave: e.target.value })}
            />
          </label>
          <label>
            % deducible
            <input
              type="number"
              min={0}
              max={100}
              className="mt-1 block w-20 text-right"
              value={r.pct}
              onChange={(e) => setR({ ...r, pct: Number(e.target.value) })}
            />
          </label>
          <label>
            Desde
            <input
              type="date"
              className="mt-1 block"
              value={r.desde}
              onChange={(e) => setR({ ...r, desde: e.target.value })}
            />
          </label>
          {d.sectores.length > 0 && (
            <label>
              Sector
              <select
                className="mt-1 block"
                value={r.sector_id ?? ''}
                onChange={(e) =>
                  setR({ ...r, sector_id: e.target.value ? Number(e.target.value) : null })
                }
              >
                <option value="">Común a todos</option>
                {d.sectores.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.nombre}
                  </option>
                ))}
              </select>
            </label>
          )}
        </div>
        <label className="mt-2 block">
          Criterio (por qué este porcentaje)
          <textarea
            className="mt-1 block w-full max-w-2xl"
            rows={2}
            value={r.criterio}
            onChange={(e) => setR({ ...r, criterio: e.target.value })}
          />
        </label>
        {d.proporcion_93 !== null && r.id === 0 && (
          <p className="text-xs text-stone-500">
            Propuesta: {d.proporcion_93} %, la proporción de operaciones sujetas sobre el total de
            ingresos de {d.year}. Mantén el criterio en el tiempo.
          </p>
        )}
        <div className="mt-2 flex gap-2">
          <button type="button" className="rounded btn-primary px-3 py-1" onClick={save}>
            {r.id ? 'Guardar cambios' : 'Añadir regla'}
          </button>
          {r.id !== 0 && (
            <button
              type="button"
              className="rounded border border-stone-300 px-3 py-1"
              onClick={() => setR(reglaVacia(d.proporcion_93))}
            >
              Cancelar
            </button>
          )}
        </div>
        {error && (
          <p role="alert" className="mt-1 text-red-700">
            {error}
          </p>
        )}
      </fieldset>
      {d.aplicaciones.length > 0 && (
        <table className="mt-3 text-sm">
          <caption className="text-left font-medium">
            Aplicado en {d.year}: {euros(d.no_deducido)} de IVA que no se deduce
          </caption>
          <thead>
            <tr className="text-left">
              <th className="pr-2">Factura</th>
              <th className="px-2">Registro</th>
              <th className="px-2 text-right">Cuota</th>
              <th className="px-2 text-right">Factura</th>
              <th className="px-2 text-right">Aplicado</th>
              <th className="px-2 text-right">No deducido</th>
            </tr>
          </thead>
          <tbody>
            {d.aplicaciones.map((a) => (
              <tr key={a.recibida_id} className="border-t border-stone-100">
                <td className="pr-2">
                  {a.numero} · {a.proveedor}
                  {a.sector && (
                    <span className="block text-xs text-stone-500">Sector: {a.sector}</span>
                  )}
                </td>
                <td className="px-2">{dmy(a.registro)}</td>
                <td className={cell}>{euros(a.cuota)}</td>
                <td className={cell}>{a.pct_factura} %</td>
                <td className={cell}>{a.pct_aplicado} %</td>
                <td className={cell}>{euros(a.no_deducido)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

function Sectores({ d, onSaved }: { d: DiagnosticoHolding; onSaved: (m: string) => void }) {
  const [nombre, setNombre] = useState('');
  const [cnae, setCnae] = useState('');
  const [pct, setPct] = useState('100');
  const [volumen, setVolumen] = useState('');
  const [error, setError] = useState<string | null>(null);
  const add = () => {
    const vol = parseEuros(volumen || '0');
    const p = Number(pct);
    if (vol === null || !Number.isInteger(p) || p < 0 || p > 100) {
      setError('Revisa el porcentaje y el volumen.');
      return;
    }
    ivaHoldingSectorGuardar({ id: 0, nombre, cnae, pct: p, volumen: vol, notas: '' })
      .then(() => {
        setError(null);
        setNombre('');
        setCnae('');
        setVolumen('');
        onSaved('Actividad añadida.');
      })
      .catch((e) => setError(errorMessage(e)));
  };
  const separar = d.config.sectores_diferenciados;
  return (
    <section aria-labelledby="iva-holding-sectores" className="mt-6">
      <h3 id="iva-holding-sectores" className="font-medium">
        Sectores diferenciados (art. 9.1.º.c LIVA)
      </h3>
      <p className="text-stone-600">
        Anota cada actividad con su CNAE, su porcentaje de deducción y su volumen de operaciones del
        año anterior. TaxPain comprueba si alguna forma un sector aparte.
      </p>
      {d.comprobacion.filas.length > 0 && (
        <table className="mt-2 text-sm">
          <caption className="sr-only">Actividades anotadas</caption>
          <thead>
            <tr className="text-left">
              <th className="pr-2">Actividad</th>
              <th className="px-2">CNAE</th>
              <th className="px-2 text-right">Deducción</th>
              <th className="px-2 text-right">Volumen</th>
              <th className="px-2">Resultado</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {d.comprobacion.filas.map((f) => (
              <tr key={f.id} className="border-t border-stone-100">
                <td className="pr-2">{f.nombre}</td>
                <td className="px-2 font-mono">{f.cnae || '—'}</td>
                <td className={cell}>{f.pct} %</td>
                <td className={cell}>{euros(f.volumen)}</td>
                <td className="px-2">
                  {PAPEL[f.papel]}
                  {f.papel !== 'principal' && ` (${f.diferencia} puntos)`}
                </td>
                <td className="px-2">
                  <ConfirmAction
                    label="Borrar"
                    ariaLabel={`Borrar la actividad ${f.nombre}`}
                    question={
                      <>¿Borrar «{f.nombre}»? Las reglas que la usaban quedan sin sector.</>
                    }
                    confirmLabel="Sí, borrarla"
                    onConfirm={async () => {
                      await ivaHoldingSectorBorrar(f.id, true);
                      onSaved('Actividad borrada.');
                    }}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {d.comprobacion.filas.length > 0 && <p className="mt-1">{d.comprobacion.explicacion}</p>}
      <div className="mt-2 flex flex-wrap items-end gap-3">
        <label>
          Actividad
          <input
            className="mt-1 block w-56"
            value={nombre}
            onChange={(e) => setNombre(e.target.value)}
          />
        </label>
        <label>
          CNAE
          <input
            className="mt-1 block w-20"
            value={cnae}
            onChange={(e) => setCnae(e.target.value)}
          />
        </label>
        <label>
          % de deducción
          <input
            type="number"
            min={0}
            max={100}
            className="mt-1 block w-20 text-right"
            value={pct}
            onChange={(e) => setPct(e.target.value)}
          />
        </label>
        <label>
          Volumen del año anterior
          <input
            className="mt-1 block w-32 text-right"
            value={volumen}
            onChange={(e) => setVolumen(e.target.value)}
          />
        </label>
        <button type="button" className="rounded border border-stone-300 px-3 py-1" onClick={add}>
          Añadir actividad
        </button>
      </div>
      {error && (
        <p role="alert" className="mt-1 text-red-700">
          {error}
        </p>
      )}
      <div className="mt-3">
        <p>
          {separar
            ? 'Se separa la deducción: las recibidas con una regla de sector deducen con el porcentaje de ese sector y sin la prorrata general (art. 101 LIVA).'
            : 'No se separa la deducción por sectores.'}
        </p>
        <ConfirmAction
          label={separar ? 'Dejar de separar por sectores' : 'Separar la deducción por sectores'}
          className="text-link underline"
          danger={false}
          question={
            separar
              ? '¿Dejar de separar? Los 303 que calcules después aplicarán a todo la prorrata general.'
              : '¿Separar la deducción? Las recibidas con una regla de sector deducirán con el porcentaje de su sector en los 303 que calcules después. Los ya presentados no cambian.'
          }
          confirmLabel={separar ? 'Sí, dejar de separar' : 'Sí, separar'}
          onConfirm={async () => {
            await ivaHoldingConfig({ sectores_diferenciados: !separar });
            onSaved(separar ? 'Ya no se separa por sectores.' : 'Se separa por sectores.');
          }}
        />
      </div>
    </section>
  );
}

function DatosBien({
  assetId,
  nombre,
  conRecibida,
  onSaved,
}: {
  assetId: number;
  nombre: string;
  conRecibida: boolean;
  onSaved: (m: string) => void;
}) {
  const [cuota, setCuota] = useState('');
  const [inmueble, setInmueble] = useState(false);
  const [venta, setVenta] = useState<VentaIva | ''>('');
  const [ivaVenta, setIvaVenta] = useState('');
  const [error, setError] = useState<string | null>(null);
  const save = () => {
    const c = cuota.trim() ? parseEuros(cuota) : null;
    const iv = ivaVenta.trim() ? parseEuros(ivaVenta) : null;
    if ((cuota.trim() && c === null) || (ivaVenta.trim() && iv === null)) {
      setError('Revisa los importes.');
      return;
    }
    const datos: DatosBienIva = {
      cuota: conRecibida ? null : c,
      deducible: null,
      inmueble,
      venta: venta || null,
      iva_venta: iv,
    };
    ivaHoldingBienGuardar(assetId, datos)
      .then(() => {
        setError(null);
        onSaved(`Datos de IVA de ${nombre} guardados.`);
      })
      .catch((e) => setError(errorMessage(e)));
  };
  return (
    <details className="mt-1">
      <summary className="cursor-pointer text-link">Anotar datos de IVA de {nombre}</summary>
      <div className="mt-1 flex flex-wrap items-end gap-3">
        {!conRecibida && (
          <label>
            IVA soportado
            <input
              className="mt-1 block w-28 text-right"
              value={cuota}
              onChange={(e) => setCuota(e.target.value)}
            />
          </label>
        )}
        <label className="flex items-center gap-1">
          <input
            type="checkbox"
            checked={inmueble}
            onChange={(e) => setInmueble(e.target.checked)}
          />
          Terreno o edificación (diez años)
        </label>
        <label>
          Entrega
          <select
            className="mt-1 block"
            value={venta}
            onChange={(e) => setVenta(e.target.value as VentaIva | '')}
          >
            <option value="">Según la factura de venta</option>
            {VENTAS.map((x) => (
              <option key={x.id} value={x.id}>
                {x.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          IVA repercutido en la venta
          <input
            className="mt-1 block w-28 text-right"
            value={ivaVenta}
            onChange={(e) => setIvaVenta(e.target.value)}
          />
        </label>
        <button type="button" className="rounded border border-stone-300 px-3 py-1" onClick={save}>
          Guardar
        </button>
      </div>
      {error && (
        <p role="alert" className="mt-1 text-red-700">
          {error}
        </p>
      )}
    </details>
  );
}

function FilaBien({ b, onSaved }: { b: FilaBienInversion; onSaved: (m: string) => void }) {
  return (
    <li className="mt-3 border-t border-stone-100 pt-2">
      <p>
        <strong>{b.nombre}</strong> · {dmy(b.fecha)}
        {b.factura && ` · factura ${b.factura} (${b.nif})`} · {b.inmueble ? 'inmueble, ' : ''}
        regulariza hasta {b.fin_periodo}
      </p>
      <p className="text-stone-600">
        Base {euros(b.base)} · IVA {euros(b.cuota)} · deducible {b.pct_factura} % · deducido el año
        de compra al {pctTxt(b.prorrata_compra)}
        {b.deduccion_inicial !== null && ` (${euros(b.deduccion_inicial)})`}
      </p>
      {b.anios.length > 0 && (
        <table className="mt-1 text-sm">
          <caption className="sr-only">Regularización anual de {b.nombre}</caption>
          <thead>
            <tr className="text-left">
              <th className="pr-2">Año</th>
              <th className="px-2 text-right">Prorrata</th>
              <th className="px-2 text-right">Diferencia</th>
              <th className="px-2 text-right">Regularización</th>
              <th className="px-2">Nota</th>
            </tr>
          </thead>
          <tbody>
            {b.anios.map((a) => (
              <tr key={a.year}>
                <td className="pr-2">{a.year}</td>
                <td className={cell}>{pctTxt(a.prorrata)}</td>
                <td className={cell}>{a.diferencia === null ? '—' : `${a.diferencia} puntos`}</td>
                <td className={cell}>{euros(a.importe)}</td>
                <td className="px-2 text-stone-600">{a.nota}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {b.venta && (
        <p className="mt-1">
          Entregado el {dmy(b.venta.fecha)}: {ventaLabel(b.venta.tipo)}
          {b.venta.origen && ` (${b.venta.origen})`}.{' '}
          {b.venta.regularizacion !== null
            ? `Regularización única por ${b.venta.anios_restantes} años: ${euros(b.venta.regularizacion)} en [43] del 303 de ese periodo (art. 110 LIVA).`
            : 'Sin regularización por la entrega.'}
        </p>
      )}
      {b.avisos.map((a) => (
        <p key={a} className="text-amber-800">
          {a}
        </p>
      ))}
      {b.asset_id !== null && (
        <DatosBien
          assetId={b.asset_id}
          nombre={b.nombre}
          conRecibida={b.received_id !== null}
          onSaved={onSaved}
        />
      )}
    </li>
  );
}

function Libro({ l, onSaved }: { l: LibroBienesInversion; onSaved: (m: string) => void }) {
  return (
    <section aria-labelledby="iva-holding-libro" className="mt-6">
      <h3 id="iva-holding-libro" className="font-medium">
        Libro registro de bienes de inversión ({l.year})
      </h3>
      <p className="text-stone-600">
        Bienes de inversión de las recibidas y del inmovilizado durante su periodo de
        regularización: cuatro años después del de compra (nueve los inmuebles). Si la prorrata
        definitiva de un año difiere en más de 10 puntos de la del año de compra, se regulariza la
        quinta (o décima) parte en el último 303 del año (arts. 107 y 109 LIVA); si se vende antes,
        de una vez por lo que queda (art. 110).
      </p>
      <p className="mt-1">
        Regularización anual de {l.year}: <strong>{euros(l.total_anual)}</strong> · por ventas del
        año: <strong>{euros(l.total_ventas)}</strong>
        {!l.con_prorrata && ' · sin prorrata configurada: sólo se regularizan las ventas'}
      </p>
      {l.avisos.map((a) => (
        <p key={a} className="text-amber-800">
          {a}
        </p>
      ))}
      {l.bienes.length === 0 ? (
        <p className="mt-1 text-stone-500">
          No hay bienes de inversión en periodo de regularización.
        </p>
      ) : (
        <ul>
          {l.bienes.map((b) => (
            <FilaBien key={`${b.asset_id}-${b.received_id}`} b={b} onSaved={onSaved} />
          ))}
        </ul>
      )}
      {l.sin_datos.length > 0 && (
        <div className="mt-3">
          <p className="font-medium">Inmovilizado sin datos de IVA</p>
          <ul>
            {l.sin_datos.map((a) => (
              <li key={a.asset_id} className="mt-1">
                {a.nombre} ({a.cuenta}, {dmy(a.adquirido)})
                <DatosBien
                  assetId={a.asset_id}
                  nombre={a.nombre}
                  conRecibida={false}
                  onSaved={onSaved}
                />
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}

/** Contabilidad → IVA y libros: IVA de holding y cartera (G28). */
export function IvaHolding({ year }: { year: number }) {
  const [d, setD] = useState<DiagnosticoHolding | null>(null);
  const [l, setL] = useState<LibroBienesInversion | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(() => {
    Promise.all([ivaHoldingInforme(year), ivaHoldingLibro(year)])
      .then(([a, b]) => {
        setD(a);
        setL(b);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);
  const onSaved = (m: string) => {
    setMsg(m);
    load();
  };
  if (error)
    return (
      <p role="alert" className="mt-4 text-red-700">
        No se pudo cargar el IVA de holding: {error}
      </p>
    );
  if (!d || !l) return <p className="mt-4 text-stone-500">Cargando el IVA de holding…</p>;
  return (
    <div className="mt-8 border-t border-stone-200 pt-4 text-sm">
      {msg && (
        <p role="status" className="mb-2">
          {msg}
        </p>
      )}
      <Diagnostico d={d} />
      <Reglas key={`r-${d.reglas.length}-${d.proporcion_93}`} d={d} onSaved={onSaved} />
      <Sectores d={d} onSaved={onSaved} />
      <Libro l={l} onSaved={onSaved} />
    </div>
  );
}

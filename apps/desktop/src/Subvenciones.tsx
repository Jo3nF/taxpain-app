import { useCallback, useEffect, useState } from 'react';
import {
  type AccionSubvencion,
  errorMessage,
  euros,
  type FixedAsset,
  fixedAssets,
  formatDate,
  type PropuestaSubvencion,
  parseEuros,
  type ResumenSubvenciones,
  type Subvencion,
  subvencionBorrar,
  subvencionDeshacer,
  subvenciones,
  subvencionGuardar,
  subvencionPropuesta,
  subvencionRegistrar,
  type TipoSubvencion,
  type VistaSubvencion,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const TIPOS: Record<TipoSubvencion, string> = {
  capital: 'De capital: financia un bien (130 → 746)',
  donacion: 'Donación o legado de capital (131 → 746)',
  gastos: 'Para gastos de ejercicios futuros (132 → 747)',
  explotacion: 'A la explotación (740)',
};

const ACCIONES: Record<AccionSubvencion['accion'], string> = {
  concesion: 'Contabilizar la concesión',
  no_reintegrable: 'Pasar a no reintegrable',
  cobro: 'Registrar el cobro',
  imputacion: 'Imputar a resultados',
};

const nueva = (year: number): Subvencion => ({
  id: 0,
  concedente: '',
  concedente_nif: '',
  fecha_concesion: `${year}-01-01`,
  importe: 0,
  finalidad: '',
  tipo: 'capital',
  condiciones: '',
  reintegrable: false,
  fixed_asset_id: null,
  efecto_impositivo: true,
  tipo_impositivo: 2500,
  minimis: false,
  cuenta_deudora: '4708',
  notas: '',
});

const cents = (v: number) => (v ? (v / 100).toFixed(2).replace('.', ',') : '');

/** Ficha de alta o de cambio. */
function Ficha({
  initial,
  bienes,
  bloqueada,
  onDone,
}: {
  initial: Subvencion;
  bienes: FixedAsset[];
  bloqueada: boolean;
  onDone: (msg: string | null) => void;
}) {
  const [s, setS] = useState(initial);
  const [importe, setImporte] = useState(cents(initial.importe));
  const [tipoIs, setTipoIs] = useState(String(initial.tipo_impositivo / 100));
  const [error, setError] = useState<string | null>(null);
  const set = (p: Partial<Subvencion>) => setS({ ...s, ...p });
  const capital = s.tipo === 'capital' || s.tipo === 'donacion';
  async function guardar() {
    const m = parseEuros(importe);
    const t = Number.parseFloat(tipoIs.replace(',', '.'));
    if (!s.concedente.trim()) return setError('Falta el concedente');
    if (m === null || m <= 0) return setError('Revisa el importe');
    if (!(t >= 0 && t <= 100)) return setError('El tipo impositivo va de 0 a 100');
    try {
      await subvencionGuardar({
        ...s,
        importe: m,
        tipo_impositivo: Math.round(t * 100),
        fixed_asset_id: capital ? s.fixed_asset_id : null,
      });
      onDone(s.id ? 'Subvención guardada.' : 'Subvención anotada.');
    } catch (e) {
      setError(errorMessage(e));
    }
  }
  return (
    <div className="form-grid mt-2 rounded-md border border-stone-200 p-4">
      <label className="span-2">
        Concedente
        <input value={s.concedente} onChange={(x) => set({ concedente: x.target.value })} />
      </label>
      <label>
        NIF del concedente
        <input value={s.concedente_nif} onChange={(x) => set({ concedente_nif: x.target.value })} />
      </label>
      <label>
        Fecha de la concesión
        <input
          type="date"
          disabled={bloqueada}
          value={s.fecha_concesion}
          onChange={(x) => set({ fecha_concesion: x.target.value })}
        />
      </label>
      <label>
        Importe concedido
        <input
          className="text-right"
          inputMode="decimal"
          disabled={bloqueada}
          value={importe}
          onChange={(x) => setImporte(x.target.value)}
        />
      </label>
      <label className="span-2">
        Tipo
        <select
          disabled={bloqueada}
          value={s.tipo}
          onChange={(x) => set({ tipo: x.target.value as TipoSubvencion })}
        >
          {Object.entries(TIPOS).map(([k, v]) => (
            <option key={k} value={k}>
              {v}
            </option>
          ))}
        </select>
      </label>
      {capital && (
        <label className="span-2">
          Bien del inmovilizado que financia
          <select
            disabled={bloqueada}
            value={s.fixed_asset_id ?? ''}
            onChange={(x) =>
              set({ fixed_asset_id: x.target.value ? Number(x.target.value) : null })
            }
          >
            <option value="">Ninguno (la imputación se teclea)</option>
            {bienes.map((b) => (
              <option key={b.id} value={b.id}>
                {b.name}
              </option>
            ))}
          </select>
        </label>
      )}
      <label className="span-full">
        Finalidad
        <input value={s.finalidad} onChange={(x) => set({ finalidad: x.target.value })} />
      </label>
      <label className="span-full">
        Condiciones
        <input value={s.condiciones} onChange={(x) => set({ condiciones: x.target.value })} />
      </label>
      <label className="span-full flex gap-2">
        <input
          type="checkbox"
          disabled={bloqueada}
          checked={s.reintegrable}
          onChange={(x) => set({ reintegrable: x.target.checked })}
        />
        Quedan condiciones por cumplir: es reintegrable (deuda{' '}
        {s.tipo === 'explotacion' ? '522' : '172'} hasta que se cumplan)
      </label>
      {s.tipo !== 'explotacion' && (
        <>
          <label className="span-2 flex gap-2">
            <input
              type="checkbox"
              disabled={bloqueada}
              checked={s.efecto_impositivo}
              onChange={(x) => set({ efecto_impositivo: x.target.checked })}
            />
            Registrar el efecto impositivo (479: pasivo por diferencias temporarias imponibles)
          </label>
          {s.efecto_impositivo && (
            <label>
              Tipo del Impuesto sobre Sociedades (%)
              <input
                inputMode="decimal"
                disabled={bloqueada}
                value={tipoIs}
                onChange={(x) => setTipoIs(x.target.value)}
              />
            </label>
          )}
        </>
      )}
      <label>
        Cuenta del deudor
        <input
          disabled={bloqueada}
          value={s.cuenta_deudora}
          onChange={(x) => set({ cuenta_deudora: x.target.value })}
        />
        <span className="block text-xs text-stone-600">
          4708 si concede una Administración; 44x si es un particular
        </span>
      </label>
      <label className="flex gap-2">
        <input
          type="checkbox"
          checked={s.minimis}
          onChange={(x) => set({ minimis: x.target.checked })}
        />
        Ayuda de minimis
      </label>
      {bloqueada && (
        <p className="span-full text-stone-600">
          Ya tiene asientos: sólo se cambian los textos. Para cambiar las cifras, deshaz los
          asientos.
        </p>
      )}
      <div className="span-full form-actions">
        <button type="button" className="rounded btn-primary px-3 py-1" onClick={guardar}>
          Guardar
        </button>
        <button type="button" className="rounded border px-3 py-1" onClick={() => onDone(null)}>
          Cancelar
        </button>
        {error && (
          <p role="alert" className="text-red-700">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}

/** Prepara un asiento, lo enseña y lo graba con confirmación. */
function Asiento({
  id,
  accion,
  onDone,
}: {
  id: number;
  accion: AccionSubvencion;
  onDone: (msg: string) => void;
}) {
  const [p, setP] = useState<PropuestaSubvencion | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ver = () =>
    subvencionPropuesta(id, accion)
      .then((x) => {
        setP(x);
        setError(null);
      })
      .catch((e) => {
        setP(null);
        setError(errorMessage(e));
      });
  return (
    <div className="mt-1">
      <button type="button" className="rounded border px-2 py-0.5" onClick={ver}>
        Ver el asiento: {ACCIONES[accion.accion].toLowerCase()}
      </button>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      {p && (
        <div className="mt-1 rounded border border-stone-200 p-2">
          <p>
            {formatDate(p.asiento.date)} · {p.asiento.concept}
          </p>
          <table aria-label="Asiento propuesto" className="mt-1">
            <tbody>
              {p.asiento.lines.map((l) => (
                <tr key={`${l.account}-${l.debit}-${l.credit}`}>
                  <td className="pr-3 font-mono">{l.account}</td>
                  <td className="pr-3 text-right tabular-nums">{l.debit ? euros(l.debit) : ''}</td>
                  <td className="text-right tabular-nums">{l.credit ? euros(l.credit) : ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <ConfirmAction
            label={ACCIONES[accion.accion]}
            className="mt-1 rounded btn-primary px-2 py-0.5"
            danger={false}
            question={`Se graba este asiento en el diario (${euros(p.importe)}). ¿Seguir?`}
            confirmLabel="Contabilizar"
            onConfirm={async () => {
              await subvencionRegistrar(id, accion);
              setP(null);
              onDone('Asiento contabilizado.');
            }}
          />
        </div>
      )}
    </div>
  );
}

function Tarjeta({
  x,
  year,
  bienes,
  onChange,
}: {
  x: VistaSubvencion;
  year: number;
  bienes: FixedAsset[];
  onChange: (msg: string | null) => void;
}) {
  const s = x.subvencion;
  const [editar, setEditar] = useState(false);
  const [fecha, setFecha] = useState(`${year}-12-31`);
  const [cobro, setCobro] = useState(cents(x.pendiente_cobro));
  const [tesoreria, setTesoreria] = useState('572');
  const [imputar, setImputar] = useState(cents(x.propuesta_imputacion ?? 0));
  const yaImputada = x.asientos.some((a) => a.tipo === 'imputacion' && a.ejercicio === year);
  const puedeImputar = x.concedida && !x.en_deuda && s.tipo !== 'explotacion' && !yaImputada;
  const importeCobro = parseEuros(cobro) ?? 0;
  const importeImputar = parseEuros(imputar);
  return (
    <article aria-label={s.concedente} className="mt-3 rounded-md border border-stone-200 p-3">
      <h4 className="font-medium">
        {s.concedente} · {euros(s.importe)} · {TIPOS[s.tipo]}
      </h4>
      <p className="text-stone-600">
        Concedida el {formatDate(s.fecha_concesion)}
        {s.finalidad && <> para {s.finalidad}</>}
        {x.bien && <> · financia «{x.bien}»</>}
        {s.minimis && <> · de minimis</>}
        {s.condiciones && <> · condiciones: {s.condiciones}</>}
      </p>
      <table className="mt-1" aria-label={`Estado de ${s.concedente}`}>
        <tbody>
          <tr>
            <td className="pr-4">Cobrado</td>
            <td className="text-right tabular-nums">{euros(x.cobrado)}</td>
          </tr>
          <tr>
            <td className="pr-4">Imputado a resultados en {year}</td>
            <td className="text-right tabular-nums">{euros(x.imputado_ejercicio)}</td>
          </tr>
          <tr>
            <td className="pr-4">Pendiente de imputar (bruto)</td>
            <td className="text-right tabular-nums">{euros(x.pendiente_imputar)}</td>
          </tr>
          <tr>
            <td className="pr-4">En el patrimonio neto (neto del impuesto)</td>
            <td className="text-right tabular-nums">{euros(x.saldo_pn)}</td>
          </tr>
          <tr>
            <td className="pr-4">Pasivo por impuesto diferido (479)</td>
            <td className="text-right tabular-nums">{euros(x.pasivo_diferido)}</td>
          </tr>
        </tbody>
      </table>
      {x.avisos.length > 0 && (
        <ul className="mt-1 list-disc pl-5 text-amber-800">
          {x.avisos.map((a) => (
            <li key={a}>{a}</li>
          ))}
        </ul>
      )}
      {!x.concedida && <Asiento id={s.id} accion={{ accion: 'concesion' }} onDone={onChange} />}
      {x.en_deuda && (
        <div className="mt-2">
          <label>
            Fecha en que se cumplen las condiciones{' '}
            <input type="date" value={fecha} onChange={(e) => setFecha(e.target.value)} />
          </label>
          <Asiento id={s.id} accion={{ accion: 'no_reintegrable', fecha }} onDone={onChange} />
        </div>
      )}
      {x.concedida && x.pendiente_cobro > 0 && (
        <div className="mt-2 flex flex-wrap gap-2">
          <label>
            Fecha del cobro{' '}
            <input type="date" value={fecha} onChange={(e) => setFecha(e.target.value)} />
          </label>
          <label>
            Importe cobrado{' '}
            <input
              className="text-right"
              inputMode="decimal"
              value={cobro}
              onChange={(e) => setCobro(e.target.value)}
            />
          </label>
          <label>
            Tesorería <input value={tesoreria} onChange={(e) => setTesoreria(e.target.value)} />
          </label>
          <Asiento
            id={s.id}
            accion={{ accion: 'cobro', fecha, importe: importeCobro, tesoreria }}
            onDone={onChange}
          />
        </div>
      )}
      {puedeImputar && (
        <div className="mt-2">
          <label>
            Imputación de {year}
            {x.propuesta_imputacion != null && ' (al ritmo de la amortización del bien)'}{' '}
            <input
              className="text-right"
              inputMode="decimal"
              value={imputar}
              onChange={(e) => setImputar(e.target.value)}
            />
          </label>
          <Asiento
            id={s.id}
            accion={{ accion: 'imputacion', year, importe: importeImputar }}
            onDone={onChange}
          />
        </div>
      )}
      <div className="mt-2 flex flex-wrap gap-3">
        <button type="button" className="underline" onClick={() => setEditar(!editar)}>
          {editar ? 'Cerrar la ficha' : 'Editar la ficha'}
        </button>
        {x.asientos.length > 0 ? (
          <ConfirmAction
            label="Deshacer el último asiento"
            question="Se borra del diario el último asiento de esta subvención. ¿Seguir?"
            confirmLabel="Deshacer"
            onConfirm={async () => {
              await subvencionDeshacer(s.id);
              onChange('Asiento deshecho.');
            }}
          />
        ) : (
          <ConfirmAction
            label="Borrar"
            question={`Se borra la ficha de ${s.concedente}. ¿Seguir?`}
            confirmLabel="Borrar"
            onConfirm={async () => {
              await subvencionBorrar(s.id);
              onChange('Subvención borrada.');
            }}
          />
        )}
      </div>
      {editar && (
        <Ficha
          initial={s}
          bienes={bienes}
          bloqueada={x.asientos.length > 0}
          onDone={(m) => {
            setEditar(false);
            onChange(m);
          }}
        />
      )}
    </article>
  );
}

/** Contabilidad → Subvenciones: registro, asientos (NRV 18.ª) y efecto impositivo. */
export function Subvenciones({ year }: { year: number }) {
  const [r, setR] = useState<ResumenSubvenciones | null>(null);
  const [bienes, setBienes] = useState<FixedAsset[]>([]);
  const [alta, setAlta] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    subvenciones(year)
      .then((x) => {
        setR(x);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
    fixedAssets()
      .then(setBienes)
      .catch(() => setBienes([]));
  }, [year]);
  useEffect(load, [load]);
  const changed = (m: string | null) => {
    setMsg(m);
    load();
  };
  return (
    <section aria-label="Subvenciones" className="mt-4 text-sm">
      <h3 className="font-semibold">Subvenciones, donaciones y legados de {year}</h3>
      {error && (
        <p role="alert" className="text-red-700">
          No se pudo cargar: {error}
        </p>
      )}
      {msg && (
        <p role="status" className="text-emerald-800">
          {msg}
        </p>
      )}
      {r && (
        <>
          <table className="mt-2" aria-label="Totales de las subvenciones">
            <tbody>
              <tr>
                <td className="pr-4">Imputado a resultados en {year}</td>
                <td className="text-right tabular-nums">{euros(r.imputado_ejercicio)}</td>
              </tr>
              <tr>
                <td className="pr-4">En el patrimonio neto al cierre</td>
                <td className="text-right tabular-nums">{euros(r.saldo_pn)}</td>
              </tr>
              <tr>
                <td className="pr-4">Pasivo por impuesto diferido (479)</td>
                <td className="text-right tabular-nums">{euros(r.pasivo_diferido)}</td>
              </tr>
              <tr>
                <td className="pr-4">
                  Ayudas de minimis de {year - 2} a {year} (tope {euros(r.tope_minimis)})
                </td>
                <td className="text-right tabular-nums">{euros(r.minimis_tres_años)}</td>
              </tr>
            </tbody>
          </table>
          <ul className="mt-2 list-disc pl-5 text-stone-600">
            {r.notas.map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>
          {r.subvenciones.length === 0 && <p className="mt-2">No hay subvenciones anotadas.</p>}
          {r.subvenciones.map((x) => (
            <Tarjeta key={x.subvencion.id} x={x} year={year} bienes={bienes} onChange={changed} />
          ))}
        </>
      )}
      <div className="mt-3">
        {alta ? (
          <Ficha
            initial={nueva(year)}
            bienes={bienes}
            bloqueada={false}
            onDone={(m) => {
              setAlta(false);
              if (m) changed(m);
            }}
          />
        ) : (
          <button
            type="button"
            className="rounded btn-primary px-3 py-1"
            onClick={() => setAlta(true)}
          >
            Nueva subvención
          </button>
        )}
      </div>
    </section>
  );
}

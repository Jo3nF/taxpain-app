import { CircleAlert, CircleCheck } from 'lucide-react';
import { type ReactNode, useCallback, useEffect, useState } from 'react';
import {
  type Ajuste,
  ajustesCierre,
  ajustesCierreComprobaciones,
  ajustesCierreCrear,
  ajustesCierreDeshacer,
  ajustesCierreDeshacerReversion,
  ajustesCierrePropuesta,
  ajustesCierreRevertir,
  type Comprobacion,
  type Deteriorable,
  EXCLUSION_LABEL,
  type ExclusionDeterioro,
  MOTIVO_LABEL,
  type MotivoDeterioro,
  type NuevoAjuste,
  type Periodificable,
  type PropuestaAjuste,
  type ResumenAjustes,
} from './ajustesCierreApi';
import { errorMessage, euros, formatDate, type ProposedLine, parseEuros } from './api';
import { ConfirmAction } from './ConfirmAction';

/** Abre otra vista de Contabilidad (y, en el mayor, una cuenta). */
export type IrA = (view: string, cuenta?: string) => void;

const cell = 'px-2 py-1 text-right tabular-nums';

const centsText = (c: number) => (c ? (c / 100).toFixed(2).replace('.', ',') : '');

function scrollTo(id: string) {
  document.getElementById(id)?.scrollIntoView?.({ behavior: 'smooth', block: 'start' });
}

function Lineas({ lines, caption }: { lines: ProposedLine[]; caption?: string }) {
  return (
    <table className="mt-2 w-full">
      {caption && <caption className="text-left text-stone-600">{caption}</caption>}
      <thead className="text-left text-stone-500">
        <tr>
          <th className="font-normal">Cuenta</th>
          <th className="px-2 text-right font-normal">Debe</th>
          <th className="px-2 text-right font-normal">Haber</th>
        </tr>
      </thead>
      <tbody>
        {lines.map((l) => (
          <tr key={`${l.account}-${l.debit}-${l.credit}`} className="border-t border-stone-100">
            <td className="py-1">
              {l.account} <span className="text-stone-500">{l.name}</span>
            </td>
            <td className={cell}>{l.debit ? euros(l.debit) : ''}</td>
            <td className={cell}>{l.credit ? euros(l.credit) : ''}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Pide el asiento de un ajuste, lo enseña y lo contabiliza tras confirmar. */
function Proponer({
  year,
  nuevo,
  label = 'Ver el asiento',
  onDone,
}: {
  year: number;
  /** `null`: faltan datos (se dice en `error`). */
  nuevo: NuevoAjuste | null;
  label?: string;
  onDone: () => void;
}) {
  const [p, setP] = useState<PropuestaAjuste | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ver = () => {
    if (!nuevo) {
      setError('Revisa los datos.');
      return;
    }
    ajustesCierrePropuesta(year, nuevo)
      .then((r) => {
        setP(r);
        setError(null);
      })
      .catch((e) => {
        setP(null);
        setError(errorMessage(e));
      });
  };
  return (
    <div className="mt-2">
      <button type="button" className="rounded border border-stone-300 px-3 py-1" onClick={ver}>
        {label}
      </button>
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
      {p && (
        <div className="mt-2 rounded-md bg-stone-50 p-3">
          <p>
            {p.descripcion}: {euros(p.importe)}, asiento del {formatDate(p.fecha)}.
          </p>
          <Lineas lines={p.lines} />
          {p.reversion_fecha && (
            <Lineas
              lines={p.reversion}
              caption={`Y su reversión automática el ${formatDate(p.reversion_fecha)}:`}
            />
          )}
          {p.motivo_fiscal && <p className="mt-2 text-stone-700">{p.motivo_fiscal}</p>}
          {p.avisos.length > 0 && (
            <ul className="mt-2 space-y-1 rounded-md bg-amber-50 p-2 text-amber-950">
              {p.avisos.map((a) => (
                <li key={a}>{a}</li>
              ))}
            </ul>
          )}
          <div className="mt-2">
            <ConfirmAction
              label="Contabilizar"
              question={
                p.reversion_fecha
                  ? `¿Contabilizar el ajuste de ${euros(p.importe)} con fecha ${formatDate(p.fecha)} y su reversión el ${formatDate(p.reversion_fecha)}?`
                  : `¿Contabilizar el ajuste de ${euros(p.importe)} con fecha ${formatDate(p.fecha)}?`
              }
              confirmLabel="Sí, contabilizar"
              danger={false}
              className="btn-primary rounded-md px-3 py-1.5"
              onConfirm={async () => {
                if (!nuevo) return;
                await ajustesCierreCrear(year, nuevo);
                setP(null);
                onDone();
              }}
            />
          </div>
        </div>
      )}
    </div>
  );
}

/** Los ajustes ya contabilizados de unos tipos, con su deshacer. */
function Hechos({
  ajustes,
  closed,
  onDone,
}: {
  ajustes: Ajuste[];
  closed: boolean;
  onDone: () => void;
}) {
  if (ajustes.length === 0) return null;
  return (
    <ul className="mt-3 space-y-2">
      {ajustes.map((a) => (
        <li key={a.id} className="rounded-md border border-stone-200 p-2">
          <p className="flex flex-wrap items-center gap-2">
            <span className="pill pill-green">Contabilizado</span>
            <span className="font-medium">{a.descripcion}</span>
            <span className="tabular-nums">{euros(a.importe)}</span>
            {a.reversion_fecha && (
              <span className="text-stone-600">(revertido el {formatDate(a.reversion_fecha)})</span>
            )}
            {(a.tipo === 'deterioro' || a.tipo === 'provision') && (
              <span className={a.deducible ? 'text-stone-600' : 'text-amber-800'}>
                {a.deducible ? 'Deducible' : 'No deducible: corrección en aumento en el 200'}
              </span>
            )}
          </p>
          <Lineas lines={a.lines} />
          {!closed && (
            <ConfirmAction
              label="Deshacer"
              ariaLabel={`Deshacer: ${a.descripcion}`}
              question={
                a.reversion_fecha && (a.tipo === 'deterioro' || a.tipo === 'provision')
                  ? 'Ya está revertido: deshaz antes la reversión.'
                  : '¿Quitar el ajuste y, si la tiene, su reversión del 1 de enero?'
              }
              confirmLabel="Sí, quitarlo"
              onConfirm={async () => {
                await ajustesCierreDeshacer(a.id);
                onDone();
              }}
            />
          )}
        </li>
      ))}
    </ul>
  );
}

function Seccion({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <section id={id} aria-labelledby={`${id}-t`} className="card p-5">
      <h3 id={`${id}-t`} className="font-semibold">
        {title}
      </h3>
      {children}
    </section>
  );
}

/** Pasos del cierre en su orden, con enlace a cada uno. */
export function PasosCierre({ year, onGo }: { year: number; onGo?: IrA | undefined }) {
  const pasos: [string, string, () => void][] = [
    ['Comprobaciones de saldos', 'abajo', () => scrollTo('paso-comprobaciones')],
    ['Periodificaciones de gastos e ingresos', 'abajo', () => scrollTo('paso-periodificaciones')],
    ['Facturas pendientes de recibir o de emitir', 'abajo', () => scrollTo('paso-pendientes')],
    ['Deterioro de clientes', 'abajo', () => scrollTo('paso-deterioro')],
    ['Provisiones', 'abajo', () => scrollTo('paso-provisiones')],
    ['Amortizaciones y leasing', 'Inmovilizado', () => onGo?.('inmovilizado')],
    [
      'Intereses devengados y reclasificación al corto plazo de los préstamos',
      'Préstamos',
      () => onGo?.('prestamos'),
    ],
    ['Existencias finales', 'abajo', () => scrollTo('paso-existencias')],
    ['Saldos en moneda extranjera', 'abajo', () => scrollTo('paso-divisa')],
    ['Cartera de valores a valor razonable o deterioro', 'abajo', () => scrollTo('paso-valores')],
    ['Liquidación del IVA del último periodo', 'IVA y libros', () => onGo?.('impuestos')],
    ['Impuesto sobre Sociedades', 'abajo', () => scrollTo('paso-is')],
    [`Cerrar ${year}`, 'abajo', () => scrollTo('paso-cerrar')],
    [
      'Aplicación del resultado (con las cuentas aprobadas)',
      'abajo',
      () => scrollTo('paso-cerrar'),
    ],
  ];
  return (
    <nav aria-label="Pasos del cierre" className="card p-5">
      <h3 className="font-semibold">Pasos del cierre de {year}</h3>
      <ol className="mt-2 list-decimal space-y-1 pl-5">
        {pasos.map(([label, where, go]) => (
          <li key={label}>
            <button type="button" className="text-link underline" onClick={go}>
              {label}
            </button>{' '}
            <span className="text-stone-500">({where})</span>
          </li>
        ))}
      </ol>
    </nav>
  );
}

function Comprobaciones({ items, onGo }: { items: Comprobacion[] | null; onGo?: IrA | undefined }) {
  return (
    <Seccion id="paso-comprobaciones" title="Comprobaciones de saldos">
      <p className="mt-1 text-stone-600">
        Saldos que no deberían quedar así al cierre. No se arreglan solos: cada uno lleva a donde se
        corrige.
      </p>
      {items && (
        <ul className="mt-3 space-y-2">
          {items.map((c) => (
            <li key={c.id} className="flex items-start gap-2">
              {c.ok ? (
                <CircleCheck
                  size={16}
                  className="mt-0.5 shrink-0 text-emerald-600"
                  aria-hidden="true"
                />
              ) : (
                <CircleAlert
                  size={16}
                  className="mt-0.5 shrink-0 text-amber-600"
                  aria-hidden="true"
                />
              )}
              <span>
                <span className="font-medium">{c.label}</span>
                <span className="sr-only">{c.ok ? ' (bien)' : ' (revisar)'}</span>
                <span className="block text-stone-600">{c.detail}</span>
                {!c.ok && (
                  <>
                    <span className="block text-stone-700">
                      {c.cuentas
                        .slice(0, 6)
                        .map((s) => `${s.account} ${s.name}: ${euros(s.saldo)}`)
                        .join(' · ')}
                      {c.cuentas.length > 6 && ` y ${c.cuentas.length - 6} más`}
                    </span>
                    {onGo && (
                      <button
                        type="button"
                        className="text-link underline"
                        onClick={() => onGo(c.ir, c.cuenta ?? undefined)}
                      >
                        {c.ir === 'mayor' ? `Abrir el mayor de la ${c.cuenta}` : 'Ir a arreglarlo'}
                      </button>
                    )}
                  </>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}
    </Seccion>
  );
}

/** Un año de servicio desde `d` («AAAA-MM-DD»): hasta el día anterior un año después. */
function unAno(d: string): string {
  const [y, m, day] = d.split('-').map(Number);
  const t = new Date(Date.UTC((y ?? 2000) + 1, (m ?? 1) - 1, day ?? 1) - 86_400_000);
  return t.toISOString().slice(0, 10);
}

function FilaPeriodificable({
  year,
  p,
  onDone,
}: {
  year: number;
  p: Periodificable;
  onDone: () => void;
}) {
  const [desde, setDesde] = useState(p.desde ?? p.fecha);
  const [hasta, setHasta] = useState(p.hasta ?? unAno(p.fecha));
  const nombre = `${p.numero} de ${p.tercero}`;
  return (
    <li className="rounded-md border border-stone-200 p-2">
      <p>
        <span className="font-medium">
          {p.kind === 'recibida' ? 'Recibida' : 'Emitida'} {p.numero}
        </span>{' '}
        {p.tercero}, {formatDate(p.fecha)}: {euros(p.importe)} (
        {p.cuentas.map((c) => `${c.account} ${c.name}`).join(', ')})
      </p>
      {p.ajuste_id !== null ? (
        <p className="mt-1 text-stone-600">Periodificada en este cierre (abajo).</p>
      ) : (
        <>
          <div className="mt-2 flex flex-wrap items-end gap-3">
            <label>
              Servicio desde
              <input
                type="date"
                aria-label={`Servicio desde (${nombre})`}
                className="mt-1 block"
                value={desde}
                onChange={(e) => setDesde(e.target.value)}
              />
            </label>
            <label>
              hasta
              <input
                type="date"
                aria-label={`Servicio hasta (${nombre})`}
                className="mt-1 block"
                value={hasta}
                onChange={(e) => setHasta(e.target.value)}
              />
            </label>
          </div>
          <Proponer
            year={year}
            label={`Periodificar ${p.numero}`}
            nuevo={{
              tipo: p.kind === 'recibida' ? 'gasto_anticipado' : 'ingreso_anticipado',
              factura_id: p.factura_id,
              desde,
              hasta,
            }}
            onDone={onDone}
          />
        </>
      )}
    </li>
  );
}

function Periodificaciones({
  r,
  todas,
  setTodas,
  onDone,
}: {
  r: ResumenAjustes;
  todas: boolean;
  setTodas: (b: boolean) => void;
  onDone: () => void;
}) {
  const hechos = r.ajustes.filter(
    (a) => a.tipo === 'gasto_anticipado' || a.tipo === 'ingreso_anticipado',
  );
  return (
    <Seccion id="paso-periodificaciones" title="Periodificaciones de gastos e ingresos">
      <p className="mt-1 text-stone-600">
        Un seguro, una suscripción anual o un alquiler pagados por adelantado son en parte gasto del
        año que viene: esa parte, por días, pasa a la 480 (gastos anticipados). Lo cobrado por
        adelantado pasa a la 485 (ingresos anticipados). La factura no dice el periodo del servicio:
        indícalo. El 1 de enero ({formatDate(r.reversion)}) se revierte solo.
      </p>
      {!r.closed && (
        <>
          <label className="mt-2 flex items-center gap-2">
            <input type="checkbox" checked={todas} onChange={(e) => setTodas(e.target.checked)} />
            Mostrar todas las facturas del ejercicio (si no, sólo alquileres, seguros, publicidad,
            otros servicios y prestaciones de servicios)
          </label>
          {r.periodificables.length === 0 ? (
            <p className="mt-2 text-stone-600">No hay facturas que proponer.</p>
          ) : (
            <ul className="mt-3 space-y-2">
              {r.periodificables.map((p) => (
                <FilaPeriodificable
                  key={`${p.kind}-${p.factura_id}`}
                  year={r.year}
                  p={p}
                  onDone={onDone}
                />
              ))}
            </ul>
          )}
        </>
      )}
      <Hechos ajustes={hechos} closed={r.closed} onDone={onDone} />
    </Seccion>
  );
}

function FacturasPendientes({ r, onDone }: { r: ResumenAjustes; onDone: () => void }) {
  const [tipo, setTipo] = useState<'pendiente_recibir' | 'pendiente_emitir'>('pendiente_recibir');
  const [descripcion, setDescripcion] = useState('');
  const [importe, setImporte] = useState('');
  const [cuenta, setCuenta] = useState('629');
  const [factura, setFactura] = useState<{ kind: string; id: number } | null>(null);
  const hechos = r.ajustes.filter(
    (a) => a.tipo === 'pendiente_recibir' || a.tipo === 'pendiente_emitir',
  );
  const cents = importe.trim() ? parseEuros(importe) : null;
  return (
    <Seccion id="paso-pendientes" title="Facturas pendientes de recibir o de emitir">
      <p className="mt-1 text-stone-600">
        Un gasto de este ejercicio cuya factura llega después (la asesoría de diciembre, la luz) va
        a la 4009; un trabajo hecho y aún sin facturar, a la 4309. Por la base, sin IVA. El 1 de
        enero se revierte solo: la factura, cuando llegue o la emitas, se contabiliza como siempre y
        el gasto o el ingreso queda en {r.year}.
      </p>
      {!r.closed && (
        <>
          {r.sugerencias.length > 0 && (
            <>
              <p className="mt-3 font-medium">Facturas de después del cierre: ¿son de {r.year}?</p>
              <ul className="mt-1 space-y-1">
                {r.sugerencias.map((s) => (
                  <li key={`${s.kind}-${s.factura_id}`} className="flex flex-wrap gap-2">
                    <span>
                      {s.kind === 'recibida' ? 'Recibida' : 'Emitida'} {s.numero} {s.tercero},{' '}
                      {formatDate(s.fecha)}, base {euros(s.base)}
                    </span>
                    <button
                      type="button"
                      className="text-link underline"
                      onClick={() => {
                        setTipo(s.kind === 'recibida' ? 'pendiente_recibir' : 'pendiente_emitir');
                        setDescripcion(
                          `${s.kind === 'recibida' ? 'Factura' : 'Emitida'} ${s.numero} de ${s.tercero} (${formatDate(s.fecha)})`,
                        );
                        setImporte(centsText(s.base));
                        setCuenta(s.cuenta ?? (s.kind === 'recibida' ? '629' : '705'));
                        setFactura({ kind: s.kind, id: s.factura_id });
                      }}
                    >
                      Usar {s.numero}
                    </button>
                  </li>
                ))}
              </ul>
            </>
          )}
          <div className="form-grid mt-3">
            <label>
              Tipo
              <select
                value={tipo}
                onChange={(e) => {
                  const t = e.target.value as typeof tipo;
                  setTipo(t);
                  setCuenta(t === 'pendiente_recibir' ? '629' : '705');
                  setFactura(null);
                }}
              >
                <option value="pendiente_recibir">Gasto sin factura (4009)</option>
                <option value="pendiente_emitir">Ingreso sin facturar (4309)</option>
              </select>
            </label>
            <label>
              Descripción
              <input value={descripcion} onChange={(e) => setDescripcion(e.target.value)} />
            </label>
            <label>
              Importe (base)
              <input
                className="text-right"
                inputMode="decimal"
                value={importe}
                onChange={(e) => setImporte(e.target.value)}
              />
            </label>
            <label>
              Cuenta de {tipo === 'pendiente_recibir' ? 'gasto' : 'ingreso'}
              <input
                className="font-mono"
                value={cuenta}
                onChange={(e) => setCuenta(e.target.value.trim())}
              />
            </label>
          </div>
          <Proponer
            year={r.year}
            nuevo={
              cents === null
                ? null
                : {
                    tipo,
                    descripcion,
                    importe: cents,
                    cuenta,
                    factura_kind: factura?.kind ?? null,
                    factura_id: factura?.id ?? null,
                  }
            }
            onDone={() => {
              setDescripcion('');
              setImporte('');
              setFactura(null);
              onDone();
            }}
          />
        </>
      )}
      <Hechos ajustes={hechos} closed={r.closed} onDone={onDone} />
    </Seccion>
  );
}

function FilaDeterioro({ year, d, onDone }: { year: number; d: Deteriorable; onDone: () => void }) {
  const [motivo, setMotivo] = useState<MotivoDeterioro>('seis_meses');
  const [exclusion, setExclusion] = useState<ExclusionDeterioro>(
    d.vinculada ? 'vinculada' : 'ninguna',
  );
  const [importe, setImporte] = useState(centsText(d.importe));
  const cents = parseEuros(importe);
  return (
    <li className="rounded-md border border-stone-200 p-2">
      <p>
        <span className="font-medium">{d.numero}</span> {d.cliente}
        {d.nif && ` (${d.nif})`}: {euros(d.importe)} vencido el {formatDate(d.vence)}, {d.dias} días
        al cierre.
        {d.vinculada && <span className="text-amber-800"> Cliente vinculado.</span>}
      </p>
      <div className="form-grid mt-2">
        <label>
          Importe del deterioro ({d.numero})
          <input
            className="text-right"
            inputMode="decimal"
            value={importe}
            onChange={(e) => setImporte(e.target.value)}
          />
        </label>
        <label>
          Circunstancia (art. 13.1 LIS)
          <select value={motivo} onChange={(e) => setMotivo(e.target.value as MotivoDeterioro)}>
            {Object.entries(MOTIVO_LABEL).map(([k, v]) => (
              <option key={k} value={k}>
                {v}
              </option>
            ))}
          </select>
        </label>
        <label>
          Crédito excluido
          <select
            value={exclusion}
            onChange={(e) => setExclusion(e.target.value as ExclusionDeterioro)}
          >
            {Object.entries(EXCLUSION_LABEL).map(([k, v]) => (
              <option key={k} value={k}>
                {v}
              </option>
            ))}
          </select>
        </label>
      </div>
      <Proponer
        year={year}
        label={`Deteriorar ${d.numero}`}
        nuevo={
          cents === null
            ? null
            : {
                tipo: 'deterioro',
                factura_id: d.factura_id,
                vencimiento: d.vencimiento,
                importe: cents,
                motivo,
                exclusion,
              }
        }
        onDone={onDone}
      />
    </li>
  );
}

/** Deterioros o provisiones de años anteriores: revertirlos en este cierre. */
function Anteriores({
  r,
  tipo,
  onDone,
}: {
  r: ResumenAjustes;
  tipo: 'deterioro' | 'provision';
  onDone: () => void;
}) {
  const lista = r.anteriores.filter((a) => a.tipo === tipo);
  if (lista.length === 0) return null;
  return (
    <>
      <p className="mt-3 font-medium">De ejercicios anteriores</p>
      <ul className="mt-1 space-y-2">
        {lista.map((a) => (
          <li key={a.id} className="flex flex-wrap items-center gap-2">
            <span>
              {a.year}: {a.descripcion}, {euros(a.importe)}
              {a.reversion_year === r.year && ' (revertido en este cierre)'}
            </span>
            {!r.closed &&
              (a.reversion_year === r.year ? (
                <ConfirmAction
                  label="Deshacer la reversión"
                  ariaLabel={`Deshacer la reversión: ${a.descripcion}`}
                  question="¿Quitar el asiento de reversión?"
                  confirmLabel="Sí, quitarlo"
                  onConfirm={async () => {
                    await ajustesCierreDeshacerReversion(a.id);
                    onDone();
                  }}
                />
              ) : (
                <ConfirmAction
                  label={`Revertir en ${r.year}`}
                  ariaLabel={`Revertir en ${r.year}: ${a.descripcion}`}
                  question={
                    tipo === 'deterioro'
                      ? `¿Revertir el deterioro (490 a 794) con fecha ${formatDate(r.end)}? Hazlo si se ha cobrado o ya no hace falta; si es incobrable del todo, contabiliza la pérdida (650) a mano.`
                      : `¿Revertir la provisión por exceso (a la 795) con fecha ${formatDate(r.end)}? Si se ha pagado, aplícala a mano contra la provisión.`
                  }
                  confirmLabel="Sí, revertir"
                  danger={false}
                  className="text-link underline"
                  onConfirm={async () => {
                    await ajustesCierreRevertir(a.id, r.year);
                    onDone();
                  }}
                />
              ))}
          </li>
        ))}
      </ul>
    </>
  );
}

function DeterioroClientes({ r, onDone }: { r: ResumenAjustes; onDone: () => void }) {
  const hechos = r.ajustes.filter((a) => a.tipo === 'deterioro');
  return (
    <Seccion id="paso-deterioro" title="Deterioro de clientes">
      <p className="mt-1 text-stone-600">
        Lo que un cliente debe y no se espera cobrar se deteriora (694 a 490). Aquí salen los cobros
        que al cierre llevan más de seis meses vencidos. Fiscalmente es deducible con esos seis
        meses, en concurso, con alzamiento de bienes o con reclamación judicial (art. 13.1 LIS),
        salvo que el deudor sea público, vinculado, o el crédito esté garantizado o renovado; lo no
        deducible se suma solo al 200.
      </p>
      {r.deterioro_anterior !== 0 && (
        <p className="mt-2 text-stone-700">
          La 490 empieza el ejercicio con {euros(r.deterioro_anterior)} de deterioros anteriores.
        </p>
      )}
      {!r.closed &&
        (r.deteriorables.length === 0 ? (
          <p className="mt-2 text-stone-600">
            No hay cobros vencidos hace más de seis meses sin deterioro.
          </p>
        ) : (
          <ul className="mt-3 space-y-2">
            {r.deteriorables.map((d) => (
              <FilaDeterioro
                key={`${d.factura_id}-${d.vencimiento}`}
                year={r.year}
                d={d}
                onDone={onDone}
              />
            ))}
          </ul>
        ))}
      <Hechos ajustes={hechos} closed={r.closed} onDone={onDone} />
      <Anteriores r={r} tipo="deterioro" onDone={onDone} />
    </Seccion>
  );
}

const PROVISIONES: [string, string][] = [
  ['142', '142 Otras responsabilidades (litigios, a largo plazo)'],
  ['141', '141 Impuestos (actas, a largo plazo)'],
  ['145', '145 Actuaciones medioambientales'],
  ['5292', '5292 Otras responsabilidades a corto plazo'],
  ['5291', '5291 Impuestos a corto plazo'],
  ['4999', '4999 Otras operaciones comerciales (garantías)'],
];

function Provisiones({ r, onDone }: { r: ResumenAjustes; onDone: () => void }) {
  const [descripcion, setDescripcion] = useState('');
  const [importe, setImporte] = useState('');
  const [prov, setProv] = useState('142');
  const [cuenta, setCuenta] = useState('678');
  const [deducible, setDeducible] = useState(false);
  const cents = importe.trim() ? parseEuros(importe) : null;
  const hechos = r.ajustes.filter((a) => a.tipo === 'provision');
  return (
    <Seccion id="paso-provisiones" title="Provisiones">
      <p className="mt-1 text-stone-600">
        Un riesgo probable de importe incierto (una demanda, un acta de la inspección, garantías) se
        provisiona contra su gasto. Ojo: muchas provisiones no son deducibles (art. 14 LIS:
        obligaciones implícitas, reestructuraciones, contratos onerosos, devoluciones de ventas…);
        por defecto se tratan como no deducibles y se suman al 200 como corrección en aumento. Si la
        tuya lo es, márcala y guarda por qué.
      </p>
      {!r.closed && (
        <>
          <div className="form-grid mt-3">
            <label>
              Riesgo o litigio
              <input value={descripcion} onChange={(e) => setDescripcion(e.target.value)} />
            </label>
            <label>
              Importe estimado
              <input
                className="text-right"
                inputMode="decimal"
                value={importe}
                onChange={(e) => setImporte(e.target.value)}
              />
            </label>
            <label>
              Cuenta de la provisión
              <select value={prov} onChange={(e) => setProv(e.target.value)}>
                {PROVISIONES.map(([k, v]) => (
                  <option key={k} value={k}>
                    {v}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Cuenta de gasto (678, 631, 623, 695…)
              <input
                className="font-mono"
                value={cuenta}
                onChange={(e) => setCuenta(e.target.value.trim())}
              />
            </label>
            <label>
              <input
                type="checkbox"
                checked={deducible}
                onChange={(e) => setDeducible(e.target.checked)}
              />
              Es deducible (art. 14 LIS)
            </label>
          </div>
          <Proponer
            year={r.year}
            nuevo={
              cents === null
                ? null
                : {
                    tipo: 'provision',
                    descripcion,
                    importe: cents,
                    cuenta,
                    contrapartida: prov,
                    deducible,
                  }
            }
            onDone={() => {
              setDescripcion('');
              setImporte('');
              onDone();
            }}
          />
        </>
      )}
      <Hechos ajustes={hechos} closed={r.closed} onDone={onDone} />
      <Anteriores r={r} tipo="provision" onDone={onDone} />
    </Seccion>
  );
}

/**
 * Ajustes de cierre: pasos, comprobaciones de saldos, periodificaciones, facturas pendientes,
 * deterioro de clientes y provisiones. Cada asiento se enseña antes y se confirma.
 */
export function AjustesCierre({
  year,
  onGo,
  onChanged,
}: {
  year: number;
  onGo?: IrA | undefined;
  onChanged?: () => void;
}) {
  const [r, setR] = useState<ResumenAjustes | null>(null);
  const [checks, setChecks] = useState<Comprobacion[] | null>(null);
  const [todas, setTodas] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    ajustesCierre(year, todas)
      .then((x) => {
        setR(x);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
    ajustesCierreComprobaciones(year)
      .then(setChecks)
      .catch((e) => setError(errorMessage(e)));
  }, [year, todas]);
  useEffect(load, [load]);
  const done = () => {
    load();
    onChanged?.();
  };
  return (
    <>
      <PasosCierre year={year} onGo={onGo} />
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      <Comprobaciones items={checks} onGo={onGo} />
      {r && (
        <>
          {r.closed && (
            <p className="card p-5 text-stone-600">
              El ejercicio {year} está cerrado: sus ajustes se ven abajo, pero para cambiarlos hay
              que reabrirlo.
            </p>
          )}
          <Periodificaciones r={r} todas={todas} setTodas={setTodas} onDone={done} />
          <FacturasPendientes r={r} onDone={done} />
          <DeterioroClientes r={r} onDone={done} />
          <Provisiones r={r} onDone={done} />
          {(r.fiscal.aumentos !== 0 || r.fiscal.disminuciones !== 0) && (
            <div className="card p-5">
              <h3 className="font-semibold">Efecto en el Impuesto sobre Sociedades</h3>
              <p className="mt-1 text-stone-700">
                Correcciones que el borrador del 200 ya suma: {euros(r.fiscal.aumentos)} en aumento
                y {euros(r.fiscal.disminuciones)} en disminución.
              </p>
              <ul className="mt-2 list-disc pl-5 text-stone-600">
                {r.fiscal.detalle.map((d) => (
                  <li key={d}>{d}</li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}
    </>
  );
}

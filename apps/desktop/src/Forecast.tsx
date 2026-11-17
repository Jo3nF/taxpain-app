import { AlertTriangle } from 'lucide-react';
import { type KeyboardEvent, type PointerEvent, useEffect, useId, useState } from 'react';
import {
  type CashForecast,
  cashForecast,
  daysBetween,
  errorMessage,
  euros,
  type ForecastBucket,
  type ForecastCategory,
  type ForecastHorizon,
  formatDate,
} from './api';

const CATEGORY: Record<ForecastCategory, string> = {
  collections: 'Cobro de cliente',
  payments: 'Pago a proveedor',
  taxes: 'Impuestos',
  payroll: 'Nóminas',
  social_security: 'Seguros sociales',
  leasing: 'Leasing',
  loans: 'Préstamo',
  recurring: 'Asiento periódico',
};

const MONTHS_SHORT = [
  'ene',
  'feb',
  'mar',
  'abr',
  'may',
  'jun',
  'jul',
  'ago',
  'sep',
  'oct',
  'nov',
  'dic',
];
const MONTHS = [
  'Enero',
  'Febrero',
  'Marzo',
  'Abril',
  'Mayo',
  'Junio',
  'Julio',
  'Agosto',
  'Septiembre',
  'Octubre',
  'Noviembre',
  'Diciembre',
];

/** Colores de la serie (validados contra la superficie clara y la oscura). */
const LINE = 'var(--color-brand-600)';
const DANGER = 'var(--color-red-500)';

type Mode = 'weekly' | 'monthly';

/** "2026-10-26" → "26 oct". */
function shortDate(iso: string): string {
  const [, m, d] = iso.split('-');
  return `${Number(d)} ${MONTHS_SHORT[Number(m) - 1]}`;
}

function bucketLabel(b: ForecastBucket, mode: Mode): string {
  if (mode === 'weekly') return `Semana del ${shortDate(b.start)}`;
  const [y, m] = b.start.split('-');
  return `${MONTHS[Number(m) - 1]} de ${y}`;
}

const compact = new Intl.NumberFormat('es-ES', { notation: 'compact', maximumFractionDigits: 1 });
const tick = (cents: number) => `${compact.format(cents / 100)} €`;
const signed = (cents: number) => `${cents > 0 ? '+' : ''}${euros(cents)}`;

/** Paso «redondo» (1, 2 o 5 × 10ⁿ céntimos) para unas cuatro divisiones. */
function niceStep(range: number): number {
  const raw = Math.max(range / 4, 1);
  const mag = 10 ** Math.floor(Math.log10(raw));
  const n = raw / mag;
  return (n <= 1 ? 1 : n <= 2 ? 2 : n <= 5 ? 5 : 10) * mag;
}

/** Saldo previsto día a día (escalones), con el mínimo marcado y un periodo activo al pasar el
 * ratón o con las flechas del teclado. Los números están en la tabla de debajo. */
function BalanceChart({ h, mode, today }: { h: ForecastHorizon; mode: Mode; today: string }) {
  const id = `fc${useId().replace(/[^a-zA-Z0-9]/g, '')}`;
  const [active, setActive] = useState<number | null>(null);
  const span = Math.max(1, daysBetween(today, h.end) + 1);
  const x = (iso: string) => (Math.min(span, Math.max(0, daysBetween(today, iso))) / span) * 100;
  const values = h.points.map((p) => p.balance);
  const lo = Math.min(0, ...values);
  const hi = Math.max(0, ...values);
  const step = niceStep(hi - lo || 100_000);
  const min = Math.floor(lo / step) * step;
  const max = Math.max(Math.ceil(hi / step) * step, min + step);
  const y = (c: number) => ((max - c) / (max - min)) * 100;
  const ticks: number[] = [];
  for (let t = min; t <= max; t += step) ticks.push(t);

  // Coordenadas del SVG: 0–1000 en los dos ejes (se estira al tamaño del gráfico).
  const first = h.points[0]?.balance ?? 0;
  let line = `M0 ${y(first) * 10}`;
  for (const p of h.points.slice(1)) line += ` H${x(p.date) * 10} V${y(p.balance) * 10}`;
  line += ' H1000';
  const zero = y(0) * 10;
  const area = `${line} V${zero} H0 Z`;

  const n = h.buckets.length;
  const b = active === null ? null : h.buckets[active];
  const bandLeft = b ? x(b.start) : 0;
  const bandWidth = b ? ((daysBetween(b.start, b.end) + 1) / span) * 100 : 0;
  const center = bandLeft + bandWidth / 2;
  const negative = h.lowest < 0;
  const lx = x(h.lowest_date);
  const ly = y(h.lowest);

  function onPointerMove(e: PointerEvent<HTMLElement>) {
    const r = e.currentTarget.getBoundingClientRect();
    if (r.width <= 0) return;
    const day = Math.floor(((e.clientX - r.left) / r.width) * span);
    const i = h.buckets.findIndex(
      (k) => daysBetween(today, k.start) <= day && day <= daysBetween(today, k.end),
    );
    setActive(i >= 0 ? i : null);
  }

  function onKeyDown(e: KeyboardEvent<HTMLElement>) {
    const moves: Record<string, (a: number | null) => number | null> = {
      ArrowRight: (a) => (a === null ? 0 : Math.min(n - 1, a + 1)),
      ArrowLeft: (a) => (a === null ? 0 : Math.max(0, a - 1)),
      Home: () => 0,
      End: () => n - 1,
      Escape: () => null,
    };
    const move = moves[e.key];
    if (!move) return;
    e.preventDefault();
    setActive(move);
  }

  return (
    <div className="mt-4">
      <figure
        // biome-ignore lint/a11y/noNoninteractiveTabindex: el gráfico se recorre con las flechas
        tabIndex={0}
        aria-label={`Saldo previsto ${mode === 'weekly' ? 'por semanas' : 'por meses'}. Usa las flechas para recorrer los periodos.`}
        className="relative ml-14 h-44 rounded-sm"
        onPointerMove={onPointerMove}
        onPointerLeave={() => setActive(null)}
        onKeyDown={onKeyDown}
        onBlur={() => setActive(null)}
      >
        <div aria-hidden="true">
          {b && (
            <div
              className="absolute inset-y-0 rounded-sm bg-stone-100"
              style={{ left: `${bandLeft}%`, width: `${bandWidth}%` }}
            />
          )}
          <svg
            aria-hidden="true"
            viewBox="0 0 1000 1000"
            preserveAspectRatio="none"
            className="absolute inset-0 h-full w-full overflow-visible"
          >
            <defs>
              <clipPath id={`${id}-pos`}>
                <rect x="0" y="-10" width="1000" height={zero + 10} />
              </clipPath>
              <clipPath id={`${id}-neg`}>
                <rect x="0" y={zero} width="1000" height={1010 - zero} />
              </clipPath>
            </defs>
            {ticks.map((t) => (
              <line
                key={t}
                x1="0"
                x2="1000"
                y1={y(t) * 10}
                y2={y(t) * 10}
                stroke={t === 0 && min < 0 ? 'var(--color-stone-400)' : 'var(--color-hairline)'}
                strokeWidth="1"
                vectorEffect="non-scaling-stroke"
              />
            ))}
            <path d={area} fill={LINE} fillOpacity="0.1" clipPath={`url(#${id}-pos)`} />
            <path
              d={line}
              fill="none"
              stroke={LINE}
              strokeWidth="2"
              strokeLinejoin="round"
              strokeLinecap="round"
              vectorEffect="non-scaling-stroke"
              clipPath={`url(#${id}-pos)`}
            />
            {min < 0 && (
              <>
                <path d={area} fill={DANGER} fillOpacity="0.12" clipPath={`url(#${id}-neg)`} />
                <path
                  d={line}
                  fill="none"
                  stroke={DANGER}
                  strokeWidth="2"
                  strokeLinejoin="round"
                  strokeLinecap="round"
                  vectorEffect="non-scaling-stroke"
                  clipPath={`url(#${id}-neg)`}
                />
              </>
            )}
          </svg>
          {ticks.map((t) => (
            <span
              key={t}
              className="absolute -left-14 w-12 -translate-y-1/2 text-right text-[0.7rem] tabular-nums text-stone-500"
              style={{ top: `${y(t)}%` }}
            >
              {tick(t)}
            </span>
          ))}
          <span
            className="absolute h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full"
            style={{
              left: `${lx}%`,
              top: `${ly}%`,
              background: negative ? DANGER : LINE,
              boxShadow: '0 0 0 2px var(--color-surface)',
            }}
          />
          {/* Debajo del punto (el resto de la línea está por encima del mínimo) si cabe; si no,
              lo dice la cifra de arriba, marcada con el mismo punto. */}
          {ly <= 82 && (
            <span
              className="absolute whitespace-nowrap text-xs font-medium text-stone-700"
              style={{
                left: `${lx}%`,
                top: `${ly}%`,
                transform: `translate(${lx > 60 ? 'calc(-100% - 10px)' : '10px'}, 4px)`,
              }}
            >
              Mínimo {euros(h.lowest)}
            </span>
          )}
          {b && (
            <div
              className="pointer-events-none absolute top-1 z-10 min-w-40 rounded-lg border border-stone-200 bg-white px-3 py-2 text-xs shadow-[var(--shadow-raised)]"
              style={{
                left: `${center}%`,
                transform:
                  center < 18
                    ? 'translateX(-10%)'
                    : center > 82
                      ? 'translateX(-90%)'
                      : 'translateX(-50%)',
              }}
            >
              <p className="text-sm font-semibold tabular-nums">{euros(b.closing)}</p>
              <p className="text-stone-500">{bucketLabel(b, mode)} · saldo final</p>
              <p className="mt-1 tabular-nums">
                <strong className="font-medium">{signed(b.total_in)}</strong>{' '}
                <span className="text-stone-500">entradas</span>
              </p>
              <p className="tabular-nums">
                <strong className="font-medium">{signed(-b.total_out)}</strong>{' '}
                <span className="text-stone-500">salidas</span>
              </p>
            </div>
          )}
        </div>
      </figure>
      <div className="relative ml-14 mt-1.5 h-8" aria-hidden="true">
        {h.buckets.map((k, i) => {
          const [yy, mm] = k.start.split('-');
          const label = mode === 'weekly' ? shortDate(k.start) : MONTHS_SHORT[Number(mm) - 1];
          const year = mode === 'monthly' && (i === 0 || mm === '01') ? yy : null;
          return (
            <span
              key={k.start}
              className={`absolute text-[0.7rem] leading-tight text-stone-500 ${mode === 'weekly' && i % 2 ? 'hidden lg:block' : ''}`}
              style={{ left: `${x(k.start)}%` }}
            >
              {label}
              {year && <span className="block">{year}</span>}
            </span>
          );
        })}
      </div>
      <p className="sr-only" aria-live="polite">
        {b
          ? `${bucketLabel(b, mode)}: entradas ${euros(b.total_in)}, salidas ${euros(b.total_out)}, saldo final ${euros(b.closing)}.`
          : ''}
      </p>
    </div>
  );
}

/** Previsión de tesorería del panel: saldo previsto, mínimo, avisos y detalle. */
export function Forecast() {
  const [data, setData] = useState<CashForecast | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<Mode>('weekly');

  useEffect(() => {
    cashForecast()
      .then(setData)
      .catch((e) => setError(errorMessage(e)));
  }, []);

  if (error) {
    return (
      <p className="card mt-5 p-5 text-sm text-stone-500">
        No se ha podido calcular la previsión de tesorería: {error}
      </p>
    );
  }
  if (!data) return null;

  const h = mode === 'weekly' ? data.weekly : data.monthly;
  const items = data.items.filter((i) => i.date <= h.end);
  const negative = data.warnings.find((w) => w.kind === 'negative');
  const others = data.warnings.filter((w) => w.kind !== 'negative');
  const nt = data.not_yours;
  const toggle = (m: Mode, text: string) => (
    <button
      type="button"
      aria-pressed={mode === m}
      onClick={() => setMode(m)}
      className="rounded-md px-2.5 py-1 font-medium text-stone-600"
    >
      {text}
    </button>
  );

  return (
    <section className="card mt-5 p-5" aria-labelledby="forecast-title">
      <div className="flex flex-wrap items-start gap-3">
        <div className="min-w-0 flex-1">
          <h3 id="forecast-title" className="text-sm font-semibold">
            Previsión de tesorería
          </h3>
          <p className="mt-0.5 text-xs text-stone-500">
            Saldo previsto de bancos y caja con los cobros, pagos, impuestos y gastos fijos ya
            conocidos.
          </p>
        </div>
        <fieldset className="inline-flex gap-0.5 rounded-lg border border-stone-200 bg-stone-100 p-0.5 text-sm">
          <legend className="sr-only">Horizonte de la previsión</legend>
          {toggle('weekly', '13 semanas')}
          {toggle('monthly', '12 meses')}
        </fieldset>
      </div>

      <dl className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-3">
        <div className="rounded-lg bg-stone-50 px-3 py-2.5">
          <dt className="text-xs font-medium text-stone-500">Saldo hoy</dt>
          <dd className="mt-0.5 text-lg font-semibold">{euros(data.opening)}</dd>
          <dd className="text-xs text-stone-500">Bancos y caja (57) según la contabilidad</dd>
        </div>
        <div className="rounded-lg bg-stone-50 px-3 py-2.5">
          <dt className="flex items-center gap-1.5 text-xs font-medium text-stone-500">
            <span
              className="inline-block h-2 w-2 rounded-full"
              style={{ background: h.lowest < 0 ? DANGER : LINE }}
              aria-hidden="true"
            />
            Mínimo previsto en {mode === 'weekly' ? '13 semanas' : '12 meses'}
          </dt>
          <dd className={`mt-0.5 text-lg font-semibold ${h.lowest < 0 ? 'text-red-700' : ''}`}>
            {euros(h.lowest)}
          </dd>
          <dd className="text-xs text-stone-500">el {formatDate(h.lowest_date)}</dd>
        </div>
        <div className="rounded-lg bg-stone-50 px-3 py-2.5">
          <dt className="text-xs font-medium text-stone-500">Dinero que no es tuyo</dt>
          <dd className="mt-0.5 text-lg font-semibold">{euros(nt.total)}</dd>
          <dd className="text-xs text-stone-500">
            {nt.vat >= 0
              ? `IVA ${euros(nt.vat)} y retenciones ${euros(nt.withholdings)} por ingresar`
              : `Retenciones ${euros(nt.withholdings)} por ingresar; IVA a compensar ${euros(-nt.vat)}`}
          </dd>
        </div>
      </dl>

      {negative && (
        <p
          className={`mt-4 flex items-start gap-2 rounded-lg p-3 text-sm ${h.first_negative ? 'bg-red-50 text-red-800' : 'bg-amber-50 text-amber-900'}`}
        >
          <AlertTriangle size={18} className="mt-px shrink-0" aria-hidden="true" />
          <span>
            <strong className="font-semibold">Atención:</strong> {negative.text}
          </span>
        </p>
      )}
      {others.length > 0 && (
        <ul className="mt-3 space-y-1 rounded-lg bg-amber-50 p-3 text-sm text-amber-900">
          {others.map((w) => (
            <li key={w.text} className="flex items-start gap-2">
              <AlertTriangle size={16} className="mt-0.5 shrink-0" aria-hidden="true" />
              {w.text}
            </li>
          ))}
        </ul>
      )}

      <BalanceChart key={mode} h={h} mode={mode} today={data.today} />
      {items.length === 0 && (
        <p className="mt-2 text-sm text-stone-500">
          No hay cobros ni pagos previstos en este periodo.
        </p>
      )}

      <details className="mt-4">
        <summary className="cursor-pointer text-sm">
          {mode === 'weekly' ? 'Ver por semanas' : 'Ver por meses'}
        </summary>
        <div className="mt-2 overflow-x-auto">
          <table className="w-full text-sm">
            <caption className="sr-only">
              Saldo previsto {mode === 'weekly' ? 'por semanas' : 'por meses'}
            </caption>
            <thead>
              <tr>
                <th scope="col" className="text-left">
                  Periodo
                </th>
                <th scope="col" className="text-right">
                  Entradas
                </th>
                <th scope="col" className="text-right">
                  Salidas
                </th>
                <th scope="col" className="text-right">
                  Saldo final
                </th>
              </tr>
            </thead>
            <tbody>
              {h.buckets.map((b) => (
                <tr key={b.start}>
                  <th scope="row" className="py-1.5 text-left font-normal">
                    {bucketLabel(b, mode)}
                  </th>
                  <td className="text-right tabular-nums">{signed(b.total_in)}</td>
                  <td className="text-right tabular-nums">{signed(-b.total_out)}</td>
                  <td
                    className={`text-right font-medium tabular-nums ${b.closing < 0 ? 'text-red-700' : ''}`}
                  >
                    {euros(b.closing)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </details>

      <details className="mt-2">
        <summary className="cursor-pointer text-sm">Movimientos previstos ({items.length})</summary>
        <div className="mt-2 overflow-x-auto">
          <table className="w-full text-sm">
            <caption className="sr-only">Cobros y pagos previstos</caption>
            <thead>
              <tr>
                <th scope="col" className="text-left">
                  Fecha
                </th>
                <th scope="col" className="text-left">
                  Concepto
                </th>
                <th scope="col" className="text-right">
                  Importe
                </th>
              </tr>
            </thead>
            <tbody>
              {items.map((i) => (
                <tr key={`${i.date}-${i.category}-${i.concept}-${i.amount}`}>
                  <td className="py-1.5 align-top tabular-nums whitespace-nowrap">
                    {formatDate(i.date)}
                  </td>
                  <td className="py-1.5">
                    {i.concept}
                    {i.overdue && <span className="pill pill-amber ml-2">vencido</span>}
                    {i.estimated && <span className="pill pill-stone ml-2">estimado</span>}
                    <span className="block text-xs text-stone-500">
                      {CATEGORY[i.category]} · {i.source}
                      {i.overdue ? ` · vencía el ${formatDate(i.due_date)}` : ''}
                    </span>
                  </td>
                  <td className="py-1.5 text-right align-top tabular-nums whitespace-nowrap">
                    {signed(i.amount)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </details>

      <details className="mt-2">
        <summary className="cursor-pointer text-sm">Cómo se calcula</summary>
        <ul className="mt-2 list-disc space-y-1 pl-5 text-xs text-stone-600">
          <li>
            Parte del saldo de bancos y caja de hoy y suma, en su fecha, los cobros y pagos
            pendientes de la cartera (lo vencido cuenta hoy), los modelos a ingresar, las nóminas,
            el leasing, las cuotas de los préstamos sin registrar (cobros de los concedidos, pagos
            de los recibidos) y los asientos periódicos que pasan por el banco.
          </li>
          <li>
            «Dinero que no es tuyo»: el IVA repercutido menos el soportado y las retenciones
            practicadas que aún hay que ingresar a Hacienda.
          </li>
          {data.notes.map((n) => (
            <li key={n}>{n}</li>
          ))}
        </ul>
      </details>
    </section>
  );
}

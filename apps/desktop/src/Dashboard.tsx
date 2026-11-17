import {
  ArrowDownLeft,
  ArrowUpRight,
  CalendarClock,
  ChevronRight,
  CircleCheck,
  FileSearch,
  FolderInput,
  type LucideIcon,
  Scale,
  ShieldAlert,
  TrendingDown,
  TrendingUp,
  Wallet,
} from 'lucide-react';
import { useEffect, useState } from 'react';
import { AvisoArchivo } from './Archivo';
import {
  avisoCambioEjercicio,
  type BackupStatus,
  backupStatus,
  type CertificateInfo,
  type CertificateKind,
  type CifraPresentada,
  type Dashboard as DashboardData,
  type DeadlineView,
  dashboardGet,
  dashboardReceivables,
  daysBetween,
  dueIn,
  errorMessage,
  euros,
  expedientesAvisos,
  fiscalCalendar,
  folderReviewLast,
  formatDate,
  type Hallazgo,
  listCertificates,
  recurringList,
  type VigenciaEjercicio,
} from './api';
import { RestoreBanner } from './Backups';
import { CambioEjercicio } from './CambioEjercicio';
import { trimestrePorCerrar } from './CierreTrimestre';
import { CompaniesOverview } from './CompaniesOverview';
import { Forecast } from './Forecast';
import { cierreTrimestreResumen, puestaMarcha } from './trimestreApi';
import { BloqueUrgente } from './Urgentes';

const MONTHS = ['Ene', 'Feb', 'Mar', 'Abr', 'May', 'Jun', 'Jul', 'Ago', 'Sep', 'Oct', 'Nov', 'Dic'];

/**
 * Aviso del certificado: el más nuevo de cada titular (persona o entidad) que caduca en 60 días
 * o menos (o ya caducó). Si ya se renovó, el viejo que sigue guardado no avisa. El de persona
 * física se renueva en línea en la Sede de la FNMT durante los 60 días anteriores a caducar; el de
 * representante de persona jurídica la FNMT no lo renueva: hay que solicitar uno nuevo, y conviene
 * empezar pronto (el 303/111 del 4T y las notificaciones no esperan).
 */
export function certificateAlert(
  certs: Pick<CertificateInfo, 'valid_to' | 'holder_nif' | 'entity_nif' | 'kind'>[],
  todayIso: string,
): string | null {
  const newest = new Map<string, { to: string; kind: CertificateKind }>();
  for (const c of certs) {
    const key = `${c.entity_nif ?? ''}|${c.holder_nif ?? ''}`;
    const to = c.valid_to.slice(0, 10);
    const seen = newest.get(key);
    if (!seen || to > seen.to) newest.set(key, { to, kind: c.kind });
  }
  const soon = [...newest.values()]
    .map((c) => ({ ...c, days: daysBetween(todayIso, c.to) }))
    .filter((x) => x.days <= 60)
    .sort((a, b) => a.days - b.days)[0];
  if (!soon) return null;
  const when = `${soon.days < 0 ? 'caducó' : 'caduca'} el ${formatDate(soon.to)}`;
  return soon.kind === 'representante_pj'
    ? `El certificado de representante ${when}: no se renueva, solicita uno nuevo en la FNMT (lleva días)`
    : `Tu certificado ${when}: renuévalo en la Sede de la FNMT (sólo en los 60 días anteriores)`;
}

export type DashboardTarget =
  | 'importar'
  | 'evolucion'
  | 'recibidas'
  | 'contabilidad'
  | 'notificaciones'
  | 'facturas'
  | 'presupuestos'
  | 'modelos'
  | 'calendario'
  | 'gestoria'
  | 'archivo'
  | 'ajustes';
/** Subvista con la que se abre la sección (por ahora, el banco de Contabilidad). */
export type DashboardView = 'banco' | 'cartera';

type Tone = 'brand' | 'red' | 'stone' | 'amber';
const TONES: Record<Tone, string> = {
  brand: 'bg-brand-50 text-brand-700',
  red: 'bg-red-50 text-red-700',
  stone: 'bg-stone-100 text-stone-600',
  amber: 'bg-amber-50 text-amber-700',
};

function Figure({
  label,
  cents,
  hint,
  icon: Icon,
  tone,
}: {
  label: string;
  cents: number;
  hint?: string;
  icon: LucideIcon;
  tone: Tone;
}) {
  return (
    <div className="card flex items-start gap-3 p-4">
      <span className={`grid h-10 w-10 shrink-0 place-items-center rounded-xl ${TONES[tone]}`}>
        <Icon size={20} strokeWidth={1.8} aria-hidden="true" />
      </span>
      <div className="min-w-0">
        <p className="text-[0.8rem] font-medium text-stone-500">{label}</p>
        <p
          className={`mt-0.5 text-[1.45rem] font-semibold tracking-tight tabular-nums ${cents < 0 ? 'text-red-700' : ''}`}
        >
          {euros(cents)}
        </p>
        {hint && <p className="mt-0.5 text-xs text-stone-500">{hint}</p>}
      </div>
    </div>
  );
}

/** Barras de ingresos y gastos por mes, con la tabla de datos para lectores de pantalla. */
function MonthlyChart({ data, upTo }: { data: DashboardData; upTo: number }) {
  const months = data.months.slice(0, upTo);
  const max = Math.max(1, ...months.flatMap((m) => [m.income, m.expenses]));
  const h = (c: number) => `${Math.max(0, Math.round((c / max) * 100))}%`;
  return (
    <figure className="card mt-5 p-5">
      <div className="flex items-baseline justify-between">
        <figcaption className="text-sm font-semibold">Ingresos y gastos por mes</figcaption>
        <p className="flex gap-4 text-xs text-stone-600" aria-hidden="true">
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-2.5 w-2.5 rounded-full bg-brand-500" />
            Ingresos
          </span>
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-2.5 w-2.5 rounded-full bg-stone-300" />
            Gastos
          </span>
        </p>
      </div>
      <div className="chart mt-4 flex h-44 items-end gap-2 sm:gap-3" aria-hidden="true">
        {months.map((m) => (
          <div key={m.month} className="flex h-full flex-1 flex-col justify-end">
            <div
              className="flex h-full items-end justify-center gap-1"
              title={`${MONTHS[m.month - 1]}: ingresos ${euros(m.income)}, gastos ${euros(m.expenses)}`}
            >
              <div className="bar-income w-full max-w-5" style={{ height: h(m.income) }} />
              <div className="bar-expense w-full max-w-5" style={{ height: h(m.expenses) }} />
            </div>
            <p className="mt-2 text-center text-xs text-stone-500">{MONTHS[m.month - 1]}</p>
          </div>
        ))}
      </div>
      <table className="sr-only">
        <caption>Ingresos y gastos por mes</caption>
        <thead>
          <tr>
            <th scope="col">Mes</th>
            <th scope="col">Ingresos</th>
            <th scope="col">Gastos</th>
          </tr>
        </thead>
        <tbody>
          {months.map((m) => (
            <tr key={m.month}>
              <th scope="row">{MONTHS[m.month - 1]}</th>
              <td>{euros(m.income)}</td>
              <td>{euros(m.expenses)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </figure>
  );
}

/**
 * Sin asientos del año: en vez de ceros (que parecen «sin actividad»), lo dice, explica cómo
 * traer la contabilidad y, si hay declaraciones de la AEAT leídas, da sus sumas.
 */
function SinContabilidad({
  year,
  presentado,
  onNavigate,
}: {
  year: number;
  presentado: CifraPresentada[];
  onNavigate: (t: DashboardTarget) => void;
}) {
  return (
    <div className="mt-4 grid gap-4 lg:grid-cols-2">
      <section className="card p-5" aria-labelledby="sin-contabilidad-h">
        <h3 id="sin-contabilidad-h" className="flex items-center gap-2 text-base font-semibold">
          <FolderInput size={20} aria-hidden="true" className="text-brand-700" />
          TaxPain no tiene la contabilidad de {year}
        </h3>
        <p className="mt-2 text-sm text-stone-600">
          No hay ningún asiento de {year}, así que aquí no hay ingresos, gastos ni saldos que
          enseñar. No quiere decir que la empresa no haya tenido actividad.
        </p>
        <p className="mt-2 text-sm text-stone-600">
          En Importar puedes traer el diario, los libros de facturas o la carpeta de la empresa. Si
          empiezas ahora, registra las facturas y los asientos del año.
        </p>
        <button type="button" className="btn-primary mt-4" onClick={() => onNavigate('importar')}>
          Ir a Importar
        </button>
      </section>
      {presentado.length > 0 && (
        <section className="card p-5" aria-labelledby="presentado-h">
          <h3 id="presentado-h" className="flex items-center gap-2 text-base font-semibold">
            <FileSearch size={20} aria-hidden="true" className="text-brand-700" />
            Según lo presentado a la AEAT en {year}
          </h3>
          <p className="mt-2 text-sm text-stone-600">
            Sumas de las declaraciones presentadas hasta hoy (también las de la gestoría).
          </p>
          <dl className="mt-3 divide-y divide-stone-200 text-sm">
            {presentado.map((c) => (
              <div key={c.id} className="grid grid-cols-[1fr_auto] items-baseline gap-x-3 py-2">
                <dt>{c.titulo}</dt>
                <dd className="text-right font-semibold tabular-nums">{euros(c.valor)}</dd>
                <dd className="col-span-2 mt-0.5 text-xs text-stone-600">
                  {c.fuente} · {c.periodos}
                </dd>
              </div>
            ))}
          </dl>
          <button
            type="button"
            className="mt-4 rounded-md border border-stone-300 px-3 py-1.5 text-sm"
            onClick={() => onNavigate('evolucion')}
          >
            Ver la evolución
          </button>
        </section>
      )}
    </div>
  );
}

export function Dashboard({
  onNavigate,
  now = new Date(),
}: {
  onNavigate: (t: DashboardTarget, view?: DashboardView) => void;
  now?: Date;
}) {
  const year = now.getFullYear();
  const month = now.getMonth() + 1;
  const todayIso = `${year}-${String(month).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
  const [data, setData] = useState<DashboardData | null>(null);
  const [next, setNext] = useState<{ today: string; list: DeadlineView[] } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [backup, setBackup] = useState<BackupStatus | null>(null);
  const [overdue, setOverdue] = useState<{ n: number; amount: number } | null>(null);
  const [recurringDue, setRecurringDue] = useState(0);
  const [accepted, setAccepted] = useState<{ n: number; total: number }>({ n: 0, total: 0 });
  const [certAlert, setCertAlert] = useState<string | null>(null);
  const [revisados, setRevisados] = useState<Hallazgo[]>([]);
  const [cambio, setCambio] = useState<VigenciaEjercicio | null>(null);
  // Trimestre por cerrar (si le falta algo) y guía «Dejar la gestoría» empezada sin acabar.
  const [trimestre, setTrimestre] = useState<{ q: number; year: number; n: number } | null>(null);
  const [guia, setGuia] = useState<{ hechos: number; total: number } | null>(null);
  const [plazos, setPlazos] = useState<string[]>([]);

  useEffect(() => {
    // Recordatorio de copia: si no se puede leer, el panel se muestra igual.
    backupStatus()
      .then(setBackup)
      .catch(() => setBackup(null));
    // Facturas emitidas vencidas sin cobrar (para mandar recordatorios desde Cobros y pagos),
    // sin abonos ni lo abonado, y presupuestos aceptados por facturar (de cualquier año).
    dashboardReceivables()
      .then((r) => {
        setOverdue({ n: r.overdue_invoices, amount: r.overdue_amount });
        setAccepted({ n: r.accepted_quotes, total: r.accepted_total });
      })
      .catch(() => {
        setOverdue(null);
        setAccepted({ n: 0, total: 0 });
      });
    // Certificado que caduca en menos de 60 días (o ya caducado): renovarlo o pedir otro lleva días.
    listCertificates()
      .then((certs) => setCertAlert(certificateAlert(certs, todayIso)))
      .catch(() => setCertAlert(null));
    // Lo urgente de la última revisión de la carpeta (la guardada; si no hay, nada).
    folderReviewLast()
      .then((r) => setRevisados(r?.hallazgos ?? []))
      .catch(() => setRevisados([]));
    // En diciembre y enero: lo que hay que revisar del año nuevo (si falla, no se enseña).
    avisoCambioEjercicio()
      .then(setCambio)
      .catch(() => setCambio(null));
    const porCerrar = trimestrePorCerrar(new Date(`${todayIso}T12:00:00`));
    cierreTrimestreResumen(porCerrar.year)
      .then((rs) => {
        const r = rs.find((x) => x.quarter === porCerrar.quarter);
        setTrimestre(
          r && !r.presentado && !r.listo && r.pendientes > 0
            ? { q: r.quarter, year: porCerrar.year, n: r.pendientes }
            : null,
        );
      })
      .catch(() => setTrimestre(null));
    puestaMarcha()
      .then((g) =>
        setGuia(
          g.empezada && g.resueltos < g.pasos.length
            ? { hechos: g.resueltos, total: g.pasos.length }
            : null,
        ),
      )
      .catch(() => setGuia(null));
    // Plazos de expedientes (requerimientos, recursos, pagos) y certificados que caducan.
    expedientesAvisos()
      .then((a) =>
        setPlazos(
          a.map(
            (x) =>
              `${x.texto} · ${x.dias < 0 ? `el ${formatDate(x.fecha)}` : `${formatDate(x.fecha)} (${dueIn(x.dias)})`}`,
          ),
        ),
      )
      .catch(() => setPlazos([]));
    // Facturas periódicas que tocan (se emiten a mano, con confirmación).
    recurringList()
      .then((ts) => setRecurringDue(ts.filter((t) => t.due).length))
      .catch(() => setRecurringDue(0));
  }, [todayIso]);

  useEffect(() => {
    dashboardGet(year, month)
      .then(setData)
      .catch((e) => setError(errorMessage(e)));
    // Los plazos son un extra: sin perfil fiscal, el panel se muestra igual. Se piden también los
    // del año siguiente: a finales de diciembre, los próximos (303 del 4T, 390…) vencen en enero.
    Promise.all([fiscalCalendar(year), fiscalCalendar(year + 1).catch(() => null)])
      .then(([c, after]) => {
        const seen = new Set<string>();
        const list = [...c.deadlines, ...(after?.deadlines ?? [])]
          .filter((d) => d.due_on >= c.today)
          .filter((d) => {
            const key = `${d.model}-${d.period}-${d.fiscal_year}`;
            if (seen.has(key)) return false;
            seen.add(key);
            return true;
          })
          .sort((a, b) => a.due_on.localeCompare(b.due_on))
          .slice(0, 3);
        setNext({ today: c.today, list });
      })
      .catch(() => setNext(null));
  }, [year, month]);

  if (error) {
    return (
      <p role="alert" className="mt-6 text-sm text-red-700">
        {error}
      </p>
    );
  }
  if (!data) return null;

  const p = data.pending;
  type Task = { n: number; text: string; to: DashboardTarget; view?: DashboardView };
  const one = (n: number, singular: string, many: string) => (n === 1 ? singular : many);
  const all: Task[] = [
    {
      n: p.notices_pending,
      text: one(
        p.notices_pending,
        'notificación de la AEAT sin abrir',
        'notificaciones de la AEAT sin abrir',
      ),
      to: 'notificaciones',
    },
    {
      n: p.received_to_review,
      text: one(
        p.received_to_review,
        'factura recibida por revisar',
        'facturas recibidas por revisar',
      ),
      to: 'recibidas',
    },
    {
      n: p.bank_unreconciled,
      text: one(
        p.bank_unreconciled,
        'movimiento del banco sin conciliar',
        'movimientos del banco sin conciliar',
      ),
      to: 'contabilidad',
      view: 'banco',
    },
    {
      n: overdue?.n ?? 0,
      text: `${one(overdue?.n ?? 0, 'factura vencida sin cobrar', 'facturas vencidas sin cobrar')} (${euros(overdue?.amount ?? 0)})`,
      to: 'contabilidad',
      view: 'cartera',
    },
    {
      n: accepted.n,
      text: `${one(accepted.n, 'presupuesto aceptado por facturar', 'presupuestos aceptados por facturar')} (${euros(accepted.total)})`,
      to: 'presupuestos',
    },
    {
      n: recurringDue,
      text: one(recurringDue, 'factura periódica por emitir', 'facturas periódicas por emitir'),
      to: 'facturas',
    },
    {
      n: trimestre?.n ?? 0,
      text: `${one(trimestre?.n ?? 0, 'cosa pendiente', 'cosas pendientes')} para cerrar el ${trimestre?.q ?? ''}T de ${trimestre?.year ?? ''}`,
      to: 'calendario',
    },
    {
      n: guia ? guia.total - guia.hechos : 0,
      text: `${one(guia ? guia.total - guia.hechos : 0, 'paso', 'pasos')} por hacer de la guía «Dejar la gestoría» (${guia?.hechos ?? 0} de ${guia?.total ?? 0})`,
      to: 'gestoria',
    },
    {
      n: p.verifactu_unsent,
      text: one(
        p.verifactu_unsent,
        'registro VERI*FACTU sin enviar',
        'registros VERI*FACTU sin enviar',
      ),
      to: 'facturas',
    },
  ];
  const tasks = all.filter((t) => t.n > 0);
  // Sin copia de seguridad reciente, o las automáticas fallan: una tarea más (lleva a Ajustes →
  // Copias de seguridad).
  const backupTask =
    backup?.settings.auto && backup.last_error
      ? 'La copia de seguridad automática no se ha podido hacer'
      : backup?.remind &&
        (backup.days_since === null
          ? 'Haz tu primera copia de seguridad'
          : `Última copia de seguridad hace ${backup.days_since} días`);
  // Avisos que llevan a Ajustes: copia de seguridad y certificado a punto de caducar.
  const alerts = [backupTask, certAlert].filter((a): a is string => Boolean(a));
  // Sin asientos del año, las cifras de la contabilidad (y lo que sale de ellas) no valen.
  const sin = data.sin_contabilidad === true;

  return (
    <section className="mt-5" aria-labelledby="panel-title">
      <RestoreBanner className="mb-5" />
      <div className="hero">
        <div className="hero-main">
          <p className="hero-eyebrow">Resumen del año</p>
          <h2 id="panel-title" className="hero-title">
            Tu negocio en {year}
          </h2>
          <p className="hero-sub">
            {sin
              ? 'Cuando TaxPain tenga tu contabilidad del año, aquí verás ingresos, gastos, tesorería y lo pendiente de cobro y pago.'
              : 'Cifras de tu contabilidad (bases sin IVA). Son orientativas: revisa los modelos antes de presentarlos.'}
          </p>
        </div>
        {next?.list[0] && (
          <div className="hero-next">
            <p className="hero-eyebrow">Próximo plazo</p>
            <p className="text-lg font-semibold">
              {next.list[0].title} · {next.list[0].period}
            </p>
            <p className="text-sm">
              {formatDate(next.list[0].due_on)} ·{' '}
              <strong>{dueIn(daysBetween(next.today, next.list[0].due_on))}</strong>
            </p>
          </div>
        )}
      </div>
      <div className="mt-4 empty:hidden">
        <BloqueUrgente
          hallazgos={revisados}
          id="panel-urgente"
          compacto
          onOpen={{
            label: 'Ver la revisión de la carpeta',
            go: () => onNavigate('importar'),
          }}
        />
      </div>
      {cambio && <CambioEjercicio vigencia={cambio} />}
      <AvisoArchivo onOpen={() => onNavigate('archivo')} />
      {sin ? (
        <SinContabilidad year={year} presentado={data.presentado ?? []} onNavigate={onNavigate} />
      ) : (
        <>
          <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
            <Figure label="Ingresos" cents={data.income} icon={TrendingUp} tone="brand" />
            <Figure label="Gastos" cents={data.expenses} icon={TrendingDown} tone="stone" />
            <Figure
              label="Resultado"
              cents={data.profit}
              hint="Antes de impuestos"
              icon={Scale}
              tone={data.profit < 0 ? 'red' : 'brand'}
            />
            <Figure
              label="Tesorería"
              cents={data.cash}
              hint="Bancos y caja según la contabilidad"
              icon={Wallet}
              tone="brand"
            />
            <Figure
              label="Pendiente de cobro"
              cents={data.receivables}
              icon={ArrowDownLeft}
              tone="amber"
            />
            <Figure
              label="Pendiente de pago"
              cents={data.payables}
              icon={ArrowUpRight}
              tone="stone"
            />
          </div>
          <MonthlyChart data={data} upTo={month} />
          <Forecast />
        </>
      )}

      <div className="mt-5 grid gap-4 lg:grid-cols-2">
        <div className="card p-5">
          <h3 className="text-sm font-semibold">Impuestos del trimestre (estimación)</h3>
          {sin ? (
            <p className="mt-2 text-sm text-stone-600">
              Sin la contabilidad de {year}, TaxPain no puede estimarlos.
            </p>
          ) : data.estimates.length === 0 ? (
            <p className="mt-2 text-sm text-stone-500">
              Configura tus facturas y modelos para ver la estimación.
            </p>
          ) : (
            <ul className="mt-3 divide-y divide-stone-200 text-sm">
              {data.estimates.map((e) => (
                <li key={e.model} className="flex items-center justify-between py-2">
                  <span className="flex items-center gap-2">
                    <span className="chip">{e.model}</span>
                    {e.period?.endsWith('P')
                      ? `${e.quarter}.º pago fraccionado`
                      : `${e.quarter}.º trimestre`}
                  </span>
                  {e.note ? (
                    <span className="max-w-[60%] text-right text-xs text-stone-600">
                      Sin estimar: {e.note}
                    </span>
                  ) : (
                    <span className="font-semibold tabular-nums">
                      {e.result >= 0 ? euros(e.result) : `${euros(-e.result)} a tu favor`}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          )}
          {next && next.list.length > 0 && (
            <>
              <h3 className="mt-5 text-sm font-semibold">Próximos plazos</h3>
              <ul className="mt-2 space-y-2 text-sm">
                {next.list.map((d) => {
                  const left = daysBetween(next.today, d.due_on);
                  return (
                    <li
                      key={`${d.model}-${d.period}-${d.fiscal_year}`}
                      className="flex items-center justify-between rounded-lg bg-stone-50 px-3 py-2"
                    >
                      <span className="flex items-center gap-2">
                        <CalendarClock size={16} aria-hidden="true" className="text-stone-500" />
                        {d.title} · {d.period}
                      </span>
                      <span className={`pill ${left <= 7 ? 'pill-amber' : 'pill-stone'}`}>
                        {formatDate(d.due_on)} ({dueIn(left)})
                      </span>
                    </li>
                  );
                })}
              </ul>
            </>
          )}
        </div>
        <div className="card p-5">
          <h3 className="text-sm font-semibold">Pendiente</h3>
          {tasks.length === 0 && alerts.length === 0 && plazos.length === 0 ? (
            <p className="mt-3 flex items-center gap-2 text-sm text-stone-500">
              <CircleCheck size={18} aria-hidden="true" className="text-emerald-600" />
              Nada pendiente. Todo al día.
            </p>
          ) : (
            <ul className="mt-3 space-y-2 text-sm">
              {tasks.map((t) => (
                <li key={t.to + t.text}>
                  <button
                    type="button"
                    className="task-row group flex w-full items-center justify-between rounded-lg border border-stone-200 px-3 py-2.5 text-left"
                    onClick={() => (t.view ? onNavigate(t.to, t.view) : onNavigate(t.to))}
                  >
                    <span>
                      <span className="count">{t.n}</span> {t.text}
                    </span>
                    <ChevronRight
                      size={16}
                      aria-hidden="true"
                      className="text-stone-400 transition group-hover:translate-x-0.5"
                    />
                  </button>
                </li>
              ))}
              {plazos.map((a) => (
                <li key={a}>
                  <button
                    type="button"
                    className="task-row group flex w-full items-center justify-between rounded-lg border border-amber-200 bg-amber-50 px-3 py-2.5 text-left text-amber-950"
                    onClick={() => onNavigate('notificaciones')}
                  >
                    <span className="flex items-center gap-2">
                      <CalendarClock size={16} aria-hidden="true" />
                      {a}
                    </span>
                    <ChevronRight
                      size={16}
                      aria-hidden="true"
                      className="transition group-hover:translate-x-0.5"
                    />
                  </button>
                </li>
              ))}
              {alerts.map((a) => (
                <li key={a}>
                  <button
                    type="button"
                    className="task-row group flex w-full items-center justify-between rounded-lg border border-amber-200 bg-amber-50 px-3 py-2.5 text-left text-amber-950"
                    onClick={() => onNavigate('ajustes')}
                  >
                    <span className="flex items-center gap-2">
                      <ShieldAlert size={16} aria-hidden="true" />
                      {a}
                    </span>
                    <ChevronRight
                      size={16}
                      aria-hidden="true"
                      className="transition group-hover:translate-x-0.5"
                    />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
      <CompaniesOverview onCheck={() => onNavigate('modelos')} />
    </section>
  );
}

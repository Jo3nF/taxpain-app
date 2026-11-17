import { CalendarPlus, UserPen } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import {
  type CalendarView,
  type DeadlineView,
  daysBetween,
  type ExpedienteVista,
  errorMessage,
  expedientesList,
  exportIcal,
  fiscalCalendar,
  formatDate,
  getProfile,
  isTauri,
  type TaxpayerProfile,
  type ThresholdAlert,
  thresholdAlerts,
} from './api';
import { YearStepper } from './Controls';
import { plazosDelAño } from './Expedientes';
import { ProfileForm } from './ProfileForm';

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

function urgency(days: number): string {
  if (days < 0) return 'pill-stone opacity-70';
  if (days <= 2) return 'pill-red font-semibold';
  if (days <= 7) return 'pill-amber';
  return 'pill-stone';
}

const SHORT_MONTHS = [
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

function remaining(days: number): string {
  if (days < 0) return 'Plazo terminado';
  if (days === 0) return 'Vence hoy';
  if (days === 1) return 'Vence mañana';
  return `Faltan ${days} días`;
}

function DeadlineRow({ d, today }: { d: DeadlineView; today: string }) {
  const days = daysBetween(today, d.due_on);
  const open = daysBetween(today, d.starts_on) <= 0;
  return (
    <li className="flex items-start gap-4 py-3">
      <div className="date-tile" aria-hidden="true">
        <span className="text-lg font-semibold leading-none">{Number(d.due_on.slice(8, 10))}</span>
        <span className="text-[0.65rem] uppercase tracking-wide">
          {SHORT_MONTHS[Number(d.due_on.slice(5, 7)) - 1]}
        </span>
      </div>
      <div className="min-w-0 flex-1">
        <p className="font-medium">
          {d.title} · {d.period} {d.fiscal_year}
        </p>
        <p className="text-sm text-stone-600">{d.description}</p>
        <p className="mt-0.5 text-xs text-stone-500">
          Del {formatDate(d.starts_on)} al {formatDate(d.due_on)}
          {d.due_on !== d.nominal_end && ' (trasladado por día inhábil)'}
          {d.domiciliation_end &&
            ` · domiciliación hasta el ${formatDate(d.domiciliation_end)}${
              d.domiciliation_official ? '' : ' (estimada)'
            }`}
          {d.tentative && ' · fechas estimadas'}
        </p>
      </div>
      <p className={`pill mt-0.5 shrink-0 ${urgency(days)}`}>
        {open || days < 0 ? remaining(days) : `Abre el ${formatDate(d.starts_on)}`}
      </p>
    </li>
  );
}

/** Avisos de umbral: lo que los datos dicen que cambia en tus obligaciones. */
function Alerts({ alerts }: { alerts: ThresholdAlert[] }) {
  if (alerts.length === 0) return null;
  return (
    <section aria-labelledby="umbrales-title" className="mt-4 space-y-2">
      <h3 id="umbrales-title" className="text-sm font-semibold">
        Avisos según tus datos
      </h3>
      <ul className="space-y-2">
        {alerts.map((a) => (
          <li
            key={`${a.code}-${a.title}`}
            className={`rounded-md p-3 text-sm ${
              a.severity === 'warning'
                ? 'bg-amber-100 text-amber-900'
                : 'border border-stone-200 bg-white'
            }`}
          >
            <p className="font-medium">{a.title}</p>
            <p className="mt-0.5">{a.detail}</p>
            <p className="mt-0.5">
              <strong>Qué hacer:</strong> {a.action}
            </p>
            <p className="mt-0.5 text-xs opacity-75">{a.rule}</p>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Plazos de los expedientes abiertos (requerimientos, recursos, pagos) del año. */
function PlazosExpedientes({
  plazos,
  today,
}: {
  plazos: { key: string; titulo: string; vence: string }[];
  today: string;
}) {
  if (plazos.length === 0) return null;
  return (
    <section aria-labelledby="cal-exp-title" className="mt-4">
      <h3 id="cal-exp-title" className="text-sm font-semibold">
        Plazos de tus expedientes
      </h3>
      <ul className="mt-1 divide-y divide-stone-200 text-sm">
        {plazos.map((p) => {
          const days = daysBetween(today, p.vence);
          return (
            <li key={p.key} className="flex items-center justify-between gap-2 py-2">
              <span>{p.titulo}</span>
              <span className={`pill shrink-0 ${urgency(days)}`}>
                {formatDate(p.vence)} · {remaining(days)}
              </span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

export function FiscalCalendar() {
  const [profile, setProfile] = useState<TaxpayerProfile | null>(null);
  const [editing, setEditing] = useState(false);
  const [view, setView] = useState<CalendarView | null>(null);
  const [year, setYear] = useState(new Date().getFullYear());
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [alerts, setAlerts] = useState<ThresholdAlert[]>([]);
  const [expedientes, setExpedientes] = useState<ExpedienteVista[]>([]);

  const load = useCallback(async (y: number) => {
    setError(null);
    try {
      const p = await getProfile();
      setProfile(p);
      setEditing(p === null);
      setView(p ? await fiscalCalendar(y) : null);
      // Los avisos son un extra: si fallan, el calendario se ve igual.
      setAlerts(p ? await thresholdAlerts().catch(() => []) : []);
      setExpedientes(await expedientesList().catch(() => []));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    if (isTauri()) void load(year);
  }, [load, year]);

  if (!isTauri()) {
    return <p className="mt-4 text-sm text-stone-500">Disponible en la app de escritorio.</p>;
  }
  if (!loaded) return null;

  // Primero lo que viene; lo ya vencido, plegado al final.
  const today = view?.today ?? '';
  const upcoming = (view?.deadlines ?? []).filter((d) => d.due_on >= today);
  const past = (view?.deadlines ?? []).filter((d) => d.due_on < today);
  const groupByMonth = (ds: DeadlineView[]) => {
    const byMonth = new Map<number, DeadlineView[]>();
    for (const d of ds) {
      const m = Number(d.due_on.slice(5, 7)) - 1;
      byMonth.set(m, [...(byMonth.get(m) ?? []), d]);
    }
    return [...byMonth.entries()];
  };
  const months = (ds: DeadlineView[], t: string) =>
    groupByMonth(ds).map(([m, list]) => (
      <div key={m} className="mt-4">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-stone-500">
          {MONTHS[m]}
        </h3>
        <ul className="divide-y divide-stone-200">
          {list.map((d) => (
            <DeadlineRow key={`${d.model}-${d.fiscal_year}-${d.period}`} d={d} today={t} />
          ))}
        </ul>
      </div>
    ));

  return (
    <section aria-labelledby="cal-title" className="mt-8">
      <div className="flex items-center justify-between">
        <h2 id="cal-title" className="text-lg font-semibold">
          Calendario fiscal {year}
        </h2>
        {profile && !editing && (
          <div className="flex flex-wrap items-center gap-2 text-sm">
            <button type="button" onClick={() => setEditing(true)} className="btn-ghost">
              <UserPen size={16} aria-hidden="true" />
              Editar perfil
            </button>
            <button
              type="button"
              onClick={() => exportIcal(year).catch((e) => setError(errorMessage(e)))}
              className="btn-ghost"
            >
              <CalendarPlus size={16} aria-hidden="true" />
              Exportar a Calendario (.ics)
            </button>
            <YearStepper year={year} onChange={setYear} />
          </div>
        )}
      </div>

      {editing && (
        <div className="mt-4">
          <p className="mb-3 text-sm text-stone-600">
            Cuéntanos tu situación y calculamos qué modelos te tocan y cuándo.
          </p>
          <ProfileForm
            initial={profile}
            onSaved={() => {
              setEditing(false);
              void load(year);
            }}
          />
        </div>
      )}

      {view && !editing && (
        <>
          {!view.holidays_loaded && (
            <p role="alert" className="mt-3 rounded-md bg-amber-100 p-3 text-sm text-amber-900">
              Aún no tenemos los festivos oficiales de {year}: un plazo que caiga en festivo podría
              acabar un día después de lo indicado.
            </p>
          )}
          {view.aeat_calendar_loaded === false && (
            <p className="mt-3 rounded-md bg-stone-100 p-3 text-sm text-stone-700">
              La AEAT aún no ha publicado el calendario del contribuyente de {year} (suele salir en
              diciembre): los plazos salen de la norma de cada modelo, con su traslado por festivo,
              pero el último día para domiciliar y la campaña de Renta son estimados.
            </p>
          )}
          {view.local_holidays === 0 && (
            <p className="mt-3 rounded-md bg-stone-100 p-3 text-sm text-stone-700">
              Festivos locales de {year}: pendientes de publicar o de añadir. TaxPain no los
              inventa; cuando salgan en el boletín de tu comunidad o provincia, añádelos en el
              perfil (también trasladan los plazos).
            </p>
          )}
          <Alerts alerts={alerts} />
          <PlazosExpedientes
            plazos={plazosDelAño(expedientes, year).filter((p) => p.vence >= view.today)}
            today={view.today}
          />
          {upcoming.length === 0 && (
            <p className="mt-4 text-sm text-stone-600">No quedan vencimientos en {year}.</p>
          )}
          {months(upcoming, view.today)}
          {past.length > 0 && (
            <details className="mt-6">
              <summary className="cursor-pointer text-sm text-stone-500">
                Plazos ya terminados ({past.length})
              </summary>
              {months(past, view.today)}
            </details>
          )}
        </>
      )}

      {error && (
        <p role="alert" className="mt-3 text-sm text-red-700">
          {error}
        </p>
      )}
    </section>
  );
}

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  daysBetween,
  type Expediente,
  errorMessage,
  euros,
  expedienteDesdeNotificacion,
  formatDate,
  listCertificates,
  type Notice,
  noticeAppear,
  noticeDownload,
  noticeSavePdf,
  noticesCourtesyDays,
  noticesList,
  noticesRefresh,
  noticesSetCourtesyDays,
  openSede,
  pickFolder,
} from './api';
import { CertificateSelect } from './CertificateSelect';
import { Expedientes } from './Expedientes';

/** Trámites que la AEAT sólo ofrece en la Sede web (no tienen servicio para aplicaciones). */
const SEDE = [
  {
    label: 'Mis datos censales',
    url: 'https://sede.agenciatributaria.gob.es/Sede/ayuda/consultas-informaticas/otros-servicios-ayuda-tecnica/datos-censales.html',
  },
  {
    label: 'Deudas y pagos pendientes',
    url: 'https://sede.agenciatributaria.gob.es/Sede/deudas-apremios-embargos-subastas/pagar-aplazar-consultar.html',
  },
  {
    label: 'Certificado de estar al corriente',
    url: 'https://sede.agenciatributaria.gob.es/Sede/procedimientoini/G304.shtml',
  },
];

const META_LABELS: Record<string, string> = {
  Concepto: 'Concepto',
  DescripcionProcedimiento: 'Procedimiento',
  Modelo: 'Modelo',
  Ejercicio: 'Ejercicio',
  Periodo: 'Periodo',
  Importe: 'Importe',
  NombreOficinaRemitente: 'Oficina',
  FechaAcceso: 'Fecha de acceso',
};

/**
 * El importe de una notificación: la AEAT lo manda en céntimos («12345» son 123,45 €); si ya
 * trae decimales, tal cual.
 */
export function importeAeat(v: string): string {
  const t = v.trim();
  return /^-?\d+$/.test(t) ? euros(Number(t)) : t;
}

/** Hoy en España (la fecha que rige los plazos), `YYYY-MM-DD`. */
export function spainToday(now: Date = new Date()): string {
  return new Intl.DateTimeFormat('en-CA', { timeZone: 'Europe/Madrid' }).format(now);
}

function dayBefore(iso: string): string {
  return new Date(Date.parse(`${iso}T00:00:00Z`) - 86_400_000).toISOString().slice(0, 10);
}

/** Cuenta atrás de una notificación pendiente (Ley 39/2015 art. 43.2: diez días naturales). */
function Countdown({ deemed, today }: { deemed: string; today: string }) {
  // Por prudencia se pide abrirla como tarde la víspera del día en que se da por notificada.
  const openBy = dayBefore(deemed);
  const left = daysBetween(today, openBy);
  if (left < 0) {
    return (
      <p className="mt-1 text-xs text-stone-700">
        Por la fecha en que se puso a tu disposición, se dio por notificada el {formatDate(deemed)}:
        los plazos para contestar o recurrir ya corren. Consulta en la AEAT para confirmarlo.
      </p>
    );
  }
  const tone = left <= 2 ? 'pill-red font-semibold' : left <= 5 ? 'pill-amber' : 'pill-stone';
  return (
    <p className="mt-1 text-xs text-stone-700">
      <span className={`pill mr-2 ${tone}`}>
        {left === 0
          ? 'Último día para abrirla: hoy'
          : `Quedan ${left} ${left === 1 ? 'día' : 'días'} para abrirla`}
      </span>
      Ábrela como tarde el {formatDate(openBy)}: el {formatDate(deemed)} se dará por notificada
      aunque no la abras.
    </p>
  );
}

/** Días de cortesía: recordatorio local de los que se marcaron en la Sede. */
function CourtesyDays({ today }: { today: string }) {
  const [days, setDays] = useState<string[]>([]);
  const [day, setDay] = useState('');
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    noticesCourtesyDays()
      .then(setDays)
      .catch(() => setDays([]));
  }, []);
  const save = async (next: string[]) => {
    setError(null);
    try {
      setDays(await noticesSetCourtesyDays(next));
    } catch (e) {
      setError(errorMessage(e));
    }
  };
  const year = today.slice(0, 4);
  const thisYear = days.filter((d) => d.startsWith(year)).length;
  const upcoming = days.filter((d) => d >= today);
  return (
    <div className="mt-8 rounded-lg border border-stone-200 bg-white p-4 text-sm">
      <p className="font-medium">Días de cortesía</p>
      <p className="text-stone-600">
        En la Sede de la AEAT puedes elegir hasta 30 días al año en los que Hacienda no te pondrá
        notificaciones (por ejemplo, tus vacaciones). Se marcan allí, no en TaxPain: apúntalos aquí
        para tenerlos a la vista. Las notificaciones que ya estén a tu disposición siguen contando
        sus 10 días.
      </p>
      <p className="mt-2">
        {thisYear} de 30 días apuntados en {year}.
      </p>
      {upcoming.length > 0 && (
        <ul className="mt-2 flex flex-wrap gap-2">
          {upcoming.map((d) => (
            <li key={d} className="pill pill-stone">
              {formatDate(d)}{' '}
              <button
                type="button"
                className="text-link"
                aria-label={`Quitar el ${formatDate(d)}`}
                onClick={() => save(days.filter((x) => x !== d))}
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="mt-2 flex flex-wrap items-end gap-2">
        <label className="field-inline">
          Día de cortesía
          <input type="date" value={day} onChange={(e) => setDay(e.target.value)} />
        </label>
        <button
          type="button"
          disabled={!day}
          className="rounded border border-stone-300 px-2 py-1 disabled:opacity-50"
          onClick={async () => {
            await save([...days, day]);
            setDay('');
          }}
        >
          Apuntar
        </button>
      </div>
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
    </div>
  );
}

/** Huella del certificado elegido en el selector de la pestaña (vacío: el primero). */
async function certificate(chosen: string): Promise<string> {
  if (chosen) return chosen;
  const c = (await listCertificates())[0];
  if (!c) throw { message: 'Importa primero tu certificado en Ajustes.' };
  return c.fingerprint_sha256;
}

function Appear({
  n,
  cert,
  onDone,
  onCancel,
}: {
  n: Notice;
  cert: string;
  onDone: () => void;
  onCancel: () => void;
}) {
  const [agree, setAgree] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const firstRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    firstRef.current?.focus();
  }, []);
  return (
    <div
      role="dialog"
      aria-label="Abrir la notificación"
      className="mt-2 space-y-2 rounded-md border border-red-300 bg-red-50 p-3 text-sm"
      onKeyDown={(e) => {
        if (e.key === 'Escape' && !busy) {
          e.stopPropagation();
          onCancel();
        }
      }}
    >
      <p>
        Al abrirla, la notificación{' '}
        <strong>se da por practicada y empiezan a contar los plazos</strong> para responder o
        recurrir (art. 43 de la Ley 39/2015). Si no la abres, se entenderá notificada a los 10 días
        naturales de ponerse a tu disposición.
      </p>
      <label className="flex items-center gap-2">
        <input
          ref={firstRef}
          type="checkbox"
          checked={agree}
          onChange={(e) => setAgree(e.target.checked)}
        />
        Entiendo que empiezan los plazos
      </label>
      <div className="form-actions">
        <button
          type="button"
          disabled={!agree || busy}
          className="rounded btn-danger px-3 py-1.5 disabled:opacity-50"
          onClick={async () => {
            setBusy(true);
            setError(null);
            try {
              await noticeAppear(n.id, await certificate(cert));
              onDone();
            } catch (e) {
              setError(errorMessage(e));
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? 'Abriendo…' : 'Abrir la notificación'}
        </button>
        <button
          type="button"
          disabled={busy}
          className="rounded border border-stone-300 px-3 py-1.5"
          onClick={onCancel}
        >
          Cancelar
        </button>
      </div>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
    </div>
  );
}

function Row({
  n,
  cert,
  today,
  onChange,
  onExpediente,
}: {
  n: Notice;
  cert: string;
  today: string;
  onChange: (msg?: string) => void;
  onExpediente: (e: Expediente) => void;
}) {
  const [appearing, setAppearing] = useState(false);
  // Al cancelar, el foco vuelve al botón que abrió el panel.
  const triggerRef = useRef<HTMLButtonElement>(null);
  const wasAppearing = useRef(false);
  useEffect(() => {
    if (!appearing && wasAppearing.current) triggerRef.current?.focus();
    wasAppearing.current = appearing;
  }, [appearing]);
  const pending = n.kind === 'N' && n.state === 'P';
  const save = async (certification: boolean) => {
    try {
      const dir = await pickFolder();
      if (dir) onChange(`Guardado en ${await noticeSavePdf(n.id, certification, dir)}`);
    } catch (e) {
      onChange(errorMessage(e));
    }
  };
  return (
    <li className={`py-3 text-sm ${pending ? 'font-medium' : ''}`}>
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <span>
          {pending && (
            <span className="mr-2 rounded bg-red-100 px-1.5 text-red-800">Pendiente</span>
          )}
          {n.kind === 'N' ? 'Notificación' : 'Comunicación'} · {n.subject}
        </span>
        <span className="text-stone-500">{formatDate(n.available_at.slice(0, 10))}</span>
      </div>
      {pending && n.deemed_notified_on && <Countdown deemed={n.deemed_notified_on} today={today} />}
      {n.metadata.length > 0 && (
        <dl className="mt-1 grid grid-cols-[auto_1fr] gap-x-3 text-xs text-stone-600">
          {n.metadata
            .filter(([k, v]) => META_LABELS[k] && v)
            .map(([k, v]) => (
              <div key={k} className="contents">
                <dt>{META_LABELS[k]}</dt>
                <dd>{k === 'Importe' ? importeAeat(String(v)) : v}</dd>
              </div>
            ))}
        </dl>
      )}
      <div className="mt-1 flex gap-3">
        {pending && !appearing && (
          <button
            ref={triggerRef}
            type="button"
            className="text-red-800"
            onClick={() => setAppearing(true)}
          >
            Abrir notificación…
          </button>
        )}
        {!pending && !n.has_content && (
          <button
            type="button"
            className="text-link"
            onClick={async () => {
              try {
                await noticeDownload(n.id, await certificate(cert));
                onChange();
              } catch (e) {
                onChange(errorMessage(e));
              }
            }}
          >
            Descargar
          </button>
        )}
        <button
          type="button"
          className="text-link"
          onClick={() =>
            expedienteDesdeNotificacion(n.id)
              .then(onExpediente)
              .catch((e) => onChange(errorMessage(e)))
          }
        >
          Crear expediente…
        </button>
        {n.has_content && (
          <>
            <button type="button" className="text-link" onClick={() => save(false)}>
              Guardar PDF…
            </button>
            {n.kind === 'N' && (
              <button type="button" className="text-link" onClick={() => save(true)}>
                Guardar justificante de comparecencia…
              </button>
            )}
          </>
        )}
      </div>
      {appearing && (
        <Appear
          n={n}
          cert={cert}
          onCancel={() => setAppearing(false)}
          onDone={() => {
            setAppearing(false);
            onChange('Notificación abierta y guardada.');
          }}
        />
      )}
    </li>
  );
}

export function Notices() {
  const [list, setList] = useState<Notice[]>([]);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [cert, setCert] = useState('');
  const [borrador, setBorrador] = useState<Expediente | null>(null);
  const usado = useCallback(() => setBorrador(null), []);
  const load = useCallback(() => {
    noticesList()
      .then(setList)
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  useEffect(load, [load]);
  const pending = list.filter((n) => n.kind === 'N' && n.state === 'P').length;
  const today = spainToday();

  return (
    <section aria-labelledby="notif-h" className="mt-6">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 id="notif-h" className="text-lg font-semibold">
          Notificaciones de la AEAT
          {pending > 0 && (
            <span className="ml-2 rounded bg-red-100 px-2 text-sm text-red-800">
              {pending} pendiente{pending > 1 ? 's' : ''}
            </span>
          )}
        </h2>
        <button
          type="button"
          disabled={busy}
          className="rounded border border-stone-300 px-2 py-1 text-sm"
          onClick={async () => {
            setBusy(true);
            setMsg(null);
            try {
              const n = await noticesRefresh(await certificate(cert));
              setMsg(n ? `${n} ${n === 1 ? 'nueva' : 'nuevas'}.` : 'No hay nada nuevo.');
              load();
            } catch (e) {
              setMsg(errorMessage(e));
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? 'Consultando…' : 'Consultar en la AEAT'}
        </button>
      </div>
      <div className="mt-2">
        <CertificateSelect value={cert} onChange={setCert} />
      </div>
      <p className="mt-1 text-sm text-stone-600">
        Consultar y descargar comunicaciones no tiene efectos. Abrir una notificación pendiente sí:
        TaxPain te lo pedirá siempre antes.
      </p>
      {msg && (
        <p role="status" className="mt-2 text-sm">
          {msg}
        </p>
      )}
      <ul className="mt-3 divide-y divide-stone-200">
        {list.length === 0 && <li className="py-3 text-sm text-stone-500">Nada consultado aún.</li>}
        {list.map((n) => (
          <Row
            key={n.id}
            n={n}
            cert={cert}
            today={today}
            onChange={(m) => {
              if (m) setMsg(m);
              load();
            }}
            onExpediente={(e) => {
              setBorrador(e);
              document.getElementById('exp-h')?.scrollIntoView?.({ behavior: 'smooth' });
            }}
          />
        ))}
      </ul>
      <Expedientes today={today} borrador={borrador} onBorradorUsado={usado} />
      <CourtesyDays today={today} />
      <div className="mt-8 rounded-lg border border-stone-200 bg-white p-4 text-sm">
        <p className="font-medium">En la Sede de la AEAT</p>
        <p className="text-stone-600">
          Estos trámites sólo existen en la web de la Agencia Tributaria: se abren en tu navegador.
        </p>
        <ul className="mt-2 flex flex-wrap gap-3">
          {SEDE.map((s) => (
            <li key={s.url}>
              <button
                type="button"
                className="text-link underline"
                onClick={() => openSede(s.url).catch((e) => setMsg(errorMessage(e)))}
              >
                {s.label}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}

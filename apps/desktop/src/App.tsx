import {
  Archive,
  Bell,
  Bitcoin,
  BookOpen,
  CalendarDays,
  ChartCandlestick,
  ClipboardList,
  FileText,
  FolderInput,
  Inbox,
  Landmark,
  LayoutDashboard,
  LogOut,
  type LucideIcon,
  PiggyBank,
  ReceiptText,
  Search,
  Settings,
  ShieldCheck,
  Sparkles,
  TrendingUp,
  Users,
} from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { Accounting, type View as AccountingView } from './Accounting';
import { AcercaDe } from './AcercaDe';
import { AhorroPage } from './Ahorro';
import { Archivo } from './Archivo';
import { Assistant } from './Assistant';
import { Auditoria } from './Auditoria';
import {
  type AppInfo,
  type AuditWarning,
  appInfo,
  auditRollbackAcknowledge,
  companyDocOpen,
  errorMessage,
  fiscalYearInfo,
  hayFacturacion,
  type InvoicingSettings,
  integridadEmpresa,
  invoiceOpen,
  invoicingSettings,
  isTauri,
  receivedOpen,
  setFacturacion,
} from './api';
import { Background } from './Background';
import { Backups, RestoreBanner } from './Backups';
import { Censo } from './Censo';
import { Certificates } from './Certificates';
import { CierreTrimestre } from './CierreTrimestre';
import { ClaudeMcp } from './ClaudeMcp';
import { CommandPalette, type Jump, type PaletteCommand } from './CommandPalette';
import { CompanySwitcher } from './CompanySwitcher';
import { Contacts } from './Contacts';
import { Criptomonedas } from './Criptomonedas';
import { Dashboard } from './Dashboard';
import { EvolucionPage } from './Evolucion';
import { FiscalCalendar } from './FiscalCalendar';
import { Importar } from './Importar';
import { Inversiones } from './Inversiones';
import { Invoices } from './Invoices';
import { InvoicingSetup } from './InvoicingSetup';
import { LogoMark, SpainFlag } from './Logo';
import { Models } from './Models';
import { Notices } from './Notices';
import { ObligacionesNoFiscales } from './ObligacionesNoFiscales';
import { PeriodoCerradoGate } from './PeriodoCerradoGate';
import { Periodos } from './Periodos';
import { PuestaEnMarcha } from './PuestaEnMarcha';
import { Quotes } from './Quotes';
import { Received } from './Received';
import { Updates } from './Updates';
import { VerifactuLab } from './VerifactuLab';

type Tab =
  | 'panel'
  | 'evolucion'
  | 'ahorro'
  | 'calendario'
  | 'importar'
  | 'gestoria'
  | 'facturas'
  | 'presupuestos'
  | 'recibidas'
  | 'contactos'
  | 'contabilidad'
  | 'modelos'
  | 'cripto'
  | 'valores'
  | 'notificaciones'
  | 'archivo'
  | 'asistente'
  | 'ajustes';

type TabInfo = { id: Tab; label: string; icon: LucideIcon; description: string };

/** Secciones agrupadas como en la barra lateral. */
const GROUPS: { title: string; tabs: TabInfo[] }[] = [
  {
    title: 'General',
    tabs: [
      {
        id: 'panel',
        label: 'Panel',
        icon: LayoutDashboard,
        description: 'Cómo va tu negocio este año y qué tienes pendiente.',
      },
      {
        id: 'evolucion',
        label: 'Evolución',
        icon: TrendingUp,
        description:
          'La empresa año a año según lo presentado en la AEAT, y lo que hay que revisar: lo que falta, lo que no cuadra y quién debe corregirlo.',
      },
      {
        id: 'ahorro',
        label: 'Ahorrar impuestos',
        icon: PiggyBank,
        description:
          'Lo que puedes aprovechar según la situación de tu empresa (reservas, inversiones que desgravan, gastos, IVA, cripto…), cuánto te ahorrarías y los pasos para conseguirlo.',
      },
      {
        id: 'calendario',
        label: 'Calendario',
        icon: CalendarDays,
        description:
          'Cerrar el trimestre, los modelos que te tocan y cuándo vencen, y las otras obligaciones de la sociedad.',
      },
      {
        id: 'importar',
        label: 'Importar',
        icon: FolderInput,
        description:
          'Trae lo que tienes de la empresa y la gestoría (documentos, libros, diario, facturas y modelos), compara sus modelos con los de TaxPain y revisa la carpeta en busca de errores.',
      },
      {
        id: 'gestoria',
        label: 'Dejar la gestoría',
        icon: LogOut,
        description:
          'Los pasos para empezar a llevar tú la empresa si vienes de una gestoría: qué pedirle, qué importar, qué revocar en la Sede y cómo quedarte con el calendario.',
      },
    ],
  },
  {
    title: 'Facturación',
    tabs: [
      {
        id: 'facturas',
        label: 'Facturas',
        icon: FileText,
        description: 'Emite facturas y rectificativas con VERI*FACTU.',
      },
      {
        id: 'presupuestos',
        label: 'Presupuestos',
        icon: ClipboardList,
        description: 'Presupuestos y facturas proforma: envíalos, márcalos aceptados y factúralos.',
      },
      {
        id: 'recibidas',
        label: 'Recibidas',
        icon: Inbox,
        description: 'Las facturas de tus proveedores: revisa lo que se ha leído y confírmalo.',
      },
      {
        id: 'contactos',
        label: 'Clientes y proveedores',
        icon: Users,
        description:
          'La ficha de cada uno, lo facturado cada año, lo pendiente de cobro y pago y sus facturas.',
      },
    ],
  },
  {
    title: 'Contabilidad',
    tabs: [
      {
        id: 'contabilidad',
        label: 'Contabilidad',
        icon: BookOpen,
        description: 'Diario, mayor, banco, inmovilizado, cuentas anuales y cierre.',
      },
      {
        id: 'modelos',
        label: 'Modelos',
        icon: Landmark,
        description:
          'Borradores de los modelos de la AEAT: revísalos, guarda el fichero oficial y, en los que lo admiten, preséntalos con tu certificado.',
      },
      {
        id: 'cripto',
        label: 'Criptomonedas',
        icon: Bitcoin,
        description:
          'Compras, ventas, permutas y traspasos de criptomonedas: ganancias y pérdidas de cada año (FIFO), asientos, casillas de la Renta y modelo 721.',
      },
      {
        id: 'valores',
        label: 'Inversiones en valores',
        icon: ChartCandlestick,
        description:
          'Acciones, ETF y fondos en tus brókeres (Interactive Brokers por su API, XTB por PDF): coste medio, resultado del año, dividendos y retenciones en origen, modelo 720 y asientos propuestos.',
      },
      {
        id: 'notificaciones',
        label: 'Notificaciones',
        icon: Bell,
        description: 'Avisos de la AEAT en tu buzón electrónico.',
      },
      {
        id: 'archivo',
        label: 'Archivo y auditoría',
        icon: Archive,
        description:
          'El archivo de cada ejercicio para conservarlo, hasta cuándo guardarlo, la exportación completa, los periodos cerrados y el registro de auditoría.',
      },
    ],
  },
  {
    title: 'Ayuda',
    tabs: [
      {
        id: 'asistente',
        label: 'Asistente',
        icon: Sparkles,
        description: 'Pregunta sobre tus impuestos. Tus datos salen anonimizados.',
      },
      {
        id: 'ajustes',
        label: 'Ajustes',
        icon: Settings,
        description: 'Certificado, datos de facturación, copias de seguridad y actualizaciones.',
      },
    ],
  },
];
const TABS: TabInfo[] = GROUPS.flatMap((g) => g.tabs);
const TAB_INFO = Object.fromEntries(TABS.map((t) => [t.id, t])) as Record<Tab, TabInfo>;

/**
 * Ajustes de facturación. `null` es «sin configurar»; un fallo al leerlos no lo es (guardar el
 * formulario vacío sobrescribiría los de verdad con otro número de instalación).
 */
function useInvoicingSettings() {
  const [settings, setSettings] = useState<InvoicingSettings | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `attempt` vuelve a pedirlos
  useEffect(() => {
    setError(null);
    invoicingSettings()
      .then(setSettings)
      .catch((e) => setError(errorMessage(e)));
  }, [attempt]);
  return { settings, setSettings, error, retry: () => setAttempt((n) => n + 1) };
}

function SettingsError({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div role="alert" className="mt-6 space-y-2 rounded-md bg-red-50 p-3 text-sm text-red-900">
      <p>No se han podido leer los ajustes de facturación: {message}</p>
      <button
        type="button"
        className="rounded border border-stone-300 px-3 py-1.5"
        onClick={onRetry}
      >
        Reintentar
      </button>
    </div>
  );
}

function InvoicingTab() {
  const { settings, setSettings, error, retry } = useInvoicingSettings();
  if (error) return <SettingsError message={error} onRetry={retry} />;
  if (settings === undefined) return null;
  if (settings === null) {
    return (
      <section className="mt-6">
        <h2 className="text-lg font-semibold">Configura tus facturas</h2>
        <p className="mb-3 text-sm text-stone-600">
          Una sola vez: tus datos, el formato y dónde guardamos una copia de cada factura.
        </p>
        <InvoicingSetup initial={null} onSaved={setSettings} />
      </section>
    );
  }
  return (
    <>
      {isTauri() && <RestoreBanner />}
      <Invoices settings={settings} />
    </>
  );
}

/**
 * Criptomonedas en su propia sección: con el ejercicio en curso de la contabilidad (en uno no
 * natural, de julio a junio, puede ser el del año anterior), como en Contabilidad.
 */
function CriptoTab() {
  const [year, setYear] = useState<number | null>(null);
  useEffect(() => {
    fiscalYearInfo(null)
      .then((f) => setYear(f.year))
      .catch(() => setYear(new Date().getFullYear()));
  }, []);
  return year === null ? null : <Criptomonedas year={year} />;
}

/** Inversiones en valores, con el ejercicio en curso de la contabilidad (como Criptomonedas). */
function ValoresTab() {
  const [year, setYear] = useState<number | null>(null);
  useEffect(() => {
    fiscalYearInfo(null)
      .then((f) => setYear(f.year))
      .catch(() => setYear(new Date().getFullYear()));
  }, []);
  return year === null ? null : <Inversiones year={year} />;
}

function QuotesTab() {
  const { settings, error, retry } = useInvoicingSettings();
  if (error) return <SettingsError message={error} onRetry={retry} />;
  if (settings === undefined) return null;
  if (settings === null) {
    return (
      <p className="mt-6 text-sm text-stone-600">
        Antes, configura tus datos de facturación en Facturas: los presupuestos salen con ellos.
      </p>
    );
  }
  return <Quotes settings={settings} />;
}

/**
 * Apartados de una página larga (Ajustes), arriba y siempre a la vista: cada uno lleva a su
 * sección y se marca el que se está leyendo.
 */
function SectionNav({ label, sections }: { label: string; sections: [string, string][] }) {
  const [current, setCurrent] = useState(sections[0]?.[0]);
  const [stuck, setStuck] = useState(false);
  const bar = useRef<HTMLDivElement>(null);
  const ids = sections.map(([id]) => id).join(' ');

  useEffect(() => {
    const list = ids.split(' ');
    const onScroll = () => {
      const top = bar.current?.getBoundingClientRect().top ?? 1;
      const offset = (bar.current?.offsetHeight ?? 0) + 24;
      setStuck(window.scrollY > 0 && top <= 4);
      // Al final de la página, el último (puede no llegar arriba del todo).
      const end = document.documentElement.scrollHeight - window.innerHeight;
      if (window.scrollY > 0 && window.scrollY >= end - 2) {
        setCurrent(list[list.length - 1]);
        return;
      }
      let at = list[0];
      for (const id of list) {
        const el = document.getElementById(id);
        if (el && el.getBoundingClientRect().top <= offset + 8) at = id;
      }
      setCurrent(at);
    };
    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, [ids]);

  return (
    <div ref={bar} className="section-nav-bar" data-stuck={stuck || undefined}>
      <nav aria-label={label}>
        {sections.map(([id, name]) => (
          <button
            key={id}
            type="button"
            aria-current={current === id ? 'location' : undefined}
            onClick={() => {
              setCurrent(id);
              document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
            }}
          >
            {name}
          </button>
        ))}
      </nav>
    </div>
  );
}

function ArchivoTab() {
  return (
    <>
      <SectionNav
        label="Apartados de Archivo y auditoría"
        sections={[
          ['archivo-ejercicio', 'Archivo del ejercicio'],
          ['archivo-periodos', 'Periodos cerrados'],
          ['archivo-auditoria', 'Auditoría'],
        ]}
      />
      <div id="archivo-ejercicio" className="section-anchor">
        <Archivo />
      </div>
      <div id="archivo-periodos" className="section-anchor">
        <Periodos />
      </div>
      <div id="archivo-auditoria" className="section-anchor">
        <Auditoria />
      </div>
    </>
  );
}

function SettingsTab({ version }: { version: string | null }) {
  const { settings, setSettings, error, retry } = useInvoicingSettings();
  const desktop = isTauri();
  const sections: [string, string][] = [
    ['ajustes-certificado', 'Certificado'],
    ['ajustes-facturacion', 'Facturación'],
    ...(desktop
      ? ([
          ['ajustes-copias', 'Copias de seguridad'],
          ['ajustes-avisos', 'Avisos de plazos'],
          ['ajustes-claude', 'Claude (MCP)'],
          ['ajustes-pruebas', 'Pruebas VERI*FACTU'],
          ['ajustes-actualizaciones', 'Actualizaciones'],
        ] as [string, string][])
      : []),
    ['ajustes-acerca', 'Acerca de TaxPain'],
  ];
  return (
    <>
      <SectionNav label="Apartados de Ajustes" sections={sections} />
      <div id="ajustes-certificado" className="section-anchor">
        <Certificates />
      </div>
      <div id="ajustes-facturacion" className="section-anchor">
        {error && <SettingsError message={error} onRetry={retry} />}
        {!error && settings !== undefined && (
          <section className="mt-8" aria-labelledby="invoicing-title">
            <div className="flex items-center gap-3">
              <span className="icon-tile" aria-hidden="true">
                <ReceiptText size={18} />
              </span>
              <h2 id="invoicing-title" className="text-lg font-semibold">
                Facturación
              </h2>
            </div>
            <div className="mt-4">
              <InvoicingSetup initial={settings} onSaved={setSettings} />
            </div>
          </section>
        )}
      </div>
      {desktop && (
        <>
          <div id="ajustes-copias" className="section-anchor">
            <Backups />
          </div>
          <div id="ajustes-avisos" className="section-anchor">
            <Background />
          </div>
          <div id="ajustes-claude" className="section-anchor">
            <ClaudeMcp />
          </div>
          {hayFacturacion() && (
            <div id="ajustes-pruebas" className="section-anchor">
              <VerifactuLab />
            </div>
          )}
          <div id="ajustes-actualizaciones" className="section-anchor">
            <Updates version={version} />
          </div>
        </>
      )}
      <div id="ajustes-acerca" className="section-anchor">
        <AcercaDe />
      </div>
    </>
  );
}

export function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [tab, setTab] = useState<Tab>(() => (isTauri() ? 'panel' : 'calendario'));
  // Subvista con la que abrir la sección (p. ej. el banco desde el panel).
  const [view, setView] = useState<AccountingView | undefined>(undefined);
  // Elemento que abrir al entrar en la sección (desde la búsqueda ⌘K).
  const [focus, setFocus] = useState<Jump | null>(null);
  const [palette, setPalette] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const go = (t: Tab, v?: AccountingView) => {
    setView(v);
    setFocus(null);
    setNotice(null);
    setTab(t);
  };

  // Navegación desde las guías (cierre del trimestre, dejar la gestoría).
  const navegar = (t: string, v?: string) => go(t as Tab, v as AccountingView | undefined);

  // ⌘K / Ctrl+K: búsqueda global desde cualquier sitio.
  useEffect(() => {
    if (!isTauri()) return;
    const onKey = (e: KeyboardEvent) => {
      // Con un diálogo modal abierto (la conversación con la IA), la búsqueda quedaría debajo.
      if (document.querySelector('dialog[open]')) return;
      if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setPalette((p) => !p);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const jump = (j: Jump) => {
    setNotice(null);
    const fail = (e: unknown) => setNotice(errorMessage(e));
    if ('openInvoice' in j) invoiceOpen(j.openInvoice, false).catch(fail);
    if ('openReceived' in j) receivedOpen(j.openReceived).catch(fail);
    if ('openDocument' in j) companyDocOpen(j.openDocument).catch(fail);
    setView(undefined);
    setFocus('contact' in j || 'entry' in j || 'account' in j || 'rectificar' in j ? j : null);
    setTab(j.tab as Tab);
  };
  const commands: PaletteCommand[] = [
    ...TABS.map((t) => ({ label: t.label, hint: t.description, jump: { tab: t.id } })),
    {
      label: 'Copias de seguridad',
      hint: 'Hacer una copia o restaurar (Ajustes).',
      jump: { tab: 'ajustes' },
    },
  ];

  useEffect(() => {
    appInfo()
      .then((i) => {
        setFacturacion(i?.facturacion !== false);
        setInfo(i);
      })
      .catch(() => setInfo(null));
  }, []);

  // Comprobación de integridad de la BD de la empresa abierta (una vez por sesión y empresa).
  const [integridad, setIntegridad] = useState<string[]>([]);
  useEffect(() => {
    if (!isTauri()) return;
    integridadEmpresa()
      .then(setIntegridad)
      .catch(() => setIntegridad([]));
  }, []);

  const current = TAB_INFO[tab];
  const desktop = isTauri();

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="flex items-center gap-2.5 px-2">
          <LogoMark size={30} />
          <h1 className="sidebar-title">TaxPain</h1>
        </div>
        {desktop && (
          <div className="mt-5 px-1">
            <CompanySwitcher />
          </div>
        )}
        {desktop && (
          <button type="button" className="sidebar-search" onClick={() => setPalette(true)}>
            <Search size={15} aria-hidden="true" />
            <span className="flex-1 text-left">Buscar…</span>
            <kbd aria-label="Comando K">⌘K</kbd>
          </button>
        )}
        {desktop && (
          <nav className="mt-5 flex-1 overflow-y-auto" aria-label="Secciones">
            {GROUPS.map((g) => (
              <div key={g.title} className="mb-4">
                <p className="sidebar-group">{g.title}</p>
                {g.tabs.map((t) => {
                  const Icon = t.icon;
                  return (
                    <button
                      key={t.id}
                      type="button"
                      aria-current={tab === t.id ? 'page' : undefined}
                      onClick={() => go(t.id)}
                      className="sidebar-item"
                    >
                      <Icon size={18} strokeWidth={1.8} aria-hidden="true" />
                      <span>{t.label}</span>
                    </button>
                  );
                })}
              </div>
            ))}
          </nav>
        )}
        <div className="sidebar-foot">
          {info?.keys_secure ? (
            <p className="flex items-center gap-2">
              <ShieldCheck size={15} aria-hidden="true" className="text-emerald-600" />
              Claves protegidas en el Llavero
            </p>
          ) : (
            <p>Tu certificado no sale de tu ordenador.</p>
          )}
          <p className="sidebar-made">
            <SpainFlag />
            <span>
              Hecho en España
              <span className="block">
                {info ? `Versión ${info.version}` : 'Vista previa (fuera de la app de escritorio)'}
              </span>
            </span>
          </p>
        </div>
      </aside>
      {desktop && <PeriodoCerradoGate />}
      <CommandPalette
        open={palette}
        onClose={() => setPalette(false)}
        commands={commands}
        onJump={jump}
      />
      <main className="app-main">
        <header className="page-head">
          <h2 className="text-2xl font-semibold tracking-tight">{current.label}</h2>
          <p className="mt-1 text-sm text-stone-500">{current.description}</p>
        </header>
        {info?.verifactu_lab && (
          <p role="status" className="mt-4 rounded-md bg-amber-100 p-3 text-sm text-amber-900">
            Estás en la <strong>empresa de pruebas VERI*FACTU</strong>: las facturas de aquí son de
            prueba y van al entorno de pruebas de la AEAT. Para volver a tu empresa, cámbiala en el
            selector de la barra lateral.
          </p>
        )}
        {info && !info.keys_secure && (
          <p role="alert" className="mt-4 rounded-md bg-amber-100 p-3 text-sm text-amber-900">
            Modo desarrollo: las claves no están protegidas por el Keychain.
          </p>
        )}
        {notice && (
          <p role="alert" className="mt-4 rounded-md bg-red-50 p-3 text-sm text-red-900">
            {notice}
          </p>
        )}
        {integridad.length > 0 && (
          <div role="alert" className="mt-4 rounded-md bg-red-100 p-3 text-sm text-red-900">
            <p>
              <strong>La base de datos de esta empresa tiene daños</strong> ({integridad.length}{' '}
              {integridad.length === 1 ? 'problema' : 'problemas'}). No hagas cambios importantes:
              haz ahora una exportación completa (Archivo y auditoría) y restaura la última copia de
              seguridad en Ajustes. TaxPain no ha tocado nada.
            </p>
            <details className="mt-1 text-xs">
              <summary>Detalle</summary>
              <ul className="mt-1 list-disc pl-5">
                {integridad.map((p) => (
                  <li key={p}>{p}</li>
                ))}
              </ul>
            </details>
          </div>
        )}
        {info && (info.audit_warning || info.audit_rollback) && (
          <AuditNotice
            warning={
              info.audit_warning ??
              (info.audit_rollback
                ? { kind: 'rollback', detected_at: '', ...info.audit_rollback }
                : null)
            }
            onAcknowledged={() =>
              setInfo((i) => (i ? { ...i, audit_warning: null, audit_rollback: null } : i))
            }
          />
        )}
        <div className="page-body">
          {tab === 'panel' && desktop && <Dashboard onNavigate={go} />}
          {tab === 'evolucion' && desktop && <EvolucionPage onNavigate={go} />}
          {tab === 'ahorro' && desktop && (
            <AhorroPage onNavigate={(d) => go(d.tab, 'view' in d ? d.view : undefined)} />
          )}
          {tab === 'calendario' && (
            <>
              {desktop && <CierreTrimestre onGo={navegar} />}
              <FiscalCalendar />
              {desktop && <Censo />}
              {desktop && <ObligacionesNoFiscales />}
            </>
          )}
          {tab === 'gestoria' && desktop && <PuestaEnMarcha onGo={navegar} />}
          {tab === 'importar' && desktop && <Importar onNavigate={go} />}
          {tab === 'facturas' && desktop && <InvoicingTab />}
          {tab === 'presupuestos' && desktop && <QuotesTab />}
          {tab === 'recibidas' && desktop && (
            <Received onRectificar={(x) => jump({ tab: 'modelos', rectificar: x })} />
          )}
          {tab === 'contactos' && desktop && (
            <Contacts
              key={focus && 'contact' in focus ? focus.contact : 'lista'}
              initialKey={focus && 'contact' in focus ? focus.contact : undefined}
            />
          )}
          {tab === 'contabilidad' && desktop && (
            <Accounting
              key={focus ? JSON.stringify(focus) : (view ?? 'diario')}
              {...(view ? { initialView: view } : {})}
              initialYear={focus && 'year' in focus ? focus.year : undefined}
              initialEntry={focus && 'entry' in focus ? focus.entry : undefined}
              initialAccount={focus && 'account' in focus ? focus.account : undefined}
            />
          )}
          {tab === 'modelos' && desktop && (
            <Models
              key={focus && 'rectificar' in focus ? JSON.stringify(focus) : 'modelos'}
              rectificar={focus && 'rectificar' in focus ? focus.rectificar : undefined}
            />
          )}
          {tab === 'cripto' && desktop && <CriptoTab />}
          {tab === 'valores' && desktop && <ValoresTab />}
          {tab === 'notificaciones' && desktop && <Notices />}
          {tab === 'archivo' && desktop && <ArchivoTab />}
          {tab === 'asistente' && desktop && <Assistant />}
          {tab === 'ajustes' && <SettingsTab version={info?.version ?? null} />}
        </div>
      </main>
    </div>
  );
}

/** Aviso de auditoría de la empresa abierta, con «Entendido» para quitarlo (queda anotado). */
function AuditNotice({
  warning,
  onAcknowledged,
}: {
  warning: AuditWarning | null;
  onAcknowledged: () => void;
}) {
  const [error, setError] = useState<string | null>(null);
  if (!warning) return null;
  return (
    <div role="alert" className="mt-4 rounded-md bg-red-100 p-3 text-sm text-red-900">
      {warning.kind === 'rollback' ? (
        <p>
          Tus datos son más antiguos que la última vez que se abrió TaxPain (el registro de
          auditoría llegaba a la entrada {warning.anchored_seq} y ahora termina en la{' '}
          {warning.current_seq}). Puede que se haya restaurado una copia de seguridad antigua:
          revisa las facturas y presentaciones recientes. Queda anotado en la auditoría.
        </p>
      ) : (
        <p>
          No se ha podido comprobar que el registro de auditoría de esta empresa esté completo:
          falta
          {warning.kind === 'anchor_missing' ? ' ' : ' o no se puede leer '}
          la marca que TaxPain guarda aparte para comprobarlo. Suele pasar al copiar la carpeta de
          datos a mano o al restaurarla a medias. Tus datos siguen ahí; revisa que las facturas y
          presentaciones recientes estén como las dejaste. Queda anotado en la auditoría.
        </p>
      )}
      {error && <p className="mt-1">{error}</p>}
      <button
        type="button"
        className="mt-2 rounded border border-red-300 bg-white px-3 py-1"
        onClick={() =>
          auditRollbackAcknowledge()
            .then(onAcknowledged)
            .catch((e) => setError(errorMessage(e)))
        }
      >
        Entendido
      </button>
    </div>
  );
}

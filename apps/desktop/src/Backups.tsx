import {
  ArchiveRestore,
  CircleCheck,
  FolderSync,
  HardDriveDownload,
  KeyRound,
  ShieldCheck,
  TriangleAlert,
} from 'lucide-react';
import { type FormEvent, type ReactNode, useCallback, useEffect, useState } from 'react';
import {
  type BackupSettings,
  type BackupStatus,
  type BackupSummary,
  backupCreate,
  backupInspect,
  backupRestore,
  backupSaveSettings,
  backupSetPassphrase,
  backupStatus,
  backupVerifyPassphrase,
  type Company,
  companySwitch,
  errorMessage,
  fileSize,
  formatDate,
  isNotConfirmed,
  pickBackupFile,
  pickFolder,
  plural,
  type RestoreCheck,
  restoreAcknowledge,
  restoreCheck,
} from './api';
import { COMPANIES_CHANGED } from './CompanySwitcher';
import { ConfirmAction } from './ConfirmAction';

/** Fecha y hora de un instante RFC 3339, en la hora del Mac. */
function when(at: string): string {
  const d = new Date(at);
  if (Number.isNaN(d.getTime())) return at;
  return d.toLocaleString('es-ES', { dateStyle: 'long', timeStyle: 'short' });
}

function ago(days: number): string {
  if (days === 0) return 'hoy';
  if (days === 1) return 'ayer';
  return `hace ${days} días`;
}

function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

const INPUT = 'mt-1 block w-full rounded-md border border-stone-300 px-3 py-2';

function Card({ icon, title, children }: { icon: ReactNode; title: string; children: ReactNode }) {
  return (
    <div className="card p-5">
      <h3 className="flex items-center gap-2 font-semibold">
        <span className="text-stone-500" aria-hidden="true">
          {icon}
        </span>
        {title}
      </h3>
      <div className="mt-3 text-sm">{children}</div>
    </div>
  );
}

/**
 * Formulario para elegir (o cambiar) la contraseña de las copias. Con ella se guarda también el
 * código de recuperación de los datos (también en el alta de la empresa).
 */
export function PassphraseForm({
  min,
  changing,
  onDone,
  onCancel,
}: {
  min: number;
  changing: boolean;
  onDone: () => void;
  onCancel?: () => void;
}) {
  const [pass, setPass] = useState('');
  const [again, setAgain] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if ([...pass].length < min) {
      setError(`La contraseña ha de tener al menos ${min} caracteres.`);
      return;
    }
    if (pass !== again) {
      setError('Las dos contraseñas no coinciden.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await backupSetPassphrase(pass);
      setPass('');
      setAgain('');
      onDone();
    } catch (err) {
      // Cambiarla pide Touch ID: si se cancela, no ha cambiado nada y se puede volver a probar.
      setError(
        isNotConfirmed(err)
          ? 'No se ha cambiado la contraseña: no lo has confirmado con Touch ID ni con la contraseña del Mac.'
          : errorMessage(err),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} className="space-y-3">
      <div className="form-grid">
        <label>
          {changing ? 'Contraseña nueva' : 'Contraseña de las copias'}
          <input
            type="password"
            autoComplete="new-password"
            value={pass}
            onChange={(e) => setPass(e.target.value)}
            className={INPUT}
          />
        </label>
        <label>
          Repítela
          <input
            type="password"
            autoComplete="new-password"
            value={again}
            onChange={(e) => setAgain(e.target.value)}
            className={INPUT}
          />
        </label>
      </div>
      <p className="text-stone-600">
        Al menos {min} caracteres. Una frase de varias palabras es fácil de recordar y difícil de
        adivinar.
      </p>
      <p className="flex gap-2 rounded-md bg-amber-50 p-3 text-amber-900">
        <TriangleAlert size={16} className="mt-0.5 shrink-0" aria-hidden="true" />
        <span>
          Si la olvidas, nadie podrá abrir tus copias ni recuperar tus datos en otro Mac: ni tú ni
          nosotros. No se guarda en ningún sitio. Apúntala en tu gestor de contraseñas o en un papel
          en lugar seguro.
          {changing && ' Las copias que ya tienes se siguen abriendo con la contraseña anterior.'}
        </span>
      </p>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      <div className="form-actions">
        <button
          type="submit"
          disabled={busy}
          className="btn-primary rounded-md px-4 py-2 disabled:opacity-50"
        >
          {busy ? 'Guardando…' : 'Guardar la contraseña'}
        </button>
        {onCancel && (
          <button type="button" className="btn-ghost" onClick={onCancel}>
            Cancelar
          </button>
        )}
      </div>
    </form>
  );
}

/** Comprueba que el usuario recuerda la contraseña. */
function VerifyForm({ onClose }: { onClose: () => void }) {
  const [pass, setPass] = useState('');
  const [result, setResult] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      setResult(await backupVerifyPassphrase(pass));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} className="mt-4 space-y-3 rounded-lg border border-stone-200 p-4">
      <div className="form-grid">
        <label className="span-2">
          Escribe la contraseña de las copias
          <input
            type="password"
            autoComplete="off"
            value={pass}
            onChange={(e) => {
              setPass(e.target.value);
              setResult(null);
            }}
            className={INPUT}
          />
        </label>
      </div>
      {result !== null && (
        <p role="status" className={result ? 'text-emerald-700' : 'text-red-700'}>
          {result
            ? 'Correcta: es la contraseña de tus copias.'
            : 'No es la contraseña de tus copias.'}
        </p>
      )}
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      <div className="form-actions">
        <button
          type="submit"
          disabled={busy || !pass}
          className="btn-primary rounded-md px-4 py-2 disabled:opacity-50"
        >
          {busy ? 'Comprobando…' : 'Comprobar'}
        </button>
        <button type="button" className="btn-ghost" onClick={onClose}>
          Cerrar
        </button>
      </div>
    </form>
  );
}

function AutoSettings({ st, onSaved }: { st: BackupStatus; onSaved: () => void }) {
  const [s, setS] = useState<BackupSettings>(st.settings);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const changed = JSON.stringify(s) !== JSON.stringify(st.settings);

  async function save() {
    setBusy(true);
    setMsg(null);
    try {
      setS(await backupSaveSettings(s));
      setMsg({ ok: true, text: 'Guardado.' });
      onSaved();
    } catch (e) {
      setMsg({ ok: false, text: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }

  async function choose() {
    try {
      const f = await pickFolder();
      if (f) setS({ ...s, folder: f });
    } catch (e) {
      setMsg({ ok: false, text: errorMessage(e) });
    }
  }

  return (
    <div className="space-y-3">
      <label className="flex items-center gap-2">
        <input
          type="checkbox"
          role="switch"
          aria-checked={s.auto}
          checked={s.auto}
          onChange={(e) => setS({ ...s, auto: e.target.checked })}
        />
        Hacer copias automáticas
      </label>
      <div className="form-grid">
        <label>
          Frecuencia
          <select
            value={s.every_days}
            onChange={(e) => setS({ ...s, every_days: Number(e.target.value) })}
          >
            <option value={1}>Cada día</option>
            <option value={7}>Cada semana</option>
            <option value={30}>Cada mes</option>
          </select>
        </label>
        <label>
          Conservar
          <select value={s.keep} onChange={(e) => setS({ ...s, keep: Number(e.target.value) })}>
            {[5, 10, 20, 50].map((n) => (
              <option key={n} value={n}>
                Las últimas {n}
              </option>
            ))}
          </select>
        </label>
      </div>
      <div>
        <p className="text-[0.8125rem] font-medium text-stone-700">Carpeta</p>
        <p className="mt-1 break-all rounded-md bg-stone-50 px-3 py-2 text-stone-700">
          {s.folder ?? 'Sin elegir'}
        </p>
        <div className="mt-2 flex flex-wrap gap-2">
          <button
            type="button"
            className="rounded-md border border-stone-300 px-3"
            onClick={choose}
          >
            Elegir carpeta…
          </button>
          {st.suggested_folder && st.suggested_folder !== s.folder && (
            <button
              type="button"
              className="btn-ghost"
              onClick={() => setS({ ...s, folder: st.suggested_folder })}
            >
              Usar {st.suggested_folder}
            </button>
          )}
        </div>
      </div>
      <p className="text-stone-600">
        Se hacen mientras TaxPain está abierto. Elige una carpeta de iCloud Drive, Google Drive o
        Dropbox para que las copias queden fuera de este Mac. Sólo se borran las automáticas más
        antiguas; las que hagas a mano no se tocan.
      </p>
      {msg && (
        <p
          role={msg.ok ? 'status' : 'alert'}
          className={msg.ok ? 'text-emerald-700' : 'text-red-700'}
        >
          {msg.text}
        </p>
      )}
      <div className="form-actions">
        <button
          type="button"
          disabled={busy || !changed}
          onClick={save}
          className="btn-primary rounded-md px-4 py-2 disabled:opacity-50"
        >
          {busy ? 'Guardando…' : 'Guardar'}
        </button>
      </div>
    </div>
  );
}

function Restore() {
  const [path, setPath] = useState<string | null>(null);
  const [pass, setPass] = useState('');
  const [summary, setSummary] = useState<BackupSummary | null>(null);
  const [duplicateOk, setDuplicateOk] = useState(false);
  const [done, setDone] = useState<Company | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function run(label: string, f: () => Promise<void>) {
    setBusy(label);
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  const choose = () =>
    run('elegir', async () => {
      const p = await pickBackupFile();
      if (!p) return;
      setPath(p);
      setPass('');
      setSummary(null);
      setDone(null);
    });

  const open = (e: FormEvent) => {
    e.preventDefault();
    if (!path) return;
    void run('abrir', async () => setSummary(await backupInspect(path, pass)));
  };

  if (done) {
    return (
      <div className="space-y-3">
        <p role="status" className="flex items-start gap-2 text-emerald-800">
          <CircleCheck size={16} className="mt-0.5 shrink-0" aria-hidden="true" />
          Restaurada como empresa nueva: {done.name}. La que tenías abierta no ha cambiado.
        </p>
        <p className="text-stone-600">
          Al abrirla, comprueba en el panel que su última factura es la última que emitiste antes de
          volver a facturar, e importa de nuevo el certificado.
        </p>
        <button
          type="button"
          className="btn-primary rounded-md px-4 py-2"
          onClick={() =>
            void run('cambiar', async () => {
              await companySwitch(done.id);
              window.location.reload();
            })
          }
        >
          Abrir {done.name}
        </button>
        {error && (
          <p role="alert" className="text-red-700">
            {error}
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="space-y-3">
      <p className="text-stone-600">
        Abre una copia hecha en este Mac o en otro. Se restaura como una empresa nueva: la que
        tienes abierta no se toca. Los certificados no van en la copia: impórtalos otra vez después.
        La empresa restaurada te pedirá una contraseña de copias nueva (no hereda la de la copia).
      </p>
      <button
        type="button"
        className="rounded-md border border-stone-300 px-3"
        disabled={busy !== null}
        onClick={() => void choose()}
      >
        Elegir copia…
      </button>
      {path && !summary && (
        <form onSubmit={open} className="space-y-3 rounded-lg border border-stone-200 p-4">
          <p className="break-all text-stone-700">{baseName(path)}</p>
          <div className="form-grid">
            <label className="span-2">
              Contraseña de la copia
              <input
                type="password"
                autoComplete="off"
                value={pass}
                onChange={(e) => setPass(e.target.value)}
                className={INPUT}
              />
            </label>
          </div>
          <div className="form-actions">
            <button
              type="submit"
              disabled={busy !== null || !pass}
              className="btn-primary rounded-md px-4 py-2 disabled:opacity-50"
            >
              {busy === 'abrir' ? 'Abriendo…' : 'Abrir la copia'}
            </button>
          </div>
        </form>
      )}
      {path && summary && (
        <div className="space-y-3 rounded-lg border border-stone-200 p-4">
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
            <dt className="text-stone-500">Empresa</dt>
            <dd className="font-medium">
              {summary.company}
              {summary.nif && <span className="text-stone-500"> · {summary.nif}</span>}
            </dd>
            <dt className="text-stone-500">Copia</dt>
            <dd>
              {when(summary.created_at)} (TaxPain {summary.app_version})
            </dd>
            <dt className="text-stone-500">Contenido</dt>
            <dd>
              {plural(summary.invoices_issued, 'factura emitida', 'facturas emitidas')},{' '}
              {plural(summary.invoices_received, 'recibida', 'recibidas')},{' '}
              {plural(summary.journal_entries, 'asiento')}
            </dd>
            {summary.last_invoice_date && (
              <>
                <dt className="text-stone-500">Última factura</dt>
                <dd>{formatDate(summary.last_invoice_date)}</dd>
              </>
            )}
          </dl>
          {summary.newer_version ? (
            <p role="alert" className="text-red-700">
              La copia es de una versión más nueva de TaxPain: actualiza la app para restaurarla.
            </p>
          ) : (
            <>
              {summary.same_nif_company && (
                <div className="space-y-2 rounded-md bg-amber-50 p-3 text-amber-900">
                  <p>
                    En este Mac ya tienes «{summary.same_nif_company}» con el mismo NIF. Si la
                    restauras tendrás dos empresas que emiten en las mismas series: se repetirían
                    números y la cadena de VERI*FACTU. Úsala sólo para consultar, o deja de facturar
                    con la otra.
                  </p>
                  <label>
                    <input
                      type="checkbox"
                      checked={duplicateOk}
                      onChange={(e) => setDuplicateOk(e.target.checked)}
                    />
                    Entendido: no facturaré con las dos.
                  </label>
                </div>
              )}
              <div className="form-actions">
                <button
                  type="button"
                  disabled={busy !== null || (Boolean(summary.same_nif_company) && !duplicateOk)}
                  className="btn-primary rounded-md px-4 py-2 disabled:opacity-50"
                  onClick={() =>
                    void run('restaurar', async () => {
                      setDone(await backupRestore(path, pass));
                      // La empresa nueva sale ya en el selector, sin reiniciar la app.
                      window.dispatchEvent(new Event(COMPANIES_CHANGED));
                    })
                  }
                >
                  {busy === 'restaurar' ? 'Restaurando…' : 'Restaurar como empresa nueva'}
                </button>
                <button
                  type="button"
                  className="btn-ghost"
                  onClick={() => {
                    setPath(null);
                    setSummary(null);
                    setPass('');
                    setDuplicateOk(false);
                  }}
                >
                  Cancelar
                </button>
              </div>
            </>
          )}
        </div>
      )}
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
    </div>
  );
}

/** Copias de seguridad cifradas (Ajustes). */
export function Backups() {
  const [st, setSt] = useState<BackupStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [panel, setPanel] = useState<'verificar' | 'cambiar' | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  const load = useCallback(() => {
    backupStatus()
      .then((s) => {
        setSt(s);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(load, [load]);

  async function create() {
    if (!st) return;
    setBusy(true);
    setMsg(null);
    try {
      const r = await backupCreate(st.settings.folder ?? st.suggested_folder, st.suggested_name);
      if (r) {
        setMsg({
          ok: true,
          text: `Copia guardada y comprobada: ${baseName(r.path)} (${fileSize(r.bytes)}).`,
        });
      }
    } catch (e) {
      setMsg({ ok: false, text: errorMessage(e) });
    } finally {
      setBusy(false);
      load();
    }
  }

  return (
    <section className="mt-8" aria-labelledby="backups-title">
      <div className="flex items-start gap-3">
        <span className="icon-tile" aria-hidden="true">
          <ShieldCheck size={18} />
        </span>
        <div>
          <h2 id="backups-title" className="text-lg font-semibold">
            Copias de seguridad
          </h2>
          <p className="text-sm text-stone-600">
            Tus datos están cifrados con una clave del llavero de este Mac, que no sale de él. Una
            copia de seguridad es un fichero cifrado con una contraseña tuya: guárdalo en iCloud
            Drive, Google Drive o un disco y podrás abrirlo en otro Mac si este se pierde o se
            estropea.
          </p>
          <p className="mt-2 text-sm text-stone-600">
            Con la misma contraseña se guarda junto a tus datos un{' '}
            <strong>código de recuperación</strong>: si pasas la carpeta de TaxPain a otro Mac (Time
            Machine, Asistente de Migración) y el llavero no tiene la clave, TaxPain te pedirá esta
            contraseña y los abrirá.
          </p>
        </div>
      </div>
      {error && (
        <p role="alert" className="mt-4 text-sm text-red-700">
          No se pudo leer el estado de las copias: {error}
        </p>
      )}
      {st && !st.has_passphrase && (
        <div className="mt-4">
          <Card icon={<KeyRound size={16} />} title="Primero, elige la contraseña de las copias">
            <p className="mb-3 flex gap-2 text-amber-900">
              <TriangleAlert size={16} className="mt-0.5 shrink-0" aria-hidden="true" />
              <span>
                Esta empresa todavía no tiene código de recuperación: si el llavero de este Mac
                pierde la clave (cambio de Mac, cuenta de usuario nueva), no habría forma de abrir
                sus datos. Elige la contraseña para crearlo.
              </span>
            </p>
            <PassphraseForm min={st.min_passphrase} changing={false} onDone={load} />
          </Card>
        </div>
      )}
      {st?.has_passphrase && (
        <div className="mt-4 grid gap-4 xl:grid-cols-2">
          <Card icon={<HardDriveDownload size={16} />} title="Copia de esta empresa">
            {st.last ? (
              <p className="flex items-start gap-2">
                <CircleCheck
                  size={16}
                  className="mt-0.5 shrink-0 text-emerald-600"
                  aria-hidden="true"
                />
                <span>
                  Última copia {ago(st.days_since ?? 0)} ({when(st.last.at)}),{' '}
                  {fileSize(st.last.bytes)}
                  {st.last.auto && ', automática'}.
                  <span className="block break-all text-stone-500">{st.last.path}</span>
                </span>
              </p>
            ) : (
              <p className="flex items-start gap-2 text-amber-900">
                <TriangleAlert size={16} className="mt-0.5 shrink-0" aria-hidden="true" />
                Todavía no has hecho ninguna copia.
              </p>
            )}
            <p className="mt-2 flex items-start gap-2 text-stone-700">
              <KeyRound size={16} className="mt-0.5 shrink-0 text-stone-500" aria-hidden="true" />
              {st.recovery_code
                ? 'Código de recuperación guardado junto a tus datos: con la carpeta de TaxPain y esta contraseña se abren en otro Mac.'
                : 'El código de recuperación se creará la próxima vez que abras TaxPain.'}
            </p>
            {st.last_error && (
              <p role="alert" className="mt-2 text-red-700">
                La última copia falló ({when(st.last_error.at)}): {st.last_error.message}
              </p>
            )}
            <div className="mt-4 form-actions">
              <button
                type="button"
                disabled={busy}
                onClick={() => void create()}
                className="btn-primary rounded-md px-4 py-2 disabled:opacity-50"
              >
                {busy ? 'Haciendo la copia…' : 'Hacer copia ahora'}
              </button>
            </div>
            <div className="mt-2 -ml-3 flex flex-wrap gap-1">
              <button
                type="button"
                className="btn-ghost"
                aria-expanded={panel === 'verificar'}
                onClick={() => setPanel(panel === 'verificar' ? null : 'verificar')}
              >
                Comprobar la contraseña
              </button>
              <button
                type="button"
                className="btn-ghost"
                aria-expanded={panel === 'cambiar'}
                onClick={() => setPanel(panel === 'cambiar' ? null : 'cambiar')}
              >
                Cambiar la contraseña
              </button>
            </div>
            {msg && (
              <p
                role={msg.ok ? 'status' : 'alert'}
                className={`mt-3 ${msg.ok ? 'text-emerald-700' : 'text-red-700'}`}
              >
                {msg.text}
              </p>
            )}
            {panel === 'verificar' && <VerifyForm onClose={() => setPanel(null)} />}
            {panel === 'cambiar' && (
              <div className="mt-4 rounded-lg border border-stone-200 p-4">
                <PassphraseForm
                  min={st.min_passphrase}
                  changing
                  onDone={() => {
                    setPanel(null);
                    setMsg({ ok: true, text: 'Contraseña cambiada.' });
                    load();
                  }}
                  onCancel={() => setPanel(null)}
                />
              </div>
            )}
          </Card>
          <Card icon={<FolderSync size={16} />} title="Copias automáticas">
            <AutoSettings key={JSON.stringify(st.settings)} st={st} onSaved={load} />
          </Card>
        </div>
      )}
      {st && (
        <div className="mt-4">
          <Card icon={<ArchiveRestore size={16} />} title="Restaurar una copia">
            <Restore />
          </Card>
        </div>
      )}
    </section>
  );
}

/**
 * Aviso tras restaurar una copia: antes de emitir hay que comprobar que no se emitió nada
 * después de la copia en el otro Mac (se repetirían números y se rompería la cadena VERI*FACTU).
 */
export function RestoreBanner({
  className = 'mt-5',
  onChecked,
}: {
  className?: string;
  onChecked?: () => void;
}) {
  const [check, setCheck] = useState<RestoreCheck | null>(null);
  useEffect(() => {
    restoreCheck()
      .then(setCheck)
      .catch(() => setCheck(null));
  }, []);
  if (!check || check.restored.acknowledged_at) return null;
  const vf = check.last_verifactu;
  return (
    <section
      aria-labelledby="restore-title"
      className={`${className} rounded-lg border border-amber-300 bg-amber-50 p-4 text-sm text-amber-950`}
    >
      <h2 id="restore-title" className="flex items-center gap-2 font-semibold">
        <TriangleAlert size={16} aria-hidden="true" />
        Empresa restaurada de una copia del {when(check.restored.copy_created_at)}
      </h2>
      <p className="mt-2">
        Antes de emitir facturas, comprueba que después de esa copia no emitiste ninguna en el otro
        Mac. Si lo hiciste, emitir aquí repetiría números y rompería la cadena de VERI*FACTU: haz
        una copia más reciente allí y restáurala.
      </p>
      {check.last_by_series.length === 0 ? (
        <p className="mt-2">La copia no tiene facturas emitidas.</p>
      ) : (
        <ul className="mt-2 list-disc space-y-0.5 pl-5">
          {check.last_by_series.map(([series, number, date]) => (
            <li key={series}>
              Última factura de la serie {series}: <strong>{number}</strong> ({formatDate(date)})
            </li>
          ))}
          {vf && (
            <li>
              Último registro VERI*FACTU: {vf[0]} del {vf[1]}, huella{' '}
              <code className="text-xs">
                {vf[2].slice(0, 8)}…{vf[2].slice(-8)}
              </code>
            </li>
          )}
        </ul>
      )}
      <div className="mt-3">
        <ConfirmAction
          label="Son las últimas: ya puedo emitir"
          question="¿Confirmas que no emitiste ninguna factura después de esta copia? Queda anotado en la auditoría."
          confirmLabel="Sí, son las últimas"
          className="btn-primary rounded-md px-4 py-2"
          danger={false}
          onConfirm={async () => {
            await restoreAcknowledge();
            setCheck(null);
            onChecked?.();
          }}
        />
      </div>
    </section>
  );
}

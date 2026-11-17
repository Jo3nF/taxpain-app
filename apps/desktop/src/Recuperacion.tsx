import { Archive, LifeBuoy, TriangleAlert } from 'lucide-react';
import { type FormEvent, useState } from 'react';
import {
  type CopiaEncontrada,
  type Diagnostico,
  errorMessage,
  fileSize,
  pickBackupFile,
  pickZipSave,
  type Rescate,
  recuperacionRescatar,
  recuperacionRestaurar,
} from './api';

const INPUT = 'mt-1 block w-full rounded-md border border-stone-300 px-3 py-2';
const BUTTON = 'rounded-md border border-stone-300 bg-white px-3 py-1.5 text-sm';
const PRIMARY = 'rounded-md bg-stone-900 px-3 py-1.5 text-sm text-white disabled:opacity-60';

function fecha(iso: string | null): string {
  if (!iso) return '';
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString('es-ES');
}

/** Abrir una copia: se elige (o se propone) y se pide su contraseña. Nada más se toca. */
function AbrirCopia({ copias, onRestored }: { copias: CopiaEncontrada[]; onRestored: () => void }) {
  const [ruta, setRuta] = useState<string | null>(null);
  const [pass, setPass] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function elegir() {
    setError(null);
    try {
      const p = await pickBackupFile();
      if (p) setRuta(p);
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!ruta || busy) return;
    setBusy(true);
    setError(null);
    try {
      await recuperacionRestaurar(ruta, pass);
      onRestored();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-2">
      <h3 className="font-semibold">Abrir una copia de seguridad</h3>
      <p className="text-stone-600">
        La copia se restaura como empresa nueva y se abre; la que no abre se queda como está, por si
        hace falta. La emisión de facturas queda en pausa hasta que compruebes la numeración.
      </p>
      {copias.length > 0 && (
        <ul className="divide-y divide-stone-200 rounded-md border border-stone-200">
          {copias.map((c) => (
            <li key={c.ruta} className="flex items-center justify-between gap-2 px-3 py-2">
              <span className="min-w-0">
                <span className="block truncate font-medium">{c.nombre}</span>
                <span className="text-xs text-stone-500">
                  {fecha(c.modificada)} · {fileSize(c.bytes)}
                </span>
              </span>
              <button
                type="button"
                className={BUTTON}
                aria-pressed={ruta === c.ruta}
                onClick={() => setRuta(c.ruta)}
              >
                {ruta === c.ruta ? 'Elegida' : 'Elegir'}
              </button>
            </li>
          ))}
        </ul>
      )}
      <button type="button" className={BUTTON} onClick={elegir}>
        {copias.length > 0 ? 'Elegir otra copia…' : 'Elegir la copia (.taxpain)…'}
      </button>
      {ruta && (
        <form onSubmit={submit} className="space-y-2" aria-label="Abrir la copia">
          <p className="break-all text-xs text-stone-500">{ruta}</p>
          <label className="block">
            Contraseña de la copia
            <input
              type="password"
              className={INPUT}
              value={pass}
              onChange={(e) => setPass(e.target.value)}
              autoComplete="off"
            />
          </label>
          <button type="submit" className={PRIMARY} disabled={busy || pass.length === 0}>
            {busy ? 'Restaurando…' : 'Restaurar y abrir'}
          </button>
        </form>
      )}
      {error && (
        <p role="alert" className="text-red-800">
          {error}
        </p>
      )}
    </div>
  );
}

/** Exportar lo que se pueda: los ficheros tal cual y, si se puede leer, cada tabla en CSV. */
function Rescatar({ diag }: { diag: Diagnostico }) {
  const [aviso, setAviso] = useState(false);
  const [busy, setBusy] = useState(false);
  const [hecho, setHecho] = useState<Rescate | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function exportar() {
    setBusy(true);
    setError(null);
    try {
      const path = await pickZipSave(`TaxPain-rescate-${diag.id}.zip`);
      if (!path) return;
      setHecho(await recuperacionRescatar(diag.id, path));
      setAviso(false);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-2">
      <h3 className="font-semibold">Exportar lo que se pueda</h3>
      <p className="text-stone-600">
        Copia los ficheros de la empresa tal cual (cifrados) y,{' '}
        {diag.rescate_legible
          ? 'como el Llavero tiene la clave, cada tabla en CSV y JSON con sus documentos.'
          : 'si se pudiera leer, cada tabla en CSV; ahora el Llavero no tiene la clave, así que sólo saldrán los ficheros cifrados.'}{' '}
        Sólo lee: no cambia nada de la empresa.
      </p>
      {!aviso ? (
        <button type="button" className={BUTTON} onClick={() => setAviso(true)}>
          <Archive size={14} aria-hidden="true" className="mr-1 inline" />
          Exportar a un ZIP…
        </button>
      ) : (
        <fieldset
          aria-label="Confirmar la exportación"
          className="space-y-2 rounded-md bg-amber-50 p-3 text-amber-900"
        >
          <p>
            El ZIP lleva tus datos <strong>sin cifrar</strong> (facturas, contabilidad, clientes).
            Guárdalo en un sitio seguro y bórralo cuando ya no lo necesites.
          </p>
          <div className="flex gap-2">
            <button type="button" className={PRIMARY} disabled={busy} onClick={exportar}>
              {busy ? 'Exportando…' : 'Entendido, elegir dónde guardarlo'}
            </button>
            <button
              type="button"
              className={BUTTON}
              disabled={busy}
              onClick={() => setAviso(false)}
            >
              Cancelar
            </button>
          </div>
        </fieldset>
      )}
      {hecho && (
        <p role="status" className="text-emerald-800">
          Guardado {hecho.fichero}: {hecho.originales.length} ficheros originales
          {hecho.tablas > 0 ? `, ${hecho.tablas} tablas y ${hecho.filas} filas` : ''}.
          {hecho.sin_volcado ? ` ${hecho.sin_volcado}.` : ''}
        </p>
      )}
      {error && (
        <p role="alert" className="text-red-800">
          {error}
        </p>
      )}
    </div>
  );
}

/** Qué le pasa a una empresa que no abre y qué se puede hacer, sin tocar nada sin confirmar. */
export function RecuperacionEmpresa({
  diag,
  onRestored,
}: {
  diag: Diagnostico;
  onRestored: () => void;
}) {
  return (
    <section aria-labelledby={`rec-${diag.id}`} className="space-y-4 text-sm">
      <div>
        <h2 id={`rec-${diag.id}`} className="flex items-center gap-2 text-base font-semibold">
          <TriangleAlert size={18} aria-hidden="true" className="text-amber-600" />
          {diag.empresa ? `${diag.empresa}: ` : ''}
          {diag.titulo}
        </h2>
        <p className="mt-2 whitespace-pre-line">{diag.explicacion}</p>
        <details className="mt-2 text-xs text-stone-500">
          <summary>Detalle técnico (para pedir ayuda)</summary>
          <p className="mt-1 break-all">
            Empresa {diag.id}: {diag.detalle}
          </p>
        </details>
      </div>
      <AbrirCopia copias={diag.copias} onRestored={onRestored} />
      <Rescatar diag={diag} />
    </section>
  );
}

/** Al arrancar, si no abre ninguna empresa: la pantalla de recuperación de cada una. */
export function PantallaRecuperacion({
  text,
  broken,
  onRestored,
}: {
  text: string;
  broken: Diagnostico[];
  onRestored: () => void;
}) {
  return (
    <div className="space-y-6">
      <h1 className="flex items-center gap-2 text-lg font-semibold">
        <LifeBuoy size={18} aria-hidden="true" />
        TaxPain no puede abrir tus datos
      </h1>
      {broken.length === 0 ? (
        <p className="whitespace-pre-line">{text}</p>
      ) : (
        broken.map((d) => (
          <div key={d.id} className="border-t border-stone-200 pt-4">
            <RecuperacionEmpresa diag={d} onRestored={onRestored} />
          </div>
        ))
      )}
    </div>
  );
}

import { KeyRound, LifeBuoy, TriangleAlert } from 'lucide-react';
import { type FormEvent, type ReactNode, useEffect, useState } from 'react';
import {
  backupStatus,
  type Diagnostico,
  errorMessage,
  isTauri,
  vaultLocked,
  vaultRecover,
} from './api';
import { PassphraseForm } from './Backups';
import { LogoMark } from './Logo';
import { PantallaRecuperacion } from './Recuperacion';

const INPUT = 'mt-1 block w-full rounded-md border border-stone-300 px-3 py-2';

/** Pantalla centrada, sin la barra lateral: antes de entrar en la app. */
function Screen({ children }: { children: ReactNode }) {
  return (
    <main className="mx-auto max-w-xl px-6 py-12">
      <div className="mb-6 flex items-center gap-2.5">
        <LogoMark size={30} />
        <span className="text-lg font-semibold">TaxPain</span>
      </div>
      <div className="card p-6 text-sm">{children}</div>
    </main>
  );
}

/**
 * El llavero de este Mac no tiene la clave de los datos, pero junto a ellos está el código de
 * recuperación: con la contraseña de las copias se recuperan y la clave vuelve al llavero.
 */
export function RecoverVault({
  several,
  onRecovered,
}: {
  several: boolean;
  onRecovered: () => void;
}) {
  const [pass, setPass] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await vaultRecover(pass);
      setPass('');
      onRecovered();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Screen>
      <h1 className="flex items-center gap-2 text-lg font-semibold">
        <LifeBuoy size={18} aria-hidden="true" />
        Recupera tus datos
      </h1>
      <p className="mt-3">
        Tus datos están en este Mac, pero el llavero no tiene la clave que los cifra. Suele pasar al
        cambiar de Mac, con una cuenta de usuario nueva o si se borró del llavero. Tus datos no se
        han tocado.
      </p>
      <p className="mt-2">
        Escribe la <strong>contraseña de las copias de seguridad</strong> (la que elegiste al dar de
        alta la empresa o en Ajustes → Copias de seguridad). TaxPain abrirá tus datos con el código
        de recuperación y volverá a guardar la clave en el llavero.
        {several && ' Si tienes varias empresas, usa la de cualquiera de ellas.'}
      </p>
      <form onSubmit={submit} className="mt-4 space-y-3">
        <label className="block font-medium text-stone-700">
          Contraseña de las copias
          <input
            type="password"
            autoComplete="off"
            value={pass}
            onChange={(e) => setPass(e.target.value)}
            className={INPUT}
          />
        </label>
        {error && (
          <p role="alert" className="text-red-700">
            {error}
          </p>
        )}
        <button
          type="submit"
          disabled={busy || !pass}
          className="btn-primary rounded-md px-4 py-2 disabled:opacity-50"
        >
          {busy ? 'Recuperando… (tarda unos segundos)' : 'Recuperar mis datos'}
        </button>
      </form>
      <p className="mt-4 text-stone-600">
        Si no la recuerdas, no hay otra forma de abrir estos datos: ni nosotros podemos. Si tienes
        una copia de seguridad (.taxpain) cuya contraseña sí recuerdas, pide ayuda para restaurarla.
      </p>
    </Screen>
  );
}

/**
 * Alta de la empresa: antes de empezar se elige la contraseña de las copias, que guarda también
 * el código de recuperación de los datos.
 */
export function ProtectSetup({ min, onDone }: { min: number; onDone: () => void }) {
  return (
    <Screen>
      <h1 className="flex items-center gap-2 text-lg font-semibold">
        <KeyRound size={18} aria-hidden="true" />
        Antes de empezar: protege tus datos
      </h1>
      <p className="mt-3">
        Tus datos se guardan cifrados con una clave del llavero de este Mac. Elige ahora una
        contraseña para tus copias de seguridad: con ella se guarda también un{' '}
        <strong>código de recuperación</strong> junto a tus datos, y si cambias de Mac o el llavero
        pierde la clave, la carpeta de TaxPain y esta contraseña bastarán para abrirlos.
      </p>
      <p className="mt-2 mb-4 flex gap-2 font-medium text-amber-900">
        <TriangleAlert size={16} className="mt-0.5 shrink-0" aria-hidden="true" />
        Sin esta contraseña no se puede recuperar nada.
      </p>
      <PassphraseForm min={min} changing={false} onDone={onDone} />
    </Screen>
  );
}

type Gate =
  | { kind: 'cargando' }
  | { kind: 'bloqueado'; several: boolean }
  | { kind: 'fallo'; text: string; broken: Diagnostico[] }
  | { kind: 'alta'; min: number }
  | { kind: 'listo' };

/**
 * Antes de la app: si los datos no se han podido abrir por falta de la clave, se recuperan con
 * la contraseña de las copias; si la empresa es nueva y aún no la tiene, se pide (alta).
 */
export function DataGate({ children }: { children: ReactNode }) {
  const [gate, setGate] = useState<Gate>(() =>
    isTauri() ? { kind: 'cargando' } : { kind: 'listo' },
  );

  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    (async () => {
      let next: Gate = { kind: 'listo' };
      try {
        const locked = await vaultLocked();
        if (locked?.failed) {
          // No se han podido abrir los datos (lo explica también un diálogo): sin llamar a nada
          // que los necesite.
          next = { kind: 'fallo', text: locked.failed, broken: locked.broken ?? [] };
        } else if (locked) {
          next = { kind: 'bloqueado', several: locked.several };
        } else {
          const st = await backupStatus();
          if (st.setup_required) next = { kind: 'alta', min: st.min_passphrase };
        }
      } catch {
        // Si no se puede saber, se entra en la app: Ajustes lo vuelve a ofrecer.
      }
      if (!cancelled) setGate(next);
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (gate.kind === 'cargando') return null;
  if (gate.kind === 'fallo') {
    return (
      <Screen>
        <PantallaRecuperacion
          text={gate.text}
          broken={gate.broken}
          onRestored={() => window.location.reload()}
        />
      </Screen>
    );
  }
  if (gate.kind === 'bloqueado') {
    return <RecoverVault several={gate.several} onRecovered={() => window.location.reload()} />;
  }
  if (gate.kind === 'alta') {
    return <ProtectSetup min={gate.min} onDone={() => setGate({ kind: 'listo' })} />;
  }
  return <>{children}</>;
}

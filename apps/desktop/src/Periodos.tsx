import { Lock } from 'lucide-react';
import { type FormEvent, useCallback, useEffect, useState } from 'react';
import {
  errorMessage,
  type PeriodoCerrado,
  type PeriodoSugerido,
  periodoCerrar,
  periodoReabrir,
  periodosListar,
  periodosSugeridos,
  type TipoPeriodo,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const INPUT = 'mt-1 block w-full rounded-md border border-stone-300 px-2 py-1.5 text-sm';
const BUTTON = 'rounded-md border border-stone-300 bg-white px-3 py-1.5 text-sm';

function fecha(d: string): string {
  const [y, m, day] = d.split('-');
  return `${day}/${m}/${y}`;
}

/**
 * Periodos cerrados: un trimestre con el 303 presentado o un ejercicio cerrado. Emitir,
 * registrar o cambiar algo con fecha de un periodo cerrado pide confirmación y avisa de la
 * rectificativa o la complementaria (no se impide: la ley deja rectificar).
 */
export function Periodos({ now = new Date() }: { now?: Date }) {
  const [lista, setLista] = useState<PeriodoCerrado[]>([]);
  const [sugeridos, setSugeridos] = useState<PeriodoSugerido[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [tipo, setTipo] = useState<TipoPeriodo>('trimestre');
  const [ejercicio, setEjercicio] = useState(now.getFullYear());
  const [trimestre, setTrimestre] = useState(Math.max(1, Math.floor(now.getMonth() / 3)));
  const [motivo, setMotivo] = useState('');
  const [nuevo, setNuevo] = useState(false);

  const cargar = useCallback(() => {
    periodosListar()
      .then(setLista)
      .catch((e) => setError(errorMessage(e)));
    periodosSugeridos()
      .then(setSugeridos)
      .catch(() => setSugeridos([]));
  }, []);

  useEffect(cargar, [cargar]);

  async function cerrar(t: TipoPeriodo, e: number, q: number | null, m: string) {
    setError(null);
    await periodoCerrar(t, e, q, m);
    setNuevo(false);
    setMotivo('');
    cargar();
  }

  function submit(e: FormEvent) {
    e.preventDefault();
  }

  return (
    <section className="mt-8" aria-labelledby="periodos-title">
      <div className="flex items-center gap-3">
        <span className="icon-tile" aria-hidden="true">
          <Lock size={18} />
        </span>
        <h2 id="periodos-title" className="text-lg font-semibold">
          Periodos cerrados
        </h2>
      </div>
      <p className="mt-1 text-sm text-stone-600">
        Cierra el trimestre cuando hayas presentado su 303 y el ejercicio cuando esté cerrado. Si
        luego vas a emitir, registrar o cambiar algo con fecha de un periodo cerrado, TaxPain te
        avisa y te pide que lo confirmes: lo que ya está declarado se corrige con una rectificativa
        o una complementaria. Cada aviso confirmado queda en la auditoría.
      </p>
      {error && (
        <p role="alert" className="mt-2 text-sm text-red-800">
          {error}
        </p>
      )}
      {sugeridos.length > 0 && (
        <div className="mt-3 rounded-md bg-amber-50 p-3 text-sm text-amber-900">
          <p className="font-medium">Conviene cerrar:</p>
          <ul className="mt-1 space-y-1">
            {sugeridos.map((s) => (
              <li
                key={`${s.tipo}-${s.ejercicio}-${s.trimestre}`}
                className="flex items-center gap-2"
              >
                <span className="flex-1">
                  {s.etiqueta} ({s.motivo})
                </span>
                <ConfirmAction
                  label="Cerrar"
                  ariaLabel={`Cerrar el ${s.etiqueta}`}
                  danger={false}
                  className="underline"
                  question={`¿Cerrar el ${s.etiqueta}? Lo que hagas luego con esas fechas pedirá confirmación.`}
                  confirmLabel="Cerrar el periodo"
                  onConfirm={() => cerrar(s.tipo, s.ejercicio, s.trimestre, s.motivo)}
                />
              </li>
            ))}
          </ul>
        </div>
      )}
      {lista.length === 0 ? (
        <p className="mt-3 text-sm text-stone-500">Aún no hay ningún periodo cerrado.</p>
      ) : (
        <ul className="mt-3 divide-y divide-stone-200 rounded-md border border-stone-200 text-sm">
          {lista.map((p) => (
            <li key={p.id} className="flex items-center gap-3 px-3 py-2">
              <span className="flex-1">
                <strong>{p.etiqueta}</strong> · del {fecha(p.desde)} al {fecha(p.hasta)}
                {p.motivo && <span className="text-stone-500"> · {p.motivo}</span>}
              </span>
              <ConfirmAction
                label="Reabrir"
                ariaLabel={`Reabrir el ${p.etiqueta}`}
                question={`¿Reabrir el ${p.etiqueta}? Dejará de avisar al cambiar cosas de esas fechas. Se pide Touch ID y queda en la auditoría.`}
                confirmLabel="Reabrir"
                onConfirm={async () => {
                  await periodoReabrir(p.id);
                  cargar();
                }}
              />
            </li>
          ))}
        </ul>
      )}
      {!nuevo ? (
        <button type="button" className={`${BUTTON} mt-3`} onClick={() => setNuevo(true)}>
          Cerrar otro periodo…
        </button>
      ) : (
        <form
          onSubmit={submit}
          className="mt-3 grid gap-3 sm:grid-cols-4"
          aria-label="Cerrar un periodo"
        >
          <label className="text-sm">
            Qué
            <select
              className={INPUT}
              value={tipo}
              onChange={(e) => setTipo(e.target.value as TipoPeriodo)}
            >
              <option value="trimestre">Trimestre</option>
              <option value="ejercicio">Ejercicio</option>
            </select>
          </label>
          <label className="text-sm">
            Año
            <input
              type="number"
              className={INPUT}
              value={ejercicio}
              onChange={(e) => setEjercicio(Number(e.target.value))}
            />
          </label>
          {tipo === 'trimestre' && (
            <label className="text-sm">
              Trimestre
              <select
                className={INPUT}
                value={trimestre}
                onChange={(e) => setTrimestre(Number(e.target.value))}
              >
                {[1, 2, 3, 4].map((t) => (
                  <option key={t} value={t}>
                    {t}T
                  </option>
                ))}
              </select>
            </label>
          )}
          <label className="text-sm">
            Motivo
            <input
              className={INPUT}
              value={motivo}
              placeholder="303 presentado"
              onChange={(e) => setMotivo(e.target.value)}
            />
          </label>
          <div className="flex gap-2 sm:col-span-4">
            <ConfirmAction
              label="Cerrar el periodo"
              danger={false}
              className={BUTTON}
              question={`¿Cerrar el ${tipo === 'trimestre' ? `${trimestre}T ${ejercicio}` : `ejercicio ${ejercicio}`}?`}
              confirmLabel="Cerrar"
              onConfirm={() =>
                cerrar(tipo, ejercicio, tipo === 'trimestre' ? trimestre : null, motivo)
              }
            />
            <button type="button" className={BUTTON} onClick={() => setNuevo(false)}>
              Cancelar
            </button>
          </div>
        </form>
      )}
    </section>
  );
}

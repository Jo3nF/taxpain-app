import { useCallback, useEffect, useState } from 'react';
import {
  errorMessage,
  euros,
  formatDate,
  type Modalidad202,
  model202SetModality,
  model202Simulation,
  type PeriodoSimulado202,
  type Simulacion202,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const FUENTE: Record<PeriodoSimulado202['fuente'], string> = {
  contabilidad: 'Contabilidad',
  presupuesto: 'Presupuesto',
  ano_anterior: 'Mismos meses del año anterior',
  sin_datos: 'Sin datos',
};

const NOMBRE: Record<Modalidad202, string> = {
  Cuota: 'Por la cuota del último Impuesto sobre Sociedades (art. 40.2 LIS)',
  Base: 'Por la base imponible del periodo (art. 40.3 LIS)',
};

/**
 * 202: modalidad del ejercicio (cuota o base), simulador que compara lo que se pagaría en el año
 * con cada una y plazo de la opción en el censo (036).
 */
export function Pago202({ year, onChange }: { year: number; onChange: () => void }) {
  const [sim, setSim] = useState<Simulacion202 | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [choice, setChoice] = useState<Modalidad202>('Cuota');
  const [msg, setMsg] = useState<string | null>(null);

  const load = useCallback(() => {
    setError(null);
    model202Simulation(year)
      .then((s) => {
        setSim(s);
        setChoice(s.modalidad.elegida ?? s.modalidad.efectiva);
      })
      .catch((e) => {
        setSim(null);
        setError(errorMessage(e));
      });
  }, [year]);
  useEffect(load, [load]);

  if (error) {
    return (
      <p role="alert" className="mt-4 text-sm text-red-700">
        {error}
      </p>
    );
  }
  if (!sim) return null;
  const m = sim.modalidad;
  const saved = m.elegida ?? 'Cuota';
  return (
    <section
      aria-labelledby="pago202-h"
      className="mt-4 rounded-lg border border-stone-200 bg-white p-4 text-sm"
    >
      <h3 id="pago202-h" className="font-semibold">
        Modalidad del 202 en {sim.ejercicio}
      </h3>
      <p className="mt-1 text-stone-600">
        Se aplica: <strong>{NOMBRE[m.efectiva]}</strong>.
        {m.cifra_negocios !== null &&
          ` Cifra de negocios del ejercicio anterior: ${euros(m.cifra_negocios)}.`}
      </p>
      {m.aviso && (
        <p role="status" className="mt-2 rounded border border-amber-300 bg-amber-50 p-2">
          {m.aviso}
        </p>
      )}
      <fieldset className="mt-3">
        <legend className="font-medium">Modalidad elegida para {sim.ejercicio}</legend>
        {(['Cuota', 'Base'] as const).map((k) => (
          <label key={k} className="mt-1 flex items-start gap-2">
            <input
              type="radio"
              name={`modalidad-202-${sim.ejercicio}`}
              checked={choice === k}
              disabled={m.obligatoria}
              onChange={() => setChoice(k)}
            />
            {NOMBRE[k]}
          </label>
        ))}
      </fieldset>
      {!m.obligatoria && choice !== saved && (
        <div className="mt-2">
          <ConfirmAction
            label="Guardar la modalidad"
            className="rounded border border-stone-300 px-3 py-1"
            danger={false}
            question={
              choice === 'Base'
                ? `El borrador del 202 de ${sim.ejercicio} pasará a calcularse por la base. Recuerda que la opción se comunica a la AEAT en el modelo 036 entre el ${formatDate(sim.opcion_desde)} y el ${formatDate(sim.opcion_hasta)} (TaxPain no la presenta) y vale hasta que renuncies.`
                : `El borrador del 202 de ${sim.ejercicio} volverá a calcularse por la cuota. Si habías optado por la base en el censo, la renuncia también se hace en el 036 en ese plazo.`
            }
            confirmLabel="Guardar"
            onConfirm={async () => {
              await model202SetModality(sim.ejercicio, choice);
              setMsg('Modalidad guardada.');
              load();
              onChange();
            }}
          />
        </div>
      )}
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}

      <h4 className="mt-4 font-medium">Simulador: lo que pagarías en {sim.ejercicio}</h4>
      <div className="mt-2 overflow-x-auto">
        <table className="w-full text-left">
          <caption className="sr-only">Pagos del 202 por cada modalidad</caption>
          <thead>
            <tr className="border-b border-stone-200">
              <th scope="col">Pago</th>
              <th scope="col">Base hasta</th>
              <th scope="col" className="text-right">
                Por la cuota
              </th>
              <th scope="col" className="text-right">
                Base imponible
              </th>
              <th scope="col" className="text-right">
                Por la base
              </th>
              <th scope="col">De dónde sale</th>
            </tr>
          </thead>
          <tbody>
            {sim.periodos.map((p) => (
              <tr key={`${p.period}-${p.hasta}`} className="border-b border-stone-100">
                <th scope="row">{p.period}P</th>
                <td>{formatDate(p.hasta)}</td>
                <td className="text-right">{p.cuota === null ? '—' : euros(p.cuota)}</td>
                <td className="text-right">{euros(p.base_imponible)}</td>
                <td className="text-right">{euros(p.base)}</td>
                <td>{FUENTE[p.fuente]}</td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr className="font-semibold">
              <th scope="row" colSpan={2}>
                Total del año
              </th>
              <td className="text-right">
                {sim.total_cuota === null ? '—' : euros(sim.total_cuota)}
              </td>
              <td />
              <td className="text-right">{euros(sim.total_base)}</td>
              <td />
            </tr>
          </tfoot>
        </table>
      </div>
      <p className="mt-3">{sim.conclusion}</p>
      <p className="mt-1 text-stone-600">
        Plazo de la opción (o de la renuncia) en el censo: del {formatDate(sim.opcion_desde)} al{' '}
        {formatDate(sim.opcion_hasta)}.
      </p>
      {sim.notas.length > 0 && (
        <ul className="mt-2 list-disc pl-5 text-stone-600">
          {sim.notas.map((n) => (
            <li key={n}>{n}</li>
          ))}
        </ul>
      )}
    </section>
  );
}

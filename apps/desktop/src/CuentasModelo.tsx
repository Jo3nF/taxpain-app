import { useEffect, useRef, useState } from 'react';
import {
  type AccountsModel,
  cuentasModeloAviso,
  cuentasModeloElegir,
  type Ecpn,
  ecpn,
  ecpnSave,
  errorMessage,
  euros,
  type MemoriaTextos,
  type ModeloAviso,
  memoriaSave,
  memoriaTextos,
  memoriaTextosGuardar,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const NOMBRE: Record<AccountsModel, string> = {
  Pymes: 'PYMES',
  Abreviado: 'abreviado',
  Normal: 'normal',
};

const decimas = (d: number) => `${Math.trunc(d / 10)},${Math.abs(d % 10)}`;

/**
 * Qué modelo de cuentas toca por tamaño (arts. 257 y 258 LSC, RD 1515/2007) con los datos de los
 * dos últimos ejercicios, y el elegido para la sociedad. Al cargar, pone el modelo elegido en la
 * vista (`onModel`); si se mira otro, ofrece guardarlo como el de la sociedad.
 */
export function ModeloCuentas({
  year,
  model,
  onModel,
}: {
  year: number;
  model: AccountsModel;
  onModel: (m: AccountsModel) => void;
}) {
  const [a, setA] = useState<ModeloAviso | null>(null);
  const [error, setError] = useState<string | null>(null);
  const first = useRef(true);
  useEffect(() => {
    cuentasModeloAviso(year)
      .then((x) => {
        setA(x);
        if (first.current) {
          first.current = false;
          onModel(x.elegido);
        }
      })
      .catch((e) => setError(errorMessage(e)));
  }, [year, onModel]);
  if (error)
    return (
      <p role="alert" className="text-sm text-red-700">
        {error}
      </p>
    );
  if (!a) return null;
  return (
    <section aria-label="Modelo de cuentas" className="card p-4 text-sm">
      <h3 className="font-semibold">Modelo de cuentas anuales</h3>
      <p className="mt-1">
        Por tamaño puedes usar el modelo <strong>{NOMBRE[a.recomendado]}</strong>. La sociedad tiene
        elegido el <strong>{NOMBRE[a.elegido]}</strong> (memoria, ECPN, 200 y depósito).
      </p>
      <table className="mt-2 text-sm">
        <thead className="text-stone-500">
          <tr>
            <th className="pr-3 text-left font-normal">Ejercicio</th>
            <th className="px-2 text-right font-normal">Activo</th>
            <th className="px-2 text-right font-normal">Cifra de negocios</th>
            <th className="px-2 text-right font-normal">Plantilla media</th>
            <th className="px-2 text-right font-normal">Condiciones del abreviado</th>
          </tr>
        </thead>
        <tbody>
          {a.ejercicios.map((e) => (
            <tr key={e.year}>
              <td className="pr-3">{e.year}</td>
              <td className="px-2 text-right tabular-nums">{euros(e.activo)}</td>
              <td className="px-2 text-right tabular-nums">{euros(e.cifra_negocios)}</td>
              <td className="px-2 text-right tabular-nums">{decimas(e.plantilla_decimas)}</td>
              <td className="px-2 text-right">{e.cumple_abreviado} de 3</td>
            </tr>
          ))}
        </tbody>
      </table>
      <ul className="mt-2 list-disc pl-5 text-stone-600">
        {a.explicacion.map((t) => (
          <li key={t}>{t}</li>
        ))}
      </ul>
      {a.aviso && (
        <p role="alert" className="mt-2 rounded bg-amber-100 p-2 text-amber-900">
          {a.aviso}
        </p>
      )}
      {model !== a.elegido && (
        <div className="mt-2">
          <ConfirmAction
            label={`Usar el modelo ${NOMBRE[model]} para la sociedad`}
            className="text-link underline"
            danger={false}
            question={`La memoria, el ECPN, el 200 y el depósito de cuentas pasarán a usar el modelo ${NOMBRE[model]}. ¿Seguir?`}
            confirmLabel="Sí, usarlo"
            onConfirm={async () => {
              await cuentasModeloElegir(model);
              setA(await cuentasModeloAviso(year));
            }}
          />
        </div>
      )}
    </section>
  );
}

/** Borrador de la memoria del modelo `model`, con los textos que no salen de la contabilidad. */
export function MemoriaDraft({ year, model }: { year: number; model: AccountsModel }) {
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [t, setT] = useState<MemoriaTextos | null>(null);
  const [firmantes, setFirmantes] = useState('');
  const [honorarios, setHonorarios] = useState('');
  useEffect(() => {
    memoriaTextos(year)
      .then((x) => {
        setT(x);
        setFirmantes(x.firmantes.join('\n'));
        setHonorarios(
          x.honorarios_auditoria === null ? '' : (x.honorarios_auditoria / 100).toFixed(2),
        );
      })
      .catch(() => setT(null));
  }, [year]);
  const save = (pdf: boolean) =>
    memoriaSave(year, pdf, model)
      .then((p) => p && setMsg({ ok: true, text: `Memoria guardada en ${p}.` }))
      .catch((e) => setMsg({ ok: false, text: errorMessage(e) }));
  async function guardarTextos() {
    if (!t) return;
    const h = honorarios.trim().replace(/\./g, '').replace(',', '.');
    const cents = h === '' ? null : Math.round(Number(h) * 100);
    if (cents !== null && (Number.isNaN(cents) || cents < 0)) {
      setMsg({ ok: false, text: 'Los honorarios han de ser un importe (0 si no se audita).' });
      return;
    }
    const nuevos = {
      ...t,
      honorarios_auditoria: cents,
      firmantes: firmantes
        .split('\n')
        .map((s) => s.trim())
        .filter(Boolean),
    };
    try {
      await memoriaTextosGuardar(year, nuevos);
      setT(nuevos);
      setMsg({ ok: true, text: 'Textos de la memoria guardados.' });
    } catch (e) {
      setMsg({ ok: false, text: errorMessage(e) });
    }
  }
  return (
    <div className="card p-4 text-sm">
      <h3 className="font-semibold">
        Memoria {year} ({NOMBRE[model]}, borrador)
      </h3>
      <p className="mt-1 text-stone-600">
        Las notas del modelo {NOMBRE[model]} con los datos de tu contabilidad: aplicación del
        resultado, inmovilizado con sus movimientos, activos y pasivos financieros con los
        vencimientos de los préstamos, fondos propios, situación fiscal con el 200, ingresos y
        gastos, plantilla media, periodo medio de pago… Lo que no se puede saber va entre corchetes.
        Edítala en Pages o Word y adjúntala en PDF al depósito de cuentas (o deja que el depósito
        incluya este borrador).
      </p>
      {t && (
        <details className="mt-2">
          <summary className="cursor-pointer text-stone-700">
            Lo que no sale de la contabilidad (hechos posteriores, auditoría, firmas)
          </summary>
          <div className="form-grid mt-2">
            <label className="span-full">
              Hechos posteriores al cierre
              <textarea
                rows={2}
                value={t.hechos_posteriores}
                onChange={(e) => setT({ ...t, hechos_posteriores: e.target.value })}
              />
            </label>
            <label>
              Honorarios de auditoría (€; 0 si no se audita)
              <input
                inputMode="decimal"
                value={honorarios}
                onChange={(e) => setHonorarios(e.target.value)}
              />
            </label>
            <label>
              Fecha de formulación
              <input
                type="date"
                value={t.formulacion ?? ''}
                onChange={(e) => setT({ ...t, formulacion: e.target.value || null })}
              />
            </label>
            <label className="span-full">
              Administradores que firman (uno por línea)
              <textarea rows={2} value={firmantes} onChange={(e) => setFirmantes(e.target.value)} />
            </label>
            <label className="span-full">
              Nota fija de regímenes especiales (se repite cada año; p. ej. el canje de valores del
              art. 86 LIS)
              <textarea
                rows={2}
                value={t.nota_fija}
                onChange={(e) => setT({ ...t, nota_fija: e.target.value })}
              />
            </label>
          </div>
          <button
            type="button"
            className="mt-2 rounded-md border border-stone-300 px-3 py-1.5"
            onClick={() => void guardarTextos()}
          >
            Guardar textos
          </button>
        </details>
      )}
      <div className="mt-3 flex flex-wrap gap-2">
        <button
          type="button"
          className="btn-primary rounded-md px-3 py-1.5"
          onClick={() => void save(false)}
        >
          Guardar para editar (RTF)…
        </button>
        <button
          type="button"
          className="rounded-md border border-stone-300 px-3 py-1.5"
          onClick={() => void save(true)}
        >
          Guardar en PDF…
        </button>
      </div>
      {msg && (
        <p role={msg.ok ? 'status' : 'alert'} className={`mt-2 ${msg.ok ? '' : 'text-red-700'}`}>
          {msg.text}
        </p>
      )}
    </div>
  );
}

/** Estado de cambios en el patrimonio neto: parte A y estado total (columnas con importes). */
export function EcpnView({ year, model }: { year: number; model: AccountsModel }) {
  const [e, setE] = useState<Ecpn | null>(null);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  useEffect(() => {
    ecpn(year, model)
      .then(setE)
      .catch((x) => setMsg({ ok: false, text: errorMessage(x) }));
  }, [year, model]);
  if (!e)
    return msg ? (
      <p role="alert" className="text-sm text-red-700">
        {msg.text}
      </p>
    ) : null;
  const cols = e.columnas
    .map((_, i) => i)
    .filter((i) => e.filas.some((f) => (f.importes[i] ?? 0) !== 0));
  const filas = e.filas.filter(
    (f) =>
      f.fila === 'SaldoInicial' ||
      f.fila === 'SaldoFinal' ||
      f.total !== 0 ||
      f.importes.some((x) => x !== 0),
  );
  const save = (pdf: boolean) =>
    ecpnSave(year, pdf, model)
      .then((p) => p && setMsg({ ok: true, text: `ECPN guardado en ${p}.` }))
      .catch((x) => setMsg({ ok: false, text: errorMessage(x) }));
  return (
    <section aria-label="Estado de cambios en el patrimonio neto" className="text-sm">
      <h3 className="font-semibold">Estado de cambios en el patrimonio neto</h3>
      <p className="text-stone-600">{e.nota}</p>
      <h4 className="mt-2 font-medium">A) Ingresos y gastos reconocidos</h4>
      <table className="w-full">
        <tbody>
          {e.ingresos_gastos.map((l) => (
            <tr key={l.label}>
              <td>{l.label}</td>
              <td className="px-2 text-right tabular-nums">{euros(l.amount)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <h4 className="mt-2 font-medium">B) Estado total de cambios</h4>
      <div className="overflow-x-auto">
        <table className="w-full">
          <thead className="text-stone-500">
            <tr>
              <th />
              {cols.map((i) => (
                <th key={e.columnas[i]} className="px-2 text-right font-normal">
                  {e.columnas[i]}
                </th>
              ))}
              <th className="px-2 text-right font-normal">Total</th>
            </tr>
          </thead>
          <tbody>
            {filas.map((f) => (
              <tr key={f.fila} className={f.fila === 'SaldoFinal' ? 'font-semibold' : ''}>
                <td>{f.label}</td>
                {cols.map((i) => (
                  <td key={e.columnas[i]} className="px-2 text-right tabular-nums">
                    {euros(f.importes[i] ?? 0)}
                  </td>
                ))}
                <td className="px-2 text-right tabular-nums">{euros(f.total)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="mt-2 flex flex-wrap gap-2">
        <button
          type="button"
          className="rounded-md border border-stone-300 px-3 py-1.5"
          onClick={() => void save(false)}
        >
          ECPN para editar (RTF)…
        </button>
        <button
          type="button"
          className="rounded-md border border-stone-300 px-3 py-1.5"
          onClick={() => void save(true)}
        >
          ECPN en PDF…
        </button>
      </div>
      {msg && (
        <p role={msg.ok ? 'status' : 'alert'} className={`mt-2 ${msg.ok ? '' : 'text-red-700'}`}>
          {msg.text}
        </p>
      )}
    </section>
  );
}

import { type FormEvent, useCallback, useEffect, useState } from 'react';
import {
  type CuotaRetaInput,
  type Employee,
  errorMessage,
  euros,
  type FichaReta,
  formatDate,
  type PagadorReta,
  parseEuros,
  type ResumenReta,
  retaCuotaDelete,
  retaCuotaRegister,
  retaFichaSave,
  retaResumen,
  tramoNombre,
} from './api';
import { ConfirmAction } from './ConfirmAction';

/** Céntimos → texto para un campo («1.000,00» → «1000,00»). */
function cents(v: number | null): string {
  return v == null ? '' : (v / 100).toFixed(2).replace('.', ',');
}

/** Centésimas de punto → «31,40 %». */
function pct(bp: number): string {
  return `${(bp / 100).toLocaleString('es-ES', { maximumFractionDigits: 2 })} %`;
}

type Campos = {
  base: string;
  previsto: string;
  meses: string;
  pctDiv: string;
  otros: string;
  gastos: string;
  acuerdoImporte: string;
};

function camposDe(f: FichaReta): Campos {
  return {
    base: cents(f.base_elegida),
    previsto: cents(f.rendimiento_previsto),
    meses: String(f.meses_alta),
    pctDiv: String(f.pct_dividendos / 100),
    otros: cents(f.otros_rendimientos),
    gastos: cents(f.gastos_deducibles),
    acuerdoImporte: cents(f.acuerdo_importe),
  };
}

/** Ficha anual del RETA y soporte societario de la retribución. */
function FichaForm({ initial, onSaved }: { initial: FichaReta; onSaved: () => void }) {
  const [f, setF] = useState(initial);
  const [c, setC] = useState(() => camposDe(initial));
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<FichaReta>) => setF({ ...f, ...patch });
  const campo = (k: keyof Campos, label: string, hint?: string) => (
    <label>
      {label}
      <input
        className="text-right"
        inputMode="decimal"
        value={c[k]}
        onChange={(x) => setC({ ...c, [k]: x.target.value })}
      />
      {hint && <span className="block text-xs text-stone-600">{hint}</span>}
    </label>
  );
  async function submit(ev: FormEvent) {
    ev.preventDefault();
    const money = (s: string) => (s.trim() ? parseEuros(s) : 0);
    const opt = (s: string) => (s.trim() ? parseEuros(s) : null);
    const base = money(c.base);
    const otros = money(c.otros);
    const gastos = money(c.gastos);
    const previsto = opt(c.previsto);
    const acuerdo = opt(c.acuerdoImporte);
    const meses = Number.parseInt(c.meses, 10);
    const pctDiv = Number.parseFloat(c.pctDiv.replace(',', '.'));
    if (
      base === null ||
      otros === null ||
      gastos === null ||
      (c.previsto.trim() && previsto === null) ||
      (c.acuerdoImporte.trim() && acuerdo === null) ||
      !(meses >= 1 && meses <= 12) ||
      !(pctDiv >= 0 && pctDiv <= 100)
    ) {
      return setError('Revisa los importes, los meses de alta (1 a 12) y el porcentaje (0 a 100)');
    }
    try {
      await retaFichaSave({
        ...f,
        base_elegida: base,
        rendimiento_previsto: previsto,
        meses_alta: meses,
        pct_dividendos: Math.round(pctDiv * 100),
        otros_rendimientos: otros,
        gastos_deducibles: gastos,
        acuerdo_importe: acuerdo,
        acuerdo_fecha: f.acuerdo_fecha || null,
        alta_societario: f.alta_societario || null,
      });
      setError(null);
      onSaved();
    } catch (err) {
      setError(errorMessage(err));
    }
  }
  return (
    <form onSubmit={submit} className="form-grid mt-2 rounded-md border border-stone-200 p-4">
      {campo('base', 'Base de cotización mensual')}
      {campo('previsto', 'Rendimiento neto mensual previsto', 'El que comunicaste a la TGSS')}
      {campo('meses', 'Meses de alta en el año')}
      <label>
        Fecha de alta como societario
        <input
          type="date"
          value={f.alta_societario ?? ''}
          onChange={(x) => set({ alta_societario: x.target.value || null })}
        />
        <span className="block text-xs text-stone-600">
          Vacía si viene de años anteriores. Con menos de 90 días de alta en el año no hay deducción
          del 3 % ni base mínima de los societarios.
        </span>
      </label>
      <label className="span-full flex gap-2">
        <input
          type="checkbox"
          checked={f.control_efectivo}
          onChange={(x) => set({ control_efectivo: x.target.checked })}
        />
        Socio con control efectivo de la sociedad: sus dividendos cuentan en el rendimiento
      </label>
      {f.control_efectivo && campo('pctDiv', 'Parte de los dividendos que computa (%)')}
      {campo('otros', 'Rendimientos netos de fuera de la sociedad (año)')}
      {campo('gastos', 'Gastos deducibles (año, sin la cuota)')}
      <label className="span-2">
        ¿Quién paga la cuota?
        <select value={f.pagador} onChange={(x) => set({ pagador: x.target.value as PagadorReta })}>
          <option value="Sociedad">La sociedad (retribución en especie)</option>
          <option value="Administrador">
            El administrador (si se la reembolsan, va en el bruto)
          </option>
        </select>
      </label>
      {f.pagador === 'Sociedad' && (
        <>
          <label>
            Cuenta de gasto
            <input value={f.cuenta_gasto} onChange={(x) => set({ cuenta_gasto: x.target.value })} />
          </label>
          <label>
            Cuenta de tesorería
            <input
              value={f.cuenta_tesoreria}
              onChange={(x) => set({ cuenta_tesoreria: x.target.value })}
            />
          </label>
        </>
      )}
      <p className="span-full text-stone-600">
        {f.pagador === 'Sociedad'
          ? 'La cuota que paga la sociedad es retribución del administrador por su cargo (art. 17.2.e LIRPF), deducible para la sociedad si los estatutos dicen que el cargo es retribuido (art. 15.e LIS). TaxPain la trata como retribución en especie, con su ingreso a cuenta del 19 o del 35 %, y la asienta en la cuenta de gasto (642 por defecto; hay quien usa la 649 o la 640) contra tesorería. Pendiente de verificar: la DGT la ha calificado también como renta dineraria; con ese criterio, elige la otra opción.'
          : 'Si la paga el administrador y la sociedad se la reembolsa, el reembolso es retribución dineraria: súmalo al bruto de su nómina (lleva la retención del 19 o del 35 %). Aquí sólo se anota la cuota para el cálculo, sin asiento.'}
      </p>
      <fieldset className="span-full">
        <legend className="font-medium">Soporte societario (art. 217 LSC)</legend>
        <label className="flex gap-2">
          <input
            type="checkbox"
            checked={f.estatutos_retribuido}
            onChange={(x) => set({ estatutos_retribuido: x.target.checked })}
          />
          Los estatutos dicen que el cargo de administrador es retribuido
        </label>
        <div className="form-grid mt-2">
          <label>
            Fecha del acuerdo de la junta
            <input
              type="date"
              value={f.acuerdo_fecha ?? ''}
              onChange={(x) => set({ acuerdo_fecha: x.target.value || null })}
            />
          </label>
          {campo('acuerdoImporte', 'Importe máximo anual aprobado')}
          <label className="span-2">
            Nota
            <input value={f.acuerdo_nota} onChange={(x) => set({ acuerdo_nota: x.target.value })} />
          </label>
        </div>
      </fieldset>
      <div className="span-full form-actions">
        <button type="submit" className="rounded btn-primary px-3 py-1">
          Guardar la ficha de {f.year}
        </button>
        {error && (
          <p role="alert" className="text-red-700">
            {error}
          </p>
        )}
      </div>
    </form>
  );
}

/** Alta de una cuota pagada, con confirmación (graba el asiento si la paga la sociedad). */
function CuotaForm({ r, onDone }: { r: ResumenReta; onDone: () => void }) {
  const [date, setDate] = useState('');
  const [amount, setAmount] = useState(cents(r.cuota_mensual));
  const [asiento, setAsiento] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const importe = parseEuros(amount || '0');
  const sociedad = r.ficha.pagador === 'Sociedad';
  const valid = !!date && importe !== null && importe > 0;
  const input: CuotaRetaInput = {
    employee_id: r.employee_id,
    date,
    amount: importe ?? 0,
    pagador: r.ficha.pagador,
    asiento: sociedad && asiento,
  };
  return (
    <div className="form-grid mt-2">
      <label>
        Fecha del cargo
        <input type="date" value={date} onChange={(x) => setDate(x.target.value)} />
      </label>
      <label>
        Importe de la cuota
        <input
          className="text-right"
          inputMode="decimal"
          value={amount}
          onChange={(x) => setAmount(x.target.value)}
        />
      </label>
      {sociedad && (
        <label className="span-2 flex items-start gap-2">
          <input type="checkbox" checked={asiento} onChange={(x) => setAsiento(x.target.checked)} />
          Contabilizarla ({r.ficha.cuenta_gasto} a {r.ficha.cuenta_tesoreria}). Desmárcalo si el
          cargo del banco ya se contabiliza con una regla.
        </label>
      )}
      <div className="span-full form-actions">
        <ConfirmAction
          label="Registrar la cuota"
          className="rounded btn-primary px-3 py-1"
          danger={false}
          disabled={!valid}
          question={
            valid
              ? `¿Registrar la cuota de ${euros(importe ?? 0)} del ${formatDate(date)}${input.asiento ? ' y su asiento' : ''}?`
              : 'Pon la fecha y el importe'
          }
          confirmLabel="Sí, registrar"
          onConfirm={async () => {
            try {
              await retaCuotaRegister(input);
              setError(null);
              setDate('');
              onDone();
            } catch (e) {
              setError(errorMessage(e));
            }
          }}
        />
        {error && (
          <p role="alert" className="text-red-700">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}

const SENTIDO = {
  Ninguna: 'sin regularización',
  AIngresar: 'a ingresar a la TGSS',
  ADevolver: 'a devolver por la TGSS',
} as const;

function Resumen({ r, onChange }: { r: ResumenReta; onChange: () => void }) {
  const reg = r.regularizacion;
  const row = (label: string, value: number, strong = false) => (
    <tr className={strong ? 'font-medium' : undefined}>
      <td className="pr-4">{label}</td>
      <td className="text-right tabular-nums">{euros(value)}</td>
    </tr>
  );
  return (
    <div className="mt-3 space-y-4">
      {r.tabla && r.tabla.pendiente.length > 0 && (
        <div role="note" className="rounded-md bg-amber-50 p-3 text-amber-900">
          <p className="font-medium">
            Cifras de {r.tabla.year} sin norma publicada todavía ({r.tabla.norma}). Compruébalas con
            la orden de cotización del año antes de decidir:
          </p>
          <ul className="list-disc pl-5">
            {r.tabla.pendiente.map((p) => (
              <li key={p}>{p}</li>
            ))}
          </ul>
        </div>
      )}
      {r.avisos.length > 0 && (
        <ul aria-label="Avisos del RETA" className="list-disc space-y-1 pl-5 text-amber-800">
          {r.avisos.map((a) => (
            <li key={a}>{a}</li>
          ))}
        </ul>
      )}
      <section aria-label="Rendimiento computable">
        <h4 className="font-medium">Rendimiento neto computable de {r.ficha.year}</h4>
        <table className="mt-1">
          <tbody>
            {row('Retribuciones dinerarias (nóminas)', r.dinerarias)}
            {row('Retribución en especie (cuotas que paga la sociedad)', r.especie)}
            {row(
              `Dividendos de la sociedad (${euros(r.dividendos)} al ${r.ficha.control_efectivo ? pct(r.ficha.pct_dividendos) : '0 %'})`,
              r.rendimiento.dividendos_computados,
            )}
            {row('Rendimientos de fuera de la sociedad', r.ficha.otros_rendimientos)}
            {row('Gastos deducibles', -r.ficha.gastos_deducibles)}
            {row('Rendimiento neto', r.rendimiento.neto, true)}
            {row(
              `Deducción por gastos genéricos (${pct(r.rendimiento.deduccion_pct)}; ${r.dias_alta_societario} días de alta como societario${r.noventa_dias ? '' : ', menos de 90'})`,
              -r.rendimiento.deduccion,
            )}
            {row('Rendimiento computable del año', r.rendimiento.computable, true)}
            {row(`Promedio mensual (${r.ficha.meses_alta} meses)`, r.rendimiento.mensual, true)}
          </tbody>
        </table>
      </section>
      {reg && r.tabla && (
        <section aria-label="Tramo y regularización">
          <h4 className="font-medium">Tramo y regularización estimada</h4>
          <p>
            Tramo de los rendimientos: <strong>{reg.tramo_nombre}</strong> (base entre{' '}
            {euros(reg.base_min)} y {euros(reg.base_max)}).{' '}
            {r.tramo_previsto
              ? `Rendimiento previsto comunicado: tramo ${tramoNombre(r.tramo_previsto)}.`
              : 'No consta el rendimiento previsto comunicado a la TGSS.'}
          </p>
          <p>
            Base elegida {euros(r.ficha.base_elegida)} → cuota mensual {euros(r.cuota_mensual)} (
            {pct(r.tabla.tipo)}).{' '}
            {r.base_recomendada != null &&
              `Base más baja sin regularización: ${euros(r.base_recomendada)}.`}
          </p>
          <p className={reg.sentido === 'AIngresar' ? 'text-red-700' : undefined}>
            Regularización de la TGSS en {r.ficha.year + 1}:{' '}
            <strong>
              {reg.sentido === 'Ninguna'
                ? 'ninguna'
                : `${euros(reg.importe)} ${SENTIDO[reg.sentido]}`}
            </strong>{' '}
            (cotizado {euros(reg.cuota_pagada)}, correspondería {euros(reg.cuota_correcta)} en{' '}
            {reg.meses} meses). Es una estimación: la TGSS regulariza con los rendimientos que le
            comunica la AEAT.
          </p>
        </section>
      )}
      <section aria-label="Cambios de base">
        <h4 className="font-medium">Cambios de base</h4>
        <p>
          Ahora puedes pedir el cambio de base o del rendimiento previsto hasta el{' '}
          {formatDate(r.ventana_abierta.hasta)}, con efectos del{' '}
          {formatDate(r.ventana_abierta.efectos)} (Import@ss). Hasta seis cambios al año; salen
          también en el calendario fiscal si el perfil marca el RETA. Pendiente de verificar el
          artículo del reglamento de cotización.
        </p>
        <ul className="list-disc pl-5">
          {r.proximas_ventanas.map((w) => (
            <li key={w.hasta}>
              Del {formatDate(w.desde)} al {formatDate(w.hasta)} → efectos del{' '}
              {formatDate(w.efectos)}
            </li>
          ))}
        </ul>
      </section>
      <section aria-label="Cuotas pagadas">
        <h4 className="font-medium">Cuotas de {r.ficha.year}</h4>
        {r.cuotas.length === 0 ? (
          <p className="text-stone-600">Ninguna registrada.</p>
        ) : (
          <table className="mt-1">
            <thead>
              <tr className="text-left text-stone-600">
                <th className="pr-4">Fecha</th>
                <th className="pr-4 text-right">Importe</th>
                <th className="pr-4">Paga</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {r.cuotas.map((q) => (
                <tr key={q.id}>
                  <td className="pr-4">{formatDate(q.date)}</td>
                  <td className="pr-4 text-right tabular-nums">{euros(q.amount)}</td>
                  <td className="pr-4">
                    {q.pagador === 'Sociedad' ? 'Sociedad' : 'Administrador'}
                    {q.entry_id ? ' · con asiento' : ''}
                  </td>
                  <td>
                    <ConfirmAction
                      label="Borrar"
                      ariaLabel={`Borrar la cuota del ${formatDate(q.date)}`}
                      question={`¿Borrar la cuota del ${formatDate(q.date)}${q.entry_id ? ' y su asiento' : ''}?`}
                      confirmLabel="Sí, borrar"
                      onConfirm={async () => {
                        await retaCuotaDelete(q.id);
                        onChange();
                      }}
                    />
                  </td>
                </tr>
              ))}
              <tr className="font-medium">
                <td className="pr-4">Total</td>
                <td className="pr-4 text-right tabular-nums">{euros(r.cuotas_total)}</td>
                <td colSpan={2} />
              </tr>
            </tbody>
          </table>
        )}
        {r.especie > 0 && (
          <p className="mt-1">
            Ingreso a cuenta de la especie:{' '}
            {r.tipo_retencion != null
              ? `${euros(r.ingreso_a_cuenta)} (${pct(r.tipo_retencion)})`
              : 'falta saber si es el 19 o el 35 %'}
            .
          </p>
        )}
        <CuotaForm key={`${r.employee_id}-${r.cuotas.length}`} r={r} onDone={onChange} />
      </section>
      <section aria-label="Retribución y acuerdo de la junta">
        <h4 className="font-medium">Retribución del año y acuerdo de la junta</h4>
        <p>
          Pagado en {r.ficha.year}: {euros(r.retribucion_total)} (dinerario y en especie).{' '}
          {r.acuerdo.importe_maximo != null
            ? `Máximo aprobado por la junta${r.acuerdo.fecha ? ` el ${formatDate(r.acuerdo.fecha)}` : ''}: ${euros(r.acuerdo.importe_maximo)}.`
            : 'No consta el acuerdo de la junta que fija el máximo anual.'}
        </p>
      </section>
      {r.tabla && (
        <details>
          <summary className="cursor-pointer">Tabla de tramos de {r.tabla.year}</summary>
          <table className="mt-1">
            <thead>
              <tr className="text-left text-stone-600">
                <th className="pr-4">Tramo</th>
                <th className="pr-4 text-right">Rendimiento mensual hasta</th>
                <th className="pr-4 text-right">Base mínima</th>
                <th className="text-right">Base máxima</th>
              </tr>
            </thead>
            <tbody>
              {r.tabla.tramos.map((t) => (
                <tr key={tramoNombre(t)}>
                  <td className="pr-4">{tramoNombre(t)}</td>
                  <td className="pr-4 text-right tabular-nums">
                    {t.hasta != null ? euros(t.hasta) : 'más'}
                  </td>
                  <td className="pr-4 text-right tabular-nums">{euros(t.base_min)}</td>
                  <td className="text-right tabular-nums">{euros(t.base_max)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="text-stone-600">
            Base mínima de los autónomos societarios: {euros(r.tabla.base_min_societarios)}.
          </p>
        </details>
      )}
    </div>
  );
}

/**
 * Autónomo societario: el administrador o socio con control que cotiza en el RETA. Ficha con la
 * base elegida y el rendimiento previsto, rendimiento computable con lo que paga la sociedad,
 * tramo, regularización estimada, cuotas pagadas y soporte societario de la retribución.
 */
export function AutonomoSocietario({ year, staff }: { year: number; staff: Employee[] }) {
  const admins = staff.filter((s) => s.administrator && s.ss_regime === 'Autonomo');
  const [selected, setSelected] = useState<number | null>(null);
  const [r, setR] = useState<ResumenReta | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const id = selected ?? admins[0]?.id ?? null;
  const load = useCallback(() => {
    if (id == null) return;
    retaResumen(id, year)
      .then((x) => {
        setR(x);
        setMsg(null);
      })
      .catch((e) => setMsg(errorMessage(e)));
  }, [id, year]);
  useEffect(load, [load]);
  return (
    <section aria-label="Autónomo societario" className="mt-8">
      <h3 className="font-medium">Autónomo societario</h3>
      {admins.length === 0 ? (
        <p className="text-stone-600">
          Si un administrador o socio con control cotiza en autónomos (RETA), marca «Autónomos
          (RETA)» en su ficha: aquí verás su cuota, el tramo de rendimientos y la regularización que
          hará la Seguridad Social.
        </p>
      ) : (
        <>
          <p className="text-stone-600">
            Cotiza por los rendimientos reales: su sueldo y lo que la sociedad le paga en especie,
            los dividendos si tiene el control y lo que gane fuera, menos el 3 % de gastos
            genéricos. Si la base elegida no cae en el tramo, la TGSS lo regulariza al año
            siguiente.
          </p>
          {admins.length > 1 && (
            <label className="mt-2 block">
              Administrador
              <select
                className="ml-2"
                value={id ?? ''}
                onChange={(x) => setSelected(Number(x.target.value))}
              >
                {admins.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </select>
            </label>
          )}
          {r && r.employee_id === id && (
            <>
              <h4 className="mt-3 font-medium">
                Ficha del RETA de {r.name} en {year}
                {r.ficha.guardada ? '' : ' (propuesta, sin guardar)'}
              </h4>
              <FichaForm
                key={`${r.employee_id}-${year}-${JSON.stringify(r.ficha)}`}
                initial={r.ficha}
                onSaved={load}
              />
              <Resumen r={r} onChange={load} />
            </>
          )}
        </>
      )}
      {msg && (
        <p role="alert" className="text-red-700">
          {msg}
        </p>
      )}
    </section>
  );
}

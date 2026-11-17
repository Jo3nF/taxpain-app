import { ExternalLink } from 'lucide-react';
import { type ReactNode, useEffect, useState } from 'react';
import {
  type ClaseConcepto,
  type Contrato,
  type DatosFiniquito,
  type DatosNomina,
  type Employee,
  errorMessage,
  euros,
  type FichaNomina,
  type Finiquito,
  finiquitoCalcular,
  formatDate,
  type NominaCalculada,
  nominaCalcular,
  nominaFicha,
  nominaFichaSave,
  nominaRegistrar,
  openSede,
  PROGRAMA_RETENCIONES,
  parseEuros,
  type ResumenSeguros,
  segurosSociales,
  segurosSocialesPagar,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const MESES = [
  'enero',
  'febrero',
  'marzo',
  'abril',
  'mayo',
  'junio',
  'julio',
  'agosto',
  'septiembre',
  'octubre',
  'noviembre',
  'diciembre',
];

/** Céntimos (o centésimas de punto) → «1.234,56» para un campo de texto. */
function num(cents: number): string {
  return euros(cents).replace(' €', '');
}

/** Centésimas de punto → «4,7 %». */
export function pct(bp: number): string {
  return `${(bp / 100).toLocaleString('es-ES', { maximumFractionDigits: 2 })} %`;
}

/** Campo de importe o porcentaje en texto; `parseEuros` vale para los dos (12,5 → 1250). */
function Num({
  label,
  value,
  onChange,
  className,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  className?: string;
}) {
  return (
    <label className={className}>
      {label}
      <input
        className="text-right"
        inputMode="decimal"
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
    </label>
  );
}

const CLASES: [ClaseConcepto, string][] = [
  ['Salarial', 'Salarial (cotiza y tributa)'],
  ['NoSalarialSujeta', 'No salarial que cotiza (exceso de dietas…)'],
  ['NoSalarialExenta', 'No salarial exenta (dietas dentro del límite…)'],
];

let claves = 0;
const nextKey = () => ++claves;

interface FichaForm {
  naf: string;
  categoria: string;
  ccc: string;
  antiguedad: string;
  cnae: string;
  grupo: number;
  contrato: Contrato;
  jornada: string;
  salario_base: string;
  complementos: { key: number; nombre: string; importe: string; clase: ClaseConcepto }[];
  pagas_numero: number;
  pagas_importe: string;
  prorrateadas: boolean;
  at_it: string;
  at_ims: string;
  tipo_irpf: string;
}

const NUEVA: FichaForm = {
  naf: '',
  categoria: '',
  ccc: '',
  antiguedad: '',
  cnae: '',
  grupo: 7,
  contrato: 'Indefinido',
  jornada: '100',
  salario_base: '',
  complementos: [],
  pagas_numero: 2,
  pagas_importe: '',
  prorrateadas: false,
  at_it: '',
  at_ims: '',
  tipo_irpf: '',
};

function toForm(f: FichaNomina): FichaForm {
  return {
    naf: f.naf,
    categoria: f.categoria,
    ccc: f.ccc,
    antiguedad: f.antiguedad ?? '',
    cnae: f.cnae,
    grupo: f.grupo,
    contrato: f.contrato,
    jornada: num(f.jornada),
    salario_base: num(f.salario_base),
    complementos: f.complementos.map((c) => ({ ...c, key: nextKey(), importe: num(c.importe) })),
    pagas_numero: f.pagas.numero,
    pagas_importe: num(f.pagas.importe),
    prorrateadas: f.pagas.prorrateadas,
    at_it: num(f.at_it),
    at_ims: num(f.at_ims),
    tipo_irpf: num(f.tipo_irpf),
  };
}

/** Ficha del formulario → la de Rust, o el primer campo que no se entiende. */
function fromForm(employeeId: number, f: FichaForm): FichaNomina | string {
  const v = (s: string, what: string, empty = true) => {
    const n = parseEuros(s || (empty ? '0' : ''));
    if (n === null) throw new Error(what);
    return n;
  };
  try {
    return {
      employee_id: employeeId,
      naf: f.naf.trim(),
      categoria: f.categoria.trim(),
      ccc: f.ccc.trim(),
      antiguedad: f.antiguedad || null,
      cnae: f.cnae.trim(),
      grupo: f.grupo,
      contrato: f.contrato,
      jornada: v(f.jornada, 'Revisa el coeficiente de jornada', false),
      salario_base: v(f.salario_base, 'Revisa el salario base', false),
      complementos: f.complementos
        .filter((c) => c.nombre.trim() || c.importe)
        .map((c) => ({
          nombre: c.nombre.trim() || 'Complemento',
          importe: v(c.importe, `Revisa el importe de «${c.nombre}»`),
          clase: c.clase,
        })),
      pagas: {
        numero: f.pagas_numero,
        importe: v(f.pagas_importe, 'Revisa el importe de las pagas'),
        prorrateadas: f.prorrateadas,
        en_este_mes: 0,
      },
      at_it: v(f.at_it, 'Revisa la tarifa de AT y EP (IT)'),
      at_ims: v(f.at_ims, 'Revisa la tarifa de AT y EP (IMS)'),
      tipo_irpf: v(f.tipo_irpf, 'Revisa el tipo de retención'),
    };
  } catch (e) {
    return (e as Error).message;
  }
}

function Tabla({ head, children }: { head: string[]; children: ReactNode }) {
  return (
    <div className="overflow-x-auto">
      <table className="mt-1 w-full">
        <thead>
          <tr className="text-left text-stone-600">
            {head.map((h, i) => (
              <th key={h} className={`px-2${i > 0 ? ' text-right' : ''}`}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>{children}</tbody>
      </table>
    </div>
  );
}

function Resultado({ n }: { n: NominaCalculada }) {
  return (
    <div className="mt-3 space-y-3 rounded-md bg-stone-50 p-3">
      <Tabla head={['Devengos', 'Importe']}>
        {n.devengos.map((d) => (
          <tr key={d.concepto}>
            <td className="px-2">
              {d.concepto}
              {d.clase === 'NoSalarialExenta' ? ' (no cotiza ni tributa)' : ''}
            </td>
            <td className="px-2 text-right tabular-nums">{euros(d.importe)}</td>
          </tr>
        ))}
        <tr className="font-medium">
          <td className="px-2">Total devengado</td>
          <td className="px-2 text-right tabular-nums">{euros(n.total_devengado)}</td>
        </tr>
      </Tabla>
      <p>
        Base de contingencias comunes: {euros(n.remuneracion_mensual)} + prorrata de pagas{' '}
        {euros(n.prorrata_pagas)} = <strong>{euros(n.base_comunes)}</strong>
        {n.tope === 'Minimo' ? ' (base mínima del grupo)' : ''}
        {n.tope === 'Maximo' ? ' (base máxima)' : ''} · contingencias profesionales:{' '}
        {euros(n.base_profesionales)} · topes del grupo {n.grupo}: {euros(n.base_minima)} a{' '}
        {euros(n.base_maxima)}.
      </p>
      <Tabla head={['Cotización', 'Base', 'Trabajador', 'Empresa']}>
        {n.cuotas.map((c) => (
          <tr key={c.clave}>
            <td className="px-2">{c.concepto}</td>
            <td className="px-2 text-right tabular-nums">{euros(c.base)}</td>
            <td className="px-2 text-right tabular-nums">
              {euros(c.trabajador)}
              {c.tipo_trabajador ? ` (${pct(c.tipo_trabajador)})` : ''}
            </td>
            <td className="px-2 text-right tabular-nums">
              {euros(c.empresa)}
              {c.tipo_empresa ? ` (${pct(c.tipo_empresa)})` : ''}
            </td>
          </tr>
        ))}
        <tr className="font-medium">
          <td className="px-2" colSpan={2}>
            Total cotizaciones
          </td>
          <td className="px-2 text-right tabular-nums">{euros(n.total_trabajador)}</td>
          <td className="px-2 text-right tabular-nums">{euros(n.total_empresa)}</td>
        </tr>
      </Tabla>
      <p>
        Retención del IRPF: {euros(n.base_irpf)} × {pct(n.tipo_irpf)} = {euros(n.retencion_irpf)}
        {n.anticipos ? ` · anticipos ${euros(n.anticipos)}` : ''}
        {n.otras_deducciones ? ` · otras deducciones ${euros(n.otras_deducciones)}` : ''}.
      </p>
      <p className="font-medium">
        Líquido a percibir: {euros(n.liquido)} · coste para la empresa: {euros(n.coste_empresa)}
      </p>
      <p className="text-stone-600">{n.norma}</p>
      {n.avisos.length > 0 && (
        <ul className="list-disc pl-5 text-amber-800">
          {n.avisos.map((a) => (
            <li key={a}>{a}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

function hoyIso(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

function ultimoDia(year: number, month: number): string {
  const d = new Date(year, month, 0).getDate();
  return `${year}-${String(month).padStart(2, '0')}-${String(d).padStart(2, '0')}`;
}

/** Ficha de cotización del trabajador y nómina del mes calculada, con su registro. */
function CalcularNomina({
  year,
  workers,
  onRegistered,
}: {
  year: number;
  workers: Employee[];
  onRegistered: () => void;
}) {
  const [employee, setEmployee] = useState(workers[0]?.id ?? 0);
  const [f, setF] = useState<FichaForm>(NUEVA);
  const [month, setMonth] = useState(new Date().getMonth() + 1);
  const [mes, setMes] = useState({
    dias: '30',
    horas: '',
    fuerza: false,
    paga: 0,
    anticipos: '',
    otras: '',
  });
  const [nomina, setNomina] = useState<NominaCalculada | null>(null);
  const [datos, setDatos] = useState<DatosNomina | null>(null);
  const [fecha, setFecha] = useState(ultimoDia(year, month));
  const [msg, setMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const set = (p: Partial<FichaForm>) => {
    setF({ ...f, ...p });
    setNomina(null);
  };
  useEffect(() => {
    let current = true;
    setNomina(null);
    if (!employee) return;
    nominaFicha(employee)
      .then((x) => current && setF(x ? toForm(x) : NUEVA))
      .catch((e) => current && setError(errorMessage(e)));
    return () => {
      current = false;
    };
  }, [employee]);
  useEffect(() => setFecha(ultimoDia(year, month)), [year, month]);

  async function guardar() {
    const ficha = fromForm(employee, f);
    if (typeof ficha === 'string') return setError(ficha);
    try {
      await nominaFichaSave(ficha);
      setError(null);
      setMsg('Ficha guardada');
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  async function calcular() {
    setMsg(null);
    const ficha = fromForm(employee, f);
    if (typeof ficha === 'string') return setError(ficha);
    const horas = parseEuros(mes.horas || '0');
    const anticipos = parseEuros(mes.anticipos || '0');
    const otras = parseEuros(mes.otras || '0');
    const dias = Number.parseInt(mes.dias, 10);
    if (horas === null || anticipos === null || otras === null || !dias)
      return setError('Revisa los días, las horas extraordinarias y las deducciones del mes');
    const d: DatosNomina = {
      year,
      month,
      grupo: ficha.grupo,
      contrato: ficha.contrato,
      dias,
      jornada: ficha.jornada,
      salario_base: ficha.salario_base,
      complementos: ficha.complementos,
      horas_extra: horas,
      horas_extra_fuerza_mayor: mes.fuerza,
      pagas: { ...ficha.pagas, en_este_mes: ficha.pagas.prorrateadas ? 0 : mes.paga },
      at_it: ficha.at_it,
      at_ims: ficha.at_ims,
      retencion: { modo: 'Manual', tipo: ficha.tipo_irpf },
      anticipos,
      otras_deducciones: otras,
    };
    try {
      setNomina(await nominaCalcular(d));
      setDatos(d);
      setError(null);
    } catch (e) {
      setNomina(null);
      setError(errorMessage(e));
    }
  }

  const who = workers.find((w) => w.id === employee);
  return (
    <div className="text-sm">
      <div className="form-grid mt-2">
        <label className="span-2">
          Trabajador
          <select value={employee} onChange={(e) => setEmployee(Number(e.target.value))}>
            {workers.map((w) => (
              <option key={w.id} value={w.id}>
                {w.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          Grupo de cotización
          <select value={f.grupo} onChange={(e) => set({ grupo: Number(e.target.value) })}>
            {Array.from({ length: 11 }, (_, i) => i + 1).map((g) => (
              <option key={g} value={g}>
                {g}
              </option>
            ))}
          </select>
        </label>
        <label>
          Contrato
          <select
            value={f.contrato}
            onChange={(e) => set({ contrato: e.target.value as Contrato })}
          >
            <option value="Indefinido">Indefinido</option>
            <option value="Temporal">Temporal</option>
          </select>
        </label>
        <Num label="Jornada (%)" value={f.jornada} onChange={(jornada) => set({ jornada })} />
        <Num
          label="Salario base mensual"
          value={f.salario_base}
          onChange={(salario_base) => set({ salario_base })}
        />
        <label>
          Pagas extra al año
          <input
            inputMode="numeric"
            value={f.pagas_numero}
            onChange={(e) => set({ pagas_numero: Number.parseInt(e.target.value, 10) || 0 })}
          />
        </label>
        <Num
          label="Importe de cada paga"
          value={f.pagas_importe}
          onChange={(pagas_importe) => set({ pagas_importe })}
        />
        <label className="flex items-end gap-2">
          <input
            type="checkbox"
            checked={f.prorrateadas}
            onChange={(e) => set({ prorrateadas: e.target.checked })}
          />
          Pagas prorrateadas
        </label>
        <Num label="AT y EP: IT (%)" value={f.at_it} onChange={(at_it) => set({ at_it })} />
        <Num label="AT y EP: IMS (%)" value={f.at_ims} onChange={(at_ims) => set({ at_ims })} />
        <Num
          label="Retención IRPF (%)"
          value={f.tipo_irpf}
          onChange={(tipo_irpf) => set({ tipo_irpf })}
        />
        <label>
          CNAE
          <input value={f.cnae} onChange={(e) => set({ cnae: e.target.value })} />
        </label>
        <label>
          Nº de afiliación
          <input value={f.naf} onChange={(e) => set({ naf: e.target.value })} />
        </label>
        <label>
          Código de cuenta de cotización
          <input value={f.ccc} onChange={(e) => set({ ccc: e.target.value })} />
        </label>
        <label>
          Categoría profesional
          <input value={f.categoria} onChange={(e) => set({ categoria: e.target.value })} />
        </label>
        <label>
          Antigüedad
          <input
            type="date"
            value={f.antiguedad}
            onChange={(e) => set({ antiguedad: e.target.value })}
          />
        </label>
      </div>
      <p className="mt-2 text-stone-600">
        La tarifa de AT y EP es la de la actividad (CNAE) en la tabla de la disposición adicional
        4.ª de la Ley 42/2006. TaxPain no calcula la retención del IRPF: pon el tipo que te da el
        programa de la AEAT.{' '}
        <button
          type="button"
          className="inline-flex items-center gap-1 underline"
          onClick={() => openSede(PROGRAMA_RETENCIONES).catch(() => {})}
        >
          <ExternalLink size={14} aria-hidden="true" />
          Programa de cálculo de retenciones
        </button>
      </p>
      <fieldset className="mt-3">
        <legend className="font-medium">Complementos y otras percepciones del mes</legend>
        {f.complementos.map((c, i) => (
          <div key={c.key} className="form-grid">
            <label className="span-2">
              Concepto
              <input
                value={c.nombre}
                onChange={(e) => {
                  const complementos = [...f.complementos];
                  complementos[i] = { ...c, nombre: e.target.value };
                  set({ complementos });
                }}
              />
            </label>
            <Num
              label="Importe"
              value={c.importe}
              onChange={(importe) => {
                const complementos = [...f.complementos];
                complementos[i] = { ...c, importe };
                set({ complementos });
              }}
            />
            <label className="span-2">
              Clase
              <select
                value={c.clase}
                onChange={(e) => {
                  const complementos = [...f.complementos];
                  complementos[i] = { ...c, clase: e.target.value as ClaseConcepto };
                  set({ complementos });
                }}
              >
                {CLASES.map(([k, l]) => (
                  <option key={k} value={k}>
                    {l}
                  </option>
                ))}
              </select>
            </label>
            <button
              type="button"
              className="self-end underline"
              aria-label={`Quitar ${c.nombre || 'complemento'}`}
              onClick={() => set({ complementos: f.complementos.filter((_, j) => j !== i) })}
            >
              Quitar
            </button>
          </div>
        ))}
        <button
          type="button"
          className="mt-1 underline"
          onClick={() =>
            set({
              complementos: [
                ...f.complementos,
                { key: nextKey(), nombre: '', importe: '', clase: 'Salarial' },
              ],
            })
          }
        >
          Añadir complemento
        </button>
      </fieldset>
      <div className="form-actions mt-2">
        <button type="button" className="rounded btn-ghost px-3 py-1" onClick={guardar}>
          Guardar ficha
        </button>
      </div>
      <h4 className="mt-4 font-medium">Mes que se liquida</h4>
      <div className="form-grid mt-1">
        <label>
          Mes
          <select value={month} onChange={(e) => setMonth(Number(e.target.value))}>
            {MESES.map((m, i) => (
              <option key={m} value={i + 1}>
                {m} de {year}
              </option>
            ))}
          </select>
        </label>
        <label>
          Días cotizados
          <input
            inputMode="numeric"
            value={mes.dias}
            onChange={(e) => setMes({ ...mes, dias: e.target.value })}
          />
        </label>
        <Num
          label="Horas extraordinarias"
          value={mes.horas}
          onChange={(horas) => setMes({ ...mes, horas })}
        />
        <label className="flex items-end gap-2">
          <input
            type="checkbox"
            checked={mes.fuerza}
            onChange={(e) => setMes({ ...mes, fuerza: e.target.checked })}
          />
          De fuerza mayor
        </label>
        {!f.prorrateadas && (
          <label>
            Pagas extra que se cobran este mes
            <input
              inputMode="numeric"
              value={mes.paga}
              onChange={(e) => setMes({ ...mes, paga: Number.parseInt(e.target.value, 10) || 0 })}
            />
          </label>
        )}
        <Num
          label="Anticipos"
          value={mes.anticipos}
          onChange={(anticipos) => setMes({ ...mes, anticipos })}
        />
        <Num
          label="Otras deducciones"
          value={mes.otras}
          onChange={(otras) => setMes({ ...mes, otras })}
        />
      </div>
      <div className="form-actions mt-2">
        <button type="button" className="rounded btn-primary px-3 py-1" onClick={calcular}>
          Calcular nómina
        </button>
      </div>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      {msg && <p role="status">{msg}</p>}
      {nomina && datos && (
        <>
          <Resultado n={nomina} />
          <div className="form-actions mt-2 flex flex-wrap items-end gap-2">
            <label>
              Fecha del recibo
              <input type="date" value={fecha} onChange={(e) => setFecha(e.target.value)} />
            </label>
            <ConfirmAction
              label="Registrar nómina"
              danger={false}
              className="rounded btn-primary px-3 py-1"
              question={`Se registrará la nómina de ${who?.name ?? ''} de ${MESES[month - 1]} con su asiento (640 y 642 contra 4751, 476 y 465) del ${formatDate(fecha)}, y pasará al 111 y al 190.`}
              confirmLabel="Registrar"
              onConfirm={async () => {
                await nominaRegistrar({ employee_id: employee, fecha, datos });
                setNomina(null);
                setMsg('Nómina registrada con su asiento');
                onRegistered();
              }}
            />
          </div>
          {(nomina.anticipos > 0 || nomina.otras_deducciones > 0) && (
            <p className="text-stone-600">
              El asiento deja en la 465 el líquido antes de anticipos y otras deducciones:
              compénsalos con su cuenta (460…) al pagar la nómina.
            </p>
          )}
        </>
      )}
    </div>
  );
}

/** Seguros sociales del mes: resumen de cotizaciones, plazo y pago. */
function SegurosSociales({ year, refresh }: { year: number; refresh: number }) {
  const [month, setMonth] = useState(Math.max(1, new Date().getMonth()));
  const [r, setR] = useState<ResumenSeguros | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pago, setPago] = useState({ fecha: '', importe: '', cuenta: '572' });
  const [n, setN] = useState(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `refresh` y `n` recargan el resumen.
  useEffect(() => {
    let current = true;
    segurosSociales(year, month)
      .then((x) => {
        if (!current) return;
        setR(x);
        setPago({ fecha: x.vence, importe: num(x.total), cuenta: '572' });
        setError(null);
      })
      .catch((e) => current && setError(errorMessage(e)));
    return () => {
      current = false;
    };
  }, [year, month, refresh, n]);
  const importe = parseEuros(pago.importe);
  return (
    <div className="text-sm">
      <label className="mt-2 block max-w-xs">
        Mes de devengo
        <select value={month} onChange={(e) => setMonth(Number(e.target.value))}>
          {MESES.map((m, i) => (
            <option key={m} value={i + 1}>
              {m} de {year}
            </option>
          ))}
        </select>
      </label>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      {r && r.trabajadores.length === 0 && (
        <p className="mt-2 text-stone-600">Ninguna nómina con cotizaciones en ese mes.</p>
      )}
      {r && r.trabajadores.length > 0 && (
        <div className="mt-2 space-y-3">
          <Tabla head={['Concepto', 'Trabajador', 'Empresa', 'Total']}>
            {r.conceptos.map((c) => (
              <tr key={c.clave}>
                <td className="px-2">{c.concepto}</td>
                <td className="px-2 text-right tabular-nums">{euros(c.trabajador)}</td>
                <td className="px-2 text-right tabular-nums">{euros(c.empresa)}</td>
                <td className="px-2 text-right tabular-nums">{euros(c.total)}</td>
              </tr>
            ))}
            {r.trabajadores
              .filter((t) => t.sin_desglose)
              .map((t) => (
                <tr key={t.payslip_id}>
                  <td className="px-2">{t.employee} (recibo de la gestoría, sin desglose)</td>
                  <td className="px-2 text-right tabular-nums">{euros(t.trabajador)}</td>
                  <td className="px-2 text-right tabular-nums">{euros(t.empresa)}</td>
                  <td className="px-2 text-right tabular-nums">
                    {euros(t.trabajador + t.empresa)}
                  </td>
                </tr>
              ))}
            <tr className="font-medium">
              <td className="px-2">Total a ingresar</td>
              <td className="px-2 text-right tabular-nums">{euros(r.total_trabajador)}</td>
              <td className="px-2 text-right tabular-nums">{euros(r.total_empresa)}</td>
              <td className="px-2 text-right tabular-nums">{euros(r.total)}</td>
            </tr>
          </Tabla>
          <Tabla head={['Asiento del devengo (ya hecho en cada nómina)', 'Debe', 'Haber']}>
            {r.devengo.map((l) => (
              <tr key={l.cuenta}>
                <td className="px-2">
                  {l.cuenta} {l.nombre}
                </td>
                <td className="px-2 text-right tabular-nums">{l.debe ? euros(l.debe) : ''}</td>
                <td className="px-2 text-right tabular-nums">{l.haber ? euros(l.haber) : ''}</td>
              </tr>
            ))}
          </Tabla>
          <ul className="list-disc pl-5 text-stone-700">
            {r.avisos.map((a) => (
              <li key={a}>{a}</li>
            ))}
          </ul>
          {r.pago?.entry_id ? (
            <p role="status">
              Pago registrado el {formatDate(r.pago.fecha)}: {euros(r.pago.importe)} (476 contra{' '}
              {r.pago.cuenta}).
            </p>
          ) : (
            <div className="form-grid">
              <label>
                Fecha del cargo
                <input
                  type="date"
                  value={pago.fecha}
                  onChange={(e) => setPago({ ...pago, fecha: e.target.value })}
                />
              </label>
              <Num
                label="Importe del RLC"
                value={pago.importe}
                onChange={(v) => setPago({ ...pago, importe: v })}
              />
              <label>
                Cuenta de tesorería
                <input
                  value={pago.cuenta}
                  onChange={(e) => setPago({ ...pago, cuenta: e.target.value })}
                />
              </label>
              <div className="span-full form-actions">
                <ConfirmAction
                  label="Registrar el pago"
                  danger={false}
                  className="rounded btn-primary px-3 py-1"
                  disabled={importe === null || !pago.fecha}
                  question={`Se hará el asiento del pago de los seguros sociales de ${MESES[month - 1]}: 476 al debe y ${pago.cuenta} al haber por ${importe === null ? '—' : euros(importe)}, el ${pago.fecha ? formatDate(pago.fecha) : '—'}. No se paga ni se envía nada.`}
                  confirmLabel="Registrar"
                  onConfirm={async () => {
                    if (importe === null) return;
                    await segurosSocialesPagar({
                      year,
                      month,
                      fecha: pago.fecha,
                      importe,
                      cuenta: pago.cuenta,
                    });
                    setN(n + 1);
                  }}
                />
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Finiquito básico: días del mes, vacaciones, pagas y fin de contrato temporal. */
function FiniquitoForm() {
  const [d, setD] = useState({
    alta: '',
    baja: hoyIso(),
    salario: '',
    dias: '0',
    vacaciones: '30',
    disfrutadas: '0',
    paga: '',
    desde: '',
    hasta: '',
    temporal: false,
    diasAno: '12',
  });
  const [f, setF] = useState<Finiquito | null>(null);
  const [error, setError] = useState<string | null>(null);
  const int = (s: string) => Number.parseInt(s, 10) || 0;
  async function calcular() {
    const salario = parseEuros(d.salario);
    const paga = parseEuros(d.paga || '0');
    if (!d.alta || !d.baja || salario === null || paga === null)
      return setError('Revisa las fechas y los importes');
    const datos: DatosFiniquito = {
      alta: d.alta,
      baja: d.baja,
      salario_mensual: salario,
      dias_pendientes: int(d.dias),
      vacaciones_anuales: int(d.vacaciones),
      vacaciones_disfrutadas: int(d.disfrutadas),
      pagas:
        paga > 0 && d.desde && d.hasta
          ? [
              {
                nombre: 'la paga extra',
                importe: paga,
                devengo_desde: d.desde,
                devengo_hasta: d.hasta,
              },
            ]
          : [],
      fin_temporal: d.temporal,
      dias_por_ano: int(d.diasAno),
    };
    try {
      setF(await finiquitoCalcular(datos));
      setError(null);
    } catch (e) {
      setF(null);
      setError(errorMessage(e));
    }
  }
  const text = (k: keyof typeof d, label: string, type = 'text') => (
    <label>
      {label}
      <input
        type={type}
        inputMode={type === 'text' ? 'decimal' : undefined}
        value={String(d[k])}
        onChange={(e) => setD({ ...d, [k]: e.target.value })}
      />
    </label>
  );
  return (
    <div className="text-sm">
      <div className="form-grid mt-2">
        {text('alta', 'Fecha de alta', 'date')}
        {text('baja', 'Fecha de baja', 'date')}
        {text('salario', 'Salario mensual')}
        {text('dias', 'Días del mes sin pagar')}
        {text('vacaciones', 'Vacaciones al año (días)')}
        {text('disfrutadas', 'Vacaciones disfrutadas este año')}
        {text('paga', 'Paga extra pendiente (entera)')}
        {text('desde', 'Su devengo desde', 'date')}
        {text('hasta', 'Su devengo hasta', 'date')}
        <label className="flex items-end gap-2">
          <input
            type="checkbox"
            checked={d.temporal}
            onChange={(e) => setD({ ...d, temporal: e.target.checked })}
          />
          Fin de contrato temporal
        </label>
        {d.temporal && text('diasAno', 'Días de indemnización por año')}
      </div>
      <p className="mt-1 text-stone-600">
        Una paga por formulario; si hay dos pendientes, súmalas con su devengo. El fin de contrato
        temporal son doce días por año (art. 49.1.c ET), salvo los formativos y los de sustitución.
      </p>
      <div className="form-actions mt-2">
        <button type="button" className="rounded btn-primary px-3 py-1" onClick={calcular}>
          Calcular finiquito
        </button>
      </div>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      {f && (
        <div className="mt-2 rounded-md bg-stone-50 p-3">
          <Tabla head={['Finiquito', 'Importe']}>
            {f.lineas.map((l) => (
              <tr key={l.concepto}>
                <td className="px-2">
                  {l.concepto}
                  <span className="block text-stone-600">{l.detalle}</span>
                </td>
                <td className="px-2 text-right tabular-nums">{euros(l.importe)}</td>
              </tr>
            ))}
            <tr className="font-medium">
              <td className="px-2">Total bruto</td>
              <td className="px-2 text-right tabular-nums">{euros(f.total)}</td>
            </tr>
          </Tabla>
          <p className="mt-1">
            Cotiza y tributa: {euros(f.sujeto)} · indemnización (no cotiza):{' '}
            {euros(f.indemnizacion)}
          </p>
          <ul className="list-disc pl-5 text-stone-700">
            {f.avisos.map((a) => (
              <li key={a}>{a}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

/**
 * Nóminas calculadas en TaxPain para los trabajadores por cuenta ajena (no administradores):
 * ficha y cálculo del mes, seguros sociales del mes y finiquito. Nada se envía a la TGSS.
 */
export function Nominas({
  year,
  staff,
  onRegistered,
}: {
  year: number;
  staff: Employee[];
  onRegistered: () => void;
}) {
  const workers = staff.filter((s) => s.active && !s.administrator);
  const [refresh, setRefresh] = useState(0);
  return (
    <>
      <section aria-label="Calcular la nómina" className="mt-6">
        <h3 className="font-medium">Calcular la nómina</h3>
        <p className="text-stone-600">
          Para trabajadores por cuenta ajena del Régimen General: bases con los topes del grupo,
          cuotas del trabajador y de la empresa y líquido, con las tablas de cotización del año.
          Comprueba el resultado con la liquidación de la TGSS: TaxPain no se conecta al Sistema RED
          ni a SILTRA.
        </p>
        {workers.length === 0 ? (
          <p className="mt-1 text-stone-600">
            Da de alta un empleado que no sea administrador para calcular su nómina.
          </p>
        ) : (
          <CalcularNomina
            year={year}
            workers={workers}
            onRegistered={() => {
              setRefresh(refresh + 1);
              onRegistered();
            }}
          />
        )}
      </section>
      <section aria-label="Seguros sociales del mes" className="mt-6">
        <h3 className="font-medium">Seguros sociales del mes</h3>
        <SegurosSociales year={year} refresh={refresh} />
      </section>
      <section aria-label="Finiquito" className="mt-6">
        <h3 className="font-medium">Finiquito</h3>
        <FiniquitoForm />
      </section>
    </>
  );
}

import { type FormEvent, useCallback, useEffect, useRef, useState } from 'react';
import { AutonomoSocietario } from './AutonomoSocietario';
import {
  adminReceiptPdf,
  type DirectorSs,
  type DirectorTurnover,
  directorSubkey,
  directorTurnover,
  type Employee,
  employeeSave,
  employees,
  errorMessage,
  euros,
  formatDate,
  nominaReciboPdf,
  nominasCalculadas,
  parseEuros,
  payslipDelete,
  payslipRegister,
  payslips,
  pickFolder,
  type StoredPayslip,
  TURNOVER_SOURCE,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { ProvinceCodeSelect } from './Controls';
import { Nominas } from './Nominas';
import { Travel } from './Travel';

const EMPTY: Employee = {
  id: 0,
  nif: '',
  name: '',
  province: '',
  birth_year: 0,
  family: 3,
  spouse_nif: null,
  disability: 0,
  contract: 1,
  mobility: false,
  children_under3: 0,
  children_rest: 0,
  children_whole: false,
  active: true,
  administrator: false,
  ss_regime: 'Ninguno',
};

const SS: [DirectorSs, string][] = [
  ['Autonomo', 'Autónomos (RETA)'],
  ['Asimilado', 'Régimen general como asimilado'],
  ['Ninguno', 'Sin alta por el cargo'],
];

/** Ficha del empleado con los datos que pide la clave A del 190 (o la E, si es administrador). */
function EmployeeForm({ initial, onSaved }: { initial: Employee; onSaved: () => void }) {
  const [e, setE] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<Employee>) => setE({ ...e, ...patch });
  const num = (v: string) => Number.parseInt(v, 10) || 0;
  async function submit(ev: FormEvent) {
    ev.preventDefault();
    try {
      await employeeSave(e);
      onSaved();
    } catch (err) {
      setError(errorMessage(err));
    }
  }
  return (
    <form
      onSubmit={submit}
      className="form-grid mt-3 rounded-md border border-stone-200 p-4 text-sm"
    >
      <label>
        NIF
        <input value={e.nif} onChange={(x) => set({ nif: x.target.value })} />
      </label>
      <label className="span-2">
        Apellidos y nombre
        <input value={e.name} onChange={(x) => set({ name: x.target.value })} />
      </label>
      <label className="span-2">
        Provincia
        <ProvinceCodeSelect value={e.province} onChange={(province) => set({ province })} />
      </label>
      <label className="span-full flex gap-2">
        <input
          type="checkbox"
          checked={e.administrator ?? false}
          onChange={(x) => set({ administrator: x.target.checked })}
        />
        Consejero o administrador de la sociedad (clave E del 190)
      </label>
      {e.administrator && (
        <>
          <label className="span-2">
            Seguridad Social por el cargo
            <select
              value={e.ss_regime ?? 'Ninguno'}
              onChange={(x) => set({ ss_regime: x.target.value as DirectorSs })}
            >
              {SS.map(([k, l]) => (
                <option key={k} value={k}>
                  {l}
                </option>
              ))}
            </select>
          </label>
          <p className="span-full text-stone-600">
            Retención fija del 35 %, o del 19 % si la cifra de negocios del último ejercicio cerrado
            fue inferior a 100.000 € (art. 101.2 LIRPF). Sólo la retribución del cargo (incluidas
            las funciones de dirección y gerencia); otro trabajo para la sociedad va como empleado
            (clave A) o como actividad profesional.
          </p>
        </>
      )}
      {!e.administrator && (
        <>
          <label>
            Año de nacimiento
            <input
              inputMode="numeric"
              value={e.birth_year || ''}
              onChange={(x) => set({ birth_year: num(x.target.value) })}
            />
          </label>
          <label className="span-2">
            Situación familiar
            <select value={e.family} onChange={(x) => set({ family: num(x.target.value) })}>
              <option value={1}>1 · Monoparental con hijos</option>
              <option value={2}>2 · Cónyuge a cargo (ingresos ≤ 1.500 €)</option>
              <option value={3}>3 · Otras situaciones</option>
            </select>
          </label>
          {e.family === 2 && (
            <label>
              NIF del cónyuge
              <input
                value={e.spouse_nif ?? ''}
                onChange={(x) => set({ spouse_nif: x.target.value || null })}
              />
            </label>
          )}
          <label>
            Contrato
            <select value={e.contract} onChange={(x) => set({ contract: num(x.target.value) })}>
              <option value={1}>1 · General</option>
              <option value={2}>2 · Inferior al año</option>
              <option value={3}>3 · Relación especial</option>
              <option value={4}>4 · Jornaleros</option>
            </select>
          </label>
          <label className="span-2">
            Discapacidad
            <select value={e.disability} onChange={(x) => set({ disability: num(x.target.value) })}>
              <option value={0}>Sin discapacidad</option>
              <option value={1}>Entre el 33 y el 65 %</option>
              <option value={2}>Entre el 33 y el 65 % con movilidad reducida</option>
              <option value={3}>Igual o superior al 65 %</option>
            </select>
          </label>
          <label>
            Hijos menores de 3 años
            <input
              inputMode="numeric"
              value={e.children_under3}
              onChange={(x) => set({ children_under3: num(x.target.value) })}
            />
          </label>
          <label>
            Resto de hijos
            <input
              inputMode="numeric"
              value={e.children_rest}
              onChange={(x) => set({ children_rest: num(x.target.value) })}
            />
          </label>
          <div className="span-full flex flex-wrap gap-x-6 gap-y-2">
            <label className="flex items-start gap-2">
              <input
                type="checkbox"
                checked={e.children_whole}
                onChange={(x) => set({ children_whole: x.target.checked })}
              />
              Los hijos le computan por entero
            </label>
            <label className="flex items-start gap-2">
              <input
                type="checkbox"
                checked={e.mobility}
                onChange={(x) => set({ mobility: x.target.checked })}
              />
              Movilidad geográfica
            </label>
          </div>
        </>
      )}
      <div className="span-full form-actions">
        <button type="submit" className="rounded btn-primary px-3 py-1">
          Guardar empleado
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

/** Retención de un administrador: 19 % o 35 % del bruto, al céntimo. */
export function directorIrpf(gross: number, small: boolean): number {
  return Math.round((gross * (small ? 19 : 35)) / 100);
}

function PayslipForm({ staff, onDone }: { staff: Employee[]; onDone: () => void }) {
  const [employee, setEmployee] = useState(staff.find((s) => s.active)?.id ?? 0);
  const [date, setDate] = useState('');
  const [v, setV] = useState({ gross: '', irpf: '', ss_employee: '', ss_company: '' });
  const [error, setError] = useState<string | null>(null);
  const [turnover, setTurnover] = useState<DirectorTurnover | null>(null);
  /** Consultando la cifra de negocios: hasta que llegue no se sabe si es el 19 % o el 35 %. */
  const [turnoverPending, setTurnoverPending] = useState(false);
  const [manualSmall, setManualSmall] = useState(false);
  // Sin dobles envíos: dos clics (o clic + Intro) no registran la nómina y su asiento dos veces.
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const amounts = [v.gross, v.irpf, v.ss_employee, v.ss_company].map((x) => parseEuros(x || '0'));
  const ok = amounts.every((a) => a !== null);
  const [gross = 0, irpf = 0, sse = 0, ssc = 0] = amounts.map((a) => a ?? 0);
  const who = staff.find((s) => s.id === employee);
  const admin = who?.administrator ?? false;
  useEffect(() => {
    setTurnover(null);
    setTurnoverPending(false);
    if (!admin || !date) return;
    let current = true;
    setTurnoverPending(true);
    directorTurnover(date)
      .then((t) => current && setTurnover(t))
      .catch(() => current && setTurnover(null))
      .finally(() => current && setTurnoverPending(false));
    return () => {
      current = false;
    };
  }, [admin, date]);
  const small = turnover?.small ?? manualSmall;
  const expected = admin && !turnoverPending ? directorIrpf(gross, small) : null;
  async function submit(ev: FormEvent) {
    ev.preventDefault();
    if (busyRef.current) return;
    if (!ok || !date) return setError('Revisa la fecha y los importes');
    // Sin la respuesta, «no marcado» se enviaría como más de 100.000 € (35 %, E.01/E.04).
    if (admin && turnoverPending) return;
    busyRef.current = true;
    setBusy(true);
    try {
      await payslipRegister({
        employee_id: employee,
        date,
        gross,
        irpf,
        ss_employee: sse,
        ss_company: ssc,
        small_entity: admin && turnover?.small == null ? manualSmall : null,
      });
      setV({ gross: '', irpf: '', ss_employee: '', ss_company: '' });
      setError(null);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }
  const field = (k: keyof typeof v, label: string) => (
    <label>
      {label}
      <input
        className="text-right"
        inputMode="decimal"
        value={v[k]}
        onChange={(x) => setV({ ...v, [k]: x.target.value })}
      />
    </label>
  );
  return (
    <form onSubmit={submit} className="form-grid mt-2 text-sm">
      <label className="span-2">
        Perceptor
        <select value={employee} onChange={(x) => setEmployee(Number(x.target.value))}>
          {staff
            .filter((s) => s.active)
            .map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
        </select>
      </label>
      <label>
        Fecha
        <input type="date" value={date} onChange={(x) => setDate(x.target.value)} />
      </label>
      {field('gross', 'Bruto')}
      {field('irpf', 'Retención IRPF')}
      {field('ss_employee', admin ? 'SS descontada' : 'SS trabajador')}
      {field('ss_company', 'SS empresa')}
      <span className="flex min-h-9 items-center">
        Líquido: {ok ? euros(gross - irpf - sse) : '—'}
      </span>
      {admin && date && (
        <div className="span-full space-y-1 rounded-md bg-stone-50 p-3">
          {turnoverPending ? (
            <p role="status">Consultando la cifra de negocios del último ejercicio cerrado…</p>
          ) : turnover?.small != null ? (
            <p>
              Cifra de negocios de {turnover.year} ({TURNOVER_SOURCE[turnover.source]}):{' '}
              {turnover.turnover != null ? euros(turnover.turnover) : '—'} → retención del{' '}
              {small ? '19' : '35'} % y subclave E.0
              {directorSubkey(who?.ss_regime ?? 'Ninguno', small)}.
            </p>
          ) : (
            <label className="flex items-start gap-2">
              <input
                type="checkbox"
                checked={manualSmall}
                onChange={(x) => setManualSmall(x.target.checked)}
              />
              <span>
                La cifra de negocios del último ejercicio cerrado antes del pago
                {turnover?.turnover != null
                  ? ` (en TaxPain constan ${euros(turnover.turnover)} de ${turnover.year}, pero no cubren todo el ejercicio: si empezó a mitad de año, elévala al año)`
                  : ' (no está en TaxPain: ni el 200 ni las ventas en la contabilidad)'}{' '}
                fue inferior a 100.000 €. En el primer ejercicio de una sociedad nueva, 19 % (DGT
                V1727-15).
              </span>
            </label>
          )}
          {expected !== null && ok && expected !== irpf && (
            <p className="flex flex-wrap items-center gap-2 text-amber-800">
              Le corresponde una retención de {euros(expected)}.
              <button
                type="button"
                className="underline"
                onClick={() => setV({ ...v, irpf: (expected / 100).toFixed(2).replace('.', ',') })}
              >
                Ponerla
              </button>
            </p>
          )}
        </div>
      )}
      <div className="span-full form-actions">
        <button
          type="submit"
          className="rounded btn-primary px-3 py-1 disabled:opacity-50"
          disabled={busy || (admin && turnoverPending)}
        >
          Registrar nómina
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

/**
 * Nóminas: fichas de empleado y recibos de salarios (los importes del recibo de la gestoría
 * laboral). Cada recibo genera su asiento y alimenta el 111 y la clave A del 190.
 */
export function Payroll({ year }: { year: number }) {
  const [staff, setStaff] = useState<Employee[]>([]);
  const [slips, setSlips] = useState<StoredPayslip[]>([]);
  const [editing, setEditing] = useState<Employee | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [calculated, setCalculated] = useState<Set<number>>(new Set());
  const load = useCallback(() => {
    Promise.all([employees(), payslips(year), nominasCalculadas(year).catch(() => [])])
      .then(([e, p, c]) => {
        setStaff(e);
        setSlips(p);
        setCalculated(new Set(c));
      })
      .catch((e) => setMsg(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);
  const total = (k: 'gross' | 'irpf' | 'ss_employee' | 'ss_company' | 'net') =>
    slips.reduce((s, x) => s + x[k], 0);

  return (
    <div className="mt-4 text-sm">
      <p className="text-stone-600">
        Registra cada recibo con los importes que te pasa la gestoría laboral, o calcúlalo más abajo
        para un trabajador por cuenta ajena: TaxPain hace el asiento (640 y 642 contra 4751, 476 y
        465) y lo lleva al 111 y al 190 (clave A, o la E si es la retribución de un administrador).
      </p>
      <section aria-label="Empleados" className="mt-6">
        <h3 className="font-medium">Empleados</h3>
        <ul className="mt-1">
          {staff.map((s) => (
            <li key={s.id} className="flex gap-2">
              <span>
                {s.name} · {s.nif}
                {s.administrator
                  ? ' · administrador'
                  : s.birth_year
                    ? ''
                    : ' · falta el año de nacimiento'}
              </span>
              <button type="button" className="underline" onClick={() => setEditing(s)}>
                Editar
              </button>
            </li>
          ))}
        </ul>
        {editing ? (
          <EmployeeForm
            key={editing.id}
            initial={editing}
            onSaved={() => {
              setEditing(null);
              load();
            }}
          />
        ) : (
          <button type="button" className="mt-1 underline" onClick={() => setEditing(EMPTY)}>
            Añadir empleado
          </button>
        )}
      </section>
      {staff.length > 0 && (
        <section aria-label="Nueva nómina" className="mt-6">
          <h3 className="font-medium">Nueva nómina</h3>
          <PayslipForm staff={staff} onDone={load} />
        </section>
      )}
      <section aria-label={`Nóminas de ${year}`} className="mt-6">
        <h3 className="font-medium">Nóminas de {year}</h3>
        {slips.length === 0 ? (
          <p className="text-stone-600">Ninguna.</p>
        ) : (
          <table className="mt-1 w-full">
            <thead>
              <tr className="text-left text-stone-600">
                <th className="px-2">Fecha</th>
                <th className="px-2">Empleado</th>
                <th className="px-2 text-right">Bruto</th>
                <th className="px-2 text-right">IRPF</th>
                <th className="px-2 text-right">SS trabajador</th>
                <th className="px-2 text-right">SS empresa</th>
                <th className="px-2 text-right">Líquido</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {slips.map((p) => (
                <tr key={p.id}>
                  <td className="px-2">{formatDate(p.date)}</td>
                  <td className="px-2">
                    {p.employee}
                    {p.director_subkey ? ` · E.0${p.director_subkey}` : ''}
                  </td>
                  <td className="px-2 text-right tabular-nums">{euros(p.gross)}</td>
                  <td className="px-2 text-right tabular-nums">{euros(p.irpf)}</td>
                  <td className="px-2 text-right tabular-nums">{euros(p.ss_employee)}</td>
                  <td className="px-2 text-right tabular-nums">{euros(p.ss_company)}</td>
                  <td className="px-2 text-right tabular-nums">{euros(p.net)}</td>
                  <td className="px-2">
                    {p.director_subkey ? (
                      <button
                        type="button"
                        className="mr-2 underline"
                        aria-label={`Recibo de retribución de ${p.employee} del ${formatDate(p.date)} (PDF)`}
                        onClick={async () => {
                          const dir = await pickFolder();
                          if (!dir) return;
                          const place = new Date().toLocaleDateString('es-ES', {
                            day: 'numeric',
                            month: 'long',
                            year: 'numeric',
                          });
                          try {
                            setSaved(
                              `Recibo guardado en ${await adminReceiptPdf(p.id, place, dir)}`,
                            );
                          } catch (e) {
                            setMsg(errorMessage(e));
                          }
                        }}
                      >
                        Recibo (PDF)…
                      </button>
                    ) : null}
                    {calculated.has(p.id) ? (
                      <button
                        type="button"
                        className="mr-2 underline"
                        aria-label={`Recibo de salarios de ${p.employee} del ${formatDate(p.date)} (PDF)`}
                        onClick={async () => {
                          const dir = await pickFolder();
                          if (!dir) return;
                          try {
                            setSaved(`Recibo guardado en ${await nominaReciboPdf(p.id, dir)}`);
                          } catch (e) {
                            setMsg(errorMessage(e));
                          }
                        }}
                      >
                        Recibo de salarios (PDF)…
                      </button>
                    ) : null}
                    <ConfirmAction
                      label="Borrar"
                      question={`¿Borrar la nómina de ${p.employee} del ${formatDate(p.date)} y su asiento?`}
                      confirmLabel="Sí, borrar"
                      onConfirm={async () => {
                        try {
                          await payslipDelete(p.id);
                          load();
                        } catch (e) {
                          setMsg(errorMessage(e));
                        }
                      }}
                    />
                  </td>
                </tr>
              ))}
              <tr className="font-medium">
                <td className="px-2" colSpan={2}>
                  Total
                </td>
                <td className="px-2 text-right tabular-nums">{euros(total('gross'))}</td>
                <td className="px-2 text-right tabular-nums">{euros(total('irpf'))}</td>
                <td className="px-2 text-right tabular-nums">{euros(total('ss_employee'))}</td>
                <td className="px-2 text-right tabular-nums">{euros(total('ss_company'))}</td>
                <td className="px-2 text-right tabular-nums">{euros(total('net'))}</td>
                <td />
              </tr>
            </tbody>
          </table>
        )}
      </section>
      {saved && <p role="status">{saved}</p>}
      <Nominas year={year} staff={staff} onRegistered={load} />
      <AutonomoSocietario year={year} staff={staff} />
      <Travel year={year} staff={staff} />
      {msg && (
        <p role="alert" className="text-red-700">
          {msg}
        </p>
      )}
    </div>
  );
}

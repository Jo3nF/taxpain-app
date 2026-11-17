import { type FormEvent, useCallback, useEffect, useRef, useState } from 'react';
import {
  type Employee,
  errorMessage,
  euros,
  formatDate,
  parseEuros,
  type StoredTrip,
  type Trip,
  type TripCalc,
  travelDelete,
  travelList,
  travelPreview,
  travelRegister,
} from './api';
import { ConfirmAction } from './ConfirmAction';

/** Céntimos exceptuados por km (Orden HFP/792/2023: 0,26 € desde el 17-07-2023). */
export function kmRateCents(date: string): number {
  return date && date < '2023-07-17' ? 19 : 26;
}

type Amounts = {
  km_amount: string;
  meals: string;
  lodging: string;
  tolls_parking: string;
  transport: string;
};

const NO_AMOUNTS: Amounts = {
  km_amount: '',
  meals: '',
  lodging: '',
  tolls_parking: '',
  transport: '',
};

/**
 * Gastos de viaje (dietas y kilometraje): lo exceptuado por el art. 9 RIRPF se contabiliza en la
 * 629 y va en la clave L.01 del 190; el exceso es retribución y ha de ir en la nómina.
 */
export function Travel({ year, staff }: { year: number; staff: Employee[] }) {
  const [trips, setTrips] = useState<StoredTrip[]>([]);
  const [person, setPerson] = useState<number>(0);
  const [nif, setNif] = useState('');
  const [name, setName] = useState('');
  const [labor, setLabor] = useState(true);
  const [date, setDate] = useState('');
  const [route, setRoute] = useState({ origin: '', destination: '', reason: '' });
  const [abroad, setAbroad] = useState(false);
  const [overnight, setOvernight] = useState(false);
  const [days, setDays] = useState('1');
  const [km, setKm] = useState('');
  const [a, setA] = useState<Amounts>(NO_AMOUNTS);
  const [counter, setCounter] = useState('465');
  const [calc, setCalc] = useState<TripCalc | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Sin dobles envíos: dos clics no registran el viaje y su asiento dos veces.
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const load = useCallback(() => {
    travelList(year)
      .then(setTrips)
      .catch((e) => setError(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);

  const card = staff.find((s) => s.id === person);
  const money = Object.fromEntries(
    Object.entries(a).map(([k, v]) => [k, parseEuros(v || '0')]),
  ) as Record<keyof Amounts, number | null>;
  const valid = Object.values(money).every((m) => m !== null);
  const trip = (): Trip => ({
    date,
    employee_id: card ? card.id : null,
    nif: card ? card.nif : nif.trim(),
    name: card ? card.name : name.trim(),
    labor,
    origin: route.origin,
    destination: route.destination,
    reason: route.reason,
    abroad,
    overnight,
    days: Number.parseInt(days, 10) || 0,
    km: Number.parseInt(km, 10) || 0,
    km_amount: money.km_amount ?? 0,
    meals: money.meals ?? 0,
    lodging: money.lodging ?? 0,
    tolls_parking: money.tolls_parking ?? 0,
    transport: money.transport ?? 0,
    counter_account: counter.trim(),
  });
  const changed = () => setCalc(null);
  const amount = (k: keyof Amounts, label: string) => (
    <label>
      {label}
      <input
        className="text-right"
        inputMode="decimal"
        value={a[k]}
        onChange={(e) => {
          setA({ ...a, [k]: e.target.value });
          changed();
        }}
      />
    </label>
  );
  async function submit(ev: FormEvent) {
    ev.preventDefault();
    if (busyRef.current) return;
    if (!valid || !date) return setError('Revisa la fecha y los importes.');
    busyRef.current = true;
    setBusy(true);
    try {
      await travelRegister(trip());
      setError(null);
      setMsg('Viaje registrado.');
      setA(NO_AMOUNTS);
      setKm('');
      setRoute({ origin: route.origin, destination: '', reason: '' });
      setCalc(null);
      load();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }

  return (
    <section aria-label="Gastos de viaje" className="mt-6">
      <h3 className="font-medium">Gastos de viaje (dietas y kilometraje)</h3>
      <p className="mt-1 text-stone-600">
        Exento (art. 9 RIRPF): 0,26 €/km más peajes y aparcamiento justificados; manutención de
        26,67 € al día sin pernoctar o 53,34 € pernoctando (48,08 y 91,35 € en el extranjero); hotel
        y billetes, lo justificado. Sólo fuera del municipio del trabajo y de la residencia, y no
        más de nueve meses seguidos en el mismo sitio. Lo exento va a la 629 y a la clave L.01 del
        190; el exceso es retribución y ha de ir en la nómina.
      </p>
      <form onSubmit={submit} className="mt-3 space-y-4">
        <div className="form-grid">
          <label>
            Quién viaja
            <select
              value={person}
              onChange={(e) => {
                const id = Number(e.target.value);
                setPerson(id);
                const c = staff.find((s) => s.id === id);
                setLabor(!c?.administrator);
                setCounter(c?.administrator ? '551' : '465');
                changed();
              }}
            >
              <option value={0}>Otra persona</option>
              {staff.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                  {s.administrator ? ' (administrador)' : ''}
                </option>
              ))}
            </select>
          </label>
          {!card && (
            <>
              <label>
                NIF
                <input
                  value={nif}
                  onChange={(e) => {
                    setNif(e.target.value);
                    changed();
                  }}
                />
              </label>
              <label className="span-2">
                Apellidos y nombre
                <input
                  value={name}
                  onChange={(e) => {
                    setName(e.target.value);
                    changed();
                  }}
                />
              </label>
            </>
          )}
          <label className="flex gap-2">
            <input
              type="checkbox"
              checked={labor}
              onChange={(e) => {
                setLabor(e.target.checked);
                changed();
              }}
            />
            Con contrato laboral
          </label>
        </div>
        <div className="form-grid">
          <label>
            Fecha del viaje
            <input
              type="date"
              value={date}
              onChange={(e) => {
                setDate(e.target.value);
                changed();
              }}
            />
          </label>
          {(['origin', 'destination', 'reason'] as const).map((k) => (
            <label key={k} className={k === 'reason' ? 'span-2' : undefined}>
              {{ origin: 'Origen', destination: 'Destino', reason: 'Motivo' }[k]}
              <input
                value={route[k]}
                onChange={(e) => {
                  setRoute({ ...route, [k]: e.target.value });
                  changed();
                }}
              />
            </label>
          ))}
          <div className="span-full flex flex-wrap gap-x-6 gap-y-2">
            <label className="flex items-start gap-2">
              <input
                type="checkbox"
                checked={abroad}
                onChange={(e) => {
                  setAbroad(e.target.checked);
                  changed();
                }}
              />
              Extranjero
            </label>
            <label className="flex items-start gap-2">
              <input
                type="checkbox"
                checked={overnight}
                onChange={(e) => {
                  setOvernight(e.target.checked);
                  changed();
                }}
              />
              Pernocta
            </label>
          </div>
        </div>
        <div className="form-grid">
          <label>
            Días de dieta
            <input
              className="text-right"
              inputMode="numeric"
              value={days}
              onChange={(e) => {
                setDays(e.target.value);
                changed();
              }}
            />
          </label>
          {amount('meals', 'Dieta de manutención')}
          <label>
            Kilómetros
            <input
              className="text-right"
              inputMode="numeric"
              value={km}
              onChange={(e) => {
                setKm(e.target.value);
                changed();
              }}
            />
          </label>
          {amount('km_amount', 'Kilometraje pagado')}
          {km && (
            <div className="flex min-h-9 items-center">
              <button
                type="button"
                className="text-link underline"
                onClick={() => {
                  const cents = (Number.parseInt(km, 10) || 0) * kmRateCents(date);
                  setA({ ...a, km_amount: (cents / 100).toFixed(2).replace('.', ',') });
                  changed();
                }}
              >
                A 0,{kmRateCents(date)} €/km
              </button>
            </div>
          )}
          {amount('lodging', 'Hotel (justificado)')}
          {amount('tolls_parking', 'Peajes y aparcamiento')}
          {amount('transport', 'Billetes (justificados)')}
          <label>
            Contrapartida
            <input
              className="font-mono"
              inputMode="numeric"
              value={counter}
              onChange={(e) => {
                setCounter(e.target.value);
                changed();
              }}
            />
          </label>
        </div>
        <div className="form-actions">
          <button
            type="button"
            className="rounded border border-stone-300 px-3 py-1"
            disabled={!valid || !date}
            onClick={() => {
              setMsg(null);
              setError(null);
              travelPreview(trip())
                .then(setCalc)
                .catch((e) => {
                  setCalc(null);
                  setError(errorMessage(e));
                });
            }}
          >
            Calcular
          </button>
          <button type="submit" className="rounded btn-primary px-3 py-1" disabled={busy || !calc}>
            Registrar viaje
          </button>
        </div>
      </form>
      <p className="mt-2 text-stone-600">
        Contrapartida: 551 si se le debe a un administrador, 465 a un empleado, o 57x si ya se pagó.
        Un administrador sin contrato laboral no tiene dietas exentas (DGT V1796-25): sólo se le
        reembolsan, sin tributar, los gastos justificados pagados por cuenta de la sociedad.
      </p>
      {calc && (
        <div className="mt-2 rounded-md bg-stone-50 p-3">
          <p>
            Total {euros(calc.total)}: exento {euros(calc.exempt)}
            {calc.not_income > 0 && `, gastos de la sociedad ${euros(calc.not_income)}`},
            retribución {euros(calc.taxable)}. Se contabiliza {euros(calc.posted)} en la 629.
          </p>
          {calc.warnings.length > 0 && (
            <ul className="mt-2 list-disc space-y-1 pl-5 text-amber-800">
              {calc.warnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          )}
        </div>
      )}
      {trips.length > 0 && (
        <table className="mt-3 w-full">
          <caption className="text-left font-medium">Viajes de {year}</caption>
          <thead>
            <tr className="text-left text-stone-600">
              <th className="px-2">Fecha</th>
              <th className="px-2">Quién</th>
              <th className="px-2">Destino y motivo</th>
              <th className="px-2 text-right">Exento</th>
              <th className="px-2 text-right">Retribución</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {trips.map((t) => (
              <tr key={t.id}>
                <td className="px-2">{formatDate(t.trip.date)}</td>
                <td className="px-2">{t.trip.name}</td>
                <td className="px-2">
                  {t.trip.destination} · {t.trip.reason}
                </td>
                <td className="px-2 text-right tabular-nums">{euros(t.exempt)}</td>
                <td className="px-2 text-right tabular-nums">{euros(t.taxable)}</td>
                <td className="px-2">
                  <ConfirmAction
                    label="Borrar"
                    question={`¿Borrar el viaje del ${formatDate(t.trip.date)} a ${t.trip.destination} y su asiento?`}
                    confirmLabel="Sí, borrar"
                    onConfirm={async () => {
                      try {
                        await travelDelete(t.id);
                        load();
                      } catch (e) {
                        setError(errorMessage(e));
                      }
                    }}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      {msg && <p role="status">{msg}</p>}
    </section>
  );
}

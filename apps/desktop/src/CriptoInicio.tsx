import { useRef, useState } from 'react';
import {
  type CriptoPlatform,
  type CriptoPropuestaInicio,
  criptoDeleteInicio,
  criptoPropuestaInicio,
  criptoQty,
  criptoSaveInicio,
  errorMessage,
  euros,
  type CriptoInicio as Inicio,
  parseCriptoQty,
  parseEuros,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const plain = (c: number) => (c / 100).toFixed(2).replace('.', ',');
const fecha = (iso: string) => {
  const [y, m, d] = iso.split('-');
  return `${d}/${m}/${y}`;
};
const cell = 'px-2 py-1 text-right tabular-nums';
/**
 * Cantidad para editarla: coma decimal y sin puntos de miles («5000,25»), porque al leerla un
 * punto sin coma detrás es el decimal («5.000» serían 5 unidades).
 */
const editable = (q: string) => q.replace('.', ',');
/** Hoy en local (AAAA-MM-DD). */
const hoyIso = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
};
const ayerIso = () => {
  const d = new Date();
  d.setDate(d.getDate() - 1);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
};

type Fila = { ticker: string; platform: string; cantidad: string; coste: string };
type Form = { fecha: string; cuenta: string; origen: string; nota: string; filas: Fila[] };

const filaVacia = (): Fila => ({ ticker: '', platform: '', cantidad: '', coste: '' });

function toForm(i: Inicio): Form {
  return {
    fecha: i.fecha,
    cuenta: i.cuenta,
    origen: i.origen,
    nota: i.nota,
    filas: i.posiciones.map((p) => ({
      ticker: p.ticker,
      platform: p.platform_id ? String(p.platform_id) : '',
      cantidad: editable(p.cantidad),
      coste: plain(p.coste),
    })),
  };
}

/** Lo que hay que corregir y la etiqueta del campo (para llevar el foco a él). */
type Fallo = { error: string; campo: string };

/** El formulario como punto de partida, o lo que hay que corregir. */
function fromForm(f: Form, hoy: string): Inicio | Fallo {
  const fecha = 'Fecha de corte';
  if (!/^\d{4}-\d{2}-\d{2}$/.test(f.fecha))
    return { error: 'Pon la fecha de corte.', campo: fecha };
  if (f.fecha >= hoy)
    return {
      error:
        'La fecha de corte ha de ser anterior a hoy: es el cierre del último ejercicio cerrado y declarado.',
      campo: fecha,
    };
  const posiciones = [];
  const vistas = new Set<string>();
  for (const [i, x] of f.filas.entries()) {
    if (!x.ticker.trim() && !x.cantidad.trim() && !x.coste.trim()) continue;
    const ticker = x.ticker.trim().toUpperCase();
    if (!ticker) return { error: `Fila ${i + 1}: falta la moneda.`, campo: `Moneda ${i + 1}` };
    const cantidad = parseCriptoQty(x.cantidad);
    if (!cantidad || cantidad.startsWith('-'))
      return {
        error: `${ticker}: revisa la cantidad (mayor que cero, con coma decimal: «1,5»).`,
        campo: `Cantidad ${i + 1}`,
      };
    const coste = parseEuros(x.coste || '0');
    if (coste === null || coste < 0)
      return {
        error: `${ticker}: revisa el coste (en euros, sin negativos: «1.234,56»).`,
        campo: `Coste ${i + 1}`,
      };
    const clave = `${ticker}|${x.platform}`;
    if (vistas.has(clave))
      return {
        error: `${ticker} está dos veces en la misma plataforma: júntalas en una fila.`,
        campo: `Moneda ${i + 1}`,
      };
    vistas.add(clave);
    posiciones.push({
      ticker,
      platform_id: x.platform ? Number(x.platform) : null,
      cantidad,
      coste,
    });
  }
  return { fecha: f.fecha, cuenta: f.cuenta.trim(), origen: f.origen, nota: f.nota, posiciones };
}

/** Coste por unidad para enseñarlo (no se guarda). */
function porUnidad(cantidad: string, coste: string): string {
  const q = parseCriptoQty(cantidad);
  const c = parseEuros(coste || '0');
  if (!q || c === null || Number(q) <= 0) return '—';
  return euros(Math.round(c / Number(q)));
}

/**
 * Punto de partida de Criptomonedas: lo que calculó la gestoría a una fecha de corte (cantidades y
 * coste pendiente). Desde el día siguiente el cálculo sigue sobre él; lo de antes es historial.
 */
export function CriptoInicio({
  inicio,
  platforms,
  year,
  huecos,
  onSaved,
}: {
  inicio: Inicio | null;
  platforms: CriptoPlatform[];
  year: number;
  /** Ventas o traspasos de más de lo que había (sin punto de partida). */
  huecos: number;
  onSaved: () => void;
}) {
  const [form, setForm] = useState<Form | null>(null);
  const [propuesta, setPropuesta] = useState<CriptoPropuestaInicio | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  /** Avisos al guardar o quitar (asientos que hay que rehacer). */
  const [avisos, setAvisos] = useState<string[]>([]);
  /** Etiqueta del campo con el error. */
  const [invalido, setInvalido] = useState<string | null>(null);
  const [polvoAnadido, setPolvoAnadido] = useState(false);
  const [busy, setBusy] = useState(false);
  const formRef = useRef<HTMLFormElement>(null);
  const errorId = 'cripto-inicio-error';
  const marca = (campo: string) =>
    invalido === campo ? { 'aria-invalid': true, 'aria-describedby': errorId } : {};

  const usarGestoria = async () => {
    setBusy(true);
    setMsg(null);
    setAvisos([]);
    setInvalido(null);
    setPolvoAnadido(false);
    try {
      // Si ya se ha escrito la cuenta del diario, la propuesta sale de ella.
      const p = await criptoPropuestaInicio(form?.cuenta || undefined);
      setPropuesta(p);
      setForm(
        toForm({
          fecha: p.fecha,
          cuenta: p.cuenta,
          origen: p.origen,
          nota: form?.nota ?? inicio?.nota ?? '',
          posiciones: p.posiciones.length
            ? p.posiciones
            : [{ ticker: '', platform_id: null, cantidad: '', coste: 0 }],
        }),
      );
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const aMano = () => {
    setPropuesta(null);
    setMsg(null);
    setAvisos([]);
    setError(null);
    setInvalido(null);
    setForm(
      inicio
        ? toForm(inicio)
        : {
            fecha: `${year - 1}-12-31`,
            cuenta: '',
            origen: 'a mano',
            nota: '',
            filas: [filaVacia()],
          },
    );
  };

  const guardar = async () => {
    if (!form) return;
    const r = fromForm(form, hoyIso());
    if ('error' in r) {
      setError(r.error);
      setInvalido(r.campo);
      formRef.current?.querySelector<HTMLElement>(`[aria-label="${r.campo}"]`)?.focus();
      return;
    }
    setInvalido(null);
    setBusy(true);
    try {
      const av = await criptoSaveInicio(r);
      setForm(null);
      setPropuesta(null);
      setError(null);
      setAvisos(av ?? []);
      setMsg('Punto de partida guardado.');
      onSaved();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const setFila = (i: number, f: Partial<Fila>) => {
    if (!form) return;
    setForm({ ...form, filas: form.filas.map((x, j) => (j === i ? { ...x, ...f } : x)) });
  };

  const total = form?.filas.reduce((a, x) => a + (parseEuros(x.coste || '0') ?? 0), 0) ?? 0;
  const diario = propuesta?.coste_diario ?? null;
  const nombre = (id: number | null) =>
    id === null
      ? 'Sin plataforma'
      : (platforms.find((p) => p.id === id)?.name ?? `plataforma ${id}`);

  return (
    <section aria-labelledby="cripto-inicio-h" className="mt-4 rounded border border-stone-200 p-3">
      <h4 id="cripto-inicio-h" className="font-medium">
        Punto de partida
      </h4>
      <p className="mt-1 text-stone-600">
        Si faltan registros de años antiguos (exchanges que cerraron, DeFi), el cálculo empieza en
        lo que calculó la gestoría: a una fecha de corte, cada moneda con su cantidad y su coste
        pendiente. Desde el día siguiente se sigue con el método de arriba; las operaciones de antes
        quedan como historial (se ven, no cuentan ni se contabilizan).
      </p>
      {error && (
        <p id={errorId} role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
      <div role="status">
        {msg && <p className="mt-2">{msg}</p>}
        {avisos.length > 0 && (
          <ul className="mt-1 list-disc pl-5 text-amber-800">
            {avisos.map((a) => (
              <li key={a}>{a}</li>
            ))}
          </ul>
        )}
      </div>
      {!inicio && !form && huecos > 0 && (
        <p className="mt-2 text-amber-800">
          Faltan operaciones:{' '}
          {huecos === 1 ? 'una venta o traspaso sale' : `${huecos} ventas o traspasos salen`} de más
          unidades de las que había. Pon el punto de partida para que lo de antes no cuente.
        </p>
      )}
      {inicio && !form && (
        <>
          <p className="mt-2">
            Desde el <strong>{fecha(inicio.fecha)}</strong>
            {inicio.cuenta && (
              <>
                {' '}
                · cuenta <span className="font-mono">{inicio.cuenta}</span>
              </>
            )}
            {inicio.origen && <span className="text-stone-600"> · {inicio.origen}</span>}
          </p>
          <table className="mt-1">
            <thead className="text-left text-stone-500">
              <tr>
                <th>Moneda</th>
                <th>Plataforma</th>
                <th className={cell}>Cantidad</th>
                <th className={cell}>Coste pendiente</th>
                <th className={cell}>Por unidad</th>
              </tr>
            </thead>
            <tbody>
              {inicio.posiciones.map((p) => (
                <tr key={`${p.ticker}-${p.platform_id ?? 0}`}>
                  <td className="font-mono">{p.ticker}</td>
                  <td>{nombre(p.platform_id)}</td>
                  <td className={cell}>{criptoQty(p.cantidad)}</td>
                  <td className={cell}>{euros(p.coste)}</td>
                  <td className={cell}>{porUnidad(p.cantidad, plain(p.coste))}</td>
                </tr>
              ))}
              <tr className="font-medium">
                <td colSpan={3}>Total</td>
                <td className={cell}>
                  {euros(inicio.posiciones.reduce((a, p) => a + p.coste, 0))}
                </td>
                <td />
              </tr>
            </tbody>
          </table>
          {inicio.nota && <p className="mt-1 text-stone-600">{inicio.nota}</p>}
        </>
      )}
      {!form && (
        <div className="mt-2 flex flex-wrap gap-2">
          <button
            type="button"
            className="rounded btn-primary px-3 py-1"
            onClick={usarGestoria}
            disabled={busy}
          >
            Usar los cálculos de la gestoría
          </button>
          <button type="button" className="rounded border border-stone-300 px-3" onClick={aMano}>
            {inicio ? 'Cambiar' : 'Ponerlo a mano'}
          </button>
          {inicio && (
            <ConfirmAction
              label="Quitar el punto de partida"
              question="El cálculo volverá a salir de todas las operaciones importadas, también las incompletas de años antiguos. ¿Quitarlo?"
              confirmLabel="Sí, quitarlo"
              onConfirm={async () => {
                const av = await criptoDeleteInicio();
                setAvisos(av ?? []);
                setMsg('Punto de partida quitado.');
                onSaved();
              }}
            />
          )}
        </div>
      )}
      {form && (
        <form
          ref={formRef}
          noValidate
          aria-label="Punto de partida"
          className="mt-3"
          onSubmit={(e) => {
            e.preventDefault();
            guardar();
          }}
        >
          {propuesta && (
            <div className="mb-2 rounded bg-stone-50 p-2 text-stone-700">
              <p>
                Propuesta a {fecha(propuesta.fecha)}: el ejercicio {propuesta.motivo}.{' '}
                {propuesta.reparto_seguro
                  ? 'Cantidades y coste por moneda deducidos de lo que hay: compruébalos y guarda.'
                  : 'No se puede deducir con seguridad el coste de cada moneda: revísalo antes de guardar.'}
              </p>
              {propuesta.notas.length > 0 && (
                <ul className="mt-1 list-disc pl-5">
                  {propuesta.notas.map((n) => (
                    <li key={n}>{n}</li>
                  ))}
                </ul>
              )}
              {propuesta.polvo.length > 0 && !polvoAnadido && (
                <p className="mt-1">
                  Polvo (menos de 1 €, sin coste):{' '}
                  {propuesta.polvo.map((p) => `${p.ticker} ${criptoQty(p.cantidad)}`).join(', ')}.{' '}
                  <button
                    type="button"
                    className="underline"
                    onClick={() => {
                      setForm({
                        ...form,
                        filas: [
                          ...form.filas,
                          ...propuesta.polvo.map((p) => ({
                            ticker: p.ticker,
                            platform: p.platform_id ? String(p.platform_id) : '',
                            cantidad: editable(p.cantidad),
                            coste: plain(p.coste),
                          })),
                        ],
                      });
                      setPolvoAnadido(true);
                    }}
                  >
                    Añadir el polvo
                  </button>
                </p>
              )}
            </div>
          )}
          <div className="flex flex-wrap gap-3">
            <label>
              Fecha de corte
              <input
                type="date"
                aria-label="Fecha de corte"
                className="mt-1 block"
                max={ayerIso()}
                {...marca('Fecha de corte')}
                value={form.fecha}
                onChange={(e) => setForm({ ...form, fecha: e.target.value })}
                required
              />
            </label>
            <label>
              Cuenta del diario
              <input
                className="mt-1 block w-32 font-mono"
                value={form.cuenta}
                placeholder="2070001"
                onChange={(e) => setForm({ ...form, cuenta: e.target.value })}
              />
            </label>
          </div>
          <p className="mt-1 text-xs text-stone-500">
            La fecha de corte es el último día que no cuenta (el cierre del último ejercicio
            declarado). La cuenta es donde la gestoría tiene el coste: las ventas de estas monedas
            se contabilizan contra ella (vacía: una subcuenta por moneda). Si los cálculos de la
            gestoría no la encuentran, escríbela y vuelve a pedirlos.
          </p>
          <table className="mt-2">
            <thead className="text-left text-stone-500">
              <tr>
                <th>Moneda</th>
                <th>Plataforma o monedero</th>
                <th className={cell}>Cantidad</th>
                <th className={cell}>Coste pendiente (€)</th>
                <th className={cell}>Por unidad</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {form.filas.map((x, i) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: filas sin id que se editan en su sitio
                <tr key={i}>
                  <td>
                    <input
                      aria-label={`Moneda ${i + 1}`}
                      {...marca(`Moneda ${i + 1}`)}
                      className="w-20 font-mono uppercase"
                      value={x.ticker}
                      placeholder="BTC"
                      onChange={(e) => setFila(i, { ticker: e.target.value })}
                    />
                  </td>
                  <td>
                    <select
                      aria-label={`Plataforma ${i + 1}`}
                      value={x.platform}
                      onChange={(e) => setFila(i, { platform: e.target.value })}
                    >
                      <option value="">Sin plataforma</option>
                      {platforms.map((p) => (
                        <option key={p.id} value={p.id}>
                          {p.name}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td>
                    <input
                      aria-label={`Cantidad ${i + 1}`}
                      {...marca(`Cantidad ${i + 1}`)}
                      inputMode="decimal"
                      className="w-32 text-right"
                      value={x.cantidad}
                      onChange={(e) => setFila(i, { cantidad: e.target.value })}
                    />
                  </td>
                  <td>
                    <input
                      aria-label={`Coste ${i + 1}`}
                      {...marca(`Coste ${i + 1}`)}
                      inputMode="decimal"
                      className="w-32 text-right"
                      value={x.coste}
                      placeholder="0,00"
                      onChange={(e) => setFila(i, { coste: e.target.value })}
                    />
                  </td>
                  <td className={cell}>{porUnidad(x.cantidad, x.coste)}</td>
                  <td>
                    <button
                      type="button"
                      className="text-red-700 underline"
                      aria-label={`Quitar la fila ${i + 1}`}
                      onClick={() =>
                        setForm({ ...form, filas: form.filas.filter((_, j) => j !== i) })
                      }
                    >
                      Quitar
                    </button>
                  </td>
                </tr>
              ))}
              <tr className="font-medium">
                <td colSpan={3}>Total</td>
                <td className={cell}>{euros(total)}</td>
                <td colSpan={2} />
              </tr>
            </tbody>
          </table>
          {diario !== null && (
            <p
              role="status"
              aria-label="Cuadre con el diario"
              className={total === diario ? 'mt-1 text-stone-600' : 'mt-1 text-amber-800'}
            >
              Diario a {fecha(propuesta?.fecha ?? form.fecha)}: {euros(diario)}.{' '}
              {total === diario
                ? 'Cuadra.'
                : total < diario
                  ? `Faltan ${euros(diario - total)} por repartir.`
                  : `Sobran ${euros(total - diario)}.`}
            </p>
          )}
          <label className="mt-2 block">
            Nota
            <input
              className="mt-1 block w-full"
              value={form.nota}
              placeholder="Hoja de la gestoría, balance del 200…"
              onChange={(e) => setForm({ ...form, nota: e.target.value })}
            />
          </label>
          <div className="mt-2 flex flex-wrap gap-2">
            <button
              type="button"
              className="rounded border border-stone-300 px-3"
              onClick={() => setForm({ ...form, filas: [...form.filas, filaVacia()] })}
            >
              Añadir moneda
            </button>
            <button
              type="button"
              className="rounded border border-stone-300 px-3"
              onClick={usarGestoria}
              disabled={busy}
            >
              Usar los cálculos de la gestoría
            </button>
            <button type="submit" className="rounded btn-primary px-3 py-1" disabled={busy}>
              Guardar el punto de partida
            </button>
            <button
              type="button"
              className="rounded border border-stone-300 px-3"
              onClick={() => {
                setForm(null);
                setPropuesta(null);
                setError(null);
                setInvalido(null);
              }}
            >
              Cancelar
            </button>
          </div>
        </form>
      )}
      <p className="mt-3 text-xs text-stone-600">
        Para una sociedad, el resultado de las criptomonedas es contable y entra en la base del
        Impuesto sobre Sociedades (art. 10.3 LIS). El precio medio ponderado es el criterio del PGC
        para bienes intercambiables entre sí (NRV 10.ª) y, una vez elegido, se mantiene (principio
        de uniformidad). El punto de partida es el coste que figura en el balance del último
        ejercicio cerrado y declarado. Si faltan registros de años antiguos, se parte de lo
        declarado: la deuda de los ejercicios prescritos ya no se puede liquidar (art. 66.a LGT,
        cuatro años), pero Hacienda puede comprobar datos de esos años que afecten a los que no lo
        están, como el coste de lo que vendas ahora (arts. 66 bis.1 y 115.1 LGT). Guarda los
        soportes que tengas: la hoja de la gestoría, historiales y extractos.
      </p>
    </section>
  );
}

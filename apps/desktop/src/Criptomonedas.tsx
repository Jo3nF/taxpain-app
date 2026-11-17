import { useCallback, useEffect, useState } from 'react';
import {
  CRIPTO_TIPOS,
  type CriptoLectura,
  type CriptoMetodo,
  type CriptoNaturaleza,
  type CriptoOp,
  type CriptoPlatform,
  type CriptoPrice,
  type CriptoReport,
  type CriptoSettings,
  type CriptoTipo,
  criptoDeleteOp,
  criptoDeletePlatform,
  criptoDeletePrice,
  criptoExport,
  criptoImpairment,
  criptoImport,
  criptoOps,
  criptoPlatforms,
  criptoPostEntries,
  criptoPrices,
  criptoQty,
  criptoReadFile,
  criptoReport,
  criptoSaveOp,
  criptoSavePlatform,
  criptoSaveSettings,
  criptoSetPrices,
  criptoTipoLabel,
  errorMessage,
  euros,
  parseCriptoQty,
  parseEuros,
  pickCriptoFile,
  pickFolder,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { CriptoInicio } from './CriptoInicio';

const plain = (c: number) => (c / 100).toFixed(2).replace('.', ',');
const fecha = (iso: string) => {
  const [y, m, d] = iso.split('-');
  return `${d}/${m}/${y}`;
};
const cell = 'px-2 py-1 text-right tabular-nums';

const METODO_POR_QUE: Record<CriptoMetodo, string> = {
  pmp: 'Precio medio ponderado: cada venta sale al coste medio de lo que había. Admitido para existencias por la NRV 10.ª del PGC; es el que suelen usar las sociedades.',
  fifo: 'FIFO: lo primero que se compró es lo primero que sale. Admitido por la NRV 10.ª del PGC y el único que vale en el IRPF (art. 37.2 LIRPF por analogía, DGT V0999-18).',
};
const NATURALEZA_POR_QUE: Record<CriptoNaturaleza, string> = {
  existencias:
    'Existencias (grupo 3): las tienes para venderlas en el curso ordinario de la actividad (consulta del BOICAC 120/2019). Su deterioro al cierre es deducible en el Impuesto sobre Sociedades.',
  intangible:
    'Inmovilizado intangible (grupo 20): las tienes como inversión o para usarlas (consulta del BOICAC 120/2019). Su deterioro no es deducible (art. 13.2.a LIS): ajuste positivo en el 200.',
};

function emptyOp(platform: number): CriptoOp {
  return {
    id: 0,
    date: new Date().toISOString().slice(0, 10),
    time: '00:00:00',
    kind: 'compra',
    platform_id: platform,
    to_platform_id: null,
    ticker: '',
    qty: '',
    ticker2: null,
    qty2: null,
    eur: 0,
    fee_eur: 0,
    ref_price: null,
    note: '',
    source: 'manual',
    fingerprint: null,
    entry_id: null,
  };
}

type OpForm = {
  date: string;
  time: string;
  kind: CriptoTipo;
  platform_id: string;
  to_platform_id: string;
  ticker: string;
  qty: string;
  ticker2: string;
  qty2: string;
  eur: string;
  fee_eur: string;
  note: string;
};

function toForm(o: CriptoOp): OpForm {
  return {
    date: o.date,
    time: o.time.slice(0, 5),
    kind: o.kind,
    platform_id: String(o.platform_id),
    to_platform_id: o.to_platform_id ? String(o.to_platform_id) : '',
    ticker: o.ticker,
    qty: criptoQty(o.qty),
    ticker2: o.ticker2 ?? '',
    qty2: criptoQty(o.qty2),
    eur: o.eur ? plain(o.eur) : '',
    fee_eur: o.fee_eur ? plain(o.fee_eur) : '',
    note: o.note,
  };
}

/** Criptomonedas: posiciones, operaciones, resultados por los dos métodos, 721 y asientos. */
export function Criptomonedas({ year: initialYear }: { year: number }) {
  const [year, setYear] = useState(initialYear);
  const [report, setReport] = useState<CriptoReport | null>(null);
  const [ops, setOps] = useState<CriptoOp[]>([]);
  const [platforms, setPlatforms] = useState<CriptoPlatform[]>([]);
  const [prices, setPrices] = useState<CriptoPrice[]>([]);
  const [settings, setSettings] = useState<CriptoSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [editing, setEditing] = useState<CriptoOp | null>(null);
  const [form, setForm] = useState<OpForm | null>(null);
  const [lectura, setLectura] = useState<CriptoLectura | null>(null);
  const [showNormas, setShowNormas] = useState(false);
  const [newPlatform, setNewPlatform] = useState<CriptoPlatform | null>(null);
  const [newPrice, setNewPrice] = useState({ date: `${year}-12-31`, ticker: '', price: '' });

  const load = useCallback(() => {
    Promise.all([criptoReport(year), criptoOps(year), criptoPlatforms(), criptoPrices()])
      .then(([r, o, p, pr]) => {
        setReport(r);
        setSettings(r.settings);
        setOps(o);
        setPlatforms(p);
        setPrices(pr);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);

  const run = (p: Promise<unknown>, ok: string | ((v: unknown) => string)) =>
    p
      .then((v) => {
        setError(null);
        setMsg(typeof ok === 'function' ? ok(v) : ok);
        load();
      })
      .catch((e) => setError(errorMessage(e)));

  const startOp = (o: CriptoOp) => {
    setEditing(o);
    setForm(toForm(o));
    setMsg(null);
  };

  const saveOp = () => {
    if (!form || !editing) return;
    const qty = parseCriptoQty(form.qty);
    if (!qty || qty.startsWith('-')) return setError('Revisa la cantidad.');
    const eur = parseEuros(form.eur || '0');
    const fee = parseEuros(form.fee_eur || '0');
    if (eur === null || fee === null) return setError('Revisa los euros o la comisión.');
    const qty2 = form.kind === 'permuta' ? parseCriptoQty(form.qty2) : null;
    if (form.kind === 'permuta' && (!qty2 || !form.ticker2.trim()))
      return setError('La permuta necesita el activo y la cantidad que recibes.');
    const op: CriptoOp = {
      ...editing,
      date: form.date,
      time: form.time.length === 5 ? `${form.time}:00` : form.time || '00:00:00',
      kind: form.kind,
      platform_id: Number(form.platform_id),
      to_platform_id:
        form.kind === 'traspaso' && form.to_platform_id ? Number(form.to_platform_id) : null,
      ticker: form.ticker.trim().toUpperCase(),
      qty,
      ticker2: form.kind === 'permuta' ? form.ticker2.trim().toUpperCase() : null,
      qty2,
      eur,
      fee_eur: fee,
      note: form.note,
    };
    run(criptoSaveOp(op), editing.id ? 'Operación guardada.' : 'Operación añadida.').then(() => {
      setEditing(null);
      setForm(null);
    });
  };

  const importFile = async () => {
    const path = await pickCriptoFile();
    if (!path) return;
    try {
      setLectura(await criptoReadFile(path));
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const exportar = async (pdf: boolean) => {
    const dir = await pickFolder();
    if (!dir) return;
    run(criptoExport(year, dir, pdf), (p) => `Guardado en ${String(p)}.`);
  };

  const r = report;
  const years = [initialYear + 1, initialYear, initialYear - 1, initialYear - 2, initialYear - 3];
  const posicionesSinPrecio = r?.posiciones.filter((p) => p.valor === null).length ?? 0;
  /** Fecha de corte del punto de partida: lo de hasta ese día es historial. */
  const corte = r?.inicio?.fecha ?? null;
  const historial = (date: string) => corte !== null && date <= corte;

  return (
    <section aria-labelledby="cripto-h" className="mt-4 text-sm">
      <h3 id="cripto-h" className="text-lg font-medium">
        Criptomonedas
      </h3>
      <p className="text-stone-600">
        Lo que tienes en cada plataforma, lo que has ganado o perdido al vender, permutar o pagar
        con cripto, el modelo 721 y los asientos. Los modelos 172 y 173 los presentan las
        plataformas españolas, no tú: no tienes que hacer nada con ellos.
      </p>
      <label className="mt-3 block">
        Ejercicio
        <select className="ml-2" value={year} onChange={(e) => setYear(Number(e.target.value))}>
          {years.map((y) => (
            <option key={y} value={y}>
              {y}
            </option>
          ))}
        </select>
      </label>
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}

      {settings && r && (
        <div className="mt-4 rounded border border-stone-200 p-3">
          <p className="font-medium">Cómo se valoran</p>
          <div className="mt-2 flex flex-wrap gap-4">
            <label>
              Método
              <select
                className="ml-2"
                value={settings.metodo}
                onChange={(e) =>
                  setSettings({ ...settings, metodo: e.target.value as CriptoMetodo })
                }
              >
                <option value="pmp">Precio medio ponderado</option>
                <option value="fifo">FIFO</option>
              </select>
            </label>
            <label>
              Naturaleza
              <select
                className="ml-2"
                value={settings.naturaleza}
                onChange={(e) =>
                  setSettings({
                    ...settings,
                    naturaleza: e.target.value as CriptoNaturaleza,
                  })
                }
              >
                <option value="existencias">Existencias</option>
                <option value="intangible">Inmovilizado intangible</option>
              </select>
            </label>
            <button
              type="button"
              className="rounded border border-stone-300 px-3"
              onClick={() => run(criptoSaveSettings(settings), 'Ajustes guardados.')}
            >
              Guardar ajustes
            </button>
          </div>
          <p className="mt-2 text-stone-600">{METODO_POR_QUE[settings.metodo]}</p>
          <p className="mt-1 text-stone-600">{NATURALEZA_POR_QUE[settings.naturaleza]}</p>
          {r.persona_fisica && (
            <p className="mt-1 text-amber-800">
              Eres persona física: la Renta y los asientos usan FIFO aunque elijas precio medio; las
              ganancias y pérdidas van a la base del ahorro, no a la actividad.
            </p>
          )}
          <details className="mt-2">
            <summary className="cursor-pointer text-stone-600">Cuentas de los asientos</summary>
            <div className="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-4">
              {(
                [
                  ['existencias', 'Existencias (base)'],
                  ['intangible', 'Intangible (base)'],
                  ['tesoreria', 'Tesorería'],
                  ['puente', 'Puente pagos/cobros'],
                  ['ventas', 'Ingreso (existencias)'],
                  ['variacion', 'Coste (existencias)'],
                  ['beneficio', 'Beneficio (intangible)'],
                  ['perdida', 'Pérdida (intangible)'],
                  ['comisiones', 'Comisiones'],
                  ['recompensas', 'Recompensas'],
                  ['deterioro_gasto', 'Deterioro: gasto'],
                  ['deterioro', 'Deterioro: corrección'],
                  ['reversion', 'Reversión del deterioro'],
                ] as const
              ).map(([k, label]) => (
                <label key={k} className="text-xs">
                  {label}
                  <input
                    className="mt-1 block w-full font-mono"
                    value={settings.cuentas[k]}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        cuentas: { ...settings.cuentas, [k]: e.target.value.trim() },
                      })
                    }
                  />
                </label>
              ))}
            </div>
          </details>
        </div>
      )}

      {r && (
        <CriptoInicio
          inicio={r.inicio}
          platforms={platforms}
          year={year}
          huecos={r.huecos}
          onSaved={load}
        />
      )}

      {r && r.avisos.length > 0 && (
        <ul
          aria-label="Avisos"
          className="mt-3 list-disc rounded bg-amber-50 p-3 pl-7 text-amber-900"
        >
          {r.avisos.map((a) => (
            <li key={a}>{a}</li>
          ))}
        </ul>
      )}

      {r && (
        <>
          <h4 className="mt-5 font-medium">
            Posiciones a {fecha(r.fecha)}
            {corte === r.fecha && ' (punto de partida)'}
          </h4>
          {corte !== null && r.fecha < corte ? (
            <p className="text-stone-600">
              Antes del punto de partida ({fecha(corte)}): no se calculan, porque faltan
              operaciones. Valen las que declaró la gestoría.
            </p>
          ) : r.posiciones.length === 0 ? (
            <p className="text-stone-600">Sin criptomonedas a esa fecha.</p>
          ) : (
            <table className="mt-1 w-full">
              <thead className="text-left text-stone-500">
                <tr>
                  <th>Activo</th>
                  <th>Plataforma</th>
                  <th className={cell}>Cantidad</th>
                  <th className={cell}>Coste</th>
                  <th className={cell}>Precio de cierre</th>
                  <th className={cell}>Valor</th>
                  <th className={cell}>Plusvalía latente</th>
                </tr>
              </thead>
              <tbody>
                {r.posiciones.map((p) => (
                  <tr key={`${p.ticker}-${p.platform_id}`}>
                    <td className="font-mono">{p.ticker}</td>
                    <td>
                      {p.platform}
                      {p.abroad && (
                        <span className="ml-1 text-xs text-stone-500">(extranjero)</span>
                      )}
                    </td>
                    <td className={cell}>{criptoQty(p.cantidad)}</td>
                    <td className={cell}>{euros(p.coste)}</td>
                    <td className={cell}>
                      {p.precio ? `${criptoQty(p.precio)} €` : '—'}
                      {p.precio_fecha && (
                        <span className="ml-1 text-xs text-stone-500">
                          ({fecha(p.precio_fecha)})
                        </span>
                      )}
                    </td>
                    <td className={cell}>{p.valor === null ? '—' : euros(p.valor)}</td>
                    <td className={`${cell} ${(p.latente ?? 0) < 0 ? 'text-red-700' : ''}`}>
                      {p.latente === null ? '—' : euros(p.latente)}
                    </td>
                  </tr>
                ))}
                <tr className="font-medium">
                  <td colSpan={3}>Total</td>
                  <td className={cell}>{euros(r.coste_total)}</td>
                  <td />
                  <td className={cell}>{euros(r.valor_total)}</td>
                  <td className={cell}>{euros(r.valor_total - r.coste_total)}</td>
                </tr>
              </tbody>
            </table>
          )}

          <h4 className="mt-5 font-medium">Resultados realizados de {year}</h4>
          {r.antes_del_inicio && corte !== null ? (
            <p className="mt-1">
              <strong>Calculado por la gestoría.</strong> El punto de partida es el {fecha(corte)}:
              los resultados de {year} son los que se declararon y las {r.ops_historial} operaciones
              de {year} son historial (se ven, no cuentan).
            </p>
          ) : (
            <>
              <table className="mt-1">
                <thead className="text-left text-stone-500">
                  <tr>
                    <th />
                    <th className={cell}>Precio medio{r.metodo_declaracion === 'pmp' && ' ✓'}</th>
                    <th className={cell}>FIFO{r.metodo_declaracion === 'fifo' && ' ✓'}</th>
                  </tr>
                </thead>
                <tbody>
                  {(
                    [
                      ['Ganancias', 'ganancias'],
                      ['Pérdidas', 'perdidas'],
                      ['Neto', 'neto'],
                      ['Recompensas (ingreso)', 'recompensas'],
                      ['Comisiones (gasto)', 'comisiones'],
                    ] as const
                  ).map(([label, k]) => (
                    <tr key={k} className={k === 'neto' ? 'font-medium' : ''}>
                      <td className="pr-2">{label}</td>
                      <td className={cell}>{euros(r.resumen_pmp[k])}</td>
                      <td className={cell}>{euros(r.resumen_fifo[k])}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <p className="text-xs text-stone-500">
                ✓ = el que se contabiliza y se declara (
                {r.metodo_declaracion === 'fifo' ? 'FIFO' : 'precio medio'}
                ). {r.realizados.length} transmisiones en {year}.
              </p>
              {r.realizados.length > 0 && (
                <details className="mt-1">
                  <summary className="cursor-pointer text-stone-600">Ver cada transmisión</summary>
                  <table className="mt-1 w-full">
                    <thead className="text-left text-stone-500">
                      <tr>
                        <th>Fecha</th>
                        <th>Tipo</th>
                        <th>Activo</th>
                        <th>Plataforma</th>
                        <th className={cell}>Cantidad</th>
                        <th className={cell}>Valor</th>
                        <th className={cell}>Coste</th>
                        <th className={cell}>Resultado</th>
                      </tr>
                    </thead>
                    <tbody>
                      {r.realizados.map((x) => (
                        <tr key={`${x.op}-${x.ticker}`}>
                          <td>{fecha(x.fecha)}</td>
                          <td>{criptoTipoLabel(x.tipo)}</td>
                          <td className="font-mono">{x.ticker}</td>
                          <td>{x.platform}</td>
                          <td className={cell}>{criptoQty(x.cantidad)}</td>
                          <td className={cell}>{euros(x.valor)}</td>
                          <td className={cell}>{euros(x.coste)}</td>
                          <td className={`${cell} ${x.resultado < 0 ? 'text-red-700' : ''}`}>
                            {euros(x.resultado)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </details>
              )}
            </>
          )}
        </>
      )}

      <h4 className="mt-5 font-medium">Operaciones de {year}</h4>
      <div className="mt-1 flex flex-wrap gap-2">
        <button
          type="button"
          className="rounded btn-primary px-3 py-1"
          onClick={() => startOp(emptyOp(platforms[0]?.id ?? 0))}
          disabled={platforms.length === 0}
        >
          Nueva operación
        </button>
        <button type="button" className="rounded border border-stone-300 px-3" onClick={importFile}>
          Importar fichero…
        </button>
        <button
          type="button"
          className="rounded border border-stone-300 px-3"
          onClick={() => exportar(false)}
        >
          Exportar a Excel…
        </button>
        <button
          type="button"
          className="rounded border border-stone-300 px-3"
          onClick={() => exportar(true)}
        >
          Exportar a PDF…
        </button>
      </div>
      {platforms.length === 0 && (
        <p className="mt-1 text-stone-600">
          Añade antes una plataforma (abajo) o importa un fichero.
        </p>
      )}
      {form && editing && (
        <form
          aria-label="Operación"
          className="mt-3 grid grid-cols-2 gap-2 rounded border border-stone-200 p-3 sm:grid-cols-4"
          onSubmit={(e) => {
            e.preventDefault();
            saveOp();
          }}
        >
          <label>
            Fecha
            <input
              type="date"
              className="mt-1 block w-full"
              value={form.date}
              onChange={(e) => setForm({ ...form, date: e.target.value })}
              required
            />
          </label>
          <label>
            Hora
            <input
              type="time"
              className="mt-1 block w-full"
              value={form.time}
              onChange={(e) => setForm({ ...form, time: e.target.value })}
            />
          </label>
          <label>
            Tipo
            <select
              className="mt-1 block w-full"
              value={form.kind}
              onChange={(e) => setForm({ ...form, kind: e.target.value as CriptoTipo })}
            >
              {CRIPTO_TIPOS.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.label}
                </option>
              ))}
            </select>
          </label>
          <label>
            Plataforma
            <select
              className="mt-1 block w-full"
              value={form.platform_id}
              onChange={(e) => setForm({ ...form, platform_id: e.target.value })}
            >
              {platforms.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </label>
          {form.kind === 'traspaso' && (
            <label>
              Destino
              <select
                className="mt-1 block w-full"
                value={form.to_platform_id}
                onChange={(e) => setForm({ ...form, to_platform_id: e.target.value })}
              >
                <option value="">—</option>
                {platforms.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label>
            {form.kind === 'permuta' ? 'Activo que entregas' : 'Activo'}
            <input
              className="mt-1 block w-full font-mono uppercase"
              value={form.ticker}
              onChange={(e) => setForm({ ...form, ticker: e.target.value })}
              placeholder="BTC"
              required
            />
          </label>
          <label>
            Cantidad
            <input
              className="mt-1 block w-full text-right"
              value={form.qty}
              onChange={(e) => setForm({ ...form, qty: e.target.value })}
              placeholder="0,5"
              required
            />
          </label>
          {form.kind === 'permuta' && (
            <>
              <label>
                Activo que recibes
                <input
                  className="mt-1 block w-full font-mono uppercase"
                  value={form.ticker2}
                  onChange={(e) => setForm({ ...form, ticker2: e.target.value })}
                  placeholder="ETH"
                />
              </label>
              <label>
                Cantidad recibida
                <input
                  className="mt-1 block w-full text-right"
                  value={form.qty2}
                  onChange={(e) => setForm({ ...form, qty2: e.target.value })}
                />
              </label>
            </>
          )}
          {form.kind !== 'traspaso' && form.kind !== 'perdida' && (
            <label>
              {form.kind === 'compra' || form.kind === 'cobro'
                ? 'Euros pagados o cobrados'
                : form.kind === 'venta'
                  ? 'Euros cobrados'
                  : 'Valor de mercado en euros'}
              <input
                className="mt-1 block w-full text-right"
                value={form.eur}
                onChange={(e) => setForm({ ...form, eur: e.target.value })}
                placeholder="0,00"
              />
            </label>
          )}
          {form.kind !== 'perdida' && form.kind !== 'recompensa' && form.kind !== 'comision' && (
            <label>
              Comisión en euros
              <input
                className="mt-1 block w-full text-right"
                value={form.fee_eur}
                onChange={(e) => setForm({ ...form, fee_eur: e.target.value })}
                placeholder="0,00"
              />
            </label>
          )}
          <label className="col-span-2">
            Nota
            <input
              className="mt-1 block w-full"
              value={form.note}
              onChange={(e) => setForm({ ...form, note: e.target.value })}
            />
          </label>
          <div className="col-span-2 flex gap-2 sm:col-span-4">
            <button type="submit" className="rounded btn-primary px-3 py-1">
              {editing.id ? 'Guardar cambios' : 'Añadir operación'}
            </button>
            <button
              type="button"
              className="rounded border border-stone-300 px-3"
              onClick={() => {
                setEditing(null);
                setForm(null);
              }}
            >
              Cancelar
            </button>
          </div>
        </form>
      )}
      {lectura && (
        <section
          className="mt-3 rounded border border-stone-300 bg-stone-50 p-3"
          aria-label="Vista previa de la importación"
        >
          <p className="font-medium">
            {lectura.kind === 'cointracking' && 'Trade List de CoinTracking'}
            {lectura.kind === 'kraken' && 'Ledgers de Kraken'}
            {lectura.kind === 'hoja' && 'Hoja de operaciones'}
            {lectura.kind === 'saldos' && 'Hoja de saldos (precios de cierre)'}:{' '}
            {lectura.ops.length} operaciones
            {lectura.duplicates > 0 && ` (${lectura.duplicates} ya importadas, se saltan)`}
            {lectura.prices.length > 0 && `, ${lectura.prices.length} precios`}
            {lectura.new_platforms.length > 0 &&
              `. Plataformas nuevas: ${lectura.new_platforms.join(', ')}`}
            .
          </p>
          {lectura.warnings.length > 0 && (
            <ul className="mt-1 list-disc pl-5 text-amber-900">
              {lectura.warnings.slice(0, 12).map((w) => (
                <li key={w}>{w}</li>
              ))}
              {lectura.warnings.length > 12 && <li>… y {lectura.warnings.length - 12} más</li>}
            </ul>
          )}
          {lectura.ops.length > 0 && (
            <table className="mt-2 w-full text-xs">
              <thead className="text-left text-stone-500">
                <tr>
                  <th>Fecha</th>
                  <th>Tipo</th>
                  <th>Plataforma</th>
                  <th>Activo</th>
                  <th className={cell}>Cantidad</th>
                  <th className={cell}>Euros</th>
                  <th className={cell}>Comisión</th>
                </tr>
              </thead>
              <tbody>
                {lectura.ops.slice(0, 50).map((o, i) => (
                  <tr key={o.fingerprint ?? i}>
                    <td>{fecha(o.date)}</td>
                    <td>{criptoTipoLabel(o.kind)}</td>
                    <td>
                      {o.platform}
                      {o.to_platform && ` → ${o.to_platform}`}
                    </td>
                    <td className="font-mono">
                      {o.ticker}
                      {o.ticker2 && ` → ${o.ticker2}`}
                    </td>
                    <td className={cell}>
                      {criptoQty(o.qty)}
                      {o.qty2 && ` → ${criptoQty(o.qty2)}`}
                    </td>
                    <td className={cell}>{euros(o.eur)}</td>
                    <td className={cell}>{euros(o.fee_eur)}</td>
                  </tr>
                ))}
                {lectura.ops.length > 50 && (
                  <tr>
                    <td colSpan={7}>… y {lectura.ops.length - 50} más</td>
                  </tr>
                )}
              </tbody>
            </table>
          )}
          <div className="mt-2 flex gap-2">
            <button
              type="button"
              className="rounded btn-primary px-3 py-1"
              onClick={() =>
                run(criptoImport(lectura), (v) => {
                  const [n, d, p] = v as [number, number, number];
                  return `Importadas ${n} operaciones (${d} duplicadas) y ${p} precios.`;
                }).then(() => setLectura(null))
              }
            >
              Importar
            </button>
            <button
              type="button"
              className="rounded border border-stone-300 px-3"
              onClick={() => setLectura(null)}
            >
              Cancelar
            </button>
          </div>
        </section>
      )}
      {ops.length > 0 && (
        <table className="mt-2 w-full">
          <thead className="text-left text-stone-500">
            <tr>
              <th>Fecha</th>
              <th>Tipo</th>
              <th>Plataforma</th>
              <th>Activo</th>
              <th className={cell}>Cantidad</th>
              <th className={cell}>Euros</th>
              <th className={cell}>Comisión</th>
              <th>Asiento</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {ops.map((o) => (
              <tr key={o.id}>
                <td>
                  {fecha(o.date)}{' '}
                  <span className="text-xs text-stone-500">{o.time.slice(0, 5)}</span>
                </td>
                <td>{criptoTipoLabel(o.kind)}</td>
                <td>
                  {platforms.find((p) => p.id === o.platform_id)?.name ?? o.platform_id}
                  {o.to_platform_id &&
                    ` → ${platforms.find((p) => p.id === o.to_platform_id)?.name ?? ''}`}
                </td>
                <td className="font-mono">
                  {o.ticker}
                  {o.ticker2 && ` → ${o.ticker2}`}
                </td>
                <td className={cell}>
                  {criptoQty(o.qty)}
                  {o.qty2 && ` → ${criptoQty(o.qty2)}`}
                </td>
                <td className={cell}>{euros(o.eur)}</td>
                <td className={cell}>{euros(o.fee_eur)}</td>
                <td>
                  {o.entry_id ? (
                    'sí'
                  ) : historial(o.date) ? (
                    <span className="text-xs text-stone-500">historial</span>
                  ) : (
                    '—'
                  )}
                </td>
                <td className="whitespace-nowrap">
                  <button type="button" className="text-link underline" onClick={() => startOp(o)}>
                    Editar
                  </button>{' '}
                  <ConfirmAction
                    label="Borrar"
                    question={`¿Borrar la ${criptoTipoLabel(o.kind).toLowerCase()} de ${criptoQty(o.qty)} ${o.ticker} del ${fecha(o.date)}?${o.entry_id ? ' Se borra también su asiento.' : ''}`}
                    confirmLabel="Sí, borrar"
                    onConfirm={() => run(criptoDeleteOp(o.id), 'Operación borrada.')}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {r && (
        <>
          <h4 className="mt-5 font-medium">Asientos</h4>
          {r.antes_del_inicio && (
            <p className="text-stone-600">
              Ejercicio anterior al punto de partida: lo contabilizó la gestoría. Aquí sólo se
              contabiliza lo de después del {corte && fecha(corte)}.
            </p>
          )}
          <p className="text-stone-600">
            {r.ops_con_asiento} operaciones de {year} contabilizadas y {r.ops_sin_asiento} sin
            asiento. Cada operación genera su asiento con el método ✓ y la naturaleza elegida (una
            subcuenta por moneda); revísalos en el Libro diario (origen «Criptomonedas»). Las
            comisiones van a la {r.settings.cuentas.comisiones} y las recompensas a la{' '}
            {r.settings.cuentas.recompensas}. Un pago o cobro en cripto pasa por la{' '}
            {r.settings.cuentas.puente}: sáldala contra el proveedor o cliente.
          </p>
          <div className="mt-2 flex flex-wrap gap-2">
            <button
              type="button"
              className="rounded btn-primary px-3 py-1"
              disabled={r.ops_sin_asiento === 0 || r.antes_del_inicio}
              onClick={() =>
                run(criptoPostEntries(year, false), (v) => {
                  const c = v as { asientos: number; saltadas: number };
                  return `${c.asientos} asientos generados${c.saltadas ? ` (${c.saltadas} operaciones saltadas)` : ''}.`;
                })
              }
            >
              Contabilizar {r.ops_sin_asiento} operaciones
            </button>
            {r.ops_con_asiento > 0 && (
              <ConfirmAction
                label="Rehacer los asientos del ejercicio"
                question="Se borran los asientos de criptomonedas del ejercicio y se generan de nuevo con los costes al día (tras cambiar el método, la naturaleza o una operación anterior). ¿Seguir?"
                confirmLabel="Sí, rehacer"
                danger={false}
                className="rounded border border-stone-300 px-3"
                onConfirm={() =>
                  run(criptoPostEntries(year, true), (v) => {
                    const c = v as { asientos: number; borrados: number };
                    return `${c.borrados} asientos borrados y ${c.asientos} generados.`;
                  })
                }
              />
            )}
            <button
              type="button"
              className="rounded border border-stone-300 px-3"
              disabled={posicionesSinPrecio > 0 || r.posiciones.length === 0 || r.antes_del_inicio}
              onClick={() =>
                run(criptoImpairment(year), (v) =>
                  v
                    ? 'Deterioro al cierre registrado.'
                    : 'Sin deterioro: el valor no baja del coste.',
                )
              }
            >
              {r.deterioro_registrado ? 'Rehacer el deterioro al cierre' : 'Deterioro al cierre'}
            </button>
          </div>
          <p className="mt-1 text-xs text-stone-500">
            Deterioro: sólo si el precio de cierre deja el valor por debajo del coste (NRV 10.ª o
            6.ª PGC); con existencias es deducible, con intangible no (art. 13.2.a LIS). Necesita el
            precio de cierre de todas las posiciones. Si usas «Existencias finales» en Cierre, pon
            en las subcuentas de cripto su saldo (ya van al coste al día).
          </p>

          <h4 className="mt-5 font-medium">Modelo 721 ({r.m721.year})</h4>
          <p className={r.m721.supera ? 'text-amber-800' : 'text-stone-600'}>{r.m721.nota}</p>
          {r.m721.plataformas.length > 0 && (
            <table className="mt-1">
              <thead className="text-left text-stone-500">
                <tr>
                  <th>Plataforma</th>
                  <th>País</th>
                  <th>Activo</th>
                  <th className={cell}>Cantidad</th>
                  <th className={cell}>Valor a 31/12</th>
                </tr>
              </thead>
              <tbody>
                {r.m721.plataformas.flatMap((p) =>
                  p.activos.map(([ticker, qty, valor]) => (
                    <tr key={`${p.platform}-${ticker}`}>
                      <td>{p.platform}</td>
                      <td>{p.country}</td>
                      <td className="font-mono">{ticker}</td>
                      <td className={cell}>{criptoQty(qty)}</td>
                      <td className={cell}>{euros(valor)}</td>
                    </tr>
                  )),
                )}
                <tr className="font-medium">
                  <td colSpan={4}>Total en el extranjero</td>
                  <td className={cell}>{euros(r.m721.total_extranjero)}</td>
                </tr>
              </tbody>
            </table>
          )}
          {r.m721.supera && !r.m721.en_perfil && (
            <p className="mt-1 text-stone-600">
              Marca «Tengo criptomonedas en plataformas del extranjero» en el perfil fiscal para que
              el 721 salga en el calendario.
            </p>
          )}
        </>
      )}

      <h4 className="mt-5 font-medium">Precios de cierre</h4>
      <p className="text-stone-600">
        Euros por unidad a una fecha (la cotización a 31/12 de tu plataforma). La app no consulta
        internet: tecléalos o importa una hoja de saldos (fecha y, por moneda, «Valor en EUR» y
        «Cantidad»).
      </p>
      <form
        className="mt-2 flex flex-wrap items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          const price = parseCriptoQty(newPrice.price);
          if (!price || !newPrice.ticker.trim()) return setError('Revisa el precio o el activo.');
          run(
            criptoSetPrices([
              {
                date: newPrice.date,
                ticker: newPrice.ticker.trim().toUpperCase(),
                price,
                source: 'manual',
              },
            ]),
            'Precio guardado.',
          ).then(() => setNewPrice({ ...newPrice, ticker: '', price: '' }));
        }}
      >
        <label>
          Fecha
          <input
            type="date"
            className="mt-1 block"
            value={newPrice.date}
            onChange={(e) => setNewPrice({ ...newPrice, date: e.target.value })}
          />
        </label>
        <label>
          Activo
          <input
            className="mt-1 block w-24 font-mono uppercase"
            value={newPrice.ticker}
            onChange={(e) => setNewPrice({ ...newPrice, ticker: e.target.value })}
          />
        </label>
        <label>
          Precio en euros
          <input
            className="mt-1 block w-32 text-right"
            value={newPrice.price}
            onChange={(e) => setNewPrice({ ...newPrice, price: e.target.value })}
          />
        </label>
        <button type="submit" className="rounded border border-stone-300 px-3 py-1">
          Guardar precio
        </button>
      </form>
      {prices.length > 0 && (
        <table className="mt-2">
          <thead className="text-left text-stone-500">
            <tr>
              <th>Fecha</th>
              <th>Activo</th>
              <th className={cell}>Precio</th>
              <th>Origen</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {prices.slice(0, 60).map((p) => (
              <tr key={`${p.date}-${p.ticker}`}>
                <td>{fecha(p.date)}</td>
                <td className="font-mono">{p.ticker}</td>
                <td className={cell}>{criptoQty(p.price)} €</td>
                <td>{p.source}</td>
                <td>
                  <button
                    type="button"
                    className="text-red-700 underline"
                    onClick={() => run(criptoDeletePrice(p.date, p.ticker), 'Precio borrado.')}
                  >
                    Quitar
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <h4 className="mt-5 font-medium">Plataformas</h4>
      <p className="text-stone-600">
        Exchanges y monederos propios. «Del extranjero» = custodio que no presenta el 172/173 en
        España: cuenta para el 721 (art. 42 quater RGAT). Un monedero propio (Ledger, MetaMask) no
        tiene custodio: no cuenta.
      </p>
      {platforms.length > 0 && (
        <table className="mt-1">
          <thead className="text-left text-stone-500">
            <tr>
              <th>Nombre</th>
              <th>Tipo</th>
              <th>País</th>
              <th>Extranjero</th>
              <th>Cuenta de euros</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {platforms.map((p) => (
              <tr key={p.id}>
                <td>{p.name}</td>
                <td>{p.kind}</td>
                <td>{p.country}</td>
                <td>{p.abroad ? 'sí' : 'no'}</td>
                <td className="font-mono">{p.account || '—'}</td>
                <td className="whitespace-nowrap">
                  <button
                    type="button"
                    className="text-link underline"
                    onClick={() => setNewPlatform(p)}
                  >
                    Editar
                  </button>{' '}
                  <button
                    type="button"
                    className="text-red-700 underline"
                    onClick={() => run(criptoDeletePlatform(p.id), 'Plataforma borrada.')}
                  >
                    Quitar
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {newPlatform ? (
        <form
          aria-label="Plataforma"
          className="mt-2 flex flex-wrap items-end gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            run(criptoSavePlatform(newPlatform), 'Plataforma guardada.').then(() =>
              setNewPlatform(null),
            );
          }}
        >
          <label>
            Nombre
            <input
              className="mt-1 block"
              value={newPlatform.name}
              onChange={(e) => setNewPlatform({ ...newPlatform, name: e.target.value })}
              required
            />
          </label>
          <label>
            Tipo
            <select
              className="mt-1 block"
              value={newPlatform.kind}
              onChange={(e) =>
                setNewPlatform({ ...newPlatform, kind: e.target.value as 'exchange' | 'monedero' })
              }
            >
              <option value="exchange">Exchange</option>
              <option value="monedero">Monedero propio</option>
            </select>
          </label>
          <label>
            País
            <input
              className="mt-1 block w-16 uppercase"
              value={newPlatform.country}
              maxLength={2}
              onChange={(e) => setNewPlatform({ ...newPlatform, country: e.target.value })}
            />
          </label>
          <label className="flex items-center gap-1">
            <input
              type="checkbox"
              checked={newPlatform.abroad}
              onChange={(e) => setNewPlatform({ ...newPlatform, abroad: e.target.checked })}
            />
            Del extranjero (721)
          </label>
          <label>
            Cuenta de euros
            <input
              className="mt-1 block w-28 font-mono"
              value={newPlatform.account}
              placeholder="5720001"
              onChange={(e) => setNewPlatform({ ...newPlatform, account: e.target.value.trim() })}
            />
          </label>
          <button type="submit" className="rounded btn-primary px-3 py-1">
            Guardar plataforma
          </button>
          <button
            type="button"
            className="rounded border border-stone-300 px-3"
            onClick={() => setNewPlatform(null)}
          >
            Cancelar
          </button>
        </form>
      ) : (
        <button
          type="button"
          className="mt-2 rounded border border-stone-300 px-3 py-1"
          onClick={() =>
            setNewPlatform({
              id: 0,
              name: '',
              kind: 'exchange',
              country: 'ES',
              abroad: false,
              account: '',
            })
          }
        >
          Nueva plataforma
        </button>
      )}

      <button
        type="button"
        className="mt-5 text-stone-600 underline"
        onClick={() => setShowNormas(!showNormas)}
        aria-expanded={showNormas}
      >
        {showNormas ? 'Ocultar las normas' : 'Qué dicen las normas'}
      </button>
      {showNormas && (
        <ul className="mt-2 list-disc pl-5 text-stone-700">
          <li>
            <strong>Sociedad:</strong> el ICAC (consulta del BOICAC 120, diciembre de 2019) las
            trata como existencias si se tienen para vender en el curso ordinario y, si no, como
            inmovilizado intangible; nunca como dinero. Existencias: NRV 10.ª del PGC (coste de
            adquisición; precio medio o FIFO; deterioro si el valor baja, deducible). Intangible:
            NRV 5.ª y 6.ª (coste; deterioro no deducible, art. 13.2.a LIS). Permuta: NRV 2.ª. El
            resultado contable entra en la base del 200.
          </li>
          <li>
            <strong>Persona física:</strong> ganancias y pérdidas patrimoniales de la base del
            ahorro (arts. 33, 46 y 49 LIRPF), no rendimiento de la actividad salvo trading
            profesional. Se transmiten primero las adquiridas antes (FIFO: art. 37.2 LIRPF por
            analogía, DGT V0999-18 y V0975-22). Las permutas tributan por el mayor valor de mercado
            de lo entregado o lo recibido (art. 37.1.h). Las comisiones son mayor coste o menor
            valor de transmisión (art. 35). En Renta Web: apartado «monedas virtuales», casillas
            1800 a 1814 (comprueba los números en el manual del ejercicio).
          </li>
          <li>
            <strong>Modelo 721</strong> (Orden HFP/886/2023; art. 42 quater RGAT): saldos a 31 de
            diciembre en plataformas del extranjero valorados con la cotización de ese día, si pasan
            en conjunto de 50.000 €; del 1 de enero al 31 de marzo; los años siguientes sólo si el
            saldo sube más de 20.000 €. Una sociedad (o un autónomo con contabilidad por el Código
            de Comercio) que las tenga contabilizadas e identificadas por denominación, valor,
            custodio y país no tiene que presentarlo.
          </li>
          <li>
            <strong>Modelos 172 y 173</strong> (Orden HFP/887/2023): los presentan las plataformas,
            no tú. Sirven para cotejar lo que Hacienda sabe.
          </li>
          <li>
            Comisiones: en la compra son mayor coste; en la venta, menor valor de transmisión; las
            de traspasos y las pagadas en cripto, gasto. Un traspaso entre plataformas propias no
            realiza nada. Las recompensas (staking, airdrop) son ingreso por su valor de mercado y
            ese valor es su coste.
          </li>
        </ul>
      )}
    </section>
  );
}

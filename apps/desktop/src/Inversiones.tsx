import { useCallback, useEffect, useRef, useState } from 'react';
import {
  criptoQty,
  errorMessage,
  euros,
  formatDate,
  fxBceRate,
  parseEuros,
  pickFolder,
  plural,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import {
  type AplicadoValores,
  BROKER_LABEL,
  type Broker,
  CLASE_LABEL,
  type ContabilizadoValores,
  type CuentaBroker,
  type CuentaView,
  type LecturaValores,
  nuevaCuenta,
  type OpValores,
  type PasoBce,
  type PrecioCierre,
  parseDecimalEs,
  pickValoresFiles,
  TIPOS_OP,
  type TipoOpValores,
  tipoOpLabel,
  type Valor,
  type ValoresReport,
  type ValoresSettings,
  valoresBce,
  valoresDeleteAccount,
  valoresDeleteOp,
  valoresDeletePrice,
  valoresExport,
  valoresIbkrConnect,
  valoresIbkrDisconnect,
  valoresIbkrSync,
  valoresImport,
  valoresOps,
  valoresPostEntries,
  valoresPrices,
  valoresReadFiles,
  valoresReport,
  valoresSaveAccount,
  valoresSaveOp,
  valoresSaveSecurity,
  valoresSaveSettings,
  valoresSecurities,
  valoresSetPrices,
  valoresUndoEntries,
} from './valores';

const cell = 'px-2 py-1 text-right tabular-nums';
const boton = 'rounded border border-stone-300 px-3 py-1';

/** «2026-11-02 10:00:00 UTC» o ISO → «02/11/2026». */
const dia = (s: string | null) => (s ? formatDate(s.slice(0, 10)) : '');

/** Euros por unidad (8 decimales) a partir del tipo del BCE (unidades por euro), sin coma flotante. */
export function eurPorUnidad(rate: string): string | null {
  const m = /^(\d+)(?:\.(\d{1,6}))?$/.exec(rate.trim());
  if (!m) return null;
  const micros = BigInt(m[1] ?? '0') * 1_000_000n + BigInt((m[2] ?? '').padEnd(6, '0') || '0');
  if (micros === 0n) return null;
  const raw = (100_000_000n * 1_000_000n * 2n + micros) / (2n * micros);
  const s = raw.toString().padStart(9, '0');
  const dec = s.slice(-8).replace(/0+$/, '');
  return `${s.slice(0, -8)}${dec ? `.${dec}` : ''}`;
}

function resumenAplicado(a: AplicadoValores): string {
  const partes = [
    `${plural(a.nuevas, 'operación nueva', 'operaciones nuevas')}`,
    a.duplicadas ? `${a.duplicadas} ya estaban` : null,
    a.sustituidas ? `${a.sustituidas} leídas de posiciones sustituidas` : null,
    a.precios ? plural(a.precios, 'precio', 'precios') : null,
    a.cuentas_nuevas ? plural(a.cuentas_nuevas, 'cuenta nueva', 'cuentas nuevas') : null,
  ];
  return `${partes.filter(Boolean).join(', ')}.`;
}

/** Pasos para conectar Interactive Brokers por su Flex Web Service. */
function resumenContabilizado(c: ContabilizadoValores): string {
  const partes = [plural(c.asientos, 'asiento grabado', 'asientos grabados')];
  if (c.borrados) partes.push(plural(c.borrados, 'borrado antes', 'borrados antes'));
  if (c.saltadas)
    partes.push(plural(c.saltadas, 'operación sin asiento', 'operaciones sin asiento'));
  return `${partes.join(', ')}.${c.avisos.length ? ` ${c.avisos.join(' ')}` : ''}`;
}

function PasosIbkr() {
  return (
    <ol className="mt-2 list-decimal space-y-1 pl-5 text-stone-700">
      <li>
        Entra en el <strong>Client Portal</strong> de Interactive Brokers y abre{' '}
        <strong>Performance &amp; Reports → Flex Queries</strong>.
      </li>
      <li>
        En <strong>Activity Flex Query</strong>, pulsa <strong>+</strong> y ponle un nombre (por
        ejemplo, TaxPain).
      </li>
      <li>
        En <strong>Sections</strong> marca, con todos sus campos: Account Information, Trades (nivel
        «Executions»), Cash Transactions, Open Positions, Financial Instrument Information,
        Corporate Actions y Currency Conversion Rate. Guarda cada una.
      </li>
      <li>
        En <strong>Delivery Configuration</strong> elige el formato <strong>XML</strong> y el
        periodo «Last 365 Calendar Days» (TaxPain pide el año que quieras); en General Configuration
        deja la fecha como yyyyMMdd. Guarda la query.
      </li>
      <li>
        Apunta el <strong>Query ID</strong>: sale en el icono «Info» junto a la query.
      </li>
      <li>
        En <strong>Flex Web Service Configuration</strong> (el engranaje), activa el servicio y
        copia el <strong>token</strong>. Puedes elegir cuánto dura (de 6 horas a 1 año) y limitarlo
        a tu IP.
      </li>
      <li>
        Pega aquí el token y el Query ID. El token se guarda cifrado en tu Mac (en la base de datos
        cuya clave está en el Llavero), no vuelve a mostrarse y sólo se usa para pedir tus extractos
        a Interactive Brokers cuando pulsas «Sincronizar».
      </li>
    </ol>
  );
}

/** Qué informe descargar en xStation 5 (PDF o Excel). */
function PasosXtb() {
  return (
    <ul className="mt-2 list-disc space-y-1 pl-5 text-stone-700">
      <li>
        XTB cerró su API el 14 de marzo de 2025: sus operaciones se traen con el informe que exporta
        xStation 5, en PDF o en Excel.
      </li>
      <li>
        Excel: en xStation 5, <strong>Historial de la cuenta → Exportar → Excel</strong>. Sale un
        fichero por cuenta (una por divisa: <span className="font-mono">EUR_…xlsx</span>,{' '}
        <span className="font-mono">USD_…xlsx</span>); importa los dos. Las compras de antes del
        periodo que no estén ya importadas se toman de las posiciones (y se avisa); las de una
        cuenta en dólares se pasan a euros con el tipo del BCE; las opciones van en la línea de
        derivados.
      </li>
      <li>
        En xStation 5 (web o escritorio): <strong>Historial de la cuenta</strong> → elige el periodo
        (el año o «Todo») → <strong>Exportar</strong> → PDF. Trae las posiciones cerradas, las
        abiertas y las operaciones en efectivo (compras, ventas, dividendos, retenciones e
        intereses).
      </li>
      <li>
        Si sólo exportas las posiciones cerradas, faltan las compras de lo que sigue abierto y los
        dividendos: importa también el informe completo (lo repetido no se duplica).
      </li>
      <li>
        El precio a 31 de diciembre sale del informe exportado ese día; si no, escríbelo en «Precios
        de cierre».
      </li>
    </ul>
  );
}

function estadoConector(c: CuentaView): string {
  switch (c.conector) {
    case 'api':
      return `Conectada por API (Flex Web Service) · Query ID ${c.flex_query}`;
    case 'fichero':
      return 'Sin conectar: importa el Flex a mano o conéctala por API';
    case 'pdf':
      return 'Por el informe en PDF o Excel (XTB no tiene API)';
    default:
      return 'Operaciones a mano o por fichero';
  }
}

type OpForm = {
  date: string;
  kind: TipoOpValores;
  account_id: string;
  to_account_id: string;
  security_id: string;
  qty: string;
  price: string;
  currency: string;
  amount: string;
  fee: string;
  fx: string;
  country: string;
  note: string;
};

function opVacia(year: number, cuenta: number | undefined, valor: number | undefined): OpForm {
  // La fecha de hoy en la hora local (con toISOString sería la de UTC: de madrugada, ayer).
  const d = new Date();
  const hoy = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  return {
    date: hoy.startsWith(String(year)) ? hoy : `${year}-12-31`,
    kind: 'compra',
    account_id: cuenta ? String(cuenta) : '',
    to_account_id: '',
    security_id: valor ? String(valor) : '',
    qty: '',
    price: '',
    currency: 'EUR',
    amount: '',
    fee: '',
    fx: '',
    country: '',
    note: '',
  };
}

/** Inversiones en valores: cuentas y conectores, posiciones, resultado, dividendos, 720 y asientos. */
export function Inversiones({ year: initialYear }: { year: number }) {
  const [year, setYear] = useState(initialYear);
  const [report, setReport] = useState<ValoresReport | null>(null);
  const [ops, setOps] = useState<OpValores[]>([]);
  const [securities, setSecurities] = useState<Valor[]>([]);
  const [prices, setPrices] = useState<PrecioCierre[]>([]);
  const [settings, setSettings] = useState<ValoresSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  // La sincronización va aparte de `busy`: otra orden que acabe mientras tanto no la desbloquea.
  const [sincronizando, setSincronizando] = useState(false);
  const sincronizandoRef = useRef(false);
  const [lectura, setLectura] = useState<LecturaValores | null>(null);
  const [conectar, setConectar] = useState<{ id: number; token: string; query: string } | null>(
    null,
  );
  const [cuentaNueva, setCuentaNueva] = useState<CuentaBroker | null>(null);
  const [opForm, setOpForm] = useState<OpForm | null>(null);
  const [nuevoValor, setNuevoValor] = useState<Valor | null>(null);
  const [precio, setPrecio] = useState({
    date: `${initialYear}-12-31`,
    security_id: '',
    price: '',
  });

  const load = useCallback(() => {
    Promise.all([valoresReport(year), valoresOps(year), valoresSecurities(), valoresPrices()])
      .then(([r, o, s, p]) => {
        setReport(r);
        setSettings(r.settings);
        setOps(o);
        setSecurities(s);
        setPrices(p);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);

  /** Ejecuta una orden, dice cómo ha ido y recarga; devuelve si ha ido bien. */
  const run = async (
    etiqueta: string,
    p: Promise<unknown>,
    ok: string | ((v: unknown) => string),
  ): Promise<boolean> => {
    setBusy(etiqueta);
    setMsg(null);
    try {
      const v = await p;
      setError(null);
      setMsg(typeof ok === 'function' ? ok(v) : ok);
      load();
      return true;
    } catch (e) {
      setError(errorMessage(e));
      return false;
    } finally {
      setBusy(null);
    }
  };

  const importarFicheros = async () => {
    const paths = await pickValoresFiles();
    if (paths.length === 0) return;
    setBusy('leer');
    try {
      setLectura(await valoresReadFiles(paths));
      setError(null);
      setMsg(null);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const exportar = async (pdf: boolean) => {
    const dir = await pickFolder();
    if (!dir) return;
    await run('exportar', valoresExport(year, dir, pdf), (p) => `Guardado en ${String(p)}.`);
  };

  const guardarOp = async () => {
    if (!opForm) return;
    const t = opForm.kind;
    const conTitulos = t === 'compra' || t === 'venta' || t === 'traspaso' || t === 'split';
    const qty = conTitulos ? parseDecimalEs(opForm.qty, 18) : '0';
    if (conTitulos && (!qty || (t !== 'split' && qty.startsWith('-'))))
      return setError('Revisa los títulos.');
    const amount = parseEuros(opForm.amount || '0');
    const fee = parseEuros(opForm.fee || '0');
    if (amount === null || fee === null) return setError('Revisa el importe o la comisión.');
    const currency = opForm.currency.trim().toUpperCase() || 'EUR';
    const price = opForm.price.trim() ? parseDecimalEs(opForm.price, 8) : null;
    if (opForm.price.trim() && (!price || price.startsWith('-')))
      return setError('Revisa el precio por título.');
    const fx = currency === 'EUR' ? null : parseDecimalEs(opForm.fx, 8);
    if (currency !== 'EUR' && !fx)
      return setError(`Falta el tipo de cambio (euros por 1 ${currency}): pulsa «Tipo del BCE».`);
    if (!opForm.account_id) return setError('Elige la cuenta.');
    const op: OpValores = {
      id: 0,
      account_id: Number(opForm.account_id),
      to_account_id: t === 'traspaso' && opForm.to_account_id ? Number(opForm.to_account_id) : null,
      security_id: opForm.security_id ? Number(opForm.security_id) : null,
      date: opForm.date,
      time: '00:00:00',
      kind: t,
      qty: qty ?? '0',
      price,
      currency,
      amount,
      fee,
      fx,
      eur: 0,
      fee_eur: 0,
      country: opForm.country.trim().toUpperCase(),
      note: opForm.note,
      source: 'manual',
      fingerprint: null,
      entry_id: null,
    };
    if (await run('op', valoresSaveOp(op), 'Operación añadida.')) setOpForm(null);
  };

  const tipoBce = async (currency: string, date: string, aplicar: (v: string) => void) => {
    try {
      const r = await fxBceRate(currency, date);
      const v = eurPorUnidad(r.rate);
      if (v) aplicar(v);
      setMsg(
        `Tipo del BCE de ${currency} del ${dia(r.date)}: 1 € = ${r.rate.replace('.', ',')} ${currency}.`,
      );
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const r = report;
  const deOps = r ? r.asientos.filter((a) => a.op !== null).length : 0;
  const grabados = r ? r.asientos.filter((a) => a.entry_id !== null).length : 0;
  const years = [initialYear + 1, initialYear, initialYear - 1, initialYear - 2, initialYear - 3];
  const cuentas = r?.cuentas ?? [];
  const nombreValor = (id: number | null) =>
    securities.find((s) => s.id === id)?.ticker ?? (id ? `valor ${id}` : '');
  const nombreCuenta = (id: number) => cuentas.find((c) => c.id === id)?.name ?? '';
  const sinCambio = [
    ...new Set((r?.posiciones ?? []).filter((p) => p.precio && !p.cambio).map((p) => p.divisa)),
  ];

  return (
    <section aria-labelledby="valores-h" className="mt-4 text-sm">
      <h3 id="valores-h" className="text-lg font-medium">
        Inversiones en valores
      </h3>
      <p className="text-stone-600">
        Las acciones, ETF y fondos de tus brókeres: lo que tienes y lo que vale, lo que has ganado o
        perdido al vender (con el coste medio ponderado de cada valor), dividendos y retenciones en
        origen, el modelo 720 y los asientos. Sólo se conecta a internet para pedir tus extractos a
        Interactive Brokers cuando pulsas «Sincronizar».
      </p>
      <label className="mt-3 block">
        Ejercicio
        <select
          className="ml-2"
          value={year}
          onChange={(e) => {
            const y = Number(e.target.value);
            setYear(y);
            setPrecio((p) => ({ ...p, date: `${y}-12-31` }));
          }}
        >
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
      <p role="status" className="mt-2">
        {sincronizando ? 'Pidiendo el extracto a Interactive Brokers…' : msg}
      </p>

      {/* ---------- Cuentas y conectores ---------- */}
      <h4 className="mt-4 font-medium">Cuentas y conectores</h4>
      {cuentas.length === 0 && (
        <p className="text-stone-600">
          Aún no hay cuentas. Añade la de tu bróker o importa un fichero: la cuenta se crea sola.
        </p>
      )}
      <ul aria-label="Cuentas de bróker" className="mt-2 space-y-2">
        {cuentas.map((c) => (
          <li key={c.id} className="rounded border border-stone-200 p-3">
            <p className="font-medium">
              {c.name}
              <span className="ml-2 text-xs text-stone-500">
                {BROKER_LABEL[c.broker]}
                {c.number && ` · cuenta ${c.number}`}
                {c.country && ` · ${c.country}`}
                {c.abroad && ' · en el extranjero (720)'}
              </span>
            </p>
            <p className="text-stone-600">
              {estadoConector(c)} · {plural(c.ops, 'operación', 'operaciones')}
            </p>
            {c.last_sync && (
              <p className="text-stone-600">
                Última importación: {dia(c.last_sync)} — {c.last_sync_note}
              </p>
            )}
            <div className="mt-2 flex flex-wrap gap-2">
              {c.broker === 'ibkr' && c.token && (
                <button
                  type="button"
                  className="rounded btn-primary px-3 py-1"
                  disabled={busy !== null || sincronizando}
                  onClick={async () => {
                    if (sincronizandoRef.current) return;
                    sincronizandoRef.current = true;
                    setSincronizando(true);
                    try {
                      await run(
                        'sync',
                        valoresIbkrSync(c.id, year),
                        (a) => `Sincronizado ${year}: ${resumenAplicado(a as AplicadoValores)}`,
                      );
                    } finally {
                      sincronizandoRef.current = false;
                      setSincronizando(false);
                    }
                  }}
                >
                  {sincronizando ? 'Sincronizando…' : `Sincronizar ${year}`}
                </button>
              )}
              {c.broker === 'ibkr' && (
                <button
                  type="button"
                  className={boton}
                  onClick={() => setConectar({ id: c.id, token: '', query: c.flex_query })}
                >
                  {c.token ? 'Cambiar el token…' : 'Conectar por API…'}
                </button>
              )}
              {c.broker === 'ibkr' && c.token && (
                <ConfirmAction
                  label="Desconectar"
                  question={`¿Borrar el token del Flex Web Service de «${c.name}»? Las operaciones ya importadas se quedan.`}
                  confirmLabel="Sí, desconectar"
                  className={boton}
                  onConfirm={async () => {
                    await run('desconectar', valoresIbkrDisconnect(c.id), 'Token borrado.');
                  }}
                />
              )}
              <button
                type="button"
                className={boton}
                onClick={importarFicheros}
                disabled={busy !== null}
              >
                {c.broker === 'xtb'
                  ? 'Importar informe de XTB (PDF o Excel)…'
                  : 'Importar fichero…'}
              </button>
              {c.ops === 0 && (
                <ConfirmAction
                  label="Quitar la cuenta"
                  question={`¿Quitar la cuenta «${c.name}»?`}
                  confirmLabel="Sí, quitar"
                  onConfirm={async () => {
                    await run('cuenta', valoresDeleteAccount(c.id), 'Cuenta quitada.');
                  }}
                />
              )}
            </div>
            {c.broker === 'xtb' && (
              <details className="mt-2">
                <summary className="cursor-pointer text-stone-600">
                  Qué informe descargar en xStation 5
                </summary>
                <PasosXtb />
              </details>
            )}
            {conectar?.id === c.id && (
              <form
                aria-label={`Conectar ${c.name} por API`}
                className="mt-3 rounded border border-stone-200 bg-stone-50 p-3"
                onSubmit={(e) => {
                  e.preventDefault();
                  run(
                    'conectar',
                    valoresIbkrConnect(c.id, conectar.token, conectar.query),
                    'Conectada: ya puedes sincronizar.',
                  ).then((ok) => ok && setConectar(null));
                }}
              >
                <p className="font-medium">Conectar Interactive Brokers (Flex Web Service)</p>
                <PasosIbkr />
                <div className="mt-3 grid gap-2 sm:grid-cols-2">
                  <label>
                    Token del Flex Web Service
                    <input
                      type="password"
                      autoComplete="off"
                      className="mt-1 block w-full font-mono"
                      value={conectar.token}
                      onChange={(e) => setConectar({ ...conectar, token: e.target.value.trim() })}
                      required
                    />
                  </label>
                  <label>
                    Query ID de la Flex Query
                    <input
                      inputMode="numeric"
                      className="mt-1 block w-full font-mono"
                      value={conectar.query}
                      onChange={(e) => setConectar({ ...conectar, query: e.target.value.trim() })}
                      required
                    />
                  </label>
                </div>
                <div className="mt-3 flex gap-2">
                  <button type="submit" className="rounded btn-primary px-3 py-1">
                    Guardar y conectar
                  </button>
                  <button type="button" className={boton} onClick={() => setConectar(null)}>
                    Cancelar
                  </button>
                </div>
              </form>
            )}
          </li>
        ))}
      </ul>
      <div className="mt-2 flex flex-wrap gap-2">
        {(['ibkr', 'xtb', 'otro'] as Broker[]).map((b) => (
          <button
            key={b}
            type="button"
            className={boton}
            onClick={() => setCuentaNueva(nuevaCuenta(b))}
          >
            {b === 'otro' ? 'Añadir otra cuenta' : `Añadir cuenta de ${BROKER_LABEL[b]}`}
          </button>
        ))}
        <button type="button" className={boton} onClick={importarFicheros} disabled={busy !== null}>
          Importar informe de XTB (PDF o Excel) o Flex de IBKR…
        </button>
      </div>
      {cuentaNueva && (
        <form
          aria-label="Cuenta nueva"
          className="mt-3 grid grid-cols-2 gap-2 rounded border border-stone-200 p-3 sm:grid-cols-4"
          onSubmit={(e) => {
            e.preventDefault();
            run('cuenta', valoresSaveAccount(cuentaNueva), 'Cuenta añadida.').then(
              (ok) => ok && setCuentaNueva(null),
            );
          }}
        >
          <label>
            Nombre
            <input
              className="mt-1 block w-full"
              value={cuentaNueva.name}
              onChange={(e) => setCuentaNueva({ ...cuentaNueva, name: e.target.value })}
              required
            />
          </label>
          <label>
            Número de cuenta
            <input
              className="mt-1 block w-full font-mono"
              value={cuentaNueva.number}
              onChange={(e) => setCuentaNueva({ ...cuentaNueva, number: e.target.value })}
            />
          </label>
          <label>
            País del depositario
            <input
              className="mt-1 block w-full uppercase"
              maxLength={2}
              value={cuentaNueva.country}
              onChange={(e) =>
                setCuentaNueva({ ...cuentaNueva, country: e.target.value.toUpperCase() })
              }
            />
          </label>
          <label className="flex items-end gap-2">
            <input
              type="checkbox"
              checked={cuentaNueva.abroad}
              onChange={(e) => setCuentaNueva({ ...cuentaNueva, abroad: e.target.checked })}
            />
            En el extranjero (cuenta para el 720)
          </label>
          <div className="col-span-full flex gap-2">
            <button type="submit" className="rounded btn-primary px-3 py-1">
              Añadir la cuenta
            </button>
            <button type="button" className={boton} onClick={() => setCuentaNueva(null)}>
              Cancelar
            </button>
          </div>
        </form>
      )}

      {/* ---------- Vista previa de lo importado ---------- */}
      {lectura && (
        <section
          aria-label="Lo leído de los ficheros"
          className="mt-3 rounded border border-stone-200 bg-stone-50 p-3"
        >
          <p className="font-medium">Lo leído</p>
          <ul className="mt-1 space-y-1">
            {lectura.ficheros.map((f) => (
              <li key={`${f.nombre}-${f.cuenta}`}>
                <span className="font-mono">{f.nombre}</span>
                {f.error ? (
                  <span className="ml-2 text-red-700">no se puede leer: {f.error}</span>
                ) : (
                  <span className="ml-2 text-stone-700">
                    {f.formato} · cuenta {f.cuenta} · {f.resumen}
                    {f.desde && f.hasta && ` · ${dia(f.desde)} – ${dia(f.hasta)}`}
                  </span>
                )}
              </li>
            ))}
          </ul>
          <p className="mt-2">
            {plural(lectura.nuevas, 'operación nueva', 'operaciones nuevas')}
            {lectura.duplicadas > 0 && ` y ${lectura.duplicadas} que ya estaban (no se duplican)`}
            {lectura.precios > 0 && ` · ${plural(lectura.precios, 'precio', 'precios')}`}.
          </p>
          {lectura.avisos.length > 0 && (
            <ul aria-label="Avisos de la lectura" className="mt-1 list-disc pl-5 text-amber-900">
              {lectura.avisos.map((a) => (
                <li key={a}>{a}</li>
              ))}
            </ul>
          )}
          <div className="mt-2 flex gap-2">
            <button
              type="button"
              className="rounded btn-primary px-3 py-1"
              disabled={busy !== null || lectura.extractos.length === 0}
              onClick={() =>
                run(
                  'importar',
                  valoresImport(lectura),
                  (a) => `Importado: ${resumenAplicado(a as AplicadoValores)}`,
                ).then((ok) => ok && setLectura(null))
              }
            >
              Importar
            </button>
            <button type="button" className={boton} onClick={() => setLectura(null)}>
              Cancelar
            </button>
          </div>
        </section>
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
      {r && r.sin_euros > 0 && (
        <div className="mt-2">
          <ConfirmAction
            label={`Pasar a euros con el BCE (${plural(r.sin_euros, 'operación', 'operaciones')})`}
            question="Se descargan del BCE los tipos de referencia de las fechas de esas operaciones (sólo se envían la divisa y las fechas) y se pasan a euros. ¿Seguir?"
            confirmLabel="Sí, descargar del BCE"
            className={boton}
            onConfirm={async () => {
              await run('bce', valoresBce(), (p) => {
                const x = p as PasoBce;
                return x.divisas.length === 0
                  ? 'No hay operaciones que pasar a euros con el BCE.'
                  : `Tipos del BCE de ${x.divisas.join(', ')}: ${plural(x.operaciones, 'operación pasada', 'operaciones pasadas')} a euros.`;
              });
            }}
          />
        </div>
      )}

      {r && (
        <>
          {/* ---------- Posiciones ---------- */}
          <h4 className="mt-5 font-medium">Posiciones a {dia(r.fecha)}</h4>
          {r.posiciones.length === 0 ? (
            <p className="text-stone-600">Sin valores a esa fecha.</p>
          ) : (
            <table className="mt-1 w-full">
              <thead className="text-left text-stone-500">
                <tr>
                  <th>Valor</th>
                  <th>Cuenta</th>
                  <th className={cell}>Títulos</th>
                  <th className={cell}>Coste</th>
                  <th className={cell}>Precio de cierre</th>
                  <th className={cell}>Valor</th>
                  <th className={cell}>Plusvalía latente</th>
                </tr>
              </thead>
              <tbody>
                {r.posiciones.map((p) => (
                  <tr key={`${p.security_id}-${p.account_id}`}>
                    <td>
                      <span className="font-mono">{p.ticker}</span>
                      <span className="ml-1 text-xs text-stone-500">
                        {CLASE_LABEL[p.kind]}
                        {p.isin && ` · ${p.isin}`}
                      </span>
                    </td>
                    <td>
                      {p.account}
                      {p.abroad && (
                        <span className="ml-1 text-xs text-stone-500">(extranjero)</span>
                      )}
                    </td>
                    <td className={cell}>{criptoQty(p.cantidad)}</td>
                    <td className={cell}>{euros(p.coste)}</td>
                    <td className={cell}>
                      {p.precio ? `${criptoQty(p.precio)} ${p.divisa}` : '—'}
                      {p.precio_fecha && (
                        <span className="ml-1 text-xs text-stone-500">({dia(p.precio_fecha)})</span>
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
                  <td className={cell}>{euros(r.latente_total)}</td>
                </tr>
              </tbody>
            </table>
          )}
          {sinCambio.map((d) => (
            <button
              key={d}
              type="button"
              className={`${boton} mt-2 mr-2`}
              onClick={async () => {
                await tipoBce(d, r.fecha, () => undefined);
                load();
              }}
            >
              Tipo del BCE de {d} a {dia(r.fecha)}
            </button>
          ))}

          {/* ---------- Resultado ---------- */}
          <h4 className="mt-5 font-medium">Resultado de {year}</h4>
          <table className="mt-1">
            <tbody>
              {(
                [
                  ['Ganancias en ventas', r.resultado.ganancias],
                  ['Pérdidas en ventas', r.resultado.perdidas],
                  ['Neto de las ventas', r.resultado.neto],
                  ...(r.resultado.ventas_derivados > 0
                    ? ([['Derivados (opciones)', r.resultado.derivados]] as const)
                    : []),
                  ['Dividendos íntegros', r.resultado.dividendos],
                  ['Retenciones en origen', r.resultado.retenciones_extranjero],
                  ['Retenciones españolas', r.resultado.retenciones_espana],
                  ['Intereses', r.resultado.intereses],
                  ['Comisiones a resultados', r.resultado.comisiones],
                ] as const
              ).map(([label, v]) => (
                <tr key={label} className={label === 'Neto de las ventas' ? 'font-medium' : ''}>
                  <th scope="row" className="pr-4 text-left font-normal">
                    {label}
                  </th>
                  <td className={cell}>{euros(v)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="text-xs text-stone-500">
            {r.metodo === 'fifo'
              ? 'FIFO (persona física, art. 37.2 LIRPF).'
              : 'Coste medio ponderado por valor (NRV 9.ª del PGC).'}{' '}
            {plural(r.resultado.ventas, 'venta', 'ventas')} en {year}.
          </p>
          {r.realizados.length > 0 && (
            <details className="mt-1">
              <summary className="cursor-pointer text-stone-600">Ver cada venta</summary>
              <table className="mt-1 w-full">
                <thead className="text-left text-stone-500">
                  <tr>
                    <th>Fecha</th>
                    <th>Valor</th>
                    <th>Cuenta</th>
                    <th className={cell}>Títulos</th>
                    <th className={cell}>Valor de transmisión</th>
                    <th className={cell}>Coste</th>
                    <th className={cell}>Resultado</th>
                  </tr>
                </thead>
                <tbody>
                  {r.realizados.map((x) => (
                    <tr key={x.op}>
                      <td>{dia(x.fecha)}</td>
                      <td className="font-mono">
                        {x.ticker}
                        {x.derivado && (
                          <span className="ml-1 font-sans text-xs text-stone-500">(derivado)</span>
                        )}
                      </td>
                      <td>{x.account}</td>
                      <td className={cell}>{criptoQty(x.cantidad)}</td>
                      <td className={cell}>{euros(x.transmision)}</td>
                      <td className={cell}>{euros(x.coste)}</td>
                      <td className={`${cell} ${x.resultado < 0 ? 'text-red-700' : ''}`}>
                        {euros(x.resultado)}
                        {x.dos_meses && ' *'}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </details>
          )}

          {/* ---------- Dividendos y retenciones ---------- */}
          <h4 className="mt-5 font-medium">Dividendos y retenciones en origen</h4>
          {r.doble_imposicion.paises.length > 0 && (
            <table className="mt-1 w-full">
              <thead className="text-left text-stone-500">
                <tr>
                  <th>País</th>
                  <th className={cell}>Dividendos</th>
                  <th className={cell}>Retenido</th>
                  <th className={cell}>Convenio</th>
                  <th className={cell}>Cuota española</th>
                  <th className={cell}>Deducible</th>
                  <th className={cell}>Exceso a reclamar</th>
                </tr>
              </thead>
              <tbody>
                {r.doble_imposicion.paises.map((p) => (
                  <tr key={p.pais}>
                    <td>{p.pais}</td>
                    <td className={cell}>{euros(p.rentas)}</td>
                    <td className={cell}>{euros(p.retenido)}</td>
                    <td className={cell}>
                      {p.convenio_pct === null
                        ? 'sin datos'
                        : `${p.convenio_pct} %${p.convenio_comprobado ? '' : ' (a comprobar)'}`}
                    </td>
                    <td className={cell}>{euros(p.cuota_espanola)}</td>
                    <td className={cell}>{euros(p.deducible)}</td>
                    <td className={cell}>{euros(p.exceso)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <p className="mt-1 text-stone-600">{r.doble_imposicion.nota}</p>

          {/* ---------- 720 ---------- */}
          <h4 className="mt-5 font-medium">Modelo 720</h4>
          <p
            className={`mt-1 rounded p-3 ${r.m720.supera ? 'bg-amber-50 text-amber-900' : 'bg-stone-50 text-stone-700'}`}
          >
            {r.m720.nota}
          </p>
          <p className="mt-1 text-xs text-stone-500">
            Valores en el extranjero a {dia(r.m720.fecha)}: {euros(r.m720.total_extranjero)} (umbral{' '}
            {euros(r.m720.umbral)}). El 721 es sólo de monedas virtuales.
          </p>
        </>
      )}

      {/* ---------- Operaciones ---------- */}
      <h4 className="mt-5 font-medium">Operaciones de {year}</h4>
      <div className="mt-1 flex flex-wrap gap-2">
        <button
          type="button"
          className="rounded btn-primary px-3 py-1"
          disabled={cuentas.length === 0}
          onClick={() => setOpForm(opVacia(year, cuentas[0]?.id, securities[0]?.id))}
        >
          Nueva operación
        </button>
        <button type="button" className={boton} onClick={() => exportar(false)}>
          Exportar a Excel…
        </button>
        <button type="button" className={boton} onClick={() => exportar(true)}>
          Exportar a PDF…
        </button>
      </div>
      {opForm && (
        <form
          aria-label="Operación"
          className="mt-3 grid grid-cols-2 gap-2 rounded border border-stone-200 p-3 sm:grid-cols-4"
          onSubmit={(e) => {
            e.preventDefault();
            guardarOp();
          }}
        >
          <label>
            Fecha
            <input
              type="date"
              className="mt-1 block w-full"
              value={opForm.date}
              onChange={(e) => setOpForm({ ...opForm, date: e.target.value })}
              required
            />
          </label>
          <label>
            Tipo
            <select
              className="mt-1 block w-full"
              value={opForm.kind}
              onChange={(e) => setOpForm({ ...opForm, kind: e.target.value as TipoOpValores })}
            >
              {TIPOS_OP.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.label}
                </option>
              ))}
            </select>
          </label>
          <label>
            Cuenta
            <select
              className="mt-1 block w-full"
              value={opForm.account_id}
              onChange={(e) => setOpForm({ ...opForm, account_id: e.target.value })}
            >
              {cuentas.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </label>
          {opForm.kind === 'traspaso' && (
            <label>
              Cuenta de destino
              <select
                className="mt-1 block w-full"
                value={opForm.to_account_id}
                onChange={(e) => setOpForm({ ...opForm, to_account_id: e.target.value })}
              >
                <option value="">—</option>
                {cuentas.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label>
            Valor
            <select
              className="mt-1 block w-full"
              value={opForm.security_id}
              onChange={(e) => setOpForm({ ...opForm, security_id: e.target.value })}
            >
              <option value="">— (sin valor)</option>
              {securities.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.ticker}
                  {s.isin && ` · ${s.isin}`}
                </option>
              ))}
            </select>
          </label>
          <label>
            Títulos
            <input
              inputMode="decimal"
              className="mt-1 block w-full"
              value={opForm.qty}
              onChange={(e) => setOpForm({ ...opForm, qty: e.target.value })}
            />
          </label>
          <label>
            Precio por título
            <input
              inputMode="decimal"
              className="mt-1 block w-full"
              value={opForm.price}
              onChange={(e) => setOpForm({ ...opForm, price: e.target.value })}
            />
          </label>
          <label>
            Divisa
            <input
              className="mt-1 block w-full uppercase"
              maxLength={3}
              value={opForm.currency}
              onChange={(e) => setOpForm({ ...opForm, currency: e.target.value.toUpperCase() })}
            />
          </label>
          <label>
            Importe (sin comisión)
            <input
              inputMode="decimal"
              className="mt-1 block w-full"
              value={opForm.amount}
              onChange={(e) => setOpForm({ ...opForm, amount: e.target.value })}
            />
          </label>
          <label>
            Comisión
            <input
              inputMode="decimal"
              className="mt-1 block w-full"
              value={opForm.fee}
              onChange={(e) => setOpForm({ ...opForm, fee: e.target.value })}
            />
          </label>
          {opForm.currency !== 'EUR' && opForm.currency.length === 3 && (
            <div>
              <label>
                Euros por 1 {opForm.currency}
                <input
                  inputMode="decimal"
                  className="mt-1 block w-full font-mono"
                  value={opForm.fx}
                  onChange={(e) => setOpForm({ ...opForm, fx: e.target.value })}
                />
              </label>
              <button
                type="button"
                className={`${boton} mt-1`}
                onClick={() =>
                  tipoBce(opForm.currency, opForm.date, (v) =>
                    setOpForm((f) => (f ? { ...f, fx: v.replace('.', ',') } : f)),
                  )
                }
              >
                Tipo del BCE
              </button>
            </div>
          )}
          {(opForm.kind === 'retencion' || opForm.kind === 'dividendo') && (
            <label>
              País (retención en origen)
              <input
                className="mt-1 block w-full uppercase"
                maxLength={2}
                value={opForm.country}
                onChange={(e) => setOpForm({ ...opForm, country: e.target.value.toUpperCase() })}
              />
            </label>
          )}
          <label className="col-span-2">
            Nota
            <input
              className="mt-1 block w-full"
              value={opForm.note}
              onChange={(e) => setOpForm({ ...opForm, note: e.target.value })}
            />
          </label>
          <div className="col-span-full flex gap-2">
            <button
              type="submit"
              className="rounded btn-primary px-3 py-1"
              disabled={busy !== null}
            >
              Añadir operación
            </button>
            <button type="button" className={boton} onClick={() => setOpForm(null)}>
              Cancelar
            </button>
          </div>
        </form>
      )}
      {ops.length === 0 ? (
        <p className="mt-1 text-stone-600">Sin operaciones en {year}.</p>
      ) : (
        <table className="mt-2 w-full">
          <thead className="text-left text-stone-500">
            <tr>
              <th>Fecha</th>
              <th>Tipo</th>
              <th>Cuenta</th>
              <th>Valor</th>
              <th className={cell}>Títulos</th>
              <th className={cell}>Precio</th>
              <th className={cell}>Importe</th>
              <th className={cell}>Euros</th>
              <th>Origen</th>
              <th>
                <span className="sr-only">Acciones</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {ops.map((o) => (
              <tr key={o.id}>
                <td>{dia(o.date)}</td>
                <td>{tipoOpLabel(o.kind)}</td>
                <td>{nombreCuenta(o.account_id)}</td>
                <td className="font-mono">{nombreValor(o.security_id)}</td>
                <td className={cell}>{o.qty === '0' ? '' : criptoQty(o.qty)}</td>
                <td className={cell}>{o.price ? criptoQty(o.price) : ''}</td>
                <td className={cell}>
                  {o.currency === 'EUR' ? '' : euros(o.amount).replace('€', o.currency)}
                </td>
                <td className={cell}>{euros(o.eur)}</td>
                <td className="text-xs text-stone-500">{o.source}</td>
                <td>
                  <ConfirmAction
                    label="Borrar"
                    ariaLabel={`Borrar: ${tipoOpLabel(o.kind)} del ${dia(o.date)}`}
                    question={`¿Borrar la operación «${tipoOpLabel(o.kind)}» del ${dia(o.date)}? Si vuelves a importar el mismo fichero, volverá a entrar.`}
                    confirmLabel="Sí, borrar"
                    onConfirm={async () => {
                      await run('borrar', valoresDeleteOp(o.id), 'Operación borrada.');
                    }}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {/* ---------- Precios de cierre ---------- */}
      <details className="mt-5">
        <summary className="cursor-pointer font-medium">
          Precios de cierre ({prices.length})
        </summary>
        <p className="mt-1 text-stone-600">
          El precio de cada valor a 31 de diciembre (en su divisa) valora las posiciones y el 720.
          Los de IBKR llegan al sincronizar; los de XTB, con el informe exportado ese día.
        </p>
        <form
          aria-label="Precio de cierre"
          className="mt-2 flex flex-wrap items-end gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            const v = securities.find((s) => String(s.id) === precio.security_id);
            const p = parseDecimalEs(precio.price, 8);
            if (!v || !p || p.startsWith('-'))
              return setError('Elige el valor y escribe el precio.');
            run(
              'precio',
              valoresSetPrices([
                {
                  date: precio.date,
                  security_id: v.id,
                  price: p,
                  currency: v.currency,
                  source: 'manual',
                },
              ]),
              'Precio guardado.',
            );
          }}
        >
          <label>
            Fecha
            <input
              type="date"
              className="ml-1"
              value={precio.date}
              onChange={(e) => setPrecio({ ...precio, date: e.target.value })}
            />
          </label>
          <label>
            Valor
            <select
              className="ml-1"
              value={precio.security_id}
              onChange={(e) => setPrecio({ ...precio, security_id: e.target.value })}
            >
              <option value="">—</option>
              {securities.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.ticker} ({s.currency})
                </option>
              ))}
            </select>
          </label>
          <label>
            Precio
            <input
              inputMode="decimal"
              className="ml-1 w-28"
              value={precio.price}
              onChange={(e) => setPrecio({ ...precio, price: e.target.value })}
            />
          </label>
          <button type="submit" className={boton}>
            Guardar el precio
          </button>
        </form>
        {prices.length > 0 && (
          <table className="mt-2">
            <thead className="text-left text-stone-500">
              <tr>
                <th>Fecha</th>
                <th>Valor</th>
                <th className={cell}>Precio</th>
                <th>Origen</th>
                <th>
                  <span className="sr-only">Acciones</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {prices.map((p) => (
                <tr key={`${p.date}-${p.security_id}`}>
                  <td>{dia(p.date)}</td>
                  <td className="font-mono">{nombreValor(p.security_id)}</td>
                  <td className={cell}>
                    {criptoQty(p.price)} {p.currency}
                  </td>
                  <td className="text-xs text-stone-500">{p.source}</td>
                  <td>
                    <button
                      type="button"
                      className="text-red-700"
                      aria-label={`Borrar el precio de ${nombreValor(p.security_id)} del ${dia(p.date)}`}
                      onClick={() =>
                        run('precio', valoresDeletePrice(p.date, p.security_id), 'Precio borrado.')
                      }
                    >
                      Borrar
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </details>

      {/* ---------- Valores ---------- */}
      <details className="mt-3">
        <summary className="cursor-pointer font-medium">Valores ({securities.length})</summary>
        <p className="mt-1 text-stone-600">
          Pon el ISIN a los valores de XTB (el PDF sólo trae el ticker): así el mismo ETF en dos
          brókeres es un solo valor, con un solo coste medio.
        </p>
        <ul className="mt-2 space-y-2">
          {securities.map((s) => (
            <li key={s.id}>
              <ValorEditable
                valor={s}
                onSave={(v) => run('valor', valoresSaveSecurity(v), 'Valor guardado.')}
              />
            </li>
          ))}
        </ul>
        {nuevoValor ? (
          <div className="mt-2">
            <ValorEditable
              valor={nuevoValor}
              onSave={(v) =>
                run('valor', valoresSaveSecurity(v), 'Valor añadido.').then(
                  (ok) => ok && setNuevoValor(null),
                )
              }
            />
          </div>
        ) : (
          <button
            type="button"
            className={`${boton} mt-2`}
            onClick={() =>
              setNuevoValor({
                id: 0,
                isin: '',
                ticker: '',
                name: '',
                currency: 'EUR',
                kind: 'etf',
                country: '',
                account: '',
              })
            }
          >
            Añadir un valor
          </button>
        )}
      </details>

      {/* ---------- Asientos ---------- */}
      {r && !r.persona_fisica && (
        <details className="mt-3">
          <summary className="cursor-pointer font-medium">
            Asientos ({grabados} de {deOps} grabados)
          </summary>
          <p className="mt-1 text-stone-600">
            «Contabilizar» graba en el diario los asientos de las operaciones del ejercicio
            (compras, ventas con su resultado, dividendos íntegros, retenciones, intereses y
            comisiones) como un lote que se puede deshacer. Cada valor va en su subcuenta, lo que lo
            deja registrado de forma individualizada. Mientras una operación tenga asiento no se
            puede cambiar ni borrar. El ajuste del cierre (valor razonable o deterioro) se graba en
            el cierre guiado, paso «Cartera de valores».
          </p>
          <div className="mt-2 flex flex-wrap gap-2">
            {deOps > grabados && (
              <ConfirmAction
                label="Contabilizar"
                question={`¿Grabar en el diario los ${deOps - grabados} asientos de ${year} que faltan?`}
                confirmLabel="Sí, contabilizar"
                danger={false}
                className="btn-primary rounded-md px-3 py-1.5"
                onConfirm={async () => {
                  await run('contabilizar', valoresPostEntries(year, false), (v) =>
                    resumenContabilizado(v as ContabilizadoValores),
                  );
                }}
              />
            )}
            {grabados > 0 && (
              <>
                <ConfirmAction
                  label="Rehacer"
                  question={`¿Borrar los ${grabados} asientos de ${year} y volver a generarlos con los datos de ahora?`}
                  confirmLabel="Sí, rehacer"
                  className={boton}
                  onConfirm={async () => {
                    await run('contabilizar', valoresPostEntries(year, true), (v) =>
                      resumenContabilizado(v as ContabilizadoValores),
                    );
                  }}
                />
                <ConfirmAction
                  label="Deshacer"
                  ariaLabel="Deshacer los asientos de la cartera"
                  question={`¿Quitar del diario los ${grabados} asientos de las operaciones de ${year}?`}
                  confirmLabel="Sí, quitarlos"
                  className={boton}
                  onConfirm={async () => {
                    await run(
                      'deshacer',
                      valoresUndoEntries(year),
                      (n) => `${plural(Number(n), 'asiento quitado', 'asientos quitados')}.`,
                    );
                  }}
                />
              </>
            )}
          </div>
          <ul className="mt-2 space-y-2">
            {r.asientos.map((a) => (
              <li
                key={`${a.fecha}-${a.op ?? 'cierre'}-${a.concepto}`}
                className="rounded border border-stone-200 p-2"
              >
                <p>
                  {dia(a.fecha)} · {a.concepto}{' '}
                  {a.entry_id !== null ? (
                    <span className="pill pill-green">Grabado</span>
                  ) : a.op === null ? (
                    <span className="text-stone-500">(se graba en el cierre)</span>
                  ) : null}
                </p>
                <table className="mt-1">
                  <thead className="text-left text-stone-500">
                    <tr>
                      <th>Cuenta</th>
                      <th className={cell}>Debe</th>
                      <th className={cell}>Haber</th>
                    </tr>
                  </thead>
                  <tbody>
                    {a.lineas.map((l) => (
                      <tr key={`${l.cuenta}-${l.debe}-${l.haber}`}>
                        <td className="font-mono">{l.cuenta}</td>
                        <td className={cell}>{l.debe ? euros(l.debe) : ''}</td>
                        <td className={cell}>{l.haber ? euros(l.haber) : ''}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </li>
            ))}
          </ul>
        </details>
      )}

      {/* ---------- Criterios y ajustes ---------- */}
      {r && settings && (
        <details className="mt-3">
          <summary className="cursor-pointer font-medium">Cómo se calcula</summary>
          <ul className="mt-2 list-disc space-y-1 pl-5 text-stone-700">
            {r.notas.map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>
          {!r.persona_fisica && (
            <div className="mt-3 flex flex-wrap items-end gap-3">
              <label>
                Cartera
                <select
                  className="ml-2"
                  value={settings.cartera}
                  onChange={(e) =>
                    setSettings({
                      ...settings,
                      cartera: e.target.value as ValoresSettings['cartera'],
                    })
                  }
                >
                  <option value="coste">A coste (no de negociación)</option>
                  <option value="negociacion">Mantenidos para negociar</option>
                </select>
              </label>
              <label>
                Plazo
                <select
                  className="ml-2"
                  value={settings.plazo}
                  onChange={(e) =>
                    setSettings({ ...settings, plazo: e.target.value as ValoresSettings['plazo'] })
                  }
                >
                  <option value="corto">Corto plazo (540/541)</option>
                  <option value="largo">Largo plazo (250/251)</option>
                </select>
              </label>
              <button
                type="button"
                className={boton}
                aria-label="Guardar los ajustes"
                onClick={() => run('ajustes', valoresSaveSettings(settings), 'Ajustes guardados.')}
              >
                Guardar
              </button>
            </div>
          )}
        </details>
      )}
    </section>
  );
}

function ValorEditable({ valor, onSave }: { valor: Valor; onSave: (v: Valor) => void }) {
  const [v, setV] = useState(valor);
  useEffect(() => setV(valor), [valor]);
  const id = `valor-${valor.id}`;
  return (
    <form
      aria-label={valor.id ? `Valor ${valor.ticker}` : 'Valor nuevo'}
      className="flex flex-wrap items-end gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        onSave(v);
      }}
    >
      <label htmlFor={`${id}-ticker`}>
        Ticker
        <input
          id={`${id}-ticker`}
          className="ml-1 w-28 font-mono"
          value={v.ticker}
          onChange={(e) => setV({ ...v, ticker: e.target.value.toUpperCase() })}
          required
        />
      </label>
      <label htmlFor={`${id}-isin`}>
        ISIN
        <input
          id={`${id}-isin`}
          className="ml-1 w-36 font-mono"
          maxLength={12}
          value={v.isin}
          onChange={(e) => setV({ ...v, isin: e.target.value.toUpperCase() })}
        />
      </label>
      <label htmlFor={`${id}-name`}>
        Nombre
        <input
          id={`${id}-name`}
          className="ml-1 w-48"
          value={v.name}
          onChange={(e) => setV({ ...v, name: e.target.value })}
        />
      </label>
      <label htmlFor={`${id}-kind`}>
        Clase
        <select
          id={`${id}-kind`}
          className="ml-1"
          value={v.kind}
          onChange={(e) => setV({ ...v, kind: e.target.value as Valor['kind'] })}
        >
          {Object.entries(CLASE_LABEL).map(([k, l]) => (
            <option key={k} value={k}>
              {l}
            </option>
          ))}
        </select>
      </label>
      <label htmlFor={`${id}-cur`}>
        Divisa
        <input
          id={`${id}-cur`}
          className="ml-1 w-14 uppercase"
          maxLength={3}
          value={v.currency}
          onChange={(e) => setV({ ...v, currency: e.target.value.toUpperCase() })}
        />
      </label>
      <button type="submit" className={boton}>
        Guardar
      </button>
    </form>
  );
}

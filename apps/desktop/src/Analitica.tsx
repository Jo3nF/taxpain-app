import { ChevronDown } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import {
  type AnalyticLine,
  analyticLineSet,
  analyticLines,
  analyticReport,
  type BudgetLine,
  budgetFromActuals,
  budgetList,
  budgetReport,
  budgetSet,
  type CenterResult,
  type CostCenter,
  type CostRedistribution,
  type CostRule,
  costCenterDelete,
  costCenterSave,
  costCenters,
  costRedistributionSet,
  costRedistributions,
  costRuleSet,
  costRules,
  type Deviation,
  errorMessage,
  euros,
  formatDate,
  parseEuros,
  plural,
  type Share,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { Menu } from './Menu';

const num = 'px-2 text-right tabular-nums';
const MONTHS = ['ene', 'feb', 'mar', 'abr', 'may', 'jun', 'jul', 'ago', 'sep', 'oct', 'nov', 'dic'];

/** «TIENDA 75, WEB 25» → partes en centésimas de punto. */
/** Mensaje de resultado: los errores se anuncian como alerta, los éxitos como estado. */
type Notice = { text: string; error?: boolean | undefined };

function Notice({ msg, className }: { msg: Notice | null; className: string }) {
  if (!msg) return null;
  return msg.error ? (
    <p role="alert" className={`${className} text-red-700`}>
      {msg.text}
    </p>
  ) : (
    <p role="status" className={className}>
      {msg.text}
    </p>
  );
}

export function parseShares(s: string): Share[] | null {
  const parts = s
    .split(/[,;]\s*(?=[A-Za-z])/)
    .map((p) => p.trim())
    .filter(Boolean);
  const out: Share[] = [];
  for (const p of parts) {
    const m = /^([A-Za-z0-9-]+)\s+(\d+(?:[.,]\d{1,2})?)\s*%?$/.exec(p);
    if (!m) return null;
    const [, center, pct] = m as unknown as [string, string, string];
    out.push({
      center: center.toUpperCase(),
      pct_bp: Math.round(Number(pct.replace(',', '.')) * 100),
    });
  }
  return out;
}

const showShares = (s: Share[]) =>
  s.map((x) => `${x.center} ${(x.pct_bp / 100).toLocaleString('es-ES')} %`).join(' · ');

/** Nivel de un centro (1 = sin padre). */
function depth(centers: CostCenter[], code: string): number {
  let n = 1;
  let cur = centers.find((c) => c.code === code)?.parent;
  while (cur && n < 6) {
    n += 1;
    const p = cur;
    cur = centers.find((c) => c.code === p)?.parent;
  }
  return n;
}

/** Centros en orden de árbol (cada uno detrás de su padre). */
function treeOrder(centers: CostCenter[]): CostCenter[] {
  const out: CostCenter[] = [];
  const visit = (parent: string | null) => {
    for (const c of centers.filter((x) => (x.parent ?? null) === parent)) {
      if (out.includes(c)) continue;
      out.push(c);
      visit(c.code);
    }
  };
  visit(null);
  for (const c of centers) if (!out.includes(c)) out.push(c);
  return out;
}

function Centers({
  centers,
  onChange,
}: {
  centers: CostCenter[];
  onChange: (msg: string, error?: boolean) => void;
}) {
  const [code, setCode] = useState('');
  const [name, setName] = useState('');
  const [parent, setParent] = useState('');
  return (
    <section aria-label="Centros de coste">
      <h3 className="font-medium">Centros de coste</h3>
      <ul className="mt-1">
        {treeOrder(centers).map((c) => (
          <li
            key={c.code}
            className="flex items-center gap-3"
            style={{ paddingLeft: `${(depth(centers, c.code) - 1) * 1.25}rem` }}
          >
            <span className="w-24 font-mono">{c.code}</span>
            <span>{c.name}</span>
            <ConfirmAction
              label="Borrar"
              question={`¿Borrar el centro ${c.code}? Se quitan también sus repartos.`}
              confirmLabel="Sí, borrar"
              onConfirm={() =>
                costCenterDelete(c.code, true)
                  .then(() => onChange(`Centro ${c.code} borrado.`))
                  .catch((e) => onChange(errorMessage(e), true))
              }
            />
          </li>
        ))}
      </ul>
      <div className="mt-3 flex flex-wrap items-end gap-3">
        <label>
          Código
          <input
            className="mt-1 block w-32 font-mono"
            value={code}
            onChange={(e) => setCode(e.target.value)}
          />
        </label>
        <label>
          Nombre
          <input
            className="mt-1 block w-56"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        <label>
          Cuelga de
          <select className="mt-1 block" value={parent} onChange={(e) => setParent(e.target.value)}>
            <option value="">— (primer nivel)</option>
            {centers.map((c) => (
              <option key={c.code} value={c.code}>
                {c.code}
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          className="rounded border border-stone-300 px-3"
          onClick={() =>
            costCenterSave({ code, name, parent: parent || null })
              .then(() => {
                setCode('');
                setName('');
                setParent('');
                onChange(`Centro ${code.toUpperCase()} guardado.`);
              })
              .catch((e) => onChange(errorMessage(e), true))
          }
        >
          Añadir centro
        </button>
      </div>
    </section>
  );
}

function Rules({
  rules,
  onChange,
}: {
  rules: CostRule[];
  onChange: (msg: string, error?: boolean) => void;
}) {
  const [prefix, setPrefix] = useState('');
  const [text, setText] = useState('');
  const save = (p: string, shares: Share[]) =>
    costRuleSet(p, shares)
      .then(() =>
        onChange(shares.length ? `Reparto de ${p} guardado.` : `Reparto de ${p} quitado.`),
      )
      .catch((e) => onChange(errorMessage(e), true));
  return (
    <section aria-label="Reparto por cuenta" className="mt-8">
      <h3 className="font-medium">Reparto por cuenta</h3>
      <p className="text-stone-600">
        Cómo se reparte cada gasto (6) o ingreso (7) entre centros. Vale la regla de la cuenta más
        larga que coincida (p. ej. «62» para todos los servicios exteriores y «629» para otros).
      </p>
      <ul className="mt-1">
        {rules.map((r) => (
          <li key={r.prefix} className="flex items-center gap-3">
            <span className="w-24 font-mono">{r.prefix}</span>
            <span>{showShares(r.shares)}</span>
            <button type="button" className="text-stone-700" onClick={() => save(r.prefix, [])}>
              Quitar
            </button>
          </li>
        ))}
      </ul>
      <div className="mt-3 flex flex-wrap items-end gap-3">
        <label>
          Cuenta o grupo
          <input
            className="mt-1 block w-32 font-mono"
            value={prefix}
            onChange={(e) => setPrefix(e.target.value)}
          />
        </label>
        <label>
          Reparto
          <input
            className="mt-1 block w-72"
            placeholder="TIENDA 75, WEB 25"
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
        </label>
        <button
          type="button"
          className="rounded border border-stone-300 px-3"
          onClick={() => {
            const shares = parseShares(text);
            if (!shares) {
              onChange('Escribe el reparto como «TIENDA 75, WEB 25».', true);
              return;
            }
            save(prefix.trim(), shares).then(() => {
              setPrefix('');
              setText('');
            });
          }}
        >
          Guardar reparto
        </button>
      </div>
    </section>
  );
}

function Redistributions({
  list,
  centers,
  onChange,
}: {
  list: CostRedistribution[];
  centers: CostCenter[];
  onChange: (msg: string, error?: boolean) => void;
}) {
  const [source, setSource] = useState('');
  const [text, setText] = useState('');
  const save = (src: string, shares: Share[]) =>
    costRedistributionSet(src, shares)
      .then(() =>
        onChange(shares.length ? `Reparto de ${src} guardado.` : `Reparto de ${src} quitado.`),
      )
      .catch((e) => onChange(errorMessage(e), true));
  if (centers.length < 2) return null;
  return (
    <section aria-label="Reparto secundario" className="mt-8">
      <h3 className="font-medium">Reparto secundario</h3>
      <p className="text-stone-600">
        Lo de un centro (servicios generales, administración…) se reparte al final entre otros.
      </p>
      <ul className="mt-1">
        {list.map((r) => (
          <li key={r.source} className="flex items-center gap-3">
            <span className="w-24 font-mono">{r.source}</span>
            <span>→ {showShares(r.shares)}</span>
            <button type="button" className="text-stone-700" onClick={() => save(r.source, [])}>
              Quitar
            </button>
          </li>
        ))}
      </ul>
      <div className="mt-3 flex flex-wrap items-end gap-3">
        <label>
          Centro
          <select className="mt-1 block" value={source} onChange={(e) => setSource(e.target.value)}>
            <option value="">Elige…</option>
            {centers.map((c) => (
              <option key={c.code} value={c.code}>
                {c.code}
              </option>
            ))}
          </select>
        </label>
        <label>
          Se reparte entre
          <input
            className="mt-1 block w-72"
            placeholder="TIENDA 75, WEB 25"
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
        </label>
        <button
          type="button"
          className="rounded border border-stone-300 px-3"
          onClick={() => {
            const shares = parseShares(text);
            if (!source || !shares) {
              onChange('Elige el centro y escribe el reparto como «TIENDA 75, WEB 25».', true);
              return;
            }
            save(source, shares).then(() => {
              setSource('');
              setText('');
            });
          }}
        >
          Guardar reparto secundario
        </button>
      </div>
    </section>
  );
}

function Unassigned({
  lines,
  centers,
  onChange,
}: {
  lines: AnalyticLine[];
  centers: CostCenter[];
  onChange: (msg: string, error?: boolean) => void;
}) {
  const todo = lines.filter((l) => l.source === 'sin');
  if (todo.length === 0 || centers.length === 0) return null;
  return (
    <section aria-label="Sin repartir" className="mt-8">
      <h3 className="font-medium">Sin repartir ({todo.length})</h3>
      <table className="mt-1 w-full">
        <tbody>
          {todo.slice(0, 50).map((l) => (
            <tr key={l.line_id} className="border-t border-stone-200">
              <td className="pr-2">{formatDate(l.date)}</td>
              <td className="pr-2 font-mono">{l.account}</td>
              <td className="pr-2">{l.concept}</td>
              <td className={num}>{euros(l.amount)}</td>
              <td className="pl-2">
                <Menu
                  label={`Centro de ${l.concept}`}
                  entries={centers.map((c) => ({
                    label: `${c.code} · ${c.name}`,
                    onSelect: () =>
                      analyticLineSet(l.line_id, [{ center: c.code, pct_bp: 10000 }])
                        .then(() => onChange(`Línea asignada a ${c.code}.`))
                        .catch((err) => onChange(errorMessage(err), true)),
                  }))}
                >
                  Asignar a…
                  <ChevronDown size={14} aria-hidden="true" />
                </Menu>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

function Results({ year, round, levels }: { year: number; round: number; levels: number }) {
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [level, setLevel] = useState(0);
  const [rows, setRows] = useState<CenterResult[]>([]);
  const [error, setError] = useState<string | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `round` recarga tras cambiar repartos
  useEffect(() => {
    analyticReport(year, from || null, to || null, level || null)
      .then(setRows)
      .catch((e) => setError(errorMessage(e)));
  }, [year, from, to, level, round]);
  return (
    <section aria-label="Resultados por centro" className="mt-8">
      <h3 className="font-medium">Cuenta de resultados por centro</h3>
      <div className="mt-3 flex flex-wrap items-end gap-3">
        <label>
          Desde
          <input
            type="date"
            className="mt-1 block"
            value={from}
            onChange={(e) => setFrom(e.target.value)}
          />
        </label>
        <label>
          Hasta
          <input
            type="date"
            className="mt-1 block"
            value={to}
            onChange={(e) => setTo(e.target.value)}
          />
        </label>
        {levels > 1 && (
          <label>
            Nivel
            <select
              className="mt-1 block"
              value={level}
              onChange={(e) => setLevel(Number(e.target.value))}
            >
              <option value={0}>Cada centro</option>
              {Array.from({ length: levels }, (_, i) => i + 1).map((n) => (
                <option key={n} value={n}>
                  Nivel {n}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      {error && <p role="alert">{error}</p>}
      <table className="mt-3">
        <thead className="text-stone-500">
          <tr>
            <th className="text-left">Centro</th>
            <th className={num}>Ingresos</th>
            <th className={num}>Gastos</th>
            <th className={num}>Resultado</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.center || '-'} className="border-t border-stone-200">
              <td className="pr-4">{r.center ? `${r.center} · ${r.name}` : <em>{r.name}</em>}</td>
              <td className={num}>{euros(r.income)}</td>
              <td className={num}>{euros(r.expenses)}</td>
              <td className={`${num} ${r.result < 0 ? 'text-red-700' : ''}`}>{euros(r.result)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

function spread(total: number): number[] {
  const part = Math.trunc(total / 12);
  return Array.from({ length: 12 }, (_, i) => (i === 11 ? total - part * 11 : part));
}

function Budget({ year, centers }: { year: number; centers: CostCenter[] }) {
  const [lines, setLines] = useState<BudgetLine[]>([]);
  const [through, setThrough] = useState(12);
  const [dev, setDev] = useState<Deviation[]>([]);
  const [msg, setMsg] = useState<Notice | null>(null);
  const [account, setAccount] = useState('');
  const [center, setCenter] = useState('');
  const [annual, setAnnual] = useState('');
  const [change, setChange] = useState('0');
  const load = useCallback(() => {
    budgetList(year)
      .then(setLines)
      .catch((e) => setMsg({ text: errorMessage(e), error: true }));
    budgetReport(year, through)
      .then(setDev)
      .catch(() => setDev([]));
  }, [year, through]);
  useEffect(load, [load]);
  const done = (m: string) => {
    setMsg({ text: m });
    load();
  };
  const fail = (e: unknown) => setMsg({ text: errorMessage(e), error: true });
  return (
    <section aria-label="Presupuesto" className="mt-8">
      <h3 className="font-medium">Presupuesto {year}</h3>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <span>Con lo real de {year - 1} y un cambio del</span>
        <input
          aria-label="Cambio sobre el año anterior (%)"
          className="w-20 text-right"
          value={change}
          onChange={(e) => setChange(e.target.value)}
        />
        <span>%</span>
        <ConfirmAction
          label="Generar desde el año anterior"
          question={`¿Rehacer el presupuesto ${year} de toda la empresa con lo real de ${year - 1}? Sustituye las líneas de esas cuentas.`}
          confirmLabel="Sí, generar"
          onConfirm={() => {
            // Porcentaje con signo y hasta dos decimales: «-10», «2,5».
            const m = /^\s*(-?\d{1,3}(?:[.,]\d{1,2})?)\s*%?\s*$/.exec(change);
            if (!m?.[1]) {
              setMsg({ text: 'Revisa el cambio: un porcentaje como 5, -10 o 2,5.', error: true });
              return;
            }
            return budgetFromActuals(
              year,
              year - 1,
              3,
              Math.round(Number(m[1].replace(',', '.')) * 100),
            )
              .then((n) =>
                done(
                  `${plural(n, 'línea', 'líneas')} de presupuesto ${n === 1 ? 'generada' : 'generadas'}.`,
                ),
              )
              .catch(fail);
          }}
        />
      </div>
      <div className="mt-4 flex flex-wrap items-end gap-3">
        <label>
          Cuenta o grupo
          <input
            className="mt-1 block w-32 font-mono"
            value={account}
            onChange={(e) => setAccount(e.target.value)}
          />
        </label>
        <label>
          Centro
          <select className="mt-1 block" value={center} onChange={(e) => setCenter(e.target.value)}>
            <option value="">Toda la empresa</option>
            {centers.map((c) => (
              <option key={c.code} value={c.code}>
                {c.code}
              </option>
            ))}
          </select>
        </label>
        <label>
          Importe anual
          <input
            className="mt-1 block w-36 text-right"
            inputMode="decimal"
            value={annual}
            onChange={(e) => setAnnual(e.target.value)}
          />
        </label>
        <button
          type="button"
          className="rounded border border-stone-300 px-3"
          onClick={() => {
            const total = parseEuros(annual);
            if (total === null) {
              setMsg({ text: 'Revisa el importe.', error: true });
              return;
            }
            budgetSet(year, { account: account.trim(), center, months: spread(total) })
              .then(() => {
                setAccount('');
                setAnnual('');
                done(`Presupuesto de ${account.trim()} guardado (en doce meses iguales).`);
              })
              .catch(fail);
          }}
        >
          Guardar línea
        </button>
      </div>
      <Notice msg={msg} className="mt-2" />
      {lines.length > 0 && (
        <div className="mt-3 overflow-x-auto">
          <table className="text-xs">
            <thead className="text-stone-500">
              <tr>
                <th className="text-left">Cuenta</th>
                <th className="text-left">Centro</th>
                {MONTHS.map((m) => (
                  <th key={m} className={num}>
                    {m}
                  </th>
                ))}
                <th className={num}>Total</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {lines.map((l) => (
                <tr key={`${l.account}-${l.center}`} className="border-t border-stone-200">
                  <td className="pr-2 font-mono">{l.account}</td>
                  <td className="pr-2">{l.center || '—'}</td>
                  {l.months.map((m, i) => (
                    <td key={MONTHS[i]} className={num}>
                      {m ? (m / 100).toLocaleString('es-ES', { maximumFractionDigits: 0 }) : ''}
                    </td>
                  ))}
                  <td className={`${num} font-medium`}>
                    {euros(l.months.reduce((a, b) => a + b, 0))}
                  </td>
                  <td>
                    <button
                      type="button"
                      className="text-stone-700"
                      onClick={() =>
                        budgetSet(year, { ...l, months: Array(12).fill(0) })
                          .then(() => done(`Línea ${l.account} quitada.`))
                          .catch(fail)
                      }
                    >
                      Quitar
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <h4 className="mt-6 font-medium">Desviaciones</h4>
      <label className="mt-2 block">
        De enero a
        <select
          className="mt-1 block"
          value={through}
          onChange={(e) => setThrough(Number(e.target.value))}
        >
          {MONTHS.map((m, i) => (
            <option key={m} value={i + 1}>
              {m}
            </option>
          ))}
        </select>
      </label>
      {dev.length === 0 ? (
        <p className="mt-1 text-stone-500">Sin presupuesto para {year}.</p>
      ) : (
        <table className="mt-2">
          <thead className="text-stone-500">
            <tr>
              <th className="text-left">Cuenta</th>
              <th className={num}>Presupuesto</th>
              <th className={num}>Real</th>
              <th className={num}>Desviación</th>
              <th className={num}>%</th>
            </tr>
          </thead>
          <tbody>
            {dev.map((d) => (
              <tr key={`${d.account}-${d.center}`} className="border-t border-stone-200">
                <td className="pr-4">
                  <span className="font-mono">{d.account}</span> {d.name}
                  {d.center && <span className="text-stone-500"> · {d.center}</span>}
                </td>
                <td className={num}>{euros(d.budget)}</td>
                <td className={num}>{euros(d.actual)}</td>
                <td className={`${num} ${d.unfavourable ? 'text-red-700' : 'text-emerald-700'}`}>
                  {euros(d.deviation)}
                </td>
                <td className={num}>
                  {d.deviation_bp === null
                    ? '—'
                    : `${(d.deviation_bp / 100).toLocaleString('es-ES', { maximumFractionDigits: 1 })} %`}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

/** Contabilidad analítica por centros de coste y presupuesto con desviaciones. */
export function Analitica({ year }: { year: number }) {
  const [centers, setCenters] = useState<CostCenter[]>([]);
  const [rules, setRules] = useState<CostRule[]>([]);
  const [redist, setRedist] = useState<CostRedistribution[]>([]);
  const [lines, setLines] = useState<AnalyticLine[]>([]);
  const [msg, setMsg] = useState<Notice | null>(null);
  const [round, setRound] = useState(0);
  const load = useCallback(() => {
    costCenters()
      .then(setCenters)
      .catch((e) => setMsg({ text: errorMessage(e), error: true }));
    costRules()
      .then(setRules)
      .catch(() => setRules([]));
    costRedistributions()
      .then(setRedist)
      .catch(() => setRedist([]));
    analyticLines(year)
      .then(setLines)
      .catch(() => setLines([]));
  }, [year]);
  useEffect(load, [load]);
  const changed = (m: string, error?: boolean) => {
    setMsg({ text: m, error });
    setRound((n) => n + 1);
    load();
  };
  return (
    <div className="mt-4 text-sm">
      <Notice msg={msg} className="mb-2" />
      <Centers centers={centers} onChange={changed} />
      <Rules rules={rules} onChange={changed} />
      <Redistributions list={redist} centers={centers} onChange={changed} />
      <Results
        year={year}
        round={round}
        levels={Math.max(1, ...centers.map((c) => depth(centers, c.code)))}
      />
      <Unassigned lines={lines} centers={centers} onChange={changed} />
      <Budget year={year} centers={centers} />
      <p className="mt-4 text-xs text-stone-500">
        Repartir y presupuestar no cambia el diario: vale también para ejercicios cerrados.
      </p>
    </div>
  );
}

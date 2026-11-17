import { useCallback, useEffect, useState } from 'react';
import { A3Export } from './A3Export';
import {
  type AccountsModel,
  type AnnualAccounts,
  annualAccounts,
  avisoDestacado,
  type BankFileResult,
  type BankTx,
  bankImport,
  bankMovements,
  bankReconcileAccount,
  bankReconcileInvoice,
  bankReconcileRemittance,
  bankReconcileReturn,
  bankRuleFor,
  bankSuggestions,
  errorMessage,
  euros,
  exportBooks,
  formatDate,
  money,
  pickFolder,
  pickStatementFiles,
  plural,
  redoVat,
  type StatementLine,
  type Suggestion,
  settleVat,
} from './api';
import { BancosConectados } from './BancosConectados';
import { BankRules } from './BankRules';
import { Conciliados } from './Conciliados';
import { ConfirmAction } from './ConfirmAction';
import { ContravalorMovimiento, SaldosCuentas } from './CuentasDivisa';
import { EcpnView, MemoriaDraft, ModeloCuentas } from './CuentasModelo';
import { ExtractosImportados } from './ExtractosImportados';
import { ImportEntries } from './ImportEntries';
import { Legalizacion } from './Legalizacion';

/** Partidas del ejercicio y, si hay datos, del anterior (cuentas anuales comparativas). */
export function Lines({
  lines,
  previous,
  year,
}: {
  lines: StatementLine[];
  previous?: StatementLine[] | null | undefined;
  year: number;
}) {
  const before = new Map((previous ?? []).map((l) => [l.id, l.amount]));
  const compare = [...before.values()].some((v) => v !== 0);
  return (
    <table className="w-full text-sm">
      {compare && (
        <thead className="text-stone-500">
          <tr>
            <th />
            <th className="px-2 text-right font-normal">{year}</th>
            <th className="px-2 text-right font-normal">{year - 1}</th>
          </tr>
        </thead>
      )}
      <tbody>
        {lines
          .filter((l) => !l.leaf || l.amount !== 0 || (before.get(l.id) ?? 0) !== 0)
          .map((l) => (
            <tr key={l.id} className={l.leaf ? '' : 'font-semibold'}>
              <td className={l.leaf ? 'pl-4' : ''}>{l.label}</td>
              <td className="px-2 text-right tabular-nums">{euros(l.amount)}</td>
              {compare && (
                <td className="px-2 text-right tabular-nums text-stone-500">
                  {euros(before.get(l.id) ?? 0)}
                </td>
              )}
            </tr>
          ))}
      </tbody>
    </table>
  );
}

export function AnnualAccountsView({ year }: { year: number }) {
  const [a, setA] = useState<AnnualAccounts | null>(null);
  const [prev, setPrev] = useState<AnnualAccounts | null>(null);
  const [model, setModel] = useState<AccountsModel>('Pymes');
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    annualAccounts(year, model)
      .then(setA)
      .catch((e) => setError(errorMessage(e)));
    annualAccounts(year - 1, model)
      .then(setPrev)
      .catch(() => setPrev(null));
  }, [year, model]);
  if (error)
    return (
      <p role="alert" className="mt-4 text-sm text-red-700">
        {error}
      </p>
    );
  if (!a) return null;
  const unmapped = [...a.balance.unmapped, ...a.income.unmapped];
  return (
    <div className="mt-4 space-y-6">
      <ModeloCuentas year={year} model={model} onModel={setModel} />
      <MemoriaDraft year={year} model={model} />
      <label className="block text-sm">
        Modelo
        <select
          className="mt-1 block"
          value={model}
          onChange={(e) => setModel(e.target.value as AccountsModel)}
        >
          <option value="Pymes">PYMES</option>
          <option value="Abreviado">Abreviado</option>
          <option value="Normal">Normal</option>
        </select>
      </label>
      {!a.balanced && (
        <p role="alert" className="text-sm text-red-700">
          El balance no cuadra: revisa los asientos.
        </p>
      )}
      {unmapped.length > 0 && (
        <p role="alert" className="rounded bg-amber-100 p-2 text-sm text-amber-900">
          Cuentas con saldo sin partida en el modelo: {unmapped.map((u) => u.account).join(', ')}.
          Revisa su contabilización.
        </p>
      )}
      <section>
        <h3 className="font-semibold">Cuenta de pérdidas y ganancias</h3>
        <Lines lines={a.income.lines} previous={prev?.income.lines} year={year} />
      </section>
      <section>
        <h3 className="font-semibold">Balance</h3>
        <Lines lines={a.balance.lines} previous={prev?.balance.lines} year={year} />
      </section>
      <EcpnView year={year} model={model} />
    </div>
  );
}

export function TaxActions({ year }: { year: number }) {
  const [msg, setMsg] = useState<string | null>(null);
  // Trimestre recién liquidado: se ofrece rehacerlo si el 303 cambió después.
  const [settled, setSettled] = useState<number | null>(null);
  return (
    <div className="mt-4 space-y-7 text-sm">
      <div>
        <p className="font-medium">Liquidación del IVA (asiento 477/472 contra 4750/4700)</p>
        <div className="mt-2 flex gap-2">
          {[1, 2, 3, 4].map((q) => (
            <button
              key={q}
              type="button"
              className="rounded border border-stone-300 px-3 py-1"
              onClick={async () => {
                try {
                  const id = await settleVat(year, q);
                  setMsg(id ? `IVA del ${q}T liquidado.` : `Sin IVA en el ${q}T.`);
                  setSettled(id ? q : null);
                } catch (e) {
                  setMsg(errorMessage(e));
                  setSettled(null);
                }
              }}
            >
              {q}T
            </button>
          ))}
        </div>
        {settled !== null && (
          <div className="mt-2">
            <ConfirmAction
              label={`Rehacer la liquidación del ${settled}T`}
              className="text-link underline"
              danger={false}
              question={`Si el 303 del ${settled}T ha cambiado desde que lo liquidaste (p. ej. registraste otra factura), se borra el asiento de liquidación y se vuelve a hacer con el 303 de ahora. No se puede si ya está presentado o pagado. ¿Seguir?`}
              confirmLabel="Sí, rehacer"
              onConfirm={async () => {
                try {
                  const id = await redoVat(year, settled);
                  setMsg(
                    id
                      ? `Liquidación del ${settled}T rehecha con el 303 de ahora.`
                      : `Sin IVA en el ${settled}T: se ha quitado la liquidación.`,
                  );
                } catch (e) {
                  setMsg(errorMessage(e));
                }
              }}
            />
          </div>
        )}
      </div>
      <div>
        <p className="font-medium">Libros registro de IVA e IRPF (formato AEAT, XLSX)</p>
        <button
          type="button"
          className="mt-2 rounded border border-stone-300 px-3 py-1"
          onClick={async () => {
            const dir = await pickFolder();
            if (!dir) return;
            try {
              setMsg(`Libros guardados en ${await exportBooks(year, dir)}`);
            } catch (e) {
              setMsg(errorMessage(e));
            }
          }}
        >
          Exportar libros {year}…
        </button>
        {msg && (
          <p role="status" className="mt-2">
            {msg}
          </p>
        )}
      </div>
      <A3Export year={year} />
      <ImportEntries />
      <Legalizacion year={year} />
    </div>
  );
}

function Movement({ m, onDone }: { m: BankTx; onDone: () => void }) {
  const [sugs, setSugs] = useState<Suggestion[] | null>(null);
  const [account, setAccount] = useState('626');
  const [rule, setRule] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = async (p: Promise<unknown>) => {
    try {
      await p;
      onDone();
    } catch (e) {
      setError(errorMessage(e));
    }
  };
  return (
    <li className="py-2 text-sm">
      <div className="flex justify-between">
        <button
          type="button"
          className="text-left"
          onClick={() => {
            bankSuggestions(m.id)
              .then(setSugs)
              .catch((e) => setError(errorMessage(e)));
            bankRuleFor(m.id)
              .then((r) => {
                if (r) {
                  setAccount(r.account);
                  setRule(r.pattern);
                }
              })
              .catch(() => {});
          }}
        >
          {formatDate(m.op_date)} · {m.description}
        </button>
        <span className={m.amount < 0 ? 'text-red-700' : 'text-emerald-800'}>
          {money(m.amount, m.currency)}
        </span>
      </div>
      <ContravalorMovimiento m={m} onDone={onDone} />
      {sugs && (
        <div className="mt-1 space-y-1 pl-4">
          {sugs.map((s) => (
            <button
              key={`${s.kind}-${s.id}-${s.seq}`}
              type="button"
              className="block text-link underline"
              onClick={() =>
                run(
                  s.kind === 'remesa'
                    ? bankReconcileRemittance(m.id, s.id)
                    : s.kind === 'devolucion'
                      ? bankReconcileReturn(m.id, s.id)
                      : bankReconcileInvoice(m.id, s.kind, s.id, s.seq),
                )
              }
            >
              {s.kind === 'devolucion' ? (
                `${s.party} (anotada el ${formatDate(s.date)})`
              ) : s.kind === 'remesa' ? (
                `Remesa ${s.number} · ${s.party}, ${s.installments} ${
                  s.installments === 1 ? 'operación' : 'operaciones'
                } (${formatDate(s.date)})`
              ) : (
                <>
                  {m.amount > 0 ? 'Cobro' : 'Pago'} de {s.number}
                  {s.installments > 1 ? ` (plazo ${s.seq}/${s.installments})` : ''} · {s.party}{' '}
                  (vence {formatDate(s.due_date)})
                </>
              )}
            </button>
          ))}
          <div className="flex items-center gap-2">
            <span>O llevar a la cuenta</span>
            <input
              aria-label="Cuenta"
              className="w-24 rounded border px-1 font-mono"
              value={account}
              onChange={(e) => setAccount(e.target.value.trim())}
            />
            <button
              type="button"
              className="rounded border px-2"
              onClick={() => run(bankReconcileAccount(m.id, account, m.description))}
            >
              Contabilizar
            </button>
            {rule && <span className="text-stone-600">(regla «{rule}»)</span>}
          </div>
          {error && (
            <p role="alert" className="text-red-700">
              {error}
            </p>
          )}
        </div>
      )}
    </li>
  );
}

/** Lo que ha pasado con un fichero de extractos, en una línea. */
export function describeImport(r: BankFileResult): string {
  if (r.error || !r.resumen) return `${r.fichero}: ${r.error ?? 'no se ha podido importar'}`;
  const partes = r.resumen.extractos.map((e) => {
    if (!e.importado) return `${e.cuenta}: no se ha importado (${e.motivo})`;
    const ya = e.ya_estaban ? `, ${plural(e.ya_estaban, 'ya estaba', 'ya estaban')}` : '';
    const contabilizados = e.ya_contabilizados
      ? `, ${plural(e.ya_contabilizados, 'ya contabilizado', 'ya contabilizados')} en el diario`
      : '';
    const saldos = e.cuadre.estado === 'sin_saldos' ? ' (sin saldos para comprobarlo)' : '';
    return `${e.cuenta}: ${plural(e.movimientos_nuevos, 'movimiento nuevo', 'movimientos nuevos')}${ya}${contabilizados}${saldos}`;
  });
  return `${r.fichero}: ${partes.join('; ') || 'sin movimientos'}`;
}

/**
 * Los avisos de la importación de un fichero (signos deducidos o invertidos, cuenta adivinada o
 * provisional, posibles duplicados, importado aunque no cuadra…), con la cuenta delante si el
 * fichero trae varias.
 */
export function importWarnings(r: BankFileResult): string[] {
  const extractos = r.resumen?.extractos ?? [];
  return [
    ...new Set(
      extractos.flatMap((e) =>
        e.avisos.map((a) => (extractos.length > 1 ? `${e.cuenta}: ${a}` : a)),
      ),
    ),
  ];
}

export function Bank() {
  const [list, setList] = useState<BankTx[]>([]);
  const [msg, setMsg] = useState<string | null>(null);
  const [results, setResults] = useState<{ path: string; r: BankFileResult }[]>([]);
  const [version, setVersion] = useState(0);
  const load = useCallback(() => {
    bankMovements(true)
      .then(setList)
      .catch((e) => setMsg(errorMessage(e)));
    setVersion((n) => n + 1);
  }, []);
  useEffect(load, [load]);
  const importar = async (paths: string[], forzar = false) => {
    setMsg(null);
    try {
      const r = await bankImport(paths, forzar);
      setResults(r.map((x, i) => ({ path: paths[i] ?? x.fichero, r: x })));
      load();
    } catch (e) {
      setMsg(errorMessage(e));
    }
  };
  const noCuadran = results.filter(({ r }) =>
    r.resumen?.extractos.some((e) => !e.importado && e.cuadre.estado === 'no_cuadra'),
  );
  return (
    <div className="mt-4">
      <button
        type="button"
        className="text-sm text-link"
        onClick={async () => {
          const paths = await pickStatementFiles();
          if (paths.length > 0) await importar(paths);
        }}
      >
        Importar extractos del banco…
      </button>
      <p className="text-xs text-stone-600">
        De cualquier banco: PDF, Excel, CSV, Norma 43, camt, OFX, QIF o MT940. Se comprueban con sus
        saldos y no se repiten los movimientos que ya estaban.
      </p>
      {msg && (
        <p role="alert" className="text-sm text-red-700">
          {msg}
        </p>
      )}
      <div role="status">
        {results.length > 0 && (
          <ul className="mt-1 space-y-0.5 text-sm">
            {results.map(({ path, r }) => (
              <li key={path}>
                {describeImport(r)}
                {importWarnings(r).length > 0 && (
                  <ul className="ml-4 list-disc text-xs">
                    {importWarnings(r).map((a) => (
                      <li
                        key={a}
                        className={avisoDestacado(a) ? 'text-amber-800' : 'text-stone-600'}
                      >
                        {a}
                      </li>
                    ))}
                  </ul>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
      {noCuadran.length > 0 && (
        <button
          type="button"
          className="mt-1 rounded border px-2 text-sm"
          onClick={() =>
            importar(
              noCuadran.map(({ path }) => path),
              true,
            )
          }
        >
          Importar igualmente los que no cuadran
        </button>
      )}
      <BancosConectados onImported={load} />
      <BankRules version={version} onDone={load} />
      <SaldosCuentas version={version} />
      <ExtractosImportados version={version} onDone={load} />
      <p className="mt-3 text-sm text-stone-600">
        Movimientos pendientes de conciliar ({list.length}). Pulsa uno para ver propuestas.
      </p>
      <ul className="divide-y divide-stone-200">
        {list.map((m) => (
          <Movement key={m.id} m={m} onDone={load} />
        ))}
      </ul>
      <Conciliados version={version} onDone={load} />
    </div>
  );
}

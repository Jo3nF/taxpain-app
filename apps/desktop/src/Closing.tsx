import { CircleAlert, CircleCheck, Copy } from 'lucide-react';
import { type ReactNode, useCallback, useEffect, useState } from 'react';
import {
  type Allocation,
  type AllocationProposal,
  type ClosingChecklist,
  type CorporateTaxProposal,
  closingAllocation,
  closingAllocationPost,
  closingAllocationUndo,
  closingChecklist,
  closingCorporateTax,
  closingCorporateTaxPost,
  closingCorporateTaxUndo,
  closingVatBalances,
  closingVatBalancesPost,
  errorMessage,
  euros,
  formatDate,
  parseEuros,
  type VatBalances,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { Dividends } from './Dividends';
import { RentaSocio } from './RentaSocio';
import { ValoracionDivisa } from './ValoracionDivisa';
import { ValoracionValores } from './ValoracionValores';

const cell = 'px-2 py-1 text-right tabular-nums';

/** Lo que conviene tener hecho antes de cerrar y los plazos de después. */
export function ClosingChecklistCard({ data }: { data: ClosingChecklist }) {
  return (
    <div className="card p-5">
      <h3 className="font-semibold">Antes de cerrar {data.year}</h3>
      <ul className="mt-3 space-y-2">
        {data.items.map((i) => (
          <li key={i.id} className="flex items-start gap-2">
            {i.ok ? (
              <CircleCheck
                size={16}
                className="mt-0.5 shrink-0 text-emerald-600"
                aria-hidden="true"
              />
            ) : (
              <CircleAlert
                size={16}
                className="mt-0.5 shrink-0 text-amber-600"
                aria-hidden="true"
              />
            )}
            <span>
              <span className="font-medium">{i.label}</span>
              <span className="sr-only">{i.ok ? ' (hecho)' : ' (pendiente)'}</span>
              <span className="block text-stone-600">{i.detail}</span>
            </span>
          </li>
        ))}
      </ul>
      {data.deadlines.length > 0 && (
        <>
          <h4 className="mt-5 font-semibold">Después del cierre</h4>
          <ul className="mt-2 space-y-1 text-stone-700">
            {data.deadlines.map((d) => (
              <li key={d.label} className="flex justify-between gap-4">
                <span>{d.label}</span>
                <span className="shrink-0 tabular-nums">{formatDate(d.date)}</span>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

function Lines({ lines }: { lines: CorporateTaxProposal['lines'] }) {
  return (
    <table className="mt-3 w-full">
      <thead className="text-left text-stone-500">
        <tr>
          <th className="font-normal">Cuenta</th>
          <th className="px-2 text-right font-normal">Debe</th>
          <th className="px-2 text-right font-normal">Haber</th>
        </tr>
      </thead>
      <tbody>
        {lines.map((l) => (
          <tr key={l.account} className="border-t border-stone-100">
            <td className="py-1">
              {l.account} <span className="text-stone-500">{l.name}</span>
            </td>
            <td className={cell}>{l.debit ? euros(l.debit) : ''}</td>
            <td className={cell}>{l.credit ? euros(l.credit) : ''}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function CorporateTax({ year, onChanged }: { year: number; onChanged: () => void }) {
  const [p, setP] = useState<CorporateTaxProposal | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    closingCorporateTax(year)
      .then((r) => {
        setP(r);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);
  const done = () => {
    load();
    onChanged();
  };
  return (
    <div className="card p-5">
      <h3 className="font-semibold">Impuesto sobre Sociedades {year}</h3>
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
      {p && (
        <>
          <p className="mt-2 text-stone-700">
            Del borrador del modelo 200: cuota {euros(p.tax)}, retenciones {euros(p.withholdings)} y
            pagos fraccionados {euros(p.installments)}.{' '}
            {p.result >= 0
              ? `A pagar con el 200 (hasta el ${formatDate(p.due)}): ${euros(p.result)}.`
              : `A devolver: ${euros(-p.result)}.`}{' '}
            Asiento con fecha {formatDate(p.date)}:
          </p>
          <Lines lines={p.lines} />
          {p.warnings.length > 0 && (
            <ul className="mt-3 space-y-1 rounded-md bg-amber-50 p-3 text-amber-950">
              {p.warnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          )}
          <div className="mt-3">
            {p.entry_id === null ? (
              <ConfirmAction
                label="Contabilizar el IS"
                question="¿Contabilizar el Impuesto sobre Sociedades con estos importes? Revisa antes el borrador del 200 (Modelos → 200)."
                confirmLabel="Sí, contabilizar"
                danger={false}
                className="btn-primary rounded-md px-3 py-1.5"
                onConfirm={async () => {
                  await closingCorporateTaxPost(year);
                  done();
                }}
              />
            ) : (
              <p className="flex flex-wrap items-center gap-3">
                <span className="pill pill-green">Contabilizado</span>
                <ConfirmAction
                  label="Deshacer"
                  question="¿Quitar el asiento del IS? Sólo con el ejercicio abierto."
                  confirmLabel="Sí, quitarlo"
                  onConfirm={async () => {
                    await closingCorporateTaxUndo(year);
                    done();
                  }}
                />
              </p>
            )}
          </div>
        </>
      )}
    </div>
  );
}

/**
 * Ajuste de los saldos fantasma de la 4700 y la 4750 que dejaban las liquidaciones del IVA
 * antiguas (no abonaban la 4700 al compensar). Se propone y se contabiliza sólo si el usuario
 * confirma (PGC PYMES, cuenta 4700 b1).
 */
function VatBalancesCard({ year, onChanged }: { year: number; onChanged: () => void }) {
  const [p, setP] = useState<VatBalances | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    closingVatBalances(year)
      .then((r) => {
        setP(r);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);
  if (!p && !error) return null;
  return (
    <div className="card p-5">
      <h3 className="font-semibold">Saldos de la 4700 y la 4750 ({year})</h3>
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
      {p && (
        <>
          <p className="mt-2 text-stone-700">
            La 4700 (IVA a compensar) tiene {euros(p.debtor)} y la 4750 (IVA a ingresar){' '}
            {euros(p.creditor)} a la vez. Suele pasar cuando un 303 compensó IVA de periodos
            anteriores y la 4700 no se abonó; si no es eso, mira en el mayor de dónde sale. Asiento
            de ajuste con fecha {formatDate(p.date)}:
          </p>
          <Lines lines={p.lines} />
          <div className="mt-3">
            <ConfirmAction
              label="Contabilizar el ajuste"
              question={`¿Saldar ${euros(p.amount)} de la 4750 contra la 4700? Comprueba antes en el mayor que la 4700 es IVA ya compensado en un 303.`}
              confirmLabel="Sí, contabilizar"
              danger={false}
              className="btn-primary rounded-md px-3 py-1.5"
              onConfirm={async () => {
                await closingVatBalancesPost(year);
                load();
                onChanged();
              }}
            />
          </div>
        </>
      )}
    </div>
  );
}

const FIELDS: [keyof Allocation, string][] = [
  ['legal', 'Reserva legal (112)'],
  ['losses', 'Compensar pérdidas anteriores (121)'],
  ['capitalization', 'Reserva de capitalización (1149)'],
  ['voluntary', 'Reservas voluntarias (113)'],
  ['dividends', 'Dividendos (526)'],
  ['remanente', 'Remanente (120)'],
];

function minutes(p: AllocationProposal, a: Allocation, date: string, company: string): string {
  const parts = FIELDS.filter(([k]) => a[k] > 0).map(
    ([k, label]) => `- ${label.replace(/ \(\d+\)$/, '')}: ${euros(a[k])}`,
  );
  return [
    `En la junta general de ${company}, celebrada el ${formatDate(date)}, se aprueban por unanimidad las cuentas anuales del ejercicio ${p.year} y la siguiente aplicación del resultado (${euros(p.result)}):`,
    '',
    ...(p.result > 0
      ? parts
      : [`- A resultados negativos de ejercicios anteriores: ${euros(-p.result)}`]),
    '',
    'Texto orientativo: adáptalo al acta y a los estatutos. Las cuentas se depositan en el Registro Mercantil en el mes siguiente a la junta.',
  ].join('\n');
}

function ResultAllocation({ year, onChanged }: { year: number; onChanged: () => void }) {
  const [p, setP] = useState<AllocationProposal | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<Record<keyof Allocation, string>>({
    legal: '',
    losses: '',
    voluntary: '',
    dividends: '',
    remanente: '',
    capitalization: '',
  });
  const [date, setDate] = useState('');
  const [copied, setCopied] = useState(false);
  const load = useCallback(() => {
    closingAllocation(year)
      .then((r) => {
        setP(r);
        setError(null);
        const t = (c: number) => (c ? (c / 100).toFixed(2).replace('.', ',') : '');
        setForm({
          legal: t(r.suggested.legal),
          losses: t(r.suggested.losses),
          voluntary: t(r.suggested.voluntary),
          dividends: t(r.suggested.dividends),
          remanente: t(r.suggested.remanente),
          capitalization: t(r.suggested.capitalization),
        });
        setDate((d) => d || r.meeting_deadline);
      })
      .catch((e) => setError(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);

  if (error) {
    return (
      <div className="card p-5">
        <h3 className="font-semibold">Aplicación del resultado {year}</h3>
        <p className="mt-2 text-stone-600">{error}</p>
      </div>
    );
  }
  if (!p) return null;
  const parsed = Object.fromEntries(
    FIELDS.map(([k]) => [k, form[k].trim() ? parseEuros(form[k]) : 0]),
  ) as Record<keyof Allocation, number | null>;
  const valid = Object.values(parsed).every((x) => x !== null);
  const allocation = parsed as unknown as Allocation;
  const total = valid ? FIELDS.reduce((s, [k]) => s + (allocation[k] ?? 0), 0) : 0;
  const profit = p.result > 0;

  return (
    <div className="card p-5">
      <h3 className="font-semibold">Aplicación del resultado {year}</h3>
      <p className="mt-2 text-stone-700">
        {profit ? `Beneficio de ${euros(p.result)}` : `Pérdidas de ${euros(-p.result)}`}. Capital{' '}
        {euros(p.capital)}, reserva legal {euros(p.legal_reserve)}
        {p.losses > 0 && `, pérdidas anteriores sin compensar ${euros(p.losses)}`}. La junta ha de
        aprobarla antes del {formatDate(p.meeting_deadline)}.
      </p>
      {p.warnings.map((w) => (
        <p key={w} className="mt-2 rounded-md bg-amber-50 p-3 text-amber-900">
          {w}
        </p>
      ))}
      {p.entry_id !== null ? (
        <p className="mt-3 flex flex-wrap items-center gap-3">
          <span className="pill pill-green">Contabilizada</span>
          <ConfirmAction
            label="Deshacer"
            question="¿Quitar el asiento de la aplicación del resultado?"
            confirmLabel="Sí, quitarlo"
            onConfirm={async () => {
              await closingAllocationUndo(year);
              load();
              onChanged();
            }}
          />
        </p>
      ) : (
        <>
          {profit && (
            <>
              {p.required_compensation > 0 && (
                <p className="mt-2 rounded-md bg-amber-50 p-3 text-amber-900">
                  Las pérdidas de ejercicios anteriores dejan el patrimonio neto por debajo del
                  capital: el beneficio compensa primero {euros(p.required_compensation)} de ellas,
                  antes de la reserva legal (art. 28.3 de la Resolución del ICAC de 5-3-2019).
                </p>
              )}
              <p className="mt-2 text-stone-600">
                Mínimo a la reserva legal: {euros(p.legal_minimum)} (10 % del beneficio hasta que
                llegue al 20 % del capital, art. 274 LSC).
                {p.capitalization_minimum > 0 &&
                  ` A la reserva de capitalización, al menos ${euros(p.capitalization_minimum)} (la reducción del 200; indisponible 3 años desde los ejercicios iniciados en 2024, art. 25.1 LIS).`}{' '}
                Dividendos: como mucho {euros(p.max_dividends)} (el patrimonio neto no puede quedar
                por debajo del capital, art. 273.2 LSC)
                {p.interim_dividend > 0 &&
                  `, de los que ${euros(p.interim_dividend)} ya se repartieron a cuenta (557)`}
                ; llevan una retención del 19 % al pagarlos (modelo 123).
              </p>
              <div className="form-grid mt-3">
                {FIELDS.map(([k, label]) => (
                  <label key={k}>
                    {label}
                    <input
                      className="text-right"
                      inputMode="decimal"
                      value={form[k]}
                      onChange={(e) => setForm({ ...form, [k]: e.target.value })}
                    />
                  </label>
                ))}
              </div>
              <p
                className={`mt-2 ${valid && total === p.result ? 'text-stone-600' : 'text-red-700'}`}
              >
                {valid ? `Suma ${euros(total)} de ${euros(p.result)}.` : 'Revisa los importes.'}
              </p>
            </>
          )}
          <label className="mt-3 block">
            Fecha de la junta
            <input
              type="date"
              className="mt-1 block"
              value={date}
              onChange={(e) => setDate(e.target.value)}
            />
          </label>
          <div className="mt-4 flex flex-wrap items-start gap-3">
            <ConfirmAction
              label="Contabilizar la aplicación"
              question="¿Contabilizar la aplicación del resultado aprobada por la junta?"
              confirmLabel="Sí, contabilizar"
              danger={false}
              className="btn-primary rounded-md px-3 py-1.5"
              onConfirm={async () => {
                if (!valid) throw { message: 'Revisa los importes.' };
                await closingAllocationPost(year, date, allocation);
                load();
                onChanged();
              }}
            />
            {valid && (
              <button
                type="button"
                className="btn-ghost"
                onClick={() => {
                  navigator.clipboard
                    ?.writeText(minutes(p, allocation, date, 'la sociedad'))
                    .then(() => setCopied(true))
                    .catch(() => setCopied(false));
                }}
              >
                <Copy size={14} aria-hidden="true" />
                {copied ? 'Copiado' : 'Copiar texto para el acta'}
              </button>
            )}
          </div>
          <p className="mt-2 text-stone-600">
            El acta completa (o la decisión del socio único), numerada en el libro y con su
            certificación, está en Contabilidad → Libros societarios, plantilla «Aprobación de
            cuentas».
          </p>
        </>
      )}
    </div>
  );
}

/**
 * Cierre guiado: comprobaciones, ajustes de cierre (`ajustes`, que recibe con qué recargar la
 * lista), IS, cierre, aplicación del resultado y pago de dividendos.
 */
export function GuidedClosing({
  year,
  children,
  ajustes,
}: {
  year: number;
  children: ReactNode;
  ajustes?: (onChanged: () => void) => ReactNode;
}) {
  const [data, setData] = useState<ClosingChecklist | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    closingChecklist(year)
      .then((r) => {
        setData(r);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
  }, [year]);
  useEffect(load, [load]);
  return (
    <div className="mt-4 space-y-4 text-sm">
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      {data && <ClosingChecklistCard data={data} />}
      {data && ajustes?.(load)}
      {data && !data.closed && <VatBalancesCard year={year} onChanged={load} />}
      {data && (
        <div id="paso-divisa">
          <ValoracionDivisa year={year} onChanged={load} />
        </div>
      )}
      {data?.company && (
        <div id="paso-valores">
          <ValoracionValores year={year} onChanged={load} />
        </div>
      )}
      {data?.company && !data.closed && (
        <div id="paso-is">
          <CorporateTax year={year} onChanged={load} />
        </div>
      )}
      <div id="paso-cerrar" className="card space-y-2 p-5">
        <h3 className="font-semibold">Cerrar el ejercicio</h3>
        {children}
      </div>
      {data?.lsc && data.closed && <ResultAllocation year={year} onChanged={load} />}
      {data?.lsc && <Dividends year={year} />}
      {data?.lsc && (
        <div className="card p-5">
          <RentaSocio year={year} />
        </div>
      )}
    </div>
  );
}

import { useCallback, useEffect, useState } from 'react';
import {
  errorMessage,
  euros,
  formatDate,
  type Leasing,
  type LeasingInstallment,
  type Lessor,
  leasingCreate,
  leasingDelete,
  leasingFixDueDates,
  leasingList,
  leasingPostDue,
  leasingReclassify,
  leasingRegularize,
  leasingSetLessor,
  parseEuros,
  plural,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { localToday } from './JournalEntryForm';

/** «50» → 50; null si no es un porcentaje entero de 0 a 100. */
const wholePct = (s: string): number | null => {
  const t = s.trim();
  return /^\d{1,3}$/.test(t) && Number(t) <= 100 ? Number(t) : null;
};

/**
 * Datos del arrendador de un contrato que aún no los tiene: sin ellos no se pueden registrar
 * las facturas de sus cuotas.
 */
function LessorForm({ id, onSaved }: { id: number; onSaved: (msg: string) => void }) {
  const [l, setL] = useState({ nif: '', name: '', pct: '100' });
  const [error, setError] = useState<string | null>(null);
  return (
    <div className="mt-1 space-y-1">
      <p className="text-stone-600">
        Indica el arrendador (la entidad de leasing): cada cuota se registra como una factura suya,
        para deducir su IVA en el 303.
      </p>
      <div className="flex flex-wrap items-end gap-2">
        <label>
          NIF del arrendador
          <input value={l.nif} onChange={(e) => setL({ ...l, nif: e.target.value })} />
        </label>
        <label>
          Nombre
          <input value={l.name} onChange={(e) => setL({ ...l, name: e.target.value })} />
        </label>
        <label>
          IVA deducible %
          <input value={l.pct} onChange={(e) => setL({ ...l, pct: e.target.value })} />
        </label>
        <button
          type="button"
          className="rounded border border-stone-300 px-2 py-0.5"
          onClick={async () => {
            const pct = wholePct(l.pct);
            if (pct === null) {
              setError('El IVA deducible es un porcentaje de 0 a 100 (100 salvo uso mixto).');
              return;
            }
            try {
              await leasingSetLessor(id, {
                nif: l.nif.trim(),
                name: l.name.trim(),
                deductible_pct: pct,
              });
              setError(null);
              onSaved('Arrendador guardado.');
            } catch (e) {
              setError(errorMessage(e));
            }
          }}
        >
          Guardar arrendador
        </button>
      </div>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
    </div>
  );
}

/**
 * Cuotas contabilizadas con el sistema antiguo (asiento directo contra la 572, sin factura del
 * arrendador): su IVA no está en el 303, ni en el libro registro ni en Cobros y pagos. Se ofrece
 * regularizarlas, con confirmación; las que no se pueden (303 presentado, ejercicio cerrado…)
 * se explican.
 */
function LegacyNotice({
  leasing: l,
  installments,
  onDone,
}: {
  leasing: Leasing;
  installments: LeasingInstallment[];
  onDone: (msg: string) => void;
}) {
  const legacy = installments.filter((q) => q.legacy);
  if (legacy.length === 0) return null;
  const blocked = legacy.filter((q) => q.blocked);
  const ready = legacy.length - blocked.length;
  return (
    <div className="mt-1 space-y-1 rounded border border-amber-300 bg-amber-50 p-2 text-stone-900">
      <p>
        Hay {plural(legacy.length, 'cuota registrada', 'cuotas registradas')} con el sistema antiguo
        (un asiento pagado desde la 572, sin factura del arrendador): su IVA no está en el 303, ni
        en el libro registro de recibidas, ni en Cobros y pagos.
      </p>
      {blocked.length > 0 && (
        <div>
          <p>
            {blocked.length === 1
              ? 'Esta no se puede regularizar desde aquí:'
              : `Estas ${blocked.length} no se pueden regularizar desde aquí:`}
          </p>
          <ul className="list-disc pl-5">
            {blocked.map((q) => (
              <li key={q.n}>
                Cuota {q.n}/{l.months} del {formatDate(q.date)}: {q.blocked}.
              </li>
            ))}
          </ul>
          <p className="text-stone-700">
            Si no dedujiste su IVA, consúltalo con tu gestor: normalmente se puede deducir en un 303
            posterior dentro de los cuatro años siguientes o corrigiendo el 303 de su trimestre.
          </p>
        </div>
      )}
      {ready > 0 && !l.lessor && <p>Indica antes el arrendador para poder regularizarlas.</p>}
      {ready > 0 && l.lessor && (
        <ConfirmAction
          label={`Regularizar ${plural(ready, 'cuota', 'cuotas')}`}
          className="rounded border border-stone-300 bg-white px-2 py-0.5"
          danger={false}
          confirmLabel="Sí, regularizar"
          question={
            <>
              Para cada una de {ready === 1 ? 'esta cuota' : `estas ${ready} cuotas`} se registra la
              factura de {l.lessor.name} con la fecha de la cuota (entra en el libro registro y su
              IVA en el 303 del trimestre actual), se da por pagada ese mismo día desde la misma
              cuenta del banco y se borra su asiento antiguo, para que el gasto, el IVA y la deuda
              no se cuenten dos veces. Los saldos no cambian.
            </>
          }
          onConfirm={async () => {
            const r = await leasingRegularize(l.id, true);
            onDone(
              `${plural(r.done, 'cuota regularizada', 'cuotas regularizadas')} de ${l.name}: su IVA entra en el 303 del trimestre actual.${
                r.blocked.length > 0 ? ` Sin regularizar: ${r.blocked.join('; ')}.` : ''
              }`,
            );
          }}
        />
      )}
    </div>
  );
}

/**
 * Cuotas registradas antes del arreglo de octubre de 2026: su factura vencía a +30 días (las
 * condiciones de pago del arrendador) y no el día de la cuota, así que la cartera y el banco no
 * proponían la que tocaba. Se ofrece corregirlas (sólo las aún sin pagar ni conciliar).
 */
function DueDatesNotice({
  leasing: l,
  installments,
  onDone,
}: {
  leasing: Leasing;
  installments: LeasingInstallment[];
  onDone: (msg: string) => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const wrong = installments.filter((q) => q.due_mismatch).length;
  if (wrong === 0) return null;
  return (
    <div className="mt-1 space-y-1 rounded border border-amber-300 bg-amber-50 p-2 text-stone-900">
      <p>
        {wrong === 1
          ? 'Hay 1 cuota sin pagar cuyo vencimiento no es el día de la cuota'
          : `Hay ${wrong} cuotas sin pagar cuyo vencimiento no es el día de la cuota`}{' '}
        (las registradas con una versión anterior vencían a 30 días): la cartera y el banco no
        proponen la que toca.
      </p>
      <button
        type="button"
        className="rounded border border-stone-300 bg-white px-2 py-0.5"
        onClick={async () => {
          try {
            const n = await leasingFixDueDates(l.id);
            setError(null);
            onDone(
              `${plural(n, 'vencimiento corregido', 'vencimientos corregidos')} de ${l.name}: cada cuota vence el día de su cuota.`,
            );
          } catch (e) {
            setError(errorMessage(e));
          }
        }}
      >
        Corregir vencimientos
      </button>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
    </div>
  );
}

/** Contratos de leasing: alta (bien y deuda), cuadro de cuotas y cuotas vencidas. */
export function LeasingView({ year }: { year: number }) {
  const [list, setList] = useState<{ leasing: Leasing; installments: LeasingInstallment[] }[]>([]);
  const [f, setF] = useState({
    name: '',
    start: localToday(),
    principal: '',
    months: '48',
    rate: '5',
    option: '',
    account: '218',
    depreciation: '16',
    lessorNif: '',
    lessorName: '',
    deductible: '100',
  });
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(() => {
    leasingList()
      .then(setList)
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  useEffect(load, [load]);
  /** «5» / «2,5» → centésimas de punto (500, 250); null si no es un porcentaje válido. */
  const pct = (s: string): number | null => {
    const t = s.trim().replace(',', '.');
    return /^\d{1,3}(\.\d{1,2})?$/.test(t) ? Math.round(Number(t) * 100) : null;
  };

  return (
    <section aria-label="Leasing" className="mt-6 space-y-2 text-sm">
      <h3 className="font-medium">Leasing</h3>
      {list.map(({ leasing: l, installments }) => {
        const posted = installments.filter((q) => q.posted).length;
        const next = installments.find((q) => !q.posted);
        const due = installments.filter((q) => !q.posted && q.date <= localToday()).length;
        return (
          <div key={l.id} className="rounded border border-stone-200 p-2">
            <p>
              {l.name} · {euros(l.principal)} · {l.months} cuotas de{' '}
              {euros(installments[0]?.amount ?? 0)} + IVA · registradas {posted}/{l.months}
              {next && ` · siguiente el ${formatDate(next.date)}`}
            </p>
            {l.lessor && <p className="text-stone-600">Arrendador: {l.lessor.name}</p>}
            {!l.lessor && (
              <LessorForm
                id={l.id}
                onSaved={(m) => {
                  setMsg(m);
                  load();
                }}
              />
            )}
            <LegacyNotice
              leasing={l}
              installments={installments}
              onDone={(m) => {
                setMsg(m);
                load();
              }}
            />
            <DueDatesNotice
              leasing={l}
              installments={installments}
              onDone={(m) => {
                setMsg(m);
                load();
              }}
            />
            <div className="mt-1 flex flex-wrap items-center gap-3">
              {due > 0 && l.lessor && (
                <button
                  type="button"
                  className="rounded border border-stone-300 px-2 py-0.5"
                  onClick={async () => {
                    try {
                      const r = await leasingPostDue(l.id, localToday());
                      const n = r.posted;
                      const linked =
                        r.linked.length > 0
                          ? ` ${r.linked.length === 1 ? 'Una cuota ya tenía su factura en Recibidas y se ha enlazado con ella' : `${r.linked.length} cuotas ya tenían su factura en Recibidas y se han enlazado con ellas`} (${r.linked.join('; ')}).`
                          : '';
                      setMsg(
                        n === 0
                          ? `Ninguna factura nueva de ${l.name}.${linked}`
                          : `${plural(n, 'cuota', 'cuotas')} de ${l.name} ${n === 1 ? 'registrada' : 'registradas'} como ${n === 1 ? 'factura' : 'facturas'} de ${l.lessor?.name}: su IVA entra en el 303. Págalas conciliando el cargo del banco o en Cobros y pagos.${linked}`,
                      );
                      load();
                    } catch (e) {
                      setMsg(errorMessage(e));
                    }
                  }}
                >
                  Registrar las {due} cuotas vencidas
                </button>
              )}
              {posted === 0 && (
                <ConfirmAction
                  label="Borrar el contrato"
                  confirmLabel="Sí, borrar el contrato"
                  question={
                    <>
                      Se borra el leasing «{l.name}» dado de alta por error: el contrato, sus cuotas
                      previstas, el asiento de alta del bien contra la deuda (y el de paso a la 174,
                      si lo hubo) y el bien del inmovilizado. Sólo se puede si no tiene cuotas
                      contabilizadas, ni una factura del arrendador en Recibidas, ni amortización, y
                      si su ejercicio está abierto. Queda en la auditoría.
                    </>
                  }
                  onConfirm={async () => {
                    await leasingDelete(l.id, true);
                    setMsg(
                      `Leasing «${l.name}» borrado (contrato, cuotas previstas, asiento de alta y bien del inmovilizado). Si otros contratos comparten su cuenta de deuda, vuelve a pulsar «Pasar a la 174» al cierre.`,
                    );
                    load();
                  }}
                />
              )}
            </div>
          </div>
        );
      })}
      {list.length > 0 && (
        <button
          type="button"
          className="rounded border border-stone-300 px-2 py-0.5"
          onClick={async () => {
            try {
              const n = await leasingReclassify(year);
              setMsg(
                n
                  ? `Deuda a más de un año de ${year} pasada a la 174 (${plural(n, 'asiento')}).`
                  : `La deuda a largo plazo de ${year} ya estaba en la 174.`,
              );
            } catch (e) {
              setMsg(errorMessage(e));
            }
          }}
        >
          Pasar a la 174 la deuda a más de un año (cierre {year})
        </button>
      )}
      <details>
        <summary>Nuevo contrato de leasing</summary>
        <div className="form-grid mt-3">
          <label className="span-2">
            Bien
            <input value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} />
          </label>
          <label>
            Fecha
            <input
              type="date"
              value={f.start}
              onChange={(e) => setF({ ...f, start: e.target.value })}
            />
          </label>
          <label>
            Valor al contado
            <input
              value={f.principal}
              onChange={(e) => setF({ ...f, principal: e.target.value })}
            />
          </label>
          <label>
            Cuotas (meses)
            <input value={f.months} onChange={(e) => setF({ ...f, months: e.target.value })} />
          </label>
          <label>
            Interés anual %
            <input value={f.rate} onChange={(e) => setF({ ...f, rate: e.target.value })} />
          </label>
          <label>
            Opción de compra
            <input value={f.option} onChange={(e) => setF({ ...f, option: e.target.value })} />
          </label>
          <label className="span-2">
            Tipo
            <select value={f.account} onChange={(e) => setF({ ...f, account: e.target.value })}>
              <option value="218">Elementos de transporte (218)</option>
              <option value="213">Maquinaria (213)</option>
              <option value="217">Equipos informáticos (217)</option>
            </select>
          </label>
          <label>
            Amortización anual %
            <input
              value={f.depreciation}
              onChange={(e) => setF({ ...f, depreciation: e.target.value })}
            />
          </label>
          <label>
            NIF del arrendador
            <input
              value={f.lessorNif}
              onChange={(e) => setF({ ...f, lessorNif: e.target.value })}
            />
          </label>
          <label>
            Nombre del arrendador
            <input
              value={f.lessorName}
              onChange={(e) => setF({ ...f, lessorName: e.target.value })}
            />
          </label>
          <label>
            IVA deducible %
            <input
              value={f.deductible}
              onChange={(e) => setF({ ...f, deductible: e.target.value })}
            />
          </label>
          <p className="span-2 text-stone-600">
            Cada cuota se registra como una factura del arrendador (entra en el 303 y en Cobros y
            pagos). Sus facturas de las cuotas no se pueden registrar además en Recibidas: la app
            las rechaza para no deducir el IVA dos veces.
          </p>
          <div className="form-actions">
            <button
              type="button"
              className="rounded btn-primary px-3 py-1"
              onClick={async () => {
                const principal = parseEuros(f.principal);
                const option = parseEuros(f.option || '0');
                const months = /^\d{1,3}$/.test(f.months.trim()) ? Number(f.months) : 0;
                const rate = pct(f.rate);
                const depreciation = pct(f.depreciation);
                if (!f.name.trim() || principal === null || option === null || !(months > 0)) {
                  setMsg('Revisa los datos del contrato');
                  return;
                }
                if (rate === null || rate > 10000) {
                  setMsg('Revisa el interés anual: un porcentaje como 5 o 4,75.');
                  return;
                }
                if (depreciation === null || !(depreciation > 0 && depreciation <= 10000)) {
                  setMsg('Revisa la amortización anual: un porcentaje entre 0 y 100, como 16.');
                  return;
                }
                const deductible = wholePct(f.deductible);
                if (!f.lessorNif.trim() || !f.lessorName.trim() || deductible === null) {
                  setMsg(
                    'Indica el NIF y el nombre del arrendador y el IVA deducible (100 salvo uso mixto).',
                  );
                  return;
                }
                const lessor: Lessor = {
                  nif: f.lessorNif.trim(),
                  name: f.lessorName.trim(),
                  deductible_pct: deductible,
                };
                try {
                  await leasingCreate(
                    {
                      id: 0,
                      name: f.name.trim(),
                      start: f.start,
                      principal,
                      months,
                      rate,
                      purchase_option: option,
                      vat: 2100,
                      asset_account: f.account,
                      debt_account: '5240001',
                      lessor,
                    },
                    depreciation,
                  );
                  setMsg('Contrato dado de alta: bien en el inmovilizado y deuda en la 5240001.');
                  load();
                } catch (e) {
                  setMsg(errorMessage(e));
                }
              }}
            >
              Dar de alta
            </button>
          </div>
        </div>
      </details>
      {msg && <p role="status">{msg}</p>}
    </section>
  );
}

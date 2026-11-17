import { useCallback, useEffect, useState } from 'react';
import {
  type BankChoice,
  errorMessage,
  euros,
  formatDate,
  pickFolder,
  plural,
  type Remittance,
  type RemittanceCandidate,
  type RemittanceItem,
  type RemittanceKind,
  remittanceBanks,
  remittanceCandidates,
  remittanceCreate,
  remittanceList,
  remittanceReturn,
  remittanceSave,
  remittanceUndo,
  remittanceUndoReturn,
  type SepaSettings,
  saveSepaSettings,
  sepaSettings,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { localToday } from './JournalEntryForm';

const small = 'rounded border border-stone-300 px-1 py-0.5';

function addDays(iso: string, days: number) {
  const d = new Date(`${iso}T12:00:00Z`);
  d.setUTCDate(d.getUTCDate() + days);
  return d.toISOString().slice(0, 10);
}

const key = (i: { invoice_id: number; seq: number }) => `${i.invoice_id}-${i.seq}`;

async function saveXml(r: Remittance): Promise<string | null> {
  const dir = await pickFolder();
  if (!dir) return null;
  return remittanceSave(r.id, dir);
}

function NewRemittance({
  kind,
  banks,
  onDone,
}: {
  kind: RemittanceKind;
  banks: BankChoice[];
  onDone: (msg: string) => void;
}) {
  const [cands, setCands] = useState<RemittanceCandidate[]>([]);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [until, setUntil] = useState('');
  const [iban, setIban] = useState(banks[0]?.iban ?? '');
  const [date, setDate] = useState(addDays(localToday(), 2));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    remittanceCandidates(kind, localToday())
      .then((c) => {
        setCands(c);
        setPicked(new Set(c.filter((x) => !x.problem).map((x) => key(x.item))));
      })
      .catch((e) => setError(errorMessage(e)));
  }, [kind]);
  useEffect(() => {
    if (!iban && banks[0]) setIban(banks[0].iban);
  }, [banks, iban]);

  const shown = cands.filter((c) => !until || c.item.due_date <= until);
  const chosen = shown.filter((c) => picked.has(key(c.item)) && !c.problem);
  const total = chosen.reduce((s, c) => s + c.item.amount, 0);
  const toggle = (k: string) => {
    const next = new Set(picked);
    if (next.has(k)) next.delete(k);
    else next.add(k);
    setPicked(next);
  };
  const create = async () => {
    setError(null);
    try {
      const r = await remittanceCreate(
        kind,
        chosen.map((c) => ({ invoice_id: c.item.invoice_id, seq: c.item.seq })),
        iban,
        date,
      );
      let msg = `Remesa ${r.reference} generada y contabilizada: ${r.items.length} ${
        r.items.length === 1 ? 'operación' : 'operaciones'
      }, ${euros(r.total)}.`;
      try {
        const path = await saveXml(r);
        if (path) msg += ` Fichero guardado en ${path}: súbelo a la banca electrónica.`;
      } catch (e) {
        msg += ` No se pudo guardar el fichero: ${errorMessage(e)}`;
      }
      onDone(msg);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const count = kind === 'cobros' ? plural(chosen.length, 'recibo') : plural(chosen.length, 'pago');
  return (
    <section aria-label={kind === 'cobros' ? 'Nueva remesa de recibos' : 'Nueva orden de pagos'}>
      {cands.length === 0 ? (
        <p className="text-stone-500">
          {kind === 'cobros'
            ? 'No hay recibos pendientes: salen los vencimientos de clientes con forma de pago «Recibo domiciliado» (Condiciones de pago).'
            : 'No hay pagos pendientes a proveedores que se paguen por transferencia.'}
        </p>
      ) : (
        <>
          <div className="flex flex-wrap items-end gap-3">
            <label>
              Vencen hasta
              <input
                type="date"
                className="mt-1 block"
                value={until}
                onChange={(e) => setUntil(e.target.value)}
              />
            </label>
            {banks.length > 0 ? (
              <label>
                Cuenta de la empresa
                <select
                  className="mt-1 block max-w-full"
                  value={iban}
                  onChange={(e) => setIban(e.target.value)}
                >
                  {banks.map((b) => (
                    <option key={b.iban} value={b.iban}>
                      {b.name} · {b.iban}
                    </option>
                  ))}
                </select>
              </label>
            ) : (
              <label>
                IBAN de la empresa
                <input
                  className="mt-1 block w-72 font-mono"
                  value={iban}
                  onChange={(e) => setIban(e.target.value)}
                />
              </label>
            )}
            <label>
              {kind === 'cobros' ? 'Fecha de cargo' : 'Fecha de ejecución'}
              <input
                type="date"
                className="mt-1 block"
                value={date}
                onChange={(e) => setDate(e.target.value)}
              />
            </label>
          </div>
          <table className="mt-2 w-full">
            <thead className="text-left text-stone-500">
              <tr>
                <th />
                <th>Vence</th>
                <th>Factura</th>
                <th>{kind === 'cobros' ? 'Cliente' : 'Proveedor'}</th>
                <th>IBAN</th>
                <th className="text-right">Importe</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((c) => (
                <tr key={key(c.item)} className="border-t border-stone-200 align-top">
                  <td>
                    <input
                      type="checkbox"
                      aria-label={`Incluir ${c.item.number}`}
                      disabled={!!c.problem}
                      checked={picked.has(key(c.item)) && !c.problem}
                      onChange={() => toggle(key(c.item))}
                    />
                  </td>
                  <td>{formatDate(c.item.due_date)}</td>
                  <td>
                    {c.item.number}
                    {c.item.installments > 1 && ` (${c.item.seq}/${c.item.installments})`}
                  </td>
                  <td>
                    {c.item.party}
                    {c.problem && <span className="block text-xs text-red-700">{c.problem}</span>}
                  </td>
                  <td className="font-mono text-xs">{c.iban ?? '—'}</td>
                  <td className="text-right tabular-nums">{euros(c.item.amount)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="mt-3 flex flex-wrap items-center gap-3">
            <button
              type="button"
              className="rounded btn-primary px-3 py-1 disabled:opacity-50"
              disabled={chosen.length === 0 || !iban}
              onClick={create}
            >
              Generar {kind === 'cobros' ? 'remesa' : 'orden'} ({count}, {euros(total)})
            </button>
            <span className="text-xs text-stone-500">
              {kind === 'cobros'
                ? 'Fichero SEPA de adeudos (pain.008, Cuaderno 19.14). Los recibos pasan a la 4312 hasta que el banco abone la remesa.'
                : 'Fichero SEPA de transferencias (pain.001, Cuaderno 34.14). Los pagos pasan a la 555 hasta que el banco cargue la orden.'}
            </span>
          </div>
        </>
      )}
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
    </section>
  );
}

function ReturnItem({
  r,
  i,
  account,
  onDone,
}: {
  r: Remittance;
  i: RemittanceItem;
  account: string;
  onDone: (msg: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [date, setDate] = useState(localToday());
  const [acc, setAcc] = useState(account);
  if (!open)
    return (
      <button type="button" className="text-stone-700" onClick={() => setOpen(true)}>
        {r.kind === 'cobros' ? 'Devuelto…' : 'Rechazado…'}
      </button>
    );
  return (
    <span className="inline-flex flex-wrap items-center justify-end gap-2">
      <input
        type="date"
        aria-label={`Fecha de la devolución de ${i.number}`}
        className={small}
        value={date}
        onChange={(e) => setDate(e.target.value)}
      />
      {r.in_bank && (
        <input
          aria-label={`Cuenta del banco de la devolución de ${i.number}`}
          className={`${small} w-24 font-mono`}
          value={acc}
          onChange={(e) => setAcc(e.target.value)}
        />
      )}
      <button
        type="button"
        className="rounded btn-primary px-2 py-0.5"
        onClick={() =>
          remittanceReturn(r.id, i.invoice_id, i.seq, date, r.in_bank ? acc : null)
            .then(() => onDone(`${i.number} vuelve a estar pendiente.`))
            .catch((e) => onDone(errorMessage(e)))
        }
      >
        Anotar devolución
      </button>
      <button type="button" className="px-1" onClick={() => setOpen(false)}>
        ×
      </button>
    </span>
  );
}

function RemittanceRow({
  r,
  banks,
  onDone,
}: {
  r: Remittance;
  banks: BankChoice[];
  onDone: (msg: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const bankAccount = banks.find((b) => b.iban === r.bank_iban)?.account ?? '572';
  const active = r.items.filter((i) => !i.returned_on).length;
  return (
    <li className="border-t border-stone-200 py-2">
      <div className="flex flex-wrap items-center gap-3">
        <button type="button" className="font-medium" onClick={() => setOpen(!open)}>
          {open ? '▾' : '▸'} {r.reference}
        </button>
        <span>
          {r.kind === 'cobros' ? 'Cargo' : 'Ejecución'} {formatDate(r.charge_date)}
        </span>
        <span>
          {active} {active === 1 ? 'operación' : 'operaciones'} · {euros(r.total)}
        </span>
        <span className={r.in_bank ? 'text-emerald-700' : 'text-stone-600'}>
          {r.in_bank
            ? r.kind === 'cobros'
              ? 'Abonada por el banco'
              : 'Cargada por el banco'
            : 'Pendiente del banco'}
        </span>
        <button
          type="button"
          className="text-stone-700 underline"
          onClick={() =>
            saveXml(r)
              .then((p) => p && onDone(`Fichero guardado en ${p}.`))
              .catch((e) => onDone(errorMessage(e)))
          }
        >
          Guardar fichero
        </button>
        {!r.in_bank && (
          <ConfirmAction
            label="Deshacer"
            question={`¿Deshacer la remesa ${r.reference}? Sus vencimientos vuelven a pendientes y se borran sus asientos. Hazlo sólo si no la has enviado al banco.`}
            confirmLabel="Sí, deshacer"
            onConfirm={() =>
              remittanceUndo(r.id, true)
                .then(() => onDone(`Remesa ${r.reference} deshecha.`))
                .catch((e) => onDone(errorMessage(e)))
            }
          />
        )}
      </div>
      {open && (
        <table className="mt-1 ml-5 w-[calc(100%-1.25rem)] text-xs">
          <tbody>
            {r.items.map((i) => (
              <tr key={key(i)} className="align-top">
                <td>{i.number}</td>
                <td>{i.party}</td>
                <td className="text-right tabular-nums">{euros(i.amount)}</td>
                <td className="pl-3 text-right">
                  {i.returned_on ? (
                    <span className="inline-flex flex-wrap items-center justify-end gap-2">
                      <span className="text-red-700">
                        {r.kind === 'cobros' ? 'Devuelto' : 'Rechazado'} el{' '}
                        {formatDate(i.returned_on)}
                      </span>
                      <ConfirmAction
                        label={r.kind === 'cobros' ? 'Deshacer devolución' : 'Deshacer rechazo'}
                        className="text-stone-700 underline"
                        question={`¿Deshacer la ${r.kind === 'cobros' ? 'devolución' : 'anotación de rechazo'} de ${i.number}? Vuelve a la remesa ${r.reference} como ${r.kind === 'cobros' ? 'cobrado' : 'pagado'} y se borra el asiento de la devolución; si estaba conciliado con un movimiento del banco, ese movimiento vuelve a pendientes.`}
                        confirmLabel="Sí, deshacer"
                        onConfirm={() =>
                          remittanceUndoReturn(r.id, i.invoice_id, i.seq, true)
                            .then(() => onDone(`${i.number} vuelve a la remesa ${r.reference}.`))
                            .catch((e) => onDone(errorMessage(e)))
                        }
                      />
                    </span>
                  ) : (
                    <ReturnItem r={r} i={i} account={bankAccount} onDone={onDone} />
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </li>
  );
}

/** Remesas SEPA: recibos domiciliados a clientes y transferencias a proveedores. */
export function Remesas() {
  const [kind, setKind] = useState<RemittanceKind>('cobros');
  const [list, setList] = useState<Remittance[]>([]);
  const [banks, setBanks] = useState<BankChoice[]>([]);
  const [settings, setSettings] = useState<SepaSettings | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [round, setRound] = useState(0);

  const load = useCallback(() => {
    remittanceList()
      .then(setList)
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  useEffect(() => {
    load();
    remittanceBanks()
      .then(setBanks)
      .catch(() => setBanks([]));
    sepaSettings()
      .then(setSettings)
      .catch(() => setSettings(null));
  }, [load]);
  const done = (m: string) => {
    setMsg(m);
    setRound((n) => n + 1);
    load();
  };
  const shown = list.filter((r) => r.kind === kind);

  return (
    <div className="text-sm">
      <div className="flex flex-wrap items-end gap-x-6 gap-y-3">
        <label>
          Tipo
          <select
            className="mt-1 block"
            value={kind}
            onChange={(e) => setKind(e.target.value as RemittanceKind)}
          >
            <option value="cobros">Recibos a clientes (adeudos)</option>
            <option value="pagos">Pagos a proveedores (transferencias)</option>
          </select>
        </label>
        {kind === 'cobros' && settings && (
          <span className="flex flex-wrap items-end gap-3">
            <label title="Lo da el banco al contratar los adeudos; casi siempre 000">
              Sufijo de acreedor
              <input
                className="mt-1 block w-20 font-mono"
                maxLength={3}
                value={settings.creditor_suffix}
                onChange={(e) => setSettings({ ...settings, creditor_suffix: e.target.value })}
              />
            </label>
            <label className="flex min-h-9 items-center gap-2">
              <input
                type="checkbox"
                checked={settings.b2b}
                onChange={(e) => setSettings({ ...settings, b2b: e.target.checked })}
              />
              B2B (sólo empresas)
            </label>
            <button
              type="button"
              className="rounded border border-stone-300 px-3"
              onClick={() =>
                saveSepaSettings(settings)
                  .then(() => setMsg('Ajustes SEPA guardados.'))
                  .catch((e) => setMsg(errorMessage(e)))
              }
            >
              Guardar
            </button>
          </span>
        )}
      </div>
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}
      <h3 className="mt-4 font-medium">
        {kind === 'cobros' ? 'Nueva remesa de recibos' : 'Nueva orden de transferencias'}
      </h3>
      <div className="mt-1">
        <NewRemittance key={`${kind}-${round}`} kind={kind} banks={banks} onDone={done} />
      </div>
      <h3 className="mt-5 font-medium">Generadas</h3>
      {shown.length === 0 ? (
        <p className="text-stone-500">Ninguna todavía.</p>
      ) : (
        <ul>
          {shown.map((r) => (
            <RemittanceRow key={r.id} r={r} banks={banks} onDone={done} />
          ))}
        </ul>
      )}
      <p className="mt-3 text-xs text-stone-500">
        TaxPain no envía nada al banco: genera el fichero para que lo subas a tu banca electrónica.
        Cuando el banco abone o cargue la remesa, cásala con el movimiento en la pestaña Banco.
      </p>
    </div>
  );
}

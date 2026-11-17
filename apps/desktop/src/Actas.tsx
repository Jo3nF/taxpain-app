import { useEffect, useState } from 'react';
import {
  type ActaRequest,
  actaSave,
  type DividendCertificate,
  type DividendPayment,
  dividendCertificates,
  dividendPayments,
  errorMessage,
  euros,
  formatDate,
  parseEuros,
} from './api';

const input = 'mt-1 block rounded-md border border-stone-300 px-2 py-1.5';
const cell = 'px-2 py-1 text-right tabular-nums';

type Msg = { ok: boolean; text: string } | null;

/** Botones «RTF» y «PDF» para guardar un acta. */
function SaveButtons({
  label,
  request,
  name,
  onMsg,
}: {
  label: string;
  request: () => ActaRequest | null;
  name: string;
  onMsg: (m: Msg) => void;
}) {
  const save = (pdf: boolean) => {
    const r = request();
    if (!r) return;
    actaSave(r, name, pdf)
      .then((p) => p && onMsg({ ok: true, text: `Guardado en ${p}.` }))
      .catch((e) => onMsg({ ok: false, text: errorMessage(e) }));
  };
  return (
    <div className="flex flex-wrap gap-2">
      <button
        type="button"
        className="btn-primary rounded-md px-3 py-1.5"
        onClick={() => save(false)}
      >
        {label} (RTF)…
      </button>
      <button
        type="button"
        className="rounded-md border border-stone-300 px-3 py-1.5"
        onClick={() => save(true)}
      >
        {label} (PDF)…
      </button>
    </div>
  );
}

/**
 * Actas de la junta (o decisiones del socio único) y certificados de retenciones de los socios:
 * aprobación de las cuentas de `year`, repartos de dividendos, reservas y certificados del año.
 */
export function Actas({ year }: { year: number }) {
  const [msg, setMsg] = useState<Msg>(null);
  const [payments, setPayments] = useState<DividendPayment[]>([]);
  const [certs, setCerts] = useState<DividendCertificate[]>([]);
  const [reserve, setReserve] = useState({
    date: '',
    from: '113',
    to: '112',
    amount: '',
    reason: '',
  });

  useEffect(() => {
    dividendPayments(year)
      .then(setPayments)
      .catch(() => setPayments([]));
    dividendCertificates(year)
      .then(setCerts)
      .catch(() => setCerts([]));
  }, [year]);

  const reserveRequest = (): ActaRequest | null => {
    const amount = parseEuros(reserve.amount);
    if (amount === null || amount <= 0) {
      setMsg({ ok: false, text: 'Pon el importe del traspaso.' });
      return null;
    }
    return {
      kind: 'reservas',
      date: reserve.date || null,
      from: reserve.from.trim(),
      to: reserve.to.trim(),
      amount,
      reason: reserve.reason,
    };
  };

  return (
    <div className="card p-4 text-sm">
      <h3 className="font-semibold">Actas y certificados a los socios</h3>
      <p className="mt-1 text-stone-600">
        Borradores con los datos de la contabilidad y de Socios: con un solo socio, decisiones del
        socio único (art. 15 LSC); si no, acta de la junta general. Lo que no sale de TaxPain
        (lugar, hora, asistentes, firmas) va entre corchetes. Complétalas, fírmalas y guárdalas en
        el libro de actas, que se legaliza con los demás libros.
      </p>

      <h4 className="mt-4 font-medium">Aprobación de las cuentas {year}</h4>
      <p className="text-stone-600">
        Con la aplicación del resultado contabilizada en el cierre (reservas, compensación de
        pérdidas, dividendo) o, si aún no lo está, con la propuesta.
      </p>
      <div className="mt-2">
        <SaveButtons
          label="Acta de cuentas"
          name={`Acta cuentas ${year}`}
          request={() => ({ kind: 'cuentas', year })}
          onMsg={setMsg}
        />
      </div>

      <h4 className="mt-4 font-medium">Repartos de dividendos de {year}</h4>
      {payments.length === 0 ? (
        <p className="text-stone-600">No hay pagos de dividendos registrados en Socios.</p>
      ) : (
        <ul className="mt-2 space-y-2">
          {payments.map((p) => (
            <li key={p.id} className="flex flex-wrap items-center gap-3">
              <span>
                {formatDate(p.date)} · bruto {euros(p.gross)} · retención {euros(p.withheld)}
              </span>
              <SaveButtons
                label="Acta"
                name={`Acta dividendos ${p.date}`}
                request={() => ({ kind: 'dividendo', payment_id: p.id })}
                onMsg={setMsg}
              />
            </li>
          ))}
        </ul>
      )}

      <h4 className="mt-4 font-medium">Dotación o traspaso de reservas</h4>
      <div className="form-grid mt-2">
        <label>
          Fecha
          <input
            type="date"
            className={input}
            value={reserve.date}
            onChange={(e) => setReserve({ ...reserve, date: e.target.value })}
          />
        </label>
        <label>
          De la cuenta
          <input
            className={input}
            value={reserve.from}
            onChange={(e) => setReserve({ ...reserve, from: e.target.value })}
          />
        </label>
        <label>
          A la cuenta
          <input
            className={input}
            value={reserve.to}
            onChange={(e) => setReserve({ ...reserve, to: e.target.value })}
          />
        </label>
        <label>
          Importe
          <input
            className={input}
            inputMode="decimal"
            value={reserve.amount}
            onChange={(e) => setReserve({ ...reserve, amount: e.target.value })}
          />
        </label>
        <label className="span-2">
          Motivo
          <input
            className={input}
            value={reserve.reason}
            onChange={(e) => setReserve({ ...reserve, reason: e.target.value })}
          />
        </label>
      </div>
      <div className="mt-2">
        <SaveButtons
          label="Acta de reservas"
          name={`Acta reservas ${year}`}
          request={reserveRequest}
          onMsg={setMsg}
        />
      </div>

      <h4 className="mt-4 font-medium">Certificados de retenciones {year}</h4>
      {certs.length === 0 ? (
        <p className="text-stone-600">Sin dividendos pagados en {year}.</p>
      ) : (
        <>
          <p className="text-stone-600">
            Uno por socio (art. 108.3 RIRPF), con los mismos pagos que el 193.
          </p>
          <table className="mt-2">
            <thead>
              <tr>
                <th className="px-2 py-1 text-left">Socio</th>
                <th className={cell}>Bruto</th>
                <th className={cell}>Retención</th>
                <th className={cell}>Líquido</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {certs.map((c) => (
                <tr key={c.nif}>
                  <td className="px-2 py-1">
                    {c.name} · {c.nif}
                  </td>
                  <td className={cell}>{euros(c.gross)}</td>
                  <td className={cell}>{euros(c.withheld)}</td>
                  <td className={cell}>{euros(c.net)}</td>
                  <td className="px-2 py-1">
                    <SaveButtons
                      label="Certificado"
                      name={`Certificado retenciones ${year} ${c.nif}`}
                      request={() => ({ kind: 'certificados', year, nif: c.nif })}
                      onMsg={setMsg}
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="mt-2">
            <SaveButtons
              label="Todos los certificados"
              name={`Certificados retenciones ${year}`}
              request={() => ({ kind: 'certificados', year, nif: null })}
              onMsg={setMsg}
            />
          </div>
        </>
      )}
      {msg && (
        <p role={msg.ok ? 'status' : 'alert'} className={`mt-2 ${msg.ok ? '' : 'text-red-700'}`}>
          {msg.text}
        </p>
      )}
    </div>
  );
}

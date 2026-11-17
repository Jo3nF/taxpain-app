import { useState } from 'react';
import {
  errorMessage,
  euros,
  money,
  parseEuros,
  parseRate,
  type ReceivedFx,
  receivedRegisterTyped,
} from './api';
import {
  CurrencyRate,
  CurrencySelect,
  emptyRate,
  type RateValue,
  receivedInEuros,
  toForeignCurrency,
} from './CurrencyRate';
import { localToday } from './JournalEntryForm';

const input = 'mt-1 block w-full rounded border border-stone-300 px-2 py-1';

/** Céntimos × centésimas de punto, redondeado al céntimo. */
const pct = (cents: number, bp: number) => Math.round((cents * bp) / 10000);

/** Trimestre de una fecha AAAA-MM-DD como número creciente (año × 4 + trimestre). */
const quarterOf = (d: string) =>
  Number(d.slice(0, 4)) * 4 + Math.floor((Number(d.slice(5, 7)) - 1) / 3);

/**
 * Factura recibida tecleada en el diario, sin documento: con su panel de IVA y retención. Se
 * registra como cualquier recibida revisada (asiento, libro de IVA, 303, 347, cartera).
 */
export function TypedInvoice({ onClose, onSaved }: { onClose: () => void; onSaved: () => void }) {
  const [v, setV] = useState({
    nif: '',
    name: '',
    number: '',
    date: localToday(),
    base: '',
    vatBp: '2100',
    vat: '',
    whBp: '0',
    nature: 'general' as 'general' | 'alquiler' | 'profesional',
    deductible: '100',
  });
  const [msg, setMsg] = useState<string | null>(null);
  /** Moneda de la factura («EUR» = euros): los importes se escriben en ella. */
  const [currency, setCurrency] = useState('EUR');
  const [fx, setFx] = useState<RateValue>(() => emptyRate(v.date));
  const code = currency !== 'EUR' ? currency : null;
  const base = parseEuros(v.base);
  const autoVat = base === null ? null : pct(base, Number(v.vatBp));
  const vat = v.vat.trim() ? parseEuros(v.vat) : autoVat;
  const wh = base === null ? null : pct(base, Number(v.whBp));
  const total = base !== null && vat !== null && wh !== null ? base + vat - wh : null;
  const micros = code ? parseRate(fx.rate) : null;
  const eur =
    micros !== null && base !== null && vat !== null && wh !== null && total !== null
      ? receivedInEuros({ base, vat, withholding: wh, total }, micros)
      : null;
  const set = (k: keyof typeof v) => (e: { target: { value: string } }) =>
    setV({ ...v, [k]: e.target.value });

  async function save() {
    if (!v.date) return setMsg('Falta la fecha de la factura');
    if (base === null || vat === null || wh === null || total === null) {
      setMsg('Revisa los importes');
      return;
    }
    const pctText = v.deductible.trim();
    if (!/^\d{1,3}$/.test(pctText) || Number(pctText) > 100) {
      return setMsg('El IVA deducible es un porcentaje entero entre 0 y 100');
    }
    // En moneda extranjera, los importes escritos son en esa moneda: se pasan a euros al tipo.
    let amounts = { base, vat, withholding: wh, total };
    let fxAmounts: ReceivedFx | null = null;
    if (code) {
      const c = toForeignCurrency(code, fx);
      if (typeof c === 'string') return setMsg(c);
      const inEur = receivedInEuros(amounts, parseRate(c.rate) ?? 1);
      if (!inEur) return setMsg('Revisa los importes');
      fxAmounts = { currency: c, ...amounts };
      amounts = inEur;
    }
    try {
      await receivedRegisterTyped(
        {
          supplier_nif: v.nif.trim().toUpperCase(),
          supplier_name: v.name.trim(),
          number: v.number.trim(),
          issue_date: v.date,
          ...amounts,
          deductible_pct: Number(pctText),
          capital_good: false,
          nature: v.nature,
          asset_class: null,
        },
        fxAmounts,
      );
      setMsg(`Factura ${v.number.trim()} registrada con su asiento.`);
      // La siguiente factura empieza limpia: sin la retención, naturaleza ni % de la anterior.
      setV({
        ...v,
        number: '',
        base: '',
        vat: '',
        whBp: '0',
        nature: 'general',
        deductible: '100',
      });
      onSaved();
    } catch (e) {
      setMsg(errorMessage(e));
    }
  }

  return (
    <section
      aria-label="Factura recibida sin documento"
      className="form-grid mt-3 rounded-md border border-stone-200 bg-white p-4 text-sm"
      onKeyDown={(e) => {
        if (e.key === 'Escape') onClose();
      }}
    >
      <p className="span-full text-stone-600">
        Para facturas que no tienes en PDF o XML. Si la tienes, mejor súbela en Recibidas: queda
        guardada con el asiento.
      </p>
      <label>
        NIF del proveedor
        <input className={input} value={v.nif} onChange={set('nif')} />
      </label>
      <label className="span-2">
        Nombre
        <input className={input} value={v.name} onChange={set('name')} />
      </label>
      <label>
        Número
        <input className={input} value={v.number} onChange={set('number')} />
      </label>
      <label>
        Fecha
        <input type="date" className={input} value={v.date} onChange={set('date')} />
      </label>
      <CurrencySelect
        value={currency}
        className={input}
        onChange={(c) => {
          // El tipo de otra moneda (y su «BCE») no vale para esta: se empieza de cero.
          if (c !== currency) setFx(emptyRate(v.date || fx.rateDate));
          setCurrency(c);
        }}
      />
      {code && (
        <CurrencyRate code={code} value={fx} accrual={v.date} onChange={setFx} inputClass={input} />
      )}
      <label>
        {code ? `Base (${code})` : 'Base'}
        <input className={input} inputMode="decimal" value={v.base} onChange={set('base')} />
      </label>
      <label>
        Tipo de IVA
        <select className={input} value={v.vatBp} onChange={set('vatBp')}>
          <option value="2100">21 %</option>
          <option value="1000">10 %</option>
          <option value="400">4 %</option>
          <option value="0">Sin IVA (exenta)</option>
        </select>
      </label>
      <label>
        Cuota de IVA
        <input
          className={input}
          inputMode="decimal"
          placeholder={autoVat === null ? '' : money(autoVat, code)}
          value={v.vat}
          onChange={set('vat')}
        />
      </label>
      <label>
        Retención
        <select className={input} value={v.whBp} onChange={set('whBp')}>
          <option value="0">Sin retención</option>
          <option value="700">7 % (profesional, inicio)</option>
          <option value="1500">15 % (profesional)</option>
          <option value="1900">19 % (alquiler)</option>
        </select>
      </label>
      <label>
        Naturaleza
        <select className={input} value={v.nature} onChange={set('nature')}>
          <option value="general">Compra o servicio</option>
          <option value="profesional">Profesional (111)</option>
          <option value="alquiler">Alquiler (115)</option>
        </select>
      </label>
      <label>
        IVA deducible %
        <input
          className={input}
          inputMode="numeric"
          value={v.deductible}
          onChange={set('deductible')}
        />
      </label>
      <p className="span-full" aria-live="polite">
        Total: {total === null ? '—' : money(total, code)}
        {wh ? ` (retención ${money(wh, code)})` : ''}
        {eur && ` · en euros: ${euros(eur.total)} (base ${euros(eur.base)}, IVA ${euros(eur.vat)})`}
      </p>
      {/^\d{4}-\d{2}-\d{2}$/.test(v.date) && quarterOf(v.date) < quarterOf(localToday()) && (
        <p className="span-full rounded-md bg-amber-50 p-2 text-amber-900">
          La factura es de un trimestre anterior: su IVA se deduce en el 303 del trimestre en que la
          registras (el actual), no en el de su fecha. Se puede deducir hasta cuatro años después
          (art. 99 LIVA).
        </p>
      )}
      <div className="span-full form-actions">
        <button type="button" className="rounded-md btn-primary px-3 py-1" onClick={save}>
          Registrar la factura
        </button>
        <button type="button" className="btn-ghost" onClick={onClose}>
          Cerrar
        </button>
      </div>
      {msg && (
        <p role="status" className="span-full">
          {msg}
        </p>
      )}
    </section>
  );
}

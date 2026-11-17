import { useState } from 'react';
import {
  CURRENCIES,
  errorMessage,
  type ForeignCurrency,
  formatDate,
  fxBceRate,
  parseRate,
  rateDateWarning,
  rateDecimal,
  rateText,
  toEur,
} from './api';

/** Tipo de cambio tal como se escribe en un formulario. */
export interface RateValue {
  /** 1 € = `rate` unidades de la moneda («1,1193»). */
  rate: string;
  rateDate: string;
  source: 'bce' | 'manual';
}

export const emptyRate = (rateDate = ''): RateValue => ({ rate: '', rateDate, source: 'manual' });

/** Lo guardado → formulario. */
export function rateValueOf(c: ForeignCurrency): RateValue {
  const micros = parseRate(c.rate);
  return { rate: micros ? rateText(micros) : c.rate, rateDate: c.rate_date, source: c.source };
}

/** Formulario → moneda del borrador, o el error que mostrar. */
export function toForeignCurrency(code: string, v: RateValue): ForeignCurrency | string {
  const micros = parseRate(v.rate);
  if (micros === null) return `Pon el tipo de cambio (1 € = … ${code})`;
  if (!v.rateDate) return 'Pon la fecha del tipo de cambio';
  return { code, rate: rateDecimal(micros), rate_date: v.rateDate, source: v.source };
}

/** Selector de moneda: «EUR» es el euro. */
export function CurrencySelect({
  value,
  onChange,
  className,
}: {
  value: string;
  onChange: (code: string) => void;
  className: string;
}) {
  return (
    <label className="text-sm">
      Moneda
      <select
        className={className}
        value={value || 'EUR'}
        onChange={(e) => onChange(e.target.value)}
      >
        <option value="EUR">Euro (EUR)</option>
        {CURRENCIES.map((c) => (
          <option key={c.code} value={c.code}>
            {c.code} · {c.name}
          </option>
        ))}
      </select>
    </label>
  );
}

/**
 * Tipo de cambio (1 € = … moneda), su fecha y el botón «Tipo del BCE», que descarga el tipo de
 * referencia vigente en `accrual` (la fecha de la operación) sólo cuando se pulsa. Los hijos van
 * en la rejilla del formulario.
 */
export function CurrencyRate({
  code,
  value,
  accrual,
  onChange,
  inputClass,
  warn = true,
  oldRateOk = false,
}: {
  code: string;
  value: RateValue;
  /** Fecha del devengo: la de la operación o, si no, la de la factura. */
  accrual: string;
  onChange: (v: RateValue) => void;
  inputClass: string;
  /** Avisar si el tipo no es el vigente al devengo (no en un presupuesto). */
  warn?: boolean;
  /** Un tipo anterior vale (rectificativa al tipo de la original): sólo se avisa si es posterior. */
  oldRateOk?: boolean;
}) {
  const [status, setStatus] = useState<{ busy: boolean; text: string | null; ok: boolean }>({
    busy: false,
    text: null,
    ok: false,
  });
  const ecb = CURRENCIES.find((c) => c.code === code)?.ecb ?? false;
  const invalid = value.rate.trim() !== '' && parseRate(value.rate) === null;
  const warning = warn ? rateDateWarning(value.rateDate, accrual, oldRateOk) : null;

  async function bce() {
    if (!accrual) {
      setStatus({ busy: false, ok: false, text: 'Pon antes la fecha de la factura.' });
      return;
    }
    setStatus({ busy: true, text: null, ok: false });
    try {
      const r = await fxBceRate(code, accrual);
      const micros = parseRate(r.rate);
      onChange({ rate: micros ? rateText(micros) : r.rate, rateDate: r.date, source: 'bce' });
      setStatus({
        busy: false,
        ok: true,
        text: `Tipo de referencia del BCE del ${formatDate(r.date)}${r.date !== accrual ? ' (último publicado antes de esa fecha)' : ''}.`,
      });
    } catch (e) {
      setStatus({
        busy: false,
        ok: false,
        text: `No se pudo consultar al BCE: ${errorMessage(e)}`,
      });
    }
  }

  return (
    <>
      <label className="text-sm">
        Tipo (1 € = … {code})
        <input
          className={inputClass}
          inputMode="decimal"
          aria-invalid={invalid}
          value={value.rate}
          onChange={(e) => onChange({ ...value, rate: e.target.value, source: 'manual' })}
        />
      </label>
      <label className="text-sm">
        Fecha del tipo
        <input
          type="date"
          className={inputClass}
          value={value.rateDate}
          onChange={(e) => onChange({ ...value, rateDate: e.target.value, source: 'manual' })}
        />
      </label>
      <div className="flex items-end text-sm">
        {ecb && (
          <button
            type="button"
            className="rounded-md border border-stone-300 px-3 py-1.5 disabled:opacity-50"
            disabled={status.busy}
            onClick={() => void bce()}
          >
            {status.busy ? 'Consultando al BCE…' : 'Tipo del BCE'}
          </button>
        )}
      </div>
      <p
        role="status"
        className={`col-span-full text-sm ${
          invalid || (status.text && !status.ok)
            ? 'text-red-700'
            : warning
              ? 'text-amber-800'
              : status.text
                ? 'text-emerald-700'
                : 'text-xs text-stone-500'
        }`}
      >
        {invalid
          ? 'El tipo es un número con hasta 6 decimales (p. ej. 1,1193).'
          : (warning ??
            status.text ??
            (ecb
              ? '«Tipo del BCE» descarga el tipo de referencia de la fecha de la operación (sólo envía la moneda y la fecha).'
              : 'El BCE no publica esta moneda: escribe el tipo vigente el día de la operación.'))}
      </p>
    </>
  );
}

/** Céntimos × centésimas de punto, redondeado al céntimo (la mitad, hacia fuera). */
function applyBp(cents: number, bp: number): number {
  const q = Math.round((Math.abs(cents) * bp) / 10000);
  return cents < 0 ? -q : q;
}

/** Tipos de IVA (con y sin recargo) con los que se reconoce la cuota (`received::euro_vat`). */
const VAT_RATES_BP = [2100, 1000, 500, 400, 2620, 1140, 562, 450];

/**
 * IVA en euros de una factura en otra moneda como lo calcula quien la emite (art. 12 del
 * Reglamento de facturación): la base en euros por el tipo de IVA, si la cuota en la moneda es
 * la de un tipo español; si no, la cuota convertida al tipo de cambio.
 */
export function euroVat(base: number, vat: number, micros: number): number {
  if (vat === 0) return 0;
  const bp = VAT_RATES_BP.find((r) => applyBp(base, r) === vat);
  return bp === undefined ? toEur(vat, micros) : applyBp(toEur(base, micros), bp);
}

/**
 * Importes en euros de una factura en moneda extranjera, como los calcula el proceso nativo:
 * base y retención al tipo, el IVA en euros de la factura (`vatEur`, o [`euroVat`]) y el total
 * sumado igual que en la moneda. `null` si no cuadran.
 */
export function receivedInEuros(
  a: { base: number; vat: number; withholding: number; total: number },
  micros: number,
  vatEur: number | null = null,
): { base: number; vat: number; withholding: number; total: number } | null {
  const base = toEur(a.base, micros);
  const vat = vatEur ?? euroVat(a.base, a.vat, micros);
  const withholding = toEur(a.withholding, micros);
  if (a.total === a.base + a.vat - a.withholding) {
    return { base, vat, withholding, total: base + vat - withholding };
  }
  if (a.total === a.base + a.vat) return { base, vat, withholding, total: base + vat };
  return null;
}

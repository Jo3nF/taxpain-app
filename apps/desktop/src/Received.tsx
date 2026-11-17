import { Sparkles } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  type AssetClass,
  CURRENCIES,
  type ExtractedField,
  type Extraction,
  errorMessage,
  euros,
  formatDate,
  modelSettings,
  money,
  onFileDrop,
  type ProrataUse,
  parseEuros,
  parseRate,
  pickFolder,
  pickReceivedFiles,
  plural,
  type ReceivedCheck,
  type ReceivedFx,
  type ReceivedResumen,
  type ReceivedSummary,
  type Rectificable,
  type Reviewed,
  rateText,
  receivedAiExtract,
  receivedAiFillPending,
  receivedCheck,
  receivedCorrect,
  receivedDiscard,
  receivedImport,
  receivedList,
  receivedOpen,
  receivedReview,
  receivedUndiscard,
  receivedUnreview,
  receivedUnreviewCheck,
  setWatchedFolders,
  watchedFolders,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import {
  CurrencyRate,
  CurrencySelect,
  emptyRate,
  euroVat,
  type RateValue,
  rateValueOf,
  receivedInEuros,
  toForeignCurrency,
} from './CurrencyRate';

const input = 'mt-1 block w-full rounded-md border px-2 py-1';

/** Confianza de la extracción, para el borde del campo: lo dudoso salta a la vista. */
function confidence(f: ExtractedField<unknown> | null): 'falta' | 'baja' | 'media' | undefined {
  if (!f) return 'falta';
  return f.confidence === 'Alta' ? undefined : f.confidence === 'Media' ? 'media' : 'baja';
}

/** Céntimos → «1234,56» para un campo del formulario. */
const centsText = (c: number) => (c / 100).toFixed(2).replace('.', ',');

/** Lo pendiente de confirmar: los datos y lo que ha devuelto la comprobación. */
interface PendingConfirm {
  check: ReceivedCheck;
  reviewed: Reviewed;
  fx: ReceivedFx | null;
}

/**
 * Formulario de revisión de una recibida. `correct`: corregir una ya revisada (empieza con los
 * datos que se confirmaron y rehace su asiento); si no, la primera revisión (con lo extraído).
 */
function ReviewForm({
  r,
  onDone,
  correct = false,
  onCancel,
}: {
  r: ReceivedSummary;
  onDone: (message?: string, rectificar?: Rectificable[]) => void;
  correct?: boolean;
  onCancel?: () => void;
}) {
  // Lo que propone el asistente sustituye a la extracción automática (siempre para revisar).
  const [ai, setAi] = useState<Extraction | null>(null);
  const [aiBusy, setAiBusy] = useState(false);
  const e = ai ?? r.extraction;
  /** Al corregir: lo confirmado (en la moneda de la factura si es extranjera). */
  const was = correct ? (r.reviewed ?? null) : null;
  const wasFx = correct ? (r.fx ?? null) : null;
  const cents = (f: ExtractedField<number> | null) => (f ? centsText(f.value) : '');
  const [v, setV] = useState(() =>
    was
      ? {
          supplier_nif: was.supplier_nif,
          supplier_name: was.supplier_name,
          number: was.number,
          issue_date: was.issue_date,
          base: centsText(wasFx?.base ?? was.base),
          vat: centsText(wasFx?.vat ?? was.vat),
          withholding: centsText(wasFx?.withholding ?? was.withholding),
          total: centsText(wasFx?.total ?? was.total),
          deductible_pct: String(was.deductible_pct),
        }
      : {
          supplier_nif: e.supplier_nif?.value ?? '',
          supplier_name: e.supplier_name?.value ?? '',
          number: e.number?.value ?? '',
          issue_date: e.issue_date?.value ?? '',
          base: cents(e.base),
          vat: cents(e.vat),
          withholding: cents(e.withholding) || '0,00',
          total: cents(e.total),
          deductible_pct: '100',
        },
  );
  const [nature, setNature] = useState<'general' | 'alquiler' | 'profesional'>(
    was?.nature ?? (e.withholding ? 'profesional' : 'general'),
  );
  const [capitalGood, setCapitalGood] = useState(was?.capital_good ?? false);
  const [regime, setRegime] = useState<'interior' | 'intracomunitaria' | 'isp'>(
    was?.vat_regime ?? 'interior',
  );
  const [country, setCountry] = useState(was?.supplier_country ?? '');
  const [rate, setRate] = useState(was?.self_assessed_rate ?? 2100);
  const [goods, setGoods] = useState(was?.intra_goods ?? false);
  const [cashBasis, setCashBasis] = useState(was?.cash_basis ?? false);
  const [dua, setDua] = useState(was?.import_dua ?? false);
  const [assetClass, setAssetClass] = useState<AssetClass>(was?.asset_class ?? 'informatica');
  const [prorataUse, setProrataUse] = useState<ProrataUse>(was?.prorata_use ?? 'comun');
  const [special, setSpecial] = useState(false);
  /**
   * Moneda de la factura («EUR» = euros) y su tipo de cambio: los de la factura si los trae (una
   * Facturae en dólares, por ejemplo), para comprobarlos.
   */
  const invoiceFx = e.fx && CURRENCIES.some((c) => c.code === e.fx?.code) ? e.fx : null;
  const [currency, setCurrency] = useState(
    wasFx?.currency.code ?? (was ? 'EUR' : (invoiceFx?.code ?? 'EUR')),
  );
  const [fx, setFx] = useState<RateValue>(() => {
    if (wasFx) return rateValueOf(wasFx.currency);
    const micros = invoiceFx?.rate ? parseRate(invoiceFx.rate) : null;
    const date = invoiceFx?.rate_date ?? e.issue_date?.value ?? '';
    return micros === null
      ? emptyRate(date)
      : { rate: rateText(micros), rateDate: date, source: 'manual' };
  });
  /** IVA en euros que figura en la factura (vacío: la base en euros por el tipo de IVA). */
  const [vatEur, setVatEur] = useState(() => {
    const stored = wasFx ? wasFx.vat_eur : invoiceFx?.vat_eur;
    return stored != null ? centsText(stored) : '';
  });
  const [confirm, setConfirm] = useState<PendingConfirm | null>(null);
  const [busy, setBusy] = useState(false);
  const code = currency !== 'EUR' ? currency : null;
  const fxMicros = code ? parseRate(fx.rate) : null;
  const [baseCents, vatCents] = [parseEuros(v.base), parseEuros(v.vat)];
  const autoVatEur =
    fxMicros !== null && baseCents !== null && vatCents !== null
      ? euroVat(baseCents, vatCents, fxMicros)
      : null;
  /** El IVA en euros escrito (sólo con IVA en la factura): `undefined` si no se entiende. */
  const typedVatEur =
    regime === 'interior' && vatEur.trim() ? (parseEuros(vatEur) ?? undefined) : null;
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    modelSettings()
      .then((s) => setSpecial(s.prorata != null && (s.prorata_special ?? false)))
      .catch((e) =>
        console.warn(
          'Ajustes de los modelos no disponibles (sin prorrata especial):',
          errorMessage(e),
        ),
      );
  }, []);

  async function save() {
    // Lo obligatorio, antes de llamar al proceso nativo (si no, el error sería técnico).
    if (!v.issue_date) return setError('Falta la fecha de la factura');
    if (!v.number.trim()) return setError('Falta el número de factura');
    const amounts = [v.base, v.vat, v.withholding, v.total].map(parseEuros);
    if (amounts.some((a) => a === null)) return setError('Revisa los importes');
    const [base = 0, vat = 0, withholding = 0, total = 0] = amounts as number[];
    // En moneda extranjera los importes escritos son en esa moneda: se pasan a euros al tipo.
    let fxAmounts: ReceivedFx | null = null;
    let eur = { base, vat, withholding, total };
    if (code) {
      const c = toForeignCurrency(code, fx);
      if (typeof c === 'string') return setError(c);
      const micros = parseRate(c.rate) ?? 1;
      if (typedVatEur === undefined) return setError('Revisa el IVA en euros');
      const inEur = receivedInEuros({ base, vat, withholding, total }, micros, typedVatEur);
      if (!inEur)
        return setError(`Los importes en ${code} no cuadran: base + IVA − retención ≠ total`);
      fxAmounts = { currency: c, base, vat, withholding, total, vat_eur: typedVatEur };
      eur = inEur;
    }
    const pctText = v.deductible_pct.trim();
    const pct = Number(pctText);
    if (!/^\d{1,3}$/.test(pctText) || pct > 100) {
      return setError('El IVA deducible es un porcentaje entero entre 0 y 100');
    }
    // Art. 108.Dos.5.º LIVA: por debajo de 3.005,06 € (sin IVA) no es bien de inversión. Marcado,
    // iría a las casillas [30]/[31] del 303 y a amortizar, en vez de a [28]/[29] y a gasto del año.
    if (capitalGood && eur.base < 300_506) {
      return setError(
        'Por debajo de 3.005,06 € sin IVA no es un bien de inversión a efectos del IVA (art. 108.Dos LIVA): desmarca la casilla y queda como gasto del año.',
      );
    }
    setError(null);
    try {
      const reviewed: Reviewed = {
        supplier_nif: v.supplier_nif,
        supplier_name: v.supplier_name,
        number: v.number,
        issue_date: v.issue_date,
        ...eur,
        deductible_pct: pct,
        capital_good: capitalGood,
        nature,
        asset_class: capitalGood ? assetClass : null,
        vat_regime: regime,
        supplier_country: regime === 'interior' ? null : country.trim().toUpperCase() || null,
        self_assessed_rate: regime === 'interior' ? null : rate,
        intra_goods: regime === 'intracomunitaria' && goods,
        cash_basis: regime === 'interior' && cashBasis,
        import_dua: regime === 'interior' && dua,
        prorata_use: special ? prorataUse : 'comun',
      };
      // Primero se comprueba y se enseña el resumen; se graba al confirmarlo.
      const check = await receivedCheck(r.id, reviewed, fxAmounts);
      setConfirm({ check, reviewed, fx: fxAmounts });
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  async function apply() {
    if (!confirm) return;
    setBusy(true);
    setError(null);
    try {
      if (correct) {
        const out = await receivedCorrect(r.id, confirm.reviewed, confirm.fx);
        const extra = [
          out.ejercicio_anterior != null
            ? `el asiento pasa del ejercicio ${out.ejercicio_anterior}`
            : null,
          out.punteos_quitados > 0
            ? `${plural(out.punteos_quitados, 'línea punteada', 'líneas punteadas')} sin punteo (cambió su importe)`
            : null,
        ].filter(Boolean);
        onDone(
          `Factura ${confirm.check.despues.numero} corregida y su asiento rehecho${
            extra.length ? `: ${extra.join('; ')}` : ''
          }.${confirm.check.avisos.length ? ' Revisa los avisos en Modelos.' : ''}`,
          out.rectificar ?? confirm.check.rectificar ?? [],
        );
      } else {
        await receivedReview(r.id, confirm.reviewed, confirm.fx);
        onDone();
      }
    } catch (err) {
      setError(errorMessage(err));
      setConfirm(null);
    } finally {
      setBusy(false);
    }
  }

  const field = (
    key: keyof typeof v,
    label: string,
    f: ExtractedField<unknown> | null,
    type = 'text',
    className?: string,
  ) => (
    <label className={className}>
      {label}
      <input
        type={type}
        required={key === 'issue_date' || key === 'number'}
        data-confidence={confidence(f)}
        value={v[key]}
        onChange={(ev) => setV({ ...v, [key]: ev.target.value })}
      />
    </label>
  );

  return (
    <div className="mt-2 space-y-4 rounded-md border border-stone-200 p-4">
      <div className="form-grid">
        {field(
          'supplier_nif',
          regime === 'interior' ? 'NIF proveedor' : 'NIF-IVA o identificación',
          e.supplier_nif,
        )}
        {field('supplier_name', 'Proveedor', e.supplier_name, 'text', 'span-2')}
        {field('number', 'Número', e.number)}
        {field('issue_date', 'Fecha', e.issue_date, 'date')}
      </div>

      <div className="form-grid">
        {field('base', code ? `Base (${code})` : 'Base', e.base)}
        {field('vat', code ? `IVA (${code})` : 'IVA', e.vat)}
        {field('withholding', code ? `Retención (${code})` : 'Retención', e.withholding)}
        {field('total', code ? `Total (${code})` : 'Total', e.total)}
        <CurrencySelect
          value={currency}
          className={input}
          onChange={(c) => {
            // El tipo de otra moneda (y su «BCE») no vale para esta: se empieza de cero.
            if (c !== currency) setFx(emptyRate(v.issue_date || fx.rateDate));
            setCurrency(c);
          }}
        />
        {code && (
          <>
            <CurrencyRate
              code={code}
              value={fx}
              accrual={v.issue_date}
              onChange={setFx}
              inputClass={input}
            />
            {regime === 'interior' && (
              <label
                className="span-2"
                title="La factura en otra moneda dice la cuota de IVA en euros (art. 12 del Reglamento de facturación): es la que se deduce."
              >
                IVA en euros (el de la factura)
                <input
                  className={input}
                  inputMode="decimal"
                  aria-invalid={typedVatEur === undefined}
                  placeholder={autoVatEur === null ? '' : euros(autoVatEur)}
                  value={vatEur}
                  onChange={(ev) => setVatEur(ev.target.value)}
                />
              </label>
            )}
            <EurPreview code={code} v={v} fx={fx} vatEur={typedVatEur ?? null} />
          </>
        )}
      </div>

      <div className="form-grid">
        <label className="span-2">
          Tipo de gasto
          <select
            className={input}
            value={nature}
            onChange={(ev) => setNature(ev.target.value as typeof nature)}
          >
            <option value="general">General</option>
            <option value="profesional">Servicios profesionales (retención → 111/190)</option>
            <option value="alquiler">Alquiler de local (retención → 115/180)</option>
          </select>
        </label>
        <label className="span-2">
          IVA de la compra
          <select
            className={input}
            value={regime}
            onChange={(ev) => {
              const next = ev.target.value as typeof regime;
              setRegime(next);
              if (next !== 'interior') setV({ ...v, vat: '0,00' });
            }}
          >
            <option value="interior">Con IVA en la factura (proveedor español)</option>
            <option value="intracomunitaria">Proveedor de otro país de la UE (sin IVA)</option>
            <option value="isp">
              Proveedor de fuera de la UE u otra inversión del sujeto pasivo
            </option>
          </select>
        </label>
        <label>
          IVA deducible (%)
          <input
            className={input}
            inputMode="numeric"
            maxLength={3}
            title="Menos de 100 si la compra no es sólo para la actividad (p. ej. 50 en un vehículo de uso mixto)"
            value={v.deductible_pct}
            onChange={(ev) => setV({ ...v, deductible_pct: ev.target.value })}
          />
        </label>
        {regime !== 'interior' && (
          <>
            <label>
              País (p. ej. IE)
              <input
                className={input}
                value={country}
                maxLength={2}
                onChange={(ev) => setCountry(ev.target.value)}
              />
            </label>
            <label>
              IVA a autorrepercutir
              <select
                className={input}
                value={rate}
                onChange={(ev) => setRate(Number(ev.target.value))}
              >
                <option value={2100}>21 %</option>
                <option value={1000}>10 %</option>
                <option value={400}>4 %</option>
              </select>
            </label>
            <p className="span-full text-xs text-stone-600">
              La factura no lleva IVA: TaxPain lo declara como devengado y como deducible en el 303
              (
              {regime === 'intracomunitaria'
                ? '[10]/[11] y [36]/[37]; también va al 349'
                : '[12]/[13] y [28]/[29]'}
              ).
            </p>
          </>
        )}
        {special && (
          <label className="span-2">
            Destino de la compra (prorrata especial)
            <select
              className={input}
              value={prorataUse}
              onChange={(ev) => setProrataUse(ev.target.value as ProrataUse)}
            >
              <option value="comun">Uso común: se deduce con el porcentaje de prorrata</option>
              <option value="plena">
                Sólo operaciones con derecho a deducir: se deduce entera
              </option>
              <option value="ninguna">Sólo operaciones exentas: no se deduce</option>
            </select>
          </label>
        )}
      </div>

      <div className="flex flex-col items-start gap-2.5">
        {regime === 'intracomunitaria' && (
          <label>
            <input type="checkbox" checked={goods} onChange={(ev) => setGoods(ev.target.checked)} />
            Son bienes (si no, servicios: software, publicidad, alojamiento…)
          </label>
        )}
        {regime === 'interior' && (
          <label>
            <input
              type="checkbox"
              checked={cashBasis}
              onChange={(ev) => setCashBasis(ev.target.checked)}
            />
            La factura dice «Régimen especial del criterio de caja» (el IVA se deduce al pagarla)
          </label>
        )}
        {regime === 'interior' && (
          <label>
            <input type="checkbox" checked={dua} onChange={(ev) => setDua(ev.target.checked)} />
            Es un DUA de importación (base = valor en aduana; total = IVA pagado en la aduana)
          </label>
        )}
        <label>
          <input
            type="checkbox"
            checked={capitalGood}
            onChange={(ev) => setCapitalGood(ev.target.checked)}
          />
          Es un bien de inversión: más de 3.005,06 € sin IVA y dura más de un año (ordenador,
          mobiliario, vehículo…)
        </label>
      </div>
      {capitalGood && (
        <div className="form-grid">
          <label className="span-2">
            Tipo de bien (se dará de alta en el inmovilizado y se amortizará)
            <select
              className={input}
              value={assetClass}
              onChange={(ev) => setAssetClass(ev.target.value as AssetClass)}
            >
              <option value="informatica">Equipos informáticos (217 · 25 %)</option>
              <option value="mobiliario">Mobiliario (216 · 10 %)</option>
              <option value="transporte">Vehículos (218 · 16 %)</option>
              <option value="maquinaria">Maquinaria (213 · 12 %)</option>
              <option value="utillaje">Útiles y herramientas (214 · 25 %)</option>
              <option value="instalaciones">Instalaciones (215 · 10 %)</option>
              <option value="construcciones">Edificios, sin el suelo (211 · 2 %)</option>
            </select>
          </label>
        </div>
      )}

      {e.warnings.length > 0 && (
        <ul className="text-xs text-amber-800">
          {e.warnings.map((w) => (
            <li key={w}>⚠ {w}</li>
          ))}
        </ul>
      )}
      {confirm ? (
        <ConfirmPanel
          check={confirm.check}
          busy={busy}
          onConfirm={apply}
          onBack={() => setConfirm(null)}
        />
      ) : (
        <div className="form-actions border-t border-stone-200 pt-4">
          <button type="button" className="rounded-md btn-primary px-3 py-1 text-sm" onClick={save}>
            {correct ? 'Revisar la corrección' : 'Confirmar datos'}
          </button>
          {correct ? (
            <>
              <button
                type="button"
                className="btn-ghost"
                onClick={() => receivedOpen(r.id).catch((e) => setError(errorMessage(e)))}
              >
                Ver documento
              </button>
              <button type="button" className="btn-ghost" onClick={onCancel}>
                Cancelar
              </button>
            </>
          ) : (
            <PendingActions
              r={r}
              v={v}
              setV={setV}
              aiBusy={aiBusy}
              setAiBusy={setAiBusy}
              setAi={setAi}
              setError={setError}
              onDone={onDone}
            />
          )}
        </div>
      )}
      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}
    </div>
  );
}

type FormValues = {
  supplier_nif: string;
  supplier_name: string;
  number: string;
  issue_date: string;
  base: string;
  vat: string;
  withholding: string;
  total: string;
  deductible_pct: string;
};

/** Los botones de una pendiente: rellenar con el asistente, ver el documento y descartar. */
function PendingActions({
  r,
  v,
  setV,
  aiBusy,
  setAiBusy,
  setAi,
  setError,
  onDone,
}: {
  r: ReceivedSummary;
  v: FormValues;
  setV: (v: FormValues) => void;
  aiBusy: boolean;
  setAiBusy: (b: boolean) => void;
  setAi: (x: Extraction) => void;
  setError: (e: string | null) => void;
  onDone: () => void;
}) {
  return (
    <>
      <button
        type="button"
        disabled={aiBusy}
        className="btn-ghost disabled:opacity-50"
        onClick={async () => {
          setAiBusy(true);
          setError(null);
          try {
            const x = await receivedAiExtract(r.id);
            setAi(x);
            const c = (f: ExtractedField<number> | null, prev: string) =>
              f ? (f.value / 100).toFixed(2).replace('.', ',') : prev;
            setV({
              ...v,
              supplier_nif: x.supplier_nif?.value ?? v.supplier_nif,
              supplier_name: x.supplier_name?.value ?? v.supplier_name,
              number: x.number?.value ?? v.number,
              issue_date: x.issue_date?.value ?? v.issue_date,
              base: c(x.base, v.base),
              vat: c(x.vat, v.vat),
              withholding: c(x.withholding, v.withholding),
              total: c(x.total, v.total),
            });
          } catch (err) {
            setError(errorMessage(err));
          } finally {
            setAiBusy(false);
          }
        }}
      >
        {aiBusy ? 'Preguntando…' : 'Rellenar con el asistente'}
      </button>
      <button
        type="button"
        className="btn-ghost"
        onClick={() => receivedOpen(r.id).catch((e) => setError(errorMessage(e)))}
      >
        Ver documento
      </button>
      <ConfirmAction
        label="Descartar"
        question={`¿Descartar «${r.filename}»? Sólo si no es una factura tuya o está repetida: no se contabiliza ni se deduce su IVA. Podrás recuperarla en «Descartadas».`}
        confirmLabel="Sí, descartar"
        className="btn-ghost text-red-700"
        onConfirm={async () => {
          await receivedDiscard(r.id);
          onDone();
        }}
      />
    </>
  );
}

/** «Base 100,00 €» con el antes si cambia: «Base 100,00 € → 120,00 €». */
function Row({
  label,
  before,
  after,
}: {
  label: string;
  before?: string | undefined;
  after: string;
}) {
  const changed = before !== undefined && before !== after;
  return (
    <>
      <dt className="text-stone-500">{label}</dt>
      <dd className={changed ? 'font-medium' : undefined}>
        {changed ? (
          <>
            <s className="text-stone-500">{before}</s> → {after}
          </>
        ) : (
          after
        )}
      </dd>
    </>
  );
}

const REGIMES: Record<ReceivedResumen['regimen'], string> = {
  interior: 'con IVA en la factura',
  intracomunitaria: 'adquisición intracomunitaria (se autorrepercute)',
  isp: 'inversión del sujeto pasivo (se autorrepercute)',
};

/** Resumen para confirmar la revisión o la corrección, con los avisos. */
function ConfirmPanel({
  check,
  busy,
  onConfirm,
  onBack,
}: {
  check: ReceivedCheck;
  busy: boolean;
  onConfirm: () => void;
  onBack: () => void;
}) {
  const confirmRef = useRef<HTMLButtonElement>(null);
  useEffect(() => confirmRef.current?.focus(), []);
  const a = check.despues;
  const b = check.antes ?? undefined;
  const total = (x: ReceivedResumen) =>
    `${euros(x.total)}${x.moneda && x.total_moneda != null ? ` (${money(x.total_moneda, x.moneda)})` : ''}`;
  return (
    <div
      role="alertdialog"
      aria-label={check.correccion ? 'Confirmar la corrección' : 'Confirmar los datos'}
      className="space-y-3 rounded-md border border-amber-300 bg-amber-50 p-3 text-sm text-stone-900"
      onKeyDown={(ev) => {
        if (ev.key === 'Escape' && !busy) {
          ev.preventDefault();
          ev.stopPropagation();
          onBack();
        }
      }}
    >
      <p className="font-medium">
        {check.correccion
          ? '¿Corregir la factura con estos datos? Se rehace su asiento y queda en la auditoría con el antes y el después.'
          : '¿Confirmar estos datos? La factura se contabiliza y su IVA entra en el 303.'}
      </p>
      <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-0.5">
        <Row
          label="Proveedor"
          before={b && `${b.proveedor} (${b.proveedor_nif})`}
          after={`${a.proveedor} (${a.proveedor_nif})`}
        />
        <Row label="Número" before={b?.numero} after={a.numero} />
        <Row label="Fecha" before={b && formatDate(b.fecha)} after={formatDate(a.fecha)} />
        <Row label="Base" before={b && euros(b.base)} after={euros(a.base)} />
        <Row
          label="IVA"
          before={b && `${euros(b.iva)} · ${b.deducible_pct} % deducible`}
          after={`${euros(a.iva)} · ${a.deducible_pct} % deducible`}
        />
        <Row label="Retención" before={b && euros(b.retencion)} after={euros(a.retencion)} />
        <Row label="Total" before={b && total(b)} after={total(a)} />
        <Row label="Régimen" before={b && REGIMES[b.regimen]} after={REGIMES[a.regimen]} />
        <Row label="Va al 303 del" after={check.trimestre} />
      </dl>
      {check.correccion && (
        <p className="text-xs text-stone-600">
          Corrige lo que se tecleó mal. Si es la factura del proveedor la que está mal, pídele una
          rectificativa (art. 15 del Reglamento de facturación) y regístrala como otra recibida.
        </p>
      )}
      {check.avisos.length > 0 && (
        <ul className="space-y-1 text-amber-900">
          {check.avisos.map((w) => (
            <li key={w}>⚠ {w}</li>
          ))}
        </ul>
      )}
      <div className="flex gap-2">
        <button
          ref={confirmRef}
          type="button"
          className="rounded-md btn-primary px-3 py-1"
          disabled={busy}
          onClick={onConfirm}
        >
          {busy ? 'Guardando…' : check.correccion ? 'Sí, corregir' : 'Sí, confirmar'}
        </button>
        <button type="button" className="btn-ghost" disabled={busy} onClick={onBack}>
          Volver a los datos
        </button>
      </div>
    </div>
  );
}

/** Equivalente en euros de lo escrito en la moneda (lo que se contabiliza y declara). */
function EurPreview({
  code,
  v,
  fx,
  vatEur,
}: {
  code: string;
  v: { base: string; vat: string; withholding: string; total: string };
  fx: RateValue;
  /** IVA en euros de la factura, si se ha escrito. */
  vatEur: number | null;
}) {
  const micros = parseRate(fx.rate);
  const a = [v.base, v.vat, v.withholding, v.total].map(parseEuros);
  if (micros === null || a.some((x) => x === null)) return null;
  const [base = 0, vat = 0, withholding = 0, total = 0] = a as number[];
  const eur = receivedInEuros({ base, vat, withholding, total }, micros, vatEur);
  return (
    <p className="span-full text-sm text-stone-700" aria-live="polite">
      {eur
        ? `En euros (lo que se contabiliza): base ${euros(eur.base)}, IVA ${euros(eur.vat)}${eur.withholding ? `, retención ${euros(eur.withholding)}` : ''}, total ${euros(eur.total)}. Si el proveedor no aplica IVA, TaxPain lo autorrepercute sobre la base en euros.`
        : `Los importes en ${code} no cuadran: base + IVA − retención ≠ total.`}
    </p>
  );
}

/**
 * «Deshacer la revisión»: pregunta con lo que conviene saber (lo comprueba Rust) y la devuelve a
 * pendientes. Si no se puede (pagada, conciliada, ejercicio cerrado…), dice por qué.
 */
function UnreviewAction({ r, onDone }: { r: ReceivedSummary; onDone: (msg: string) => void }) {
  const [avisos, setAvisos] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const name = `${r.number ?? r.filename} de ${r.supplier_name ?? ''}`.trim();
  if (avisos === null) {
    return (
      <>
        <button
          type="button"
          className="btn-ghost text-red-700"
          aria-label={`Deshacer la revisión de la factura ${name}`}
          onClick={async () => {
            setError(null);
            try {
              setAvisos(await receivedUnreviewCheck(r.id));
            } catch (e) {
              setError(errorMessage(e));
            }
          }}
        >
          Deshacer la revisión
        </button>
        {error && (
          <span role="alert" className="basis-full text-red-700">
            No se puede deshacer: {error}
          </span>
        )}
      </>
    );
  }
  return (
    <div
      role="alertdialog"
      aria-label="Deshacer la revisión"
      className="basis-full space-y-2 rounded-md border border-red-300 bg-red-50 p-3 text-stone-900"
    >
      <p className="font-medium">¿Deshacer la revisión de la factura {name}?</p>
      <ul className="space-y-1">
        {avisos.map((w) => (
          <li key={w}>⚠ {w}</li>
        ))}
      </ul>
      <div className="flex gap-2">
        <button
          type="button"
          className="rounded-md bg-red-700 px-3 py-1 text-white"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              await receivedUnreview(r.id);
              setAvisos(null);
              onDone(
                `La factura ${name} vuelve a «Pendientes de revisar»; su asiento se ha borrado.`,
              );
            } catch (e) {
              setError(errorMessage(e));
              setAvisos(null);
            } finally {
              setBusy(false);
            }
          }}
        >
          Sí, deshacer
        </button>
        <button type="button" className="btn-ghost" disabled={busy} onClick={() => setAvisos(null)}>
          Cancelar
        </button>
      </div>
    </div>
  );
}

/** Texto del periodo de un modelo que se puede rectificar: «303 del 3T de 2026». */
export function rectificableLabel(x: Rectificable): string {
  if (x.modelo === '200') return `200 de ${x.ejercicio}`;
  const p = x.modelo === '202' ? `${x.trimestre}P` : `${x.trimestre}T`;
  return `${x.modelo} del ${p} de ${x.ejercicio}`;
}

export function Received({
  onRectificar,
}: {
  /** Abre Modelos con la rectificativa de ese periodo preparada. */
  onRectificar?: (x: Rectificable) => void;
} = {}) {
  const [list, setList] = useState<ReceivedSummary[]>([]);
  /** Modelos presentados que tocó la última corrección (para preparar su rectificativa). */
  const [rectificar, setRectificar] = useState<Rectificable[]>([]);
  const [open, setOpen] = useState<number | null>(null);
  /** Revisada que se está corrigiendo. */
  const [correcting, setCorrecting] = useState<number | null>(null);
  const [folders, setFolders] = useState<string[]>([]);
  const [message, setMessage] = useState<string | null>(null);
  const [filling, setFilling] = useState(false);

  const load = useCallback(() => {
    receivedList(null)
      .then(setList)
      .catch((e) => setMessage(errorMessage(e)));
  }, []);

  useEffect(() => {
    load();
    watchedFolders()
      .then(setFolders)
      .catch(() => setFolders([]));
  }, [load]);

  const importPaths = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0) return;
      try {
        const { outcomes: out, errors } = await receivedImport(paths);
        const dups = out.filter((o) => o.duplicate).length;
        setMessage(
          `${plural(out.length - dups, 'importada', 'importadas')}${
            dups ? `, ${dups} ${dups === 1 ? 'ya estaba' : 'ya estaban'}` : ''
          }.${errors.length ? ` No se ha podido importar: ${errors.join('; ')}.` : ''}`,
        );
        load();
      } catch (e) {
        setMessage(errorMessage(e));
        load();
      }
    },
    [load],
  );

  // Arrastrar PDF, Facturae o fotos sobre la ventana los importa (quedan pendientes de revisar).
  const [dragging, setDragging] = useState(false);
  useEffect(() => {
    let stop: (() => void) | null = null;
    let cancelled = false;
    void onFileDrop((e) => {
      if (e.type === 'over') setDragging(true);
      else setDragging(false);
      if (e.type === 'drop') {
        const files = e.paths.filter((p) =>
          /\.(pdf|xml|xsig|jpe?g|png|heic|heif|tiff?|webp)$/i.test(p),
        );
        if (files.length < e.paths.length)
          setMessage('Sólo se importan PDF, Facturae (.xml, .xsig) y fotos (JPG, PNG, HEIC).');
        void importPaths(files);
      }
    }).then((f) => {
      if (cancelled) f();
      else stop = f;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [importPaths]);

  const pending = list.filter((r) => r.status === 'pendiente');
  const reviewed = list.filter((r) => r.status === 'revisada');
  const discarded = list.filter((r) => r.status === 'descartada');

  return (
    <section
      className={`mt-6 rounded-lg ${dragging ? 'outline-2 outline-emerald-600 outline-dashed' : ''}`}
    >
      {dragging && (
        <p role="status" className="mb-2 text-sm text-emerald-800">
          Suelta los PDF o Facturae para importarlos.
        </p>
      )}
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold">Facturas recibidas</h2>
        <div className="flex gap-3 text-sm">
          <button
            type="button"
            className="text-link"
            onClick={async () => {
              await importPaths(await pickReceivedFiles());
            }}
          >
            Importar PDF / Facturae
          </button>
          <button
            type="button"
            onClick={async () => {
              const p = await pickFolder();
              if (!p) return;
              try {
                setFolders(await setWatchedFolders([...folders, p]));
              } catch (e) {
                setMessage(errorMessage(e));
              }
            }}
          >
            {folders.length ? 'Vigilar otra carpeta' : 'Vigilar una carpeta'}
          </button>
        </div>
      </div>
      <p className="text-xs text-stone-500">
        También puedes arrastrar los PDF, Facturae o fotos de tickets a esta ventana: el texto de
        las fotos y los escaneados lo lee el Mac.
      </p>
      {folders.length > 0 && (
        <div className="mt-1 text-xs text-stone-500">
          <p>
            Carpetas vigiladas, con sus subcarpetas: las facturas recibidas nuevas entran solas
            aquí; los extractos, modelos y demás documentos se avisan en Importar.
          </p>
          <ul className="mt-1 space-y-0.5">
            {folders.map((f) => (
              <li key={f} className="flex items-center gap-2">
                <span className="break-all">{f}</span>
                <button
                  type="button"
                  className="underline"
                  aria-label={`Dejar de vigilar ${f}`}
                  onClick={async () => {
                    try {
                      setFolders(await setWatchedFolders(folders.filter((x) => x !== f)));
                    } catch (e) {
                      setMessage(errorMessage(e));
                    }
                  }}
                >
                  Quitar
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
      {message && (
        <p role="status" className="mt-2 text-sm">
          {message}
        </p>
      )}
      {onRectificar && rectificar.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-2 text-sm">
          {rectificar.map((x) => (
            <button
              key={`${x.modelo}-${x.ejercicio}-${x.trimestre}`}
              type="button"
              className="rounded border border-amber-400 px-3 py-1"
              onClick={() => onRectificar(x)}
            >
              Preparar la rectificativa del {rectificableLabel(x)}
            </button>
          ))}
        </div>
      )}

      <div className="mt-4 flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-stone-500">
          Pendientes de revisar ({pending.length})
        </h3>
        {pending.some((r) => r.total === null || r.supplier_name === null) && (
          <button
            type="button"
            className="flex items-center gap-1.5 text-sm"
            disabled={filling}
            onClick={async () => {
              setFilling(true);
              try {
                const r = await receivedAiFillPending(50);
                setMessage(
                  `El asistente ha completado ${plural(r.filled, 'factura')}${
                    r.failed.length > 0 ? `; ${r.failed.length} no: ${r.failed[0]}` : ''
                  }. Revísalas antes de confirmarlas.`,
                );
                load();
              } catch (e) {
                setMessage(errorMessage(e));
              } finally {
                setFilling(false);
              }
            }}
          >
            <Sparkles size={15} aria-hidden="true" />
            {filling ? 'Completando…' : 'Completar con el asistente'}
          </button>
        )}
      </div>
      <ul className="divide-y divide-stone-200">
        {pending.map((r) => (
          <li key={r.id} className="py-2 text-sm">
            <button
              type="button"
              className="text-left"
              onClick={() => setOpen(open === r.id ? null : r.id)}
            >
              <span className="font-medium">{r.supplier_name ?? r.filename}</span>
              {r.total !== null && <span className="ml-2">{euros(r.total)}</span>}
              {r.currency && r.currency_total != null && (
                <span className="ml-1 text-xs text-stone-500">
                  ({money(r.currency_total, r.currency)})
                </span>
              )}
              {r.extraction.warnings.length > 0 && <span className="ml-2 text-amber-700">⚠</span>}
            </button>
            {open === r.id && (
              <ReviewForm
                // Si la extracción cambia (la completa el asistente), el formulario se rehace con
                // ella: confirmar no debe pisar lo nuevo con lo que había al abrirlo.
                key={JSON.stringify(r.extraction)}
                r={r}
                onDone={() => {
                  setOpen(null);
                  load();
                }}
              />
            )}
          </li>
        ))}
      </ul>

      <h3 className="mt-4 text-sm font-semibold uppercase tracking-wide text-stone-500">
        Revisadas ({reviewed.length})
      </h3>
      <ul className="divide-y divide-stone-200">
        {reviewed.map((r) => (
          <li key={r.id} className="py-2 text-sm">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span>
                {r.supplier_name} · {r.number} · {r.issue_date ? formatDate(r.issue_date) : ''}
              </span>
              <span className="flex flex-wrap items-center gap-3">
                <span>
                  {r.total !== null ? euros(r.total) : ''}
                  {r.currency && r.currency_total != null && (
                    <span className="ml-1 text-xs text-stone-500">
                      ({money(r.currency_total, r.currency)})
                    </span>
                  )}
                </span>
                {r.reviewed && correcting !== r.id && (
                  <button
                    type="button"
                    className="text-link"
                    aria-label={`Corregir la factura ${r.number ?? ''} de ${r.supplier_name ?? ''}`}
                    onClick={() => setCorrecting(r.id)}
                  >
                    Corregir…
                  </button>
                )}
                {correcting !== r.id && (
                  <UnreviewAction
                    r={r}
                    onDone={(msg) => {
                      setMessage(msg);
                      load();
                    }}
                  />
                )}
              </span>
            </div>
            {correcting === r.id && (
              <ReviewForm
                key={`corregir-${r.id}`}
                r={r}
                correct
                onCancel={() => setCorrecting(null)}
                onDone={(msg, links) => {
                  setCorrecting(null);
                  if (msg) setMessage(msg);
                  setRectificar(links ?? []);
                  load();
                }}
              />
            )}
          </li>
        ))}
      </ul>

      {discarded.length > 0 && (
        <details className="mt-4">
          <summary className="cursor-pointer text-sm font-semibold uppercase tracking-wide text-stone-500">
            Descartadas ({discarded.length})
          </summary>
          <ul className="divide-y divide-stone-200">
            {discarded.map((r) => (
              <li key={r.id} className="flex items-center justify-between gap-2 py-2 text-sm">
                <span>
                  {r.supplier_name ?? r.filename}
                  {r.total !== null && <span className="ml-2">{euros(r.total)}</span>}
                </span>
                <button
                  type="button"
                  className="text-link"
                  onClick={async () => {
                    try {
                      await receivedUndiscard(r.id);
                      setMessage(`«${r.supplier_name ?? r.filename}» vuelve a pendientes.`);
                      load();
                    } catch (e) {
                      setMessage(errorMessage(e));
                    }
                  }}
                >
                  Recuperar
                </button>
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}

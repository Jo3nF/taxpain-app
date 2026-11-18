import { ChevronDown, FileText, Plus, X } from 'lucide-react';
import {
  type Dispatch,
  type SetStateAction,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from 'react';
import {
  COUNTRIES,
  type ContactSuggestion,
  contactsSearch,
  dashboardGet,
  EU_COUNTRIES,
  errorMessage,
  euForServices,
  euros,
  everyLabel,
  type ForeignCurrency,
  formatDate,
  hayFacturacion,
  type InvoiceDraft,
  type InvoiceFormat,
  type InvoiceLine,
  type InvoiceTemplate,
  type InvoicingSettings,
  type IssuedSummary,
  invoiceCancel,
  invoiceDraftOf,
  invoiceIssue,
  invoiceLanguageOf,
  invoiceOpen,
  invoicePreview,
  invoiceSave,
  invoiceSignFacturae,
  invoicesList,
  kindLabel,
  listCertificates,
  money,
  parseEuros,
  parseQuantity,
  parseRate,
  plural,
  type Quote,
  quoteCreate,
  quoteIssueInvoice,
  quoteUpdate,
  type RecordIssue,
  rateText,
  recurringCreate,
  recurringList,
  sendStateLabel,
  type Totals,
  toEur,
  type VatTreatment,
  verifactuIssues,
  verifactuResubmit,
  verifactuSend,
  viesCheck,
} from './api';
import { CertificateSelect } from './CertificateSelect';
import { ConfirmAction } from './ConfirmAction';
import { ProvinceDatalist, YearStepper } from './Controls';
import {
  CurrencyRate,
  CurrencySelect,
  emptyRate,
  type RateValue,
  rateValueOf,
  toForeignCurrency,
} from './CurrencyRate';
import { InvoiceEmailPanel } from './Email';
import { Menu } from './Menu';
import { type FormOss, OssFields, parsePct, pctText } from './OssFactura';
import { provinceAfterPostCode } from './provincias';
import { NumberingCheck, type RectChoice, RectificacionFields } from './Rectificacion';
import { Recurring } from './Recurring';
import { VerifactuCola } from './VerifactuCola';

const input = 'mt-1 block w-full rounded-md border border-stone-300 px-2 py-1';

const VAT_OPTIONS: { label: string; value: VatTreatment }[] = [
  { label: '21 %', value: { Taxed: { rate: 2100 } } },
  { label: '10 %', value: { Taxed: { rate: 1000 } } },
  { label: '4 %', value: { Taxed: { rate: 400 } } },
  { label: 'Exenta (art. 20)', value: { Exempt: 'E1' } },
  { label: 'Servicio UE (ISP)', value: { NotSubject: 'N2' } },
  { label: 'Servicio fuera UE', value: { NotSubject: 'N2' } },
  { label: 'Bienes UE (art. 25)', value: { Exempt: 'E5' } },
  { label: 'Exportación (art. 21)', value: { Exempt: 'E2' } },
  { label: 'Inversión sujeto pasivo', value: 'ReverseCharge' },
];
/** Opciones de IVA propuestas para un cliente de la UE y de fuera. */
const VAT_EU_SERVICE = 4;
const VAT_NON_EU_SERVICE = 5;
const VAT_EU_GOODS = 6;
const VAT_EXPORT = 7;
const VAT_REVERSE = 8;
const isTaxedOption = (i: number) => i <= 2;

interface LineForm {
  description: string;
  quantity: string;
  price: string;
  vat: number;
  withholding: boolean;
  /** Anticipo: precio positivo al facturarlo; negativo al descontarlo en la factura final. */
  advance?: boolean;
}

interface Form {
  clientName: string;
  /** NIF (España), NIF-IVA (UE) o identificador fiscal del país del cliente. */
  clientNif: string;
  /** País del cliente (ISO); España si no se indica. */
  country?: string;
  /** Cliente de otro país de la UE que es un particular (sin NIF-IVA): lleva IVA español. */
  consumer?: boolean;
  /** Particular de la UE con el IVA de su país (ventanilla única, OSS). */
  oss?: FormOss;
  /** Idioma del PDF: vacío = el de los ajustes. */
  language?: '' | 'Es' | 'En';
  /** Fecha de la operación si es distinta de la de expedición. */
  operationDate?: string;
  street: string;
  postCode: string;
  town: string;
  province: string;
  date: string;
  description: string;
  lines: LineForm[];
  /** Moneda de la factura: vacío o «EUR» = euros. */
  currency?: string | undefined;
  /** Tipo de cambio si no es en euros. */
  fx?: RateValue | undefined;
}

/** Datos del cliente de una factura emitida (para su rectificativa): nombre, NIF, país y
 * dirección, como los tenía. */
export function clientOf(
  d: InvoiceDraft,
): Pick<
  Form,
  | 'clientName'
  | 'clientNif'
  | 'country'
  | 'consumer'
  | 'oss'
  | 'street'
  | 'postCode'
  | 'town'
  | 'province'
> {
  const {
    clientName,
    clientNif,
    country = 'ES',
    consumer,
    oss,
    street,
    postCode,
    town,
    province,
  } = fromDraft(d, 0);
  return {
    clientName,
    clientNif,
    country,
    ...(consumer ? { consumer } : {}),
    ...(oss ? { oss } : {}),
    street,
    postCode,
    town,
    province,
  };
}

/** Por qué la casilla de retención está desactivada (la retención por defecto es «Sin
 * retención»); `undefined` si se puede marcar. */
const NO_WITHHOLDING_HINT = (s: InvoicingSettings) =>
  s.default_withholding === null
    ? 'Para aplicar retención de IRPF, elige el tipo en Ajustes → Facturación (ahora «Sin retención»).'
    : undefined;

/** Moneda extranjera del formulario (o null si es en euros). */
export function foreignCode(f: Form): string | null {
  return f.currency && f.currency !== 'EUR' ? f.currency : null;
}

/** Total aproximado de las líneas (para mostrar el equivalente en euros mientras se escribe). */
function approxTotal(lines: LineForm[], s: InvoicingSettings): number | null {
  let total = 0;
  for (const l of lines) {
    const price = parseEuros(l.price);
    const qty = parseQuantity(l.quantity);
    if (price === null || qty === null) return null;
    const base = Math.round((qty * price) / 1000);
    const v = VAT_OPTIONS[l.vat]?.value;
    const rate = v && typeof v === 'object' && 'Taxed' in v ? v.Taxed.rate : 0;
    const wh =
      l.withholding && s.default_withholding !== null
        ? Math.round((base * s.default_withholding) / 10000)
        : 0;
    total += base + Math.round((base * rate) / 10000) - wh;
  }
  return total;
}

/** Fecha local (no UTC: entre las 00:00 y las 02:00 en España sería el día anterior). */
export function today(now: Date = new Date()): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}`;
}

/** Convierte el formulario en un borrador para el proceso nativo (o devuelve el error). */
export function toDraft(
  f: Form,
  s: InvoicingSettings,
  rectifies?: IssuedSummary | undefined,
  /** Presupuesto o proforma: basta el nombre del cliente (el NIF se pone al facturar). */
  quote = false,
  /** Factura de un presupuesto: lleva los datos del cliente con su NIF. */
  fromQuote = false,
  /** Rectificativa: motivo, tipo y otras facturas que agrupa (por defecto R1 o R5, diferencias). */
  rect?: RectChoice,
): InvoiceDraft | string {
  const lines: InvoiceLine[] = [];
  for (const [i, l] of f.lines.entries()) {
    const price = parseEuros(l.price);
    // Con `Number()`, «1.000» (mil, como en el precio) sería una unidad.
    const qty = parseQuantity(l.quantity);
    if (!l.description.trim() || price === null || qty === null || qty === 0) {
      return `Revisa la línea ${i + 1}`;
    }
    const vat = VAT_OPTIONS[l.vat]?.value ?? { Taxed: { rate: s.default_vat } };
    lines.push({
      description: l.description.trim(),
      quantity_milli: qty,
      unit_price: price,
      discount: 0,
      vat,
      surcharge: null,
      withholding: l.withholding ? s.default_withholding : null,
      ...(l.advance ? { advance: true } : {}),
    });
  }
  const country = f.country || 'ES';
  const foreign = country !== 'ES';
  const eu = EU_COUNTRIES.includes(country);
  const consumer = eu && f.consumer === true;
  // Ventanilla única: las líneas no llevan IVA español (N2) y se suma el IVA del país.
  let oss: InvoiceDraft['oss'] = null;
  if (consumer && f.oss) {
    const rate = parsePct(f.oss.rate);
    if (rate === null) return 'Pon el tipo de IVA del país del cliente (p. ej. «20»)';
    oss = { rate, supply: f.oss.supply };
    for (const l of lines) {
      l.vat = { NotSubject: 'N2' };
      l.withholding = null;
    }
  }
  const taxId = f.clientNif.trim().toUpperCase();
  const name = f.clientName.trim();
  const hasClient = taxId !== '' || (quote && name !== '');
  if (quote && !name) return 'Pon el nombre del cliente';
  if (fromQuote && !taxId) {
    return 'Pon el NIF del cliente: la factura de un presupuesto lleva sus datos (sin NIF sería una factura simplificada)';
  }
  if (rectifies && !hasClient && rectifies.kind !== 'F2') {
    return 'La rectificativa necesita los datos del cliente';
  }
  // Validación VERI*FACTU 3.1.3.13: en una R5 (rectificativa de simplificada) la AEAT rechaza el
  // bloque Destinatarios, y una factura emitida ya no se puede corregir.
  if (rectifies?.kind === 'F2' && (taxId || name)) {
    return 'La rectificativa de una simplificada (R5) no lleva los datos del cliente: déjalos vacíos. Si hay que identificarlo, emite una factura completa nueva.';
  }
  if (foreign && !hasClient && !quote && !consumer) {
    return eu
      ? 'Pon el NIF-IVA del cliente (sin él, la operación lleva IVA español: si es un particular, márcalo)'
      : 'Pon el identificador fiscal del cliente';
  }
  // Con un nombre y sin NIF no sale una factura completa: sería una simplificada sin el cliente.
  if (!quote && !taxId && name) {
    return consumer
      ? 'Pon el documento de identidad del cliente o borra su nombre: sin él, la factura es simplificada y no lleva sus datos'
      : 'Pon el NIF del cliente o borra su nombre: sin NIF, la factura es simplificada y no lleva sus datos';
  }
  if (hasClient && !name) return 'Pon el nombre o la razón social del cliente';
  const code = foreignCode(f);
  let currency: ForeignCurrency | null = null;
  if (code) {
    const c = toForeignCurrency(code, f.fx ?? emptyRate());
    if (typeof c === 'string') return c;
    currency = c;
  }
  const kind: InvoiceDraft['kind'] = rectifies
    ? {
        Rectificativa: {
          code: rectifies.kind === 'F2' ? 5 : (rect?.code ?? 1),
          method: rect?.method ?? 'Diferencias',
        },
      }
    : hasClient
      ? 'Completa'
      : 'Simplificada';
  return {
    kind,
    series: null,
    issue_date: f.date,
    operation_date: f.operationDate && f.operationDate !== f.date ? f.operationDate : null,
    recipient: hasClient
      ? {
          id: foreign
            ? {
                Foreign: {
                  country,
                  // Un particular de la UE no tiene NIF-IVA: su documento de identidad.
                  id_type: eu && !consumer ? 'NifIva' : 'DocumentoOficial',
                  id: taxId.replace(/[\s.-]/g, ''),
                },
              }
            : { Nif: taxId },
          name: f.clientName.trim(),
          address: f.street.trim()
            ? {
                street: f.street.trim(),
                post_code: f.postCode.trim(),
                town: f.town.trim(),
                province: f.province.trim(),
                country,
              }
            : null,
          person: null,
        }
      : null,
    lines,
    rectified: rectifies
      ? [rectifies, ...(rect?.also ?? [])].map((i): [string, string] => [i.number, i.issue_date])
      : [],
    rectified_amounts: null,
    description: f.description.trim() || (lines[0]?.description ?? ''),
    notes: null,
    ...(currency ? { currency } : {}),
    ...(oss ? { oss } : {}),
  };
}

/** Formulario a partir de un borrador (para «Duplicar» una factura, editar un presupuesto…).
 * `keepDate`: conserva la fecha del borrador (al editar un presupuesto); si no, la de hoy. */
export function fromDraft(d: InvoiceDraft, defaultVat: number, keepDate = false): Form {
  const r = d.recipient;
  const country = r && 'Foreign' in r.id ? r.id.Foreign.country : 'ES';
  const consumer =
    !!r && 'Foreign' in r.id && r.id.Foreign.id_type !== 'NifIva' && EU_COUNTRIES.includes(country);
  const vatIndex = (v: VatTreatment) => {
    if (typeof v === 'object' && 'NotSubject' in v && v.NotSubject === 'N2') {
      return euForServices(country) && !consumer ? VAT_EU_SERVICE : VAT_NON_EU_SERVICE;
    }
    const i = VAT_OPTIONS.findIndex((o) => JSON.stringify(o.value) === JSON.stringify(v));
    return i < 0 ? defaultVat : i;
  };
  return {
    clientName: r?.name ?? '',
    clientNif: r ? ('Nif' in r.id ? r.id.Nif : r.id.Foreign.id) : '',
    country,
    ...(consumer ? { consumer: true } : {}),
    ...(consumer && d.oss
      ? { oss: { rate: pctText(d.oss.rate).replace(' %', ''), supply: d.oss.supply } }
      : {}),
    language: '',
    operationDate: '',
    street: r?.address?.street ?? '',
    postCode: r?.address?.post_code ?? '',
    town: r?.address?.town ?? '',
    province: r?.address?.province ?? '',
    date: keepDate ? d.issue_date : today(),
    description: d.description,
    lines: d.lines.map((l) => ({
      description: l.description,
      quantity: String(l.quantity_milli / 1000).replace('.', ','),
      price: (l.unit_price / 100).toFixed(2).replace('.', ','),
      vat: vatIndex(l.vat),
      withholding: l.withholding !== null,
      ...(l.advance ? { advance: true } : {}),
    })),
    ...(d.currency ? { currency: d.currency.code, fx: rateValueOf(d.currency) } : {}),
  };
}

/** Opción de IVA de una línea que encaja con el cliente: en España o a un particular de la UE,
 * IVA español (sin entregas UE, exportaciones ni servicios no sujetos; la inversión del sujeto
 * pasivo sólo en España); a una empresa de la UE, el servicio sin IVA y los bienes por el art.
 * 25; fuera de la UE, el servicio sin IVA y los bienes como exportación. */
export function fitVat(
  vat: number,
  country: string,
  consumer: boolean,
  defaultVat: number,
): number {
  const foreign = country !== 'ES';
  const eu = EU_COUNTRIES.includes(country);
  if (!foreign || (eu && consumer)) {
    const notHere = [VAT_EU_SERVICE, VAT_NON_EU_SERVICE, VAT_EU_GOODS, VAT_EXPORT];
    if (notHere.includes(vat) || (foreign && vat === VAT_REVERSE)) return defaultVat;
    return vat;
  }
  const service = euForServices(country) ? VAT_EU_SERVICE : VAT_NON_EU_SERVICE;
  if (
    isTaxedOption(vat) ||
    vat === VAT_EU_SERVICE ||
    vat === VAT_NON_EU_SERVICE ||
    vat === VAT_REVERSE
  ) {
    // A una empresa de fuera, un servicio no lleva IVA español.
    return service;
  }
  if (eu && vat === VAT_EXPORT) return VAT_EU_GOODS;
  if (!eu && vat === VAT_EU_GOODS) return VAT_EXPORT;
  return vat;
}

/** Cambia el país del cliente (y si es un particular de la UE) y adapta lo que depende de ello
 * (IVA de las líneas, retención, idioma de la factura). */
export function withClient(f: Form, country: string, consumer: boolean, defaultVat: number): Form {
  const wasForeign = (f.country || 'ES') !== 'ES';
  const foreign = country !== 'ES';
  const isConsumer = consumer && EU_COUNTRIES.includes(country);
  const lines = f.lines.map((l) => {
    const vat = fitVat(l.vat, country, isConsumer, defaultVat);
    // Sin retención de IRPF a un cliente de fuera.
    const withholding = foreign ? false : l.withholding;
    return vat === l.vat && withholding === l.withholding ? l : { ...l, vat, withholding };
  });
  const language =
    foreign && !wasForeign && !f.language ? 'En' : !foreign && wasForeign ? '' : f.language;
  // El IVA del país del cliente (OSS) sólo con un particular de la UE del mismo país.
  const { oss, ...rest } = f;
  const keepOss = isConsumer && oss && country === (f.country || 'ES');
  return {
    ...rest,
    ...(keepOss ? { oss } : {}),
    country,
    consumer: isConsumer,
    lines,
    language: language ?? '',
  };
}

const withCountry = (f: Form, country: string, defaultVat: number) =>
  withClient(f, country, f.consumer ?? false, defaultVat);

function ClientFields({
  f,
  setF,
  defaultVat,
  hasWithholding,
  quote = false,
  rectifying = false,
}: {
  f: Form;
  setF: Dispatch<SetStateAction<Form>>;
  defaultVat: number;
  hasWithholding: boolean;
  /** Presupuesto: el NIF es opcional y no hay fecha de operación. */
  quote?: boolean;
  /** Rectificativa: parte del tipo de la factura que rectifica (no se avisa de que es antiguo). */
  rectifying?: boolean;
}) {
  const provinces = useId();
  const [suggestions, setSuggestions] = useState<ContactSuggestion[]>([]);
  const [vies, setVies] = useState<{ busy: boolean; text: string | null; ok: boolean }>({
    busy: false,
    text: null,
    ok: false,
  });
  const [query, setQuery] = useState('');
  const country = f.country || 'ES';
  const eu = EU_COUNTRIES.includes(country);
  const consumer = eu && f.consumer === true;
  const code = foreignCode(f);

  // La comprobación de VIES es de un NIF-IVA y un país: si cambian (a mano o al elegir otro
  // cliente), deja de valer, y la respuesta de una consulta anterior se descarta.
  const viesRequest = useRef(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: se reinicia al cambiar el NIF o el país
  useEffect(() => {
    viesRequest.current += 1;
    setVies({ busy: false, text: null, ok: false });
  }, [country, f.clientNif]);

  async function checkVies() {
    const request = ++viesRequest.current;
    setVies({ busy: true, text: null, ok: false });
    try {
      const r = await viesCheck(country, f.clientNif, f.clientName.trim() || null, 'cliente');
      if (request !== viesRequest.current) return;
      setVies({
        busy: false,
        ok: r.valid,
        text: r.valid
          ? `Válido en VIES${r.name ? ` · ${r.name}` : ''}${r.request_id ? ` · consulta ${r.request_id}` : ''}. Guardado en la ficha.`
          : 'VIES dice que no es válido: sin un NIF-IVA válido, la operación lleva IVA español.',
      });
      // Sobre el formulario de ahora (la consulta tarda): sólo si sigue sin nombre.
      const name = r.name;
      if (r.valid && name) {
        setF((cur) => (cur.clientName.trim() ? cur : { ...cur, clientName: name }));
      }
    } catch (e) {
      if (request !== viesRequest.current) return;
      setVies({ busy: false, ok: false, text: `No se pudo consultar VIES: ${errorMessage(e)}` });
    }
  }

  // Clientes guardados que encajan con lo que se escribe (nombre o NIF).
  useEffect(() => {
    const q = query.trim();
    if (q.length < 2) {
      setSuggestions([]);
      return;
    }
    let cancelled = false;
    const t = setTimeout(() => {
      contactsSearch(q)
        .then((r) => {
          if (!cancelled) setSuggestions(r);
        })
        .catch(() => {
          if (!cancelled) setSuggestions([]);
        });
    }, 200);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [query]);

  function pick(sug: ContactSuggestion) {
    const p = sug.party;
    const c = 'Foreign' in p.id ? p.id.Foreign.country : 'ES';
    // Un cliente de la UE guardado sin NIF-IVA es un particular.
    const next = withClient(
      f,
      c,
      'Foreign' in p.id && p.id.Foreign.id_type !== 'NifIva',
      defaultVat,
    );
    setF({
      ...next,
      clientNif: 'Nif' in p.id ? p.id.Nif : p.id.Foreign.id,
      clientName: p.name,
      street: p.address?.street ?? '',
      postCode: p.address?.post_code ?? '',
      town: p.address?.town ?? '',
      province: p.address?.province ?? '',
      language: sug.language ?? next.language ?? '',
    });
    setQuery('');
    setSuggestions([]);
  }

  const idLabel =
    country === 'ES'
      ? quote
        ? 'NIF del cliente (opcional)'
        : 'NIF del cliente (vacío = simplificada)'
      : consumer
        ? quote
          ? 'Documento de identidad (opcional)'
          : 'Documento de identidad (vacío = simplificada)'
        : eu
          ? 'NIF-IVA del cliente'
          : 'Identificador fiscal';

  return (
    <div className="space-y-3">
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <label className="text-sm">
          País del cliente
          <select
            className={input}
            value={country}
            onChange={(e) => setF(withCountry(f, e.target.value, defaultVat))}
          >
            {COUNTRIES.map(([code, name]) => (
              <option key={code} value={code}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <label className="text-sm">
          {idLabel}
          <input
            className={input}
            value={f.clientNif}
            placeholder={eu && !consumer ? `${country === 'GR' ? 'EL' : country}…` : undefined}
            onChange={(e) => {
              setF({ ...f, clientNif: e.target.value });
              setQuery(e.target.value);
            }}
          />
        </label>
        <div className="relative col-span-2 text-sm">
          <label className="block">
            Nombre o razón social
            <input
              className={input}
              value={f.clientName}
              autoComplete="off"
              onChange={(e) => {
                setF({ ...f, clientName: e.target.value });
                setQuery(e.target.value);
              }}
            />
          </label>
          {suggestions.length > 0 && (
            <ul
              aria-label="Clientes guardados"
              className="absolute z-10 mt-1 w-full overflow-hidden rounded-md border border-stone-200 bg-white shadow-lg"
            >
              {suggestions.map((sug) => (
                <li key={sug.key}>
                  <button
                    type="button"
                    className="flex w-full items-center justify-between gap-3 px-3 py-2 text-left hover:bg-stone-100"
                    onClick={() => pick(sug)}
                  >
                    <span className="truncate font-medium">{sug.party.name}</span>
                    <span className="shrink-0 text-xs text-stone-500">{sug.key}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
        <label className="col-span-2 text-sm">
          Dirección
          <input
            className={input}
            value={f.street}
            onChange={(e) => setF({ ...f, street: e.target.value })}
          />
        </label>
        <label className="text-sm">
          {country === 'ES' ? 'CP' : 'Código postal'}
          <input
            className={input}
            value={f.postCode}
            autoComplete="postal-code"
            onChange={(e) =>
              setF({
                ...f,
                postCode: e.target.value,
                ...(country === 'ES'
                  ? { province: provinceAfterPostCode(f.postCode, e.target.value, f.province) }
                  : {}),
              })
            }
          />
        </label>
        <label className="text-sm">
          Población
          <input
            className={input}
            value={f.town}
            onChange={(e) => setF({ ...f, town: e.target.value })}
          />
        </label>
        <label className="text-sm">
          {country === 'ES' ? 'Provincia' : 'Región'}
          <input
            className={input}
            value={f.province}
            list={country === 'ES' ? provinces : undefined}
            onChange={(e) => setF({ ...f, province: e.target.value })}
          />
          {country === 'ES' && <ProvinceDatalist id={provinces} />}
        </label>
        <label className="text-sm">
          Fecha
          <input
            type="date"
            className={input}
            value={f.date}
            max={quote ? undefined : today()}
            onChange={(e) => setF({ ...f, date: e.target.value })}
          />
        </label>
        {!quote && (
          <label className="text-sm">
            Fecha de la operación
            <input
              type="date"
              className={input}
              value={f.operationDate ?? ''}
              max={f.date}
              onChange={(e) => setF({ ...f, operationDate: e.target.value })}
            />
          </label>
        )}
        <label className="text-sm">
          Idioma de la factura
          <select
            className={input}
            value={f.language ?? ''}
            onChange={(e) => setF({ ...f, language: e.target.value as '' | 'Es' | 'En' })}
          >
            <option value="">Como en los ajustes</option>
            <option value="Es">Español</option>
            <option value="En">Inglés</option>
          </select>
        </label>
        <CurrencySelect
          value={f.currency ?? 'EUR'}
          className={input}
          onChange={(c) =>
            setF({
              ...f,
              currency: c,
              // El tipo de otra moneda (y su «BCE») no vale para esta: se empieza de cero.
              fx:
                c === 'EUR'
                  ? undefined
                  : c === f.currency && f.fx
                    ? f.fx
                    : emptyRate(f.operationDate || f.date),
            })
          }
        />
        {code && (
          <CurrencyRate
            code={code}
            value={f.fx ?? emptyRate()}
            accrual={(!quote && f.operationDate) || f.date}
            onChange={(fx) => setF({ ...f, fx })}
            inputClass={input}
            warn={!quote}
            oldRateOk={rectifying}
          />
        )}
      </div>
      {code && (
        <p className="rounded-md bg-stone-50 p-3 text-sm text-stone-700">
          {quote
            ? `Importes en ${code}. El tipo es orientativo: al facturarlo se pone el del día de la operación.`
            : rectifying
              ? `Importes en ${code} al tipo de la factura que se rectifica: así una anulación total deja a cero los euros contabilizados y declarados. Si la corrección es de otra operación posterior, pon el tipo de ese día.`
              : `Precios e importes en ${code}. TaxPain contabiliza y declara en euros al tipo vigente el día de la operación (art. 79.Once de la Ley del IVA) y la factura muestra la base, el IVA y el total en euros con ese tipo (art. 12 del Reglamento de facturación).`}
        </p>
      )}
      {eu && (
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={consumer}
            onChange={(e) => setF(withClient(f, country, e.target.checked, defaultVat))}
          />
          Es un particular (no es empresa ni profesional): sin NIF-IVA y con IVA español
        </label>
      )}
      {consumer && !quote && (
        <OssFields
          country={country}
          year={Number(f.date.slice(0, 4)) || new Date().getFullYear()}
          oss={f.oss}
          onChange={(oss) => {
            if (oss) {
              setF({ ...f, oss });
            } else {
              const { oss: _quitado, ...rest } = f;
              setF(withClient(rest, country, true, defaultVat));
            }
          }}
        />
      )}
      {eu && !consumer && (
        <div className="flex flex-wrap items-center gap-3 text-sm">
          <button
            type="button"
            className="rounded-md border border-stone-300 px-3 py-1.5 disabled:opacity-50"
            disabled={vies.busy || f.clientNif.trim().length < 4}
            onClick={() => void checkVies()}
          >
            {vies.busy ? 'Consultando VIES…' : 'Comprobar el NIF-IVA en VIES'}
          </button>
          <span className="text-xs text-stone-500">
            Envía el NIF-IVA del cliente y el tuyo a la Comisión Europea.
          </span>
          {vies.text && (
            <p role="status" className={`w-full ${vies.ok ? 'text-emerald-700' : 'text-red-700'}`}>
              {vies.text}
            </p>
          )}
        </div>
      )}
      {country !== 'ES' && (
        <p className="rounded-md bg-stone-50 p-3 text-sm text-stone-700">
          {consumer
            ? 'Particular de la UE: en general la factura lleva IVA español (art. 69 de la Ley del IVA) y no va en el modelo 349. Excepción: las ventas de bienes a distancia y los servicios de telecomunicaciones, radiodifusión o electrónicos a particulares de la UE tributan en el país del cliente si pasan de 10.000 € al año en toda la UE (ventanilla única, OSS).'
            : country === 'XI'
              ? 'Irlanda del Norte: sólo la venta de bienes a una empresa con NIF-IVA «XI» es intracomunitaria (exenta, art. 25, y en el modelo 349: elige «Bienes UE»). Un servicio se trata como a Reino Unido, fuera de la UE.'
              : eu
                ? 'Cliente empresa de la UE: un servicio no lleva IVA español (lo declara el cliente, «inversión del sujeto pasivo») y va en el modelo 349. Comprueba antes su NIF-IVA en VIES. Si vendes bienes, elige «Bienes UE».'
                : 'Cliente de fuera de la UE: un servicio a una empresa no lleva IVA español; una venta de bienes es una exportación exenta (art. 21).'}
          {hasWithholding && ' Sin retención de IRPF: no se aplica a clientes de fuera.'}
        </p>
      )}
    </div>
  );
}

/** Modo presupuesto o proforma del formulario: se guarda en vez de emitirse. */
export interface QuoteMode {
  kind: 'presupuesto' | 'proforma';
  /** Presupuesto que se cambia (si no, uno nuevo). */
  id: number | null;
  validUntil: string | null;
  onSaved: (q: Quote) => void;
}

/** «AAAA-MM-DD» `days` días después de hoy. */
function inDays(days: number): string {
  const d = new Date();
  d.setDate(d.getDate() + days);
  return today(d);
}

export function NewInvoice({
  settings,
  onIssued,
  onCancel,
  rectifies,
  rectifiesDraft,
  initial,
  initialLanguage,
  quote,
  fromQuote,
  simplifiedMoved,
  rectifyOthers = [],
}: {
  settings: InvoicingSettings;
  /** Con el aviso de la exportación (copia, Facturae, UBL) si algo no se pudo guardar. */
  onIssued: (warning: string | null) => void;
  onCancel: () => void;
  rectifies?: IssuedSummary | undefined;
  /** La factura que se rectifica tal como se emitió: de ella salen los datos del cliente. */
  rectifiesDraft?: InvoiceDraft | undefined;
  /** Borrador del que partir («Duplicar», un presupuesto). */
  initial?: InvoiceDraft | undefined;
  initialLanguage?: 'Es' | 'En' | null | undefined;
  /** Presupuesto o proforma en vez de factura. */
  quote?: QuoteMode | undefined;
  /** La factura sale de este presupuesto: al emitirla queda enlazada. */
  fromQuote?: number | undefined;
  /** Ya hay simplificadas en la serie ordinaria: avisar de que ahora van en la suya. */
  simplifiedMoved?: boolean | undefined;
  /** Rectificativa: otras facturas del mismo cliente que puede agrupar. */
  rectifyOthers?: IssuedSummary[] | undefined;
}) {
  const defaultVat = Math.max(
    0,
    VAT_OPTIONS.findIndex(
      (o) =>
        typeof o.value === 'object' &&
        'Taxed' in o.value &&
        o.value.Taxed.rate === settings.default_vat,
    ),
  );
  const [validUntil, setValidUntil] = useState(
    quote ? (quote.validUntil ?? (quote.kind === 'presupuesto' ? inDays(30) : '')) : '',
  );
  const rectificada =
    rectifiesDraft && rectifies?.kind !== 'F2' ? fromDraft(rectifiesDraft, defaultVat) : null;
  const [f, setF] = useState<Form>(() =>
    initial
      ? {
          // Al editar un presupuesto se conserva su fecha; si no (duplicar, facturar), hoy.
          ...fromDraft(initial, defaultVat, quote !== undefined && quote.id !== null),
          language: initialLanguage ?? '',
        }
      : {
          clientName: rectifies?.recipient_name ?? '',
          clientNif: '',
          country: 'ES',
          street: '',
          postCode: '',
          town: '',
          province: '',
          // Rectificativa: el cliente de la factura que rectifica (salvo la de una simplificada,
          // R5, que va sin él).
          ...(rectifiesDraft && rectifies?.kind !== 'F2' ? clientOf(rectifiesDraft) : {}),
          language: initialLanguage ?? '',
          operationDate: '',
          // La rectificativa de una factura en otra moneda, en esa moneda y a su tipo.
          // `TODO(VERIFY)`: qué tipo aplica a la rectificativa (el de la original o el del día;
          // ver `invoicing::check_rate`).
          ...(rectifies?.currency_rate
            ? {
                currency: rectifies.currency_rate.code,
                fx: rateValueOf(rectifies.currency_rate),
              }
            : {}),
          date: today(),
          description: '',
          lines: [
            {
              description: '',
              quantity: '1',
              price: '',
              // Rectificativa: con el IVA y la retención de la factura que rectifica (una
              // rectificativa a un cliente de la UE no lleva IVA español ni IRPF).
              vat: rectificada?.lines[0]?.vat ?? defaultVat,
              withholding: rectificada
                ? (rectificada.lines[0]?.withholding ?? false)
                : settings.default_withholding !== null,
            },
          ],
        },
  );
  /** Particular de la UE con el IVA de su país (ventanilla única): las líneas no llevan IVA español. */
  const ossOn =
    f.oss !== undefined && f.consumer === true && EU_COUNTRIES.includes(f.country || 'ES');
  const [rect, setRect] = useState<RectChoice>({
    code: rectifies?.kind === 'F2' ? 5 : 1,
    method: 'Diferencias',
    also: [],
  });
  /** Por sustitución se parte de las líneas de la factura que se sustituye (importes completos). */
  function changeRect(c: RectChoice) {
    if (c.method === 'Sustitucion' && rect.method !== 'Sustitucion' && rectificada) {
      setF({ ...f, lines: rectificada.lines, description: rectificada.description });
    }
    setRect(c);
  }
  const [totals, setTotals] = useState<Totals | null>(null);
  /** Totales en euros de la última vista previa (factura en moneda extranjera) y su moneda. */
  const [eurTotals, setEurTotals] = useState<Totals | null>(null);
  const [totalsCode, setTotalsCode] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [confirmedDraft, setConfirmedDraft] = useState<InvoiceDraft | null>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (confirming) confirmRef.current?.focus();
  }, [confirming]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const setLine = (i: number, patch: Partial<LineForm>) =>
    setF({ ...f, lines: f.lines.map((l, j) => (j === i ? { ...l, ...patch } : l)) });

  // Cualquier cambio en el formulario invalida la confirmación: se emite exactamente lo que se
  // ha confirmado (`confirmedDraft`), nunca lo que haya en el formulario en ese momento.
  // biome-ignore lint/correctness/useExhaustiveDependencies: se reinicia al cambiar el formulario
  useEffect(() => {
    setConfirming(false);
    setConfirmedDraft(null);
  }, [f, rect]);

  async function preview(open: boolean): Promise<InvoiceDraft | null> {
    setError(null);
    const d = toDraft(f, settings, rectifies, false, fromQuote !== undefined, rect);
    if (typeof d === 'string') {
      setError(d);
      return null;
    }
    try {
      const p = await invoicePreview(d, open, f.language || null);
      setTotals(p.totals);
      setEurTotals(p.eur_totals ?? null);
      setTotalsCode(d.currency?.code ?? null);
      return d;
    } catch (e) {
      setError(errorMessage(e));
      return null;
    }
  }

  async function saveQuote() {
    if (!quote || busy) return;
    setError(null);
    const d = toDraft(f, settings, undefined, true);
    if (typeof d === 'string') {
      setError(d);
      return;
    }
    setBusy(true);
    try {
      const input = {
        kind: quote.kind,
        draft: d,
        valid_until: quote.kind === 'presupuesto' && validUntil ? validUntil : null,
        language: f.language || null,
      };
      quote.onSaved(
        quote.id === null ? await quoteCreate(input) : await quoteUpdate(quote.id, input),
      );
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function issue() {
    const d = confirmedDraft;
    if (!d || busy) return;
    setBusy(true);
    try {
      // La de un presupuesto se emite y lo deja facturado de una vez (o nada, si falla).
      const out =
        fromQuote !== undefined
          ? await quoteIssueInvoice(fromQuote, d, true, f.language || null)
          : await invoiceIssue(d, true, f.language || null);
      setConfirming(false);
      // El formulario se cierra: el aviso lo muestra la lista.
      onIssued(out.export_error ? `Emitida ${out.number}, con avisos: ${out.export_error}` : null);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const code = foreignCode(f);
  const micros = code ? parseRate(f.fx?.rate ?? '') : null;
  const approx = code && micros ? approxTotal(f.lines, settings) : null;

  return (
    <div className="mt-4 space-y-3 rounded-lg border border-stone-200 bg-white p-4">
      {rectifies && (
        <RectificacionFields
          rectifies={rectifies}
          choice={rect}
          onChange={changeRect}
          others={rectifyOthers}
          canSubstitute={rectifiesDraft !== undefined}
        />
      )}
      <ClientFields
        f={f}
        setF={setF}
        defaultVat={defaultVat}
        hasWithholding={settings.default_withholding !== null}
        quote={quote !== undefined}
        rectifying={rectifies !== undefined}
      />

      <table className="w-full text-sm">
        <thead>
          <tr className="text-left text-stone-500">
            <th>Concepto</th>
            <th className="w-20">Cant.</th>
            <th className="w-28">Precio{code ? ` (${code})` : ''}</th>
            <th className="w-48">IVA</th>
            <th className="w-16">IRPF</th>
            <th className="w-20" title="Anticipo: positivo al cobrarlo, negativo al descontarlo">
              Anticipo
            </th>
            <th className="w-10" />
          </tr>
        </thead>
        <tbody>
          {f.lines.map((l, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: las líneas no tienen id propio
            <tr key={i}>
              <td>
                <input
                  aria-label={`Concepto ${i + 1}`}
                  className={input}
                  value={l.description}
                  onChange={(e) => setLine(i, { description: e.target.value })}
                />
              </td>
              <td>
                <input
                  aria-label={`Cantidad ${i + 1}`}
                  className={input}
                  value={l.quantity}
                  onChange={(e) => setLine(i, { quantity: e.target.value })}
                />
              </td>
              <td>
                <input
                  aria-label={`Precio ${i + 1}`}
                  className={input}
                  value={l.price}
                  onChange={(e) => setLine(i, { price: e.target.value })}
                />
              </td>
              <td>
                {ossOn ? (
                  <span className="text-xs text-stone-600">IVA {f.country}</span>
                ) : (
                  <select
                    aria-label={`IVA ${i + 1}`}
                    className={input}
                    value={l.vat}
                    onChange={(e) => setLine(i, { vat: Number(e.target.value) })}
                  >
                    {VAT_OPTIONS.map((o, j) => (
                      <option key={o.label} value={j}>
                        {o.label}
                      </option>
                    ))}
                  </select>
                )}
              </td>
              <td className="text-center" title={NO_WITHHOLDING_HINT(settings)}>
                <input
                  type="checkbox"
                  aria-label={`Retención IRPF ${i + 1}`}
                  aria-description={NO_WITHHOLDING_HINT(settings)}
                  disabled={settings.default_withholding === null}
                  checked={l.withholding}
                  onChange={(e) => setLine(i, { withholding: e.target.checked })}
                />
              </td>
              <td className="text-center">
                <input
                  type="checkbox"
                  aria-label={`Anticipo ${i + 1}`}
                  checked={l.advance ?? false}
                  onChange={(e) => setLine(i, { advance: e.target.checked })}
                />
              </td>
              <td>
                {f.lines.length > 1 && (
                  <button
                    type="button"
                    aria-label="Quitar línea"
                    className="btn-icon"
                    onClick={() => setF({ ...f, lines: f.lines.filter((_, j) => j !== i) })}
                  >
                    <X size={16} aria-hidden="true" />
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <button
        type="button"
        className="text-sm text-link"
        onClick={() =>
          setF({
            ...f,
            lines: [
              ...f.lines,
              {
                ...(f.lines.at(-1) ??
                  f.lines[0] ?? { quantity: '1', vat: defaultVat, withholding: false }),
                description: '',
                price: '',
              },
            ],
          })
        }
      >
        + Añadir línea
      </button>

      {approx !== null && micros !== null && (
        <p className="text-right text-sm text-stone-600" aria-live="polite">
          Total aproximado: {money(approx, code)} ≈ {euros(toEur(approx, micros))} (1 € ={' '}
          {rateText(micros)} {code})
        </p>
      )}

      {totals && (
        <dl className="ml-auto w-72 text-sm">
          <div className="flex justify-between">
            <dt>Base</dt>
            <dd>{money(totals.base, totalsCode)}</dd>
          </div>
          <div className="flex justify-between">
            <dt>Impuestos</dt>
            <dd>{money(totals.tax_total, totalsCode)}</dd>
          </div>
          {totals.oss && (
            <div className="flex justify-between">
              <dt>
                IVA de {totals.oss.country} {pctText(totals.oss.rate)} (OSS)
              </dt>
              <dd>{money(totals.oss.vat, totalsCode)}</dd>
            </div>
          )}
          {totals.withholding !== 0 && (
            <div className="flex justify-between">
              <dt>Retención</dt>
              <dd>-{money(totals.withholding, totalsCode)}</dd>
            </div>
          )}
          <div className="flex justify-between font-semibold">
            <dt>Total</dt>
            <dd>{money(totals.payable, totalsCode)}</dd>
          </div>
          {eurTotals && (
            <>
              <div className="mt-1 flex justify-between border-t border-stone-200 pt-1 text-stone-600">
                <dt>Base en euros</dt>
                <dd>{euros(eurTotals.base)}</dd>
              </div>
              <div className="flex justify-between text-stone-600">
                <dt>IVA en euros</dt>
                <dd>{euros(eurTotals.tax_total)}</dd>
              </div>
              <div className="flex justify-between font-semibold">
                <dt>Total en euros</dt>
                <dd>{euros(eurTotals.payable)}</dd>
              </div>
            </>
          )}
        </dl>
      )}

      {quote?.kind === 'presupuesto' && (
        <div className="form-grid">
          <label>
            Válido hasta
            <input
              type="date"
              className={input}
              value={validUntil}
              onChange={(e) => setValidUntil(e.target.value)}
            />
          </label>
        </div>
      )}
      {quote && (
        <div className="form-actions border-t border-stone-200 pt-4">
          <button
            type="button"
            disabled={busy}
            className="rounded-md btn-primary px-4 py-1.5 text-sm font-medium disabled:opacity-50"
            onClick={saveQuote}
          >
            {busy
              ? 'Guardando…'
              : quote.kind === 'presupuesto'
                ? 'Guardar presupuesto'
                : 'Guardar proforma'}
          </button>
          <button type="button" className="btn-ghost" onClick={onCancel}>
            Cancelar
          </button>
        </div>
      )}
      <div className={quote ? 'hidden' : 'form-actions border-t border-stone-200 pt-4'}>
        <button
          type="button"
          className="rounded-md border border-stone-300 px-3 py-1.5 text-sm"
          onClick={() => preview(true)}
        >
          Ver borrador (PDF)
        </button>
        <button
          type="button"
          className="rounded-md btn-primary px-4 py-1.5 text-sm font-medium"
          onClick={async () => {
            const d = await preview(false);
            if (d) {
              setConfirmedDraft(d);
              setConfirming(true);
            }
          }}
        >
          Emitir factura…
        </button>
        {!confirming && (
          <button type="button" className="btn-ghost" onClick={onCancel}>
            Cancelar
          </button>
        )}
      </div>

      {confirming && totals && (
        <div
          role="dialog"
          aria-label="Confirmar emisión"
          className="rounded-md border border-amber-300 bg-amber-50 p-3 text-sm"
          onKeyDown={(e) => {
            if (e.key === 'Escape' && !busy) setConfirming(false);
          }}
        >
          <p className="font-medium">
            ¿Emitir la factura definitiva por {money(totals.payable, totalsCode)}
            {eurTotals && ` (${euros(eurTotals.payable)} al tipo indicado)`}?
          </p>
          <p className="mt-1 text-stone-700">
            Se le asignará el siguiente número de la serie{' '}
            {rectifies
              ? rectifies.kind === 'F2'
                ? (settings.simplified_rectifying_series ?? 'RS')
                : settings.rectifying_series
              : f.clientNif.trim() !== ''
                ? settings.default_series
                : (settings.simplified_series ?? 'S')}{' '}
            y no se podrá modificar: para corregirla habrá que emitir una rectificativa.
            {rectifies &&
              rect.method === 'Sustitucion' &&
              ` Sustituye a ${[rectifies, ...rect.also].map((i) => i.number).join(', ')}: se contabiliza y se declara la diferencia.`}
            {settings.verifactu.enabled && ' Se generará su registro VERI*FACTU encadenado.'}
          </p>
          {simplifiedMoved && !rectifies && f.clientNif.trim() === '' && (
            <p className="mt-1 text-amber-800">
              Cambio de numeración: las facturas simplificadas (sin NIF del cliente) van ahora en su
              propia serie {settings.simplified_series ?? 'S'}, separada de las facturas completas,
              como exige el Reglamento de facturación (art. 7.1.a). Las que ya emitiste conservan su
              número; las nuevas empiezan en la serie {settings.simplified_series ?? 'S'}.
            </p>
          )}
          <div className="mt-3 form-actions">
            <button
              ref={confirmRef}
              type="button"
              disabled={busy}
              className="rounded-md btn-primary px-3 py-1"
              onClick={issue}
            >
              Sí, emitir
            </button>
            <button type="button" className="btn-ghost" onClick={() => setConfirming(false)}>
              Cancelar
            </button>
          </div>
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

/** Estado del registro VERI*FACTU de una factura, en palabras. */
function vfLabel(status: string): string {
  switch (status) {
    case 'pendiente':
      return 'pendiente de remitir';
    case 'pruebas':
      return 'Aceptado en pruebas';
    case 'correcto':
      return 'remitida';
    case 'aceptado_con_errores':
      return 'aceptada con errores';
    case 'incorrecto':
      return 'rechazada';
    case 'sin_respuesta':
      return 'sin respuesta de la AEAT';
    default:
      return status.replaceAll('_', ' ');
  }
}

/** Aviso de la versión que no emite facturas (la que se descarga de la web). */
export function SinFacturacion() {
  return (
    <div className="mt-3 rounded-md border border-amber-200 bg-amber-50 p-3 text-sm text-amber-900">
      <p className="font-medium">Esta versión de TaxPain no emite facturas.</p>
      <p className="mt-1">
        Es la que se descarga de la web: quien la distribuye todavía no figura como productor del
        sistema de facturación (VERI*FACTU). Puedes ver las facturas que importes, preparar
        presupuestos y llevar todo lo demás. Para emitir facturas, compila TaxPain desde su código
        con tus datos.
      </p>
    </div>
  );
}

export function Invoices({ settings }: { settings: InvoicingSettings }) {
  const facturacion = hayFacturacion();
  const [year, setYear] = useState(new Date().getFullYear());
  const [list, setList] = useState<IssuedSummary[]>([]);
  const [creating, setCreating] = useState(false);
  const [rectifying, setRectifying] = useState<IssuedSummary | null>(null);
  /** La factura que se rectifica tal como se emitió (datos del cliente) y su idioma. */
  const [rectifyingDraft, setRectifyingDraft] = useState<{
    draft: InvoiceDraft | undefined;
    language: 'Es' | 'En' | null;
  } | null>(null);
  const [emailing, setEmailing] = useState<IssuedSummary | null>(null);
  const [duplicate, setDuplicate] = useState<{
    id: number;
    draft: InvoiceDraft;
    language: 'Es' | 'En' | null;
  } | null>(null);
  const [templates, setTemplates] = useState<InvoiceTemplate[]>([]);
  const [cert, setCert] = useState('');
  const [sending, setSending] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const [issues, setIssues] = useState<RecordIssue[]>([]);
  // El envío manda todos los registros pendientes, no sólo los del año que se ve: se cuentan
  // todos (el mismo recuento que el panel). Si no se puede, se cuentan los de la lista.
  const [pendingAll, setPendingAll] = useState<number | null>(null);
  // Cambia en cada recarga: el panel de la cola se vuelve a leer.
  const [queueTick, setQueueTick] = useState(0);
  const load = useCallback(() => {
    setQueueTick((t) => t + 1);
    invoicesList(year)
      .then(setList)
      .catch((e) => setMessage(errorMessage(e)));
    recurringList()
      .then(setTemplates)
      .catch(() => setTemplates([]));
    if (settings.verifactu.enabled) {
      verifactuIssues()
        .then(setIssues)
        .catch(() => setIssues([]));
      const now = new Date();
      dashboardGet(now.getFullYear(), now.getMonth() + 1)
        .then((d) => setPendingAll(d.pending.verifactu_unsent))
        .catch(() => setPendingAll(null));
    }
  }, [year, settings.verifactu.enabled]);

  useEffect(load, [load]);
  const pending = pendingAll ?? list.filter((i) => i.verifactu_status === 'pendiente').length;
  const formOpen = creating || rectifying !== null || duplicate !== null;
  // Simplificadas del año emitidas en otra serie y ninguna aún en la suya (RD 1619/2012 art.
  // 7.1.a): la numeración de las nuevas cambia y se avisa al emitir.
  const simplifiedSeries = settings.simplified_series ?? 'S';
  const appF2 = list.filter((i) => i.kind === 'F2' && i.origin !== 'importada');
  const simplifiedMoved =
    appF2.some((i) => !i.number.startsWith(simplifiedSeries)) &&
    !appF2.some((i) => i.number.startsWith(simplifiedSeries));
  const closeForm = () => {
    setCreating(false);
    setRectifying(null);
    setRectifyingDraft(null);
    setDuplicate(null);
  };

  function saveAs(i: IssuedSummary, format: InvoiceFormat) {
    invoiceSave(i.id, i.number, format)
      .then((saved) => saved && setMessage(`${i.number} guardada.`))
      .catch((err) => setMessage(errorMessage(err)));
  }

  async function saveSigned(i: IssuedSummary) {
    try {
      const fp = cert || (await listCertificates())[0]?.fingerprint_sha256 || '';
      if (!fp) {
        setMessage('Para firmar, importa primero tu certificado en Ajustes.');
        return;
      }
      if (await invoiceSignFacturae(i.id, i.number, fp))
        setMessage(`${i.number} firmada y guardada (Facturae .xsig).`);
    } catch (err) {
      setMessage(errorMessage(err));
    }
  }

  function duplicateOf(i: IssuedSummary) {
    // Con el idioma en que se emitió (una factura en inglés, en inglés).
    Promise.all([invoiceDraftOf(i.id), invoiceLanguageOf(i.id).catch(() => null)])
      .then(([draft, language]) => {
        setRectifying(null);
        setCreating(false);
        setDuplicate({ id: i.id, draft, language });
      })
      .catch((err) => setMessage(errorMessage(err)));
  }

  function rectify(i: IssuedSummary) {
    // Con el cliente y el idioma de la factura que se rectifica. Si no se pueden leer (una
    // importada de la gestoría), el formulario se abre igual y se rellenan a mano.
    Promise.all([
      invoiceDraftOf(i.id).catch(() => undefined),
      invoiceLanguageOf(i.id).catch(() => null),
    ]).then(([draft, language]) => {
      setCreating(false);
      setDuplicate(null);
      setRectifyingDraft({ draft, language });
      setRectifying(i);
    });
  }

  function repeat(i: IssuedSummary, months: number) {
    recurringCreate(i.id, months)
      .then((t) => {
        setMessage(
          `Se repetirá ${everyLabel(t.every_months).toLowerCase()}: la siguiente, el ${formatDate(t.start)}. La emites tú cuando toque (te avisa el panel).`,
        );
        load();
      })
      .catch((err) => setMessage(errorMessage(err)));
  }

  async function send() {
    if (!cert) return setMessage('Importa primero tu certificado electrónico.');
    if (sending) return;
    setSending(true);
    try {
      const r = await verifactuSend(cert, true);
      setMessage(
        r
          ? `Envío ${sendStateLabel(r.estado_envio)}: ${plural(r.registros, 'registro')}${
              (r.envios ?? 1) > 1 ? ` en ${r.envios} envíos` : ''
            }${r.incidencia ? ', marcado como incidencia' : ''} (entorno de pruebas).${
              r.fallo ? ` El resto queda en cola: ${r.fallo}` : ''
            }`
          : 'Nada pendiente.',
      );
      load();
    } catch (e) {
      setMessage(errorMessage(e));
    } finally {
      setSending(false);
    }
  }

  return (
    <section className="mt-6">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold">Facturas emitidas {year}</h2>
        <div className="flex items-center gap-2 text-sm">
          <YearStepper year={year} onChange={setYear} />
          {facturacion && (
            <button
              type="button"
              className={formOpen ? 'rounded border px-3' : 'btn-primary rounded px-3'}
              onClick={() => (formOpen ? closeForm() : setCreating(true))}
            >
              {!formOpen && <Plus size={16} aria-hidden="true" />}
              {formOpen ? 'Cerrar' : 'Nueva factura'}
            </button>
          )}
        </div>
      </div>
      {!facturacion && <SinFacturacion />}
      {facturacion && formOpen && (
        <NewInvoice
          key={rectifying?.id ?? (duplicate ? `copia-${duplicate.id}` : 'nueva')}
          settings={settings}
          rectifies={rectifying ?? undefined}
          rectifiesDraft={rectifyingDraft?.draft}
          initial={duplicate?.draft}
          initialLanguage={duplicate?.language ?? rectifyingDraft?.language}
          simplifiedMoved={simplifiedMoved}
          rectifyOthers={
            rectifying
              ? list.filter(
                  (x) =>
                    x.id !== rectifying.id &&
                    x.status === 'emitida' &&
                    x.kind === rectifying.kind &&
                    x.kind !== 'F2' &&
                    x.recipient_name !== null &&
                    x.recipient_name === rectifying.recipient_name &&
                    x.issue_date <= today(),
                )
              : []
          }
          onCancel={closeForm}
          onIssued={(warning) => {
            closeForm();
            setMessage(warning);
            load();
          }}
        />
      )}
      {issues.length > 0 && (
        <div role="alert" className="mt-3 rounded-md border border-red-300 bg-red-50 p-3 text-sm">
          <p className="font-medium">
            La AEAT ha rechazado o aceptado con errores{' '}
            {plural(issues.length, 'registro VERI*FACTU', 'registros VERI*FACTU')}
          </p>
          <p className="mt-1 text-stone-700">
            Si el error es de datos de la factura, lo que procede es una rectificativa. Si no (un
            fallo técnico, la huella…), genera la subsanación y vuelve a enviar.
          </p>
          <ul className="mt-2 space-y-2">
            {issues.map((r) => (
              <li key={r.id}>
                <span className="font-medium">
                  {r.num_serie} · {r.record_type === 'alta' ? 'alta' : 'anulación'} ·{' '}
                  {r.send_status === 'incorrecto' ? 'rechazado' : 'aceptado con errores'}
                </span>
                {(r.error_code || r.error_text) && (
                  <span className="text-stone-700">
                    {' '}
                    — {r.error_code} {r.error_text}
                  </span>
                )}
                {!(r.record_type === 'anulacion' && r.send_status === 'aceptado_con_errores') && (
                  <ConfirmAction
                    className="ml-2 text-link underline"
                    label="Subsanar…"
                    question={`Se generará un registro nuevo de subsanación de ${r.num_serie}, encadenado al último, con los mismos datos de la factura. Quedará pendiente de envío.`}
                    confirmLabel="Sí, generar la subsanación"
                    danger={false}
                    onConfirm={async () => {
                      try {
                        await verifactuResubmit(r.id);
                        load();
                      } catch (e) {
                        setMessage(errorMessage(e));
                      }
                    }}
                  />
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
      {settings.verifactu.enabled && pending > 0 && (
        <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-2 text-sm">
          <span>
            {plural(pending, 'registro VERI*FACTU pendiente', 'registros VERI*FACTU pendientes')} de
            enviar.
          </span>
          <button
            type="button"
            disabled={sending}
            className="text-link underline disabled:opacity-50"
            onClick={send}
          >
            {sending ? 'Enviando…' : 'Enviar a la AEAT (pruebas)'}
          </button>
          <CertificateSelect value={cert} onChange={setCert} />
        </div>
      )}
      {facturacion && <VerifactuCola refresh={queueTick} />}
      {message && (
        <p role="status" className="mt-2 text-sm text-stone-700">
          {message}
        </p>
      )}
      {facturacion && <Recurring templates={templates} onChanged={load} onMessage={setMessage} />}
      <NumberingCheck key={year} year={year} />
      {emailing && (
        <InvoiceEmailPanel
          key={emailing.id}
          invoiceId={emailing.id}
          number={emailing.number}
          reminder={false}
          onClose={() => setEmailing(null)}
          onDone={(m) => {
            setEmailing(null);
            setMessage(m);
          }}
        />
      )}
      <ul className="mt-3 divide-y divide-stone-200">
        {list.map((i) => (
          <li key={i.id} className="flex flex-wrap items-center justify-between gap-3 py-3 text-sm">
            <div className="flex min-w-0 flex-1 items-center gap-3">
              <span className="icon-tile" aria-hidden="true">
                <FileText size={18} strokeWidth={1.8} />
              </span>
              <div className="min-w-0">
                <p className="font-medium">
                  {i.number} · {i.recipient_name ?? 'Factura simplificada'}
                  {i.status === 'anulada' && <span className="pill pill-red ml-2">(anulada)</span>}
                  {i.origin === 'importada' && (
                    <span className="pill pill-stone ml-2">del libro de la gestoría</span>
                  )}
                  {i.export_error && (
                    <span className="pill pill-amber ml-2" title={i.export_error}>
                      aviso al exportar
                    </span>
                  )}
                </p>
                <p className="mt-0.5 flex flex-wrap items-center gap-x-2 text-stone-500">
                  {formatDate(i.issue_date)} · {kindLabel(i.kind)}
                  {i.verifactu_status && (
                    <span
                      className={`pill ${
                        i.verifactu_status === 'correcto'
                          ? 'pill-green'
                          : i.verifactu_status === 'pendiente' || i.verifactu_status === 'pruebas'
                            ? 'pill-amber'
                            : 'pill-red'
                      }`}
                      title={i.verifactu_error ?? undefined}
                    >
                      VERI*FACTU: {vfLabel(i.verifactu_status)}
                    </span>
                  )}
                </p>
              </div>
            </div>
            <div className="flex shrink-0 items-center gap-3">
              <span className="text-right tabular-nums">
                <span className="block text-[0.95rem] font-semibold">{euros(i.payable)}</span>
                {i.currency && i.currency_payable != null && (
                  <span className="block text-xs text-stone-500">
                    {money(i.currency_payable, i.currency)}
                  </span>
                )}
              </span>
              <button
                type="button"
                className="text-link"
                onClick={() => invoiceOpen(i.id, false).catch((e) => setMessage(errorMessage(e)))}
              >
                PDF
              </button>
              {(i.has_facturae || i.has_ubl) && (
                <Menu
                  label={`Guardar ${i.number} como`}
                  entries={[
                    { heading: 'Guardar como' },
                    { label: 'PDF', onSelect: () => saveAs(i, 'pdf') },
                    ...(i.has_facturae
                      ? [{ label: 'Facturae (XML)', onSelect: () => saveAs(i, 'facturae') }]
                      : []),
                    ...(i.has_facturae && i.status === 'emitida'
                      ? [
                          {
                            label: 'Facturae firmada…',
                            hint: '.xsig, para FACe',
                            onSelect: () => saveSigned(i),
                          },
                        ]
                      : []),
                    ...(i.has_ubl
                      ? [{ label: 'UBL (XML)', onSelect: () => saveAs(i, 'ubl') }]
                      : []),
                  ]}
                >
                  Guardar
                  <ChevronDown size={14} aria-hidden="true" />
                </Menu>
              )}
              {i.status === 'emitida' && i.origin !== 'importada' && (
                <button type="button" className="text-link" onClick={() => setEmailing(i)}>
                  Correo
                </button>
              )}
              {facturacion && !i.kind.startsWith('R') && i.origin !== 'importada' && (
                <Menu
                  label={`Más acciones de ${i.number}`}
                  entries={[
                    { label: 'Duplicar', onSelect: () => duplicateOf(i) },
                    ...(i.recipient_name
                      ? [
                          { separator: true as const },
                          { heading: 'Repetir' },
                          { label: 'Cada mes', onSelect: () => repeat(i, 1) },
                          { label: 'Cada trimestre', onSelect: () => repeat(i, 3) },
                          { label: 'Cada año', onSelect: () => repeat(i, 12) },
                        ]
                      : []),
                  ]}
                >
                  Más
                  <ChevronDown size={14} aria-hidden="true" />
                </Menu>
              )}
              {facturacion && i.status === 'emitida' && !i.kind.startsWith('R') && (
                <button type="button" className="text-link" onClick={() => rectify(i)}>
                  Rectificar
                </button>
              )}
              {facturacion && i.status === 'emitida' && i.origin !== 'importada' && (
                <ConfirmAction
                  label="Anular"
                  question={`¿Anular ${i.number}? Sólo si se emitió por error y no se ha entregado: se genera el registro de anulación y no se puede deshacer.`}
                  confirmLabel={`Sí, anular ${i.number}`}
                  onConfirm={async () => {
                    // Si falla, ConfirmAction enseña el error y deja el panel abierto.
                    await invoiceCancel(i.id, true);
                    load();
                  }}
                />
              )}
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

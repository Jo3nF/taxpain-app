/**
 * Modo demo (`?demo=1`): la interfaz real con datos SINTÉTICOS, para capturas y vídeo.
 * Ningún dato es real: la titular y los NIF son los de prueba de los tests (válidos por
 * algoritmo, no asignados a personas conocidas).
 */
import type {
  AccountBalance,
  AeatCheckReport,
  Ahorro,
  AjustesPatrimonial,
  AnnualAccounts,
  AreaRevision,
  AssetSale,
  AssistantTopic,
  BancaActualizacion,
  BancaEstado,
  BankTx,
  CalendarView,
  CertificateInfo,
  ClasePatrimonial,
  Conversation,
  ConversationSummary,
  CriptoOp,
  CriptoPlatform,
  CriptoPrice,
  CriptoReport,
  DeadlineView,
  ExpedienteVista,
  FixedAsset,
  Hallazgo,
  InformePatrimonial,
  InvoicingSettings,
  IssuedSummary,
  MarcaAhorro,
  MessageNotes,
  Minute,
  ModelDraft,
  OpeningDraft,
  OrigenHallazgo,
  PrestamoCuota,
  PrestamoVista,
  ReceivedCheck,
  ReceivedResumen,
  ReceivedSummary,
  Recomendacion,
  Rectificativa,
  RectificativaAnterior,
  Reviewed,
  Revision,
  SaleCandidate,
  ShareMovement,
  Simulacion202,
  SoleContract,
  StoredEntry,
  TaxpayerProfile,
  TopicHeader,
  TopicOpened,
  VentaPatrimonial,
  VigenciaEjercicio,
} from './api';
import { draftPeriod, draftTitle, euros, formatDate, setDemoInvoke, topicKey } from './api';
import { ajustesCierreHandlers } from './demoAjustesCierre';
import { autonomoHandlers } from './demoAutonomo';
import { censoHandlers } from './demoCenso';
import { cuentasModeloHandlers } from './demoCuentasModelo';
import { extranjeroHandlers } from './demoExtranjero';
import { ivaHoldingHandlers } from './demoIvaHolding';
import { modelo200Handlers } from './demoModelo200';
import { nominasHandlers } from './demoNominas';
import { draft216, draft296, noResidentesHandlers } from './demoNoResidentes';
import { ossHandlers } from './demoOss';
import { subvencionesHandlers } from './demoSubvenciones';
import { trimestreHandlers } from './demoTrimestre';
import { valoresHandlers } from './demoValores';
import { vinculadasHandlers } from './demoVinculadas';

const TODAY = '2026-10-13';

const profile: TaxpayerProfile = {
  persona: 'Fisica',
  irpf_method: 'EstimacionDirecta',
  exento_pagos_fraccionados: false,
  iva: { periodicity: 'Trimestral', sii: false },
  retiene_trabajo_profesionales: false,
  retiene_alquileres: true,
  retiene_capital_mobiliario: false,
  retiene_no_residentes: false,
  gran_empresa: false,
  intracomunitarias: null,
  obligado_347: false,
  cierre_ejercicio: ['December', 31],
  pagos_fraccionados_is: false,
  ccaa: 'ComunitatValenciana',
  festivos_locales: [],
  bienes_extranjero: false,
  monedas_virtuales_extranjero: false,
  operaciones_vinculadas: false,
  renta_fraccionada: false,
  iae_desde: null,
  oss: false,
  ioss: false,
  opciones_censales: false,
  cotiza_reta: false,
  tiene_trabajadores: false,
};

function d(
  model: string,
  description: string,
  period: string,
  fiscal_year: number,
  starts_on: string,
  due_on: string,
  domiciliation_end: string | null,
): DeadlineView {
  return {
    model,
    title: `Modelo ${model}`,
    description,
    period,
    fiscal_year,
    starts_on,
    nominal_end: due_on,
    due_on,
    domiciliation_end,
    domiciliation_official: true,
    tentative: false,
    rule_ref: '',
  };
}

const IVA = 'IVA: declaración del periodo';
const IRPF = 'IRPF: pago fraccionado (estimación directa)';
const ALQ = 'Retenciones del alquiler de locales';

function vigenciaDemo(year: number): VigenciaEjercicio {
  return {
    year,
    comprobaciones: [
      {
        clave: 'festivos_nacionales',
        titulo: `Festivos nacionales de ${year}`,
        estado: 'listo',
        detalle: '9 días, de la resolución de fiestas laborales del BOE.',
      },
      {
        clave: 'festivos_locales',
        titulo: `Festivos locales de ${year}`,
        estado: 'pendiente',
        detalle:
          'Pendiente de publicar o de añadir: los dos de tu municipio salen en el boletín de la comunidad o de la provincia.',
      },
      {
        clave: 'calendario_aeat',
        titulo: `Calendario del contribuyente ${year} (AEAT)`,
        estado: 'supuesto',
        detalle:
          'Aún no está en TaxPain: el último día para domiciliar y la campaña de Renta son estimados.',
      },
      {
        clave: 'interes_demora',
        titulo: `Interés de demora de ${year}`,
        estado: 'supuesto',
        detalle: '4,0625 %. Se mantiene el del año anterior.',
      },
    ],
  };
}

const calendar: CalendarView = {
  year: 2026,
  today: TODAY,
  holidays_loaded: true,
  aeat_calendar_loaded: true,
  local_holidays: 2,
  deadlines: [
    d('115', ALQ, '4T', 2025, '2026-01-01', '2026-01-20', '2026-01-15'),
    d('303', IVA, '4T', 2025, '2026-01-01', '2026-01-30', '2026-01-27'),
    d('130', IRPF, '4T', 2025, '2026-01-01', '2026-01-30', '2026-01-27'),
    d('390', 'IVA: resumen anual', '0A', 2025, '2026-01-01', '2026-01-30', null),
    d(
      '180',
      'Resumen anual de retenciones de alquileres',
      '0A',
      2025,
      '2026-01-01',
      '2026-02-02',
      null,
    ),
    d('100', 'Declaración de la Renta', '0A', 2025, '2026-04-08', '2026-06-30', '2026-06-25'),
    d('303', IVA, '1T', 2026, '2026-04-01', '2026-04-20', '2026-04-15'),
    d('130', IRPF, '1T', 2026, '2026-04-01', '2026-04-20', '2026-04-15'),
    d('115', ALQ, '1T', 2026, '2026-04-01', '2026-04-20', '2026-04-15'),
    d('303', IVA, '2T', 2026, '2026-07-01', '2026-07-20', '2026-07-15'),
    d('130', IRPF, '2T', 2026, '2026-07-01', '2026-07-20', '2026-07-15'),
    d('115', ALQ, '2T', 2026, '2026-07-01', '2026-07-20', '2026-07-15'),
    d('303', IVA, '3T', 2026, '2026-10-01', '2026-10-20', '2026-10-15'),
    d('130', IRPF, '3T', 2026, '2026-10-01', '2026-10-20', '2026-10-15'),
    d('115', ALQ, '3T', 2026, '2026-10-01', '2026-10-20', '2026-10-15'),
  ],
};

const certificate: CertificateInfo = {
  fingerprint_sha256: 'a1'.repeat(32),
  subject_dn: 'CN=ESPAÑOLA ESPAÑOLA CARMEN - 12345678Z',
  issuer_dn: 'CN=AC FNMT Usuarios',
  serial: '01',
  valid_from: '2025-03-01T00:00:00Z',
  valid_to: '2029-03-01T00:00:00Z',
  kind: 'persona_fisica',
  issuer_kind: 'fnmt',
  holder_name: 'CARMEN ESPAÑOLA ESPAÑOLA',
  holder_nif: '12345678Z',
  entity_nif: null,
  entity_name: null,
  key_algorithm: 'RSA 2048',
  policy_oids: [],
};

const settings: InvoicingSettings = {
  issuer: {
    id: { Nif: '12345678Z' },
    name: 'Carmen Española Española',
    address: {
      street: 'Calle Mayor 1',
      post_code: '46001',
      town: 'València',
      province: 'Valencia',
      country: 'ES',
    },
    person: { name: 'Carmen', first_surname: 'Española', second_surname: 'Española' },
  },
  default_series: 'A',
  rectifying_series: 'R',
  simplified_series: 'S',
  language: 'Es',
  default_vat: 2100,
  default_withholding: null,
  outputs: { pdf: true, facturae: true },
  destination: { kind: 'ICloudDrive', path: '~/iCloud Drive/TaxPain' },
  rounding: 'PerRate',
  verifactu: { enabled: true, installation_id: 'TP-demo', production: false },
  footer: 'Pago por transferencia',
  activity: { code: 'A', kind: '03', iae: '0844' },
};

function inv(
  id: number,
  number: string,
  date: string,
  client: string,
  base: number,
  status: 'emitida' | 'anulada' = 'emitida',
  vf = 'correcto',
): IssuedSummary {
  const vat = Math.round(base * 0.21);
  return {
    id,
    number,
    issue_date: date,
    kind: 'F1',
    recipient_name: client,
    base,
    vat,
    total: base + vat,
    payable: base + vat,
    status,
    verifactu_status: vf,
    exported_path: null,
    has_facturae: true,
    has_ubl: false,
  };
}

const invoices: IssuedSummary[] = [
  // Venta de la furgoneta (A2026-0007, con 500 € de accesorios), su rebaja (R2026-0001) y la de
  // la impresora (A2026-0008), aún sin vincular a su baja en Inmovilizado.
  {
    ...inv(9, 'R2026-0001', '2026-10-13', 'Cliente Uno SL', -20_000, 'emitida', 'pendiente'),
    kind: 'R1',
  },
  inv(8, 'A2026-0008', '2026-10-13', 'Cliente Dos SA', 30_000, 'emitida', 'pendiente'),
  inv(7, 'A2026-0007', '2026-10-12', 'Cliente Uno SL', 1_000_000, 'emitida', 'pendiente'),
  inv(
    6,
    'A2026-0006',
    '2026-10-09',
    'Estudio Norte Arquitectura SL',
    180_000,
    'emitida',
    'pendiente',
  ),
  inv(5, 'A2026-0005', '2026-09-30', 'Panadería La Esquina SL', 45_000),
  inv(4, 'A2026-0004', '2026-09-12', 'Consultora Mediterránea SL', 120_000),
  inv(3, 'A2026-0003', '2026-08-28', 'Estudio Norte Arquitectura SL', 95_000),
  inv(2, 'A2026-0002', '2026-07-15', 'Clínica Dental Ruzafa SL', 60_000),
  inv(1, 'A2026-0001', '2026-07-02', 'Consultora Mediterránea SL', 150_000),
];

const received: ReceivedSummary[] = [
  {
    id: 3,
    filename: 'factura-coworking-octubre.pdf',
    status: 'pendiente',
    supplier_nif: 'B12345674',
    supplier_name: 'Coworking Ruzafa SL',
    number: 'CR-2026/1043',
    issue_date: '2026-10-01',
    total: 30_250,
    extraction: {
      source: 'texto',
      supplier_nif: { value: 'B12345674', confidence: 'Alta' },
      supplier_name: { value: 'Coworking Ruzafa SL', confidence: 'Media' },
      number: { value: 'CR-2026/1043', confidence: 'Media' },
      issue_date: { value: '2026-10-01', confidence: 'Media' },
      base: { value: 25_000, confidence: 'Media' },
      vat: { value: 5_250, confidence: 'Media' },
      withholding: null,
      total: { value: 30_250, confidence: 'Media' },
      warnings: [],
    },
  },
  {
    id: 2,
    filename: 'telefonia-septiembre.xml',
    status: 'revisada',
    supplier_nif: 'A22222228',
    supplier_name: 'Telecomunicaciones del Turia SA',
    number: 'T-889201',
    issue_date: '2026-09-30',
    total: 4_840,
    extraction: {
      source: 'facturae',
      supplier_nif: null,
      supplier_name: null,
      number: null,
      issue_date: null,
      base: null,
      vat: null,
      withholding: null,
      total: null,
      warnings: [],
    },
    reviewed: {
      supplier_nif: 'A22222228',
      supplier_name: 'Telecomunicaciones del Turia SA',
      number: 'T-889201',
      issue_date: '2026-09-30',
      base: 4_000,
      vat: 840,
      withholding: 0,
      total: 4_840,
      deductible_pct: 100,
      capital_good: false,
      nature: 'general',
      asset_class: null,
      vat_regime: 'interior',
      supplier_country: null,
      self_assessed_rate: null,
      intra_goods: false,
      cash_basis: false,
      import_dua: false,
      prorata_use: 'comun',
    },
    fx: null,
    registered_on: '2026-10-02',
  },
];

/** Resumen de la demo para confirmar una revisión o una corrección (sin avisos). */
function demoReceivedCheck(args: Record<string, unknown> | undefined): ReceivedCheck {
  const r = received.find((x) => x.id === args?.id);
  if (!r) throw { message: 'la factura no existe' };
  const v = args?.reviewed as Reviewed;
  const resumen = (x: Reviewed): ReceivedResumen => ({
    proveedor_nif: x.supplier_nif,
    proveedor: x.supplier_name,
    numero: x.number,
    fecha: x.issue_date,
    base: x.base,
    iva: x.vat,
    retencion: x.withholding,
    total: x.total,
    deducible_pct: x.deductible_pct,
    regimen: x.vat_regime ?? 'interior',
    moneda: null,
    total_moneda: null,
  });
  const correccion = r.status === 'revisada';
  return {
    correccion,
    despues: resumen(v),
    antes: correccion && r.reviewed ? resumen(r.reviewed) : null,
    trimestre: '4T 2026',
    avisos: correccion
      ? [
          'El 303 del 3T de 2026 ya está presentado (o la AEAT lo tiene): TaxPain no presenta nada. Si la corrección baja el IVA deducible o sube el devengado, hay que presentar una autoliquidación rectificativa de ese 303.',
        ]
      : [],
    rectificar: correccion ? [{ modelo: '303', ejercicio: 2026, trimestre: 3 }] : [],
  };
}

/** La recibida de la demo con los datos confirmados. */
function demoStoreReviewed(args: Record<string, unknown> | undefined) {
  const r = received.find((x) => x.id === args?.id);
  if (!r) throw { message: 'la factura no existe' };
  const v = args?.reviewed as Reviewed;
  Object.assign(r, {
    status: 'revisada',
    supplier_nif: v.supplier_nif,
    supplier_name: v.supplier_name,
    number: v.number,
    issue_date: v.issue_date,
    total: v.total,
    reviewed: v,
    registered_on: r.registered_on ?? '2026-11-16',
  });
}

// Inmovilizado: un portátil en uso, la furgoneta vendida con su factura (y una rectificativa
// sin pasar a la venta) y la impresora vendida sin factura (el precio sigue en la 543).
const demoAssets: FixedAsset[] = [
  {
    id: 1,
    name: 'Portátil',
    account: '217',
    depreciation_account: '2817',
    expense_account: '681',
    acquired_on: '2025-05-01',
    cost: 150_000,
    residual: 0,
    rate: 2500,
    disposed_on: null,
    method: 'lineal',
    tax_rate: null,
  },
  {
    id: 2,
    name: 'Furgoneta',
    account: '218',
    depreciation_account: '2818',
    expense_account: '681',
    acquired_on: '2023-03-01',
    cost: 2_400_000,
    residual: 0,
    rate: 1600,
    disposed_on: '2026-10-12',
    method: 'lineal',
    tax_rate: null,
  },
  {
    id: 3,
    name: 'Impresora láser',
    account: '217',
    depreciation_account: '2817',
    expense_account: '681',
    acquired_on: '2024-02-01',
    cost: 120_000,
    residual: 0,
    rate: 2500,
    disposed_on: '2026-10-05',
    method: 'lineal',
    tax_rate: null,
  },
];

function candidate(
  id: number,
  number: string,
  date: string,
  client: string,
  base: number,
  open = base,
  suggested: number | null = open,
): SaleCandidate {
  return {
    invoice_id: id,
    number,
    issue_date: date,
    operation_date: null,
    recipient_name: client,
    base,
    open,
    suggested,
    rectifies: null,
    cancels_sale: false,
  };
}

const demoSales: AssetSale[] = [
  {
    asset_id: 2,
    date: '2026-10-12',
    price: 950_000,
    invoiced: 950_000,
    unbilled: 0,
    result: -60_235,
    invoices: [
      {
        invoice_id: 7,
        number: 'A2026-0007',
        issue_date: '2026-10-12',
        amount: 950_000,
        rectifies: null,
        entry_id: 44,
      },
    ],
    pending: [
      {
        ...candidate(9, 'R2026-0001', '2026-10-13', 'Cliente Uno SL', -20_000, -20_000, null),
        rectifies: 'A2026-0007',
      },
    ],
  },
  {
    asset_id: 3,
    date: '2026-10-05',
    price: 30_000,
    invoiced: 0,
    unbilled: 30_000,
    result: -9_692,
    invoices: [],
    pending: [],
  },
];

/** Sin mayúsculas ni tildes, para buscar como la app. */
const plain = (s: string) =>
  s
    .normalize('NFD')
    .replace(/\p{Diacritic}/gu, '')
    .toLowerCase();

/** Facturas que se pueden vincular a la venta del elemento `id`; con `query`, las que la llevan. */
function demoCandidates(id: unknown, query: unknown): SaleCandidate[] {
  if (id === 2) return demoSales[0]?.pending ?? [];
  const price = id === 3 ? 30_000 : null;
  const q = typeof query === 'string' ? plain(query.trim()) : '';
  return [
    candidate(8, 'A2026-0008', '2026-10-13', 'Cliente Dos SA', 30_000),
    candidate(7, 'A2026-0007', '2026-10-12', 'Cliente Uno SL', 1_000_000, 50_000),
    candidate(6, 'A2026-0006', '2026-10-09', 'Estudio Norte Arquitectura SL', 180_000),
  ]
    .filter((c) => !q || plain(`${c.number} ${c.recipient_name ?? ''}`).includes(q))
    .map((c) => ({ ...c, suggested: price === null ? c.open : Math.min(c.open, price) }));
}

function line(account: string, debit: number, credit: number) {
  return { account, debit, credit, description: null };
}

const journal: StoredEntry[] = [
  {
    id: 1,
    number: 41,
    entry: {
      date: '2026-10-09',
      concept: 'Factura emitida A2026-0006',
      origin: { FacturaEmitida: 6 },
      lines: [line('4300003', 217_800, 0), line('705', 0, 180_000), line('477', 0, 37_800)],
    },
  },
  {
    id: 2,
    number: 42,
    entry: {
      date: '2026-10-10',
      concept: 'Cobro factura A2026-0005 · TRANSFERENCIA PANADERIA LA ESQUINA',
      origin: { Banco: 12 },
      lines: [line('5720001', 54_450, 0), line('4300002', 0, 54_450)],
    },
  },
  {
    id: 3,
    number: 43,
    entry: {
      date: '2026-10-12',
      concept: 'Factura recibida T-889201 Telecomunicaciones del Turia SA',
      origin: { FacturaRecibida: 2 },
      lines: [line('629', 4_000, 0), line('472', 840, 0), line('4100002', 0, 4_840)],
    },
  },
];

const trial: AccountBalance[] = [
  { account: '410', debit: 0, credit: 180_290 },
  { account: '430', debit: 786_500, credit: 500_700 },
  { account: '472', debit: 31_290, credit: 0 },
  { account: '477', debit: 0, credit: 136_500 },
  { account: '572', debit: 500_700, credit: 0 },
  { account: '621', debit: 72_000, credit: 0 },
  { account: '629', debit: 77_000, credit: 0 },
  { account: '705', debit: 0, credit: 650_000 },
];

function sl(id: string, label: string, amount: number, leaf = true) {
  return { id, label, amount, leaf };
}

const annual: AnnualAccounts = {
  balanced: true,
  income: {
    unmapped: [],
    lines: [
      sl('1', '1. Importe neto de la cifra de negocios.', 650_000),
      sl('7', '7. Otros gastos de explotación.', -149_000),
      sl('A', 'A) RESULTADO DE EXPLOTACIÓN', 501_000, false),
      sl('C', 'C) RESULTADO ANTES DE IMPUESTOS (A + B)', 501_000, false),
      sl('D', 'D) RESULTADO DEL EJERCICIO (C + 17)', 501_000, false),
    ],
  },
  balance: {
    unmapped: [],
    lines: [
      sl('B', 'B) ACTIVO CORRIENTE', 817_790, false),
      sl('B.II.1', '1. Clientes por ventas y prestaciones de servicios.', 285_800),
      sl('B.II.3', '3. Otros deudores.', 31_290),
      sl('B.VI', 'VI. Efectivo y otros activos líquidos equivalentes.', 500_700),
      sl('TA', 'TOTAL ACTIVO (A + B).', 817_790, false),
      sl('PA', 'A) PATRIMONIO NETO', 501_000, false),
      sl('PA-1.VII', 'VII. Resultado del ejercicio.', 501_000),
      sl('PC', 'C) PASIVO CORRIENTE', 316_790, false),
      sl('PC.IV.2', '2. Otros acreedores.', 316_790),
      sl('TP', 'TOTAL PATRIMONIO NETO Y PASIVO (A + B + C)', 817_790, false),
    ],
  },
};

const bank: BankTx[] = [
  {
    id: 14,
    bank_account: 'Cuenta profesional',
    op_date: '2026-10-11',
    amount: 217_800,
    description: 'TRANSFERENCIA ESTUDIO NORTE ARQUITECTURA SL FRA A2026-0006',
    matched: null,
  },
  {
    id: 13,
    bank_account: 'Cuenta profesional',
    op_date: '2026-10-05',
    amount: -4_840,
    description: 'RECIBO TELECOMUNICACIONES DEL TURIA',
    matched: null,
  },
  {
    id: 12,
    bank_account: 'Cuenta profesional',
    op_date: '2026-10-02',
    amount: -1_200,
    description: 'COMISION MANTENIMIENTO CUENTA',
    matched: null,
  },
];

// 303 del 3T con las emitidas de julio a septiembre (4.700 € al 21 %) y las recibidas
// registradas en el trimestre (alquiler 240 € y otros servicios 300 €, al 21 %).
/** 202 por la base (art. 40.3 LIS) de una sociedad inventada: nueve meses con 60.000 € de base. */
const draft202: ModelDraft = {
  model: '202',
  year: 2026,
  quarter: 2,
  boxes: [
    [4, 6_000_000],
    [13, 6_000_000],
    [16, 6_000_000],
    [17, 1_700],
    [18, 1_020_000],
    [28, 20_000],
    [29, 10_000],
    [30, 250_000],
    [32, 750_000],
    [34, 750_000],
  ],
  payees: [],
  details: [],
  declaration: 'I',
  result: 750_000,
  warnings: [
    'Modalidad del art. 40.3 LIS: 17 % de la base imponible de los 9 primeros meses del período impositivo, menos retenciones y pagos anteriores',
  ],
  file_name: 'B12345674_2026_2P_202.txt',
  file: '',
};

function simulacion202(year: number): Simulacion202 {
  return {
    ejercicio: year,
    modalidad: {
      ejercicio: year,
      elegida: null,
      efectiva: 'Cuota',
      obligatoria: false,
      cifra_negocios: 180_000_000,
      aviso: null,
    },
    periodos: [
      {
        period: 1,
        hasta: `${year}-03-31`,
        cuota: 540_000,
        base_imponible: 1_500_000,
        base: 255_000,
        fuente: 'contabilidad',
      },
      {
        period: 2,
        hasta: `${year}-09-30`,
        cuota: 540_000,
        base_imponible: 6_000_000,
        base: 750_000,
        fuente: 'contabilidad',
      },
      {
        period: 3,
        hasta: `${year}-11-30`,
        cuota: 540_000,
        base_imponible: 6_800_000,
        base: 151_000,
        fuente: 'ano_anterior',
      },
    ],
    total_cuota: 1_620_000,
    total_base: 1_156_000,
    opcion_desde: `${year}-02-01`,
    opcion_hasta: `${year}-02-28`,
    conclusion:
      'Por la base pagarías 4.640,00 € menos en el año (11.560,00 € frente a 16.200,00 €). Para aplicarla hay que optar en el censo (modelo 036) en febrero; la opción vale para este ejercicio y los siguientes hasta que renuncies.',
    notas: [
      'Los pagos fraccionados son a cuenta: lo pagado de más o de menos se regulariza en el 200 del ejercicio. La diferencia entre modalidades es de tesorería, no de impuesto.',
    ],
  };
}

const draft303: ModelDraft = {
  model: '303',
  year: 2026,
  quarter: 3,
  boxes: [
    [7, 470_000],
    [9, 98_700],
    [27, 98_700],
    [28, 54_000],
    [29, 11_340],
    [45, 11_340],
    [46, 87_360],
    [64, 87_360],
    [65, 10_000],
    [66, 87_360],
    [69, 87_360],
    [71, 87_360],
  ],
  payees: [],
  details: [],
  declaration: 'I',
  result: 87_360,
  warnings: [],
  file_name: '12345678Z_2026_3T_303.txt',
  file: '',
};

/** El 303 del 3T de 2026 que la demo da por presentado (datos inventados): una venta de
 * 350 € de base que no estaba entonces. */
const demo303Presentado: [number, number][] = [
  [7, 435_000],
  [9, 91_350],
  [27, 91_350],
  [28, 54_000],
  [29, 11_340],
  [45, 11_340],
  [46, 80_010],
  [64, 80_010],
  [65, 10_000],
  [66, 80_010],
  [69, 80_010],
  [71, 80_010],
];

const demoAnterior: RectificativaAnterior = {
  fuente: 'aeat',
  justificante: '3030000000017',
  presentada: '2026-10-20',
  declaracion: 'I',
  casillas: demo303Presentado,
  resultado: 80_010,
};

/** Meses completos entre el último día del plazo y la presentación (art. 27.2 LGT). */
function demoMesesCompletos(plazo: string, fecha: string): number {
  const [py = 0, pm = 0, pd = 0] = plazo.split('-').map(Number);
  const [fy = 0, fm = 0, fd = 0] = fecha.split('-').map(Number);
  let m = (fy - py) * 12 + (fm - pm);
  if (fd < pd) m -= 1;
  return Math.max(0, m);
}

function demoRectificativa(args: Record<string, unknown> | undefined): Rectificativa {
  if (args?.model !== '303') throw { message: 'demo: sólo el 303 del 3T de 2026' };
  const o = (args?.opciones ?? {}) as { fecha?: string | null; justificante?: string | null };
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  const fecha = o.fecha || `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
  const plazo = '2026-10-20';
  const prev = new Map(demo303Presentado);
  const ahora = new Map(draft303.boxes);
  const a_ingresar = (ahora.get(71) ?? 0) - (prev.get(71) ?? 0);
  ahora.set(70, prev.get(71) ?? 0);
  ahora.set(71, a_ingresar);
  const claves = [...new Set([...prev.keys(), ...ahora.keys()])].sort((x, y) => x - y);
  const filas = claves.map((k) => ({
    casilla: k,
    presentado: prev.get(k) ?? 0,
    ahora: ahora.get(k) ?? 0,
    diferencia: (ahora.get(k) ?? 0) - (prev.get(k) ?? 0),
  }));
  const tarde = fecha > plazo;
  const meses = demoMesesCompletos(plazo, fecha);
  const porcentaje = Math.min(1 + meses, 15);
  const importe = Math.round((a_ingresar * porcentaje) / 100);
  const justificante = o.justificante || demoAnterior.justificante;
  return {
    model: '303',
    year: 2026,
    quarter: 3,
    period: '3T',
    cauce: 'rectificativa',
    cauce_texto: `Autoliquidación rectificativa: sale ${euros(a_ingresar)} más a ingresar (art. 120.3 LGT y RD 117/2024).`,
    anterior: demoAnterior,
    justificante,
    filas,
    ingresado_antes: prev.get(71) ?? 0,
    devuelto_antes: 0,
    total: draft303.result,
    resultado: a_ingresar,
    a_ingresar,
    a_devolver: 0,
    a_compensar: 0,
    plazo,
    fecha,
    recargo: tarde
      ? {
          porcentaje,
          importe,
          meses,
          intereses: 0,
          intereses_desde: null,
          reducido: importe - Math.round(importe / 4),
        }
      : null,
    intereses_devolucion: 0,
    total_a_pagar: a_ingresar + (tarde ? importe : 0),
    declaracion: 'I',
    avisos: tarde
      ? [
          `Fuera de plazo y sin requerimiento: recargo del ${porcentaje} % (${euros(importe)}) (art. 27.2 LGT). No va en el fichero: lo liquida después la AEAT.`,
        ]
      : [],
    file_name: 'B12345674_2026_3T_303_rectificativa.txt',
    file: `<T303020263T0000> demo ${justificante ?? ''}`,
    calendario: [
      'Desde el 1 de abril de 2025 (el diseño de registro la trae desde el 3T de 2024)',
      'Art. 120.3 LGT (Ley 13/2023), RD 117/2024 y la orden del 303 (DR303e24v200). TODO(VERIFY)',
    ],
  };
}

/** Borradores anuales con retenciones de la demo (perceptores inventados). */
const draftAnnual = (model: string, details: [string, number][], result: number): ModelDraft => ({
  model,
  year: 2026,
  quarter: 0,
  boxes: [],
  payees: [],
  details,
  declaration: '',
  result,
  warnings: [],
  file_name: `12345678Z_2026_0A_${model}.txt`,
  file: '',
});

const DEMO_ANNUAL: Record<string, ModelDraft> = {
  '190': draftAnnual('190', [['Perceptor Demo Uno · 00000000T · clave G.01', 30_000]], 30_000),
  '180': draftAnnual('180', [['Inmuebles Ejemplo SA · A22222228', 228_000]], 228_000),
  '193': draftAnnual('193', [], 0),
  '216': draft216,
  '296': draft296,
};

const DEMO_CERT_PAYEES: Record<
  string,
  {
    nif: string;
    name: string;
    concept: string;
    gross: number;
    withheld: number;
    email: string | null;
  }[]
> = {
  '190': [
    {
      nif: '00000000T',
      name: 'Perceptor Demo Uno',
      concept: 'clave G.01',
      gross: 200_000,
      withheld: 30_000,
      email: 'perceptor@demo.example',
    },
  ],
  '180': [
    {
      nif: 'A22222228',
      name: 'Inmuebles Ejemplo SA',
      concept: 'arrendamiento',
      gross: 1_200_000,
      withheld: 228_000,
      email: null,
    },
  ],
  '193': [],
  '296': [
    {
      nif: 'FR00000000001',
      name: 'Licencias Exemple SARL',
      concept: 'cánones',
      gross: 200_000,
      withheld: 10_000,
      email: null,
    },
  ],
};

const one = {
  seq: 1,
  installments: 1,
  party_account: null,
  method: null,
  treasury: null,
  fixed: false,
};
const recibo = { method: 'recibo', treasury: '5720001', installments: 2, fixed: false };

/** Revisión de la carpeta de demostración (todo inventado). */
/**
 * Lo recuperable de la demo, en céntimos: lo que suman los hallazgos de la gestoría con importe
 * (los mismos en la revisión de la carpeta y en «Qué revisar»).
 */
const DEMO_RECUPERABLE = 218_500 + 174_800 + 23_840 + 41_400;

const REVISION: Revision = {
  generada: TODAY,
  // Una sociedad, no la autónoma de la demo: los hallazgos son del Impuesto sobre Sociedades
  // (modelos 200 y 202) y el NIF es el de lo comprobado en la AEAT.
  empresa: 'Estudio Demo SL',
  nif: 'B12345674',
  carpeta: '~/Documents/Gestoría',
  resumen: {
    ficheros: 64,
    con_detalle: 61,
    sin_leer: 1,
    no_admitidos: 2,
    por_tipo: [
      { tipo: 'Facturas recibidas', n: 38 },
      { tipo: 'Facturas emitidas', n: 14 },
      { tipo: 'Modelos presentados', n: 6 },
      { tipo: 'Libros de facturas', n: 2 },
      { tipo: 'Documentos de la empresa', n: 1 },
    ],
    // Los dos errores y dos de los avisos son de la gestoría y llevan importe: su suma es
    // lo recuperable (2.185,00 + 1.748,00 + 238,40 + 414,00).
    errores: 2,
    avisos: 8,
    info: 3,
    // El urgente: el 111 del 2T sin cargo en el banco (1.000,00 € más el 5 % de hoy).
    urgentes: 1,
    en_juego_urgente: 105_000,
    recuperable: DEMO_RECUPERABLE,
    a_favor_proximo_200: 0,
    retenciones_proximo_200: 0,
    exencion_proximo_200: 0,
    impuesto_dividendos_proximo_200: 0,
  },
  hallazgos: [
    {
      codigo: 'cruces.pago_no_esta',
      area: 'bancos',
      gravedad: 'aviso',
      responsable: 'empresa',
      ejercicio: 2026,
      titulo: 'No se ve el pago del 111 del 2T 2026 en el banco',
      detalle:
        'Sale a ingresar 1.000,00 € y en los extractos no hay ningún cargo de ese importe entre el 01/07/2026 y el 30/07/2026. Puede haberse pagado desde otra cuenta que no está en la carpeta, con un aplazamiento o no haberse pagado. Qué hacer: busca el justificante del pago (NRC) y, si no se pagó, pide a la gestoría que lo regularice cuanto antes.',
      ficheros: ['2026/Modelos/111_2T_2026.pdf'],
      importe: null,
      rectificable_hasta: null,
      urgente: {
        en_juego: 100_000,
        vencio: '2026-07-20',
        porcentaje_hoy: 5,
        recargo_hoy: 5_000,
        siguiente_desde: null,
        siguiente_porcentaje: 10,
        siguiente_recargo: 10_000,
        nota: 'Si se presentó sin ingresar, la deuda está en periodo ejecutivo desde el 21/07/2026 (art. 161 LGT): recargo del 5 % si se paga entera antes de que notifiquen la providencia de apremio, del 10 % si se paga en el plazo que da la providencia y del 20 % más intereses de demora después (art. 28 LGT). TaxPain no sabe si ya se ha notificado la providencia: mira las notificaciones de la Sede. Si se pagó desde otra cuenta o está aplazado, no hay deuda.',
      },
    },
    {
      codigo: 'sociedades.exencion_dividendos',
      area: 'sociedades',
      gravedad: 'error',
      responsable: 'gestoria',
      ejercicio: 2024,
      titulo: 'Dividendos de 2024 sin la exención del art. 21 LIS',
      detalle:
        'En el 200 de 2024 hay 10.000,00 € de dividendos de participaciones en empresas del grupo y asociadas y la exención (casilla [00370]) está vacía: tributaron enteros. Con al menos el 5 % de la participada durante un año, el 95 % está exento (art. 21.1 y 21.10 LIS): 9.500,00 € que no debían tributar. Rehecha la liquidación con la exención, la cuota baja unos 2.185,00 € (estimación). Hasta el 25/07/2029 (art. 66 LGT) se puede presentar una autoliquidación rectificativa del 200 (art. 120.4 LGT y art. 59 bis RIS).',
      ficheros: ['2024/Modelos/200_2024.pdf'],
      importe: 218_500,
      rectificable_hasta: '2029-07-25',
    },
    {
      codigo: 'sociedades.exencion_dividendos',
      area: 'sociedades',
      gravedad: 'error',
      responsable: 'gestoria',
      ejercicio: 2023,
      titulo: 'Dividendos de 2023 sin la exención del art. 21 LIS',
      detalle:
        'En el 200 de 2023 hay 8.000,00 € de dividendos de participaciones en empresas del grupo y asociadas y la exención (casilla [00370]) está vacía: tributaron enteros. Con al menos el 5 % de la participada durante un año, el 95 % está exento (art. 21.1 y 21.10 LIS): 7.600,00 € que no debían tributar. Rehecha la liquidación con la exención, la cuota baja unos 1.748,00 € (estimación). Hasta el 25/07/2028 (art. 66 LGT) se puede presentar una solicitud de rectificación (art. 120.3 LGT).',
      ficheros: ['2023/Modelos/200_2023.pdf'],
      importe: 174_800,
      rectificable_hasta: '2028-07-25',
    },
    {
      codigo: 'ficheros.copias',
      area: 'ficheros',
      gravedad: 'aviso',
      responsable: 'empresa',
      ejercicio: null,
      titulo: '1 copia de un fichero con el mismo contenido',
      detalle:
        'Hay 1 grupo de ficheros idénticos: al importar o al pasarlos a la gestoría se pueden contar dos veces (dos gastos, dos deducciones de IVA). Deja una sola copia de cada uno.',
      ficheros: [
        '2025/Facturas recibidas/Coworking Ruzafa 2025-11.pdf',
        '2025/Facturas recibidas/Coworking Ruzafa 2025-11 (1).pdf',
      ],
      importe: null,
    },
    {
      codigo: 'facturas.nif_control',
      area: 'facturas',
      gravedad: 'aviso',
      responsable: 'empresa',
      ejercicio: null,
      titulo: '1 NIF con la letra de control mal',
      detalle:
        'Un NIF mal escrito invalida la factura a efectos de la deducción del IVA (art. 6 del Reglamento de facturación) y hace que el 347 o el 349 no casen. Pide al emisor una factura corregida.',
      ficheros: ['2025/Facturas recibidas/Imprenta Turia 2025-06.pdf'],
      importe: null,
    },
    {
      codigo: 'emitidas.saltos',
      area: 'emitidas',
      gravedad: 'aviso',
      responsable: 'empresa',
      ejercicio: 2025,
      titulo: 'Saltos en la numeración de la serie «2025-»',
      detalle:
        'Faltan los números 2025-014. Si se anularon, guarda la factura anulada o una nota que lo explique: la numeración tiene que ser correlativa (art. 6.1.a del Reglamento de facturación).',
      ficheros: [],
      importe: null,
    },
    {
      codigo: 'libros.sin_pdf',
      area: 'libros',
      gravedad: 'aviso',
      responsable: 'documentacion',
      ejercicio: 2025,
      titulo: '2 facturas del libro de recibidas sin PDF en la carpeta',
      detalle:
        'El libro de recibidas de la gestoría tiene 2 facturas que no están en la carpeta: TT-2025-0912 (Telecomunicaciones del Turia SA), CR-2025-288 (Coworking Ruzafa SL). Pide una copia a la gestoría o búscalas en el correo.',
      ficheros: ['2025/Libro facturas recibidas 2025.xlsx'],
      importe: null,
    },
    {
      codigo: 'modelos.303_falta_factura',
      area: 'modelos',
      gravedad: 'aviso',
      responsable: 'documentacion',
      ejercicio: 2025,
      titulo: 'El 303 del 4T 2025 deduce más de lo que suman las facturas',
      detalle:
        'Declara 2.480,00 € de base de IVA soportado (520,80 € de cuota) y las facturas con IVA español de ese trimestre en la carpeta suman 2.167,60 € (455,20 €): faltan 312,40 € de base (65,60 € de cuota). Falta alguna factura en la carpeta, o el modelo deduce algo que no tiene factura.',
      ficheros: ['2025/Modelos/303_4T_2025.pdf'],
      importe: null,
    },
    {
      codigo: 'cruces.303_saldo_perdido',
      area: 'modelos',
      gravedad: 'aviso',
      responsable: 'gestoria',
      ejercicio: 2025,
      titulo: 'Se ha dejado de arrastrar IVA a compensar en el 303 del 2T 2025',
      detalle:
        'El 303 del 1T 2025 dejó 238,40 € a compensar en periodos siguientes, pero el 303 del 2T 2025 sólo arrastra 0,00 € en [110]: faltan 238,40 € a favor de la empresa. Qué hacer: pide a la gestoría que lo incluya en el siguiente 303 (hay cuatro años para compensarlo, art. 99.Cinco LIVA) o que pida la rectificación.',
      ficheros: ['2025/Modelos/303_2T_2025.pdf'],
      importe: 23_840,
      rectificable_hasta: '2029-07-21',
    },
    {
      codigo: 'cruces.200_202_sin_descontar',
      area: 'sociedades',
      gravedad: 'aviso',
      responsable: 'gestoria',
      ejercicio: 2024,
      titulo: 'El 200 de 2024 no descuenta todo el pago fraccionado del 2P',
      detalle:
        'El 202 del 2P 2024 ingresó 414,00 € y el 200 sólo descuenta 0,00 € en [00603]: 414,00 € pagados dos veces. Qué hacer: pide a la gestoría la rectificación del 200 para recuperarlo.',
      ficheros: ['2024/Modelos/200_2024.pdf'],
      importe: 41_400,
      rectificable_hasta: '2029-07-25',
    },
    {
      codigo: 'ficheros.no_admitidos',
      area: 'ficheros',
      gravedad: 'info',
      responsable: 'empresa',
      ejercicio: null,
      titulo: '2 ficheros en un formato que no se lee',
      detalle:
        'No se han leído porque TaxPain no entiende su formato: 2025/Presupuesto web.pages, 2025/fotos-ticket.zip.',
      ficheros: ['2025/Presupuesto web.pages', '2025/fotos-ticket.zip'],
      importe: null,
    },
    {
      codigo: 'ficheros.sin_leer',
      area: 'ficheros',
      gravedad: 'info',
      responsable: 'empresa',
      ejercicio: null,
      titulo: '1 fichero sin leer con detalle',
      detalle:
        'No se sabe qué son: Escrituras constitución.pdf. Para una revisión completa, léelos con IA (Importar → Carpeta de la empresa → «Leer con IA») o con Claude Code (/leer-carpeta) y vuelve a revisar.',
      ficheros: ['Escrituras constitución.pdf'],
      importe: null,
    },
    {
      codigo: 'facturas.divisas',
      area: 'facturas',
      gravedad: 'info',
      responsable: 'empresa',
      ejercicio: null,
      titulo: '3 facturas en otra moneda',
      detalle:
        '3 en USD. Se registran en euros con el tipo de cambio del devengo (art. 79.Once LIVA); la diferencia con lo que cobra el banco es una diferencia de cambio. En esta revisión no se suman con las de euros.',
      ficheros: [
        '2025/Facturas recibidas/hosting-2025-02.pdf',
        '2025/Facturas recibidas/hosting-2025-05.pdf',
        '2025/Facturas recibidas/hosting-2025-08.pdf',
      ],
      importe: null,
    },
  ],
  tablas: [
    {
      titulo: 'Ficheros por tipo',
      columnas: ['Tipo', 'Ficheros'],
      filas: [
        ['Facturas recibidas', '38'],
        ['Facturas emitidas', '14'],
        ['Modelos presentados', '6'],
        ['Libros de facturas', '2'],
        ['Documentos de la empresa', '1'],
      ],
    },
    {
      titulo: '303 frente a las facturas de la carpeta',
      columnas: ['Periodo', 'Concepto', 'Modelo', 'Facturas', 'Diferencia'],
      filas: [
        [
          '4T 2025',
          'IVA soportado interior: base [28]+[30]',
          '2.480,00 €',
          '2.167,60 €',
          '312,40 €',
        ],
        ['4T 2025', 'IVA soportado interior: cuota [29]+[31]', '520,80 €', '455,20 €', '65,60 €'],
        ['4T 2025', 'IVA devengado: base [01]+[04]+[07]', '9.850,00 €', '9.850,00 €', '0,00 €'],
      ],
    },
  ],
  pedir: [
    {
      a_quien: 'gestoria',
      que: 'Rectificar el Impuesto sobre Sociedades de 2024, con una autoliquidación rectificativa del 200 (art. 120.4 LGT y art. 59 bis RIS), para aplicar la exención del art. 21 LIS a los dividendos de las participadas (unos 2.185,00 € a devolver)',
    },
    {
      a_quien: 'gestoria',
      que: 'Rectificar el Impuesto sobre Sociedades de 2023, con una solicitud de rectificación (art. 120.3 LGT), para aplicar la exención del art. 21 LIS a los dividendos de las participadas (unos 1.748,00 € a devolver)',
    },
    {
      a_quien: 'gestoria',
      que: 'La factura (o las facturas) que explica los 312,40 € de base deducidos de más en el 303 del 4T 2025',
    },
    {
      a_quien: 'gestoria',
      que: 'Copia de las facturas del libro de recibidas de 2025 que no están en la carpeta: TT-2025-0912, CR-2025-288',
    },
    {
      a_quien: 'empresa',
      que: 'Una factura corregida de Imprenta Turia (el NIF tiene la letra de control mal)',
    },
  ],
};

/** Comprobación de lo presentado de 2025 en la demo (datos inventados). */
const DEMO_AEAT_CHECK: AeatCheckReport = {
  year: 2025,
  nif: 'B12345674',
  checked_at: '2026-10-20T10:00:00Z',
  without_profile: false,
  rows: [
    {
      model: '303',
      period: '1T',
      status: 'Presented',
      expected: true,
      due_on: '2025-04-21',
      presented_at: '2025-04-15T10:00:00',
      original_presented_at: null,
      late: false,
      late_amendment: false,
      receipt: '3030000000001',
      replaces: [],
      has_pdf: true,
      boxes_read: true,
      mismatches: 0,
      filed_model_id: 1,
      note: null,
    },
    {
      model: '303',
      period: '2T',
      status: 'Presented',
      expected: true,
      due_on: '2025-07-21',
      presented_at: '2025-09-01T10:00:00',
      original_presented_at: '2025-07-15T10:00:00',
      late: false,
      late_amendment: true,
      receipt: '3030000000003',
      replaces: ['3030000000002'],
      has_pdf: true,
      boxes_read: true,
      mismatches: 2,
      filed_model_id: 2,
      note: null,
    },
    {
      model: '303',
      period: '3T',
      status: 'Missing',
      expected: true,
      due_on: '2025-10-20',
      presented_at: null,
      original_presented_at: null,
      late: false,
      late_amendment: false,
      receipt: null,
      replaces: [],
      has_pdf: false,
      boxes_read: false,
      mismatches: null,
      filed_model_id: null,
      note: null,
    },
    {
      model: '111',
      period: '2T',
      status: 'NotFound',
      expected: true,
      due_on: '2025-07-21',
      presented_at: null,
      original_presented_at: null,
      late: false,
      late_amendment: false,
      receipt: null,
      replaces: [],
      has_pdf: false,
      boxes_read: false,
      mismatches: null,
      filed_model_id: null,
      note: 'Sólo hacía falta si hubo pagos con retención (nóminas, profesionales, administradores…) en el periodo.',
    },
  ],
  none_filed: ['349', '347'],
  errors: [],
  empty_answers: [
    { text: 'Usted no ha presentado la declaración solicitada', models: ['111', '349', '347'] },
  ],
  unavailable: ['721'],
};

/** Una fila de la evolución de la demo: valores de 2023 a 2025 (céntimos o número). */
function demoFila(
  id: string,
  titulo: string,
  ayuda: string,
  fuente: string,
  vals: (number | null)[],
  recuento = false,
) {
  const años = [2023, 2024, 2025];
  return {
    id,
    titulo,
    ayuda,
    recuento,
    valores: vals.map((valor, i) => {
      const antes = i > 0 ? vals[i - 1] : null;
      const diferencia =
        valor !== null && antes !== null && antes !== undefined ? valor - antes : null;
      return {
        año: años[i] ?? 2025,
        valor,
        fuente: valor === null ? null : fuente,
        diferencia,
        variacion:
          diferencia !== null && antes ? Math.round((diferencia * 1000) / Math.abs(antes)) : null,
      };
    }),
  };
}

/** Entidad patrimonial y patrimonio neto de Estudio Demo SL (importes inventados). */
const demoPatMarcas: Record<string, { clase: ClasePatrimonial; motivo: string }> = {};
let demoPatAjustes: AjustesPatrimonial = {
  tesoreria_afecta: 2_000_000,
  tesoreria_motivo: 'Tres meses de gastos de la actividad',
  beneficios_actividad: null,
  beneficios_motivo: '',
  prestamos_participativos: 0,
  dominante: false,
};
let demoPatVentas: VentaPatrimonial[] = [
  { id: 1, fecha: '2026-05-15', importe: 1_500_000, motivo: 'Venta de la furgoneta de reparto' },
];
function demoPatrimonial(year: number): InformePatrimonial {
  const fechas = [`${year}-03-31`, `${year}-06-30`, `${year}-09-30`, `${year}-12-31`];
  const base: [string, string, ClasePatrimonial, number[]][] = [
    [
      '2110001',
      'Local de la calle Mayor',
      'afecto',
      [12_000_000, 12_000_000, 12_000_000, 12_000_000],
    ],
    ['2110002', 'Apartamento en la playa', 'afecto', [9_000_000, 9_000_000, 9_000_000, 9_000_000]],
    [
      '2810001',
      'Amortización acumulada de construcciones',
      'afecto',
      [-2_100_000, -2_150_000, -2_200_000, -2_250_000],
    ],
    [
      '2403001',
      'Participaciones en Demo Servicios SL',
      'valores',
      [3_000_000, 3_000_000, 3_000_000, 3_000_000],
    ],
    ['5400001', 'Fondo de inversión', 'valores', [2_500_000, 2_500_000, 4_000_000, 4_000_000]],
    ['430', 'Clientes', 'afecto', [1_800_000, 2_200_000, 1_600_000, 2_000_000]],
    ['5720001', 'Banco', 'tesoreria', [4_500_000, 6_500_000, 5_000_000, 5_500_000]],
  ];
  const cerrados = fechas.map((f) => f < '2026-11-16');
  const partidas = base.map(([cuenta, nombre, propuesta, saldos]) => {
    const m = demoPatMarcas[cuenta];
    return {
      cuenta,
      nombre,
      clase: m?.clase ?? propuesta,
      propuesta,
      marcada: !!m,
      motivo: m?.motivo ?? '',
      desde: m ? year : null,
      correctora: cuenta.startsWith('28'),
      saldos,
    };
  });
  const pct = (a: number, b: number) => (b > 0 ? Math.floor((a * 10_000) / b) : 0);
  const trimestres = fechas.map((fecha, i) => {
    const sum = (c: ClasePatrimonial) =>
      partidas.filter((p) => p.clase === c).reduce((t, p) => t + (p.saldos[i] ?? 0), 0);
    const activo = partidas.reduce((t, p) => t + (p.saldos[i] ?? 0), 0);
    const tesoreria = sum('tesoreria');
    const justificada = Math.min(demoPatAjustes.tesoreria_afecta, Math.max(tesoreria, 0));
    const vendido = demoPatVentas
      .filter((v) => v.fecha <= fecha)
      .reduce((t, v) => t + v.importe, 0);
    const ventas = Math.min(vendido, tesoreria - justificada);
    const no_afecto = sum('no_afecto') + tesoreria - justificada - ventas;
    const valores = sum('valores');
    const computa = valores + no_afecto;
    const lip = Math.max(computa - (demoPatAjustes.beneficios_actividad ?? 0), 0);
    return {
      fecha,
      cerrado: cerrados[i] ?? true,
      activo,
      afecto: sum('afecto'),
      valores,
      no_afecto,
      tesoreria,
      participaciones: sum('participacion'),
      tesoreria_justificada: justificada,
      ventas_excluidas: ventas,
      tesoreria_no_justificada: tesoreria - justificada - ventas,
      computa,
      pct: pct(computa, activo),
      computa_lip: lip,
      pct_lip: pct(lip, activo),
    };
  });
  const usados = trimestres.filter((t) => t.cerrado);
  const avg = (f: (t: (typeof trimestres)[number]) => number) =>
    Math.trunc(usados.reduce((a, t) => a + f(t), 0) / Math.max(usados.length, 1));
  const activo = avg((t) => t.activo);
  const computa = avg((t) => t.computa);
  const participaciones = avg((t) => t.participaciones);
  const capital = 300_000;
  const pnFechas = fechas.map((fecha, i) => {
    const contable = [12_000_000, 12_400_000, 12_900_000, 13_300_000][i] ?? 0;
    const mercantil = contable + demoPatAjustes.prestamos_participativos;
    return {
      fecha,
      cerrado: cerrados[i] ?? true,
      capital,
      contable,
      capital_ajustes: 0,
      coberturas: 0,
      prestamos_participativos: demoPatAjustes.prestamos_participativos,
      mercantil,
      pct_capital: pct(mercantil, capital),
    };
  });
  return {
    year,
    fechas,
    partidas,
    trimestres,
    media: {
      trimestres: usados.length,
      activo,
      valores: avg((t) => t.valores),
      no_afecto: avg((t) => t.no_afecto),
      participaciones,
      computa,
      pct: pct(computa, activo),
      pct_con_participaciones: pct(computa + participaciones, activo),
      pct_sin_tesoreria: pct(computa - avg((t) => t.tesoreria_no_justificada), activo),
      margen: Math.trunc(activo / 2) - computa,
    },
    patrimonial: pct(computa, activo) > 5_000,
    provisional: usados.length === 0,
    trimestres_lip: trimestres.filter((t) => t.cerrado && t.pct_lip > 5_000).length,
    ajustes: demoPatAjustes,
    ventas: demoPatVentas,
    beneficios_propuestos: 9_500_000,
    consecuencias: [
      {
        titulo: 'Tipo de gravamen',
        texto:
          'No se aplica el tipo de las entidades de nueva creación ni el reducido de las empresas con cifra de negocios inferior a 1 millón de euros: tributa al tipo general del 25 %.',
        norma: 'Art. 29.1 LIS',
      },
      {
        titulo: 'Incentivos de empresa de reducida dimensión',
        texto:
          'No se aplican los incentivos de las empresas de reducida dimensión ni la reserva de nivelación.',
        norma: 'Arts. 101 a 105 LIS',
      },
      {
        titulo: 'Bases imponibles negativas',
        texto:
          'Si alguien adquiere la mayoría del capital después de generarse las bases negativas y la entidad es patrimonial, no se pueden compensar.',
        norma: 'Art. 26.4 LIS',
      },
    ],
    que_revisar: [
      {
        titulo: 'Inmuebles',
        texto:
          'Un inmueble alquilado sólo es actividad económica si para el arrendamiento hay al menos una persona empleada con contrato laboral y a jornada completa.',
        norma: 'Art. 5.1 LIS',
      },
      {
        titulo: 'Participaciones',
        texto:
          'No computan si dan al menos el 5 % del capital, se tienen desde hace un año y se gestionan con medios materiales y personales.',
        norma: 'Art. 5.2.d LIS',
      },
    ],
    avisos: [],
    patrimonio: {
      forma: 'sl',
      fechas: pnFechas,
      anterior: null,
      avisos: [],
      notas: [
        {
          titulo: 'Qué patrimonio neto se usa',
          texto:
            'El de las cuentas anuales, más el capital no exigido y el registrado como pasivo, sin los ajustes por coberturas, más los préstamos participativos.',
          norma: 'Art. 36.1.c CCom',
        },
      ],
    },
  };
}

/** «Ahorrar impuestos» de la autónoma de demostración (las marcas viven en memoria). */
const demoMarcas: Record<string, MarcaAhorro> = {};
function demoAhorro(): Ahorro {
  const ficha = (r: Omit<Recomendacion, 'marca'>): Recomendacion => ({
    ...r,
    marca: demoMarcas[r.id] ?? { estado: 'pendiente', pasos: [] },
  });
  const recomendaciones = [
    ficha({
      id: 'plan_pensiones_autonomo',
      area: 'personas',
      titulo: 'Planes de pensiones: rebaja la Renta',
      resumen:
        'Lo aportado a planes de pensiones se resta de la base de la Renta: 1.500 € al año en planes individuales y hasta 4.250 € más en planes de empleo simplificados para autónomos.',
      encaje: 'conviene',
      motivo:
        'Con un rendimiento de unos 38.400,00 €, aportar 5.750,00 € a planes de pensiones antes del 31 de diciembre rebaja tu Renta de este año.',
      ahorro: 212_750,
      aplazado: false,
      calculo: '5.750,00 € por tu tipo marginal aproximado (37 %)',
      plazo: '2026-12-31',
      requisitos: ['Aportar antes del 31 de diciembre.'],
      pasos: [
        'Pide a tu banco o gestora un plan individual y un plan de empleo simplificado para autónomos.',
        'Aporta antes del 31 de diciembre.',
        'Guarda el certificado de aportaciones.',
      ],
      riesgos: ['Al rescatarlo tributa como rendimiento del trabajo.'],
      norma: 'Arts. 51 y 52 de la Ley 35/2006 del IRPF',
      enlace: 'https://www.boe.es/buscar/act.php?id=BOE-A-2006-20764#a52',
      pantalla: null,
    }),
    ficha({
      id: 'cripto_perdidas',
      area: 'cripto',
      titulo: 'Criptomonedas: compensa ganancias con pérdidas antes del 31 de diciembre',
      resumen:
        'Las ganancias de vender o cambiar criptomonedas tributan; las pérdidas las compensan.',
      encaje: 'conviene',
      motivo:
        'Este año llevas 1.840,00 € de ganancias con criptomonedas y tienes posiciones que, con los últimos precios, pierden 620,00 €. Si vendes o cambias esas antes del 31 de diciembre, la pérdida compensa la ganancia.',
      ahorro: 11_780,
      aplazado: false,
      calculo: '620,00 € compensados por un tipo del 19 %',
      plazo: '2026-12-31',
      requisitos: ['Las pérdidas tienen que realizarse (vender o cambiar) dentro del año.'],
      pasos: [
        'Importa todas las operaciones de tus plataformas en Criptomonedas.',
        'Pon los precios del día para ver qué posiciones están en pérdidas.',
        'Vende o cambia las que estén en pérdidas antes del 31 de diciembre.',
      ],
      riesgos: ['Las pérdidas que no compenses se pueden usar en los cuatro años siguientes.'],
      norma: 'Arts. 33, 46 y 49 de la Ley 35/2006 del IRPF',
      enlace: 'https://sede.agenciatributaria.gob.es/Sede/irpf/criptomonedas.html',
      pantalla: 'cripto',
    }),
    ficha({
      id: 'gastos_autonomo',
      area: 'gastos',
      titulo: 'Gastos de autónomo que se suelen olvidar',
      resumen:
        'Suministros de la vivienda en la que trabajas, seguro médico, dietas y la cuota de autónomos son gasto deducible.',
      encaje: 'valorar',
      motivo:
        'Si trabajas desde casa, parte de luz, agua, gas, teléfono e internet es gasto (30 % de la parte de la vivienda que usas para trabajar).',
      ahorro: null,
      aplazado: false,
      calculo: null,
      plazo: null,
      requisitos: ['Parte de la vivienda afecta a la actividad declarada en el censo (036).'],
      pasos: ['Declara en el censo la parte de la vivienda que usas para trabajar.'],
      riesgos: [],
      norma: 'Art. 30.2 de la Ley 35/2006 del IRPF',
      enlace: 'https://www.boe.es/buscar/act.php?id=BOE-A-2006-20764#a30',
      pantalla: 'recibidas',
    }),
    ficha({
      id: 'inicio_actividad',
      area: 'beneficio',
      titulo: 'Autónomo nuevo: 20 % menos de rendimiento',
      resumen:
        'Los autónomos en estimación directa que empiezan una actividad reducen un 20 % el rendimiento neto (hasta 100.000 €) el primer año con beneficio y el siguiente.',
      encaje: 'no_aplica',
      motivo: 'Sólo el primer año con rendimiento positivo y el siguiente desde el alta.',
      ahorro: null,
      aplazado: false,
      calculo: null,
      plazo: null,
      requisitos: [
        'Estimación directa.',
        'No haber tenido actividad económica en el año anterior al alta.',
      ],
      pasos: [
        'Confirma la fecha de alta y que el año anterior no tuviste otra actividad.',
        'En la Renta, aplica la reducción por inicio de actividad (casilla [0234]).',
      ],
      riesgos: [],
      norma: 'Art. 32.3 de la Ley 35/2006 del IRPF',
      enlace: 'https://www.boe.es/buscar/act.php?id=BOE-A-2006-20764#a32',
      pantalla: null,
    }),
  ];
  const abiertas = recomendaciones.filter(
    (r) => r.encaje === 'conviene' && r.marca.estado !== 'hecho' && r.marca.estado !== 'descartado',
  );
  return {
    situacion: {
      año: 2026,
      cierre: '2026-12-31',
      persona_fisica: true,
      regimen: null,
      cifra_negocios: null,
      base: {
        valor: 3_840_000,
        fuente: 'rendimiento neto de la actividad de 2026 hasta hoy (borrador de la Renta)',
      },
      cuota: null,
      tipo: 37,
      trabajadores: false,
      cnae: null,
      notas: [],
    },
    recomendaciones,
    ahorro_posible: abiertas.reduce((t, r) => t + (r.ahorro ?? 0), 0),
    aplazado_posible: 0,
  };
}

// ---------- Conversaciones con la IA (en memoria: se pierden al recargar) ----------

const DEMO_CRIPTO =
  'Con lo que hay en TaxPain: 1.840,00 € de ganancias y 620,00 € de pérdidas latentes. Si vendes antes del 31 de diciembre, la pérdida se resta de la ganancia y tributas por 1.220,00 €: unos 117,80 € menos al 19 % (arts. 46 y 49 LIRPF). Guarda el justificante de cada venta.';
const DEMO_PENSIONES =
  'Si aportas 5.750,00 € antes del 31 de diciembre (1.500,00 € a un plan individual y 4.250,00 € a un plan de empleo para autónomos), tu base de la Renta de 2026 baja en esa cantidad y pagas unos 2.127,50 € menos con tu tipo marginal aproximado del 37 % (arts. 51 y 52 LIRPF). Al rescatarlo tributará como rendimiento del trabajo: ahorras ahora a cambio de impuesto más adelante.';
const DEMO_PENSIONES_CON_DATO =
  'Con lo que has añadido, sólo te queda margen en el plan de empleo para autónomos: hasta 4.250,00 €. Si los aportas antes del 31 de diciembre, el ahorro sería de unos 1.572,50 € (4.250,00 € al 37 %).';
const DEMO_GENERAL =
  'Este trimestre tu 303 sale a ingresar 417,27 €: 1.890,00 € de IVA repercutido menos 1.472,73 € de IVA soportado deducible (art. 92 y ss. LIVA). Tienes 2 facturas recibidas sin revisar: si son de este trimestre, revísalas antes de presentar para no perder su IVA. Plazo: hasta el 20 de octubre.';
const DEMO_SITUACION = `- Tipo de contribuyente: autónomo (persona física) [DATO_1], NIF [NIF_1].
- Contabilidad 2026 hasta hoy (bases sin IVA): ingresos 40.817,00 €, gastos 10.591,00 €.`;
/** Lo que se enviaría (inventado): las instrucciones, el tema según su tipo y la situación. */
function demoSystem(topic: AssistantTopic): string {
  const base = 'Eres el asistente de TaxPain… (instrucciones fijas de la demo)';
  const datos = '## Datos actuales del usuario en TaxPain (anonimizados)';
  if (topic.kind === 'general') return `${base}\n\n${datos}\n${DEMO_SITUACION}`;
  const tema =
    topic.kind === 'hallazgo'
      ? topic.origin === 'aeat'
        ? `Hallazgo de «Qué revisar» de TaxPain: «${topic.title}» (texto de ejemplo de la demo).`
        : `Hallazgo de la revisión de la carpeta de la empresa de TaxPain: «${topic.title}» (texto de ejemplo de la demo; las rutas de los ficheros van como «fichero 1», «fichero 2»…).`
      : topic.kind === 'modelo'
        ? `Borrador del modelo ${topic.model} de TaxPain (${draftPeriod(topic.model, topic.year, topic.period)}): resultado, casillas y avisos (texto de ejemplo de la demo; sin el fichero ni la relación de terceros).`
        : `Recomendación de «Ahorrar impuestos» de TaxPain para el ejercicio 2026 (texto de ejemplo de la demo).`;
  return `${base}

Esta conversación es sobre un tema concreto de TaxPain: lo tienes en «Tema de esta conversación».

${datos}
### Tema de esta conversación
${tema}
### Tu situación en TaxPain
${DEMO_SITUACION}`;
}
const DEMO_DOCUMENTO =
  'CERTIFICADO DE APORTACIONES 2026 · Partícipe: CARMEN ESPAÑOLA ESPAÑOLA · Plan de Pensiones Individual Demo · Aportado del 01/01/2026 al 30/09/2026: 1.500,00 €';

const demoNotes = (used: string[], sources: string[], replaced = 2): MessageNotes => ({
  model: 'claude-opus-5-5',
  replaced,
  used,
  sources,
  unverified: [],
});

let demoNextId = 2;
const demoConversaciones: Conversation[] = [
  {
    id: 1,
    topic: { kind: 'ahorro', id: 'cripto_perdidas', year: 2026 },
    title: 'Criptomonedas: compensa ganancias con pérdidas antes del 31 de diciembre',
    created_at: '2026-10-12T17:40:00Z',
    updated_at: '2026-10-12T17:41:00Z',
    messages: [
      {
        seq: 1,
        role: 'user',
        content: '¿Cuánto me ahorro si vendo las que pierden?',
        created_at: '2026-10-12T17:40:00Z',
        notes: null,
      },
      {
        seq: 2,
        role: 'assistant',
        content: DEMO_CRIPTO,
        created_at: '2026-10-12T17:41:00Z',
        notes: demoNotes(['leer_articulo'], ['BOE-A-2006-20764, artículo 49']),
      },
    ],
    materials: [],
    full: false,
  },
];

const DEMO_ENCAJE = {
  conviene: ['Te conviene', 'Con tus datos, te conviene.'],
  valorar: ['Puede interesarte', 'Puede interesarte: depende de algo que TaxPain no sabe.'],
  aprovechado: ['Ya lo aprovechas', 'Ya lo aprovechas.'],
  no_aplica: ['No te aplica', 'Con tus datos, no te aplica.'],
} as const;
const DEMO_ESTADO = {
  pendiente: 'Pendiente',
  en_marcha: 'En marcha',
  hecho: 'Hecho',
  descartado: 'No me interesa',
} as const;

/** La cabecera de una tarjeta, como `ahorro::ia::tema_tarjeta` (§8.3 del diseño). */
function demoTarjetaHeader(r: Recomendacion, año: number, cierre: string): TopicHeader {
  const [encaje, apertura] = DEMO_ENCAJE[r.encaje];
  const que = r.aplazado ? 'Impuesto aplazado' : 'Ahorro estimado';
  const facts: [string, string][] = [['Encaje', encaje]];
  if (r.ahorro !== null) facts.push([que, euros(r.ahorro)]);
  if (r.plazo)
    facts.push([
      'Fecha límite',
      r.plazo === cierre
        ? `hasta el cierre del ejercicio (${formatDate(r.plazo)})`
        : formatDate(r.plazo),
    ]);
  const hechos = r.marca.pasos.filter((i) => i < r.pasos.length).length;
  facts.push(['Estado', DEMO_ESTADO[r.marca.estado]]);
  facts.push(['Pasos hechos', `${hechos} de ${r.pasos.length}`]);
  facts.push(['Norma', r.norma]);
  let intro = `${apertura} ${r.motivo}`;
  if (r.ahorro !== null) intro += ` ${que}: ${euros(r.ahorro)}.`;
  if (r.plazo)
    intro +=
      r.plazo === cierre
        ? ` Cuenta lo que hagas hasta el cierre del ejercicio (${formatDate(r.plazo)}).`
        : ` Fecha límite: ${formatDate(r.plazo)}.`;
  intro +=
    ' Pregunta lo que quieras o añade datos que TaxPain no tenga (cifras, decisiones o documentos).';
  const suggestions =
    r.encaje === 'conviene'
      ? [
          'Explícamelo con mis datos',
          '¿Qué tengo que hacer y antes de cuándo?',
          r.ahorro !== null ? '¿Cómo se ha calculado el ahorro?' : '¿Qué riesgos tiene en mi caso?',
        ]
      : r.encaje === 'valorar'
        ? [
            '¿Qué dato falta para saber si me conviene?',
            'Explícamelo con mis datos',
            '¿Qué riesgos tiene?',
          ]
        : r.encaje === 'aprovechado'
          ? ['¿Lo aprovecho del todo?', '¿Qué tengo que vigilar los próximos años?']
          : ['¿Por qué no me aplica?', '¿Qué tendría que cambiar para que me aplique?'];
  if (r.marca.estado === 'hecho') suggestions[1] = '¿Qué papeles guardo por si me los piden?';
  return {
    group: `Ahorrar impuestos · ${año}`,
    title: r.titulo,
    facts,
    intro,
    suggestions,
    notice: null,
  };
}

const DEMO_GRAVEDAD = { error: 'Error', aviso: 'Aviso', info: 'Para saber' } as const;
const DEMO_RESPONSABLE = {
  empresa: 'Empresa',
  gestoria: 'Gestoría',
  documentacion: 'Documentación',
} as const;
const DEMO_AREA: Record<AreaRevision, string> = {
  ficheros: 'Ficheros',
  facturas: 'Facturas',
  emitidas: 'Facturas emitidas',
  libros: 'Libros registro',
  modelos: 'Modelos presentados',
  sociedades: 'Impuesto sobre Sociedades',
  bancos: 'Bancos',
  plazos: 'Plazos y Registro Mercantil',
  aeat: 'AEAT: certificados, requerimientos y devoluciones',
  socios: 'Socios y administradores',
};

/** La cabecera de un hallazgo, como `revision::ia::tema_hallazgo`. */
function demoHallazgoHeader(h: Hallazgo, origin: OrigenHallazgo): TopicHeader {
  const de = origin === 'aeat' ? 'Qué revisar' : 'Revisión de la carpeta';
  const importe = h.importe ?? 0;
  const rect = h.rectificable_hasta
    ? `${formatDate(h.rectificable_hasta)}${h.rectificable_aprox ? ' aprox.' : ''}`
    : null;
  const facts: [string, string][] = [
    ['Importancia', DEMO_GRAVEDAD[h.gravedad]],
    ['Responsable', DEMO_RESPONSABLE[h.responsable]],
    ['Área', DEMO_AREA[h.area]],
  ];
  if (h.ejercicio !== null) facts.push(['Ejercicio', String(h.ejercicio)]);
  if (importe > 0) facts.push(['Recuperable', euros(importe)]);
  if (importe < 0) facts.push(['A pagar', euros(-importe)]);
  if (rect) facts.push(['Se puede rectificar hasta', rect]);
  if (h.ficheros.length > 0) facts.push(['Ficheros afectados', String(h.ficheros.length)]);
  let intro = `${
    origin === 'aeat'
      ? 'Al comparar lo que tiene la AEAT con lo que sabe TaxPain sale esto:'
      : 'Al revisar la carpeta de la empresa sale esto:'
  } ${h.detalle}`;
  if (importe > 0) intro += ` Recuperable (estimado): ${euros(importe)}.`;
  if (importe < 0) intro += ` A pagar (estimado): ${euros(-importe)}.`;
  if (rect) intro += ` Se puede rectificar hasta el ${rect}.`;
  intro +=
    ' Pregunta lo que quieras o añade datos que TaxPain no tenga (cifras, decisiones o documentos).';
  const suggestions = ['¿Qué significa y es grave?', '¿Qué le pido a mi gestoría?'];
  if (importe > 0) suggestions.push('¿Cómo pido la devolución y hasta cuándo puedo?');
  return {
    group: h.ejercicio !== null ? `${de} · ${h.ejercicio}` : de,
    title: h.titulo,
    facts,
    intro,
    suggestions,
    notice: null,
  };
}

const DEMO_DECLARACION: Record<string, string> = {
  I: 'a ingresar',
  U: 'a ingresar por domiciliación',
  C: 'a compensar en los próximos trimestres',
  D: 'a devolver',
  N: 'sin actividad o resultado cero',
  B: 'negativo: a deducir en los próximos pagos fraccionados',
};

/** La cabecera del borrador de un modelo con tipo de declaración, como `models::ia::tema_borrador`. */
function demoBorradorHeader(d: ModelDraft): TopicHeader {
  const p = draftPeriod(d.model, d.year, d.quarter);
  const q = DEMO_DECLARACION[d.declaration] ?? d.declaration;
  const facts: [string, string][] = [
    ['Resultado', `${euros(d.result)} · ${q}`],
    ['Periodo', p],
  ];
  if (d.warnings.length > 0) facts.push(['Avisos', String(d.warnings.length)]);
  return {
    group: `Modelos · ${d.year}`,
    title: draftTitle(d.model, d.year, d.quarter),
    facts,
    intro: `Con tus facturas y tu contabilidad, el borrador del modelo ${d.model} ${d.quarter ? 'del' : 'de'} ${p} sale ${euros(d.result)} (${q}). Pregunta lo que quieras o añade datos que TaxPain no tenga (cifras, decisiones o documentos).`,
    suggestions: [
      d.declaration === 'D' ? '¿Por qué me sale a devolver?' : '¿Por qué me sale a ingresar?',
      'Explícame las casillas principales',
      '¿Qué reviso antes de presentarlo?',
    ],
    notice: null,
  };
}

function demoHeader(topic: AssistantTopic, c: Conversation | undefined): TopicHeader {
  if (topic.kind === 'modelo') {
    if (topic.model !== '303') throw { message: 'demo: sólo el 303' };
    return demoBorradorHeader(draft303);
  }
  if (topic.kind === 'hallazgo') {
    const lista: Hallazgo[] = topic.origin === 'aeat' ? DEMO_HALLAZGOS_AEAT : REVISION.hallazgos;
    const h = lista.find(
      (x) => x.codigo === topic.code && x.ejercicio === topic.year && x.titulo === topic.title,
    );
    if (!h)
      throw {
        message:
          'Este hallazgo ya no sale en «Qué revisar»: vuelve a abrir Evolución para verlo con tus datos de ahora.',
      };
    return demoHallazgoHeader(h, topic.origin);
  }
  if (topic.kind === 'general')
    return {
      group: 'Asistente',
      title: c?.title ?? 'Nueva conversación',
      facts: [],
      intro:
        'Pregunta sobre tus impuestos: la IA ve un resumen de tus cifras, plazos y avisos de TaxPain. Puedes añadir datos o documentos que TaxPain no tenga.',
      suggestions: [
        '¿Cuánto IVA me toca pagar este trimestre y por qué?',
        '¿Qué me falta por revisar antes del próximo plazo?',
      ],
      notice: null,
    };
  const a = demoAhorro();
  const { año, cierre } = a.situacion;
  if (topic.kind === 'ahorro_plan') {
    const abiertas = (e: string) =>
      a.recomendaciones.filter(
        (r) => r.encaje === e && r.marca.estado !== 'hecho' && r.marca.estado !== 'descartado',
      ).length;
    const n = abiertas('conviene');
    return {
      group: `Ahorrar impuestos · tu plan de ${año}`,
      title: `Tu plan de ahorro de ${año}`,
      facts: [
        ['Ahorro posible', euros(a.ahorro_posible)],
        ['Te convienen', String(n)],
        ['Puedes valorar', String(abiertas('valorar'))],
        ['Cierre del ejercicio', formatDate(cierre)],
      ],
      intro: `En ${año} podrías ahorrarte unos ${euros(a.ahorro_posible)} con ${n} ${n === 1 ? 'recomendación que te conviene' : 'recomendaciones que te convienen'}. Pregunta por dónde empezar o añade datos que TaxPain no tenga.`,
      suggestions: [
        '¿Por cuál empiezo?',
        `¿Qué tengo que hacer antes del 31 de diciembre de ${año}?`,
        '¿Se pueden combinar entre ellas?',
      ],
      notice: null,
    };
  }
  const r = a.recomendaciones.find((x) => x.id === topic.id);
  if (!r) throw { message: 'Esta recomendación no es para tu tipo de contribuyente.' };
  return demoTarjetaHeader(r, año, cierre);
}

function demoFind(topic: AssistantTopic, id: number | null): Conversation | undefined {
  if (id !== null) {
    const c = demoConversaciones.find((x) => x.id === id);
    if (!c) throw { message: 'Esta conversación ya no existe: se ha borrado.' };
    return c;
  }
  if (topic.kind === 'general') return undefined;
  return demoConversaciones.find((x) => topicKey(x.topic) === topicKey(topic));
}

const demoOpened = (topic: AssistantTopic, c: Conversation | undefined): TopicOpened => ({
  header: demoHeader(topic, c),
  conversation: c ? structuredClone(c) : null,
  changed: false,
});

/** La conversación del tema (la crea si no la hay, como al guardar la primera respuesta). */
function demoEnsure(topic: AssistantTopic, id: number | null, title: string): Conversation {
  const found = demoFind(topic, id);
  if (found) return found;
  const at = new Date().toISOString();
  const c: Conversation = {
    id: demoNextId++,
    topic,
    title,
    created_at: at,
    updated_at: at,
    messages: [],
    materials: [],
    full: false,
  };
  demoConversaciones.push(c);
  return c;
}

function demoTitle(topic: AssistantTopic, question: string | null): string {
  if (topic.kind === 'hallazgo') return topic.title;
  if (topic.kind === 'modelo') return draftTitle(topic.model, topic.year, topic.period);
  if (topic.kind === 'ahorro_plan') return `Tu plan de ahorro de ${topic.year}`;
  if (topic.kind === 'ahorro')
    return demoAhorro().recomendaciones.find((r) => r.id === topic.id)?.titulo ?? topic.id;
  if (!question) return `Conversación del ${formatDate(new Date().toISOString().slice(0, 10))}`;
  return question.length > 60 ? `${question.slice(0, 59).trimEnd()}…` : question;
}

/** La respuesta de ejemplo según el tema (y lo añadido). */
function demoReply(topic: AssistantTopic, c: Conversation): { text: string; notes: MessageNotes } {
  if (topic.kind === 'general')
    return {
      text: DEMO_GENERAL,
      notes: demoNotes(
        ['borrador_modelo', 'buscar_facturas', 'leer_articulo'],
        ['BOE-A-1992-28740, artículo 99'],
        1,
      ),
    };
  if (topic.kind === 'ahorro' && topic.id === 'cripto_perdidas')
    return {
      text: DEMO_CRIPTO,
      notes: demoNotes(['leer_articulo'], ['BOE-A-2006-20764, artículo 49']),
    };
  if (topic.kind === 'ahorro' && topic.id === 'plan_pensiones_autonomo') {
    const conDato = c.materials.some((m) => m.content.includes('1.500'));
    return {
      text: conDato ? DEMO_PENSIONES_CON_DATO : DEMO_PENSIONES,
      notes: demoNotes(['borrador_modelo', 'leer_articulo'], ['BOE-A-2006-20764, artículo 52']),
    };
  }
  const r =
    topic.kind === 'ahorro'
      ? demoAhorro().recomendaciones.find((x) => x.id === topic.id)
      : undefined;
  const m = r?.enlace.match(/id=(BOE-[A-Z]-\d{4}-\d+)#a(\d+)/);
  const extra = c.materials.length > 0 ? ' y lo que has añadido' : '';
  return {
    text: `(Respuesta de ejemplo de la demo.) Con tus datos reales, la IA explicaría «${c.title}» con las cifras de TaxPain${extra} y te diría qué hacer y antes de cuándo.`,
    notes: demoNotes(['borrador_modelo', 'leer_articulo'], m ? [`${m[1]}, artículo ${m[2]}`] : []),
  };
}

function demoSummary(c: Conversation): ConversationSummary {
  return {
    id: c.id,
    topic: c.topic,
    title: c.title,
    updated_at: c.updated_at,
    questions: c.messages.filter((m) => m.role === 'user').length,
    materials: c.materials.length,
  };
}

/** Añade un dato o un documento a la conversación del tema (como `add_material`). */
function demoAddMaterial(
  args: Record<string, unknown> | undefined,
  m: Omit<Conversation['materials'][number], 'id' | 'created_at' | 'chars'>,
): Conversation {
  const topic = args?.topic as AssistantTopic;
  const id = (args?.conversationId as number | null) ?? null;
  const existing = demoFind(topic, id);
  if (existing && existing.materials.length >= 20)
    throw {
      message: 'Ya hay 20 datos y documentos en esta conversación: quita alguno para añadir otro.',
    };
  if (m.filename && existing?.materials.some((x) => x.filename === m.filename))
    throw { message: 'Ese documento ya está en esta conversación.' };
  const c = existing ?? demoEnsure(topic, null, demoTitle(topic, null));
  const at = new Date().toISOString();
  const max = c.materials.reduce((n, x) => Math.max(n, x.id), 0);
  c.materials.push({ ...m, id: max + 1, created_at: at, chars: m.content.length });
  c.updated_at = at;
  return structuredClone(c);
}

/** Lo que hay guardado en la demo (para las pruebas). */
export function demoConversationsCount(): number {
  return demoConversaciones.length;
}

const DEMO_EVOLUCION = {
  años: [2023, 2024, 2025],
  sin_comprobar: false,
  sin_casillas: 0,
  grupos: [
    {
      titulo: 'Actividad y cuentas',
      filas: [
        demoFila(
          'cifra_negocios',
          'Cifra de negocios',
          'Ventas y servicios del año.',
          '200 [00255]',
          [9_700_000, 11_050_000, null],
        ),
        demoFila('resultado', 'Resultado contable', 'El beneficio del año.', '200 [00500]', [
          1_420_000,
          1_610_000,
          null,
        ]),
      ],
    },
    {
      titulo: 'Impuesto sobre Sociedades',
      filas: [
        demoFila(
          'cuota_integra',
          'Cuota íntegra',
          'El impuesto antes de deducciones.',
          '200 [00562]',
          [null, 368_000, null],
        ),
      ],
    },
    {
      titulo: 'IVA',
      filas: [
        demoFila(
          'volumen',
          'Volumen de operaciones',
          'Lo facturado en el año a efectos del IVA.',
          '390 [108]',
          [9_850_000, 11_200_000, 12_430_000],
        ),
        demoFila(
          'iva_devengado',
          'IVA devengado',
          'El IVA de las ventas.',
          '390 [47]',
          [2_068_500, 2_352_000, 2_610_300],
        ),
        demoFila(
          'iva_deducible',
          'IVA deducible',
          'El IVA de las compras y gastos.',
          '390 [64]',
          [812_000, 901_400, 958_200],
        ),
        demoFila(
          'iva_resultado',
          'Resultado del IVA del año',
          'Positivo: a ingresar.',
          '390 [84]',
          [1_256_500, 1_450_600, 1_652_100],
        ),
      ],
    },
    {
      titulo: 'Retenciones',
      filas: [
        demoFila(
          'ret_trabajo',
          'Retenciones de nóminas y profesionales',
          'Lo retenido e ingresado.',
          'suma de los 111 [28]',
          [420_000, 455_000, 482_500],
        ),
        demoFila(
          'perceptores',
          'Perceptores con retención',
          'Según el 190.',
          '190 (registros)',
          [3, 3, 4],
          true,
        ),
      ],
    },
    {
      titulo: 'Declaraciones',
      filas: [
        demoFila(
          'presentadas',
          'Declaraciones presentadas',
          'Las que tiene la AEAT.',
          'AEAT',
          [14, 15, 15],
          true,
        ),
        demoFila(
          'fuera_de_plazo',
          'Presentadas fuera de plazo',
          'Pueden llevar recargo.',
          'comprobación con la AEAT',
          [0, 1, 0],
          true,
        ),
      ],
    },
  ],
};

// En el orden en que los da la revisión: por gravedad, área, ejercicio (el último, antes) y título.
const DEMO_HALLAZGOS_AEAT: Hallazgo[] = [
  {
    codigo: 'modelos.faltan',
    area: 'modelos' as const,
    gravedad: 'error' as const,
    responsable: 'gestoria' as const,
    ejercicio: 2025,
    titulo: 'Falta el 202 del 3P 2025',
    detalle:
      'Según el perfil fiscal tocaba y la AEAT no lo tiene. Pregunta a la gestoría si lo presentó por otra vía o preséntalo cuanto antes.',
    ficheros: [],
    importe: null,
    rectificable_hasta: '2029-04-20',
  },
  {
    codigo: 'sociedades.exencion_dividendos',
    area: 'sociedades' as const,
    gravedad: 'error' as const,
    responsable: 'gestoria' as const,
    ejercicio: 2024,
    titulo: 'Dividendos de 2024 sin la exención del art. 21 LIS',
    detalle:
      'En el 200 de 2024 hay 10.000,00 € de dividendos de participaciones en empresas del grupo y asociadas y la exención (casilla [00370]) está vacía: tributaron enteros. Con al menos el 5 % de la participada durante un año, el 95 % está exento (art. 21.1 y 21.10 LIS): 9.500,00 € que no debían tributar. Rehecha la liquidación con la exención, la cuota baja unos 2.185,00 € (estimación). Hasta el 25/07/2029 (art. 66 LGT) se puede presentar una autoliquidación rectificativa del 200 (art. 120.4 LGT y art. 59 bis RIS).',
    ficheros: ['AEAT · modelo 200 2024 0A · justificante 2000000000002'],
    importe: 218_500,
    rectificable_hasta: '2029-07-25',
  },
  {
    codigo: 'sociedades.exencion_dividendos',
    area: 'sociedades' as const,
    gravedad: 'error' as const,
    responsable: 'gestoria' as const,
    ejercicio: 2023,
    titulo: 'Dividendos de 2023 sin la exención del art. 21 LIS',
    detalle:
      'En el 200 de 2023 hay 8.000,00 € de dividendos de participaciones en empresas del grupo y asociadas y la exención (casilla [00370]) está vacía: tributaron enteros. Con al menos el 5 % de la participada durante un año, el 95 % está exento (art. 21.1 y 21.10 LIS): 7.600,00 € que no debían tributar. Rehecha la liquidación con la exención, la cuota baja unos 1.748,00 € (estimación). Hasta el 25/07/2028 (art. 66 LGT) se puede presentar una solicitud de rectificación (art. 120.3 LGT).',
    ficheros: ['AEAT · modelo 200 2023 0A · justificante 2000000000001'],
    importe: 174_800,
    rectificable_hasta: '2028-07-25',
  },
  {
    codigo: 'cruces.303_saldo_perdido',
    area: 'modelos' as const,
    gravedad: 'aviso' as const,
    responsable: 'gestoria' as const,
    ejercicio: 2025,
    titulo: 'Se ha dejado de arrastrar IVA a compensar en el 303 del 2T 2025',
    detalle:
      'El 303 del 1T 2025 dejó 238,40 € a compensar en periodos siguientes, pero el 303 del 2T 2025 sólo arrastra 0,00 € en [110]: faltan 238,40 € a favor de la empresa. Qué hacer: pide a la gestoría que lo incluya en el siguiente 303 (hay cuatro años para compensarlo, art. 99.Cinco LIVA) o que pida la rectificación.',
    ficheros: ['AEAT · modelo 303 2025 2T · justificante 3030000000003'],
    importe: 23_840,
    rectificable_hasta: '2029-07-21',
  },
  {
    codigo: 'cruces.390_303',
    area: 'modelos' as const,
    gravedad: 'aviso' as const,
    responsable: 'gestoria' as const,
    ejercicio: 2024,
    titulo: 'El 390 de 2024 no cuadra con la suma de sus 303',
    detalle:
      'El IVA devengado del 390 (23.520,00 €) es 210,00 € mayor que la suma de los cuatro 303. Pide a la gestoría que lo revise: o falta una complementaria de un trimestre o el 390 tiene un error.',
    ficheros: ['AEAT · modelo 390 2024 0A · justificante 3900000000001'],
    importe: null,
    rectificable_hasta: '2028-04-20',
  },
  {
    codigo: 'cruces.200_202_sin_descontar',
    area: 'sociedades' as const,
    gravedad: 'aviso' as const,
    responsable: 'gestoria' as const,
    ejercicio: 2024,
    titulo: 'El 200 de 2024 no descuenta todo el pago fraccionado del 2P',
    detalle:
      'El 202 del 2P 2024 ingresó 414,00 € y el 200 sólo descuenta 0,00 € en [00603]: 414,00 € pagados dos veces. Qué hacer: pide a la gestoría la rectificación del 200 para recuperarlo.',
    ficheros: ['AEAT · modelo 200 2024 0A · justificante 2000000000002'],
    importe: 41_400,
    rectificable_hasta: '2029-07-25',
  },
  {
    codigo: 'cruces.fuera_de_plazo',
    area: 'plazos' as const,
    gravedad: 'aviso' as const,
    responsable: 'gestoria' as const,
    ejercicio: 2024,
    titulo: 'El 111 del 2T 2024 se presentó fuera de plazo',
    detalle:
      'Venció el 22/07/2024 y se presentó el 05/09/2024: un mes completo de retraso, recargo del 2 % sobre lo ingresado (art. 27 LGT).',
    ficheros: ['AEAT · modelo 111 2024 2T · justificante 1110000000002'],
    importe: -9_100,
    rectificable_hasta: '2028-04-20',
  },
];

/** Años comprobados en la demo. */
const DEMO_CHECKED_YEARS = [2023, 2024, 2025];

/** Resumen de un año comprobado en la demo: sale de su detalle, para que cuadren. */
function demoYearSummary(year: number) {
  const r = demoAeatCheck(year, true);
  const count = (s: string) => r.rows.filter((x) => x.status === s).length;
  return {
    year,
    presented: count('Presented'),
    missing: count('Missing'),
    not_found: count('NotFound'),
    open: count('Open'),
    pdfs: r.rows.filter((x) => x.has_pdf).length,
    boxes_read: r.rows.filter((x) => x.boxes_read).length,
    late: r.rows.filter((x) => x.late).length,
    errors: r.errors.length,
    failed: r.errors,
    unavailable: r.unavailable,
    checked_at: r.checked_at,
  };
}

/** La comprobación de la demo trasladada al año pedido (fechas del mismo modo). */
function demoAeatCheck(year: number, checked: boolean): AeatCheckReport {
  const shift = (s: string | null) =>
    s?.replace(/^2025/, String(year)).replace(/^2026/, String(year + 1)) ?? null;
  return {
    ...DEMO_AEAT_CHECK,
    year,
    checked_at: checked ? DEMO_AEAT_CHECK.checked_at : null,
    rows: DEMO_AEAT_CHECK.rows.map((r) => ({
      ...r,
      due_on: shift(r.due_on),
      presented_at: shift(r.presented_at),
      original_presented_at: shift(r.original_presented_at),
    })),
  };
}

const FACTS = {
  nif: 'B12345674',
  name: 'ESTUDIO EJEMPLO, S.L.',
  street: 'Calle de la Paz 15',
  post_code: '46003',
  town: 'Valencia',
  constitution_date: '2019-03-12',
  capital: 300_000,
  cnae: '6201',
  iae: null,
  vat_number: null,
  administrator: 'Carmen Española Española',
  notary: 'Ramón Pérez Notario',
  protocol: '1234',
  registry: 'Registro Mercantil de Madrid, Tomo 1.234, Folio 56, Hoja M-12345',
  date: '2019-03-12',
};

const CONTACTS = [
  {
    key: 'B00123455',
    id: 1,
    role: 'cliente',
    name: 'Estudio Gráfico Turia SL',
    tax_id: 'B00123455',
    country: 'ES',
    email: 'admin@turia.example',
    invoiced_year: 2_904_000,
    invoiced_total: 4_719_000,
    purchased_year: 0,
    purchased_total: 0,
    to_collect: 363_000,
    to_pay: 0,
    overdue: 0,
    last_date: '2026-10-15',
    invoices: 13,
  },
  {
    key: 'FR12345678901',
    id: 2,
    role: 'cliente',
    name: 'Atelier Lumière SAS',
    tax_id: 'FR12345678901',
    country: 'FR',
    email: 'compta@lumiere.example',
    invoiced_year: 1_200_000,
    invoiced_total: 1_200_000,
    purchased_year: 0,
    purchased_total: 0,
    to_collect: 400_000,
    to_pay: 0,
    overdue: 400_000,
    last_date: '2026-08-31',
    invoices: 3,
  },
  {
    key: 'B00654327',
    id: null,
    role: 'proveedor',
    name: 'Coworking Ruzafa SL',
    tax_id: 'B00654327',
    country: 'ES',
    email: null,
    invoiced_year: 0,
    invoiced_total: 0,
    purchased_year: 435_600,
    purchased_total: 871_200,
    to_collect: 0,
    to_pay: 0,
    overdue: 0,
    last_date: '2026-10-01',
    invoices: 22,
  },
];
// ---------- Previsión de tesorería (sintética) ----------
type DemoForecast = import('./api').CashForecast;
type DemoForecastItem = DemoForecast['items'][number];

/** Previsión de la demo: cartera, modelos y asientos periódicos inventados, con los mismos
 * cálculos de periodos y mínimos que `taxpain_app::forecast`. */
function forecastDemo(): DemoForecast {
  const iso = (d: Date) => d.toISOString().slice(0, 10);
  const utc = (y: number, m0: number, d: number) => iso(new Date(Date.UTC(y, m0, d)));
  const plusDays = (s: string, n: number) => {
    const d = new Date(`${s}T00:00:00Z`);
    d.setUTCDate(d.getUTCDate() + n);
    return iso(d);
  };
  const opening = 1_874_350;
  const item = (
    date: string,
    concept: string,
    amount: number,
    category: DemoForecastItem['category'],
    source: string,
    extra: Partial<DemoForecastItem> = {},
  ): DemoForecastItem => ({
    date,
    due_date: date,
    concept,
    amount,
    category,
    source,
    overdue: false,
    estimated: false,
    ...extra,
  });
  const tax = (date: string, concept: string, amount: number) =>
    item(date, concept, amount, 'taxes', 'borrador del modelo', { estimated: true });
  const items: DemoForecastItem[] = [
    item(TODAY, 'Cobro A2026-0087 · Cliente Ejemplo SA', 121_000, 'collections', 'cartera', {
      due_date: '2026-09-30',
      overdue: true,
    }),
    tax('2026-10-20', 'Modelo 303 3T 2026', -41_727),
    tax('2026-10-20', 'Modelo 130 3T 2026', -31_080),
    tax('2026-10-20', 'Modelo 115 3T 2026', -13_800),
    item('2026-10-22', 'Cobro A2026-0091 · Estudio Ejemplo SL', 363_000, 'collections', 'cartera'),
    item('2026-10-27', 'Pago F-2291 · Proveedor Ejemplo SL', -60_500, 'payments', 'cartera'),
    item('2026-11-05', 'Cobro A2026-0094 · Cliente Ejemplo SA', 242_000, 'collections', 'cartera'),
    item('2026-11-10', 'Pago 2026/118 · Imprenta Ejemplo SL', -145_200, 'payments', 'cartera'),
    item(
      '2026-11-20',
      'Cobro A2026-0096 (1/2) · Estudio Ejemplo SL',
      242_000,
      'collections',
      'cartera',
    ),
    item('2026-12-04', 'Pago F-2340 · Proveedor Ejemplo SL', -36_300, 'payments', 'cartera'),
    item(
      '2026-12-21',
      'Cobro A2026-0096 (2/2) · Estudio Ejemplo SL',
      242_000,
      'collections',
      'cartera',
    ),
    tax('2027-01-20', 'Modelo 115 4T 2026 (periodo en curso)', -13_800),
    tax('2027-02-01', 'Modelo 303 4T 2026 (periodo en curso)', -38_012),
    tax('2027-02-01', 'Modelo 130 4T 2026 (periodo en curso)', -29_040),
  ];
  // Asientos periódicos por banco: alquiler (día 1), iguala de un cliente (día 5) y cuota de
  // autónomos (último día del mes).
  for (let k = 0; k < 12; k++) {
    const y = 2026 + Math.floor((9 + k) / 12);
    const m0 = (9 + k) % 12;
    if (k > 0) {
      items.push(
        item(utc(y, m0, 1), 'Alquiler despacho', -72_600, 'recurring', 'asiento periódico'),
        item(
          utc(y, m0, 5),
          'Iguala mensual · Cliente Ejemplo SA',
          181_500,
          'recurring',
          'asiento periódico',
        ),
      );
    }
    items.push(
      item(utc(y, m0 + 1, 0), 'Cuota de autónomos', -32_000, 'recurring', 'asiento periódico'),
    );
  }
  // Cuotas sin registrar de los préstamos de la demo (netas de la retención).
  const limite = demoMeses(TODAY, 12);
  for (const v of demoPrestamos()) {
    const cobro = v.prestamo.sentido === 'concedido';
    for (const q of v.cuotas) {
      if (q.estado !== 'pendiente' || q.fecha < TODAY || q.fecha > limite) continue;
      items.push(
        item(
          q.fecha,
          `Préstamo ${v.prestamo.nombre}: cuota ${q.n}/${v.cuotas.length}`,
          cobro ? q.neto : -q.neto,
          'loans',
          'préstamo',
        ),
      );
    }
  }
  items.sort((a, b) => a.date.localeCompare(b.date) || a.concept.localeCompare(b.concept));

  const horizon = (ranges: [string, string][]): DemoForecast['weekly'] => {
    let running = opening;
    const buckets = ranges.map(([start, end]) => {
      const b = {
        start,
        end,
        opening: running,
        inflows: {} as Record<string, number>,
        outflows: {} as Record<string, number>,
        total_in: 0,
        total_out: 0,
        closing: 0,
      };
      for (const i of items.filter((i) => i.date >= start && i.date <= end)) {
        const side = i.amount < 0 ? b.outflows : b.inflows;
        side[i.category] = (side[i.category] ?? 0) + Math.abs(i.amount);
        if (i.amount < 0) b.total_out -= i.amount;
        else b.total_in += i.amount;
      }
      b.closing = running + b.total_in - b.total_out;
      running = b.closing;
      return b;
    });
    const end = ranges.at(-1)?.[1] ?? TODAY;
    const start = { date: TODAY, balance: opening };
    const points = [start];
    let balance = opening;
    for (const i of items.filter((i) => i.date <= end)) {
      balance += i.amount;
      const last = points.at(-1);
      if (last && points.length > 1 && last.date === i.date) last.balance = balance;
      else points.push({ date: i.date, balance });
    }
    // Mínimo y primer negativo desde el saldo al final de hoy.
    const days = points.length > 1 && points[1]?.date === TODAY ? points.slice(1) : points;
    const low = days.reduce((a, p) => (p.balance < a.balance ? p : a), days[0] ?? start);
    return {
      buckets,
      end,
      points,
      lowest: low.balance,
      lowest_date: low.date,
      first_negative: days.find((p) => p.balance < 0)?.date ?? null,
    };
  };
  const weeks: [string, string][] = [];
  let start = TODAY;
  for (let k = 0; k < 13; k++) {
    const dow = (new Date(`${start}T00:00:00Z`).getUTCDay() + 6) % 7;
    const end = plusDays(start, 6 - dow);
    weeks.push([start, end]);
    start = plusDays(end, 1);
  }
  const months: [string, string][] = Array.from({ length: 12 }, (_, k) => {
    const y = 2026 + Math.floor((9 + k) / 12);
    const m0 = (9 + k) % 12;
    return [k === 0 ? TODAY : utc(y, m0, 1), utc(y, m0 + 1, 0)];
  });
  return {
    today: TODAY,
    opening,
    weekly: horizon(weeks),
    monthly: horizon(months),
    not_yours: { vat: 79_739, withholdings: 13_800, total: 93_539 },
    items,
    warnings: [
      {
        kind: 'overdue',
        text: 'Cobros vencidos: 1 por 1.210,00 €. Se cuentan como si se cobraran esta semana.',
      },
    ],
    notes: [
      'Impuestos: el periodo en curso (con lo contabilizado hasta hoy) y los terminados sin presentar o pagar; los periodos que aún no han empezado no se incluyen.',
    ],
  };
}

/** Asistente de saldos iniciales: el balance del gestor de una empresa inventada, en el paso 2. */
const aperturaDemo: OpeningDraft = {
  date: '2026-01-01',
  step: 1,
  source: 'Sumas y saldos 2025.xlsx',
  lines: [
    { account: '1000000', name: 'Capital social', debit: 0, credit: 300_000 },
    { account: '1130000', name: 'Reservas voluntarias', debit: 0, credit: 1_250_000 },
    { account: '1290000', name: 'Resultado del ejercicio', debit: 0, credit: 845_000 },
    { account: '2170000', name: 'Equipos para procesos de información', debit: 420_000, credit: 0 },
    { account: '2817000', name: 'Amortización acumulada de equipos', debit: 0, credit: 175_000 },
    { account: '4300001', name: 'EMPRESA DEMO SL', debit: 363_000, credit: 0 },
    { account: '4750000', name: 'Hacienda Pública, acreedora por IVA', debit: 0, credit: 25_000 },
    { account: '5720001', name: 'Cuenta corriente', debit: 1_812_000, credit: 0 },
  ],
  customers: [],
  suppliers: [],
  assets: [],
  banks: [],
  tax: { vat_refund: 0, vat_due: 0, withholdings: 0 },
};

// ---------- Criptomonedas (datos sintéticos de EMPRESA DEMO) ----------

const criptoPlatforms: CriptoPlatform[] = [
  { id: 1, name: 'Kraken', kind: 'exchange', country: 'US', abroad: true, account: '' },
  { id: 2, name: 'Ledger', kind: 'monedero', country: 'ES', abroad: false, account: '' },
];

const criptoOps: CriptoOp[] = [
  {
    id: 1,
    date: '2026-01-12',
    time: '10:15:00',
    kind: 'compra',
    platform_id: 1,
    to_platform_id: null,
    ticker: 'BTC',
    qty: '0.5',
    ticker2: null,
    qty2: null,
    eur: 3_600_000,
    fee_eur: 1_800,
    ref_price: null,
    note: '',
    source: 'demo',
    fingerprint: null,
    entry_id: 101,
  },
  {
    id: 2,
    date: '2026-02-20',
    time: '16:40:00',
    kind: 'compra',
    platform_id: 1,
    to_platform_id: null,
    ticker: 'ETH',
    qty: '8',
    ticker2: null,
    qty2: null,
    eur: 1_920_000,
    fee_eur: 960,
    ref_price: null,
    note: '',
    source: 'demo',
    fingerprint: null,
    entry_id: 102,
  },
  {
    id: 3,
    date: '2026-04-03',
    time: '09:05:00',
    kind: 'traspaso',
    platform_id: 1,
    to_platform_id: 2,
    ticker: 'BTC',
    qty: '0.3',
    ticker2: null,
    qty2: null,
    eur: 0,
    fee_eur: 400,
    ref_price: null,
    note: 'a la Ledger',
    source: 'demo',
    fingerprint: null,
    entry_id: 103,
  },
  {
    id: 4,
    date: '2026-06-15',
    time: '12:00:00',
    kind: 'venta',
    platform_id: 1,
    to_platform_id: null,
    ticker: 'BTC',
    qty: '0.2',
    ticker2: null,
    qty2: null,
    eur: 1_700_000,
    fee_eur: 850,
    ref_price: null,
    note: '',
    source: 'demo',
    fingerprint: null,
    entry_id: null,
  },
  {
    id: 5,
    date: '2026-09-01',
    time: '00:00:00',
    kind: 'recompensa',
    platform_id: 1,
    to_platform_id: null,
    ticker: 'ETH',
    qty: '0.05',
    ticker2: null,
    qty2: null,
    eur: 12_500,
    fee_eur: 0,
    ref_price: null,
    note: 'staking',
    source: 'demo',
    fingerprint: null,
    entry_id: null,
  },
];

const criptoPrices: CriptoPrice[] = [
  { date: '2026-12-31', ticker: 'BTC', price: '90000', source: 'manual' },
  { date: '2026-12-31', ticker: 'ETH', price: '2600', source: 'manual' },
];

const criptoReport: CriptoReport = {
  year: 2026,
  fecha: '2026-12-31',
  settings: {
    metodo: 'pmp',
    naturaleza: 'existencias',
    cuentas: {
      existencias: '309',
      intangible: '209',
      tesoreria: '5720001',
      puente: '5550001',
      ventas: '700',
      variacion: '610',
      beneficio: '771',
      perdida: '671',
      comisiones: '626',
      recompensas: '769',
      deterioro_gasto: '693',
      deterioro: '390',
      reversion: '793',
    },
  },
  persona_fisica: true,
  metodo_declaracion: 'fifo',
  posiciones: [
    {
      ticker: 'BTC',
      platform_id: 2,
      platform: 'Ledger',
      abroad: false,
      cantidad: '0.3',
      coste: 2_161_080,
      precio: '90000',
      precio_fecha: null,
      valor: 2_700_000,
      latente: 538_920,
    },
    {
      ticker: 'ETH',
      platform_id: 1,
      platform: 'Kraken',
      abroad: true,
      cantidad: '8.05',
      coste: 1_933_460,
      precio: '2600',
      precio_fecha: null,
      valor: 2_093_000,
      latente: 159_540,
    },
  ],
  valor_total: 4_793_000,
  coste_total: 4_094_540,
  resumen_pmp: {
    metodo: 'pmp',
    year: 2026,
    ganancias: 258_430,
    perdidas: 0,
    neto: 258_430,
    recompensas: 12_500,
    comisiones: 400,
    transmisiones: 1,
  },
  resumen_fifo: {
    metodo: 'fifo',
    year: 2026,
    ganancias: 258_430,
    perdidas: 0,
    neto: 258_430,
    recompensas: 12_500,
    comisiones: 400,
    transmisiones: 1,
  },
  realizados: [
    {
      op: 4,
      fecha: '2026-06-15',
      tipo: 'venta',
      ticker: 'BTC',
      platform: 'Kraken',
      cantidad: '0.2',
      valor: 1_699_150,
      coste: 1_440_720,
      resultado: 258_430,
    },
  ],
  m721: {
    year: 2026,
    fecha: '2026-12-31',
    total_extranjero: 2_093_000,
    umbral: 5_000_000,
    supera: false,
    plataformas: [
      {
        platform: 'Kraken',
        country: 'US',
        valor: 2_093_000,
        activos: [['ETH', '8.05', 2_093_000]],
      },
    ],
    sin_precio: [],
    sin_plataforma: [],
    en_perfil: false,
    exenta_por_contabilidad: false,
    nota: 'Los saldos en el extranjero a 2026-12-31 suman 20.930,00 €: no llegan a 50.000 €, no hay que presentar el 721 de 2026 (art. 42 quater.5.d RGAT).',
  },
  ops_ejercicio: 5,
  ops_sin_asiento: 2,
  ops_con_asiento: 3,
  deterioro_registrado: null,
  avisos: [
    'Eres persona física: en el IRPF las monedas que se transmiten son las adquiridas en primer lugar (FIFO; art. 37.2 LIRPF por analogía, DGT V0999-18). El método «precio medio ponderado» de los ajustes sólo se muestra como comparación; la Renta y los asientos usan FIFO.',
  ],
  inicio: null,
  antes_del_inicio: false,
  ops_historial: 0,
  huecos: 0,
};

/**
 * Los CSV de un texto pegado, con la misma regla que `descarga_csv::extract`: 16 letras y cifras;
 * con letras y cifras, o sólo letras si está sola en su línea y en mayúsculas. Nunca sólo cifras.
 */
export function demoExtractCsv(text: string): string[] {
  const out: string[] = [];
  for (const line of text.split(/\r?\n/)) {
    const alone = line.trim();
    for (const token of line.split(/[^A-Za-z0-9]/)) {
      if (token.length !== 16) continue;
      const upper = token.toUpperCase();
      const letters = /[A-Z]/.test(upper);
      const digits = /[0-9]/.test(upper);
      if (letters && (digits || (alone === token && token === upper)) && !out.includes(upper))
        out.push(upper);
    }
  }
  return out;
}

/** Lo «descargado» por CSV en la demo, para que salga en la carpeta (sin reconocer). */
const demoCsvSaved: string[] = [];

/** Conexión con los bancos: BBVA (empresa) y Revolut (empresa), datos inventados. */
const demoBanca: BancaEstado = {
  ajustes: { aceptado: true, url_vuelta: 'https://localhost/taxpain/banco' },
  app_id: '0a1b2c3d-0000-4000-8000-00000000a1b2',
  clave: true,
  vuelta_local: false,
  pendiente: null,
  conexiones: [
    {
      id: 1,
      banco: 'BBVA',
      pais: 'ES',
      tipo: 'business',
      estado: 'activa',
      hasta: '2027-04-21T09:30:00Z',
      dias: 172,
      actualizada: '2026-10-30T08:05:00Z',
      error: null,
      cuentas: [
        {
          id: 1,
          nombre: 'Estudio Demo SL',
          iban: 'ES91 …1332',
          divisa: 'EUR',
          tarjeta: false,
          activa: true,
          cuenta_id: 1,
          subcuenta: '5720001',
          hasta_dia: '2026-10-30',
          saldo: 1_874_350,
          saldo_fecha: '2026-10-29',
        },
        {
          id: 2,
          nombre: 'Tarjeta empresa',
          iban: null,
          divisa: 'EUR',
          tarjeta: true,
          activa: false,
          cuenta_id: null,
          subcuenta: null,
          hasta_dia: null,
          saldo: null,
          saldo_fecha: null,
        },
      ],
    },
    {
      id: 2,
      banco: 'Revolut',
      pais: 'ES',
      tipo: 'business',
      estado: 'activa',
      hasta: '2026-11-09T11:00:00Z',
      dias: 9,
      actualizada: '2026-10-30T08:05:00Z',
      error: null,
      cuentas: [
        {
          id: 3,
          nombre: 'Main',
          iban: 'LT12 …1000',
          divisa: 'EUR',
          tarjeta: false,
          activa: true,
          cuenta_id: 2,
          subcuenta: '5720002',
          hasta_dia: '2026-10-30',
          saldo: 412_090,
          saldo_fecha: '2026-10-30',
        },
      ],
    },
  ],
};

const demoActualizacion = (): BancaActualizacion => ({
  movimientos_nuevos: 3,
  ya_estaban: 12,
  cuentas: [
    {
      conexion: 1,
      banco: 'BBVA',
      nombre: 'ES91 …1332',
      error: null,
      resultado: {
        cuenta: 'BBVA …1332 EUR',
        cuenta_id: 1,
        cuenta_nueva: false,
        subcuenta: '5720001',
        formato: 'Conexión con el banco',
        clase: 'Cuenta',
        divisa: 'EUR',
        desde: '2026-10-20',
        hasta: '2026-10-30',
        movimientos: 15,
        movimientos_nuevos: 3,
        ya_estaban: 12,
        saldos_completados: 0,
        ya_contabilizados: 0,
        cuadre: { estado: 'sin_saldos' },
        importado: true,
        motivo: '',
        avisos: [],
      },
    },
    {
      conexion: 2,
      banco: 'Revolut',
      nombre: 'LT12 …1000',
      error: null,
      resultado: null,
    },
  ],
});

// Libros societarios de la demo: Estudio Demo SL (B12345674), socia única Carmen Española
// Española (12345678Z), 3.000 participaciones de 1 €. Todo inventado.
const DEMO_SOCIA = 'Carmen Española Española';
const demoMovements: ShareMovement[] = [
  {
    id: 1,
    date: '2024-01-15',
    kind: 'constitucion',
    from_nif: '',
    from_name: '',
    to_nif: '12345678Z',
    to_name: DEMO_SOCIA,
    to_address: 'Calle Mayor 1, 46001 València',
    to_nationality: 'española',
    number_from: 1,
    number_to: 3000,
    unit_nominal: 100,
    price: null,
    charge: '',
    title: 'Escritura de constitución',
    notary: 'Notaría de ejemplo',
    protocol: '100',
    title_date: '2024-01-15',
    notes: '',
  },
];
const demoContracts: SoleContract[] = [
  {
    id: 1,
    date: '2026-01-31',
    object: `Retribución de ${DEMO_SOCIA}, socio único, por el ejercicio del cargo de administrador`,
    amount: 2_400_000,
    conditions: 'Doce pagos de 2.000,00 € brutos. Importe máximo anual aprobado el 10/01/2026.',
    document: 'Decisión del socio único de 10/01/2026',
    source: 'nominas:2026:12345678Z',
  },
];
const demoMinutes: Minute[] = [
  {
    id: 1,
    number: 1,
    fiscal_year: 2026,
    date: '2026-01-10',
    place: 'València',
    kind: 'socio_unico',
    template: 'retribucion',
    attendees: `${DEMO_SOCIA} (NIF 12345678Z), socio único, titular de la totalidad de las participaciones sociales.`,
    chair: '',
    secretary: '',
    agreements: `${DEMO_SOCIA}, socio único de Estudio Demo SL, en ejercicio de las competencias de la junta general (art. 15 de la Ley de Sociedades de Capital), DECIDE:\n\nPRIMERO. Fijar en 24.000,00 € brutos el importe máximo de la remuneración anual del conjunto de los administradores para el ejercicio 2026 (art. 217.3 LSC).`,
    status: 'aprobada',
    approved_at: '2026-01-10T10:00:00Z',
  },
  {
    id: 2,
    number: null,
    fiscal_year: 2026,
    date: '2026-06-15',
    place: 'València',
    kind: 'socio_unico',
    template: 'cuentas',
    attendees: `${DEMO_SOCIA} (NIF 12345678Z), socio único.`,
    chair: '',
    secretary: '',
    agreements:
      'PRIMERO. Aprobar las cuentas anuales del ejercicio 2025.\n\nSEGUNDO. Aprobar la siguiente aplicación del resultado: a reservas voluntarias, 12.000,00 €.\n\nTERCERO. Aprobar la gestión del órgano de administración.',
    status: 'borrador',
    approved_at: null,
  },
];
// ---------- Préstamos (inventados) ----------

/** `2026-01-01` + `meses` meses (día 1 o 15: siempre existe). */
function demoMeses(iso: string, meses: number): string {
  const [y, m, d] = iso.split('-').map(Number);
  const total = (m ?? 1) - 1 + meses;
  const year = (y ?? 2026) + Math.floor(total / 12);
  const month = (total % 12) + 1;
  return `${year}-${String(month).padStart(2, '0')}-${String(d ?? 1).padStart(2, '0')}`;
}

function demoCuotas(
  inicio: string,
  principal: number,
  bp: number,
  per: number,
  n: number,
  lineal: boolean,
  retencion: number,
  estado: (q: number, fecha: string) => PrestamoCuota['estado'],
): PrestamoCuota[] {
  const out: PrestamoCuota[] = [];
  let pendiente = principal;
  const capitalLineal = Math.floor(principal / n);
  for (let k = 1; k <= n; k++) {
    const intereses = Math.round((pendiente * bp * per) / 120_000);
    const capital = k === n ? pendiente : lineal ? capitalLineal : 0;
    pendiente -= capital;
    const ret = Math.round((intereses * retencion) / 10_000);
    const fecha = demoMeses(inicio, per * k);
    out.push({
      n: k,
      fecha,
      intereses,
      capital,
      capitalizados: 0,
      pago: capital + intereses,
      pendiente,
      estado: estado(k, fecha),
      retencion: ret,
      neto: capital + intereses - ret,
      devengado: 0,
      bank_tx: null,
    });
  }
  return out;
}

function demoPrestamos(): PrestamoVista[] {
  const hoy = '2026-11-16';
  const participada = demoCuotas('2026-01-01', 4_000_000, 400, 3, 12, false, 1900, (k) =>
    k <= 3 ? 'registrada' : 'pendiente',
  );
  const banco = demoCuotas('2025-06-15', 6_000_000, 450, 1, 60, true, 0, (_k, f) =>
    f < '2026-01-01' ? 'fuera' : f <= '2026-10-31' ? 'registrada' : 'pendiente',
  );
  const socia = demoCuotas('2026-03-01', 1_000_000, 300, 12, 2, false, 1900, () => 'pendiente');
  const vivo = (c: PrestamoCuota[], p: number) =>
    c.filter((q) => q.fecha <= hoy).reduce((_, q) => q.pendiente, p);
  const vista = (
    prestamo: PrestamoVista['prestamo'],
    cuotas: PrestamoCuota[],
    avisos: string[],
  ): PrestamoVista => {
    const saldo = vivo(cuotas, prestamo.principal);
    const limite = demoMeses(hoy, 12);
    const corto = cuotas
      .filter((q) => q.fecha > hoy && q.fecha <= limite)
      .reduce((a, q) => a + q.capital, 0);
    return {
      prestamo,
      alta: prestamo.id === 2 ? 'fuera' : 'contabilizada',
      alta_bank_tx: prestamo.id === 1 ? 31 : null,
      cuotas,
      devengos: [],
      saldo_vivo: saldo,
      corto_plazo: Math.min(corto, saldo),
      largo_plazo: Math.max(saldo - corto, 0),
      vencidas: cuotas.filter((q) => q.estado === 'pendiente' && q.fecha <= hoy).length,
      avisos,
    };
  };
  return [
    vista(
      {
        id: 1,
        nombre: 'Préstamo a Participada Inventada',
        sentido: 'concedido',
        contraparte: {
          origen: 'participada',
          ref_id: null,
          nif: 'A22222228',
          nombre: 'Participada Inventada SA',
          provincia: '46',
          persona_fisica: false,
          entidad_credito: false,
          vinculo: 'grupo',
        },
        inicio: '2026-01-01',
        principal: 4_000_000,
        tipo: 400,
        sistema: 'intereses_periodicos',
        periodicidad: 3,
        plazo: 36,
        carencia: 0,
        carencia_tipo: 'capital',
        cuentas: { largo: '2423001', corto: '5323001', intereses: '762', devengo: '5343' },
        retencion: 1900,
        notas: '',
      },
      participada,
      [],
    ),
    vista(
      {
        id: 2,
        nombre: 'Préstamo del Banco Inventado',
        sentido: 'recibido',
        contraparte: {
          origen: 'texto',
          ref_id: null,
          nif: '',
          nombre: 'Banco Inventado',
          provincia: '',
          persona_fisica: false,
          entidad_credito: true,
          vinculo: 'ninguno',
        },
        inicio: '2025-06-15',
        principal: 6_000_000,
        tipo: 450,
        sistema: 'lineal',
        periodicidad: 1,
        plazo: 60,
        carencia: 0,
        carencia_tipo: 'capital',
        cuentas: { largo: '1700001', corto: '5200001', intereses: '662', devengo: '527' },
        retencion: 0,
        notas: '',
      },
      banco,
      [],
    ),
    vista(
      {
        id: 3,
        nombre: 'Préstamo de la socia',
        sentido: 'recibido',
        contraparte: {
          origen: 'socio',
          ref_id: null,
          nif: '12345678Z',
          nombre: DEMO_SOCIA,
          provincia: '46',
          persona_fisica: true,
          entidad_credito: false,
          vinculo: 'otra',
        },
        inicio: '2026-03-01',
        principal: 1_000_000,
        tipo: 300,
        sistema: 'intereses_periodicos',
        periodicidad: 12,
        plazo: 24,
        carencia: 0,
        carencia_tipo: 'capital',
        cuentas: { largo: '1635001', corto: '5135001', intereses: '662', devengo: '5145' },
        retencion: 1900,
        notas: '',
      },
      socia,
      [
        'Tipo del 3 % con una parte vinculada, por debajo del interés legal del dinero (3,25 %): no es el valor de mercado del art. 18 LIS, pero es la referencia que suele mirarse. Comprueba que es el que pactaríais con un tercero.',
      ],
    ),
  ];
}

const handlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  ...valoresHandlers,
  ...vinculadasHandlers,
  ...ivaHoldingHandlers,
  ...ossHandlers,
  ...censoHandlers,
  ...extranjeroHandlers,
  ...noResidentesHandlers,
  ...ajustesCierreHandlers,
  ...cuentasModeloHandlers,
  ...modelo200Handlers,
  ...subvencionesHandlers,
  ...trimestreHandlers,
  // Copias: React sólo vuelve a pintar si cambia el objeto.
  banca_estado: () => structuredClone(demoBanca),
  banca_ajustes: (args) => {
    demoBanca.ajustes = args?.ajustes as BancaEstado['ajustes'];
    return structuredClone(demoBanca);
  },
  banca_credenciales: (args) => {
    demoBanca.app_id = String(args?.appId ?? demoBanca.app_id);
    demoBanca.clave = true;
    return structuredClone(demoBanca);
  },
  banca_quitar_credenciales: () => {
    demoBanca.clave = false;
    return structuredClone(demoBanca);
  },
  banca_bancos: () => [
    {
      name: 'BBVA',
      country: 'ES',
      psu_types: ['business', 'personal'],
      maximum_consent_validity: 15_552_000,
      beta: false,
    },
    {
      name: 'Revolut',
      country: 'ES',
      psu_types: ['business', 'personal'],
      maximum_consent_validity: 15_552_000,
      beta: false,
    },
  ],
  banca_conectar: () => ({ escucha: false }),
  banca_esperar: () => null,
  banca_terminar: () => demoActualizacion(),
  banca_cancelar: () => structuredClone(demoBanca),
  banca_actualizar: () => demoActualizacion(),
  banca_activar_cuenta: (args) => {
    for (const c of demoBanca.conexiones)
      for (const k of c.cuentas) if (k.id === args?.cuenta) k.activa = Boolean(args?.activa);
    return structuredClone(demoBanca);
  },
  banca_desconectar: (args) => {
    demoBanca.conexiones = demoBanca.conexiones.filter((c) => c.id !== args?.conexion);
    return { estado: structuredClone(demoBanca), aviso: null };
  },

  app_info: () => ({ version: '0.1.0', keys_secure: true }),
  audit_rollback_acknowledge: () => false,
  companies_list: () => [
    { id: 'principal', name: 'Carmen Española Española', nif: '12345678Z', active: true },
    { id: '0a1b2c3d4e5f6071', name: 'Estudio Española SL', nif: 'B12345674', active: false },
  ],
  list_certificates: () => [certificate],
  company_update_id: (args) => args?.info,
  company_delete: () => {
    throw { message: 'En la demo no se borran empresas' };
  },
  companies_overview: () => [
    {
      id: 'principal',
      name: 'Carmen Española Española',
      nif: '12345678Z',
      active: true,
      status: {
        aeat_checked_at: '2026-10-27T18:30:00Z' as string | null,
        has_profile: true,
        overdue: [],
        upcoming: [
          {
            model: '303',
            label: 'Modelo 303 · 3.er trimestre 2026',
            description: 'IVA',
            period: '3T',
            fiscal_year: 2026,
            due_on: '2026-10-20',
            filed: false,
          },
        ],
      },
    },
    {
      id: '0a1b2c3d4e5f6071',
      name: 'Estudio Española SL',
      nif: 'B12345674',
      active: false,
      status: {
        aeat_checked_at: null as string | null,
        has_profile: true,
        overdue: [
          {
            model: '111',
            label: 'Modelo 111 · 2.º trimestre 2026',
            description: 'Retenciones',
            period: '2T',
            fiscal_year: 2026,
            due_on: '2026-07-20',
            filed: false,
          },
        ],
        upcoming: [],
      },
    },
  ],
  get_profile: () => profile,
  deduced_profile: () => ({
    perfil: { ...profile, persona: 'Juridica', irpf_method: null, pagos_fraccionados_is: true },
    motivos: ['el NIF es de una sociedad', '303 trimestral (2024 a 2026)', '202 (2024 a 2026)'],
  }),
  profile_presets: () => [
    ['Autónomo (estimación directa)', profile],
    ['Sociedad limitada', { ...profile, persona: 'Juridica', irpf_method: null }],
  ],
  ccaa_list: () => [['ComunitatValenciana', 'Comunitat Valenciana']],
  fiscal_calendar: () => calendar,
  vigencia_ejercicio: (args) => vigenciaDemo(Number(args?.year ?? 2027)),
  // Fuera de diciembre y enero no hay aviso; la fecha de la demo es de otoño.
  aviso_cambio_ejercicio: () => null,
  invoicing_settings: () => settings,
  invoices_list: () => invoices,
  invoice_preview: (args) => ({
    totals: {
      base: 180_000,
      vat: 37_800,
      surcharge: 0,
      withholding: 0,
      tax_total: 37_800,
      invoice_total: 217_800,
      payable: 217_800,
    },
    // En moneda extranjera, los mismos importes a 1 € = 1,1193 (datos sintéticos).
    eur_totals: (args?.draft as { currency?: unknown } | undefined)?.currency
      ? {
          base: 160_815,
          vat: 33_771,
          surcharge: 0,
          withholding: 0,
          tax_total: 33_771,
          invoice_total: 194_586,
          payable: 194_586,
        }
      : null,
    opened: false,
  }),
  fx_bce_rate: (args) => ({
    currency: args?.currency,
    requested: args?.date,
    date: args?.date,
    rate: '1.1193',
    cached: false,
  }),
  received_list: () => received,
  received_discard: (args) => {
    const r = received.find((x) => x.id === args?.id && x.status === 'pendiente');
    if (!r) throw { message: 'la factura no está pendiente' };
    r.status = 'descartada';
  },
  received_undiscard: (args) => {
    const r = received.find((x) => x.id === args?.id && x.status === 'descartada');
    if (!r) throw { message: 'la factura no está descartada' };
    r.status = 'pendiente';
  },
  received_register_typed: () => 1,
  received_check: (args) => demoReceivedCheck(args),
  received_review: (args) => demoStoreReviewed(args),
  received_correct: (args) => {
    demoStoreReviewed(args);
    return {
      avisos: [],
      rectificar: [{ modelo: '303', ejercicio: 2026, trimestre: 3 }],
      ejercicio_anterior: null,
      punteos_quitados: 0,
    };
  },
  received_unreview_check: () => [
    'Vuelve a «Pendientes de revisar» y se borra su asiento: deja de contar en el 303 del 4T 2026 y en el libro registro. Si la revisas otra vez, irá al trimestre en que la revises.',
  ],
  received_unreview: (args) => {
    const r = received.find((x) => x.id === args?.id && x.status === 'revisada');
    if (!r) throw { message: 'la factura no está revisada' };
    r.status = 'pendiente';
    r.reviewed = null;
    r.registered_on = null;
  },
  partners_list: () => [],
  consolidated_accounts: () => {
    throw new Error('En la demo no hay varias empresas');
  },
  settings_184: () => ({
    entity_type: 2,
    main_activity: 1,
    regime: 2,
    activity_kind: 1,
    iae: '',
    cash: false,
    representative_nif: '',
    representative_name: '',
    rent_situation: 1,
    cadastral_ref: '',
    urban: true,
    rent_days: 365,
  }),
  watched_folders: () => ['~/Documents/Facturas recibidas'],
  watched_pending: () => [
    {
      folder: '~/Documents/Facturas recibidas',
      path: '2026/3T/Extractos/extracto-septiembre.pdf',
      kind: 'Extracto bancario',
      year: 2026,
      seen: '2026-10-02',
    },
  ],
  journal: () => journal,
  leasing_list: () => [],
  prestamos_list: () => demoPrestamos(),
  prestamo_propuesta: (a) => {
    const sentido = a?.sentido as string;
    const c = a?.contraparte as { entidad_credito?: boolean; vinculo?: string } | undefined;
    const grupo = c?.vinculo === 'grupo';
    if (sentido === 'concedido')
      return {
        cuentas: grupo
          ? { largo: '2423001', corto: '5323001', intereses: '762', devengo: '5343' }
          : { largo: '2520001', corto: '5420001', intereses: '762', devengo: '547' },
        retencion: 1900,
        motivo: 'La sociedad prestataria retiene el 19 % de los intereses que paga (473).',
      };
    return c?.entidad_credito
      ? {
          cuentas: { largo: '1700001', corto: '5200001', intereses: '662', devengo: '527' },
          retencion: 0,
          motivo: 'Los intereses que se pagan a un banco no llevan retención.',
        }
      : {
          cuentas: { largo: '1710001', corto: '5210001', intereses: '662', devengo: '528' },
          retencion: 1900,
          motivo: 'La sociedad retiene el 19 % de los intereses que paga (4751).',
        };
  },
  prestamo_create: () => 3,
  prestamo_update: () => null,
  prestamo_bank_candidates: () => [],
  prestamo_post_start: () => 101,
  prestamo_start_outside: () => null,
  prestamo_undo_start: () => null,
  prestamo_post_installment: () => 102,
  prestamo_mark_outside: () => 0,
  prestamo_undo_installment: () => null,
  prestamo_accrue: () => [{ prestamo: 'Préstamo a Participada Inventada', n: 4, importe: 26_667 }],
  prestamo_undo_accrual: () => null,
  prestamos_reclassify: () => 1,
  prestamo_delete: () => null,
  prestamo_contract_save: () => null,
  model_adjustments: () => [],
  cartera_mismatches: () => [],
  payment_period: () => ({
    average: 3150,
    paid_ratio: 2800,
    pending_ratio: 4500,
    paid_total: 1_250_000,
    pending_total: 320_000,
  }),
  entry_documents: () => [],
  posting_accounts_get: () => ({
    revenue: '705',
    expense: '629',
    customers: '430',
    suppliers: '410',
    vat_output: '477',
    vat_input: '472',
    withholding_receivable: '473',
    withholding_payable: '4751',
    bank: '572',
    subaccount_width: 7,
  }),
  ...autonomoHandlers,
  ...nominasHandlers,
  shareholders_list: () => [],
  shareholder_save: () => 1,
  shareholder_delete: () => null,
  dividend_payments: () => [],
  dividend_propose: () => {
    throw { message: 'En la demo no hay dividendos acordados en la 526' };
  },
  dividend_pay: () => 1,
  dividend_undo: () => null,
  dividend_agreements: () => [],
  dividend_agreement_check: () => {
    throw {
      message: 'En la demo no hay contabilidad de una sociedad con la que comprobar el reparto',
    };
  },
  dividend_agreement_record: () => ({ id: 1, entry_id: 1, minute_id: 3 }),
  dividend_agreement_undo: () => null,
  partners_register: () => ({
    movements: demoMovements,
    situation: {
      count: 3000,
      capital: 300_000,
      holders: [
        {
          nif: '12345678Z',
          name: DEMO_SOCIA,
          address: 'Calle Mayor 1, 46001 València',
          nationality: 'española',
          blocks: [{ from: 1, to: 3000, unit_nominal: 100, charges: [] }],
          count: 3000,
          nominal: 300_000,
          pct: 1_000_000,
        },
      ],
    },
    checks: [
      `${DEMO_SOCIA} (12345678Z) tiene 3000 participaciones (100 %) en el libro y no está en la ficha de socios: dalo de alta allí (Cierre → Socios y dividendos) para repartirle dividendos.`,
    ],
    in_sync: false,
    capital_books: 300_000,
  }),
  share_movement_save: () => 2,
  share_movement_delete: () => null,
  partners_register_apply: () => 0,
  sole_contracts: () => demoContracts,
  sole_contract_proposals: () => ({
    sole_partner: ['12345678Z', DEMO_SOCIA],
    proposals: [
      {
        id: 0,
        date: '2026-07-01',
        object: `Dividendo a favor de ${DEMO_SOCIA}, socio único`,
        amount: 500_000,
        conditions: 'Bruto 5.000,00 €, retención 950,00 € (modelo 123), exigible el 01/07/2026.',
        document: 'Decisión del socio único (libro de actas)',
        source: 'dividendo:1',
      },
      {
        id: 0,
        date: '2026-03-01',
        object: `Préstamo de ${DEMO_SOCIA}, socio único, a la sociedad`,
        amount: 1_000_000,
        conditions:
          'Plazo de 24 meses, interés nominal anual del 3 %, 2 cuotas anuales por el sistema intereses periódicos y capital al vencimiento hasta el 01/03/2028, retención del 19 % sobre los intereses. Garantías: [ninguna o las que haya].',
        document:
          'Contrato de préstamo de 01/03/2026 (el borrador sale de Contabilidad → Préstamos)',
        source: 'prestamo:3',
      },
    ],
  }),
  sole_contract_save: () => 2,
  sole_contract_delete: () => null,
  minutes_list: () => demoMinutes,
  minute_template: (args) => ({
    minute: {
      ...demoMinutes[1],
      id: 0,
      number: null,
      date: (args?.date as string) ?? TODAY,
      template: (args?.template as { template?: string } | undefined)?.template ?? 'libre',
    },
    warnings: ['Demo: el borrador es de ejemplo.'],
    liquidity: null,
  }),
  minute_save: () => 3,
  minute_delete: () => null,
  minute_approve: () => 2,
  minute_reopen: () => null,
  minute_document_save: () => null,
  minute_certificate_save: () => null,
  corporate_book_save: () => null,
  director_turnover: () => ({ year: 2025, turnover: null, small: null, source: 'contabilidad' }),
  travel_list: () => [],
  travel_preview: () => ({
    total: 15_300,
    exempt: 15_300,
    not_income: 0,
    taxable: 0,
    km_limit: 9_100,
    meals_limit: 2_667,
    posted: 15_300,
    warnings: [],
  }),
  travel_register: () => 1,
  travel_delete: () => null,
  bank_rules: () => [],
  bank_rule_proposals: () => [],
  bank_rule_for: () => null,
  tax_payment: () => ({ payable: true, payment: null, candidates: [] }),
  fiscal_year_info: (a) => {
    const year = Number(a?.year ?? 2026);
    return {
      year,
      start: `${year}-01-01`,
      end: `${year}-12-31`,
      label: String(year),
      natural: true,
      closed: false,
    };
  },
  chart: () => [
    {
      code: '4300001',
      name: 'Estudio Norte Arquitectura SL',
      source: 'tercero',
      nif: 'B12345674',
      debit: 217_800,
      credit: 0,
    },
    {
      code: '5720001',
      name: 'Bancos e instituciones de crédito c/c vista, euros',
      source: 'pgc',
      nif: null,
      debit: 1_250_000,
      credit: 310_000,
    },
    {
      code: '705',
      name: 'Prestaciones de servicios',
      source: 'pgc',
      nif: null,
      debit: 0,
      credit: 1_840_000,
    },
  ],
  pgc_accounts: () => [
    ['572', 'Bancos e instituciones de crédito c/c vista, euros'],
    ['626', 'Servicios bancarios y similares'],
    ['629', 'Otros servicios'],
    ['705', 'Prestaciones de servicios'],
  ],
  a3_options: () => ({ company: 1, digits: 8, format: 'F5', invoices: true }),
  templates_list: () => [],
  invoice_numbering: (args) => [
    {
      serie: 'A',
      clase: 'facturas completas',
      year: Number(args?.year ?? 2026),
      emitidas: 6,
      anuladas: 0,
      primera: 'A2026-0001',
      ultima: 'A2026-0006',
      huecos: [],
      fuera_de_orden: [],
      otra_clase: [],
    },
    {
      serie: 'R',
      clase: 'rectificativas',
      year: Number(args?.year ?? 2026),
      emitidas: 1,
      anuladas: 0,
      primera: 'R2026-0001',
      ultima: 'R2026-0001',
      huecos: [],
      fuera_de_orden: [],
      otra_clase: [],
    },
  ],
  incobrables_list: () => [
    {
      id: null,
      invoice_id: 3,
      number: 'A2026-0003',
      cliente: 'Cliente Inventado SA',
      nif: 'A22222228',
      issue_date: '2026-02-10',
      devengo: '2026-02-10',
      total: 121_000,
      pendiente: 121_000,
      base_pendiente: 100_000,
      iva_pendiente: 21_000,
      plazo_meses: 6,
      estado: 'candidata',
      exclusion: 'ninguna',
      reclamacion_fecha: null,
      reclamacion_medio: null,
      reclamacion_ref: null,
      rectificativa: null,
      rectificativa_fecha: null,
      comunicada_el: null,
      comunicacion_ref: null,
      notas: '',
      calendario: {
        inicio: '2026-02-10',
        desde: '2026-08-10',
        limite_rectificar: '2027-02-10',
        limite_comunicar: null,
      },
      requisitos: [
        { clave: 'pendiente', cumplido: true, texto: 'Quedan 1.210,00 € sin cobrar.' },
        {
          clave: 'plazo',
          cumplido: true,
          texto: 'Ha pasado seis meses desde el devengo (2026-02-10).',
        },
        {
          clave: 'reclamacion',
          cumplido: false,
          texto:
            'Falta reclamar el cobro por vía judicial, por requerimiento notarial o por otro medio fehaciente (burofax con certificación de texto y acuse de recibo).',
        },
      ],
      puede_rectificar: false,
      avisos: ['Quedan 86 días para emitir la rectificativa (hasta el 2027-02-10).'],
      deterioro: null,
    },
  ],
  incobrable_open: () => 1,
  incobrable_adjust: () => null,
  incobrable_claim: () => null,
  incobrable_preview: () => ({
    draft: null,
    base: -100_000,
    iva: -21_000,
    requisitos: [],
    puede: false,
  }),
  incobrable_issue: () => ({
    id: 99,
    number: 'R2026-0002',
    totals: {
      base: -100_000,
      vat: -21_000,
      surcharge: 0,
      withholding: 0,
      tax_total: -21_000,
      invoice_total: -121_000,
      payable: -121_000,
    },
    verifactu_huella: null,
    qr_url: null,
    exported_to: null,
    export_error: null,
  }),
  incobrable_communication: () => ({
    campos: [
      ['NIF del emisor (acreedor)', 'B12345674'],
      ['Factura original', 'A2026-0003'],
    ],
    adjuntos: ['Copia de la factura rectificativa R2026-0002.'],
    plazo: '2026-12-16',
    donde: 'Sede electrónica de la AEAT. TaxPain no la envía.',
  }),
  incobrable_communicated: () => null,
  incobrable_close: () => 'Seguimiento cerrado.',
  cartera_list: () =>
    [
      {
        kind: 'emitida',
        invoice_id: 6,
        number: 'A2026-0006',
        party: 'Estudio Norte Arquitectura SL',
        issue_date: '2026-10-09',
        due_date: '2026-11-10',
        amount: 108_900,
        status: 'pendiente',
        settled_on: null,
        overdue_days: 0,
        ...one,
        ...recibo,
        party_account: '4300002',
      },
      {
        kind: 'emitida',
        invoice_id: 6,
        number: 'A2026-0006',
        party: 'Estudio Norte Arquitectura SL',
        issue_date: '2026-10-09',
        due_date: '2026-12-10',
        amount: 108_900,
        status: 'pendiente',
        settled_on: null,
        overdue_days: 0,
        ...one,
        ...recibo,
        seq: 2,
        party_account: '4300002',
      },
      {
        kind: 'recibida',
        invoice_id: 3,
        number: 'CR-2026/118',
        party: 'Coworking Ruzafa SL',
        issue_date: '2026-10-01',
        due_date: '2026-10-31',
        amount: 21_780,
        status: 'pendiente',
        settled_on: null,
        overdue_days: 0,
        ...one,
      },
      {
        kind: 'emitida',
        invoice_id: 4,
        number: 'A2026-0004',
        party: 'Taller Mediterráneo SL',
        issue_date: '2026-08-28',
        due_date: '2026-09-27',
        amount: 96_800,
        status: 'pendiente',
        settled_on: null,
        overdue_days: 16,
        ...one,
      },
      {
        kind: 'recibida',
        invoice_id: 2,
        number: 'F-2026-0912',
        party: 'Proveedor de Hosting SL',
        issue_date: '2026-09-05',
        due_date: '2026-10-05',
        amount: 4_840,
        status: 'banco',
        settled_on: '2026-10-03',
        overdue_days: 0,
        ...one,
      },
      {
        kind: 'emitida',
        invoice_id: 5,
        number: 'A2026-0005',
        party: 'Clínica Dental Turia SLP',
        issue_date: '2026-09-15',
        due_date: '2026-10-15',
        amount: 60_500,
        status: 'registrada',
        settled_on: '2026-10-10',
        overdue_days: 0,
        ...one,
      },
    ].map((i) => ({ ...i, invoice_amount: i.invoice_id === 6 ? 217_800 : i.amount })),
  party_terms_list: () => [
    {
      account: '4300002',
      role: 'cliente',
      tax_id: 'B12345674',
      name: 'Estudio Norte Arquitectura SL',
      terms: {
        days: [30, 60],
        pay_day: 10,
        method: 'recibo',
        treasury: '5720001',
        iban: 'ES7921000813610123456789',
        mandate_ref: 'EN-2026-01',
        mandate_date: '2026-01-15',
      },
    },
    {
      account: '4300001',
      role: 'cliente',
      tax_id: 'B87654321',
      name: 'Taller Mediterráneo SL',
      terms: null,
    },
    {
      account: '4100001',
      role: 'proveedor',
      tax_id: 'B11111111',
      name: 'Coworking Ruzafa SL',
      terms: null,
    },
  ],
  payment_terms: () => ({ customers_days: 30, suppliers_days: 30 }),
  sepa_settings: () => ({ creditor_suffix: '000', b2b: false }),
  remittance_banks: () => [
    { name: 'Cuenta principal (1332)', iban: 'ES9121000418450200051332', account: '5720001' },
  ],
  remittance_candidates: (args) =>
    args?.kind === 'cobros'
      ? [
          {
            item: {
              kind: 'emitida',
              invoice_id: 6,
              seq: 1,
              installments: 2,
              number: 'A2026-0006',
              party: 'Estudio Norte Arquitectura SL',
              party_account: '4300002',
              issue_date: '2026-10-09',
              due_date: '2026-11-10',
              amount: 108_900,
              invoice_amount: 217_800,
              status: 'pendiente',
              settled_on: null,
              overdue_days: 0,
              method: 'recibo',
              treasury: '5720001',
              fixed: false,
              remittance: null,
            },
            iban: 'ES7921000813610123456789',
            mandate_ref: 'EN-2026-01',
            mandate_date: '2026-01-15',
            problem: null,
          },
        ]
      : [],
  remittance_list: () => [],
  import_batches: () => [],
  stock_lines: () => [],
  patrimonial_report: (args) => demoPatrimonial(Number(args?.year ?? 2026)),
  patrimonial_mark: (args) => {
    demoPatMarcas[String(args?.cuenta)] = {
      clase: args?.clase as ClasePatrimonial,
      motivo: String(args?.motivo ?? ''),
    };
  },
  patrimonial_settings: (args) => {
    demoPatAjustes = args?.ajustes as AjustesPatrimonial;
  },
  patrimonial_sale_add: (args) => {
    const id = Math.max(0, ...demoPatVentas.map((v) => v.id)) + 1;
    demoPatVentas = [
      ...demoPatVentas,
      {
        id,
        fecha: String(args?.fecha),
        importe: Number(args?.importe),
        motivo: String(args?.motivo ?? ''),
      },
    ];
    return id;
  },
  patrimonial_sale_delete: (args) => {
    demoPatVentas = demoPatVentas.filter((v) => v.id !== Number(args?.id));
  },
  cripto_settings: () => criptoReport.settings,
  cripto_platforms: () => criptoPlatforms,
  cripto_ops: (args) =>
    criptoOps.filter((o) => args?.year == null || o.date.startsWith(String(args.year))),
  cripto_prices: () => criptoPrices,
  cripto_report: (args) => ({ ...criptoReport, year: Number(args?.year ?? 2026) }),
  cripto_inicio: () => null,
  cripto_save_inicio: () => [],
  cripto_delete_inicio: () => [],
  cripto_propuesta_inicio: () => ({
    fecha: '2025-12-31',
    ejercicio: 2025,
    motivo: '2025 es el último con el 200 presentado',
    posiciones: [{ ticker: 'BTC', platform_id: null, cantidad: '0.25', coste: 1_250_000 }],
    cuenta: '3090001',
    cuentas: [{ cuenta: '3090001', nombre: 'Criptomonedas BTC', saldo: 1_250_000 }],
    coste_diario: 1_250_000,
    casilla_102: null,
    hoja: null,
    fecha_cantidades: null,
    cantidades_de_operaciones: true,
    reparto_seguro: false,
    por_repartir: 0,
    polvo: [{ ticker: 'SHIB', platform_id: null, cantidad: '1250.5', coste: 0 }],
    origen: 'Cálculos de la gestoría: diario a 31/12/2025 (3090001 Criptomonedas BTC)',
    notas: [
      'Las cantidades salen de las operaciones importadas hasta el 31/12/2025: si faltan operaciones (exchanges que cerraron, DeFi), no son de fiar; corrígelas con las de la gestoría o importa su hoja de saldos.',
    ],
  }),
  cripto_assets: () => [
    { id: 1, ticker: 'BTC', name: 'Bitcoin', account: '3090001' },
    { id: 2, ticker: 'ETH', name: 'Ether', account: '3090002' },
  ],
  financial_ratios: () => [
    {
      id: 'fondo_maniobra',
      group: 'Liquidez',
      name: 'Fondo de maniobra',
      formula: 'Activo corriente − Pasivo corriente',
      explanation: 'Lo que queda del activo a corto plazo tras pagar las deudas a corto.',
      unit: 'euros',
      value: 1_845_000,
      previous: 1_210_000,
      range: [0, 9_000_000_000_000_000],
      status: 'bien',
    },
    {
      id: 'liquidez',
      group: 'Liquidez',
      name: 'Liquidez general',
      formula: 'Activo corriente / Pasivo corriente',
      explanation: 'Cuántas veces cubre el activo corriente las deudas a corto plazo.',
      unit: 'veces',
      value: 172,
      previous: 158,
      range: [150, 200],
      status: 'bien',
    },
    {
      id: 'roe',
      group: 'Rentabilidad',
      name: 'Rentabilidad financiera (ROE)',
      formula: 'Resultado / Patrimonio neto',
      explanation: 'Lo que rinde el dinero de los socios.',
      unit: 'porcentaje',
      value: 1420,
      previous: 1180,
      range: [800, 9_000_000_000_000_000],
      status: 'bien',
    },
    {
      id: 'pmc',
      group: 'Actividad',
      name: 'Periodo medio de cobro',
      formula: 'Clientes / Ventas × 365',
      explanation: 'Días que tardan de media los clientes en pagar.',
      unit: 'dias',
      value: 6800,
      previous: 5200,
      range: [0, 6000],
      status: 'atencion',
    },
  ],
  cost_centers: () => [
    { code: 'ESTUDIO', name: 'Estudio' },
    { code: 'FORMACION', name: 'Formación' },
  ],
  cost_rules: () => [
    {
      prefix: '62',
      shares: [
        { center: 'ESTUDIO', pct_bp: 8000 },
        { center: 'FORMACION', pct_bp: 2000 },
      ],
    },
  ],
  analytic_lines: () => [],
  cost_redistributions: () => [],
  analytic_report: () => [
    { center: 'ESTUDIO', name: 'Estudio', income: 1_250_000, expenses: 412_000, result: 838_000 },
    { center: 'FORMACION', name: 'Formación', income: 180_000, expenses: 103_000, result: 77_000 },
  ],
  budget_list: () => [
    { account: '705', center: '', months: Array(12).fill(120_000) },
    { account: '62', center: '', months: Array(12).fill(45_000) },
  ],
  budget_report: () => [
    {
      account: '705',
      name: 'Prestaciones de servicios',
      center: '',
      budget: 1_200_000,
      actual: 1_430_000,
      deviation: 230_000,
      deviation_bp: 1917,
      unfavourable: false,
    },
    {
      account: '62',
      name: 'Servicios exteriores',
      center: '',
      budget: 450_000,
      actual: 515_000,
      deviation: 65_000,
      deviation_bp: 1444,
      unfavourable: true,
    },
  ],
  templates_due: () => [],
  trial_balance: () => trial,
  ledger: () => [],
  fixed_assets: () => structuredClone(demoAssets),
  fixed_asset_priors: () => [],
  asset_sales: () => structuredClone(demoSales),
  asset_sale_candidates: (args) => demoCandidates(args?.id, args?.query),
  asset_dispose: (args) => ({
    depreciation_entry: 45,
    depreciation_removed: 0,
    disposal_entry: 46,
    sale_entry: args?.invoiceId ? 47 : null,
    result: 12_000,
    warnings: [],
  }),
  asset_sale_link: (args) => ({
    entry_id: 48,
    amount: typeof args?.amount === 'number' ? args.amount : 30_000,
    result_change: 0,
    warnings: [],
  }),
  asset_sale_unlink: () => null,
  asset_edit: () => [],
  asset_delete: () => null,
  asset_undispose: () => ({ disposal_entry: 46, restored: [45], removed: null, warnings: [] }),
  depreciation_preview: (args) => ({
    year: Number(args?.year ?? 2026),
    pendientes: [{ asset_id: 1, name: 'Portátil', amount: 37_500, entry_id: null }],
    total: 37_500,
    registradas: [],
    total_registrado: 0,
    cerrado: false,
  }),
  depreciation_undo: () => ({ borrados: 1, total: 12_000, saltados: [] }),
  asset_incentives: () => [],
  asset_incentive_usage: (args) => ({
    year: Number(args?.year ?? 2026),
    escaso_valor: 0,
    escaso_valor_tope: 2_500_000,
    erd_empleo: 0,
    erd_empleo_tope: null,
  }),
  asset_set_incentive: () => [],
  depreciation_differences: () => [],
  apertura_draft: () => aperturaDemo,
  apertura_save: () => null,
  apertura_discard: () => null,
  apertura_applied: () => [],
  gestor_lots: () => [
    {
      id: 1,
      fiscal_year: 2025,
      source: 'diario',
      filename: 'Diario 2025.xlsx',
      entries: 412,
      lines: 1_530,
      accounts_created: 38,
      has_opening: true,
      has_regularization: true,
      has_closing: false,
      next_opening: false,
      closed_year: false,
      created_at: '2026-10-26T10:00:00Z',
    },
  ],
  gestor_preview: () => ({
    source: 'diario',
    filename: 'Diario 2025.xlsx',
    fiscal_year: 2025,
    fiscal_year_label: '2025',
    entries: 412,
    lines: 1_530,
    first_date: '2025-01-01',
    last_date: '2025-12-31',
    total: 48_120_000,
    result: 845_000,
    has_opening: true,
    has_regularization: true,
    has_closing: false,
    next_opening: false,
    accounts: 41,
    new_accounts: [
      { code: '43000001', name: 'EMPRESA DEMO SL' },
      { code: '57200001', name: 'BANCO DEMO' },
    ],
    sample: [],
    errors: [],
    warnings: [
      'no viene el asiento de cierre (concepto «Cierre»): el ejercicio queda abierto y se cierra con TaxPain (Contabilidad → Cierre), que hará también la apertura del siguiente',
    ],
  }),
  gestor_undo: () => 414,
  gestor_compare_models: () => ({
    fiscal_year: 2025,
    rows: [
      {
        model: '303',
        period: '1T',
        box: 27,
        label: 'IVA devengado (477)',
        ours: 1_234_500,
        theirs: 1_234_500,
        diff: 0,
      },
      {
        model: '303',
        period: '1T',
        box: 45,
        label: 'IVA deducible (472)',
        ours: 412_300,
        theirs: 412_300,
        diff: 0,
      },
      {
        model: '200',
        period: '0A',
        box: 500,
        label: 'Resultado contable (grupos 6 y 7)',
        ours: 845_000,
        theirs: 845_000,
        diff: 0,
      },
    ],
    mismatches: 0,
    warnings: [],
  }),
  apertura_banks: () => [
    {
      account: '5720001',
      name: 'Cuenta corriente',
      iban: 'ES9121000418450200051332',
      currency: 'EUR',
      kind: 'cuenta',
      statement_balance: 1_874_350,
    },
  ],
  apertura_summary: () => ({
    fiscal_year: 2026,
    fiscal_year_label: '2026',
    fiscal_year_start: '2026-01-01',
    lines: aperturaDemo.lines.map((l) => ({ ...l, from: 'balance', new_account: false })),
    debit: 2_595_000,
    credit: 2_595_000,
    difference: 0,
    errors: [],
    warnings: [],
    duplicate: null,
    also: [],
  }),
  annual_accounts: () => annual,
  bank_movements: () => bank,
  // Dos extractos inventados de la cuenta profesional con un mes sin extracto entre medias.
  bank_statements: () => ({
    extractos: [
      {
        id: 1,
        cuenta_id: 1,
        cuenta: 'Cuenta profesional',
        divisa: 'EUR',
        fichero: 'extracto-agosto.xlsx',
        formato: 'Hoja de cálculo',
        desde: '2026-08-01',
        hasta: '2026-08-31',
        saldo_inicial: 1_000_000,
        saldo_final: 1_100_000,
        movimientos: 12,
        nuevos: 12,
        propios: 12,
        conciliados: 9,
        importado_en: '2026-09-02T08:00:00Z',
        heredado: false,
      },
      {
        id: 2,
        cuenta_id: 1,
        cuenta: 'Cuenta profesional',
        divisa: 'EUR',
        fichero: 'extracto-octubre.pdf',
        formato: 'PDF',
        desde: '2026-10-01',
        hasta: '2026-10-31',
        saldo_inicial: 1_180_000,
        saldo_final: 1_250_000,
        movimientos: 8,
        nuevos: 8,
        propios: 8,
        conciliados: 0,
        importado_en: '2026-11-03T08:00:00Z',
        heredado: false,
      },
    ],
    huecos: [
      {
        cuenta_id: 1,
        cuenta: 'Cuenta profesional',
        desde: '2026-09-01',
        hasta: '2026-09-30',
        seguro: true,
        detalle:
          'El extracto anterior acaba el 31/08/2026 con un saldo de 11.000,00 € y el siguiente empieza el 01/10/2026 con 11.800,00 €: faltan movimientos por 800,00 € entre medias.',
      },
    ],
  }),
  bank_statement_undo: () => ({ borrados: 8, desconciliados: 0, compartidos: 0 }),
  // Una cuenta en dólares inventada: 5.000 USD contabilizados por 4.500 € y el último tipo del
  // BCE descargado (1,0850) del 13/11/2026.
  bank_balances: () => [
    {
      name: 'Cuenta profesional',
      account: '5720001',
      currency: 'EUR',
      kind: 'cuenta',
      saldo: 1_250_000,
      contable: 1_250_000,
      tipo: null,
      tipo_fecha: null,
      al_tipo: null,
      sin_contravalor: 0,
    },
    {
      name: 'Cuenta en dólares',
      account: '5730001',
      currency: 'USD',
      kind: 'cuenta',
      saldo: 500_000,
      contable: 450_000,
      tipo: '1.085',
      tipo_fecha: '2026-11-13',
      al_tipo: 460_829,
      sin_contravalor: 0,
    },
  ],
  bank_set_fx: () => null,
  closing_fx_valuation: (args) => ({
    year: args?.year ?? 2026,
    date: '2026-12-31',
    reversal_date: '2027-01-01',
    closed: false,
    rates: [{ currency: 'USD', rate: '1.1', date: '2026-12-31', source: 'bce' }],
    items: [
      {
        kind: 'banco',
        account: '5730001',
        label: 'Cuenta en dólares',
        currency: 'USD',
        amount: 500_000,
        amount_source: 'extracto',
        booked: 450_000,
        valued: 454_545,
        difference: 4_545,
      },
    ],
    lines: [
      { account: '5730001', name: 'Cuenta en dólares', debit: 4_545, credit: 0 },
      {
        account: '768',
        name: 'Diferencias positivas de cambio',
        debit: 0,
        credit: 4_545,
      },
    ],
    gains: 4_545,
    losses: 0,
    entry_id: null,
    reversal_id: null,
    stale: false,
    input: { tipos: {}, saldos: {} },
    warnings: [],
  }),
  closing_fx_valuation_post: () => 1,
  closing_fx_valuation_undo: () => undefined,
  bank_import: () => [
    {
      fichero: 'extracto-octubre.pdf',
      resumen: {
        fichero: 'extracto-octubre.pdf',
        movimientos_nuevos: 3,
        ya_estaban: 0,
        extractos: [
          {
            cuenta: 'Cuenta profesional …1332 EUR',
            cuenta_id: 1,
            cuenta_nueva: false,
            subcuenta: '5720001',
            formato: 'PDF',
            clase: 'Cuenta',
            divisa: 'EUR',
            desde: '2026-10-01',
            hasta: '2026-10-11',
            movimientos: 3,
            movimientos_nuevos: 3,
            ya_estaban: 0,
            saldos_completados: 0,
            ya_contabilizados: 0,
            cuadre: { estado: 'cuadra' },
            importado: true,
            motivo: '',
            avisos: [],
          },
        ],
      },
      error: null,
    },
  ],
  bank_suggestions: () => [
    {
      kind: 'emitida',
      id: 6,
      number: 'A2026-0006',
      party: 'Estudio Norte Arquitectura SL',
      date: '2026-10-09',
      due_date: '2026-11-08',
      seq: 1,
      installments: 1,
      amount: 217_800,
    },
  ],
  detect_destinations: () => [{ kind: 'ICloudDrive', path: '~/iCloud Drive/TaxPain' }],
  new_installation_id: () => 'TP-demo',
  model_settings: () => ({
    iban: null,
    domiciliar: false,
    vat_compensation_carried: 0,
    request_refund: false,
    exempt_from_390: false,
    previous_year_net: null,
    corporate_tax_base: null,
    cnae: '',
    corporate_tax_rate: '',
    turnover_over_6m: false,
    turnover_under_1m: false,
    corporate_regime: 'General',
    corporate_fines: 0,
    accounts_model: 'Pymes',
    corporate_increases: 0,
    corporate_decreases: 0,
    capitalization_reserve: 0,
    pending_losses: 0,
  }),
  filed_returns: () => [],
  aeat_check_report: (a) => {
    const y = Number(a?.year ?? 2025);
    return demoAeatCheck(y, DEMO_CHECKED_YEARS.includes(y));
  },
  aeat_check_running: () => false,
  aeat_check_year: (a) => demoAeatCheck(Number(a?.year ?? 2025), true),
  aeat_check_summaries: () => DEMO_CHECKED_YEARS.map(demoYearSummary),
  aeat_check_target: () => ({
    company_nif: 'B12345674',
    company_name: 'ESTUDIO EJEMPLO, S.L.',
    cert_nif: 'B12345674',
    cert_name: 'ESTUDIO EJEMPLO, S.L.',
    same: true,
    other_entity: false,
  }),
  aeat_check_years: (a) => {
    const out = [];
    for (let y = Number(a?.from ?? 2025); y <= Number(a?.to ?? 2025); y++) {
      out.push({ year: y, summary: demoYearSummary(y), error: null });
    }
    return out;
  },
  aeat_check_save: (a) => [
    `~/Documents/Justificantes/Modelo_303_${a?.year}_1T_justificante_3030000000001.pdf`,
    `~/Documents/Justificantes/Modelo_303_${a?.year}_2T_justificante_3030000000003.pdf`,
  ],
  aeat_check_save_years: () => [
    '~/Documents/Justificantes/2025/Modelo_303_2025_1T_justificante_3030000000001.pdf',
  ],
  // Descargar de la AEAT por CSV: el primero se guarda, el segundo ya estaba, el tercero está
  // anulado (con el que lo sustituye) y el cuarto no existe. Códigos inventados.
  aeat_csv_running: () => false,
  aeat_csv_download: (a) => {
    const csvs = demoExtractCsv(String(a?.text ?? ''));
    if (csvs.length === 0) {
      throw {
        message:
          'No hay ningún código CSV en lo que has pegado. Un CSV son 16 letras y cifras, como ABCD1234EFGH5678.',
      };
    }
    return csvs.map((csv, i) => {
      switch (i % 4) {
        case 0: {
          const path = `Documentos AEAT/Requerimiento de información - ${csv}.pdf`;
          if (!demoCsvSaved.includes(path)) demoCsvSaved.push(path);
          return { csv, state: 'guardado', path };
        }
        case 1:
          return { csv, state: 'ya_estaba', path: `Documentos AEAT/${csv}.pdf` };
        case 2:
          return { csv, state: 'anulado', replacement: 'Z9Y8X7W6V5U4T3S2' };
        default:
          return { csv, state: 'no_encontrado' };
      }
    });
  },
  folder_get: () => '~/Documents/Gestoría',
  folder_scan: () => ({
    root: '~/Documents/Gestoría',
    skipped: 3,
    items: [
      {
        path: 'Empresa/Escritura de constitución.pdf',
        size: 1_402_112,
        year: null,
        importable: true,
        warnings: [],
        found: {
          kind: 'documento',
          doc_kind: 'escritura_constitucion',
          title: 'Escritura de constitución',
          by_name: false,
          facts: FACTS,
        },
      },
      {
        path: 'Empresa/Alta ROI.pdf',
        size: 88_112,
        year: null,
        importable: true,
        warnings: [],
        found: {
          kind: 'documento',
          doc_kind: 'alta_roi',
          title: 'Alta en el ROI (NIF-IVA, operaciones intracomunitarias)',
          by_name: false,
          facts: {
            ...FACTS,
            vat_number: 'ESB12345674',
            name: null,
            street: null,
            post_code: null,
            town: null,
            constitution_date: null,
            capital: null,
            administrator: null,
            notary: null,
            protocol: null,
            registry: null,
            date: '2019-05-02',
          },
        },
      },
      {
        path: '2025/Libro facturas emitidas 2025.xlsx',
        size: 48_211,
        year: 2025,
        importable: true,
        warnings: [],
        found: {
          kind: 'libro',
          book: 'emitidas',
          invoices: 64,
          first: '2025-01-08',
          last: '2025-12-29',
          base: 7_412_000,
          vat: 1_556_520,
        },
      },
      {
        path: '2025/Libro facturas recibidas 2025.xlsx',
        size: 39_874,
        year: 2025,
        importable: true,
        warnings: ['2 fila(s) sin fecha o sin número no se leen (totales, títulos…)'],
        found: {
          kind: 'libro',
          book: 'recibidas',
          invoices: 131,
          first: '2025-01-02',
          last: '2025-12-30',
          base: 2_186_400,
          vat: 402_300,
        },
      },
      {
        path: '2025/SUENLACE.DAT',
        size: 512_000,
        year: 2025,
        importable: true,
        warnings: [],
        found: {
          kind: 'diario',
          entries: 842,
          first: '2025-01-01',
          last: '2025-12-31',
          already: false,
        },
      },
      {
        path: '2025/Modelos/303_4T_2025.pdf',
        size: 91_002,
        year: 2025,
        importable: true,
        warnings: [],
        found: { kind: 'modelo', model: '303', year: 2025, period: '4T', source: 'pdf', boxes: 14 },
      },
      {
        path: '2025/Modelos/390_2025.txt',
        size: 9_830,
        year: 2025,
        importable: true,
        warnings: [],
        found: {
          kind: 'modelo',
          model: '390',
          year: 2025,
          period: '0A',
          source: 'fichero',
          boxes: 22,
        },
      },
      {
        path: '2025/Banco/sabadell_2025.n43',
        size: 61_440,
        year: 2025,
        importable: true,
        warnings: [],
        found: {
          kind: 'banco',
          resumen: 'Banco Sabadell …1332 EUR · 01/01/2025 a 31/12/2025 · 312 movimientos',
          last: '2025-12-31',
          cuadra: true,
        },
      },
      {
        path: '2025/Banco/revolut-2025.pdf',
        size: 88_210,
        year: 2025,
        importable: true,
        warnings: [],
        found: {
          kind: 'banco',
          resumen: 'Revolut …0718 EUR · 01/01/2025 a 30/06/2025 · 57 movimientos',
          last: '2025-06-30',
          cuadra: true,
        },
      },
      {
        path: '2025/Facturas recibidas/Coworking Ruzafa 2025-11.pdf',
        size: 120_331,
        year: 2025,
        importable: true,
        warnings: [],
        found: {
          kind: 'factura_recibida',
          supplier: 'Coworking Ruzafa SL',
          number: 'CR-2025-311',
          date: '2025-11-03',
          total: 30_250,
          in_book: true,
        },
      },
      {
        path: 'Escrituras constitución.pdf',
        size: 2_302_112,
        year: null,
        importable: false,
        warnings: [],
        found: { kind: 'otro', reason: 'no parece una factura ni un modelo' },
      },
      ...demoCsvSaved.map((path) => ({
        path,
        size: 52_430,
        year: null,
        importable: false,
        warnings: [],
        found: { kind: 'otro', reason: 'no parece una factura ni un modelo' },
      })),
    ],
  }),
  folder_apply: (args) =>
    ((args?.paths as string[]) ?? []).map((path) => ({
      path,
      ok: true,
      message: path.endsWith('.xlsx')
        ? 'facturas al libro'
        : path.endsWith('.DAT')
          ? '842 asientos (lote 1)'
          : 'importado',
    })),
  filed_list: () => [
    {
      id: 1,
      model: '303',
      year: 2025,
      period: '4T',
      filename: '303_4T_2025.pdf',
      source: 'pdf',
      boxes: { 7: 2_140_000, 9: 449_400 },
    },
    {
      id: 2,
      model: '390',
      year: 2025,
      period: '0A',
      filename: '390_2025.txt',
      source: 'fichero',
      boxes: {},
    },
  ],
  filed_compare: () => ({
    filed: {
      id: 1,
      model: '303',
      year: 2025,
      period: '4T',
      filename: '303_4T_2025.pdf',
      source: 'pdf',
      boxes: {},
    },
    mismatches: 4,
    warnings: ['Casillas leídas del PDF: comprueba con el documento que estén todas y bien'],
    rows: [
      {
        box: 7,
        ours: 2_140_000,
        theirs: 2_140_000,
        diff: 0,
        label: 'Régimen general · Base imponible',
      },
      { box: 9, ours: 449_400, theirs: 449_400, diff: 0, label: 'Régimen general · Cuota' },
      { box: 27, ours: 449_400, theirs: 449_400, diff: 0, label: 'Total cuota devengada' },
      {
        box: 28,
        ours: 512_300,
        theirs: 498_100,
        diff: 14_200,
        label: 'Por cuotas soportadas en operaciones interiores corrientes · Base',
        explanation:
          'TaxPain calcula 142,00 € más: IVA soportado deducible: compara las facturas recibidas (las que se deducen en otro periodo, las que no tienen IVA deducible, bienes de inversión, prorrata) (casilla leída del PDF: comprueba que se leyó bien).',
      },
      {
        box: 29,
        ours: 107_583,
        theirs: 104_601,
        diff: 2_982,
        label: 'Por cuotas soportadas en operaciones interiores corrientes · Cuota',
        explanation:
          'TaxPain calcula 29,82 € más: IVA soportado deducible: compara las facturas recibidas (casilla leída del PDF: comprueba que se leyó bien).',
      },
      {
        box: 45,
        ours: 107_583,
        theirs: 104_601,
        diff: 2_982,
        label: 'Total a deducir',
        explanation:
          'TaxPain calcula 29,82 € más: IVA soportado deducible: compara las facturas recibidas.',
      },
      {
        box: 71,
        ours: 341_817,
        theirs: 344_799,
        diff: -2_982,
        label: 'Resultado',
        explanation:
          'TaxPain calcula 29,82 € menos: es un resultado: la diferencia viene de las casillas de arriba.',
      },
    ],
    lines: [],
  }),
  filed_save_manual: () => 3,
  company_docs_list: () => [
    {
      id: 1,
      kind: 'escritura_constitucion',
      title: 'Escritura de constitución',
      filename: 'Escritura de constitución.pdf',
      facts: FACTS,
      date: '2019-03-12',
      imported_at: '2026-10-24T10:00:00Z',
    },
  ],
  company_suggestions: () => [
    {
      field: 'cnae',
      label: 'CNAE de la actividad',
      current: null,
      proposed: '6201',
      source: 'Escritura de constitución',
    },
    {
      field: 'intracomunitarias',
      label: 'Operaciones intracomunitarias (modelo 349 en el calendario)',
      current: 'No',
      proposed: 'Sí (dada de alta en el ROI con ESB12345674)',
      source: 'Alta en el ROI (NIF-IVA, operaciones intracomunitarias)',
    },
  ],
  company_apply_suggestion: () => null,
  company_doc_ai: () => FACTS,
  company_doc_open: () => null,
  company_doc_delete: () => null,
  folder_review: () => REVISION,
  company_review: () => ({
    ...REVISION,
    carpeta: '',
    hallazgos: DEMO_HALLAZGOS_AEAT,
    resumen: {
      ...REVISION.resumen,
      errores: 3,
      avisos: 4,
      info: 0,
      urgentes: 0,
      en_juego_urgente: 0,
    },
    tablas: [],
    pedir: [],
  }),
  company_evolution: () => DEMO_EVOLUCION,
  tax_savings: () => demoAhorro(),
  assistant_save: () => null,
  assistant_set_key: () => null,
  mcp_estado: () => ({ ajustes: { activado: false, nivel: 'consultar' }, binario: null }),
  mcp_guardar: (args) => ({ ajustes: args?.ajustes, binario: null }),
  assistant_topic_open: (args) => {
    const topic = args?.topic as AssistantTopic;
    return demoOpened(topic, demoFind(topic, null));
  },
  assistant_conversation_open: (args) => {
    const c = demoFind({ kind: 'general' }, Number(args?.id));
    return demoOpened(c?.topic ?? { kind: 'general' }, c);
  },
  assistant_conversations: () =>
    [...demoConversaciones]
      .sort((a, b) => b.updated_at.localeCompare(a.updated_at))
      .map(demoSummary),
  assistant_topic_preview: (args) => ({
    system: demoSystem(args?.topic as AssistantTopic),
    messages: [
      { role: 'user', content: (args?.question as string | null) ?? '(aquí irá tu pregunta)' },
    ],
    replaced: 2,
    omitted: 0,
  }),
  assistant_topic_ask: async (args) => {
    await new Promise((r) => setTimeout(r, 600));
    const topic = args?.topic as AssistantTopic;
    const question = String(args?.question ?? '').trim();
    if (!question) throw { message: 'Escribe tu pregunta.' };
    const c = demoEnsure(
      topic,
      (args?.conversationId as number | null) ?? null,
      demoTitle(topic, question),
    );
    if (c.topic.kind === 'general' && c.title.startsWith('Conversación del '))
      c.title = demoTitle(topic, question);
    const reply = demoReply(topic, c);
    const at = new Date().toISOString();
    const seq = c.messages[c.messages.length - 1]?.seq ?? 0;
    c.messages.push(
      { seq: seq + 1, role: 'user', content: question, created_at: at, notes: null },
      { seq: seq + 2, role: 'assistant', content: reply.text, created_at: at, notes: reply.notes },
    );
    c.updated_at = at;
    return demoOpened(topic, c);
  },
  assistant_fact_add: (args) => {
    const text = String(args?.text ?? '').trim();
    if (!text) throw { message: 'Escribe el dato.' };
    return demoAddMaterial(args, {
      kind: 'dato',
      content: text,
      filename: null,
      read_by: null,
      truncated: false,
    });
  },
  assistant_file_add: (args) =>
    demoAddMaterial(args, {
      kind: 'documento',
      content: DEMO_DOCUMENTO,
      filename:
        String(args?.path ?? '')
          .split('/')
          .pop() ?? 'documento.pdf',
      read_by: 'mac',
      truncated: false,
    }),
  assistant_material_delete: (args) => {
    const c = demoFind({ kind: 'general' }, Number(args?.conversationId));
    if (!c) throw { message: 'Esta conversación ya no existe: se ha borrado.' };
    const before = c.materials.length;
    c.materials = c.materials.filter((m) => m.id !== Number(args?.materialId));
    if (c.materials.length === before) throw { message: 'Eso ya no está en la conversación.' };
    return structuredClone(c);
  },
  assistant_conversation_delete: (args) => {
    const i = demoConversaciones.findIndex((c) => c.id === Number(args?.id));
    if (i < 0) throw { message: 'Esta conversación ya no existe: se ha borrado.' };
    demoConversaciones.splice(i, 1);
    return null;
  },
  assistant_conversations_delete_all: () => demoConversaciones.splice(0).length,
  tax_savings_mark: (args) => {
    demoMarcas[String(args?.id)] = args?.marca as MarcaAhorro;
    return null;
  },
  // La última revisión guardada: la de la demo (el Panel enseña su urgente).
  folder_review_last: () => REVISION,
  folder_review_save: () => null,
  lectura_ia_get: () => ({ full: false, model: null, accepted_at: null }),
  lectura_ia_save: (args) => args?.settings,
  folder_ai_read: () => ({ read: 3, reused: 0, failed: [] }),
  recurring_list: () => [
    {
      id: 'a1b2c3d4e5f60718',
      name: 'Estudio Gráfico Turia SL · Mantenimiento web',
      draft: {
        kind: 'Completa',
        series: null,
        issue_date: '2026-09-01',
        operation_date: null,
        recipient: {
          id: { Nif: 'B00123455' },
          name: 'Estudio Gráfico Turia SL',
          address: null,
          person: null,
        },
        lines: [
          {
            description: 'Mantenimiento web {mes}',
            quantity_milli: 1000,
            unit_price: 30_000,
            discount: 0,
            vat: { Taxed: { rate: 2100 } },
            surcharge: null,
            withholding: null,
          },
        ],
        rectified: [],
        rectified_amounts: null,
        description: 'Mantenimiento web {mes}',
        notes: null,
      },
      language: null,
      every_months: 1,
      start: '2026-10-01',
      issued: 0,
      end: null,
      active: true,
      last: ['A2026-0004', '2026-09-01'],
      next_date: '2026-10-01',
      due: true,
      client: 'Estudio Gráfico Turia SL',
    },
  ],
  recurring_preview: () => ({
    draft: {
      kind: 'Completa',
      series: null,
      issue_date: '2026-10-24',
      operation_date: '2026-10-01',
      recipient: {
        id: { Nif: 'B00123455' },
        name: 'Estudio Gráfico Turia SL',
        address: null,
        person: null,
      },
      lines: [
        {
          description: 'Mantenimiento web octubre 2026',
          quantity_milli: 1000,
          unit_price: 30_000,
          discount: 0,
          vat: { Taxed: { rate: 2100 } },
          surcharge: null,
          withholding: null,
        },
      ],
      rectified: [],
      rectified_amounts: null,
      description: 'Mantenimiento web octubre 2026',
      notes: null,
    },
    totals: {
      base: 30_000,
      vat: 6_300,
      surcharge: 0,
      withholding: 0,
      tax_total: 6_300,
      invoice_total: 36_300,
      payable: 36_300,
    },
    late_vat: null,
  }),
  recurring_issue: () => ({ id: 99, number: 'A2026-0007' }),
  recurring_create: (args) => ({ every_months: args?.everyMonths ?? 1, start: '2026-11-09' }),
  recurring_update: () => null,
  recurring_delete: () => null,
  invoice_email_compose: (args) => ({
    to: 'admin@turia.example',
    subject: args?.reminder ? 'Recordatorio de pago: factura A2026-0006' : 'Factura A2026-0006',
    body: args?.reminder
      ? 'Hola:\n\nSegún nuestros registros, la factura A2026-0006, del 09/10/2026, sigue pendiente de pago por 2.178,00 €. Si ya se ha pagado, rogamos que no se tenga en cuenta este mensaje; si no, agradeceríamos el pago en los próximos días.\n\nDatos de pago: IBAN ES00 0000 0000 0000 0000 0000\n\nUn saludo,\nEstudio Demo SL\n'
      : 'Hola:\n\nAdjuntamos la factura A2026-0006, del 09/10/2026, por un importe de 2.178,00 €. Vence el 08/11/2026.\n\nDatos de pago: IBAN ES00 0000 0000 0000 0000 0000\n\nUn saludo,\nEstudio Demo SL\n',
    attachments: ['A2026-0006.pdf'],
    language: 'Es',
    reminder: args?.reminder ? 1 : null,
    last_reminder: null,
    pending: 217_800,
    overdue: args?.reminder ? 217_800 : 0,
    overdue_days: args?.reminder ? 12 : 0,
  }),
  invoice_email_open: () => null,
  invoice_language_of: () => null,
  global_search: (args) => {
    const q = String(args?.query ?? '').toLowerCase();
    const all = [
      {
        kind: 'emitida',
        title: 'A2026-0006 · Estudio Norte Arquitectura SL',
        subtitle: 'Factura emitida · 2026-10-09 · 2.178,00 €',
        target: '6',
        date: '2026-10-09',
      },
      {
        kind: 'contacto',
        title: 'Estudio Gráfico Turia SL',
        subtitle: 'Cliente · B00123455',
        target: 'B00123455',
        date: null,
      },
      {
        kind: 'cuenta',
        title: '572 Bancos e instituciones de crédito c/c vista, euros',
        subtitle: 'Cuenta',
        target: '572',
        date: null,
      },
      {
        kind: 'asiento',
        title: 'Factura A2026-0006 Estudio Norte Arquitectura SL',
        subtitle: 'Asiento 41/2026 · 2026-10-09 · 2.178,00 €',
        target: '41:2026',
        date: '2026-10-09',
      },
    ];
    return all.filter((h) => `${h.title} ${h.subtitle}`.toLowerCase().includes(q));
  },
  closing_checklist: (args) => ({
    year: args?.year ?? 2026,
    start: '2026-01-01',
    end: '2026-12-31',
    closed: false,
    company: true,
    lsc: true,
    items: [
      {
        id: 'recibidas',
        label: 'Facturas recibidas revisadas',
        ok: false,
        detail: 'Quedan 2 por revisar (Recibidas): sin revisar no cuentan.',
      },
      {
        id: 'banco',
        label: 'Banco conciliado',
        ok: false,
        detail: '5 movimientos sin conciliar (Contabilidad → Banco).',
      },
      {
        id: 'amortizaciones',
        label: 'Amortizaciones del ejercicio',
        ok: true,
        detail: 'Contabilizadas.',
      },
      {
        id: 'verifactu',
        label: 'Registros VERI*FACTU enviados',
        ok: true,
        detail: 'No queda ninguno pendiente.',
      },
      {
        id: 'iva',
        label: 'IVA del último periodo liquidado',
        ok: false,
        detail: 'Contabiliza la liquidación del último 303 (Contabilidad → IVA y libros).',
      },
      {
        id: 'is',
        label: 'Impuesto sobre Sociedades contabilizado',
        ok: false,
        detail: 'Contabilízalo antes de cerrar (abajo, desde el borrador del 200).',
      },
      {
        id: 'cierre',
        label: 'Ejercicio cerrado',
        ok: false,
        detail: 'Cuando todo lo anterior esté, cierra el ejercicio.',
      },
    ],
    deadlines: [
      { label: 'Legalizar los libros en el Registro Mercantil (4 meses)', date: '2027-04-30' },
      {
        label: 'Junta que aprueba las cuentas y la aplicación del resultado (6 meses)',
        date: '2027-06-30',
      },
      { label: 'Modelo 200 (25 días naturales tras los 6 meses)', date: '2027-07-25' },
      { label: 'Depósito de las cuentas (1 mes tras la junta; como tarde)', date: '2027-07-31' },
    ],
  }),
  // Sin saldos fantasma en la 4700 y la 4750: no se propone el ajuste.
  closing_vat_balances: () => null,
  closing_corporate_tax: () => ({
    year: 2026,
    date: '2026-12-31',
    due: '2027-07-25',
    tax: 574_294,
    withholdings: 13_800,
    installments: 120_000,
    result: 440_494,
    lines: [
      { account: '6300', name: 'Impuesto corriente', debit: 574_294, credit: 0 },
      {
        account: '473',
        name: 'Hacienda Pública, retenciones y pagos a cuenta',
        debit: 0,
        credit: 133_800,
      },
      {
        account: '4752',
        name: 'Hacienda Pública, acreedora por impuesto sobre sociedades',
        debit: 0,
        credit: 440_494,
      },
    ],
    entry_id: null,
    stale: false,
    warnings: [],
  }),
  closing_corporate_tax_post: () => 1,
  closing_corporate_tax_undo: () => null,
  closing_allocation: () => {
    throw {
      message: 'cierra antes el ejercicio 2026: el resultado se aplica con las cuentas cerradas',
    };
  },
  folder_book_imports: () => [],
  folder_book_undo: () => 0,
  quotes_list: () => [
    {
      id: 3,
      kind: 'presupuesto',
      number: 'P2026-0003',
      issue_date: '2026-10-20',
      valid_until: '2026-11-19',
      status: 'enviado',
      recipient_name: 'Atelier Lumière SAS',
      total: 450_000,
      draft: {
        kind: 'Completa',
        series: null,
        issue_date: '2026-10-20',
        operation_date: null,
        recipient: null,
        lines: [],
        rectified: [],
        rectified_amounts: null,
        description: 'Rediseño de la web',
        notes: null,
      },
      language: 'En',
      invoice_id: null,
      invoice_number: null,
      expired: false,
    },
    {
      id: 2,
      kind: 'presupuesto',
      number: 'P2026-0002',
      issue_date: '2026-09-28',
      valid_until: '2026-10-28',
      status: 'aceptado',
      recipient_name: 'Estudio Gráfico Turia SL',
      total: 181_500,
      draft: {
        kind: 'Completa',
        series: null,
        issue_date: '2026-09-28',
        operation_date: null,
        recipient: null,
        lines: [],
        rectified: [],
        rectified_amounts: null,
        description: 'Campaña de otoño',
        notes: null,
      },
      language: null,
      invoice_id: null,
      invoice_number: null,
      expired: false,
    },
    {
      id: 1,
      kind: 'presupuesto',
      number: 'P2026-0001',
      issue_date: '2026-06-02',
      valid_until: '2026-07-02',
      status: 'facturado',
      recipient_name: 'Clínica Dental Ruzafa SL',
      total: 72_600,
      draft: {
        kind: 'Completa',
        series: null,
        issue_date: '2026-06-02',
        operation_date: null,
        recipient: null,
        lines: [],
        rectified: [],
        rectified_amounts: null,
        description: 'Fotografía',
        notes: null,
      },
      language: null,
      invoice_id: 2,
      invoice_number: 'A2026-0002',
      expired: false,
    },
  ],
  quote_create: () => null,
  quote_update: () => null,
  quote_status: () => null,
  quote_delete: () => null,
  quote_open: () => null,
  quote_save: () => null,
  quote_issue_invoice: () => {
    throw { message: 'En la demo no se emiten facturas' };
  },
  quote_email_compose: () => ({
    to: 'admin@turia.example',
    subject: 'Presupuesto P2026-0002',
    body: 'Hola:\n\nAdjuntamos el presupuesto P2026-0002 por un importe de 1.815,00 €.\n\nUn saludo,\nEstudio Demo SL\n',
    attachments: ['P2026-0002.pdf'],
    language: 'Es',
    reminder: null,
    last_reminder: null,
    pending: 181_500,
    overdue_days: 0,
  }),
  quote_email_open: () => null,
  vies_check: (args) => ({
    country: args?.country,
    vat_number: args?.vat,
    valid: true,
    name: 'ATELIER LUMIERE SAS',
    address: null,
    request_id: 'WAPIAAAAZDEMO1',
    request_date: '2026-10-24T10:00:00Z',
  }),
  autostart_get: () => false,
  autostart_set: (args) => args?.enabled ?? false,
  memoria_save: () => null,
  contacts_list: () => CONTACTS,
  contact_detail: (args) => {
    const row = CONTACTS.find((c) => c.key === args?.key) ?? CONTACTS[0];
    return {
      row,
      data: row?.id
        ? {
            party: {
              id: { Nif: row.tax_id },
              name: row.name,
              address: null,
              person: null,
            },
            email: row.email,
            language: null,
            notes: null,
          }
        : null,
      party: null,
      history: [
        {
          kind: 'emitida',
          id: 12,
          number: 'A2026-0012',
          date: '2026-10-15',
          total: 363_000,
          status: 'emitida',
          pending: 363_000,
        },
        {
          kind: 'emitida',
          id: 9,
          number: 'A2026-0009',
          date: '2026-09-15',
          total: 363_000,
          status: 'emitida',
          pending: 0,
        },
      ],
      years: [
        { year: 2026, invoiced: 2_904_000, purchased: 0 },
        { year: 2025, invoiced: 1_815_000, purchased: 0 },
      ],
    };
  },
  contact_save: () => 1,
  contact_delete: () => null,
  contacts_search: () => [],
  backup_status: () => ({
    has_passphrase: true,
    settings: {
      folder:
        '/Users/demo/Library/Mobile Documents/com~apple~CloudDocs/TaxPain/Copias de seguridad',
      auto: true,
      every_days: 7,
      keep: 10,
    },
    last: {
      at: '2026-10-20T18:30:00Z',
      path: '/Users/demo/Library/Mobile Documents/com~apple~CloudDocs/TaxPain/Copias de seguridad/Estudio-Demo-SL-2026-10-20-auto.taxpain',
      bytes: 1_843_200,
      auto: true,
    },
    last_error: null,
    days_since: 4,
    remind: false,
    suggested_folder:
      '/Users/demo/Library/Mobile Documents/com~apple~CloudDocs/TaxPain/Copias de seguridad',
    suggested_name: 'Estudio-Demo-SL-2026-10-24.taxpain',
    recovery_code: true,
    setup_required: false,
    restored: null,
    min_passphrase: 10,
  }),
  backup_set_passphrase: () => null,
  vault_locked: () => null,
  recuperacion_empresas: () => [],
  recuperacion_rescatar: (args) => ({
    fichero: String(args?.path ?? 'rescate.zip')
      .split('/')
      .pop(),
    originales: ['taxpain.db', 'audit.head'],
    tablas: 84,
    filas: 1200,
    sin_volcado: null,
  }),
  recuperacion_restaurar: () => null,
  integridad_empresa: () => [],
  auditoria_acciones: () => [
    ['invoice.issue', 24],
    ['journal.manual', 9],
    ['periodo.cerrar', 3],
    ['backup.create', 6],
  ],
  auditoria_consultar: () => ({
    entradas: [
      {
        seq: 42,
        at: '2026-10-20T09:15:00Z',
        actor: 'user',
        action: 'periodo.cerrar',
        subject: '3T 2026',
        details: '{"motivo":"303 presentado"}',
        prev_hash: 'a'.repeat(64),
        hash: 'b'.repeat(64),
      },
      {
        seq: 41,
        at: '2026-10-15T11:00:00Z',
        actor: 'user',
        action: 'invoice.issue',
        subject: 'F-2026-0041',
        details: '{"total":121000}',
        prev_hash: 'c'.repeat(64),
        hash: 'a'.repeat(64),
      },
    ],
    total: 2,
    siguiente: null,
  }),
  auditoria_verificar: () => ({
    ok: true,
    entradas: 42,
    rota_en: null,
    mensaje:
      'La cadena de huellas está completa: las 42 entradas encadenan bien, ninguna se ha cambiado ni borrado.',
  }),
  periodos_listar: () => [
    {
      id: 1,
      tipo: 'trimestre',
      ejercicio: 2026,
      trimestre: 3,
      desde: '2026-07-01',
      hasta: '2026-09-30',
      motivo: '303 presentado',
      cerrado_en: '2026-10-20T09:15:00Z',
      etiqueta: '3T 2026',
    },
  ],
  periodos_sugeridos: () => [
    {
      tipo: 'ejercicio',
      ejercicio: 2025,
      trimestre: null,
      etiqueta: 'ejercicio 2025',
      motivo: 'ejercicio cerrado en la contabilidad',
    },
  ],
  periodo_cerrar: (args) => ({
    id: 2,
    tipo: args?.tipo,
    ejercicio: args?.ejercicio,
    trimestre: args?.trimestre ?? null,
    desde: `${args?.ejercicio}-01-01`,
    hasta: `${args?.ejercicio}-12-31`,
    motivo: args?.motivo ?? '',
    cerrado_en: '2026-11-17T10:00:00Z',
    etiqueta: `ejercicio ${args?.ejercicio}`,
  }),
  periodo_reabrir: () => null,
  periodo_autorizar: () => null,
  archivo_conservacion: () => [
    {
      ejercicio: 2025,
      cierre: '2025-12-31',
      fin_mercantil: '2031-12-31',
      fin_fiscal: '2030-07-25',
      fin_creditos: null,
      creditos_pendientes: false,
      conservar_hasta: '2031-12-31',
      destruible: false,
      motivo: 'seis años del Código de Comercio (art. 30): hasta el 2031-12-31',
      archivado: null,
    },
    {
      ejercicio: 2026,
      cierre: '2026-12-31',
      fin_mercantil: '2032-12-31',
      fin_fiscal: '2031-07-25',
      fin_creditos: null,
      creditos_pendientes: false,
      conservar_hasta: '2032-12-31',
      destruible: false,
      motivo: 'seis años del Código de Comercio (art. 30): hasta el 2032-12-31',
      archivado: null,
    },
  ],
  archivo_aviso: () => ({ destruibles: [], conservar: [], sin_archivo: [2025] }),
  archivo_ejercicio_contenido: () => [
    {
      clave: 'diario',
      titulo: 'Diario, mayor y sumas y saldos',
      estado: 'si',
      detalle: '6 ficheros',
      ficheros: 6,
    },
    {
      clave: 'emitidas',
      titulo: 'Facturas emitidas',
      estado: 'si',
      detalle: '48 ficheros',
      ficheros: 48,
    },
    {
      clave: 'bancos',
      titulo: 'Extractos bancarios',
      estado: 'parcial',
      detalle: 'TaxPain guarda los movimientos, no los extractos originales del banco',
      ficheros: 1,
    },
  ],
  archivo_ejercicio_exportar: (args) => ({
    fichero: String(args?.path ?? 'Archivo.zip')
      .split('/')
      .pop(),
    sha256: '0'.repeat(64),
    bytes: 2_400_000,
    ficheros: 64,
    partes: [],
  }),
  exportacion_completa: (args) => ({
    fichero: String(args?.path ?? 'export.zip')
      .split('/')
      .pop(),
    sha256: '0'.repeat(64),
    bytes: 5_200_000,
    tablas: 84,
    filas: 3400,
    adjuntos: 120,
    con_error: [],
  }),
  backup_verify_passphrase: () => true,
  backup_save_settings: (args) => args?.settings,
  backup_create: (args) => ({
    path: args?.path,
    bytes: 1_843_200,
    created_at: '2026-10-24T10:00:00Z',
    sha256: '0'.repeat(64),
    audit_entries: 120,
  }),
  backup_inspect: () => ({
    created_at: '2026-10-20T18:30:00Z',
    app_version: '0.1.0',
    company: 'Estudio Demo SL',
    nif: 'B12345674',
    invoices_issued: 42,
    invoices_received: 87,
    journal_entries: 310,
    last_invoice_date: '2026-10-15',
    newer_version: false,
    same_nif_company: null,
  }),
  backup_restore: () => ({
    id: '0123456789abcdef',
    name: 'Estudio Demo SL',
    nif: 'B12345674',
    active: false,
  }),
  restore_check: () => null,
  restore_acknowledge: () => null,
  verifactu_queue_status: () => ({
    entorno: 'pruebas',
    sin_produccion:
      'esta versión de TaxPain sólo envía al entorno de pruebas de la AEAT (el envío a producción está cerrado hasta completar la prueba con certificado real)',
    pendientes: 0,
    sin_respuesta: 0,
    rechazados: 0,
    aceptados_con_errores: 0,
    aceptados_pruebas: 4,
    remitidos: 0,
    espera_segundos: 0,
    proximo_envio: null,
    tiempo_espera: 60,
    incidencia_desde: null,
    fallos_seguidos: 0,
    ultimo_intento: null,
    ultimo_resultado: 'Correcto',
    ultimo_error: null,
    bloqueada: false,
    aviso: null,
    aviso_modo: null,
  }),
  verifactu_queue_reopen: () => null,
  declaracion_responsable: () => ({
    titulo: 'DECLARACIÓN RESPONSABLE DEL SISTEMA INFORMÁTICO DE FACTURACIÓN',
    version: '0.1.0',
    apartados: [
      { letra: 'a', etiqueta: 'Nombre del sistema informático de facturación', valor: 'TaxPain' },
      { letra: 'b', etiqueta: 'Código identificador del sistema informático', valor: 'TP' },
      { letra: 'c', etiqueta: 'Identificador completo de la versión', valor: '0.1.0' },
      {
        letra: 'd',
        etiqueta: 'Componentes hardware y software, descripción y funcionalidades',
        valor: 'Aplicación de escritorio para macOS con base de datos local cifrada.',
      },
      { letra: 'e', etiqueta: 'Sólo puede funcionar como VERI*FACTU', valor: 'Sí' },
      { letra: 'f', etiqueta: 'Permite su uso por varios obligados tributarios', valor: 'No' },
      { letra: 'g', etiqueta: 'Tipos de firma utilizados', valor: 'No aplica' },
      { letra: 'h', etiqueta: 'Nombre o razón social del productor', valor: 'Estudio Demo SL' },
      { letra: 'i', etiqueta: 'NIF del productor', valor: 'B12345674' },
      {
        letra: 'j',
        etiqueta: 'Dirección postal completa de contacto del productor',
        valor: '[dirección postal completa]',
      },
      {
        letra: 'k',
        etiqueta: 'Manifestación de cumplimiento',
        valor:
          'Estudio Demo SL declara que el sistema cumple el art. 29.2.j) LGT, el RD 1007/2023 y la Orden HAC/1177/2024.',
      },
      {
        letra: 'l',
        etiqueta: 'Fecha y lugar de la firma',
        valor: '[día, mes y año] en [localidad], España',
      },
    ],
    faltan: ['dirección postal completa del productor'],
    anexo: [],
  }),
  declaracion_responsable_save: () => null,
  verifactu_lab_status: () => ({
    is_lab: false,
    info: null,
    records: [],
    checks: [
      {
        id: 'empresa',
        label: 'Empresa de pruebas',
        ok: false,
        blocking: true,
        detail:
          'Crea la empresa de pruebas: así las facturas de prueba no gastan tu numeración real.',
      },
      {
        id: 'modo',
        label: 'VERI*FACTU en modo pruebas',
        ok: true,
        blocking: true,
        detail:
          'Los registros van al entorno de pruebas (prewww1.aeat.es) y el QR apunta a la web de pruebas.',
      },
      {
        id: 'certificado',
        label: 'Certificado electrónico',
        ok: true,
        blocking: true,
        detail: 'CARMEN ESPAÑOLA ESPAÑOLA · caduca el 2029-03-01',
      },
      {
        id: 'nif',
        label: 'El certificado es del emisor',
        ok: true,
        blocking: true,
        detail: 'Emisor y certificado: 12345678Z',
      },
      {
        id: 'horario',
        label: 'Horario del entorno de pruebas',
        ok: true,
        blocking: false,
        detail: 'Dentro del horario habitual (laborables de 8 a 15 h).',
      },
    ],
  }),
  filed_delete: () => null,
  received_ai_fill_pending: () => ({ filled: 2, failed: [] }),
  notices_list: () => [],
  expedientes_list: () => demoExpedientes(),
  expediente_preview: () => demoExpedientes()[0]?.calculo,
  expediente_create: () => 3,
  expediente_update: () => null,
  expediente_delete: () => null,
  expediente_desde_notificacion: () => demoExpedientes()[0]?.expediente,
  expediente_desde_documento: () => demoExpedientes()[0]?.expediente,
  expediente_actuacion_add: () => 1,
  expediente_actuacion_delete: () => null,
  expediente_escrito_nombre: (a) =>
    `${a?.escrito === 'recurso_reposicion' ? 'Recurso de reposición' : 'Escrito'} 2026GRC00000001.${a?.pdf ? 'pdf' : 'rtf'}`,
  expediente_escrito_save: () => null,
  expedientes_avisos: () => [
    {
      texto: 'Requerimiento de información 2026GRC00000001: Pedir la ampliación del plazo',
      fecha: '2026-11-21',
      dias: 5,
      urgente: false,
    },
  ],
  certificados_admin_list: () => [
    {
      certificado: {
        id: 1,
        tipo: 'corriente_aeat',
        descripcion: 'Para una subvención (inventada)',
        emitido_el: '2025-11-26',
        caduca_el: '2026-11-26',
        document_id: null,
        notas: '',
      },
      dias: 10,
      estado: 'caduca_pronto',
    },
  ],
  certificado_admin_save: () => 2,
  certificado_admin_desde_documento: () => ({
    id: 0,
    tipo: 'corriente_aeat',
    descripcion: 'Certificado de estar al corriente',
    emitido_el: '2026-11-01',
    caduca_el: '2027-11-01',
    document_id: null,
    notas: '',
  }),
  certificado_admin_delete: () => null,
  threshold_alerts: () => [],
  notices_courtesy_days: () => [],
  notices_set_courtesy_days: (a) => [...new Set((a?.days as string[]) ?? [])].sort(),
  assistant_status: () => ({
    settings: { provider: { kind: 'Anthropic', model: 'claude-opus-5-5' }, consent: true },
    has_api_key: true,
    remote: true,
  }),
  dashboard_get: () => ({
    year: 2026,
    months: [
      [412_000, 98_000],
      [388_500, 104_300],
      [455_200, 121_900],
      [401_800, 96_400],
      [472_300, 133_700],
      [436_900, 101_200],
      [512_600, 142_800],
      [298_400, 87_600],
      [489_700, 118_300],
      [214_300, 54_900],
      [0, 0],
      [0, 0],
    ].map(([income, expenses], i) => ({ month: i + 1, income, expenses })),
    income: 4_081_700,
    expenses: 1_059_100,
    profit: 3_022_600,
    cash: 1_874_350,
    receivables: 642_180,
    payables: 98_220,
    estimates: [
      { model: '303', quarter: 4, result: 41_727 },
      { model: '130', quarter: 4, result: 31_080 },
    ],
    pending: {
      received_to_review: 2,
      bank_unreconciled: 5,
      notices_pending: 0,
      verifactu_unsent: 0,
    },
  }),
  dashboard_receivables: () => ({
    overdue_invoices: 1,
    overdue_amount: 96_800,
    accepted_quotes: 1,
    accepted_total: 181_500,
  }),
  cash_forecast: forecastDemo,
  model_monthly: (args) => (args?.model === '303' ? false : null),
  sii_estado: () => ({
    obligatorio: false,
    voluntario: false,
    motivos: [],
    avisos: [
      'Las facturas emitidas en 2026 ya suman 6.120.000,00 €: desde enero de 2027 tendrás que llevar los libros por el SII. Prepara el alta en el censo (036).',
    ],
    formato_sii: false,
    norma: 'Arts. 62.6, 68 bis, 69 bis y 71.3 RIVA (RD 1624/1992); art. 30 RIVA (REDEME)',
  }),
  sii_plazos: () => [
    {
      libro: 'emitida',
      numero: 'F2026-0412',
      nif: 'A22222228',
      nombre: 'Cliente Demo SA',
      fecha: '2026-11-16',
      limite: '2026-11-20',
      dias: 3,
      aproximado: false,
    },
    {
      libro: 'recibida',
      numero: 'R-118',
      nif: 'B12345674',
      nombre: 'Proveedor Demo SL',
      fecha: '2026-11-17',
      limite: '2026-11-23',
      dias: 6,
      aproximado: false,
    },
  ],
  model_draft: (args) => {
    const annual = DEMO_ANNUAL[String(args?.model)];
    if (annual) return annual;
    if (args?.model === '202') return draft202;
    if (args?.model !== '303')
      throw { message: 'demo: sólo el 303, el 202, el 190, el 180, el 193, el 216 y el 296' };
    return draft303;
  },
  rectificativa_anterior: (args) =>
    args?.model === '303' && args?.year === 2026 && args?.quarter === 3 ? demoAnterior : null,
  rectificativa_preparar: (args) => demoRectificativa(args),
  rectificativa_exportar: (args) => {
    demoRectificativa(args);
    return `${String(args?.dir ?? '~/Documents')}/B12345674_2026_3T_303_rectificativa.txt`;
  },
  model_certificates_prepare: (args) => {
    const model = String(args?.model ?? '190');
    const payees = DEMO_CERT_PAYEES[model];
    if (!payees) throw { message: 'demo: certificados sólo del 190, 180 y 193' };
    return {
      model,
      year: Number(args?.year ?? 2026),
      payees,
      place: 'València',
      signer: '',
      signer_role: '',
      notes:
        model === '190'
          ? [
              'TaxPain no registra retribuciones en especie, ingresos a cuenta ni reducciones: salen a cero. Si los hubo, corrige el certificado antes de entregarlo.',
            ]
          : model === '180'
            ? [
                'Sin inmueble guardado (Inmuebles Ejemplo SA): guárdalo en Modelos → 180 → Inmuebles arrendados, o escribe la dirección y la referencia catastral aquí, o saldrán en blanco.',
              ]
            : [],
    };
  },
  model_certificates_save: (args) => {
    const dir = String(args?.dir ?? '~/Documents');
    const model = String(args?.model ?? '190');
    const opts = args?.options as { nifs?: string[] } | undefined;
    const nifs = opts?.nifs?.length ? opts.nifs : (DEMO_CERT_PAYEES[model] ?? []).map((p) => p.nif);
    const out: string[] = [];
    if (args?.each)
      for (const n of nifs) out.push(`${dir}/Certificado retenciones ${model} 2026 ${n}.pdf`);
    if (args?.combined) out.push(`${dir}/Certificados retenciones ${model} 2026 12345678Z.pdf`);
    return out;
  },
  model_certificate_email: () => null,
  model_202_simulation: (args) => simulacion202(Number(args?.year ?? 2026)),
  model_202_set_modality: () => undefined,
};

/** Expedientes inventados (Estudio Demo SL): un requerimiento y una liquidación. */
function demoExpedientes(): ExpedienteVista[] {
  const base = {
    organo: '',
    puesto_el: null,
    plazo_dias: null,
    vence_manual: null,
    importe: null,
    sancion: null,
    fin_voluntario: null,
    notice_id: null,
    document_id: null,
    notas: '',
  };
  return [
    {
      expediente: {
        ...base,
        id: 1,
        organismo: 'aeat',
        organo: 'Gestión Tributaria (inventada)',
        referencia: '2026GRC00000001',
        tipo: 'requerimiento',
        asunto: 'Requerimiento de información',
        concepto: 'IVA 2025',
        notificado_el: '2026-11-10',
        estado: 'abierto',
        documentos: [
          { descripcion: 'Libro registro de facturas recibidas de 2025', listo: true },
          { descripcion: 'Contratos con proveedores', listo: false },
        ],
      },
      actuaciones: [],
      calculo: {
        notificado: '2026-11-10',
        presunta: false,
        plazos: [
          {
            clave: 'contestar',
            titulo: 'Contestar el requerimiento',
            vence: '2026-11-24',
            computo:
              '10 días hábiles desde el día siguiente a la notificación, sin sábados, domingos ni festivos (el plazo general: si el documento da otro, tecléalo)',
            norma: 'Art. 29.2.g LGT; RGAT (RD 1065/2007); arts. 30 y 31 Ley 39/2015',
            manual: false,
          },
          {
            clave: 'pedir_ampliacion',
            titulo:
              'Pedir la ampliación del plazo (hasta 5 días hábiles más: terminaría el 01/12/2026)',
            vence: '2026-11-21',
            computo: 'Tres días antes de que acabe el plazo',
            norma: 'Art. 91 RGAT (RD 1065/2007)',
            manual: false,
          },
        ],
        proximo: {
          clave: 'pedir_ampliacion',
          titulo: 'Pedir la ampliación del plazo',
          vence: '2026-11-21',
          computo: 'Tres días antes de que acabe el plazo',
          norma: 'Art. 91 RGAT',
          manual: false,
        },
        dias: 5,
        vencido: null,
        festivos_completos: true,
        recargo: [],
        reducciones: null,
        avisos: [],
      },
    },
    {
      expediente: {
        ...base,
        id: 2,
        organismo: 'aeat',
        referencia: 'A0000000000000002',
        tipo: 'acuerdo_sancionador',
        asunto: 'Acuerdo de imposición de sanción',
        concepto: 'Modelo 303 2T 2025',
        notificado_el: '2026-11-02',
        sancion: 100_000,
        estado: 'abierto',
        documentos: [],
      },
      actuaciones: [],
      calculo: {
        notificado: '2026-11-02',
        presunta: false,
        plazos: [
          {
            clave: 'pago',
            titulo: 'Pagar en periodo voluntario (o pedir antes el aplazamiento)',
            vence: '2026-12-21',
            computo: 'Notificada del 1 al 15: hasta el 20 del mes siguiente',
            norma: 'Art. 62.2 LGT',
            manual: false,
          },
          {
            clave: 'reposicion',
            titulo: 'Recurso de reposición (potestativo)',
            vence: '2026-12-02',
            computo: 'Un mes de fecha a fecha desde el día siguiente a la notificación',
            norma: 'Arts. 222 y 223.1 LGT',
            manual: false,
          },
        ],
        proximo: {
          clave: 'reposicion',
          titulo: 'Recurso de reposición (potestativo)',
          vence: '2026-12-02',
          computo: 'Un mes de fecha a fecha',
          norma: 'Arts. 222 y 223.1 LGT',
          manual: false,
        },
        dias: 16,
        vencido: null,
        festivos_completos: true,
        recargo: [
          {
            hasta: '2026-12-21',
            texto: 'Pagando hasta el 21/12/2026 (periodo voluntario): sin recargo',
            porcentaje: 0,
            recargo: 0,
            intereses: 0,
            total: 100_000,
            hoy: true,
          },
        ],
        reducciones: {
          sancion: 100_000,
          conformidad: 70_000,
          pronto_pago: 60_000,
          ambas: 42_000,
          notas: ['Pronto pago (40 %): si pagas en el plazo de pago voluntario y no recurres.'],
        },
        avisos: [],
      },
    },
  ];
}

export function installDemo(): void {
  setDemoInvoke(async (cmd, args) => {
    const h = handlers[cmd];
    if (!h) throw { message: `demo: ${cmd} no disponible` };
    return h(args);
  });
}

// Inversiones en valores: espejo de `taxpain_app::valores` y sus comandos.
// Importes en céntimos; títulos, precios y cambios como texto decimal con punto («12.5»).

import { command } from './api';

export type Broker = 'xtb' | 'ibkr' | 'otro';
export type ClaseValor = 'accion' | 'etf' | 'fondo' | 'bono' | 'cfd' | 'derivado' | 'otro';
export type TipoOpValores =
  | 'compra'
  | 'venta'
  | 'dividendo'
  | 'retencion'
  | 'interes'
  | 'comision'
  | 'impuesto'
  | 'traspaso'
  | 'split'
  | 'deposito'
  | 'retirada';
export type Cartera = 'coste' | 'negociacion';
export type Plazo = 'corto' | 'largo';

export const BROKER_LABEL: Record<Broker, string> = {
  xtb: 'XTB',
  ibkr: 'Interactive Brokers',
  otro: 'Otro bróker',
};

export const CLASE_LABEL: Record<ClaseValor, string> = {
  accion: 'Acción',
  etf: 'ETF',
  fondo: 'Fondo',
  bono: 'Bono',
  cfd: 'CFD',
  derivado: 'Derivado (opción)',
  otro: 'Otro',
};

export const TIPOS_OP: { id: TipoOpValores; label: string }[] = [
  { id: 'compra', label: 'Compra' },
  { id: 'venta', label: 'Venta' },
  { id: 'dividendo', label: 'Dividendo' },
  { id: 'retencion', label: 'Retención' },
  { id: 'interes', label: 'Intereses' },
  { id: 'comision', label: 'Comisión' },
  { id: 'impuesto', label: 'Impuesto' },
  { id: 'traspaso', label: 'Traspaso' },
  { id: 'split', label: 'Desdoblamiento' },
  { id: 'deposito', label: 'Ingreso en el bróker' },
  { id: 'retirada', label: 'Retirada del bróker' },
];

export const tipoOpLabel = (t: TipoOpValores): string =>
  TIPOS_OP.find((x) => x.id === t)?.label ?? t;

export interface CuentaBroker {
  id: number;
  broker: Broker;
  name: string;
  number: string;
  currency: string;
  /** País del depositario (ISO 3166-1). */
  country: string;
  /** Cuenta para el 720. */
  abroad: boolean;
  /** Subcuenta del efectivo en el bróker (vacía = la de los ajustes). */
  account: string;
  /** IBKR: Query ID de la Activity Flex Query. */
  flex_query: string;
  last_sync: string | null;
  last_sync_note: string;
  /** IBKR: hay token guardado (el token nunca llega a la interfaz). */
  token: boolean;
}

export interface CuentaView extends CuentaBroker {
  ops: number;
  conector: 'api' | 'pdf' | 'fichero' | 'manual';
}

export interface Valor {
  id: number;
  isin: string;
  ticker: string;
  name: string;
  currency: string;
  kind: ClaseValor;
  country: string;
  account: string;
}

export interface OpValores {
  id: number;
  account_id: number;
  to_account_id: number | null;
  security_id: number | null;
  date: string;
  time: string;
  kind: TipoOpValores;
  qty: string;
  price: string | null;
  currency: string;
  amount: number;
  fee: number;
  /** Euros por unidad de la divisa. */
  fx: string | null;
  eur: number;
  fee_eur: number;
  country: string;
  note: string;
  source: string;
  fingerprint: string | null;
  entry_id: number | null;
}

export interface PrecioCierre {
  date: string;
  security_id: number;
  price: string;
  currency: string;
  source: string;
}

export interface CuentasValores {
  corto_patrimonio: string;
  corto_deuda: string;
  largo_patrimonio: string;
  largo_deuda: string;
  tesoreria: string;
  beneficio: string;
  perdida: string;
  beneficio_vr: string;
  perdida_vr: string;
  dividendos: string;
  intereses: string;
  comisiones: string;
  retencion_espana: string;
  impuesto_extranjero: string;
  tributos: string;
  /** Deterioro de la cartera a coste (698/596/798 a corto; 6963/296/7963 a largo). */
  deterioro_corto: string;
  correctora_corto: string;
  reversion_corto: string;
  deterioro_largo: string;
  correctora_largo: string;
  reversion_largo: string;
}

export interface ValoresSettings {
  cartera: Cartera;
  plazo: Plazo;
  cuentas: CuentasValores;
}

export interface PosicionValor {
  security_id: number;
  ticker: string;
  name: string;
  isin: string;
  kind: ClaseValor;
  account_id: number;
  account: string;
  abroad: boolean;
  country: string;
  cantidad: string;
  coste: number;
  precio: string | null;
  divisa: string;
  precio_fecha: string | null;
  cambio: string | null;
  valor: number | null;
  latente: number | null;
}

export interface VentaValor {
  op: number;
  fecha: string;
  security_id: number;
  ticker: string;
  name: string;
  account: string;
  cantidad: string;
  transmision: number;
  coste: number;
  resultado: number;
  dos_meses: boolean;
  /** Venta o vencimiento de una opción: va en la línea de derivados. */
  derivado: boolean;
}

export interface ResultadoValores {
  ventas: number;
  ganancias: number;
  perdidas: number;
  neto: number;
  /** Resultado de las opciones y otros derivados, aparte de las ventas. */
  derivados: number;
  ventas_derivados: number;
  dividendos: number;
  retenciones_extranjero: number;
  retenciones_espana: number;
  intereses: number;
  comisiones: number;
  impuestos: number;
}

export interface RentaPais {
  pais: string;
  rentas: number;
  retenido: number;
  convenio_pct: number | null;
  convenio_comprobado: boolean;
  tope_convenio: number | null;
  cuota_espanola: number;
  deducible: number;
  exceso: number;
}

export interface DobleImposicion {
  tipo_pct: number;
  paises: RentaPais[];
  retenido: number;
  deducible: number;
  nota: string;
}

export interface M720 {
  year: number;
  fecha: string;
  total_extranjero: number;
  umbral: number;
  supera: boolean;
  cuentas: {
    account: string;
    country: string;
    valor: number;
    /** (ticker, ISIN, títulos, valor en euros). */
    valores: [string, string, string, number][];
  }[];
  sin_precio: string[];
  persona_fisica: boolean;
  exenta_por_contabilidad: boolean;
  nota: string;
}

export interface AsientoPropuesto {
  fecha: string;
  concepto: string;
  op: number | null;
  /** Asiento ya grabado de la operación. */
  entry_id: number | null;
  lineas: { cuenta: string; debe: number; haber: number }[];
}

export interface ValoresReport {
  year: number;
  inicio: string;
  fecha: string;
  persona_fisica: boolean;
  metodo: 'pmp' | 'fifo';
  settings: ValoresSettings;
  cuentas: CuentaView[];
  posiciones: PosicionValor[];
  coste_total: number;
  valor_total: number;
  latente_total: number;
  resultado: ResultadoValores;
  realizados: VentaValor[];
  doble_imposicion: DobleImposicion;
  m720: M720;
  asientos: AsientoPropuesto[];
  ops_ejercicio: number;
  /** Operaciones del ejercicio en otra divisa aún sin euros. */
  sin_euros: number;
  avisos: string[];
  notas: string[];
}

/** Lo que hizo «Pasar a euros con el BCE». */
export interface PasoBce {
  divisas: string[];
  tipos: number;
  operaciones: number;
}

export interface FicheroLeido {
  nombre: string;
  formato: string;
  cuenta: string;
  resumen: string;
  desde: string | null;
  hasta: string | null;
  error?: string | null;
}

export interface LecturaValores {
  ficheros: FicheroLeido[];
  /** Lo leído, tal cual vuelve al importar. */
  extractos: unknown[];
  nuevas: number;
  duplicadas: number;
  precios: number;
  avisos: string[];
}

export interface AplicadoValores {
  nuevas: number;
  duplicadas: number;
  sustituidas: number;
  precios: number;
  cuentas_nuevas: number;
  avisos: string[];
}

export const valoresReport = (year: number) => command<ValoresReport>('valores_report', { year });
export const valoresSaveSettings = (settings: ValoresSettings) =>
  command<void>('valores_save_settings', { settings });
export const valoresAccounts = () => command<CuentaBroker[]>('valores_accounts');
export const valoresSaveAccount = (account: CuentaBroker) =>
  command<number>('valores_save_account', { account });
export const valoresDeleteAccount = (id: number) => command<void>('valores_delete_account', { id });
export const valoresSecurities = () => command<Valor[]>('valores_securities');
export const valoresSaveSecurity = (security: Valor) =>
  command<number>('valores_save_security', { security });
export const valoresOps = (year: number | null) => command<OpValores[]>('valores_ops', { year });
export const valoresSaveOp = (op: OpValores) => command<number>('valores_save_op', { op });
export const valoresDeleteOp = (id: number) => command<void>('valores_delete_op', { id });
export const valoresPrices = () => command<PrecioCierre[]>('valores_prices');
export const valoresSetPrices = (prices: PrecioCierre[]) =>
  command<number>('valores_set_prices', { prices });
export const valoresDeletePrice = (date: string, securityId: number) =>
  command<void>('valores_delete_price', { date, securityId });
export const valoresReadFiles = (paths: string[]) =>
  command<LecturaValores>('valores_read_files', { paths });
export const valoresImport = (lectura: LecturaValores) =>
  command<AplicadoValores>('valores_import', { lectura });
export const valoresIbkrConnect = (id: number, token: string, query: string) =>
  command<CuentaBroker>('valores_ibkr_connect', { id, token, query });
export const valoresIbkrDisconnect = (id: number) =>
  command<CuentaBroker>('valores_ibkr_disconnect', { id });
export const valoresIbkrSync = (id: number, year: number) =>
  command<AplicadoValores>('valores_ibkr_sync', { id, year });
export const valoresExport = (year: number, dir: string, pdf: boolean) =>
  command<string>('valores_export', { year, dir, pdf });
/** Descarga del BCE los tipos de las operaciones en otra divisa sin euros y las pasa a euros. */
export const valoresBce = () => command<PasoBce>('valores_bce');

/** Elige uno o varios ficheros de bróker: PDF o Excel de XTB, Flex de IBKR (XML o CSV). */
export async function pickValoresFiles(): Promise<string[]> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const p = await open({
    multiple: true,
    filters: [
      {
        name: 'Informes de bróker (PDF o Excel de XTB, Flex XML o CSV de IBKR)',
        extensions: ['pdf', 'xlsx', 'xml', 'csv'],
      },
    ],
  });
  if (Array.isArray(p)) return p;
  return typeof p === 'string' ? [p] : [];
}

/** Una cuenta nueva del bróker con sus valores por defecto. */
export function nuevaCuenta(broker: Broker): CuentaBroker {
  const pais: Record<Broker, string> = { xtb: 'PL', ibkr: 'IE', otro: '' };
  return {
    id: 0,
    broker,
    name: BROKER_LABEL[broker],
    number: '',
    currency: 'EUR',
    country: pais[broker],
    abroad: true,
    account: '',
    flex_query: '',
    last_sync: null,
    last_sync_note: '',
    token: false,
  };
}

/**
 * Número escrito en español a texto decimal con punto, con `maxDec` decimales como mucho (18 en
 * los títulos, 8 en precios y tipos de cambio, como en el motor). El punto es de miles si separa
 * grupos de tres cifras («1.000» son mil, «1.234,5» mil doscientos y pico, como se enseñan en la
 * tabla); si no, es el decimal («12.5», «0.001»). `null` si no se entiende, sobra precisión o es
 * cero.
 */
export function parseDecimalEs(s: string, maxDec: number): string | null {
  const t = s.trim().replace(/\s/g, '');
  const m = /^(-?)(?:([1-9]\d{0,2}(?:\.\d{3})+)(?:,(\d+))?|(\d+)(?:,(\d+))?|(\d+)\.(\d+))$/.exec(t);
  if (!m) return null;
  const int = (m[2] ?? m[4] ?? m[6] ?? '0').replace(/\./g, '');
  const dec = (m[3] ?? m[5] ?? m[7] ?? '').replace(/0+$/, '');
  if (dec.length > maxDec) return null;
  if (!/[1-9]/.test(int + dec)) return null;
  return `${m[1]}${int.replace(/^0+(?=\d)/, '')}${dec ? `.${dec}` : ''}`;
}

// ---------------- Contabilidad ----------------

/** Lo que hizo «Contabilizar». */
export interface ContabilizadoValores {
  asientos: number;
  borrados: number;
  saltadas: number;
  avisos: string[];
}

export type TratamientoCierre =
  | 'valor_razonable'
  | 'deterioro'
  | 'sin_deterioro'
  | 'excluido'
  | 'no_aplica'
  | 'sin_precio';

export interface ValorCierre {
  security_id: number;
  ticker: string;
  name: string;
  kind: ClaseValor;
  cuenta: string;
  libros: number;
  valor: number | null;
  ajuste: number;
  tratamiento: TratamientoCierre;
}

export interface EntradaCierreValores {
  excluidos: number[];
}

export interface CierreValores {
  year: number;
  fecha: string;
  cartera: Cartera;
  closed: boolean;
  valores: ValorCierre[];
  lines: { account: string; name: string; debit: number; credit: number }[];
  beneficios: number;
  perdidas: number;
  deterioro: number;
  reversion: number;
  entry_id: number | null;
  stale: boolean;
  input: EntradaCierreValores;
  avisos: string[];
  notas: string[];
}

export const valoresPostEntries = (year: number, redo: boolean) =>
  command<ContabilizadoValores>('valores_post_entries', { year, redo, confirmed: true });
export const valoresUndoEntries = (year: number) =>
  command<number>('valores_undo_entries', { year, confirmed: true });
export const valoresClosing = (year: number, input?: EntradaCierreValores) =>
  command<CierreValores>('valores_closing', { year, input: input ?? null });
export const valoresClosingPost = (year: number, input: EntradaCierreValores) =>
  command<number>('valores_closing_post', { year, input, confirmed: true });
export const valoresClosingUndo = (year: number) =>
  command<void>('valores_closing_undo', { year, confirmed: true });

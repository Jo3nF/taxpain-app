import { invoke as tauriInvoke } from '@tauri-apps/api/core';

type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

/** Sustituto de `invoke` para el modo demo (capturas y vídeo con datos sintéticos). */
let demoInvoke: InvokeFn | null = null;

export function setDemoInvoke(f: InvokeFn | null): void {
  demoInvoke = f;
}

function rawInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return (demoInvoke ? demoInvoke(cmd, args) : tauriInvoke<T>(cmd, args)) as Promise<T>;
}

/** Prefijo del error de periodo cerrado que da Rust (`taxpain_app::periodos::MARCA`). */
const PERIODO_CERRADO = /^\[periodo-cerrado:(\d+)\]\s*/;

/**
 * Quien pregunta al usuario si sigue con una operación en un periodo cerrado (lo registra
 * `PeriodoCerradoGate`). Devuelve `true` si confirma.
 */
type PreguntaPeriodo = (texto: string) => Promise<boolean>;
let preguntaPeriodo: PreguntaPeriodo | null = null;

export function setPreguntaPeriodo(f: PreguntaPeriodo | null): void {
  preguntaPeriodo = f;
}

/** Si el error es el de un periodo cerrado: su id y el aviso (sin el prefijo). */
export function periodoCerrado(e: unknown): { id: number; texto: string } | null {
  const m = PERIODO_CERRADO.exec(rawMessage(e));
  if (!m) return null;
  return { id: Number(m[1]), texto: rawMessage(e).replace(PERIODO_CERRADO, '') };
}

/**
 * `invoke` con el aviso de periodo cerrado: si Rust dice que la fecha cae en un periodo
 * cerrado, se enseña el aviso; si el usuario confirma, se deja la excepción (queda en la
 * auditoría) y se repite la operación una vez.
 */
async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await rawInvoke<T>(cmd, args);
  } catch (e) {
    const p = periodoCerrado(e);
    if (!p || !preguntaPeriodo) {
      if (p) throw { message: p.texto };
      throw e;
    }
    if (!(await preguntaPeriodo(p.texto))) {
      throw { message: `No se ha hecho nada: ${p.texto}` };
    }
    await rawInvoke<void>('periodo_autorizar', { id: p.id, que: p.texto.slice(0, 300) });
    return rawInvoke<T>(cmd, args);
  }
}

/** `invoke` para los espejos que viven en su propio fichero (p. ej. `valores.ts`). */
export const command: typeof invoke = invoke;

/** Contrato con el proceso nativo: sólo resúmenes, nunca secretos. */
export interface AppInfo {
  version: string;
  /** `false` si las claves no están en el Keychain (sólo desarrollo). */
  keys_secure: boolean;
  /** La BD estaba por detrás de la última auditoría vista (p. ej. se restauró una copia antigua). */
  audit_rollback?: { anchored_seq: number; current_seq: number } | null;
  /** Aviso de auditoría sin reconocer (se quita con `auditRollbackAcknowledge`). */
  audit_warning?: AuditWarning | null;
  /** La empresa abierta es la de pruebas VERI*FACTU. */
  verifactu_lab?: boolean;
}

/** Espejo de `taxpain_vault::audit::PendingRollback`. */
export interface AuditWarning {
  kind: 'rollback' | 'anchor_missing' | 'anchor_invalid';
  anchored_seq: number | null;
  current_seq: number;
  detected_at: string;
}

/** El usuario ha visto el aviso de auditoría («Entendido»). `false` si no había ninguno. */
export const auditRollbackAcknowledge = () => invoke<boolean>('audit_rollback_acknowledge');

export type CertificateKind = 'persona_fisica' | 'representante_pj' | 'otro';

/** Espejo de `taxpain_vault::CertificateInfo`. */
export interface CertificateInfo {
  fingerprint_sha256: string;
  subject_dn: string;
  issuer_dn: string;
  serial: string;
  valid_from: string;
  valid_to: string;
  kind: CertificateKind;
  issuer_kind: 'fnmt' | 'otro';
  holder_name: string | null;
  holder_nif: string;
  entity_nif: string | null;
  entity_name: string | null;
  key_algorithm: string;
  policy_oids: string[];
}

export interface CommandError {
  message: string;
}

export function isTauri(): boolean {
  return demoInvoke !== null || (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window);
}

/** Lo que devuelve Rust cuando el usuario cancela Touch ID (o la contraseña del Mac). */
const NOT_CONFIRMED = 'operación no confirmada por el usuario';

function rawMessage(e: unknown): string {
  if (typeof e === 'object' && e !== null && 'message' in e) return String(e.message);
  return String(e);
}

/** El usuario ha cancelado la confirmación de Touch ID: no se ha hecho nada. */
export function isNotConfirmed(e: unknown): boolean {
  return rawMessage(e) === NOT_CONFIRMED;
}

export function errorMessage(e: unknown): string {
  if (isNotConfirmed(e)) {
    return 'No se ha hecho nada: no se ha confirmado con Touch ID ni con la contraseña del Mac.';
  }
  return rawMessage(e);
}

export async function appInfo(): Promise<AppInfo | null> {
  if (!isTauri()) return null;
  return invoke<AppInfo>('app_info');
}

export async function listCertificates(): Promise<CertificateInfo[]> {
  if (!isTauri()) return [];
  return invoke<CertificateInfo[]>('list_certificates');
}

/** Abre el diálogo nativo y devuelve la ruta elegida (o `null`). */
export async function pickCertificateFile(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const path = await open({
    multiple: false,
    directory: false,
    filters: [{ name: 'Certificado electrónico', extensions: ['p12', 'pfx'] }],
  });
  return typeof path === 'string' ? path : null;
}

export async function importCertificate(
  path: string,
  passphrase: string,
): Promise<CertificateInfo> {
  return invoke<CertificateInfo>('import_certificate', { path, passphrase });
}

export async function deleteCertificate(fingerprint: string): Promise<void> {
  return invoke('delete_certificate', { fingerprint });
}

/** Días hasta la caducidad (negativo si ya caducó). */
export function daysUntil(isoDate: string, now: Date = new Date()): number {
  return Math.floor((new Date(isoDate).getTime() - now.getTime()) / 86_400_000);
}

// ---------- Calendario fiscal ----------

/** Espejo de `taxpain_core::calendar::TaxpayerProfile` (se edita partiendo de un preset). */
export interface TaxpayerProfile {
  persona: 'Fisica' | 'Juridica' | 'EntidadAtribucion';
  irpf_method: 'EstimacionDirecta' | 'Modulos' | null;
  exento_pagos_fraccionados: boolean;
  iva: { periodicity: 'Mensual' | 'Trimestral'; sii: boolean } | null;
  retiene_trabajo_profesionales: boolean;
  retiene_alquileres: boolean;
  retiene_capital_mobiliario: boolean;
  /** Retenciones a no residentes sin establecimiento permanente (216/296). */
  retiene_no_residentes: boolean;
  gran_empresa: boolean;
  intracomunitarias: 'Mensual' | 'Trimestral' | null;
  obligado_347: boolean;
  /** Se conserva tal cual lo envía el proceso nativo. */
  cierre_ejercicio: unknown;
  pagos_fraccionados_is: boolean;
  /** Código de comunidad autónoma (p. ej. `ComunitatValenciana`). */
  ccaa: string | null;
  /** Festivos locales `YYYY-MM-DD`. */
  festivos_locales: string[];
  /** 720: cuentas, valores o inmuebles en el extranjero por más de 50.000 €. */
  bienes_extranjero: boolean;
  /** 721: criptomonedas en el extranjero por más de 50.000 €. */
  monedas_virtuales_extranjero: boolean;
  /** 232 (sociedades): operaciones vinculadas. */
  operaciones_vinculadas: boolean;
  /** Renta en dos plazos (el segundo, en noviembre). */
  renta_fraccionada: boolean;
  /** Primer año en que se paga el IAE (840 en diciembre del anterior). */
  iae_desde: number | null;
  /** 369 trimestral (ventanilla única de la UE). */
  oss: boolean;
  /** 369 mensual (ventanilla única de importación). */
  ioss: boolean;
  /** Avisar de los plazos del censo de noviembre y diciembre. */
  opciones_censales: boolean;
  /** Cuota de autónomos (RETA). */
  cotiza_reta: boolean;
  /** Seguros sociales de los trabajadores. */
  tiene_trabajadores: boolean;
}

export interface DeadlineView {
  model: string;
  /** «Modelo 303», «Cuota de autónomos»… */
  title: string;
  description: string;
  period: string;
  fiscal_year: number;
  starts_on: string;
  nominal_end: string;
  due_on: string;
  domiciliation_end: string | null;
  /** `false` = fecha estimada (la AEAT aún no ha publicado su tabla). */
  domiciliation_official: boolean;
  tentative: boolean;
  rule_ref: string;
}

export interface CalendarView {
  year: number;
  today: string;
  holidays_loaded: boolean;
  /** `false` = falta el calendario del contribuyente de la AEAT del año (fechas estimadas). */
  aeat_calendar_loaded: boolean;
  /** Festivos locales del año en el perfil (0 = pendientes de publicar o de añadir). */
  local_holidays: number;
  deadlines: DeadlineView[];
}

/** Estado de un dato anual (`taxpain_app::cambio_ejercicio`). */
export type EstadoVigencia = 'listo' | 'supuesto' | 'pendiente' | 'revisar';

export interface ComprobacionVigencia {
  clave: string;
  titulo: string;
  estado: EstadoVigencia;
  detalle: string;
}

/** Qué datos de un año tiene TaxPain (festivos, calendario AEAT, diseños, parámetros). */
export interface VigenciaEjercicio {
  year: number;
  comprobaciones: ComprobacionVigencia[];
}

export async function vigenciaEjercicio(year: number): Promise<VigenciaEjercicio> {
  return invoke('vigencia_ejercicio', { year });
}

/** En diciembre y enero, lo que hay que revisar del año nuevo; el resto del año, `null`. */
export async function avisoCambioEjercicio(): Promise<VigenciaEjercicio | null> {
  return invoke('aviso_cambio_ejercicio');
}

export async function profilePresets(): Promise<[string, TaxpayerProfile][]> {
  return invoke('profile_presets');
}

export async function ccaaList(): Promise<[string, string][]> {
  return invoke('ccaa_list');
}

export async function getProfile(): Promise<TaxpayerProfile | null> {
  if (!isTauri()) return null;
  return invoke('get_profile');
}

/** Perfil fiscal deducido de lo presentado (`taxpain_app::perfil_deducido`), sin guardar. */
export interface DeducedProfile {
  perfil: TaxpayerProfile;
  /** De dónde sale cada cosa («303 trimestral (2024 a 2026)»…). */
  motivos: string[];
}

export async function deducedProfile(): Promise<DeducedProfile | null> {
  return invoke('deduced_profile');
}

export async function setProfile(profile: TaxpayerProfile): Promise<void> {
  return invoke('set_profile', { profile });
}

/** Aviso de umbral calculado con los datos de la app (`taxpain_app::umbrales`). */
export interface ThresholdAlert {
  code: string;
  severity: 'warning' | 'info';
  title: string;
  /** Lo que dicen los datos. */
  detail: string;
  /** Qué hacer. */
  action: string;
  rule: string;
}

export async function thresholdAlerts(): Promise<ThresholdAlert[]> {
  return invoke('threshold_alerts');
}

export async function fiscalCalendar(year: number): Promise<CalendarView> {
  return invoke('fiscal_calendar', { year });
}

/** Pide dónde guardar y exporta el `.ics`. Devuelve `false` si el usuario cancela. */
export async function exportIcal(year: number): Promise<boolean> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: `taxpain-${year}.ics`,
    filters: [{ name: 'Calendario', extensions: ['ics'] }],
  });
  if (!path) return false;
  await invoke('export_ical', { year, path });
  return true;
}

/** Días naturales entre dos fechas ISO `YYYY-MM-DD`. */
export function daysBetween(from: string, to: string): number {
  return Math.round((Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) / 86_400_000);
}

export function formatDate(iso: string): string {
  const [y, m, d] = iso.split('-');
  return `${d}/${m}/${y}`;
}

// ---------- Facturación ----------
// Espejo de la serialización serde de `taxpain-core::invoice` y `taxpain-app`.
// Importes en céntimos (enteros) y tipos en centésimas de punto (21 % = 2100).

export type PartyId =
  | { Nif: string }
  | { Foreign: { country: string; id_type: string; id: string } };

export interface Address {
  street: string;
  post_code: string;
  town: string;
  province: string;
  /** ISO 3166-1 alfa-2. */
  country: string;
}

export interface PersonName {
  name: string;
  first_surname: string;
  second_surname: string | null;
}

export interface Party {
  id: PartyId;
  name: string;
  address: Address | null;
  person: PersonName | null;
}

export type VatTreatment =
  | { Taxed: { rate: number } }
  | 'ReverseCharge'
  | { Exempt: 'E1' | 'E2' | 'E3' | 'E4' | 'E5' | 'E6' }
  | { NotSubject: 'N1' | 'N2' };

export interface InvoiceLine {
  description: string;
  quantity_milli: number;
  unit_price: number;
  discount: number;
  vat: VatTreatment;
  surcharge: number | null;
  withholding: number | null;
  /** Anticipo (positivo al cobrarlo, negativo al descontarlo): base a la 438. */
  advance?: boolean;
}

export type InvoiceKind =
  | 'Completa'
  | 'Simplificada'
  | 'SustitutivaDeSimplificadas'
  | { Rectificativa: { code: number; method: 'Sustitucion' | 'Diferencias' } };

export interface InvoiceDraft {
  kind: InvoiceKind;
  series: string | null;
  issue_date: string;
  operation_date: string | null;
  recipient: Party | null;
  lines: InvoiceLine[];
  rectified: [string, string][];
  rectified_amounts: [number, number] | null;
  description: string;
  notes: string | null;
  /** Moneda si no es el euro: las líneas van en esa moneda (ver «Moneda extranjera»). */
  currency?: ForeignCurrency | null;
  /** Ventanilla única: particular de otro país de la UE con el IVA de su país (líneas N2). */
  oss?: OssDraft | null;
}

/** Clase de operación del régimen de la Unión (ventanilla única). */
export type OssSupply = 'Servicios' | 'Bienes';

/** Lo que se elige en la factura con el IVA del país del cliente. */
export interface OssDraft {
  /** Tipo en centésimas de punto (20 % = 2000). */
  rate: number;
  supply: OssSupply;
}

/** IVA del Estado miembro de consumo de una factura (ya sumado al total y a lo que se cobra). */
export interface OssVat {
  country: string;
  supply: OssSupply;
  rate: number;
  base: number;
  vat: number;
}

export interface Totals {
  base: number;
  vat: number;
  surcharge: number;
  withholding: number;
  tax_total: number;
  invoice_total: number;
  payable: number;
  oss?: OssVat | null;
}

export interface Destination {
  kind: 'Local' | 'ICloudDrive' | 'GoogleDrive' | 'Dropbox';
  path: string;
}

export interface InvoicingSettings {
  issuer: Party;
  default_series: string;
  rectifying_series: string;
  /** Serie de las facturas simplificadas (RD 1619/2012 art. 7.1.a: separada de las completas). */
  simplified_series: string;
  /** Serie de las rectificativas de simplificadas (R5): separada de las rectificativas de
   * completas y de las simplificadas (RD 1619/2012 art. 7.1.a). Ajustes antiguos: «RS». */
  simplified_rectifying_series?: string;
  language: 'Es' | 'En';
  default_vat: number;
  default_withholding: number | null;
  outputs: { pdf: boolean; facturae: boolean; ubl?: boolean };
  destination: Destination | null;
  rounding: 'PerRate' | 'PerLine';
  verifactu: { enabled: boolean; installation_id: string; production: boolean };
  footer: string | null;
  /** Actividad para los libros registro: código A/B/C/D, tipo y epígrafe IAE. */
  activity?: { code: string; kind: string; iae: string } | null;
  /** Actividad del art. 4.2 RD 1619/2012 (minorista, hostelería…): simplificada hasta 3.000 €;
   * si no, hasta 400 € (art. 4.1.a). */
  simplified_up_to_3000?: boolean;
}

export interface IssuedInvoice {
  id: number;
  number: string;
  totals: Totals;
  verifactu_huella: string | null;
  qr_url: string | null;
  exported_to: string | null;
  export_error: string | null;
}

export interface IssuedSummary {
  id: number;
  number: string;
  issue_date: string;
  kind: string;
  recipient_name: string | null;
  base: number;
  vat: number;
  total: number;
  payable: number;
  status: 'emitida' | 'anulada';
  /** `pendiente`, `pruebas`, `correcto`, `aceptado_con_errores`, `incorrecto` o
   * `sin_respuesta` (la AEAT contestó dos veces sin línea para el registro). */
  verifactu_status: string | null;
  /** Motivo del rechazo o aviso de la AEAT del último registro. */
  verifactu_error?: string | null;
  exported_path: string | null;
  /** Aviso al exportar (copia, Facturae, UBL). Hoy la lista no lo guarda: llega siempre vacío. */
  export_error?: string | null;
  has_facturae: boolean;
  has_ubl: boolean;
  /** `importada`: del libro registro de la gestoría (sin VERI*FACTU). */
  origin?: 'app' | 'importada';
  /** En moneda extranjera: la moneda y lo que se cobra en ella (lo de arriba, en euros). */
  currency?: string | null;
  currency_payable?: number | null;
  /** Moneda y tipo con que se emitió (una rectificativa parte de ellos). */
  currency_rate?: ForeignCurrency | null;
}

export const invoicingSettings = () => invoke<InvoicingSettings | null>('invoicing_settings');
export const saveInvoicingSettings = (settings: InvoicingSettings) =>
  invoke<void>('save_invoicing_settings', { settings });
export const newInstallationId = () => invoke<string>('new_installation_id');
export const detectDestinations = () => invoke<Destination[]>('detect_destinations');
/** `language`: idioma del PDF de esta factura (`null` = el de los ajustes). `totals` en la
 * moneda de la factura y, si no es el euro, `eur_totals` en euros. */
export const invoicePreview = (
  draft: InvoiceDraft,
  open: boolean,
  language: 'Es' | 'En' | null = null,
) =>
  invoke<{ totals: Totals; eur_totals?: Totals | null; opened: boolean }>('invoice_preview', {
    draft,
    open,
    language,
  });
export const invoiceIssue = (
  draft: InvoiceDraft,
  confirmed: boolean,
  language: 'Es' | 'En' | null = null,
) => invoke<IssuedInvoice>('invoice_issue', { draft, confirmed, language });
export const invoicesList = (year: number) => invoke<IssuedSummary[]>('invoices_list', { year });
export const invoiceOpen = (id: number, facturae: boolean) =>
  invoke<void>('invoice_open', { id, facturae });

export type InvoiceFormat = 'pdf' | 'facturae' | 'ubl';

/** Pide dónde guardar el PDF, el Facturae o el UBL. Devuelve `false` si el usuario cancela. */
export async function invoiceSave(
  id: number,
  number: string,
  format: InvoiceFormat,
): Promise<boolean> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const file = number.replace(/[^\w.-]/g, '-');
  const [defaultPath, name, extensions] =
    format === 'pdf'
      ? [`${file}.pdf`, 'PDF', ['pdf']]
      : format === 'facturae'
        ? [`${file}.xml`, 'Facturae', ['xml']]
        : [`${file}.ubl.xml`, 'UBL', ['xml']];
  const path = await save({ defaultPath, filters: [{ name, extensions }] });
  if (!path) return false;
  await invoke('invoice_save', { id, format, path });
  return true;
}
/** Firma la Facturae con el certificado (Touch ID) y la guarda donde diga el usuario. */
export async function invoiceSignFacturae(
  id: number,
  number: string,
  fingerprint: string,
): Promise<boolean> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: `${number.replace(/[^\w.-]/g, '-')}.xsig`,
    filters: [{ name: 'Facturae firmada', extensions: ['xsig'] }],
  });
  if (!path) return false;
  await invoke('invoice_sign_facturae', { id, fingerprint, path });
  return true;
}
export const invoiceCancel = (id: number, confirmed: boolean) =>
  invoke<void>('invoice_cancel', { id, confirmed });
/** Resultado de una sesión de remisión: uno o varios lotes con un solo Touch ID. */
export interface SendResult {
  estado_envio: string;
  registros: number;
  envios?: number;
  tiempo_espera_envio: number;
  incidencia?: boolean;
  entorno?: 'pruebas' | 'produccion';
  /** La sesión se cortó por un fallo: la cola lo reintenta más tarde. */
  fallo?: string | null;
}
export const verifactuSend = (fingerprint: string, confirmed: boolean) =>
  invoke<SendResult | null>('verifactu_send', { fingerprint, confirmed });

/** Estado de la cola de remisión VERI*FACTU (Orden HAC/1177/2024 art. 16). */
export interface VerifactuQueue {
  entorno: 'pruebas' | 'produccion';
  sin_produccion: string | null;
  pendientes: number;
  sin_respuesta: number;
  rechazados: number;
  aceptados_con_errores: number;
  aceptados_pruebas: number;
  remitidos: number;
  espera_segundos: number;
  proximo_envio: string | null;
  tiempo_espera: number;
  incidencia_desde: string | null;
  fallos_seguidos: number;
  ultimo_intento: string | null;
  ultimo_resultado: string | null;
  ultimo_error: string | null;
  bloqueada: boolean;
  aviso: string | null;
  aviso_modo: string | null;
}
export const verifactuQueueStatus = () => invoke<VerifactuQueue>('verifactu_queue_status');
export const verifactuQueueReopen = () =>
  invoke<void>('verifactu_queue_reopen', { confirmed: true });

/** Declaración responsable del sistema de facturación (Orden HAC/1177/2024 art. 15). */
export interface DeclaracionResponsable {
  titulo: string;
  version: string;
  apartados: { letra: string; etiqueta: string; valor: string }[];
  faltan: string[];
  anexo: string[];
}
export const declaracionResponsable = () =>
  invoke<DeclaracionResponsable>('declaracion_responsable');
/** Pide dónde guardar la declaración en PDF y la guarda. */
export async function declaracionResponsableSave(version: string): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: `declaracion-responsable-TaxPain-${version}.pdf`,
    filters: [{ name: 'PDF', extensions: ['pdf'] }],
  });
  if (!path) return null;
  await invoke('declaracion_responsable_save', { path });
  return path;
}

/** SHA-256 (hex) del texto en UTF-8: la misma huella que se guarda en la auditoría. */
export async function sha256Hex(text: string): Promise<string> {
  const d = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(d)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

export async function pickFolder(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const p = await open({ directory: true, multiple: false });
  return typeof p === 'string' ? p : null;
}

// ---------- Recibidas ----------

export interface ExtractedField<T> {
  value: T;
  confidence: 'Baja' | 'Media' | 'Alta';
}

export interface Extraction {
  source: string;
  supplier_nif: ExtractedField<string> | null;
  supplier_name: ExtractedField<string> | null;
  number: ExtractedField<string> | null;
  issue_date: ExtractedField<string> | null;
  base: ExtractedField<number> | null;
  vat: ExtractedField<number> | null;
  withholding: ExtractedField<number> | null;
  total: ExtractedField<number> | null;
  warnings: string[];
  /** Factura en otra moneda: la moneda, el tipo y el IVA en euros que figuran en ella. */
  fx?: ExtractedFx | null;
}

/** Espejo de `taxpain_invoicing::ingest::ExtractedFx`. */
export interface ExtractedFx {
  code: string;
  /** Unidades de la moneda por 1 € («1.1193»). */
  rate: string | null;
  rate_date: string | null;
  vat_eur: number | null;
}

export interface ReceivedSummary {
  id: number;
  filename: string;
  status: 'pendiente' | 'revisada' | 'descartada';
  supplier_nif: string | null;
  supplier_name: string | null;
  number: string | null;
  issue_date: string | null;
  total: number | null;
  extraction: Extraction;
  /** En moneda extranjera: la moneda y el total en ella (`total`, en euros). */
  currency?: string | null;
  currency_total?: number | null;
  /** Revisada: los datos que se confirmaron (en euros), para corregirlos. */
  reviewed?: Reviewed | null;
  /** Revisada en moneda extranjera: la moneda, el tipo y los importes en ella. */
  fx?: ReceivedFx | null;
  /** Revisada: fecha de registro (la de la revisión), la que la lleva al 303. */
  registered_on?: string | null;
}

/** Espejo de `corregir_recibida::Resumen`: lo que se enseña para confirmar (en euros). */
export interface ReceivedResumen {
  proveedor_nif: string;
  proveedor: string;
  numero: string;
  fecha: string;
  base: number;
  iva: number;
  retencion: number;
  total: number;
  deducible_pct: number;
  regimen: 'interior' | 'intracomunitaria' | 'isp';
  moneda: string | null;
  total_moneda: number | null;
}

/** Espejo de `corregir_recibida::Comprobacion`. */
export interface ReceivedCheck {
  /** Corrección de una revisada (si no, la primera revisión). */
  correccion: boolean;
  despues: ReceivedResumen;
  antes: ReceivedResumen | null;
  /** Trimestre del 303 al que va su IVA («3T 2026»). */
  trimestre: string;
  avisos: string[];
  /** Modelos presentados que toca y se pueden rectificar desde Modelos. */
  rectificar?: Rectificable[];
}

/** Espejo de `corregir_recibida::Corregida`. */
export interface ReceivedCorrected {
  avisos: string[];
  rectificar?: Rectificable[];
  /** El asiento pasó de ejercicio: el de antes. */
  ejercicio_anterior: number | null;
  /** Líneas punteadas que cambiaron de importe (se les quita el punteo). */
  punteos_quitados: number;
}

export interface Reviewed {
  supplier_nif: string;
  supplier_name: string;
  number: string;
  issue_date: string;
  base: number;
  vat: number;
  withholding: number;
  total: number;
  deductible_pct: number;
  /** Bien de inversión (casillas de inversión del 303/390). */
  capital_good: boolean;
  /** Naturaleza: decide si la retención va al 115/180 (alquiler) o al 111/190 (profesional). */
  nature: 'general' | 'alquiler' | 'profesional';
  /** Clase de inmovilizado si es bien de inversión (cuenta y coeficiente de amortización). */
  asset_class: AssetClass | null;
  /** IVA: interior (con IVA en la factura) o con inversión del sujeto pasivo (se autorrepercute). */
  vat_regime?: 'interior' | 'intracomunitaria' | 'isp';
  /** País del proveedor (ISO alfa-2) si no es España. */
  supplier_country?: string | null;
  /** Tipo de la autorrepercusión en puntos básicos (2100 = 21 %). */
  self_assessed_rate?: number | null;
  /** Adquisición intracomunitaria de bienes (clave A del 349); si no, servicios (I). */
  intra_goods?: boolean;
  /** Proveedor en criterio de caja: el IVA se deduce al pagar la factura. */
  cash_basis?: boolean;
  /** DUA de importación (total = IVA pagado en la aduana). */
  import_dua?: boolean;
  /** Prorrata especial: destino de la compra. */
  prorata_use?: ProrataUse;
}

/** Destino de una compra con prorrata especial. */
export type ProrataUse = 'comun' | 'plena' | 'ninguna';

export type AssetClass =
  | 'informatica'
  | 'mobiliario'
  | 'transporte'
  | 'maquinaria'
  | 'utillaje'
  | 'instalaciones'
  | 'construcciones';

export async function pickReceivedFiles(): Promise<string[]> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const p = await open({
    multiple: true,
    filters: [
      {
        name: 'Facturas y fotos',
        extensions: [
          'pdf',
          'xml',
          'xsig',
          'jpg',
          'jpeg',
          'png',
          'heic',
          'heif',
          'tif',
          'tiff',
          'webp',
        ],
      },
    ],
  });
  if (!p) return [];
  return Array.isArray(p) ? p : [p];
}

/** Importa recibidas: las que han entrado y, de las que no, el fichero y el motivo. */
export const receivedImport = (paths: string[]) =>
  invoke<{ outcomes: { id: number; duplicate: boolean }[]; errors: string[] }>('received_import', {
    paths,
  });
export const receivedList = (status: string | null) =>
  invoke<ReceivedSummary[]>('received_list', { status });
/** `currency`: factura en moneda extranjera (los importes de `reviewed` los pone la conversión). */
export const receivedReview = (
  id: number,
  reviewed: Reviewed,
  currency: ReceivedFx | null = null,
) => invoke<void>('received_review', currency ? { id, reviewed, currency } : { id, reviewed });
/** Comprueba los datos antes de confirmarlos (revisión o corrección): resumen y avisos. */
export const receivedCheck = (id: number, reviewed: Reviewed, currency: ReceivedFx | null = null) =>
  invoke<ReceivedCheck>('received_check', currency ? { id, reviewed, currency } : { id, reviewed });
/** Corrige una revisada y rehace su asiento (queda en la auditoría con el antes y el después). */
export const receivedCorrect = (
  id: number,
  reviewed: Reviewed,
  currency: ReceivedFx | null = null,
) =>
  invoke<ReceivedCorrected>(
    'received_correct',
    currency ? { id, reviewed, currency } : { id, reviewed },
  );
/** Avisos (o el error que lo impide) antes de deshacer la revisión de una recibida. */
export const receivedUnreviewCheck = (id: number) =>
  invoke<string[]>('received_unreview_check', { id });
/** Una revisada sin pagos vuelve a «pendiente de revisar» (se borra su asiento). */
export const receivedUnreview = (id: number) => invoke<void>('received_unreview', { id });
export const receivedDiscard = (id: number) => invoke<void>('received_discard', { id });
/** Una descartada por error vuelve a pendiente (reimportar el fichero no vale: es un duplicado). */
export const receivedUndiscard = (id: number) => invoke<void>('received_undiscard', { id });
export const receivedRegisterTyped = (reviewed: Reviewed, currency: ReceivedFx | null = null) =>
  invoke<number>('received_register_typed', currency ? { reviewed, currency } : { reviewed });
export const receivedOpen = (id: number) => invoke<void>('received_open', { id });
/** Carpetas vigiladas de la empresa (con sus subcarpetas). */
export const watchedFolders = () => invoke<string[]>('watched_folders');
export const setWatchedFolders = (folders: string[]) =>
  invoke<string[]>('set_watched_folders', { folders });
/** Documento nuevo de una carpeta vigilada que no es una factura recibida: se importa en Importar. */
export interface WatchedPending {
  folder: string;
  path: string;
  kind: string;
  year: number | null;
  seen: string;
  /** Si no se ha podido leer, por qué (aviso «Ficheros que TaxPain no ha podido leer»). */
  error?: string | null;
}
export const watchedPending = () => invoke<WatchedPending[]>('watched_pending');
export const watchedDismiss = (folder: string | null) =>
  invoke<void>('watched_dismiss', { folder });

// ---------- Formato ----------

/** 123456 → "1.234,56 €" */
export function euros(cents: number): string {
  const sign = cents < 0 ? '-' : '';
  const abs = Math.abs(cents);
  const int = Math.floor(abs / 100)
    .toString()
    .replace(/\B(?=(\d{3})+(?!\d))/g, '.');
  return `${sign}${int},${String(abs % 100).padStart(2, '0')} €`;
}

/**
 * Importe escrito a mano → céntimos, o null si no es válido.
 * Acepta miles con punto ("12.000", "1.234,56"), decimales con coma ("1234,56") y, sin miles,
 * un punto seguido de 1–2 cifras como decimal ("1234.56", "1.50"). "1,234" es ambiguo: null.
 */
export function parseEuros(s: string): number | null {
  const t = s.trim().replace(/\s|€/g, '');
  // Miles con punto sólo si el primer grupo no empieza por 0 («0.125» no son 125 €).
  const m = /^(-?)(?:([1-9]\d{0,2}(?:\.\d{3})+)(?:,(\d{1,2}))?|(\d+)(?:[.,](\d{1,2}))?)$/.exec(t);
  if (!m) return null;
  const int = (m[2] ?? m[4] ?? '').replace(/\./g, '');
  const dec = m[3] ?? m[5] ?? '';
  const cents = Number(int) * 100 + Number(dec.padEnd(2, '0'));
  return m[1] && cents ? -cents : cents;
}

/**
 * Cantidad de una línea («1.000», «2,5», «0,250») en milésimas: miles con punto si hay decimales
 * con coma, o un solo separador con hasta tres decimales. Vacío, ambiguo («1.234.5») o no
 * numérico («0x10», «1e3») → null; `Number()` los aceptaría con otro valor.
 */
export function parseQuantity(s: string): number | null {
  const t = s.trim().replace(/\s/g, '');
  // Miles con punto sólo si el primer grupo no empieza por 0 («0.250» son 0,25).
  const m = /^(-?)(?:([1-9]\d{0,2}(?:\.\d{3})+)(?:,(\d{1,3}))?|(\d+)(?:[.,](\d{1,3}))?)$/.exec(t);
  if (!m) return null;
  const int = (m[2] ?? m[4] ?? '').replace(/\./g, '');
  const dec = m[3] ?? m[5] ?? '';
  const milli = Number(int) * 1000 + Number(dec.padEnd(3, '0'));
  return m[1] && milli ? -milli : milli;
}

/** Tipo de factura del diseño de registro VERI*FACTU (F1, F2, R1…) en palabras. */
export function kindLabel(kind: string): string {
  if (kind === 'F1') return 'Completa';
  if (kind === 'F2') return 'Simplificada';
  if (kind === 'F3') return 'Sustitutiva de simplificadas';
  if (/^R[1-5]$/.test(kind)) return `Rectificativa (${kind})`;
  return kind;
}

/** `EstadoEnvio` del servicio VERI*FACTU (Correcto, ParcialmenteCorrecto, Incorrecto) en palabras. */
export function sendStateLabel(estado: string): string {
  const labels: Record<string, string> = {
    Correcto: 'aceptado',
    ParcialmenteCorrecto: 'aceptado con algún error',
    Incorrecto: 'rechazado',
  };
  return labels[estado] ?? estado;
}

/** 1 → "1 fichero"; 3 → "3 ficheros". */
export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** Días hasta un plazo, en palabras: «vence hoy», «vence mañana», «faltan 5 días». */
export function dueIn(days: number): string {
  if (days < 0) return 'plazo terminado';
  if (days === 0) return 'vence hoy';
  if (days === 1) return 'vence mañana';
  return `faltan ${days} días`;
}

// ---------- Contabilidad ----------

export type Origin =
  | 'Nomina'
  | 'Manual'
  | 'Apertura'
  | 'Regularizacion'
  | 'Cierre'
  | { FacturaEmitida: number }
  | { FacturaRecibida: number }
  | { Banco: number }
  | { Amortizacion: number }
  | { LiquidacionIva: number }
  | { Cartera: number }
  | { Importado: number }
  | { Gestor: number }
  | { PagoModelo: number }
  | { BajaInmovilizado: number }
  | { Dividendo: number }
  | { GastosViaje: number }
  | { AnulacionFactura: number }
  | { DevolucionRemesa: number }
  | { Existencias: number }
  | { Cripto: number }
  | { AjusteCierre: number }
  | { DiferenciasCambio: number }
  | { Valores: number }
  | { AcuerdoDividendo: number }
  | { Subvencion: number }
  | { Leasing: number }
  | { Prestamo: number }
  | { ImpuestoSociedades: number }
  | { AplicacionResultado: number };

export interface EntryLine {
  account: string;
  debit: number;
  credit: number;
  description: string | null;
}

export interface JournalEntry {
  date: string;
  concept: string;
  origin: Origin;
  lines: EntryLine[];
}

export interface StoredEntry {
  id: number;
  number: number;
  entry: JournalEntry;
  /** Observaciones del asiento. */
  notes?: string | null;
  /** Documentos adjuntos. */
  attachments?: number;
}

export interface AccountBalance {
  account: string;
  debit: number;
  credit: number;
}

export interface LedgerLine {
  number: number;
  date: string;
  concept: string;
  account: string;
  debit: number;
  credit: number;
  balance: number;
  line_id: number;
  ticked: boolean;
}

export interface FixedAsset {
  id: number;
  name: string;
  account: string;
  depreciation_account: string;
  expense_account: string;
  acquired_on: string;
  cost: number;
  residual: number;
  rate: number;
  disposed_on: string | null;
  /** Método contable: lineal, porcentaje constante (degresivo) o suma de dígitos. */
  method?: DepreciationMethod;
  /** Coeficiente fiscal propio (puntos básicos) si difiere del contable. */
  tax_rate?: number | null;
}

export type DepreciationMethod = 'lineal' | 'constante' | 'digitos';

/** Ejercicio contable (puede no ser natural: p. ej. de julio a junio). */
export interface FiscalYearInfo {
  year: number;
  start: string;
  end: string;
  /** «2026» o «2026/2027». */
  label: string;
  natural: boolean;
  closed: boolean;
}

export const fiscalYearInfo = (year: number | null) =>
  invoke<FiscalYearInfo>('fiscal_year_info', { year });

export const journal = (year: number) => invoke<StoredEntry[]>('journal', { year });
export const trialBalance = (year: number, digits: number | null) =>
  invoke<AccountBalance[]>('trial_balance', { year, digits });
export const ledger = (year: number, account: string) =>
  invoke<LedgerLine[]>('ledger', { year, account });
export const manualEntry = (entry: JournalEntry) => invoke<number>('manual_entry', { entry });
export const setTick = (lineId: number, ticked: boolean) =>
  invoke<void>('set_tick', { lineId, ticked });
export const updateEntry = (id: number, entry: JournalEntry) =>
  invoke<void>('update_entry', { id, entry });
export const deleteEntry = (id: number, confirmed: boolean) =>
  invoke<void>('delete_entry', { id, confirmed });
export const deleteEntries = (ids: number[]) =>
  invoke<number>('delete_entries', { ids, confirmed: true });

/** Espejo de `taxpain_app::cartera`. */
export interface DueItem {
  kind: 'emitida' | 'recibida';
  invoice_id: number;
  /** Vencimiento (1, 2…) de `installments`. */
  seq: number;
  installments: number;
  number: string;
  party: string;
  party_account: string | null;
  issue_date: string;
  due_date: string;
  amount: number;
  invoice_amount: number;
  status: 'pendiente' | 'banco' | 'registrada';
  settled_on: string | null;
  overdue_days: number;
  method: PayMethod | null;
  treasury: string | null;
  fixed: boolean;
  /** Remesa SEPA en la que va. */
  remittance?: string | null;
  /** Moneda de la factura si no es el euro (los importes, en euros contabilizados). */
  currency?: string | null;
  /** La factura (todos sus plazos) llega al límite de pagos en efectivo (art. 7 Ley 7/2012). */
  cash_limited?: boolean;
}
export interface PaymentTerms {
  customers_days: number;
  suppliers_days: number;
}
export type PayMethod = 'transferencia' | 'recibo' | 'efectivo' | 'tarjeta' | 'otro';
export const PAY_METHODS: Record<PayMethod, string> = {
  transferencia: 'Transferencia',
  recibo: 'Recibo domiciliado',
  efectivo: 'Efectivo',
  tarjeta: 'Tarjeta',
  otro: 'Otra',
};
export interface PartyTerms {
  days: number[];
  pay_day: number | null;
  method: PayMethod;
  treasury: string | null;
  /** IBAN del cliente (recibos) o del proveedor (transferencias). */
  iban?: string | null;
  /** Mandato SEPA del cliente: referencia y fecha de firma. */
  mandate_ref?: string | null;
  mandate_date?: string | null;
  /** Clave y subclave del 190 de un proveedor profesional (G01…G04, H01…H04); null: G.01. */
  key_190?: Key190 | null;
}
export type Key190 = 'G01' | 'G02' | 'G03' | 'G04' | 'H01' | 'H02' | 'H03' | 'H04';
/** Claves del 190 que se pueden fijar a un proveedor (art. 95 RIRPF y art. 101.5 LIRPF). */
export const KEYS_190: Record<Key190, string> = {
  G01: 'G.01 · Profesional, 15 % (general)',
  G02: 'G.02 · Profesional, 7 % (recaudadores, mediadores de seguros, Loterías)',
  G03: 'G.03 · Profesional, 7 % (tres primeros años de actividad)',
  G04: 'G.04 · Profesional, 7 % (menos de 15.000 € el año anterior)',
  H01: 'H.01 · Agrícola o ganadera, 2 %',
  H02: 'H.02 · Engorde de porcino y avicultura, 1 %',
  H03: 'H.03 · Forestal, 2 %',
  H04: 'H.04 · Estimación objetiva (módulos), 1 %',
};
export interface PartyRow {
  account: string;
  role: 'cliente' | 'proveedor';
  tax_id: string;
  name: string;
  terms: PartyTerms | null;
  /** Cuenta de ventas (cliente) o de gastos (proveedor) propia. */
  counterpart?: string | null;
}
export const carteraList = (today: string) => invoke<DueItem[]>('cartera_list', { today });

/** Control de numeración de una serie en un año (0 = serie sin reinicio anual). */
export interface SerieNumeracion {
  serie: string;
  clase: string;
  year: number;
  emitidas: number;
  anuladas: number;
  primera: string | null;
  ultima: string | null;
  huecos: number[];
  fuera_de_orden: string[];
  otra_clase: string[];
}
export const invoiceNumbering = (year: number) =>
  invoke<SerieNumeracion[]>('invoice_numbering', { year });

/** Créditos incobrables (art. 80.Cuatro LIVA). */
export type MedioReclamacion = 'judicial' | 'notarial' | 'fehaciente';
export type ExclusionIncobrable =
  | 'ninguna'
  | 'garantia_real'
  | 'afianzado'
  | 'vinculada'
  | 'concurso';
export interface RequisitoIncobrable {
  clave: string;
  cumplido: boolean;
  texto: string;
}
export interface CreditoIncobrable {
  id: number | null;
  invoice_id: number;
  number: string;
  cliente: string;
  nif: string | null;
  issue_date: string;
  devengo: string;
  total: number;
  pendiente: number;
  base_pendiente: number;
  iva_pendiente: number;
  plazo_meses: number;
  estado: 'candidata' | 'abierto' | 'rectificado' | 'comunicado' | 'cobrado' | 'desistido';
  exclusion: ExclusionIncobrable;
  reclamacion_fecha: string | null;
  reclamacion_medio: MedioReclamacion | null;
  reclamacion_ref: string | null;
  rectificativa: string | null;
  rectificativa_fecha: string | null;
  comunicada_el: string | null;
  comunicacion_ref: string | null;
  notas: string;
  calendario: {
    inicio: string;
    desde: string;
    limite_rectificar: string;
    limite_comunicar: string | null;
  };
  requisitos: RequisitoIncobrable[];
  puede_rectificar: boolean;
  avisos: string[];
  deterioro: string | null;
}
export interface PropuestaR3 {
  draft: InvoiceDraft;
  base: number;
  iva: number;
  requisitos: RequisitoIncobrable[];
  puede: boolean;
}
export interface ComunicacionIncobrable {
  campos: [string, string][];
  adjuntos: string[];
  plazo: string | null;
  donde: string;
}
export const incobrablesList = (today: string) =>
  invoke<CreditoIncobrable[]>('incobrables_list', { today });
export const incobrableOpen = (
  invoiceId: number,
  plazoMeses: number | null,
  exclusion: ExclusionIncobrable,
  confirmed: boolean,
) => invoke<number>('incobrable_open', { invoiceId, plazoMeses, exclusion, confirmed });
export const incobrableAdjust = (
  id: number,
  plazoMeses: number,
  exclusion: ExclusionIncobrable,
  confirmed: boolean,
) => invoke<void>('incobrable_adjust', { id, plazoMeses, exclusion, confirmed });
export const incobrableClaim = (
  id: number,
  fecha: string,
  medio: MedioReclamacion,
  referencia: string,
  confirmed: boolean,
) => invoke<void>('incobrable_claim', { id, fecha, medio, referencia, confirmed });
export const incobrablePreview = (id: number, fecha: string) =>
  invoke<PropuestaR3>('incobrable_preview', { id, fecha });
export const incobrableIssue = (id: number, fecha: string, confirmed: boolean) =>
  invoke<IssuedInvoice>('incobrable_issue', { id, fecha, confirmed });
export const incobrableCommunication = (id: number) =>
  invoke<ComunicacionIncobrable>('incobrable_communication', { id });
export const incobrableCommunicated = (
  id: number,
  fecha: string,
  referencia: string,
  confirmed: boolean,
) => invoke<void>('incobrable_communicated', { id, fecha, referencia, confirmed });
export const incobrableClose = (
  id: number,
  estado: 'cobrado' | 'desistido',
  fecha: string,
  notas: string,
  confirmed: boolean,
) => invoke<string>('incobrable_close', { id, estado, fecha, notas, confirmed });
export const paymentTerms = () => invoke<PaymentTerms>('payment_terms');
export const setPaymentTerms = (terms: PaymentTerms) =>
  invoke<void>('set_payment_terms', { terms });
export const partyTermsList = () => invoke<PartyRow[]>('party_terms_list');
export const setPartyTerms = (account: string, terms: PartyTerms | null) =>
  invoke<void>('set_party_terms', { account, terms });
export const setDueDate = (kind: string, id: number, seq: number, due: string) =>
  invoke<void>('set_due_date', { kind, id, seq, due });
/** Desglosa lo pendiente de una factura; `null` vuelve a calcular los vencimientos. */
export const setInstallments = (
  kind: string,
  id: number,
  pending: { due_date: string; amount: number }[] | null,
) => invoke<void>('set_installments', { kind, id, pending });
/** `amountEur`: euros cobrados o pagados si la factura es en moneda extranjera y no coinciden
 * con lo contabilizado (la diferencia va a 768/668). `method`: forma de cobro o pago de este
 * vencimiento; «efectivo» es lo que el 347 declara como percibido en metálico (> 6.000 € por
 * cliente y año, art. 34.1.h RGAT). */
export const settleInvoice = (
  kind: string,
  id: number,
  seq: number,
  date: string,
  account: string,
  amountEur: number | null = null,
  method: PayMethod | null = null,
) =>
  invoke<number>('settle_invoice', {
    kind,
    id,
    seq,
    date,
    account,
    ...(amountEur === null ? {} : { amountEur }),
    ...(method === null ? {} : { method }),
  });
export const unsettleInvoice = (kind: string, id: number, seq: number, confirmed: boolean) =>
  invoke<void>('unsettle_invoice', { kind, id, seq, confirmed });

/** Espejo de `taxpain_app::remesas`. */
export type RemittanceKind = 'cobros' | 'pagos';
export interface SepaSettings {
  creditor_suffix: string;
  b2b: boolean;
}
export interface RemittanceCandidate {
  item: DueItem;
  iban: string | null;
  mandate_ref: string | null;
  mandate_date: string | null;
  problem: string | null;
}
export interface BankChoice {
  name: string;
  iban: string;
  account: string;
}
export interface RemittanceItem {
  kind: 'emitida' | 'recibida';
  invoice_id: number;
  seq: number;
  number: string;
  party: string;
  amount: number;
  returned_on: string | null;
}
export interface Remittance {
  id: number;
  kind: RemittanceKind;
  reference: string;
  bank_iban: string;
  charge_date: string;
  total: number;
  items: RemittanceItem[];
  in_bank: boolean;
  created_at: string;
}
export const sepaSettings = () => invoke<SepaSettings>('sepa_settings');
export const saveSepaSettings = (settings: SepaSettings) =>
  invoke<void>('save_sepa_settings', { settings });
export const remittanceCandidates = (kind: RemittanceKind, today: string) =>
  invoke<RemittanceCandidate[]>('remittance_candidates', { kind, today });
export const remittanceBanks = () => invoke<BankChoice[]>('remittance_banks');
export const remittanceList = () => invoke<Remittance[]>('remittance_list');
export const remittanceCreate = (
  kind: RemittanceKind,
  items: { invoice_id: number; seq: number }[],
  bankIban: string,
  chargeDate: string,
) => invoke<Remittance>('remittance_create', { kind, items, bankIban, chargeDate });
export const remittanceSave = (id: number, dir: string) =>
  invoke<string>('remittance_save', { id, dir });
export const remittanceUndo = (id: number, confirmed: boolean) =>
  invoke<void>('remittance_undo', { id, confirmed });
export const remittanceReturn = (
  id: number,
  invoiceId: number,
  seq: number,
  date: string,
  account: string | null,
) => invoke<number | null>('remittance_return', { id, invoiceId, seq, date, account });

export const remittanceUndoReturn = (
  id: number,
  invoiceId: number,
  seq: number,
  confirmed: boolean,
) => invoke<void>('remittance_undo_return', { id, invoiceId, seq, confirmed });

/** Espejo de `taxpain_app::templates`. */
export interface TemplateLine {
  /** Con `?` al final se completa al usarla. */
  account: string;
  description: string;
  debit: boolean;
  /** Importe fijo en céntimos, o `null` para pedirlo. */
  amount: number | null;
}
export interface Template {
  id: number;
  name: string;
  concept: string;
  lines: TemplateLine[];
  every_months: number | null;
  next_date: string | null;
  counter: number;
}
export const templatesList = () => invoke<Template[]>('templates_list');
export const templatesDue = (today: string) => invoke<Template[]>('templates_due', { today });
export const templateSave = (template: Template) => invoke<number>('template_save', { template });
export const templateDelete = (id: number) => invoke<void>('template_delete', { id });
export const templatePost = (id: number, entry: JournalEntry) =>
  invoke<number>('template_post', { id, entry });

/** Espejo de `taxpain_app::accounting::ChartAccount`. */
export interface ChartAccount {
  code: string;
  name: string;
  source: 'propia' | 'tercero' | 'pgc';
  nif: string | null;
  debit: number;
  credit: number;
}
export const chart = (year: number) => invoke<ChartAccount[]>('chart', { year });
export const pgcAccounts = () => invoke<[string, string][]>('pgc_accounts');
export const setAccount = (code: string, name: string) =>
  invoke<void>('set_account', { code, name });
export const closeYear = (year: number, confirmed: boolean) =>
  invoke<{ result: number; aviso?: string | null }>('close_year', { year, confirmed });
export const reopenYear = (year: number) =>
  invoke<number>('reopen_year', { year, confirmed: true });
export const fixedAssets = () => invoke<FixedAsset[]>('fixed_assets');
export const addFixedAsset = (asset: FixedAsset) => invoke<number>('add_fixed_asset', { asset });
export const postDepreciation = (year: number) => invoke<number>('post_depreciation', { year });
export const setDepreciationPlan = (
  id: number,
  method: DepreciationMethod,
  taxRateBp: number | null,
) => invoke<void>('set_depreciation_plan', { id, method, taxRateBp });

/** Nombre en español del origen de un asiento (los de `Origin` en crates/core). */
export function originLabel(o: Origin): string {
  if (typeof o === 'string') {
    const simples: Record<string, string> = {
      Nomina: 'Nómina',
      Manual: 'Manual',
      Apertura: 'Apertura',
      Regularizacion: 'Regularización',
      Cierre: 'Cierre',
    };
    return simples[o] ?? 'Otro';
  }
  const conId: Record<string, string> = {
    FacturaEmitida: 'Factura emitida',
    FacturaRecibida: 'Factura recibida',
    Banco: 'Banco',
    Amortizacion: 'Amortización',
    LiquidacionIva: 'Liquidación del IVA',
    Cartera: 'Cobro/pago',
    Importado: 'Importado',
    Gestor: 'Contabilidad del gestor',
    PagoModelo: 'Pago de un modelo',
    BajaInmovilizado: 'Baja de inmovilizado',
    Leasing: 'Leasing',
    Prestamo: 'Préstamo',
    ImpuestoSociedades: 'Impuesto sobre Sociedades',
    AplicacionResultado: 'Aplicación del resultado',
    Dividendo: 'Pago de dividendos',
    GastosViaje: 'Gastos de viaje',
    AnulacionFactura: 'Anulación de factura',
    DevolucionRemesa: 'Devolución de remesa',
    Existencias: 'Regularización de existencias',
    Cripto: 'Criptomonedas',
    AjusteCierre: 'Ajuste de cierre',
    DiferenciasCambio: 'Diferencias de cambio al cierre',
    Valores: 'Inversiones en valores',
    AcuerdoDividendo: 'Acuerdo de dividendos',
    Subvencion: 'Subvención',
  };
  const kind = Object.keys(o)[0];
  return (kind && conId[kind]) || 'Otro';
}

export interface StatementLine {
  id: string;
  label: string;
  amount: number;
  leaf: boolean;
  /** Código de la partida en el depósito de cuentas. */
  code?: string | null;
}

export interface Statement {
  lines: StatementLine[];
  unmapped: AccountBalance[];
}

export interface AnnualAccounts {
  balance: Statement;
  income: Statement;
  balanced: boolean;
}

export interface BankTx {
  id: number;
  bank_account: string;
  op_date: string;
  /** En la divisa de la cuenta (`currency`). */
  amount: number;
  description: string;
  matched: string | null;
  /** Divisa de la cuenta (ISO 4217). */
  currency?: string;
  /** `cuenta`, `tarjeta_credito`, `tarjeta_debito` (sólo informa) o `monedero`. */
  account_kind?: string;
  /** Saldo después del movimiento, si el extracto lo daba. */
  balance?: number | null;
  /** De un periodo ya contabilizado en otro sitio (diario importado o ejercicio cerrado). */
  booked_elsewhere?: boolean;
  /** Cuentas en otra divisa: contravalor en euros (guardado o del tipo del BCE ya descargado). */
  amount_eur?: number | null;
  /** Unidades de la divisa por 1 € («1.0850»). */
  fx_rate?: string | null;
  fx_source?: 'bce' | 'manual' | 'traspaso' | null;
}

/** Lo que salió al comprobar un extracto con sus saldos. */
export type Cuadre =
  | { estado: 'cuadra' }
  | { estado: 'sin_saldos' }
  | { estado: 'no_cuadra'; diferencia: number; fila: number | null; detalle: string };

/** Un extracto de un fichero importado (espejo de `extractos::importar::ExtractoImportado`). */
export interface ExtractoImportado {
  /** «BBVA …1332 EUR». */
  cuenta: string;
  cuenta_id: number | null;
  cuenta_nueva: boolean;
  subcuenta: string | null;
  formato: string;
  clase: 'Cuenta' | 'TarjetaCredito' | 'TarjetaDebito' | 'Monedero';
  divisa: string;
  desde: string | null;
  hasta: string | null;
  movimientos: number;
  movimientos_nuevos: number;
  ya_estaban: number;
  saldos_completados: number;
  /** Cuenta en otra divisa: movimientos a los que el propio extracto ha dado sus euros. */
  con_contravalor?: number;
  /** El extracto en la lista de importados (para deshacerlo). */
  extracto_id?: number | null;
  ya_contabilizados: number;
  cuadre: Cuadre;
  importado: boolean;
  motivo: string;
  avisos: string[];
}

export interface ResumenImportacion {
  fichero: string;
  extractos: ExtractoImportado[];
  movimientos_nuevos: number;
  ya_estaban: number;
}

/** Lo que ha pasado con cada fichero al importar extractos. */
export interface BankFileResult {
  fichero: string;
  resumen: ResumenImportacion | null;
  error: string | null;
}

export interface Suggestion {
  /**
   * `remesa`: el abono o cargo de una remesa SEPA entera (`id` es la remesa).
   * `devolucion`: un recibo devuelto ya anotado en Remesas (`id` es su asiento, `number` la
   * remesa y `party` el concepto).
   */
  kind: 'emitida' | 'recibida' | 'remesa' | 'devolucion';
  id: number;
  seq: number;
  installments: number;
  due_date: string;
  number: string;
  party: string;
  date: string;
  amount: number;
}

export type AccountsModel = 'Pymes' | 'Abreviado' | 'Normal';
export const annualAccounts = (year: number, model: AccountsModel = 'Pymes') =>
  invoke<AnnualAccounts>('annual_accounts', { year, model });
export const settleVat = (year: number, quarter: number) =>
  invoke<number | null>('settle_vat', { year, quarter });
/** Rehace la liquidación del IVA del trimestre (borra el asiento y lo vuelve a hacer). */
export const redoVat = (year: number, quarter: number) =>
  invoke<number | null>('redo_vat', { year, quarter, confirmed: true });
export const exportBooks = (year: number, dir: string) =>
  invoke<string>('export_books', { year, dir });
export interface A3Options {
  company: number;
  digits: number;
  /** Formato del SUENLACE: 5 (512 bytes, el vigente) o 4 (256). */
  format?: 'F4' | 'F5';
  /** Facturas como registros con IVA (1/2 + 9). */
  invoices?: boolean;
}
export const a3Options = () => invoke<A3Options>('a3_options');
export const exportA3 = (year: number, options: A3Options, dir: string) =>
  invoke<{
    path: string;
    entries: number;
    invoices: number;
    accounts: number;
    warnings: string[];
  }>('export_a3', {
    year,
    options,
    dir,
  });
/** Importa extractos de cualquier banco y formato; con `forzar`, también los que no cuadran. */
export const bankImport = (paths: string[], forzar = false) =>
  invoke<BankFileResult[]>('bank_import', { paths, forzar });

/** Un extracto importado (espejo de `extractos_importados::ExtractoGuardado`). */
export interface ExtractoGuardado {
  id: number;
  cuenta_id: number;
  cuenta: string;
  divisa: string;
  fichero: string;
  formato: string;
  desde: string | null;
  hasta: string | null;
  saldo_inicial: number | null;
  saldo_final: number | null;
  movimientos: number;
  nuevos: number;
  /** Los que sólo trae este extracto: se borran al deshacerlo. */
  propios: number;
  /** De ésos, los conciliados. */
  conciliados: number;
  importado_en: string;
  /** Rehecho al actualizar la app con lo importado antes (periodo de los movimientos). */
  heredado: boolean;
}

/** Días entre dos extractos de una cuenta sin extracto que los cubra. */
export interface HuecoExtractos {
  cuenta_id: number;
  cuenta: string;
  desde: string;
  hasta: string;
  /** Los saldos no enlazan: faltan movimientos seguro. */
  seguro: boolean;
  detalle: string;
}

export interface ListaExtractos {
  extractos: ExtractoGuardado[];
  huecos: HuecoExtractos[];
}

export interface ExtractoDeshecho {
  borrados: number;
  desconciliados: number;
  compartidos: number;
}

export const bankStatements = (cuenta: number | null = null) =>
  invoke<ListaExtractos>('bank_statements', { cuenta });
/** Deshace la importación de un extracto; con `desconciliar`, también sus conciliaciones. */
export const bankStatementUndo = (id: number, desconciliar: boolean) =>
  invoke<ExtractoDeshecho>('bank_statement_undo', { id, desconciliar, confirmed: true });
export const bankMovements = (onlyPending: boolean) =>
  invoke<BankTx[]>('bank_movements', { onlyPending });
export const bankSuggestions = (tx: number) => invoke<Suggestion[]>('bank_suggestions', { tx });

// ---------- Conexión con los bancos (solo lectura, Enable Banking) ----------
// Espejo de `taxpain_app::banca`. Importes en céntimos. La clave de la aplicación nunca vuelve.

export interface BancaAjustes {
  /** La empresa ha aceptado que TaxPain se conecte con Enable Banking. */
  aceptado: boolean;
  /** Dirección de vuelta registrada en la aplicación de Enable Banking. */
  url_vuelta: string;
}

export interface CuentaConectada {
  id: number;
  nombre: string;
  /** IBAN enmascarado («ES91 …1332»). */
  iban: string | null;
  divisa: string;
  tarjeta: boolean;
  /** Se descargan sus movimientos. */
  activa: boolean;
  cuenta_id: number | null;
  subcuenta: string | null;
  /** Último día descargado (AAAA-MM-DD). */
  hasta_dia: string | null;
  /** Saldo que da el banco y su fecha. */
  saldo: number | null;
  saldo_fecha: string | null;
}

export interface ConexionBanco {
  id: number;
  banco: string;
  pais: string;
  tipo: 'business' | 'personal';
  estado: 'activa' | 'caducada';
  /** Fin del permiso (RFC 3339) y días que quedan. */
  hasta: string | null;
  dias: number | null;
  actualizada: string | null;
  error: string | null;
  cuentas: CuentaConectada[];
}

export interface BancaEstado {
  ajustes: BancaAjustes;
  /** Id de la aplicación de Enable Banking (no es secreto). */
  app_id: string | null;
  /** Hay clave privada guardada (nunca se devuelve). */
  clave: boolean;
  /** TaxPain recoge sola la vuelta del banco (dirección en 127.0.0.1). */
  vuelta_local: boolean;
  conexiones: ConexionBanco[];
  pendiente: { banco: string; tipo: string; desde: string } | null;
}

/** Un banco de la lista de Enable Banking. */
export interface BancoDisponible {
  name: string;
  country: string;
  psu_types: string[];
  /** Segundos que puede durar el permiso como mucho. */
  maximum_consent_validity: number | null;
  beta: boolean;
}

export interface CuentaActualizada {
  conexion: number;
  banco: string;
  nombre: string;
  resultado: ExtractoImportado | null;
  error: string | null;
}

export interface BancaActualizacion {
  cuentas: CuentaActualizada[];
  movimientos_nuevos: number;
  ya_estaban: number;
}

/** Dirección de vuelta que se propone registrar en Enable Banking. */
export const URL_VUELTA_BANCO = 'https://localhost/taxpain/banco';

export const bancaEstado = () => invoke<BancaEstado>('banca_estado');
export const bancaAjustes = (ajustes: BancaAjustes) =>
  invoke<BancaEstado>('banca_ajustes', { ajustes });
/** Guarda el id de la aplicación y su clave, leída del `.pem` de `path`. */
export const bancaCredenciales = (appId: string, path: string) =>
  invoke<BancaEstado>('banca_credenciales', { appId, path });
export const bancaQuitarCredenciales = () => invoke<BancaEstado>('banca_quitar_credenciales');
export const bancaBancos = () => invoke<BancoDisponible[]>('banca_bancos');
/** Empieza la autorización y abre el banco en el navegador. */
export const bancaConectar = (banco: string, pais: string, tipo: string, maximo: number | null) =>
  invoke<{ escucha: boolean }>('banca_conectar', { banco, pais, tipo, maximo });
/**
 * Espera la vuelta del banco en 127.0.0.1 y baja los movimientos. `null` si se ha dejado de
 * esperar (se ha pegado la dirección o se ha cancelado).
 */
export const bancaEsperar = () => invoke<BancaActualizacion | null>('banca_esperar');
/** Termina con la dirección a la que ha llevado el banco (pegada) y baja los movimientos. */
export const bancaTerminar = (direccion: string) =>
  invoke<BancaActualizacion>('banca_terminar', { direccion });
export const bancaCancelar = () => invoke<BancaEstado>('banca_cancelar');
export const bancaActualizar = (conexion: number | null = null) =>
  invoke<BancaActualizacion>('banca_actualizar', { conexion });
export const bancaActivarCuenta = (cuenta: number, activa: boolean) =>
  invoke<BancaEstado>('banca_activar_cuenta', { cuenta, activa });
/** Retira el permiso y quita la conexión; `aviso` si hay que retirarlo a mano en el banco. */
export const bancaDesconectar = (conexion: number) =>
  invoke<{ estado: BancaEstado; aviso: string | null }>('banca_desconectar', { conexion });

export async function pickPemFile(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const p = await open({
    multiple: false,
    directory: false,
    filters: [{ name: 'Clave privada de la aplicación', extensions: ['pem', 'key'] }],
  });
  return typeof p === 'string' ? p : null;
}
export const bankReconcileInvoice = (
  tx: number,
  kind: string,
  invoice: number,
  seq: number | null = null,
) => invoke<number>('bank_reconcile_invoice', { tx, kind, invoice, seq });
export const bankReconcileRemittance = (tx: number, remittance: number) =>
  invoke<number>('bank_reconcile_remittance', { tx, remittance });
export const bankReconcileReturn = (tx: number, entry: number) =>
  invoke<void>('bank_reconcile_return', { tx, entry });
export const bankReconcileAccount = (tx: number, account: string, concept: string) =>
  invoke<number>('bank_reconcile_account', { tx, account, concept });

/** Movimiento conciliado con lo que se casó (espejo de `conciliados::Reconciled`). */
export interface Reconciled {
  tx: BankTx;
  /** Subcuenta de la cuenta del banco (5720001…). */
  account: string;
  entry_id: number | null;
  kind: 'emitida' | 'recibida' | 'remesa' | 'devolucion' | 'impuesto' | 'traspaso' | 'cuenta';
  /** «Factura A2026-0009 (plazo 2)», «Remesa REM-2026-0001», «Modelo 303 3T 2026», «626». */
  counterpart: string;
  /** Cliente, proveedor o concepto del asiento. */
  party: string;
  reconciled_at: string | null;
  /** Regla del banco que llevó el movimiento a esa cuenta. */
  rule: string | null;
  /** Por qué no se puede deshacer (null si se puede). */
  blocked: string | null;
}
export interface ReconciledList {
  accounts: { account: string; name: string }[];
  items: Reconciled[];
}
export const bankReconciled = (
  account: string | null = null,
  from: string | null = null,
  to: string | null = null,
) => invoke<ReconciledList>('bank_reconciled', { account, from, to });
export const bankUnreconcile = (tx: number, confirmed: boolean) =>
  invoke<void>('bank_unreconcile', { tx, confirmed });

/** Espejo de `taxpain_app::import`. */
export interface ImportPreview {
  source: 'suenlace' | 'tabla';
  entries: number;
  invoices: number;
  accounts: number;
  first_date: string | null;
  last_date: string | null;
  total: number;
  sample: JournalEntry[];
  warnings: string[];
  already: string | null;
}
export interface ImportBatch {
  id: number;
  source: 'suenlace' | 'tabla';
  filename: string;
  entries: number;
  created_at: string;
}
export const importPreview = (path: string) => invoke<ImportPreview>('import_preview', { path });
export const importEntries = (path: string) => invoke<ImportPreview>('import_entries', { path });
export const importBatches = () => invoke<ImportBatch[]>('import_batches');
export const importUndo = (id: number, confirmed: boolean) =>
  invoke<number>('import_undo', { id, confirmed });

/** Espejo de `taxpain_app::apertura` (asistente de saldos iniciales). */
export interface OpeningLine {
  account: string;
  name: string;
  debit: number;
  credit: number;
}
export interface OpeningPending {
  /** Subcuenta del balance del gestor; `null`: la que ya tenga en TaxPain o una nueva. */
  account: string | null;
  tax_id: string | null;
  name: string;
  number: string;
  issue_date: string;
  due_date: string;
  amount: number;
}
export interface OpeningAsset {
  name: string;
  account: string;
  depreciation_account: string;
  expense_account: string;
  acquired_on: string;
  cost: number;
  /** Coeficiente anual en puntos básicos. */
  rate: number;
  method: DepreciationMethod;
  /** Amortización acumulada hasta el día anterior a la apertura. */
  accumulated: number;
}
export interface OpeningBank {
  account: string;
  name: string;
  balance: number;
}
export interface OpeningTax {
  vat_refund: number;
  vat_due: number;
  withholdings: number;
}
export interface OpeningDraft {
  date: string | null;
  step: number;
  source: string | null;
  lines: OpeningLine[];
  customers: OpeningPending[];
  suppliers: OpeningPending[];
  assets: OpeningAsset[];
  banks: OpeningBank[];
  tax: OpeningTax;
}
export interface OpeningBalanceRead {
  kind: 'sumas_saldos' | 'apertura' | 'cierre' | 'diario';
  lines: OpeningLine[];
  date: string | null;
  warnings: string[];
}
export interface OpeningPendingRead {
  items: OpeningPending[];
  from_book: boolean;
  warnings: string[];
}
export interface OpeningBankInfo {
  account: string;
  name: string;
  iban: string | null;
  currency: string;
  kind: string;
  statement_balance: number | null;
}
export interface OpeningSummaryLine {
  account: string;
  name: string;
  debit: number;
  credit: number;
  from: 'balance' | 'clientes' | 'proveedores' | 'inmovilizado' | 'hacienda' | 'bancos';
  new_account: boolean;
}
export interface OpeningSummary {
  fiscal_year: number | null;
  fiscal_year_label: string | null;
  fiscal_year_start: string | null;
  lines: OpeningSummaryLine[];
  debit: number;
  credit: number;
  difference: number;
  errors: string[];
  warnings: string[];
  duplicate: string | null;
  /** Lo que se hará además del asiento (p. ej. la cuota a compensar del 303). */
  also?: string[];
}
export interface OpeningApplied {
  id: number;
  fiscal_year: number;
  date: string;
  entry_id: number | null;
  entry_number: number | null;
  pending: number;
  assets: number;
  created_at: string;
}
/** Amortización de un elemento anterior a TaxPain (CONTABILIDAD-05). */
export interface AssetPrior {
  id: number;
  amount: number;
  until: string | null;
}
export const emptyOpeningDraft = (): OpeningDraft => ({
  date: null,
  step: 0,
  source: null,
  lines: [],
  customers: [],
  suppliers: [],
  assets: [],
  banks: [],
  tax: { vat_refund: 0, vat_due: 0, withholdings: 0 },
});
export const aperturaDraft = () => invoke<OpeningDraft>('apertura_draft');
export const aperturaSave = (draft: OpeningDraft) => invoke<void>('apertura_save', { draft });
export const aperturaDiscard = () => invoke<void>('apertura_discard');
export const aperturaReadBalance = (path: string) =>
  invoke<OpeningBalanceRead>('apertura_read_balance', { path });
export const aperturaReadPending = (kind: 'emitida' | 'recibida', path: string) =>
  invoke<OpeningPendingRead>('apertura_read_pending', { kind, path });
export const aperturaBanks = (date: string | null) =>
  invoke<OpeningBankInfo[]>('apertura_banks', { date });
export const aperturaSummary = (draft: OpeningDraft) =>
  invoke<OpeningSummary>('apertura_summary', { draft });
export const aperturaApply = (draft: OpeningDraft, allowDuplicate: boolean) =>
  invoke<OpeningApplied>('apertura_apply', { draft, allowDuplicate });
export const aperturaApplied = () => invoke<OpeningApplied[]>('apertura_applied');
export const aperturaUndo = (id: number, confirmed: boolean) =>
  invoke<void>('apertura_undo', { id, confirmed });

/** Espejo de `taxpain_app::ejercicio_gestor` (importar un ejercicio entero del gestor). */
export interface GestorNewAccount {
  code: string;
  name: string;
}
export interface GestorPreview {
  source: 'diario' | 'resumido' | 'suenlace';
  filename: string;
  fiscal_year: number | null;
  fiscal_year_label: string | null;
  entries: number;
  lines: number;
  first_date: string | null;
  last_date: string | null;
  total: number;
  /** Resultado del ejercicio según el diario (negativo: pérdida). */
  result: number;
  has_opening: boolean;
  has_regularization: boolean;
  has_closing: boolean;
  /** Se creará la apertura del ejercicio siguiente (trae el cierre). */
  next_opening: boolean;
  /**
   * El ejercicio siguiente tiene su propio diario (en TaxPain o junto al fichero) con su apertura:
   * cuál. La del cierre no se marca, porque los saldos iniciales irían dos veces.
   */
  next_ledger?: string | null;
  accounts: number;
  new_accounts: GestorNewAccount[];
  sample: JournalEntry[];
  errors: string[];
  warnings: string[];
}
export interface GestorOptions {
  next_opening: boolean;
}
export interface GestorLot {
  id: number;
  fiscal_year: number;
  source: 'diario' | 'resumido' | 'suenlace';
  filename: string;
  entries: number;
  lines: number;
  accounts_created: number;
  has_opening: boolean;
  has_regularization: boolean;
  has_closing: boolean;
  next_opening: boolean;
  closed_year: boolean;
  created_at: string;
}
export interface GestorBalanceRow {
  account: string;
  name: string;
  theirs: number | null;
  ours: number;
  diff: number;
}
export interface GestorBalanceComparison {
  fiscal_year: number;
  basis: 'tras_regularizacion' | 'antes_regularizacion';
  rows: GestorBalanceRow[];
  mismatches: number;
  warnings: string[];
}
export interface GestorModelRow {
  model: string;
  period: string;
  box: number;
  label: string;
  ours: number;
  theirs: number | null;
  diff: number;
}
export interface GestorModelsComparison {
  fiscal_year: number;
  rows: GestorModelRow[];
  mismatches: number;
  warnings: string[];
}
export interface OpeningInventoryRead {
  assets: OpeningAsset[];
  warnings: string[];
}
/** Un bien del inventario del gestor para darlo de alta en Inmovilizado. */
export interface FixedAssetInventoryItem {
  asset: OpeningAsset;
  /** Ya está en Inmovilizado: no se da de alta otra vez. */
  existing: number | null;
  /** Por qué no entra (un terreno, un bien sin cuenta). */
  skip: string | null;
}
export interface FixedAssetInventoryPreview {
  items: FixedAssetInventoryItem[];
  /** Hasta qué día llega lo ya amortizado (el final del periodo o del ejercicio del listado). */
  until: string | null;
  warnings: string[];
}
export interface FixedAssetInventoryImported {
  added: number;
  existing: number;
  skipped: number;
  warnings: string[];
}
export const gestorPreview = (path: string) => invoke<GestorPreview>('gestor_preview', { path });
export const gestorImport = (path: string, year: number, options: GestorOptions) =>
  invoke<GestorLot>('gestor_import', { path, year, options });
export const gestorLots = () => invoke<GestorLot[]>('gestor_lots');
export const gestorUndo = (id: number, confirmed: boolean) =>
  invoke<number>('gestor_undo', { id, confirmed });
export const gestorCompareBalance = (year: number, path: string) =>
  invoke<GestorBalanceComparison>('gestor_compare_balance', { year, path });
export const gestorCompareModels = (year: number) =>
  invoke<GestorModelsComparison>('gestor_compare_models', { year });
export const aperturaReadInventory = (path: string) =>
  invoke<OpeningInventoryRead>('apertura_read_inventory', { path });
export const addFixedAssetWithPrior = (asset: FixedAsset, prior: number, until: string | null) =>
  invoke<number>('fixed_asset_add_with_prior', { asset, prior, until });
export const fixedAssetPriors = () => invoke<AssetPrior[]>('fixed_asset_priors');
export const fixedAssetInventoryPreview = (path: string) =>
  invoke<FixedAssetInventoryPreview>('fixed_asset_inventory_preview', { path });
export const fixedAssetInventoryImport = (assets: OpeningAsset[], until: string) =>
  invoke<FixedAssetInventoryImported>('fixed_asset_inventory_import', { assets, until });

/** Elige el balance de sumas y saldos, el diario o el listado de pendientes del gestor. */
export async function pickOpeningFile(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const p = await open({
    multiple: false,
    filters: [
      {
        name: 'Balance, diario o listado (Excel, OpenDocument, CSV, SUENLACE)',
        extensions: ['xlsx', 'xls', 'ods', 'csv', 'txt', 'dat'],
      },
    ],
  });
  return typeof p === 'string' ? p : null;
}

/** Espejo de `taxpain_app::legalizacion`. */
export interface LegalizationRequest {
  year: number;
  diary_number: number;
  inventory_number: number;
  summarized: boolean;
  address: string | null;
  town: string | null;
  postal_code: string | null;
  province_code: string | null;
  registry_code: string | null;
  tomo: string | null;
  folio: string | null;
  hoja: string | null;
  presenter_name: string | null;
  presenter_nif: string | null;
  presenter_email: string | null;
  presenter_phone: string | null;
  /** Número del libro de actas, del registro de socios y del de contratos del socio único, si
   * van en el paquete (null: no van). */
  minutes_number?: number | null;
  partners_number?: number | null;
  contracts_number?: number | null;
}
export interface LegalizationSaved {
  path: string;
  filename: string;
  books: { file: string; description: string; number: number; huella: string }[];
  warnings: string[];
  deadline: string;
}
export const legalizationBuild = (request: LegalizationRequest, dir: string) =>
  invoke<LegalizationSaved>('legalization_build', { request, dir });

/** Espejo de `taxpain_app::analitica` y `taxpain_app::presupuestos`. */
export interface CostCenter {
  code: string;
  name: string;
  /** Centro del que cuelga (hasta cuatro niveles). */
  parent?: string | null;
}
/** Reparto secundario de un centro entre otros. */
export interface CostRedistribution {
  source: string;
  shares: Share[];
}
/** Parte de un centro, en centésimas de punto (2500 = 25 %). */
export interface Share {
  center: string;
  pct_bp: number;
}
export interface CostRule {
  prefix: string;
  shares: Share[];
}
export interface AnalyticLine {
  line_id: number;
  date: string;
  concept: string;
  account: string;
  amount: number;
  shares: Share[];
  source: 'linea' | 'regla' | 'sin';
}
export interface CenterResult {
  center: string;
  name: string;
  income: number;
  expenses: number;
  result: number;
}
export interface BudgetLine {
  account: string;
  center: string;
  months: number[];
}
export interface Deviation {
  account: string;
  name: string;
  center: string;
  budget: number;
  actual: number;
  deviation: number;
  deviation_bp: number | null;
  unfavourable: boolean;
}
export const costCenters = () => invoke<CostCenter[]>('cost_centers');
export const costCenterSave = (center: CostCenter) => invoke<void>('cost_center_save', { center });
export const costCenterDelete = (code: string, confirmed: boolean) =>
  invoke<void>('cost_center_delete', { code, confirmed });
export const costRules = () => invoke<CostRule[]>('cost_rules');
export const costRuleSet = (prefix: string, shares: Share[]) =>
  invoke<void>('cost_rule_set', { prefix, shares });
export const analyticLines = (year: number, from: string | null = null, to: string | null = null) =>
  invoke<AnalyticLine[]>('analytic_lines', { year, from, to });
export const analyticLineSet = (lineId: number, shares: Share[]) =>
  invoke<void>('analytic_line_set', { lineId, shares });
export const analyticReport = (
  year: number,
  from: string | null = null,
  to: string | null = null,
  level: number | null = null,
) => invoke<CenterResult[]>('analytic_report', { year, from, to, level });
export const costRedistributions = () => invoke<CostRedistribution[]>('cost_redistributions');
export const costRedistributionSet = (source: string, shares: Share[]) =>
  invoke<void>('cost_redistribution_set', { source, shares });
export const budgetList = (year: number) => invoke<BudgetLine[]>('budget_list', { year });
export const budgetSet = (year: number, line: BudgetLine) =>
  invoke<void>('budget_set', { year, line });
export const budgetFromActuals = (year: number, source: number, digits: number, changeBp: number) =>
  invoke<number>('budget_from_actuals', { year, source, digits, changeBp });
export const budgetReport = (year: number, through: number) =>
  invoke<Deviation[]>('budget_report', { year, through });

/** Espejo de `taxpain_app::ratios`. Valores en centésimas (en euros, céntimos). */
export interface Ratio {
  id: string;
  group: string;
  name: string;
  formula: string;
  explanation: string;
  unit: 'euros' | 'veces' | 'porcentaje' | 'dias';
  value: number | null;
  previous: number | null;
  range: [number, number] | null;
  status: 'bien' | 'atencion' | 'sin_datos';
}
export const financialRatios = (year: number) => invoke<Ratio[]>('financial_ratios', { year });

/** Espejo de `taxpain_app::existencias`. */
export interface StockLine {
  account: string;
  name: string;
  initial: number;
  final_amount: number;
}
export const stockLines = (year: number) => invoke<StockLine[]>('stock_lines', { year });
export const stockRegularize = (year: number, finals: { account: string; amount: number }[]) =>
  invoke<number | null>('stock_regularize', { year, finals });

/** Espejo de `taxpain_app::patrimonial` (art. 5.2 LIS y arts. 327 y 363.1.e LSC). */
export type ClasePatrimonial = 'afecto' | 'no_afecto' | 'valores' | 'participacion' | 'tesoreria';
export interface PartidaPatrimonial {
  cuenta: string;
  nombre: string;
  clase: ClasePatrimonial;
  propuesta: ClasePatrimonial;
  marcada: boolean;
  motivo: string;
  desde: number | null;
  correctora: boolean;
  saldos: number[];
}
export interface TrimestrePatrimonial {
  fecha: string;
  cerrado: boolean;
  activo: number;
  afecto: number;
  valores: number;
  no_afecto: number;
  tesoreria: number;
  participaciones: number;
  tesoreria_justificada: number;
  ventas_excluidas: number;
  tesoreria_no_justificada: number;
  computa: number;
  /** Centésimas de punto (5230 = 52,3 %). */
  pct: number;
  computa_lip: number;
  pct_lip: number;
}
export interface MediaPatrimonial {
  trimestres: number;
  activo: number;
  valores: number;
  no_afecto: number;
  participaciones: number;
  computa: number;
  pct: number;
  pct_con_participaciones: number;
  /** Sin la tesorería no justificada. */
  pct_sin_tesoreria: number;
  margen: number;
}
export interface AjustesPatrimonial {
  tesoreria_afecta: number;
  tesoreria_motivo: string;
  beneficios_actividad: number | null;
  beneficios_motivo: string;
  prestamos_participativos: number;
  dominante: boolean;
}
export interface VentaPatrimonial {
  id: number;
  fecha: string;
  importe: number;
  motivo: string;
}
export interface NotaPatrimonial {
  titulo: string;
  texto: string;
  norma: string;
}
export interface PatrimonioFecha {
  fecha: string;
  cerrado: boolean;
  capital: number;
  contable: number;
  capital_ajustes: number;
  coberturas: number;
  prestamos_participativos: number;
  mercantil: number;
  pct_capital: number;
}
export interface AvisoMercantil {
  codigo:
    | 'causa_disolucion'
    | 'reduccion_obligatoria'
    | 'dos_tercios'
    | 'sin_dividendos'
    | 'patrimonio_negativo';
  gravedad: 'warning' | 'info';
  titulo: string;
  detalle: string;
  plazo: string | null;
  norma: string;
}
export interface InformePatrimonial {
  year: number;
  fechas: string[];
  partidas: PartidaPatrimonial[];
  trimestres: TrimestrePatrimonial[];
  media: MediaPatrimonial;
  patrimonial: boolean;
  provisional: boolean;
  trimestres_lip: number;
  ajustes: AjustesPatrimonial;
  ventas: VentaPatrimonial[];
  beneficios_propuestos: number;
  consecuencias: NotaPatrimonial[];
  que_revisar: NotaPatrimonial[];
  avisos: string[];
  patrimonio: {
    forma: 'sa' | 'sl' | 'otra' | null;
    fechas: PatrimonioFecha[];
    anterior: PatrimonioFecha | null;
    avisos: AvisoMercantil[];
    notas: NotaPatrimonial[];
  };
}
export const patrimonialReport = (year: number) =>
  invoke<InformePatrimonial>('patrimonial_report', { year });
export const patrimonialMark = (
  year: number,
  cuenta: string,
  clase: ClasePatrimonial,
  motivo: string,
) => invoke<void>('patrimonial_mark', { year, cuenta, clase, motivo });
export const patrimonialSettings = (year: number, ajustes: AjustesPatrimonial) =>
  invoke<void>('patrimonial_settings', { year, ajustes });
export const patrimonialSaleAdd = (fecha: string, importe: number, motivo: string) =>
  invoke<number>('patrimonial_sale_add', { fecha, importe, motivo });
export const patrimonialSaleDelete = (id: number, confirmed: boolean) =>
  invoke<void>('patrimonial_sale_delete', { id, confirmed });

/** Espejo de `taxpain_app::iva_holding` (arts. 9.1.º.c, 93.Cinco, 101, 104 y 107 a 110 LIVA). */
export type AmbitoRegla = 'proveedor' | 'cuenta';
export interface ReglaAfectacion {
  id: number;
  ambito: AmbitoRegla;
  clave: string;
  pct: number;
  sector_id: number | null;
  criterio: string;
  desde: string;
}
export interface SectorIva {
  id: number;
  nombre: string;
  cnae: string;
  pct: number;
  volumen: number;
  notas: string;
}
export type PapelSector = 'principal' | 'mismo_sector' | 'accesoria' | 'diferenciado' | 'sin_cnae';
export interface FilaSector {
  id: number;
  nombre: string;
  cnae: string;
  grupo: string;
  pct: number;
  volumen: number;
  diferencia: number;
  papel: PapelSector;
}
export interface ComprobacionSectores {
  filas: FilaSector[];
  hay_diferenciados: boolean;
  explicacion: string;
}
export interface AplicacionRegla {
  recibida_id: number;
  numero: string;
  proveedor: string;
  nif: string;
  registro: string;
  cuota: number;
  pct_factura: number;
  pct_aplicado: number;
  regla_id: number;
  criterio: string;
  sector: string | null;
  no_deducido: number;
}
export type PerfilHolding =
  | 'sin_cartera'
  | 'actividad_con_cartera'
  | 'holding_mixta'
  | 'holding_pura';
export interface NotaIva {
  titulo: string;
  texto: string;
  norma: string;
  por_cotejar: boolean;
}
export interface DiagnosticoHolding {
  year: number;
  perfil: PerfilHolding;
  perfil_texto: string;
  datos: {
    participadas: number;
    coste_participadas: number;
    dividendos: number;
    financieros: number;
    prestamos_participadas: number;
    intereses_participadas: number;
    ingresos_actividad: number;
    servicios_participadas: number;
  };
  senales: { clave: string; titulo: string; importe: number; detalle: string }[];
  notas: NotaIva[];
  prorrata_facturas: number | null;
  prorrata_con_financieros: number | null;
  proporcion_93: number | null;
  prorrata_configurada: number | null;
  prorrata_especial: boolean;
  config: { sectores_diferenciados: boolean };
  reglas: ReglaAfectacion[];
  sectores: SectorIva[];
  comprobacion: ComprobacionSectores;
  aplicaciones: AplicacionRegla[];
  no_deducido: number;
  avisos: string[];
}
export type VentaIva = 'sujeta' | 'exenta' | 'no_sujeta' | 'transmision_empresa' | 'perdida';
export interface AnioBien {
  year: number;
  prorrata: number | null;
  diferencia: number | null;
  importe: number;
  nota: string;
}
export interface FilaBienInversion {
  asset_id: number | null;
  received_id: number | null;
  nombre: string;
  factura: string;
  nif: string;
  fecha: string;
  inmueble: boolean;
  base: number;
  cuota: number;
  pct_factura: number;
  prorrata_compra: number | null;
  deduccion_inicial: number | null;
  fin_periodo: number;
  anios: AnioBien[];
  venta: {
    fecha: string;
    tipo: VentaIva | null;
    iva: number;
    regularizacion: number | null;
    anios_restantes: number;
    origen: string;
  } | null;
  avisos: string[];
}
export interface LibroBienesInversion {
  year: number;
  con_prorrata: boolean;
  bienes: FilaBienInversion[];
  total_anual: number;
  total_ventas: number;
  sin_datos: { asset_id: number; nombre: string; cuenta: string; adquirido: string }[];
  avisos: string[];
}
export interface DatosBienIva {
  cuota: number | null;
  deducible: number | null;
  inmueble: boolean;
  venta: VentaIva | null;
  iva_venta: number | null;
}
export const ivaHoldingInforme = (year: number) =>
  invoke<DiagnosticoHolding>('iva_holding_informe', { year });
export const ivaHoldingLibro = (year: number) =>
  invoke<LibroBienesInversion>('iva_holding_libro', { year });
export const ivaHoldingReglaGuardar = (regla: ReglaAfectacion) =>
  invoke<number>('iva_holding_regla_guardar', { regla });
export const ivaHoldingReglaBorrar = (id: number, confirmed: boolean) =>
  invoke<void>('iva_holding_regla_borrar', { id, confirmed });
export const ivaHoldingSectorGuardar = (sector: SectorIva) =>
  invoke<number>('iva_holding_sector_guardar', { sector });
export const ivaHoldingSectorBorrar = (id: number, confirmed: boolean) =>
  invoke<void>('iva_holding_sector_borrar', { id, confirmed });
export const ivaHoldingConfig = (config: { sectores_diferenciados: boolean }) =>
  invoke<void>('iva_holding_config', { config });
export const ivaHoldingBienGuardar = (assetId: number, datos: DatosBienIva) =>
  invoke<void>('iva_holding_bien_guardar', { assetId, datos });

/** Espejo de `taxpain_app::deposito`. */
export interface DepositRequest {
  year: number;
  model: AccountsModel;
  registry_code: string;
  legal_form: 'Sa' | 'Sl' | 'Otra';
  activity: string;
  cnae: string;
  address: string | null;
  town: string | null;
  postal_code: string | null;
  staff_permanent: string | null;
  staff_temporary: string | null;
  micro_criteria: boolean;
  signers: string[];
  /** Sin PDF adjunto, incluir la memoria que prepara TaxPain. */
  memoria_taxpain?: boolean;
}
export interface DepositSaved {
  path: string;
  filename: string;
  huella: string;
  claves: number;
  warnings: string[];
}
export const depositBuild = (request: DepositRequest, memoria: string | null, dir: string) =>
  invoke<DepositSaved>('deposit_build', { request, memoria, dir });

export async function pickPdf(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const p = await open({ multiple: false, filters: [{ name: 'PDF', extensions: ['pdf'] }] });
  return typeof p === 'string' ? p : null;
}

export async function pickImportFile(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const p = await open({
    multiple: false,
    filters: [
      {
        name: 'Asientos (SUENLACE, Excel, OpenDocument, CSV)',
        extensions: ['dat', 'xlsx', 'xls', 'ods', 'csv', 'txt'],
      },
    ],
  });
  return typeof p === 'string' ? p : null;
}

/** Extensiones de los extractos que se leen (de cualquier banco). */
export const STATEMENT_EXTENSIONS = [
  'pdf',
  'xlsx',
  'xls',
  'xlsm',
  'ods',
  'csv',
  'tsv',
  'txt',
  'n43',
  'aeb',
  'csb',
  'q43',
  'c43',
  'xml',
  'camt',
  '053',
  'ofx',
  'qfx',
  'qif',
  'sta',
  '940',
  'mt940',
  'htm',
  'html',
  'jpg',
  'jpeg',
  'png',
  'heic',
];

/**
 * Avisos de una importación de extractos que cambian lo que ha entrado (o lo que no ha entrado):
 * conviene mirarlos antes de conciliar.
 */
export function avisoDestacado(aviso: string): boolean {
  return /dado la vuelta|al revés|sin signo|Posible duplicado|aunque no cuadra|otra divisa|sin fecha o sin importe/i.test(
    aviso,
  );
}

export async function pickStatementFiles(): Promise<string[]> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const p = await open({
    multiple: true,
    filters: [{ name: 'Extractos bancarios', extensions: STATEMENT_EXTENSIONS }],
  });
  if (!p) return [];
  return Array.isArray(p) ? p : [p];
}

// ---------- Modelos ----------
// Espejo de `taxpain_app::models`. Importes en céntimos.

export interface ModelSettings {
  iban: string | null;
  domiciliar: boolean;
  vat_compensation_carried: number;
  /** Año al empezar el cual quedaba ese IVA por compensar (si no, el primero sin datos). */
  vat_compensation_year?: number | null;
  request_refund: boolean;
  exempt_from_390: boolean;
  /** Inscrita en el REDEME (art. 30 RIVA): 303 mensual, devolución cada mes y SII. */
  redeme?: boolean;
  /** Acogida voluntariamente al SII. */
  sii_voluntario?: boolean;
  /** Régimen especial del criterio de caja del IVA. */
  cash_basis?: boolean;
  /** Prorrata: porcentaje provisional (null = sin prorrata). */
  prorata?: number | null;
  /** Prorrata especial: el porcentaje sólo se aplica a las compras de uso común. */
  prorata_special?: boolean;
  /** Prorratas definitivas de años anteriores a TaxPain (año → %), para regularizar [43]. */
  prorata_history?: Record<string, number>;
  /** Descripción breve de la actividad (apartado de prorratas del 390). */
  activity_description?: string;
  previous_year_net: number | null;
  corporate_tax_base: number | null;
  corporate_tax_base_1p?: number | null;
  cnae: string;
  corporate_tax_rate: string;
  turnover_over_6m: boolean;
  turnover_under_1m: boolean;
  corporate_regime: 'General' | 'Micro' | 'ReducidaDimension' | 'NuevaCreacion';
  corporate_fines: number;
  /** 200: modelo de cuentas anuales de las páginas de balance y PyG. */
  accounts_model?: 'Pymes' | 'Abreviado' | 'Normal';
  corporate_increases: number;
  corporate_decreases: number;
  capitalization_reserve: number;
  pending_losses: number;
  /** Datos de cada ejercicio (130, 202 y 200). Los campos sueltos son de la versión antigua. */
  years?: Record<string, YearSettings>;
}

/** Datos de un ejercicio para el 130, el 202 y el 200. */
export interface YearSettings {
  previous_year_net: number | null;
  corporate_tax_base: number | null;
  corporate_tax_base_1p: number | null;
  corporate_regime: ModelSettings['corporate_regime'] | null;
  corporate_fines: number;
  corporate_increases: number;
  corporate_decreases: number;
  capitalization_reserve: number;
  pending_losses: number;
  /** Modalidad de estimación directa (130 y Renta); null = sin indicar. */
  irpf_regime?: IrpfRegime | null;
  /** Renta: reducción del art. 32.2 LIRPF. */
  activity_reduction?: Reduction32;
  /** Renta: otras rentas de más de 6.500 €. */
  other_income_over_6500?: boolean;
  /** Renta: reducción por inicio de actividad (art. 32.3 LIRPF). */
  activity_start?: boolean;
  /** 202: modalidad elegida para el ejercicio (null = sin elegir: la de la cuota). */
  m202_modalidad?: Modalidad202 | null;
}

export interface ModelDraft {
  model: string;
  year: number;
  quarter: number;
  /** [casilla, importe en céntimos] */
  boxes: [number, number][];
  /** [casilla, nº de perceptores] */
  payees: [number, number][];
  /** Detalle por operador (349): [descripción, importe en céntimos] */
  details: [string, number][];
  declaration: string;
  result: number;
  warnings: string[];
  file_name: string;
  file: string;
}

export const modelSettings = () => invoke<ModelSettings>('model_settings');
export const saveModelSettings = (settings: ModelSettings) =>
  invoke<void>('save_model_settings', { settings });
/** Periodo de un borrador: 1–4 trimestre; 101–112 mes (101 = enero). */
export const MES = 100;
export const MESES = [
  'Enero',
  'Febrero',
  'Marzo',
  'Abril',
  'Mayo',
  'Junio',
  'Julio',
  'Agosto',
  'Septiembre',
  'Octubre',
  'Noviembre',
  'Diciembre',
];
/** Modelos que pueden ser mensuales (gran empresa, REDEME, SII, 349 mensual). */
export const MENSUALES = ['303', '111', '115', '123', '349'];
export const modelDraft = (model: string, year: number, quarter: number) =>
  invoke<ModelDraft>('model_draft', { model, year, quarter });
/** ¿Se presenta el modelo cada mes según el perfil fiscal? (null: sin perfil.) */
export const modelMonthly = (model: string) => invoke<boolean | null>('model_monthly', { model });

export interface SiiEstado {
  obligatorio: boolean;
  voluntario: boolean;
  motivos: string[];
  avisos: string[];
  /** TaxPain genera los mensajes del SII (hoy no: faltan los esquemas oficiales). */
  formato_sii: boolean;
  norma: string;
}
export interface SiiPlazo {
  libro: 'emitida' | 'recibida';
  numero: string;
  nif: string;
  nombre: string;
  /** Expedición (emitidas) o registro (recibidas), aaaa-mm-dd. */
  fecha: string;
  /** Último día para enviarla al SII. */
  limite: string;
  /** Días hasta el límite (negativo: vencido). */
  dias: number;
  aproximado: boolean;
}
export const siiEstado = () => invoke<SiiEstado>('sii_estado');
export const siiPlazos = (desde: string, hasta: string) =>
  invoke<SiiPlazo[]>('sii_plazos', { desde, hasta });
export const modelExport = (model: string, year: number, quarter: number, dir: string) =>
  invoke<string>('model_export', { model, year, quarter, dir });

/** Espejo de `rectificativa::Anterior`: lo presentado de un periodo. */
export interface RectificativaAnterior {
  /** «taxpain», «aeat» (según la comprobación con la AEAT) o «gestor» (importado o tecleado). */
  fuente: 'taxpain' | 'aeat' | 'gestor';
  justificante: string | null;
  presentada: string | null;
  declaracion: string | null;
  casillas: [number, number][];
  resultado: number;
}

/** Espejo de `rectificativa::Opciones`. */
export interface RectificativaOpciones {
  justificante?: string | null;
  /** Fecha prevista de presentación (AAAA-MM-DD; por defecto, hoy). */
  fecha?: string | null;
  /** Lo que la AEAT ya devolvió de las anteriores, en céntimos ([109] del 303). */
  devuelto?: number | null;
  baja_domiciliacion?: boolean;
}

/** Espejo de `rectificativa::Recargo27`. */
export interface Recargo27 {
  porcentaje: number;
  importe: number;
  meses: number;
  intereses: number;
  intereses_desde: string | null;
  reducido: number;
}

/** Espejo de `rectificativa::Rectificativa`. */
export interface Rectificativa {
  model: string;
  year: number;
  quarter: number;
  period: string;
  cauce: 'rectificativa' | 'complementaria' | 'otra_via' | 'sin_cambios';
  cauce_texto: string;
  anterior: RectificativaAnterior;
  justificante: string | null;
  filas: { casilla: number; presentado: number; ahora: number; diferencia: number }[];
  ingresado_antes: number;
  devuelto_antes: number;
  total: number;
  resultado: number;
  a_ingresar: number;
  a_devolver: number;
  a_compensar: number;
  plazo: string | null;
  fecha: string;
  recargo: Recargo27 | null;
  intereses_devolucion: number;
  total_a_pagar: number;
  declaracion: string;
  avisos: string[];
  file_name: string;
  file: string;
  /** [desde cuándo, norma] */
  calendario: [string, string] | null;
}

/** Espejo de `rectificativa::Rectificable`: un modelo presentado que se puede rectificar. */
export interface Rectificable {
  modelo: string;
  ejercicio: number;
  trimestre: number;
}

/** Modelos que se rectifican desde Modelos. */
export const RECTIFICABLES = ['303', '111', '115', '123', '130', '202', '200'];

export const rectificativaAnterior = (model: string, year: number, quarter: number) =>
  invoke<RectificativaAnterior | null>('rectificativa_anterior', { model, year, quarter });
export const rectificativaPreparar = (
  model: string,
  year: number,
  quarter: number,
  opciones: RectificativaOpciones,
) => invoke<Rectificativa>('rectificativa_preparar', { model, year, quarter, opciones });
export const rectificativaExportar = (
  model: string,
  year: number,
  quarter: number,
  opciones: RectificativaOpciones,
  dir: string,
) => invoke<string>('rectificativa_exportar', { model, year, quarter, opciones, dir });

export interface PrevalidationResult {
  valid: boolean;
  messages: string[];
  pdf_path: string | null;
}

/** Modelos que la AEAT prevalida con ServValiDos. */
export const PREVALIDATABLE = ['111', '115', '123', '130', '200', '202', '303', '390'];

export const modelPrevalidate = (
  model: string,
  year: number,
  quarter: number,
  fingerprint: string,
  pdfDir: string | null,
) =>
  invoke<PrevalidationResult>('model_prevalidate', {
    model,
    year,
    quarter,
    fingerprint,
    pdfDir,
    confirmed: true,
  });

export interface FiledReceipt {
  csv: string;
  receipt_number: string;
  date: string;
  time: string;
  pdf_url: string;
  notices: string[];
  warnings: string[];
}

export interface FilingResult {
  accepted: boolean;
  errors: string[];
  receipt: FiledReceipt | null;
}

export interface FiledReturn {
  id: number;
  model: string;
  year: number;
  period: string;
  environment: 'pruebas' | 'produccion';
  declaration: string;
  result: number;
  csv: string;
  receipt_number: string;
  pdf_url: string;
  presented_at: string;
  has_receipt_pdf: boolean;
}

/** Modelos que admite la presentación directa. */
export const PRESENTABLE = ['111', '115', '123', '130', '200', '202', '303', '390'];

export const modelFileTesting = (
  model: string,
  year: number,
  quarter: number,
  fingerprint: string,
  nrc: string | null,
  reviewedFile: string,
  retryAfterUnknown = false,
) =>
  invoke<FilingResult>('model_file_testing', {
    model,
    year,
    quarter,
    fingerprint,
    nrc,
    reviewedFile,
    retryAfterUnknown,
    confirmed: true,
  });
export const filedReturns = (year: number | null) =>
  invoke<FiledReturn[]>('filed_returns', { year });

/** Declaración presentada según la AEAT (servicio de consulta, sólo lectura). */
export interface AeatPresented {
  year: string;
  model: string;
  period: string;
  receipt_number: string;
  previous_receipt: string | null;
  presented_at: string;
}

export const aeatPresented = (model: string, year: number, fingerprint: string) =>
  invoke<AeatPresented[]>('aeat_presented', { model, year, fingerprint });

/** Estado de un modelo y periodo en la comprobación del año. */
export type AeatCheckStatus = 'Presented' | 'Missing' | 'Open' | 'NotFound';

/** Fila de «Comprobar lo presentado en el año» (`taxpain-app::presentado_aeat::Row`). */
export interface AeatCheckRow {
  model: string;
  period: string;
  status: AeatCheckStatus;
  expected: boolean;
  due_on: string | null;
  presented_at: string | null;
  original_presented_at: string | null;
  late: boolean;
  late_amendment: boolean;
  receipt: string | null;
  replaces: string[];
  has_pdf: boolean;
  boxes_read: boolean;
  mismatches: number | null;
  filed_model_id: number | null;
  note: string | null;
}

/** Texto con que la AEAT dijo que no había declaraciones de esos modelos. */
export interface AeatAnswer {
  text: string;
  models: string[];
}

export interface AeatCheckReport {
  year: number;
  nif: string;
  checked_at: string | null;
  without_profile: boolean;
  rows: AeatCheckRow[];
  none_filed: string[];
  errors: string[];
  empty_answers: AeatAnswer[];
  /** Modelos que el servicio de consulta no permite consultar (hay que mirarlos en la Sede). */
  unavailable: string[];
}

/** El certificado elegido frente a la empresa abierta. */
export interface AeatCheckTarget {
  company_nif: string | null;
  company_name: string | null;
  cert_nif: string;
  cert_name: string;
  same: boolean;
  other_entity: boolean;
}

/** Resumen de un año en «Comprobar varios años». */
export interface AeatYearSummary {
  year: number;
  presented: number;
  missing: number;
  not_found: number;
  open: number;
  pdfs: number;
  /** Presentadas con sus casillas leídas. */
  boxes_read: number;
  /** Presentadas fuera de plazo. */
  late: number;
  errors: number;
  /** Modelos que no se pudieron consultar, con el mensaje («modelo: mensaje»). */
  failed: string[];
  /** Modelos que el servicio no permite consultar. */
  unavailable: string[];
  checked_at: string | null;
}

export interface AeatYearOutcome {
  year: number;
  summary: AeatYearSummary | null;
  error: string | null;
}

export interface AeatCheckProgress {
  year: number;
  step: 'consulta' | 'descarga';
  done: number;
  total: number;
}

/** De quién es el certificado y de quién la empresa (sin consultar la AEAT). */
export const aeatCheckTarget = (fingerprint: string) =>
  invoke<AeatCheckTarget>('aeat_check_target', { fingerprint });
/** Consulta la AEAT con el certificado, descarga los justificantes y devuelve la tabla. */
export const aeatCheckYear = (year: number, fingerprint: string, otherHolder = false) =>
  invoke<AeatCheckReport>('aeat_check_year', { year, fingerprint, otherHolder });
/** Comprueba varios años con una sola confirmación del certificado. */
export const aeatCheckYears = (
  from: number,
  to: number,
  fingerprint: string,
  otherHolder = false,
) => invoke<AeatYearOutcome[]>('aeat_check_years', { from, to, fingerprint, otherHolder });
/** El resumen de todos los años comprobados, sin consultar la AEAT. */
export const aeatCheckSummaries = () => invoke<AeatYearSummary[]>('aeat_check_summaries');
/** La tabla del año con lo ya guardado, sin consultar la AEAT. */
export const aeatCheckReport = (year: number) =>
  invoke<AeatCheckReport>('aeat_check_report', { year });
/** Guarda los justificantes del año en `dir`; devuelve las rutas. */
export const aeatCheckSave = (year: number, dir: string) =>
  invoke<string[]>('aeat_check_save', { year, dir });
/** Guarda los justificantes de varios años, cada uno en su subcarpeta de `dir`. */
export const aeatCheckSaveYears = (from: number, to: number, dir: string) =>
  invoke<string[]>('aeat_check_save_years', { from, to, dir });

/** Progreso de la comprobación de lo presentado (evento del proceso nativo). */
/**
 * ¿Hay una comprobación de lo presentado en marcha? (Sólo una operación con la AEAT a la vez; una
 * descarga por CSV no cuenta aquí: ver `aeatCsvRunning`.)
 */
export const aeatCheckRunning = () => invoke<boolean>('aeat_check_running');
/** Avisa cuando acaba la comprobación en marcha (bien o con error). */
export async function onAeatCheckDone(f: () => void): Promise<() => void> {
  if (!isTauri() || demoInvoke) return () => {};
  const { listen } = await import('@tauri-apps/api/event');
  return listen('aeat-check-done', () => f());
}
export async function onAeatCheckProgress(f: (p: AeatCheckProgress) => void): Promise<() => void> {
  if (!isTauri() || demoInvoke) return () => {};
  const { listen } = await import('@tauri-apps/api/event');
  return listen<AeatCheckProgress>('aeat-check-progress', (e) => f(e.payload));
}

// ---------- Descargar de la AEAT por CSV ----------

/** Qué pasó con cada CSV (espejo de `taxpain_app::descarga_csv::Item`). */
export type AeatCsvItem = { csv: string } & (
  | { state: 'guardado'; path: string }
  | { state: 'ya_estaba'; path: string }
  | { state: 'no_encontrado' }
  | { state: 'anulado'; replacement: string | null }
  | { state: 'no_disponible'; message: string }
  | { state: 'error'; message: string }
);

/**
 * Descarga de la AEAT los documentos de los CSV que hay en `text` y los guarda en la carpeta de
 * la empresa («Documentos AEAT»). Con certificado, primero el servicio web oficial; sin él, la
 * Sede pública.
 */
export const aeatCsvDownload = (text: string, fingerprint: string | null, otherHolder = false) =>
  invoke<AeatCsvItem[]>('aeat_csv_download', { text, fingerprint, otherHolder });

/** ¿Hay una descarga por CSV en marcha? (Una comprobación de lo presentado no cuenta.) */
export const aeatCsvRunning = () => invoke<boolean>('aeat_csv_running');

/** Avisa cuando acaba la descarga por CSV en marcha (bien o con error). */
export async function onAeatCsvDone(f: () => void): Promise<() => void> {
  if (!isTauri() || demoInvoke) return () => {};
  const { listen } = await import('@tauri-apps/api/event');
  return listen('aeat-csv-done', () => f());
}

/** Progreso de la descarga por CSV (evento del proceso nativo). */
export async function onAeatCsvProgress(
  f: (p: { done: number; total: number }) => void,
): Promise<() => void> {
  if (!isTauri() || demoInvoke) return () => {};
  const { listen } = await import('@tauri-apps/api/event');
  return listen<{ done: number; total: number }>('aeat-csv-progress', (e) => f(e.payload));
}

export const filedReceiptDownload = (id: number, fingerprint: string) =>
  invoke<void>('filed_receipt_download', { id, fingerprint });
export const filedReceiptSave = (id: number, dir: string) =>
  invoke<string>('filed_receipt_save', { id, dir });

// ---------- Notificaciones de la AEAT ----------

export interface Notice {
  id: string;
  /** C comunicación, N notificación */
  kind: 'C' | 'N';
  /** A accedida, P pendiente */
  state: 'A' | 'P';
  subject: string;
  available_at: string;
  environment: 'pruebas' | 'produccion';
  accessed_at: string | null;
  metadata: [string, string][];
  has_content: boolean;
  /** Notificación pendiente: día en que se dará por notificada si no se abre (`YYYY-MM-DD`). */
  deemed_notified_on: string | null;
}

export const noticesList = () => invoke<Notice[]>('notices_list');
/** Días de cortesía marcados en la Sede (`YYYY-MM-DD`). */
export const noticesCourtesyDays = () => invoke<string[]>('notices_courtesy_days');
export const noticesSetCourtesyDays = (days: string[]) =>
  invoke<string[]>('notices_set_courtesy_days', { days });
export const noticesRefresh = (fingerprint: string, days = 180) =>
  invoke<number>('notices_refresh', { fingerprint, days });
export const noticeDownload = (id: string, fingerprint: string) =>
  invoke<void>('notice_download', { id, fingerprint });
export const noticeAppear = (id: string, fingerprint: string) =>
  invoke<void>('notice_appear', { id, fingerprint, confirmed: true });
export const noticeSavePdf = (id: string, certification: boolean, dir: string) =>
  invoke<string>('notice_save_pdf', { id, certification, dir });

/** Abre una página de la Sede en el navegador (trámites que sólo existen en la web). */
export async function openSede(url: string): Promise<void> {
  const { openUrl } = await import('@tauri-apps/plugin-opener');
  await openUrl(url);
}

// Panel financiero
export interface MonthFigures {
  month: number;
  income: number;
  expenses: number;
}
export interface TaxEstimate {
  model: string;
  quarter: number;
  result: number;
  /** «4T», «3P» (pagos fraccionados del 202). */
  period?: string;
  /** Si no se puede estimar (el 202 de más de 6 millones): por qué; `result` no vale. */
  note?: string | null;
}
export interface Dashboard {
  year: number;
  months: MonthFigures[];
  income: number;
  expenses: number;
  profit: number;
  cash: number;
  receivables: number;
  payables: number;
  estimates: TaxEstimate[];
  pending: {
    received_to_review: number;
    bank_unreconciled: number;
    notices_pending: number;
    verifactu_unsent: number;
  };
  /** TaxPain no tiene asientos del año (sin contar apertura y cierre): los ceros no valen. */
  sin_contabilidad?: boolean;
  /** Sin contabilidad: lo presentado a la AEAT ese año que se ha leído. */
  presentado?: CifraPresentada[];
}
/** Una suma de lo presentado a la AEAT en el año («suma de los 303 [27]», «1T a 3T»). */
export interface CifraPresentada {
  id: string;
  titulo: string;
  valor: number;
  fuente: string;
  periodos: string;
}
export const dashboardGet = (year: number, month: number) =>
  invoke<Dashboard>('dashboard_get', { year, month });

/** Lo pendiente de cobrar y de facturar a hoy (espejo de `taxpain_app::dashboard::Receivables`). */
export interface Receivables {
  /** Facturas con algún vencimiento pasado sin cobrar (sin abonos ni lo abonado) y lo vencido. */
  overdue_invoices: number;
  overdue_amount: number;
  /** Presupuestos aceptados sin facturar, de cualquier año. */
  accepted_quotes: number;
  accepted_total: number;
}
export const dashboardReceivables = () => invoke<Receivables>('dashboard_receivables');

// Arrastrar y soltar ficheros sobre la ventana (sólo en la app de escritorio).
export type DropEvent = { type: 'over' } | { type: 'leave' } | { type: 'drop'; paths: string[] };

/** Se suscribe a los ficheros arrastrados sobre la ventana. Devuelve la función para dejar de
 * escuchar. Fuera de Tauri (vista previa, tests, demo) no hace nada. */
export async function onFileDrop(cb: (e: DropEvent) => void): Promise<() => void> {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return () => {};
  const { getCurrentWebview } = await import('@tauri-apps/api/webview');
  return getCurrentWebview().onDragDropEvent((e) => {
    const p = e.payload;
    if (p.type === 'drop') cb({ type: 'drop', paths: p.paths });
    else if (p.type === 'leave') cb({ type: 'leave' });
    else cb({ type: 'over' });
  });
}

// Asistente IA
export type AssistantProvider =
  | { kind: 'Anthropic'; model: string }
  | { kind: 'OpenAiCompatible'; base_url: string; model: string };
export interface AssistantSettings {
  provider: AssistantProvider;
  consent: boolean;
}
export interface AssistantStatus {
  settings: AssistantSettings;
  has_api_key: boolean;
  remote: boolean;
}
export interface ChatMessage {
  role: 'user' | 'assistant';
  content: string;
}
export interface AssistantOutgoing {
  system: string;
  messages: ChatMessage[];
  replaced: number;
}
export const assistantStatus = () => invoke<AssistantStatus>('assistant_status');
/**
 * Guarda los ajustes del asistente y, si llega `key`, la clave de API del proveedor nuevo a la
 * vez (Touch ID con el servidor al que irá; si se cancela, no cambia nada, ni el proveedor).
 * Lo aceptado vale para el host al que se aceptó: con un proveedor de otro host, Rust lo guarda
 * sin aceptar y devuelve `false`; entonces se vuelve a guardar, ya con ese proveedor y sin la
 * clave (ya guardada), lo que el usuario acaba de aceptar en el formulario.
 */
export async function assistantSave(
  settings: AssistantSettings,
  key: string | null = null,
): Promise<void> {
  const kept = await invoke<boolean | null>('assistant_save', { settings, key });
  if (kept === false && settings.consent)
    await invoke<boolean>('assistant_save', { settings, key: null });
}
export const assistantSetKey = (key: string | null) => invoke<void>('assistant_set_key', { key });

// Acceso de Claude por el servidor MCP (espejo de `taxpain_app::mcp_ajustes`): por empresa y
// apagado por defecto.
export interface McpAjustes {
  activado: boolean;
  nivel: 'consultar' | 'borradores';
}
export interface McpEstado {
  ajustes: McpAjustes;
  /** Ruta de `taxpain-mcp` si viene con la app; si no, hay que compilarlo aparte. */
  binario: string | null;
}
export const mcpEstado = () => invoke<McpEstado>('mcp_estado');
export const mcpGuardar = (ajustes: McpAjustes) => invoke<McpEstado>('mcp_guardar', { ajustes });

// Conversaciones con la IA (espejo de `taxpain_app::conversaciones`): generales o sobre un tema
// de TaxPain, guardadas en la empresa con lo que añade el usuario.
export type AssistantTopic =
  | { kind: 'general' }
  | { kind: 'ahorro'; id: string; year: number }
  | { kind: 'ahorro_plan'; year: number }
  /** Un hallazgo de «Qué revisar» (`aeat`) o de la última revisión de la carpeta. */
  | { kind: 'hallazgo'; origin: OrigenHallazgo; code: string; year: number | null; title: string }
  /** El borrador de un modelo: `period` es el trimestre, el periodo del 202 o 0 en los anuales. */
  | { kind: 'modelo'; model: string; year: number; period: number };
export type OrigenHallazgo = 'aeat' | 'carpeta';
/**
 * Identifica la conversación de un tema: igual que `Topic::kind` + `Topic::key` en Rust, salvo en
 * un hallazgo, que lleva su título entero en vez de su huella.
 */
export function topicKey(t: AssistantTopic): string {
  switch (t.kind) {
    case 'general':
      return 'general:';
    case 'ahorro':
      return `ahorro:${t.id}@${t.year}`;
    case 'ahorro_plan':
      return `ahorro_plan:${t.year}`;
    case 'hallazgo':
      return `hallazgo:${t.origin}/${t.code}/${t.year ?? '-'}/${t.title}`;
    case 'modelo':
      return `modelo:${t.model}/${t.year}/${t.period}`;
  }
}
/** El tema de un hallazgo: se busca por su código, su ejercicio y su título. */
export const findingTopic = (h: Hallazgo, origin: OrigenHallazgo): AssistantTopic => ({
  kind: 'hallazgo',
  origin,
  code: h.codigo,
  year: h.ejercicio,
  title: h.titulo,
});
/** «3T 2026», «2P 2026» o, en los anuales (periodo 0), «2026». */
export function draftPeriod(model: string, year: number, period: number): string {
  if (period > MES) return `${MESES[period - MES - 1] ?? period} ${year}`;
  return period ? `${period}${model === '202' ? 'P' : 'T'} ${year}` : String(year);
}
/** El tema del borrador de un modelo. */
export const draftTopic = (d: ModelDraft): AssistantTopic => ({
  kind: 'modelo',
  model: d.model,
  year: d.year,
  period: d.quarter,
});
/** «Borrador del modelo 303 · 3T 2026», como lo titula TaxPain. */
export const draftTitle = (model: string, year: number, period: number) =>
  `Borrador del modelo ${model} · ${draftPeriod(model, year, period)}`;
export interface TopicHeader {
  /** «Ahorrar impuestos · 2026», «Asistente». */
  group: string;
  title: string;
  /** Lo que sabe TaxPain del tema: [etiqueta, valor]. */
  facts: [string, string][];
  /** El primer mensaje de TaxPain (no se envía). */
  intro: string;
  /** Preguntas para empezar: rellenan el cuadro, no se envían solas. */
  suggestions: string[];
  notice: string | null;
}
export interface MessageNotes {
  model: string;
  replaced: number;
  used: string[];
  sources: string[];
  unverified: string[];
}
export interface StoredMessage extends ChatMessage {
  seq: number;
  created_at: string;
  notes: MessageNotes | null;
}
export interface ConversationMaterial {
  id: number;
  kind: 'dato' | 'documento';
  content: string;
  filename: string | null;
  read_by: 'mac' | 'ia' | null;
  truncated: boolean;
  chars: number;
  created_at: string;
}
export interface Conversation {
  id: number;
  topic: AssistantTopic;
  title: string;
  created_at: string;
  updated_at: string;
  messages: StoredMessage[];
  materials: ConversationMaterial[];
  /** Ha llegado a 100 preguntas. */
  full: boolean;
}
export interface TopicOpened {
  header: TopicHeader;
  conversation: Conversation | null;
  /** Los datos del tema han cambiado desde la última pregunta. */
  changed: boolean;
}
export interface ConversationSummary {
  id: number;
  topic: AssistantTopic;
  title: string;
  updated_at: string;
  questions: number;
  materials: number;
}
export interface TopicOutgoing extends AssistantOutgoing {
  /** Intervenciones antiguas que no van. */
  omitted: number;
}
export const assistantTopicOpen = (topic: AssistantTopic) =>
  invoke<TopicOpened>('assistant_topic_open', { topic });
export const assistantConversationOpen = (id: number) =>
  invoke<TopicOpened>('assistant_conversation_open', { id });
export const assistantConversations = () =>
  invoke<ConversationSummary[]>('assistant_conversations');
export const assistantTopicPreview = (
  topic: AssistantTopic,
  conversationId: number | null,
  question: string | null,
) => invoke<TopicOutgoing>('assistant_topic_preview', { topic, conversationId, question });
export const assistantTopicAsk = (
  topic: AssistantTopic,
  conversationId: number | null,
  question: string,
) => invoke<TopicOpened>('assistant_topic_ask', { topic, conversationId, question });
export const assistantFactAdd = (
  topic: AssistantTopic,
  conversationId: number | null,
  text: string,
) => invoke<Conversation>('assistant_fact_add', { topic, conversationId, text });
export const assistantFileAdd = (
  topic: AssistantTopic,
  conversationId: number | null,
  path: string,
) => invoke<Conversation>('assistant_file_add', { topic, conversationId, path });
export const assistantMaterialDelete = (conversationId: number, materialId: number) =>
  invoke<Conversation>('assistant_material_delete', { conversationId, materialId });
export const assistantConversationDelete = (id: number) =>
  invoke<void>('assistant_conversation_delete', { id });
export const assistantConversationsDeleteAll = () =>
  invoke<number>('assistant_conversations_delete_all');
/** Documento para una conversación (diálogo nativo). En la demo, uno de ejemplo. */
export async function pickAssistantFile(): Promise<string | null> {
  if (demoInvoke) return 'demo/certificado-aportaciones-2026.pdf';
  const { open } = await import('@tauri-apps/plugin-dialog');
  const p = await open({
    multiple: false,
    filters: [
      {
        name: 'Documentos',
        extensions: [
          'pdf',
          'jpg',
          'jpeg',
          'png',
          'heic',
          'heif',
          'webp',
          'tif',
          'tiff',
          'xlsx',
          'xlsm',
          'xls',
          'ods',
          'csv',
          'txt',
          'md',
        ],
      },
    ],
  });
  return typeof p === 'string' ? p : null;
}

// VERI*FACTU: subsanación
export interface RecordIssue {
  id: number;
  invoice_id: number;
  num_serie: string;
  record_type: 'alta' | 'anulacion';
  send_status: 'incorrecto' | 'aceptado_con_errores';
  error_code: string | null;
  error_text: string | null;
}
export const verifactuIssues = () => invoke<RecordIssue[]>('verifactu_issues');
export const verifactuResubmit = (recordId: number) =>
  invoke<number>('verifactu_resubmit', { recordId, confirmed: true });

// Actualizaciones
export interface UpdateInfo {
  version: string;
  notes: string | null;
}
export const updateCheck = () => invoke<UpdateInfo | null>('update_check');
export const updateInstall = () => invoke<void>('update_install', { confirmed: true });

// Datos fiscales de Renta: cotejo
export interface CheckLine {
  topic: 'ingresos' | 'retenciones' | 'pago_130' | 'cotizaciones';
  label: string;
  aeat: number;
  taxpain: number;
  matches: boolean;
}
export interface CrossCheck {
  year: number;
  process_date: string | null;
  lines: CheckLine[];
  notes: string[];
}
export const rentaDownload = (fingerprint: string, year: number) =>
  invoke<CrossCheck>('renta_download', { fingerprint, year });
export const rentaCrossCheck = (year: number) =>
  invoke<CrossCheck | null>('renta_cross_check', { year });

// Renta del autónomo: rendimiento de la actividad (modelo 100)
export type IrpfRegime = 'Normal' | 'Simplificada';
export type Reduction32 = 'Ninguna' | 'Dependiente' | 'RentasBajas';
export interface Box100 {
  /** Número de casilla, si está confirmado; null = búscala por su nombre en Renta Web. */
  code: string | null;
  label: string;
  amount: number;
}
export interface Result100 {
  year: number;
  regime: IrpfRegime;
  income: number;
  expenses: number;
  net_before: number;
  hard_to_justify: number;
  net: number;
  reduction_32_2: number;
  reduction_start: number;
  net_reduced: number;
  withholdings: number;
  installments: number;
  boxes: Box100[];
  warnings: string[];
}
export interface ActivityDraft {
  year: number;
  regime: IrpfRegime;
  result: Result100;
  missing_130: string[];
  check: CheckLine[];
  check_date: string | null;
  notes: string[];
  /** Ganancias y pérdidas por criptomonedas (FIFO), si hay operaciones ese año. */
  cripto?: CriptoRenta | null;
}
export const rentaActivity = (year: number) => invoke<ActivityDraft>('renta_activity', { year });

// ---------- Criptomonedas (espejo de `taxpain_app::cripto`) ----------

/** Cantidades y precios viajan como texto decimal con punto («0.00000001»), nunca como número. */
export type CriptoTipo =
  | 'compra'
  | 'venta'
  | 'permuta'
  | 'traspaso'
  | 'comision'
  | 'recompensa'
  | 'perdida'
  | 'pago'
  | 'cobro';
export type CriptoMetodo = 'pmp' | 'fifo';
export type CriptoNaturaleza = 'existencias' | 'intangible';

export const CRIPTO_TIPOS: { id: CriptoTipo; label: string }[] = [
  { id: 'compra', label: 'Compra' },
  { id: 'venta', label: 'Venta' },
  { id: 'permuta', label: 'Permuta' },
  { id: 'traspaso', label: 'Traspaso' },
  { id: 'comision', label: 'Comisión en cripto' },
  { id: 'recompensa', label: 'Recompensa' },
  { id: 'perdida', label: 'Pérdida o robo' },
  { id: 'pago', label: 'Pago en cripto' },
  { id: 'cobro', label: 'Cobro en cripto' },
];
export const criptoTipoLabel = (t: CriptoTipo): string =>
  CRIPTO_TIPOS.find((x) => x.id === t)?.label ?? t;

export interface CriptoCuentas {
  existencias: string;
  intangible: string;
  tesoreria: string;
  puente: string;
  ventas: string;
  variacion: string;
  beneficio: string;
  perdida: string;
  comisiones: string;
  recompensas: string;
  deterioro_gasto: string;
  deterioro: string;
  reversion: string;
}
export interface CriptoSettings {
  metodo: CriptoMetodo;
  naturaleza: CriptoNaturaleza;
  cuentas: CriptoCuentas;
}
export interface CriptoAsset {
  id: number;
  ticker: string;
  name: string;
  account: string;
}
export interface CriptoPlatform {
  id: number;
  name: string;
  kind: 'exchange' | 'monedero';
  country: string;
  abroad: boolean;
  account: string;
}
export interface CriptoOp {
  id: number;
  date: string;
  time: string;
  kind: CriptoTipo;
  platform_id: number;
  to_platform_id: number | null;
  ticker: string;
  qty: string;
  ticker2: string | null;
  qty2: string | null;
  eur: number;
  fee_eur: number;
  ref_price: string | null;
  note: string;
  source: string;
  fingerprint: string | null;
  entry_id: number | null;
}
export interface CriptoPrice {
  date: string;
  ticker: string;
  price: string;
  source: string;
}
export interface CriptoResumen {
  metodo: CriptoMetodo;
  year: number;
  ganancias: number;
  perdidas: number;
  neto: number;
  recompensas: number;
  comisiones: number;
  transmisiones: number;
}
export interface CriptoPosicion {
  ticker: string;
  platform_id: number;
  platform: string;
  abroad: boolean;
  cantidad: string;
  coste: number;
  precio: string | null;
  precio_fecha: string | null;
  valor: number | null;
  latente: number | null;
}
export interface CriptoRealizado {
  op: number;
  fecha: string;
  tipo: CriptoTipo;
  ticker: string;
  platform: string;
  cantidad: string;
  valor: number;
  coste: number;
  resultado: number;
}
export interface Cripto721 {
  year: number;
  fecha: string;
  total_extranjero: number;
  umbral: number;
  supera: boolean;
  plataformas: {
    platform: string;
    country: string;
    valor: number;
    activos: [string, string, number][];
  }[];
  sin_precio: string[];
  /** Monedas del punto de partida sin plataforma: no cuentan en el total hasta que se les ponga. */
  sin_plataforma: string[];
  en_perfil: boolean;
  exenta_por_contabilidad: boolean;
  nota: string;
}
export interface CriptoReport {
  year: number;
  fecha: string;
  settings: CriptoSettings;
  persona_fisica: boolean;
  metodo_declaracion: CriptoMetodo;
  posiciones: CriptoPosicion[];
  valor_total: number;
  coste_total: number;
  resumen_pmp: CriptoResumen;
  resumen_fifo: CriptoResumen;
  realizados: CriptoRealizado[];
  m721: Cripto721;
  ops_ejercicio: number;
  ops_sin_asiento: number;
  ops_con_asiento: number;
  deterioro_registrado: number | null;
  avisos: string[];
  /** Punto de partida (lo de la gestoría a la fecha de corte), si lo hay. */
  inicio: CriptoInicio | null;
  /** El ejercicio acaba en la fecha de corte o antes: lo calculó la gestoría. */
  antes_del_inicio: boolean;
  /** Operaciones del ejercicio hasta la fecha de corte (historial). */
  ops_historial: number;
  /** Sin punto de partida: ventas o traspasos de más de lo que había. */
  huecos: number;
}
/** Una moneda del punto de partida (espejo de `taxpain_app::cripto::inicio`). */
export interface CriptoPosicionInicio {
  ticker: string;
  /** null: sin plataforma (sirve para cualquiera). */
  platform_id: number | null;
  /** Texto decimal con punto. */
  cantidad: string;
  /** Coste pendiente en céntimos. */
  coste: number;
}
export interface CriptoInicio {
  /** Fecha de corte (último día que no cuenta). */
  fecha: string;
  /** Cuenta del diario donde la gestoría tiene el coste (vacía: la subcuenta de cada moneda). */
  cuenta: string;
  origen: string;
  nota: string;
  posiciones: CriptoPosicionInicio[];
}
export interface CriptoCuentaDiario {
  cuenta: string;
  nombre: string;
  saldo: number;
}
export interface CriptoPropuestaInicio {
  fecha: string;
  ejercicio: number;
  motivo: string;
  posiciones: CriptoPosicionInicio[];
  cuenta: string;
  cuentas: CriptoCuentaDiario[];
  coste_diario: number | null;
  casilla_102: number | null;
  hoja: string | null;
  fecha_cantidades: string | null;
  cantidades_de_operaciones: boolean;
  reparto_seguro: boolean;
  por_repartir: number;
  /** Monedas sin coste que valen menos de 1 € (restos): no se proponen, pero se pueden añadir. */
  polvo: CriptoPosicionInicio[];
  origen: string;
  notas: string[];
}
export interface CriptoRentaLinea {
  fecha: string;
  ticker: string;
  clave: string;
  cantidad: string;
  valor_transmision: number;
  valor_adquisicion: number;
  resultado: number;
}
export interface CriptoRenta {
  year: number;
  ganancias: number;
  perdidas: number;
  neto: number;
  recompensas: number;
  lineas: CriptoRentaLinea[];
  notas: string[];
}
export interface CriptoOpDraft extends CriptoOp {
  platform: string;
  to_platform: string | null;
}
export interface CriptoLectura {
  kind: 'cointracking' | 'kraken' | 'hoja' | 'saldos';
  ops: CriptoOpDraft[];
  prices: CriptoPrice[];
  duplicates: number;
  new_platforms: string[];
  warnings: string[];
}
export interface CriptoContabilizado {
  asientos: number;
  borrados: number;
  saltadas: number;
  avisos: string[];
}

export const criptoSettings = () => invoke<CriptoSettings>('cripto_settings');
export const criptoSaveSettings = (settings: CriptoSettings) =>
  invoke<void>('cripto_save_settings', { settings });
export const criptoAssets = () => invoke<CriptoAsset[]>('cripto_assets');
export const criptoSaveAsset = (ticker: string, name: string) =>
  invoke<number>('cripto_save_asset', { ticker, name });
export const criptoPlatforms = () => invoke<CriptoPlatform[]>('cripto_platforms');
export const criptoSavePlatform = (platform: CriptoPlatform) =>
  invoke<number>('cripto_save_platform', { platform });
export const criptoDeletePlatform = (id: number) => invoke<void>('cripto_delete_platform', { id });
export const criptoOps = (year: number | null) => invoke<CriptoOp[]>('cripto_ops', { year });
export const criptoSaveOp = (op: CriptoOp) => invoke<number>('cripto_save_op', { op });
export const criptoDeleteOp = (id: number) => invoke<void>('cripto_delete_op', { id });
export const criptoPrices = () => invoke<CriptoPrice[]>('cripto_prices');
export const criptoSetPrices = (prices: CriptoPrice[]) =>
  invoke<number>('cripto_set_prices', { prices });
export const criptoDeletePrice = (date: string, ticker: string) =>
  invoke<void>('cripto_delete_price', { date, ticker });
export const criptoReport = (year: number) => invoke<CriptoReport>('cripto_report', { year });
export const criptoReadFile = (path: string) => invoke<CriptoLectura>('cripto_read_file', { path });
export const criptoImport = (lectura: CriptoLectura) =>
  invoke<[number, number, number]>('cripto_import', { lectura });
export const criptoPostEntries = (year: number, redo: boolean) =>
  invoke<CriptoContabilizado>('cripto_post_entries', { year, redo });
export const criptoImpairment = (year: number) =>
  invoke<number | null>('cripto_impairment', { year });
export const criptoExport = (year: number, dir: string, pdf: boolean) =>
  invoke<string>('cripto_export', { year, dir, pdf });
export const criptoInicio = () => invoke<CriptoInicio | null>('cripto_inicio');
/** Devuelve avisos si el cambio afecta a asientos ya hechos (hay que rehacerlos). */
export const criptoSaveInicio = (inicio: CriptoInicio) =>
  invoke<string[]>('cripto_save_inicio', { inicio });
export const criptoDeleteInicio = () => invoke<string[]>('cripto_delete_inicio');
/**
 * Propuesta con los cálculos de la gestoría (diario, hoja de saldos y 200); no guarda nada.
 * `cuenta`: la del diario donde está el coste, si se sabe (si no, se buscan por el nombre).
 */
export const criptoPropuestaInicio = (cuenta?: string) =>
  invoke<CriptoPropuestaInicio>('cripto_propuesta_inicio', { cuenta: cuenta?.trim() || null });

/** Elige la Trade List de CoinTracking, el ledgers de Kraken, una hoja de operaciones o de saldos. */
export async function pickCriptoFile(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const p = await open({
    multiple: false,
    filters: [
      {
        name: 'Operaciones o saldos (CSV, Excel, OpenDocument)',
        extensions: ['csv', 'txt', 'xlsx', 'xls', 'ods'],
      },
    ],
  });
  return typeof p === 'string' ? p : null;
}

/**
 * Cantidad de cripto escrita a mano → texto decimal con punto para el proceso nativo, o null si
 * no es válida. Acepta coma o punto decimal, miles con punto si hay coma, hasta 18 decimales.
 */
export function parseCriptoQty(s: string): string | null {
  const t = s.trim().replace(/\s/g, '');
  // El punto es de miles sólo si detrás va la coma de los decimales («1.234,5»); si no, es el
  // decimal («0.001», «1.234» son 0,001 y 1,234: en cripto lo normal son decimales).
  const m = /^(-?)(?:([1-9]\d{0,2}(?:\.\d{3})+),(\d{1,18})|(\d+)(?:[.,](\d{1,18}))?)$/.exec(t);
  if (!m) return null;
  const int = (m[2] ?? m[4] ?? '0').replace(/\./g, '');
  const dec = (m[3] ?? m[5] ?? '').replace(/0+$/, '');
  if (!/[1-9]/.test(int + dec)) return null;
  return `${m[1]}${int.replace(/^0+(?=\d)/, '')}${dec ? `.${dec}` : ''}`;
}

/** Texto decimal con punto («1234.5») en formato español («1.234,5»). */
export function criptoQty(s: string | null | undefined): string {
  if (!s) return '';
  const neg = s.startsWith('-');
  const [int, dec] = s.replace('-', '').split('.');
  const i = (int ?? '0').replace(/\B(?=(\d{3})+(?!\d))/g, '.');
  return `${neg ? '-' : ''}${i}${dec ? `,${dec}` : ''}`;
}

/** Propuesta de datos de una recibida con el asistente (anonimizado; sólo rellena el formulario). */
export const receivedAiExtract = (id: number) => invoke<Extraction>('received_ai_extract', { id });

// ---------- Empresas ----------
// Espejo de `taxpain_app::companies`.

export interface CompanyInfo {
  name: string;
  nif: string | null;
}

export interface Company extends CompanyInfo {
  id: string;
  active: boolean;
  /** No se puede abrir: por qué (se ofrece la pantalla de recuperación). */
  no_abre?: string;
}

export const companiesList = () => invoke<Company[]>('companies_list');
/** Alta de una empresa con la contraseña de sus copias (y su código de recuperación). */
export const companyCreate = (info: CompanyInfo, passphrase: string | null = null) =>
  invoke<Company>('company_create', { info, passphrase });
export const companySwitch = (id: string) => invoke<void>('company_switch', { id });
export const companyUpdate = (info: CompanyInfo) => invoke<CompanyInfo>('company_update', { info });
/** Nombre y NIF de cualquier empresa (la abierta o no). */
export const companyUpdateId = (id: string, info: CompanyInfo) =>
  invoke<CompanyInfo>('company_update_id', { id, info });
/**
 * Borra una empresa (su carpeta va a la Papelera del Mac). `confirm`: su nombre o su NIF.
 * Devuelve `true` si era la abierta y se ha abierto otra (hay que recargar la interfaz).
 */
export const companyDelete = (id: string, confirm: string) =>
  invoke<boolean>('company_delete', { id, confirm });

// ---------- Pago de los modelos ----------

export interface TaxPayment {
  id: number;
  model: string;
  year: number;
  period: string;
  date: string;
  /** Cuenta de Hacienda (4750, 4751, 473, 4752). */
  account: string;
  treasury: string;
  amount: number;
  entry_id: number | null;
  bank_tx: number | null;
}

export interface TaxPaymentView {
  payable: boolean;
  payment: TaxPayment | null;
  /** Cargos del banco sin conciliar por el importe exacto. */
  candidates: BankTx[];
}

export const taxPayment = (model: string, year: number, quarter: number) =>
  invoke<TaxPaymentView>('tax_payment', { model, year, quarter });
export const taxPay = (
  model: string,
  year: number,
  quarter: number,
  date: string,
  treasury: string,
) => invoke<TaxPayment>('tax_pay', { model, year, quarter, date, treasury });
export const taxPayBank = (model: string, year: number, quarter: number, txId: number) =>
  invoke<TaxPayment>('tax_pay_bank', { model, year, quarter, txId });
export const taxUnpay = (model: string, year: number, quarter: number) =>
  invoke<void>('tax_unpay', { model, year, quarter });

// ---------- Reglas del banco ----------

export interface BankRule {
  id: number;
  /** Texto del concepto normalizado (minúsculas, sin cifras). */
  pattern: string;
  sign: 'cargo' | 'abono';
  account: string;
  hits: number;
}

export interface BankProposal {
  tx: BankTx;
  account: string;
  pattern: string;
}

export const bankRules = () => invoke<BankRule[]>('bank_rules');
export const bankRuleDelete = (id: number) => invoke<void>('bank_rule_delete', { id });
export const bankRuleFor = (tx: number) => invoke<BankRule | null>('bank_rule_for', { tx });
export const bankRuleProposals = () => invoke<BankProposal[]>('bank_rule_proposals');
export const bankRuleApply = (ids: number[]) => invoke<number>('bank_rule_apply', { ids });

// ---------- Nóminas ----------

export interface Employee {
  id: number;
  nif: string;
  /** Apellidos y nombre. */
  name: string;
  /** Provincia del domicilio (código INE de 2 cifras). */
  province: string;
  birth_year: number;
  /** Situación familiar 1, 2 o 3 (art. 88 RIRPF). */
  family: number;
  spouse_nif: string | null;
  disability: number;
  contract: number;
  mobility: boolean;
  children_under3: number;
  children_rest: number;
  children_whole: boolean;
  active: boolean;
  /** Consejero o administrador (clave E del 190). */
  administrator?: boolean;
  /** Régimen de Seguridad Social del administrador. */
  ss_regime?: DirectorSs;
}

export interface PayslipInput {
  employee_id: number;
  date: string;
  gross: number;
  irpf: number;
  ss_employee: number;
  ss_company: number;
  /** Administradores: cifra de negocios del último ejercicio cerrado < 100.000 € (null: la calcula TaxPain). */
  small_entity?: boolean | null;
}

export interface StoredPayslip extends PayslipInput {
  id: number;
  employee: string;
  nif: string;
  net: number;
  entry_id: number | null;
  /** Subclave E del 190 si es la retribución de un administrador. */
  director_subkey?: number | null;
}

export const employees = () => invoke<Employee[]>('employees');
export const employeeSave = (employee: Employee) => invoke<number>('employee_save', { employee });
export const payslips = (year: number) => invoke<StoredPayslip[]>('payslips', { year });
export const payslipRegister = (payslip: PayslipInput) =>
  invoke<number>('payslip_register', { payslip });
export const payslipDelete = (id: number) =>
  invoke<void>('payslip_delete', { id, confirmed: true });

// ---------- Nóminas calculadas (trabajadores por cuenta ajena) ----------

export type Contrato = 'Indefinido' | 'Temporal';
/** Salarial (cotiza y tributa), no salarial sujeta o no salarial exenta (dietas dentro del límite). */
export type ClaseConcepto = 'Salarial' | 'NoSalarialSujeta' | 'NoSalarialExenta';
export type ClaveCuota =
  | 'Comunes'
  | 'Mei'
  | 'Desempleo'
  | 'Formacion'
  | 'Fogasa'
  | 'AtIt'
  | 'AtIms'
  | 'HorasExtra'
  | 'Solidaridad';

export interface ConceptoNomina {
  nombre: string;
  importe: number;
  clase: ClaseConcepto;
}

export interface PagasExtra {
  numero: number;
  /** Importe de cada paga completa. */
  importe: number;
  prorrateadas: boolean;
  /** Pagas enteras que se cobran en este recibo (si no están prorrateadas). */
  en_este_mes: number;
}

/** Datos de cotización y salario del trabajador (tipos en centésimas de punto). */
export interface FichaNomina {
  employee_id: number;
  naf: string;
  categoria: string;
  ccc: string;
  antiguedad: string | null;
  cnae: string;
  grupo: number;
  contrato: Contrato;
  jornada: number;
  salario_base: number;
  complementos: ConceptoNomina[];
  pagas: PagasExtra;
  at_it: number;
  at_ims: number;
  tipo_irpf: number;
}

export interface DatosNomina {
  year: number;
  month: number;
  grupo: number;
  contrato: Contrato;
  dias: number;
  jornada: number;
  salario_base: number;
  complementos: ConceptoNomina[];
  horas_extra: number;
  horas_extra_fuerza_mayor: boolean;
  pagas: PagasExtra;
  at_it: number;
  at_ims: number;
  retencion: { modo: 'Manual'; tipo: number };
  anticipos: number;
  otras_deducciones: number;
}

export interface CuotaNomina {
  clave: ClaveCuota;
  concepto: string;
  base: number;
  tipo_empresa: number;
  empresa: number;
  tipo_trabajador: number;
  trabajador: number;
}

export interface DevengoNomina {
  concepto: string;
  importe: number;
  clase: ClaseConcepto;
  extraordinaria: boolean;
  horas_extra: boolean;
}

export interface NominaCalculada {
  year: number;
  month: number;
  grupo: number;
  devengos: DevengoNomina[];
  total_devengado: number;
  remuneracion_mensual: number;
  prorrata_pagas: number;
  base_minima: number;
  base_maxima: number;
  base_comunes: number;
  base_profesionales: number;
  base_horas_extra: number;
  tope: 'Minimo' | 'Maximo' | null;
  cuotas: CuotaNomina[];
  total_trabajador: number;
  total_empresa: number;
  base_irpf: number;
  tipo_irpf: number;
  retencion_irpf: number;
  anticipos: number;
  otras_deducciones: number;
  total_deducciones: number;
  liquido: number;
  coste_empresa: number;
  norma: string;
  avisos: string[];
}

export interface TablaCotizacion {
  year: number;
  base_maxima: number;
  bases_minimas: { periodo: 'mensual' | 'diaria'; importe: number }[];
  norma: string;
  pendiente: string[];
}

export interface ConceptoSeguros {
  clave: ClaveCuota;
  concepto: string;
  empresa: number;
  trabajador: number;
  total: number;
}

export interface TrabajadorSeguros {
  payslip_id: number;
  employee: string;
  nif: string;
  base_comunes: number;
  base_profesionales: number;
  trabajador: number;
  empresa: number;
  sin_desglose: boolean;
}

export interface LineaAsientoSeguros {
  cuenta: string;
  nombre: string;
  debe: number;
  haber: number;
}

export interface ResumenSeguros {
  year: number;
  month: number;
  conceptos: ConceptoSeguros[];
  trabajadores: TrabajadorSeguros[];
  total_trabajador: number;
  total_empresa: number;
  total: number;
  vence_nominal: string;
  vence: string;
  devengo: LineaAsientoSeguros[];
  pago: { fecha: string; importe: number; cuenta: string; entry_id: number | null } | null;
  avisos: string[];
}

export interface PagoSegurosInput {
  year: number;
  month: number;
  fecha: string;
  importe: number;
  cuenta: string;
}

export interface PagaFiniquito {
  nombre: string;
  importe: number;
  devengo_desde: string;
  devengo_hasta: string;
}

export interface DatosFiniquito {
  alta: string;
  baja: string;
  salario_mensual: number;
  dias_pendientes: number;
  vacaciones_anuales: number;
  vacaciones_disfrutadas: number;
  pagas: PagaFiniquito[];
  fin_temporal: boolean;
  dias_por_ano: number;
}

export interface Finiquito {
  lineas: { concepto: string; importe: number; cotiza: boolean; detalle: string }[];
  sujeto: number;
  indemnizacion: number;
  total: number;
  vacaciones_pendientes_centesimas: number;
  avisos: string[];
}

/** Programa de la AEAT para calcular el tipo de retención del trabajo. */
export const PROGRAMA_RETENCIONES =
  'https://sede.agenciatributaria.gob.es/Sede/irpf/retenciones-ingresos-cuenta/calculo-retenciones.html';

export const nominaFicha = (employeeId: number) =>
  invoke<FichaNomina | null>('nomina_ficha', { employeeId });
export const nominaFichaSave = (ficha: FichaNomina) => invoke<void>('nomina_ficha_save', { ficha });
export const nominaTabla = (year: number) =>
  invoke<TablaCotizacion | null>('nomina_tabla', { year });
export const nominaCalcular = (datos: DatosNomina) =>
  invoke<NominaCalculada>('nomina_calcular', { datos });
export const nominaRegistrar = (registro: {
  employee_id: number;
  fecha: string;
  datos: DatosNomina;
}) => invoke<number>('nomina_registrar', { registro });
export const nominasCalculadas = (year: number) => invoke<number[]>('nominas_calculadas', { year });
export const nominaReciboPdf = (payslipId: number, dir: string) =>
  invoke<string>('nomina_recibo_pdf', { payslipId, dir });
export const segurosSociales = (year: number, month: number) =>
  invoke<ResumenSeguros>('seguros_sociales', { year, month });
export const segurosSocialesPagar = (pago: PagoSegurosInput) =>
  invoke<number>('seguros_sociales_pagar', { pago, confirmed: true });
export const finiquitoCalcular = (datos: DatosFiniquito) =>
  invoke<Finiquito>('finiquito_calcular', { datos });

// ---------- Autónomo societario (RETA del administrador) ----------

/** Quién paga la cuota del RETA. */
export type PagadorReta = 'Sociedad' | 'Administrador';

/** Ficha anual del RETA de un administrador o socio con control. Importes en céntimos. */
export interface FichaReta {
  employee_id: number;
  year: number;
  base_elegida: number;
  /** Rendimiento neto mensual previsto comunicado a la TGSS. */
  rendimiento_previsto: number | null;
  meses_alta: number;
  /** Fecha de alta en el RETA como societario (null: de años anteriores). */
  alta_societario?: string | null;
  control_efectivo: boolean;
  /** Parte de los dividendos que computa, en centésimas de punto (10000 = 100 %). */
  pct_dividendos: number;
  otros_rendimientos: number;
  gastos_deducibles: number;
  pagador: PagadorReta;
  cuenta_gasto: string;
  cuenta_tesoreria: string;
  estatutos_retribuido: boolean;
  acuerdo_fecha: string | null;
  acuerdo_importe: number | null;
  acuerdo_nota: string;
  guardada: boolean;
}

export interface TramoReta {
  tabla: 'Reducida' | 'General';
  numero: number;
  desde: number;
  hasta: number | null;
  base_min: number;
  base_max: number;
}

export interface TablaReta {
  year: number;
  tramos: TramoReta[];
  base_min_societarios: number;
  /** Tipo total en centésimas de punto (3140 = 31,40 %). */
  tipo: number;
  norma: string;
  /** Lo que falta cotejar con el BOE. */
  pendiente: string[];
}

export interface RendimientoReta {
  dividendos_computados: number;
  neto: number;
  deduccion_pct: number;
  deduccion: number;
  computable: number;
  mensual: number;
}

export interface RegularizacionReta {
  tramo: TramoReta;
  tramo_nombre: string;
  base_min: number;
  base_max: number;
  base_elegida: number;
  base_correcta: number;
  cuota_pagada: number;
  cuota_correcta: number;
  importe: number;
  sentido: 'Ninguna' | 'AIngresar' | 'ADevolver';
  meses: number;
}

export interface CuotaReta {
  id: number;
  employee_id: number;
  date: string;
  amount: number;
  pagador: PagadorReta;
  entry_id: number | null;
}

export interface VentanaCambioReta {
  desde: string;
  hasta: string;
  efectos: string;
}

export interface ResumenReta {
  employee_id: number;
  name: string;
  nif: string;
  ficha: FichaReta;
  tabla: TablaReta | null;
  dinerarias: number;
  especie: number;
  dividendos: number;
  rendimiento: RendimientoReta;
  /** Días de alta como societario en el año. */
  dias_alta_societario: number;
  /** Noventa días o más: deducción del 3 % y base mínima de los societarios. */
  noventa_dias: boolean;
  tramo_previsto: TramoReta | null;
  regularizacion: RegularizacionReta | null;
  base_recomendada: number | null;
  cuota_mensual: number;
  cuotas: CuotaReta[];
  cuotas_total: number;
  tipo_retencion: number | null;
  ingreso_a_cuenta: number;
  retribucion_total: number;
  acuerdo: {
    estatutos_retribuido: boolean;
    fecha: string | null;
    importe_maximo: number | null;
    fuente: string;
  };
  ventana_abierta: VentanaCambioReta;
  proximas_ventanas: VentanaCambioReta[];
  avisos: string[];
}

export interface CuotaRetaInput {
  employee_id: number;
  date: string;
  amount: number;
  pagador: PagadorReta;
  asiento: boolean;
}

/** «General 7», «Reducida 2». */
export function tramoNombre(t: TramoReta): string {
  return `${t.tabla} ${t.numero}`;
}

export const retaResumen = (employeeId: number, year: number) =>
  invoke<ResumenReta>('reta_resumen', { employeeId, year });
export const retaFichaSave = (ficha: FichaReta) => invoke<void>('reta_ficha_save', { ficha });
export const retaCuotaRegister = (cuota: CuotaRetaInput) =>
  invoke<number>('reta_cuota_register', { cuota });
export const retaCuotaDelete = (id: number) =>
  invoke<void>('reta_cuota_delete', { id, confirmed: true });
/** Recibo de retribución del administrador (PDF) guardado en `dir`; devuelve la ruta. */
export const adminReceiptPdf = (payslipId: number, placeDate: string, dir: string) =>
  invoke<string>('admin_receipt_pdf', { payslipId, placeDate, dir });

/** Diario, mayor, sumas y saldos, balance, PyG y cartera del ejercicio en un XLSX. */
export const exportListings = (year: number, dir: string) =>
  invoke<string>('export_listings', { year, dir });

// ---------- Cuentas de la contabilización automática ----------

export interface PostingAccounts {
  revenue: string;
  expense: string;
  customers: string;
  suppliers: string;
  vat_output: string;
  vat_input: string;
  withholding_receivable: string;
  withholding_payable: string;
  bank: string;
  /** Dígitos de las subcuentas de clientes y proveedores (7 → 4300001). */
  subaccount_width: number;
}

export const postingAccounts = () => invoke<PostingAccounts>('posting_accounts_get');
export const savePostingAccounts = (accounts: PostingAccounts) =>
  invoke<void>('posting_accounts_save', { accounts });
export const setPartyCounterpart = (account: string, counterpart: string | null) =>
  invoke<void>('party_counterpart_set', { account, counterpart });

// ---------- Observaciones y adjuntos de los asientos ----------

export interface EntryDocument {
  id: number;
  filename: string;
  size: number;
}

export const entryNotesSet = (entryId: number, notes: string) =>
  invoke<void>('entry_notes_set', { entryId, notes });
export const entryDocuments = (entryId: number) =>
  invoke<EntryDocument[]>('entry_documents', { entryId });
export const entryAttach = (entryId: number, path: string) =>
  invoke<number>('entry_attach', { entryId, path });
export const entryDetach = (entryId: number, documentId: number) =>
  invoke<void>('entry_detach', { entryId, documentId });
export const entryDocumentOpen = (documentId: number) =>
  invoke<void>('entry_document_open', { documentId });

export async function pickAnyFile(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const p = await open({ multiple: false });
  return typeof p === 'string' ? p : null;
}

export interface Disposal {
  /** Amortización del ejercicio hasta la baja (la creada o la ya registrada, rehecha). */
  depreciation_entry: number | null;
  /** Lo quitado de la amortización ya registrada del ejercicio (la de después de la baja). */
  depreciation_removed: number;
  disposal_entry: number;
  /** Asiento que pasa la factura de venta de ventas a la 543. */
  sale_entry: number | null;
  /** Beneficio (positivo) o pérdida (negativo). */
  result: number;
  warnings: string[];
}

/**
 * Baja o venta (precio en céntimos, sin IVA; 0 = baja sin venta) de un elemento del
 * inmovilizado; con `invoiceId`, vendido con esa factura emitida (su base deja de contar como
 * ventas: CONTABILIDAD-08).
 */
export const assetDispose = (
  id: number,
  date: string,
  price: number,
  invoiceId: number | null = null,
) => invoke<Disposal>('asset_dispose', { id, date, price, invoiceId });

/** Datos de un elemento que se pueden cambiar (espejo de `inmovilizado::CambiosBien`). */
export interface CambiosBien {
  name: string;
  /** Puesta en funcionamiento. */
  acquired_on: string;
  /** Coeficiente anual en puntos básicos. */
  rate: number;
  residual: number;
}

export interface BajaDeshecha {
  disposal_entry: number;
  restored: number[];
  removed: number | null;
  warnings: string[];
}

export interface LineaAmortizacion {
  asset_id: number;
  name: string;
  amount: number;
  entry_id: number | null;
}

export interface PrevisionAmortizacion {
  year: number;
  pendientes: LineaAmortizacion[];
  total: number;
  registradas: LineaAmortizacion[];
  total_registrado: number;
  cerrado: boolean;
}

export interface AmortizacionDeshecha {
  borrados: number;
  total: number;
  saltados: string[];
}

/** Incentivo fiscal de la amortización de un elemento (espejo de `inmovilizado::Incentivo`). */
export type Incentivo =
  | { tipo: 'escaso_valor' }
  | { tipo: 'erd_empleo'; incremento_plantilla: number }
  | { tipo: 'erd_acelerada'; coef_tablas: number }
  | { tipo: 'otro' };

export interface IncentivoBien {
  asset_id: number;
  incentivo: Incentivo;
}

export interface UsoIncentivos {
  year: number;
  escaso_valor: number;
  escaso_valor_tope: number;
  erd_empleo: number;
  erd_empleo_tope: number | null;
}

export interface DepreciationDifference {
  asset_id: number;
  name: string;
  accounting: number;
  tax: number;
  /** Contable − fiscal: + aumento, − disminución en el 200. */
  difference: number;
  /** Baja en el ejercicio: se revierte la diferencia acumulada. */
  reversal: boolean;
}

export const assetEdit = (id: number, cambios: CambiosBien) =>
  invoke<string[]>('asset_edit', { id, cambios });
export const assetDelete = (id: number) => invoke<void>('asset_delete', { id, confirmed: true });
export const assetUndispose = (id: number) =>
  invoke<BajaDeshecha>('asset_undispose', { id, confirmed: true });
export const depreciationPreview = (year: number) =>
  invoke<PrevisionAmortizacion>('depreciation_preview', { year });
export const depreciationUndo = (year: number, asset: number | null = null) =>
  invoke<AmortizacionDeshecha>('depreciation_undo', { year, asset, confirmed: true });
export const assetIncentives = () => invoke<IncentivoBien[]>('asset_incentives');
export const assetIncentiveUsage = (year: number) =>
  invoke<UsoIncentivos>('asset_incentive_usage', { year });
export const assetSetIncentive = (
  id: number,
  incentivo: Incentivo | null,
  taxRateBp: number | null,
) => invoke<string[]>('asset_set_incentive', { id, incentivo, taxRateBp });
export const depreciationDifferences = (year: number) =>
  invoke<DepreciationDifference[]>('depreciation_differences', { year });

/** Factura emitida que puede ser la de la venta de un elemento (o una rectificativa de ésta). */
export interface SaleCandidate {
  invoice_id: number;
  number: string;
  issue_date: string;
  operation_date: string | null;
  recipient_name: string | null;
  /** Lo que lleva a ventas en su asiento (la base sin anticipos). */
  base: number;
  /** Lo que aún no está vinculado a ninguna venta. */
  open: number;
  /** Importe propuesto para este elemento; null si hay que decirlo. */
  suggested: number | null;
  /** Factura de venta que rectifica, si es una rectificativa. */
  rectifies: string | null;
  /**
   * La rectificativa deja la venta a cero o menos (la anula). Sólo si la factura vende este
   * elemento y entero; si vende más, false (la parte la dice el usuario).
   */
  cancels_sale: boolean;
}

/** Factura vinculada a la venta de un elemento. */
export interface SaleInvoice {
  invoice_id: number;
  number: string;
  issue_date: string;
  amount: number;
  rectifies: string | null;
  entry_id: number;
}

/** Baja de un elemento con su venta. */
export interface AssetSale {
  asset_id: number;
  date: string;
  /** Precio de la baja (sin IVA; 0 si se tiró). */
  price: number;
  /** Lo facturado: la factura de venta y sus rectificativas vinculadas. */
  invoiced: number;
  /**
   * Sin factura de venta vinculada, lo que la venta tiene aún en la 543 según su saldo (menos que
   * el precio si un cobro o un asiento ya la bajó). Con ella, 0.
   */
  unbilled: number;
  /** Beneficio (positivo) o pérdida (negativo), con los ajustes de sus facturas. */
  result: number;
  invoices: SaleInvoice[];
  /** Rectificativas de la factura de venta sin pasar a la venta. */
  pending: SaleCandidate[];
}

export interface SaleLink {
  entry_id: number;
  amount: number;
  /** Cambio del resultado de la venta (positivo: más beneficio). */
  result_change: number;
  warnings: string[];
}

export const assetSales = () => invoke<AssetSale[]>('asset_sales');
/**
 * Facturas que se pueden vincular a la venta del elemento `id` (las más cercanas a la baja, sin
 * las simplificadas); con `query`, las que llevan ese número, cliente o NIF.
 */
export const assetSaleCandidates = (id: number, query = '') =>
  invoke<SaleCandidate[]>('asset_sale_candidates', { id, query });
/** Vincula una factura a la venta de un elemento (`amount` en céntimos; null, el propuesto). */
export const assetSaleLink = (id: number, invoiceId: number, amount: number | null) =>
  invoke<SaleLink>('asset_sale_link', { id, invoiceId, amount });
export const assetSaleUnlink = (id: number, invoiceId: number) =>
  invoke<void>('asset_sale_unlink', { id, invoiceId, confirmed: true });

// ---------- Cuadre de la cartera y periodo medio de pago ----------

export interface Mismatch {
  account: string;
  name: string;
  ledger: number;
  pending: number;
}

export interface PaymentPeriod {
  /** Días × 100. */
  average: number;
  paid_ratio: number;
  pending_ratio: number;
  paid_total: number;
  pending_total: number;
}

export const carteraMismatches = (today: string) =>
  invoke<Mismatch[]>('cartera_mismatches', { today });
export const paymentPeriod = (year: number) => invoke<PaymentPeriod>('payment_period', { year });

// ---------- 202: modalidad de la cuota o de la base ----------

/** Espejo de `taxpain_models::m202::Modalidad`: art. 40.2 (cuota) o 40.3 LIS (base). */
export type Modalidad202 = 'Cuota' | 'Base';

/** Espejo de `taxpain_app::models::pago202::Modalidad202`. */
export interface Modalidad202Info {
  ejercicio: number;
  elegida: Modalidad202 | null;
  efectiva: Modalidad202;
  /** Más de 6 M€ de cifra de negocios: la base es obligatoria. */
  obligatoria: boolean;
  cifra_negocios: number | null;
  aviso: string | null;
}

export interface PeriodoSimulado202 {
  period: number;
  hasta: string;
  /** Por la cuota (null: falta la cuota del IS que sirve de base). */
  cuota: number | null;
  base_imponible: number;
  base: number;
  fuente: 'contabilidad' | 'presupuesto' | 'ano_anterior' | 'sin_datos';
}

/** Espejo de `taxpain_app::models::pago202::Simulacion202`. */
export interface Simulacion202 {
  ejercicio: number;
  modalidad: Modalidad202Info;
  periodos: PeriodoSimulado202[];
  total_cuota: number | null;
  total_base: number;
  opcion_desde: string;
  opcion_hasta: string;
  conclusion: string;
  notas: string[];
}

/** Lo que se pagaría en el año por cada modalidad del 202 y el plazo de la opción. */
export const model202Simulation = (year: number) =>
  invoke<Simulacion202>('model_202_simulation', { year });

/** Guarda la modalidad del 202 del ejercicio (`null`: sin elegir, vale la de la cuota). */
export const model202SetModality = (year: number, modality: Modalidad202 | null) =>
  invoke<void>('model_202_set_modality', { year, modality });

// ---------- Casillas fijadas a mano ----------

export const modelAdjustments = (model: string, year: number, quarter: number) =>
  invoke<[number, number][]>('model_adjustments', { model, year, quarter });
/** `amount` en céntimos; `null` vuelve al cálculo automático. */
export const modelAdjust = (
  model: string,
  year: number,
  quarter: number,
  boxNumber: number,
  amount: number | null,
) => invoke<void>('model_adjust', { model, year, quarter, boxNumber, amount });

/** Cartas del 347 a clientes y proveedores (PDF en `dir`). */
export const modelLetters347 = (year: number, placeDate: string, dir: string) =>
  invoke<string>('model_letters_347', { year, placeDate, dir });

// ---------- Certificados de retenciones para los perceptores (190, 180 y 193) ----------

export interface CertificatePayee {
  nif: string;
  name: string;
  /** «clave G.01», «arrendamiento», «dividendos»… */
  concept: string;
  /** Céntimos. */
  gross: number;
  withheld: number;
  email: string | null;
}

export interface CertificatesPrep {
  model: string;
  year: number;
  payees: CertificatePayee[];
  /** Lugar propuesto (población del domicilio). */
  place: string;
  /** Firmante propuesto (el administrador de Nóminas) y su cargo. */
  signer: string;
  signer_role: string;
  notes: string[];
}

export interface CertificateOptions {
  place: string;
  /** AAAA-MM-DD. */
  date: string;
  signer: string;
  signer_role: string;
  /** NIF elegidos; vacío, todos. */
  nifs: string[];
  /** 180: NIF del arrendador → inmueble (dirección y referencia catastral). */
  properties: Record<string, string>;
}

export const modelCertificatesPrepare = (model: string, year: number) =>
  invoke<CertificatesPrep>('model_certificates_prepare', { model, year });
/** Guarda los PDF en `dir`: uno por perceptor (`each`) y/o uno con todos (`combined`). */
export const modelCertificatesSave = (
  model: string,
  year: number,
  options: CertificateOptions,
  dir: string,
  each: boolean,
  combined: boolean,
) => invoke<string[]>('model_certificates_save', { model, year, options, dir, each, combined });
/** Abre en el programa de correo un mensaje sin enviar con el certificado de `nif`. */
export const modelCertificateEmail = (
  model: string,
  year: number,
  options: CertificateOptions,
  nif: string,
  to: string | null,
) => invoke<void>('model_certificate_email', { model, year, options, nif, to });

/** Copia el plan de cuentas (nombres propios y cuentas por defecto) de otra empresa. */
export const chartCopyFrom = (companyId: string) =>
  invoke<{ accounts: number; posting_accounts: boolean }>('chart_copy_from', { companyId });

/** Traspasa el saldo de `from` a `to` el día `date` (asiento manual); null si no tenía saldo. */
export const transferBalance = (from: string, to: string, date: string) =>
  invoke<number | null>('transfer_balance', { from, to, date });

// ---------- Todas las empresas ----------

export interface DeadlineStatus {
  model: string;
  /** Para la pantalla: «Modelo 303 · 3.er trimestre 2026». */
  label: string;
  description: string;
  period: string;
  fiscal_year: number;
  due_on: string;
  /** Presentado desde TaxPain (null: no se presenta aquí). */
  filed: boolean | null;
}

export interface CompanyOverview {
  id: string;
  name: string;
  nif: string | null;
  active: boolean;
  status: {
    has_profile: boolean;
    overdue: DeadlineStatus[];
    upcoming: DeadlineStatus[];
    /** Última comprobación de lo presentado con la AEAT (`null`: nunca). */
    aeat_checked_at: string | null;
  };
}

export const companiesOverview = () => invoke<CompanyOverview[]>('companies_overview');

// ---------- Leasing ----------

export interface Leasing {
  id: number;
  name: string;
  start: string;
  /** Valor al contado sin IVA (céntimos). */
  principal: number;
  months: number;
  /** Interés nominal anual en puntos básicos (5 % = 500). */
  rate: number;
  purchase_option: number;
  /** IVA de las cuotas en puntos básicos. */
  vat: number;
  asset_account: string;
  debt_account: string;
  /** Arrendador: emite una factura por cada cuota (los contratos antiguos no lo tienen). */
  lessor?: Lessor | null;
}

/** Arrendador de un leasing. Espejo de `taxpain_core::accounting::leasing::Lessor`. */
export interface Lessor {
  nif: string;
  name: string;
  /** Porcentaje deducible del IVA de las cuotas (100 salvo uso mixto). */
  deductible_pct: number;
}

export interface LeasingInstallment {
  n: number;
  date: string;
  interest: number;
  capital: number;
  vat: number;
  amount: number;
  outstanding: number;
  posted: boolean;
  /** Contabilizada con el sistema antiguo (sin factura del arrendador): su IVA no está en el 303. */
  legacy?: boolean;
  /** Por qué esa cuota antigua no se puede regularizar desde aquí. */
  blocked?: string | null;
  /** Su factura, aún sin pagar, vence en otra fecha que la de la cuota (registrada antes del arreglo). */
  due_mismatch?: boolean;
}

/** Resultado de registrar las cuotas vencidas de un leasing. */
export interface LeasingPosted {
  /** Cuotas registradas como factura nueva del arrendador. */
  posted: number;
  /** Cuotas enlazadas con la factura real del arrendador que ya estaba en Recibidas. */
  linked: string[];
}

/** Resultado de regularizar las cuotas antiguas de un leasing. */
export interface LeasingRegularized {
  done: number;
  blocked: string[];
}

export const leasingList = () =>
  invoke<{ leasing: Leasing; installments: LeasingInstallment[] }[]>('leasing_list');
export const leasingCreate = (leasing: Leasing, depreciation: number) =>
  invoke<number>('leasing_create', { leasing, depreciation });
export const leasingPostDue = (id: number, until: string) =>
  invoke<LeasingPosted>('leasing_post_due', { id, until });
export const leasingFixDueDates = (id: number) => invoke<number>('leasing_fix_due_dates', { id });
export const leasingDelete = (id: number, confirmed: boolean) =>
  invoke<null>('leasing_delete', { id, confirmed });
export const leasingSetLessor = (id: number, lessor: Lessor) =>
  invoke<null>('leasing_set_lessor', { id, lessor });
export const leasingRegularize = (id: number, confirmed: boolean) =>
  invoke<LeasingRegularized>('leasing_regularize', { id, confirmed });
export const leasingReclassify = (year: number) => invoke<number>('leasing_reclassify', { year });

// ---------- Préstamos ----------
// Espejo de `taxpain_core::accounting::prestamo` y `taxpain_app::prestamos`. Importes en
// céntimos, tipos en centésimas de punto (5 % = 500), fechas ISO.

export type PrestamoSentido = 'concedido' | 'recibido';
export type PrestamoSistema = 'frances' | 'lineal' | 'intereses_periodicos' | 'al_vencimiento';
export type PrestamoVinculo = 'ninguno' | 'grupo' | 'asociada' | 'otra';
export type PrestamoOrigen = 'contacto' | 'socio' | 'participada' | 'texto';

export interface PrestamoContraparte {
  origen: PrestamoOrigen;
  /** Id del contacto o del socio. */
  ref_id: number | null;
  nif: string;
  nombre: string;
  /** Provincia del domicilio (01–53), para el 193. */
  provincia: string;
  persona_fisica: boolean;
  /** Banco o entidad de crédito (sin retención). */
  entidad_credito: boolean;
  vinculo: PrestamoVinculo;
}

export interface PrestamoCuentas {
  largo: string;
  corto: string;
  intereses: string;
  devengo: string;
}

export interface Prestamo {
  id: number;
  nombre: string;
  sentido: PrestamoSentido;
  contraparte: PrestamoContraparte;
  inicio: string;
  principal: number;
  tipo: number;
  sistema: PrestamoSistema;
  /** Meses entre cuotas: 1, 3, 6 o 12. */
  periodicidad: number;
  /** Plazo en meses (con la carencia). */
  plazo: number;
  carencia: number;
  carencia_tipo: 'capital' | 'total';
  cuentas: PrestamoCuentas;
  /** Retención sobre los intereses (19 % = 1900). */
  retencion: number;
  notas: string;
}

export interface PrestamoCuota {
  n: number;
  fecha: string;
  intereses: number;
  capital: number;
  capitalizados: number;
  pago: number;
  pendiente: number;
  estado: 'pendiente' | 'registrada' | 'fuera';
  retencion: number;
  /** Lo que se mueve en el banco (pago − retención). */
  neto: number;
  devengado: number;
  bank_tx: number | null;
}

export interface PrestamoDevengo {
  id: number;
  n: number;
  fecha: string;
  importe: number;
}

export interface PrestamoVista {
  prestamo: Prestamo;
  alta: 'pendiente' | 'contabilizada' | 'fuera';
  alta_bank_tx: number | null;
  cuotas: PrestamoCuota[];
  devengos: PrestamoDevengo[];
  saldo_vivo: number;
  corto_plazo: number;
  largo_plazo: number;
  vencidas: number;
  avisos: string[];
}

export interface PrestamoPropuesta {
  cuentas: PrestamoCuentas;
  retencion: number;
  motivo: string;
}

/** Con qué se paga o cobra: una cuenta de tesorería o un movimiento del banco. */
export interface PrestamoPago {
  tesoreria: string | null;
  bank_tx: number | null;
}

export interface PrestamoMovimiento {
  id: number;
  fecha: string;
  importe: number;
  concepto: string;
  cuenta: string;
  banco: string;
}

export interface PrestamoDevengado {
  prestamo: string;
  n: number;
  importe: number;
}

export const prestamosList = () => invoke<PrestamoVista[]>('prestamos_list');
export const prestamoPropuesta = (sentido: PrestamoSentido, contraparte: PrestamoContraparte) =>
  invoke<PrestamoPropuesta>('prestamo_propuesta', { sentido, contraparte });
export const prestamoCreate = (prestamo: Prestamo) =>
  invoke<number>('prestamo_create', { prestamo });
export const prestamoUpdate = (id: number, prestamo: Prestamo) =>
  invoke<null>('prestamo_update', { id, prestamo });
export const prestamoBankCandidates = (id: number, n: number) =>
  invoke<PrestamoMovimiento[]>('prestamo_bank_candidates', { id, n });
export const prestamoPostStart = (id: number, pago: PrestamoPago, confirmed: boolean) =>
  invoke<number>('prestamo_post_start', { id, pago, confirmed });
export const prestamoStartOutside = (id: number, fuera: boolean) =>
  invoke<null>('prestamo_start_outside', { id, fuera });
export const prestamoUndoStart = (id: number, confirmed: boolean) =>
  invoke<null>('prestamo_undo_start', { id, confirmed });
export const prestamoPostInstallment = (
  id: number,
  n: number,
  pago: PrestamoPago,
  confirmed: boolean,
) => invoke<number>('prestamo_post_installment', { id, n, pago, confirmed });
export const prestamoMarkOutside = (id: number, hasta: string, confirmed: boolean) =>
  invoke<number>('prestamo_mark_outside', { id, hasta, confirmed });
export const prestamoUndoInstallment = (id: number, n: number, confirmed: boolean) =>
  invoke<null>('prestamo_undo_installment', { id, n, confirmed });
export const prestamoAccrue = (id: number | null, fecha: string, confirmed: boolean) =>
  invoke<PrestamoDevengado[]>('prestamo_accrue', { id, fecha, confirmed });
export const prestamoUndoAccrual = (devengo: number, confirmed: boolean) =>
  invoke<null>('prestamo_undo_accrual', { devengo, confirmed });
export const prestamosReclassify = (year: number, confirmed: boolean) =>
  invoke<number>('prestamos_reclassify', { year, confirmed });
export const prestamoDelete = (id: number, confirmed: boolean) =>
  invoke<null>('prestamo_delete', { id, confirmed });

/** Pide dónde guardar el contrato del préstamo (RTF para revisar y firmar) y lo guarda. */
export async function prestamoContractSave(id: number, nombre: string): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: `Contrato ${nombre.replace(/[/\\:]/g, '-')}.rtf`,
    filters: [{ name: 'RTF', extensions: ['rtf'] }],
  });
  if (!path) return null;
  await invoke('prestamo_contract_save', { id, path });
  return path;
}

// ---------- 184: entidades en atribución de rentas ----------
// Espejo de `taxpain_app::atribucion`.

export interface Partner {
  id: number;
  nif: string;
  name: string;
  /** Código de provincia (01-52). */
  province: string;
  address: string;
  /** Participación en diezmilésimas de punto (50 % = 500000). */
  pct: number;
  joined_on: string | null;
  left_on: string | null;
}
export interface Settings184 {
  entity_type: number;
  main_activity: number;
  regime: number;
  activity_kind: number;
  iae: string;
  cash: boolean;
  representative_nif: string;
  representative_name: string;
  rent_situation: number;
  cadastral_ref: string;
  urban: boolean;
  rent_days: number;
}
export const partnersList = () => invoke<Partner[]>('partners_list');
export const partnerSave = (partner: Partner) => invoke<number>('partner_save', { partner });
export const partnerDelete = (id: number) => invoke<void>('partner_delete', { id });
export const settings184 = () => invoke<Settings184>('settings_184');
export const saveSettings184 = (settings: Settings184) =>
  invoke<void>('save_settings_184', { settings });

/** Espejo de `taxpain_app::consolidation`. */
export interface Elimination {
  company: string;
  account: string;
  counterpart: string;
  amount: number;
}
export interface Consolidated {
  companies: string[];
  accounts: AnnualAccounts;
  balances: AccountBalance[];
  eliminations: Elimination[];
  warnings: string[];
}
export const consolidatedAccounts = (
  ids: string[],
  year: number,
  model: AccountsModel,
  eliminate: boolean,
) => invoke<Consolidated>('consolidated_accounts', { ids, year, model, eliminate });
export interface CopyOutcome {
  copied: number;
  third_party_accounts: string[];
}
export const copyEntriesTo = (ids: number[], year: number, target: string) =>
  invoke<CopyOutcome>('copy_entries_to', { ids, year, target });

// ---------- Carpeta de la empresa y modelos del gestor ----------

export interface CompanyFacts {
  nif: string | null;
  name: string | null;
  street: string | null;
  post_code: string | null;
  town: string | null;
  constitution_date: string | null;
  /** Céntimos. */
  capital: number | null;
  cnae: string | null;
  iae: string | null;
  vat_number: string | null;
  administrator: string | null;
  notary: string | null;
  protocol: string | null;
  registry: string | null;
  date: string | null;
}

export type FolderFound =
  | {
      kind: 'ia';
      tipo: string;
      titulo: string;
      descripcion: string;
      detalle: string;
      importable: boolean;
      /** Se puede marcar aunque no se proponga (un diario de fechas que ya tienen asientos). */
      selectable?: boolean;
      year: number | null;
    }
  | { kind: 'documento'; doc_kind: string; title: string; facts: CompanyFacts; by_name: boolean }
  | {
      kind: 'diario';
      entries: number;
      first: string | null;
      last: string | null;
      already: boolean;
      /** Asientos propios de TaxPain en esas fechas: no se propone (se contarían dos veces). */
      overlap?: number;
      /** Asientos de otro lote importado en esas fechas (el diario reenviado corregido). */
      imported?: number;
    }
  | {
      kind: 'libro';
      book: 'emitidas' | 'recibidas';
      invoices: number;
      first: string | null;
      last: string | null;
      base: number;
      vat: number;
      /** Facturas propias de TaxPain en esas fechas: no se propone. */
      overlap?: number;
      /** Líneas de un libro de emitidas que son la autorrepercusión de compras, no ventas. */
      self_assessed?: number;
      /** Lo que dice la lectura con IA si no es un libro de emitidas: no se propone. */
      reading?: string | null;
    }
  | {
      kind: 'banco';
      /** Lo leído: banco, cuenta, divisa, periodo y movimientos de cada extracto. */
      resumen?: string;
      last?: string | null;
      /** ¿Cuadra con sus saldos? null si no trae saldos (listados de tarjeta…). */
      cuadra?: boolean | null;
    }
  | {
      kind: 'factura_recibida';
      supplier: string | null;
      nif?: string | null;
      number: string | null;
      date: string | null;
      total: number | null;
      in_book: boolean;
      /** De un periodo que ya declaró el gestor y no está en su libro: no se propone. */
      historical?: boolean;
    }
  | { kind: 'factura_emitida'; number: string | null; date: string | null; total: number | null }
  | {
      kind: 'modelo';
      model: string;
      year: number;
      period: string;
      /** `fichero`, `pdf`, `registros` (347 o 349 en fichero) o `justificante` (de la AEAT). */
      source: string;
      boxes: number;
      /** Un justificante sin casillas leídas cuenta como presentado, pero no se compara. */
      boxes_read?: boolean;
    }
  | {
      kind: 'cripto';
      /** Lo leído, fichero a fichero (también los de dentro de un zip). */
      resumen: string;
      ops: number;
      /** Precios de cierre (de una hoja de saldos por fecha). */
      prices: number;
      /** Operaciones que ya estaban importadas. */
      duplicates: number;
      first: string | null;
      last: string | null;
      /** Ficheros de dentro del zip que no se han podido leer (el motivo va en los avisos). */
      unread: number;
    }
  | {
      kind: 'valores';
      /** «XTB (PDF)», «Interactive Brokers (Flex XML)»… */
      formato: string;
      /** Número de cuenta en el bróker. */
      cuenta: string;
      resumen: string;
      ops: number;
      prices: number;
      /** Operaciones que ya estaban importadas. */
      duplicates: number;
      first: string | null;
      last: string | null;
    }
  | { kind: 'otro'; reason: string };

export interface FolderItem {
  path: string;
  size: number;
  year: number | null;
  found: FolderFound;
  /** Se propone (marcado de entrada). */
  importable: boolean;
  /** Se puede marcar aunque no se proponga (con aviso). */
  selectable?: boolean;
  warnings: string[];
}

export interface FolderInventory {
  root: string;
  /** Lecturas de Claude Code (`taxpain-lecturas.json`) cargadas, si las hay. */
  claude_code?: {
    loaded: number;
    missing: string[];
    invalid: number;
    /** No se cargan porque el asistente de la app las leyó después. */
    kept?: number;
    /** Lecturas de copias de otro fichero (el mismo contenido): se juntan con la suya. */
    merged?: number;
    /** El fichero entero no se pudo leer (p. ej. a medio escribir). */
    error?: string | null;
  } | null;
  items: FolderItem[];
  skipped: number;
  /** Ficheros admitidos que se han dejado fuera por pasar del límite (5.000): carpeta incompleta. */
  over_limit?: number;
  /** Subcarpetas a más de ocho niveles, que no se recorren. */
  too_deep?: number;
}

export interface FolderOutcome {
  path: string;
  ok: boolean;
  message: string;
}

export const folderGet = () => invoke<string | null>('folder_get');
export const folderScan = (root: string) => invoke<FolderInventory>('folder_scan', { root });
export const folderApply = (root: string, paths: string[]) =>
  invoke<FolderOutcome[]>('folder_apply', { root, paths });

// ---------- Revisión de la carpeta ----------

export type Gravedad = 'error' | 'aviso' | 'info';
export type AreaRevision =
  | 'ficheros'
  | 'facturas'
  | 'emitidas'
  | 'libros'
  | 'modelos'
  | 'sociedades'
  | 'bancos'
  | 'plazos'
  | 'aeat'
  | 'socios';
export type Responsable = 'empresa' | 'gestoria' | 'documentacion';
export type Destinatario = 'gestoria' | 'registro' | 'aeat' | 'empresa';

/** Algo que la revisión ha encontrado en la carpeta. */
export interface Hallazgo {
  codigo: string;
  area: AreaRevision;
  gravedad: Gravedad;
  responsable: Responsable;
  ejercicio: number | null;
  titulo: string;
  detalle: string;
  /** Rutas relativas a la carpeta. */
  ficheros: string[];
  /** Céntimos: positivo, recuperable; negativo, a pagar. */
  importe: number | null;
  /** Hasta cuándo se puede rectificar (AAAA-MM-DD), si aplica. */
  rectificable_hasta?: string | null;
  /**
   * La fecha de `rectificable_hasta` es aproximada: TaxPain no tiene los festivos del año en que
   * acababa el plazo (el día de verdad puede ser algo después, nunca antes).
   */
  rectificable_aprox?: boolean;
  /** «Urgente: puede haber deuda», con el recargo de hoy y el siguiente escalón. */
  urgente?: Urgente | null;
}

/** Lo urgente de un hallazgo: puede haber dinero pendiente con Hacienda o la Seguridad Social. */
export interface Urgente {
  /** Céntimos: la cuota que puede deberse (sin recargo), si se sabe. */
  en_juego: number | null;
  /** AAAA-MM-DD: último día del plazo. */
  vencio: string | null;
  porcentaje_hoy: number | null;
  /** Céntimos. */
  recargo_hoy: number | null;
  /** AAAA-MM-DD: desde cuándo sube al siguiente escalón. */
  siguiente_desde: string | null;
  siguiente_porcentaje: number | null;
  /** Céntimos. */
  siguiente_recargo: number | null;
  nota: string;
}

export interface TablaRevision {
  titulo: string;
  /** Nombre corto de la hoja de Excel. */
  hoja?: string;
  columnas: string[];
  filas: string[][];
}

export interface Peticion {
  a_quien: Destinatario;
  que: string;
}

export interface ResumenRevision {
  ficheros: number;
  con_detalle: number;
  sin_leer: number;
  no_admitidos: number;
  por_tipo: { tipo: string; n: number }[];
  errores: number;
  avisos: number;
  info: number;
  /** Hallazgos urgentes (falta en revisiones antiguas). */
  urgentes?: number;
  /** Céntimos: lo que está en juego en los urgentes. */
  en_juego_urgente?: number;
  /** Céntimos. */
  recuperable: number;
  /**
   * Céntimos: a tener en cuenta en el próximo 200 (aún sin presentar), aparte de `recuperable`:
   * las retenciones soportadas de los dividendos que confirman los documentos de las
   * participadas más el impuesto que se ahorra con la exención. Falta en revisiones antiguas.
   */
  a_favor_proximo_200?: number;
  /** Céntimos: de lo anterior, las retenciones soportadas. */
  retenciones_proximo_200?: number;
  /** Céntimos: de lo anterior, el impuesto que se ahorra con la exención (estimación). */
  exencion_proximo_200?: number;
  impuesto_dividendos_proximo_200?: number;
}

/** Dividendo cobrado de una participada, con el bruto y la retención de sus documentos. */
export interface DividendoConfirmado {
  ejercicio: number;
  /** AAAA-MM-DD */
  fecha: string;
  pagadora: string;
  /** Céntimos. */
  neto: number;
  bruto: number;
  retencion: number;
  /** Con al menos el 5 % según los documentos: exento al 95 % y sin retención. */
  grupo: boolean;
}

export interface Revision {
  /** AAAA-MM-DD */
  generada: string;
  empresa: string;
  nif: string | null;
  carpeta: string;
  resumen: ResumenRevision;
  hallazgos: Hallazgo[];
  tablas: TablaRevision[];
  pedir: Peticion[];
  /** Dividendos para el próximo 200 confirmados con documentos (falta en revisiones antiguas). */
  dividendos?: DividendoConfirmado[];
}

export const folderReview = (root: string) => invoke<Revision>('folder_review', { root });
/** Qué revisar con lo que tiene la AEAT y lo que sabe TaxPain, sin carpeta. */
export const companyReview = () => invoke<Revision>('company_review');

/** Una cifra de un año en la evolución de la empresa. */
export interface ValorEvolucion {
  año: number;
  /** Céntimos (o el número, en los recuentos); `null`: sin dato. */
  valor: number | null;
  /** De qué modelo y casilla sale. */
  fuente: string | null;
  /** Diferencia con el año anterior (céntimos o número). */
  diferencia: number | null;
  /** La misma en décimas de tanto por ciento. */
  variacion: number | null;
  /** Avance de un año sin cerrar (sin su declaración anual): no se compara con el anterior. */
  avance?: boolean;
}

export interface FilaEvolucion {
  id: string;
  titulo: string;
  ayuda: string;
  recuento: boolean;
  valores: ValorEvolucion[];
}

export interface Evolucion {
  años: number[];
  grupos: { titulo: string; filas: FilaEvolucion[] }[];
  /** No hay ninguna declaración de la AEAT guardada. */
  sin_comprobar: boolean;
  /** Declaraciones vigentes sin casillas leídas. */
  sin_casillas: number;
}

/** La empresa año a año con lo que tiene la AEAT (sin consultarla). */
export const companyEvolution = () => invoke<Evolucion>('company_evolution');

// Ahorrar impuestos (espejo de `taxpain_app::ahorro`)
export type AreaAhorro =
  | 'beneficio'
  | 'inversion'
  | 'gastos'
  | 'personas'
  | 'iva'
  | 'donativos'
  | 'cripto';
export type EncajeAhorro = 'conviene' | 'valorar' | 'aprovechado' | 'no_aplica';
export type EstadoAhorro = 'pendiente' | 'en_marcha' | 'hecho' | 'descartado';
export interface MarcaAhorro {
  estado: EstadoAhorro;
  /** Índices de los pasos hechos. */
  pasos: number[];
}
export interface CifraAhorro {
  valor: number;
  fuente: string;
}
export interface SituacionAhorro {
  año: number;
  cierre: string;
  persona_fisica: boolean;
  regimen: string | null;
  cifra_negocios: CifraAhorro | null;
  base: CifraAhorro | null;
  cuota: CifraAhorro | null;
  tipo: number | null;
  trabajadores: boolean;
  cnae: string | null;
  notas: string[];
}
export interface Recomendacion {
  id: string;
  area: AreaAhorro;
  titulo: string;
  resumen: string;
  encaje: EncajeAhorro;
  motivo: string;
  ahorro: number | null;
  aplazado: boolean;
  calculo: string | null;
  plazo: string | null;
  requisitos: string[];
  pasos: string[];
  riesgos: string[];
  norma: string;
  enlace: string;
  /** Otra pantalla de TaxPain que ayuda: `modelos`, `cartera`, `cripto`, `contabilidad`… */
  pantalla: string | null;
  marca: MarcaAhorro;
}
export interface Ahorro {
  situacion: SituacionAhorro;
  recomendaciones: Recomendacion[];
  ahorro_posible: number;
  aplazado_posible: number;
}
export const taxSavings = () => invoke<Ahorro>('tax_savings');
export const taxSavingsMark = (id: string, marca: MarcaAhorro) =>
  invoke<void>('tax_savings_mark', { id, marca });
/** La última revisión de la empresa (se guarda al revisar). */
export const folderReviewLast = () => invoke<Revision | null>('folder_review_last');

export type FormatoInforme = 'pdf' | 'xlsx' | 'md';

const FORMATO_INFORME: Record<FormatoInforme, string> = {
  pdf: 'PDF',
  xlsx: 'Excel',
  md: 'Markdown',
};

/**
 * Guarda el informe de la revisión donde diga el usuario. `false` si cancela. El contenido lo
 * pone Rust: la última revisión de la carpeta (`carpeta`) o la revisión con lo que tiene la AEAT
 * (`aeat`, la de `companyReview`); `review` sólo da el nombre propuesto.
 */
export async function folderReviewSave(
  review: Revision,
  format: FormatoInforme,
  source: 'carpeta' | 'aeat' = 'carpeta',
): Promise<boolean> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: `revision-${review.generada}.${format}`,
    filters: [{ name: FORMATO_INFORME[format], extensions: [format] }],
  });
  if (!path) return false;
  await invoke('folder_review_save', { source, path, format });
  return true;
}

/** Importación de un libro de facturas (o de facturas leídas con IA): se puede deshacer. */
export interface BookBatch {
  id: number;
  kind: 'emitidas' | 'recibidas';
  source: 'libro' | 'lectura';
  filename: string;
  invoices: number;
  created_at: string;
}
export const folderBookImports = () => invoke<BookBatch[]>('folder_book_imports');
export const folderBookUndo = (id: number) =>
  invoke<number>('folder_book_undo', { id, confirmed: true });

export interface FiledModel {
  id: number;
  model: string;
  year: number;
  period: string;
  filename: string;
  source: 'fichero' | 'pdf' | 'manual';
  /** Casilla → céntimos. */
  boxes: Record<string, number>;
}

export interface CompareRow {
  box: number;
  ours: number;
  theirs: number | null;
  diff: number;
  /** Qué es la casilla (del diseño de registro); vacío si no se sabe. */
  label?: string;
  /** Si no cuadra: de dónde suele venir la diferencia. */
  explanation?: string | null;
}

/** Un perceptor (190, 180, 193) o tercero (347) en la comparación de una informativa. */
export interface CompareLine {
  nif: string;
  name: string;
  key: string;
  ours: number | null;
  theirs: number | null;
  ours_withheld: number | null;
  theirs_withheld: number | null;
  explanation: string | null;
}

export interface Comparison {
  filed: FiledModel;
  rows: CompareRow[];
  mismatches: number;
  warnings: string[];
  /** Informativas con fichero: perceptor a perceptor o tercero a tercero. */
  lines?: CompareLine[];
}

export const filedList = (year: number | null) => invoke<FiledModel[]>('filed_list', { year });
export const filedCompare = (id: number) => invoke<Comparison>('filed_compare', { id });
export const filedSaveManual = (
  model: string,
  year: number,
  period: string,
  boxes: Record<number, number>,
) => invoke<number>('filed_save_manual', { model, year, period, boxes });
export const filedDelete = (id: number) => invoke<void>('filed_delete', { id });

export interface FillOutcome {
  filled: number;
  failed: string[];
}

/** Completa con el asistente las recibidas pendientes a las que les faltan datos. */
export const receivedAiFillPending = (limit: number) =>
  invoke<FillOutcome>('received_ai_fill_pending', { limit });

export interface CompanyDoc {
  id: number;
  kind: string;
  title: string;
  filename: string;
  facts: CompanyFacts;
  date: string | null;
  imported_at: string;
}

export interface CompanySuggestion {
  field: string;
  label: string;
  current: string | null;
  proposed: string;
  source: string;
}

export const companyDocsList = () => invoke<CompanyDoc[]>('company_docs_list');
export const companyDocOpen = (id: number) => invoke<void>('company_doc_open', { id });
export const companyDocDelete = (id: number) => invoke<void>('company_doc_delete', { id });
export const companyDocAi = (id: number) => invoke<CompanyFacts>('company_doc_ai', { id });
export const companySuggestions = () => invoke<CompanySuggestion[]>('company_suggestions');
export const companyApplySuggestion = (field: string) =>
  invoke<void>('company_apply_suggestion', { field });

// ---------- Pruebas VERI*FACTU (entorno de pruebas de la AEAT) ----------

export interface LabCheck {
  id: string;
  label: string;
  ok: boolean;
  detail: string;
  blocking: boolean;
}

export interface LabRecord {
  num_serie: string;
  record_type: 'alta' | 'anulacion';
  send_status: string;
  error: string | null;
  csv: string | null;
  sent_at: string | null;
}

export interface LabStatus {
  is_lab: boolean;
  info: { created_at: string; invoices: string[] } | null;
  checks: LabCheck[];
  records: LabRecord[];
}

export interface LabCompared {
  num_serie: string;
  local_status: string;
  aeat_status: string | null;
  huella_ok: boolean | null;
  error: string | null;
  ok: boolean;
}

export const verifactuLabStatus = (fingerprint: string | null) =>
  invoke<LabStatus>('verifactu_lab_status', { fingerprint });
export const verifactuLabCreate = () => invoke<void>('verifactu_lab_create');
export const verifactuLabUseCertNif = (fingerprint: string | null) =>
  invoke<void>('verifactu_lab_use_cert_nif', { fingerprint });
export const verifactuLabGenerate = () => invoke<string[]>('verifactu_lab_generate');
export const verifactuLabCancel = () => invoke<string>('verifactu_lab_cancel');
export const verifactuLabQuery = (fingerprint: string, year: number, month: number) =>
  invoke<LabCompared[]>('verifactu_lab_query', { fingerprint, year, month });

export async function verifactuLabReportSave(fingerprint: string | null): Promise<boolean> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: 'pruebas-verifactu.md',
    filters: [{ name: 'Markdown', extensions: ['md'] }],
  });
  if (!path) return false;
  await invoke('verifactu_lab_report_save', { fingerprint, path });
  return true;
}

// ---------- Lectura con el asistente («Full IA») ----------

export interface LecturaIaSettings {
  full: boolean;
  model: string | null;
  accepted_at: string | null;
}

export const lecturaIaGet = () => invoke<LecturaIaSettings>('lectura_ia_get');
export const lecturaIaSave = (settings: LecturaIaSettings) =>
  invoke<LecturaIaSettings>('lectura_ia_save', { settings });

export interface AiReadOutcome {
  read: number;
  reused: number;
  failed: [string, string][];
}

export const folderAiRead = (root: string, paths: string[], again: boolean) =>
  invoke<AiReadOutcome>('folder_ai_read', { root, paths, again });

/** Progreso de la lectura con el asistente (evento del proceso nativo). */
export async function onFolderAiProgress(
  f: (p: { done: number; total: number; path: string }) => void,
): Promise<() => void> {
  if (!isTauri() || demoInvoke) return () => {};
  const { listen } = await import('@tauri-apps/api/event');
  return listen<{ done: number; total: number; path: string }>('folder-ai-progress', (e) =>
    f(e.payload),
  );
}

// ---------- Copias de seguridad ----------

export interface BackupSettings {
  folder: string | null;
  auto: boolean;
  every_days: number;
  keep: number;
}

export interface BackupStatus {
  has_passphrase: boolean;
  settings: BackupSettings;
  last: { at: string; path: string; bytes: number; auto: boolean } | null;
  last_error: { at: string; message: string } | null;
  days_since: number | null;
  /** Hace más de 30 días (o nunca) que no se hace una copia. */
  remind: boolean;
  suggested_folder: string | null;
  suggested_name: string;
  restored: Restored | null;
  min_passphrase: number;
  /** Junto a los datos está el código de recuperación: carpeta + contraseña abren los datos. */
  recovery_code: boolean;
  /** Empresa recién dada de alta sin contraseña de copias: se pide antes de empezar. */
  setup_required: boolean;
}

export interface Restored {
  at: string;
  copy_created_at: string;
  acknowledged_at: string | null;
}

export interface BackupReport {
  path: string;
  bytes: number;
  created_at: string;
  sha256: string;
  audit_entries: number;
}

export interface BackupSummary {
  created_at: string;
  app_version: string;
  company: string;
  nif: string | null;
  invoices_issued: number;
  invoices_received: number;
  journal_entries: number;
  last_invoice_date: string | null;
  newer_version: boolean;
  /** Empresa de este Mac con el mismo NIF. */
  same_nif_company: string | null;
}

export interface RestoreCheck {
  restored: Restored;
  /** Última factura de cada serie: [serie, número, fecha]. */
  last_by_series: [string, string, string][];
  /** Último registro VERI*FACTU: [número, fecha, huella]. */
  last_verifactu: [string, string, string] | null;
}

export const backupStatus = () => invoke<BackupStatus>('backup_status');
export const backupSetPassphrase = (passphrase: string) =>
  invoke<void>('backup_set_passphrase', { passphrase });
export const backupVerifyPassphrase = (passphrase: string) =>
  invoke<boolean>('backup_verify_passphrase', { passphrase });
export const backupSaveSettings = (settings: BackupSettings) =>
  invoke<BackupSettings>('backup_save_settings', { settings });
export const backupInspect = (path: string, passphrase: string) =>
  invoke<BackupSummary>('backup_inspect', { path, passphrase });
export const backupRestore = (path: string, passphrase: string) =>
  invoke<Company>('backup_restore', { path, passphrase });
/**
 * Los datos no se han podido abrir porque el Llavero no tiene su clave, pero hay código de
 * recuperación: se pide la contraseña de las copias. `null` si se abrieron con normalidad.
 */
/** Datos bloqueados (`several`: hay varias empresas) o que no se han podido abrir (`failed`: la
 * explicación). */
export const vaultLocked = () =>
  invoke<{ several: boolean; failed?: string; broken?: Diagnostico[] } | null>('vault_locked');
/** Recupera los datos con la contraseña de las copias (la clave vuelve al Llavero). */
export const vaultRecover = (passphrase: string) => invoke<void>('vault_recover', { passphrase });
export const restoreCheck = () => invoke<RestoreCheck | null>('restore_check');
export const restoreAcknowledge = () => invoke<void>('restore_acknowledge');

/** Pide dónde guardar la copia y la hace. `null` si el usuario cancela. */
export async function backupCreate(
  folder: string | null,
  name: string,
): Promise<BackupReport | null> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: folder ? `${folder}/${name}` : name,
    filters: [{ name: 'Copia de TaxPain', extensions: ['taxpain'] }],
  });
  if (!path) return null;
  return invoke<BackupReport>('backup_create', { path });
}

/** Elige un fichero de copia para restaurarlo. */
export async function pickBackupFile(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const p = await open({
    multiple: false,
    filters: [{ name: 'Copia de TaxPain', extensions: ['taxpain'] }],
  });
  return typeof p === 'string' ? p : null;
}

/** «1,2 MB». */
export function fileSize(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1).replace('.', ',')} MB`;
}

// ---------- Clientes y proveedores ----------

/** Países de la UE (códigos ISO; Grecia es GR aunque su NIF-IVA empiece por EL). «XI» es Irlanda
 * del Norte, que sólo cuenta como UE para los bienes (para los servicios es Reino Unido). */
export const EU_COUNTRIES = [
  'AT',
  'BE',
  'BG',
  'CY',
  'CZ',
  'DE',
  'DK',
  'EE',
  'FI',
  'FR',
  'GR',
  'HR',
  'HU',
  'IE',
  'IT',
  'LT',
  'LU',
  'LV',
  'MT',
  'NL',
  'PL',
  'PT',
  'RO',
  'SE',
  'SI',
  'SK',
  'XI',
];

/** Un servicio a este país es intracomunitario (Irlanda del Norte no: es Reino Unido). */
export const euForServices = (country: string) =>
  country !== 'XI' && EU_COUNTRIES.includes(country);

/** Países para elegir: España, la UE y los de fuera más habituales (código, nombre). */
export const COUNTRIES: [string, string][] = [
  ['ES', 'España'],
  ['DE', 'Alemania'],
  ['AT', 'Austria'],
  ['BE', 'Bélgica'],
  ['BG', 'Bulgaria'],
  ['CY', 'Chipre'],
  ['HR', 'Croacia'],
  ['DK', 'Dinamarca'],
  ['SK', 'Eslovaquia'],
  ['SI', 'Eslovenia'],
  ['EE', 'Estonia'],
  ['FI', 'Finlandia'],
  ['FR', 'Francia'],
  ['GR', 'Grecia'],
  ['HU', 'Hungría'],
  ['IE', 'Irlanda'],
  ['IT', 'Italia'],
  ['LV', 'Letonia'],
  ['LT', 'Lituania'],
  ['LU', 'Luxemburgo'],
  ['MT', 'Malta'],
  ['NL', 'Países Bajos'],
  ['PL', 'Polonia'],
  ['PT', 'Portugal'],
  ['CZ', 'República Checa'],
  ['RO', 'Rumanía'],
  ['SE', 'Suecia'],
  ['AD', 'Andorra'],
  ['AU', 'Australia'],
  ['CA', 'Canadá'],
  ['CH', 'Suiza'],
  ['US', 'Estados Unidos'],
  ['MX', 'México'],
  ['NO', 'Noruega'],
  ['GB', 'Reino Unido'],
  ['XI', 'Irlanda del Norte (bienes)'],
];

export function countryName(code: string): string {
  return COUNTRIES.find(([c]) => c === code)?.[1] ?? code;
}

export interface ContactData {
  party: Party;
  email: string | null;
  language: 'Es' | 'En' | null;
  notes: string | null;
  /** Última comprobación en VIES (la conserva el proceso nativo al guardar). */
  vies?: {
    valid: boolean;
    checked_at: string;
    request_id: string | null;
    name: string | null;
  } | null;
}

export interface ContactRow {
  key: string;
  id: number | null;
  role: 'cliente' | 'proveedor' | 'ambos';
  name: string;
  tax_id: string;
  country: string;
  email: string | null;
  invoiced_year: number;
  invoiced_total: number;
  purchased_year: number;
  purchased_total: number;
  to_collect: number;
  to_pay: number;
  overdue: number;
  last_date: string | null;
  invoices: number;
}

export interface ContactHistoryItem {
  kind: 'emitida' | 'recibida';
  id: number;
  number: string;
  date: string;
  total: number;
  status: string;
  pending: number;
  /** Parte de lo pendiente ya vencida. */
  overdue: number;
}

export interface ContactDetail {
  row: ContactRow;
  data: ContactData | null;
  party: Party | null;
  history: ContactHistoryItem[];
  years: { year: number; invoiced: number; purchased: number }[];
}

export interface ContactSuggestion {
  key: string;
  party: Party;
  email: string | null;
  language: 'Es' | 'En' | null;
}

export const contactsList = (year: number) => invoke<ContactRow[]>('contacts_list', { year });
export const contactDetail = (key: string, year: number) =>
  invoke<ContactDetail>('contact_detail', { key, year });
export const contactSave = (input: { id: number | null; role: string; data: ContactData }) =>
  invoke<number>('contact_save', { input });
export const contactDelete = (id: number) => invoke<void>('contact_delete', { id });
export const contactsSearch = (query: string) =>
  invoke<ContactSuggestion[]>('contacts_search', { query });

// ---------- Correo ----------

export interface EmailDraft {
  to: string | null;
  subject: string;
  body: string;
  attachments: string[];
  language: 'Es' | 'En';
  /** Recordatorio: cuál sería (1, 2, 3…). `null` si es el envío de la factura. */
  reminder: number | null;
  last_reminder: string | null;
  pending: number;
  /** Lo ya vencido (sin los plazos que aún no han llegado). */
  overdue: number;
  overdue_days: number;
}

export const invoiceEmailCompose = (id: number, reminder: boolean) =>
  invoke<EmailDraft>('invoice_email_compose', { id, reminder });
export const invoiceEmailOpen = (
  id: number,
  reminder: boolean,
  to: string | null,
  subject: string,
  body: string,
) => invoke<void>('invoice_email_open', { id, reminder, to, subject, body });

// ---------- Facturas periódicas ----------

export interface InvoiceTemplate {
  id: string;
  name: string;
  draft: InvoiceDraft;
  language: 'Es' | 'En' | null;
  every_months: number;
  start: string;
  issued: number;
  end: string | null;
  active: boolean;
  last: [string, string] | null;
  /** Siguiente fecha (null si ya terminó). */
  next_date: string | null;
  /** Le toca: la siguiente es hoy o antes. */
  due: boolean;
  client: string | null;
}

export interface TemplatePatch {
  name: string;
  every_months: number;
  /** Sólo si se cambia: la serie vuelve a empezar en esa fecha. */
  next_date: string | null;
  end: string | null;
  active: boolean;
  line_descriptions: string[] | null;
  line_prices: number[] | null;
}

export const recurringList = () => invoke<InvoiceTemplate[]>('recurring_list');
export const recurringCreate = (invoiceId: number, everyMonths: number) =>
  invoke<InvoiceTemplate>('recurring_create', { invoiceId, everyMonths });
export const recurringUpdate = (id: string, patch: TemplatePatch) =>
  invoke<void>('recurring_update', { id, patch });
export const recurringDelete = (id: string) => invoke<void>('recurring_delete', { id });
export interface RecurringPreview {
  draft: InvoiceDraft;
  totals: Totals;
  /** Factura atrasada de un periodo de IVA ya presentado o vencido: por qué (hay que elegir). */
  late_vat?: string | null;
}
export const recurringPreview = (id: string) =>
  invoke<RecurringPreview>('recurring_preview', { id });
/** Factura atrasada de un periodo de IVA cerrado: con la fecha de la operación de aquel periodo
 * (`period`) o sin ella, con el IVA en el periodo actual (`today`). */
export type LateVat = 'period' | 'today';
/** `currency`: en una plantilla en moneda extranjera, el tipo del día de la operación. */
export const recurringIssue = (
  id: string,
  confirmed: boolean,
  currency: ForeignCurrency | null = null,
  late: LateVat | null = null,
) =>
  invoke<IssuedInvoice>('recurring_issue', {
    id,
    confirmed,
    ...(currency ? { currency } : {}),
    ...(late ? { late } : {}),
  });
export const invoiceDraftOf = (id: number) => invoke<InvoiceDraft>('invoice_draft_of', { id });
/** Idioma en que se emitió una factura (para que «Duplicar» la saque igual). */
export const invoiceLanguageOf = (id: number) =>
  invoke<'Es' | 'En' | null>('invoice_language_of', { id });

export function everyLabel(months: number): string {
  if (months === 1) return 'Cada mes';
  if (months === 3) return 'Cada trimestre';
  if (months === 12) return 'Cada año';
  return `Cada ${months} meses`;
}
// ---------- Previsión de tesorería ----------
// Espejo de `taxpain_app::forecast`. Importes en céntimos; fechas ISO `YYYY-MM-DD`.

export type ForecastCategory =
  | 'collections'
  | 'payments'
  | 'taxes'
  | 'payroll'
  | 'social_security'
  | 'leasing'
  | 'loans'
  | 'recurring';

export interface ForecastItem {
  /** Fecha en la previsión: el vencimiento o, si ya pasó, hoy. */
  date: string;
  due_date: string;
  concept: string;
  /** Positivo: entra; negativo: sale. */
  amount: number;
  category: ForecastCategory;
  source: string;
  overdue: boolean;
  estimated: boolean;
}

export interface ForecastBucket {
  start: string;
  end: string;
  opening: number;
  inflows: Partial<Record<ForecastCategory, number>>;
  /** En positivo. */
  outflows: Partial<Record<ForecastCategory, number>>;
  total_in: number;
  total_out: number;
  closing: number;
}

export interface ForecastHorizon {
  buckets: ForecastBucket[];
  end: string;
  /** Saldo de hoy antes de nada y al final de cada día con movimientos. */
  points: { date: string; balance: number }[];
  lowest: number;
  lowest_date: string;
  first_negative: string | null;
}

export interface CashForecast {
  today: string;
  opening: number;
  weekly: ForecastHorizon;
  monthly: ForecastHorizon;
  /** IVA (477 − 472 + 4750) y retenciones (4751) aún sin ingresar. */
  not_yours: { vat: number; withholdings: number; total: number };
  items: ForecastItem[];
  warnings: { kind: 'negative' | 'overdue' | 'tax' | 'data'; text: string }[];
  notes: string[];
}

export const cashForecast = () => invoke<CashForecast>('cash_forecast');

// ---------- Búsqueda global ----------

export interface SearchHit {
  kind: 'emitida' | 'recibida' | 'contacto' | 'cuenta' | 'asiento' | 'documento';
  title: string;
  subtitle: string;
  target: string;
  date: string | null;
}

export const globalSearch = (query: string) => invoke<SearchHit[]>('global_search', { query });

// ---------- Cierre guiado ----------

export interface ClosingChecklist {
  year: number;
  start: string;
  end: string;
  closed: boolean;
  /** Paga el Impuesto sobre Sociedades. */
  company: boolean;
  /** SA o SL: aplicación del resultado (LSC) y dividendos. */
  lsc: boolean;
  items: { id: string; label: string; ok: boolean; detail: string }[];
  deadlines: { label: string; date: string }[];
}

export interface ProposedLine {
  account: string;
  name: string;
  debit: number;
  credit: number;
}

export interface CorporateTaxProposal {
  year: number;
  date: string;
  /** Fin del plazo del 200. */
  due: string;
  tax: number;
  withholdings: number;
  installments: number;
  result: number;
  lines: ProposedLine[];
  entry_id: number | null;
  /** El asiento contabilizado ya no coincide con el borrador del 200. */
  stale: boolean;
  warnings: string[];
}

export interface Allocation {
  legal: number;
  losses: number;
  voluntary: number;
  dividends: number;
  remanente: number;
  /** Reserva de capitalización (art. 25 LIS). */
  capitalization: number;
}

export interface AllocationProposal {
  year: number;
  result: number;
  capital: number;
  legal_reserve: number;
  legal_minimum: number;
  losses: number;
  /** Patrimonio neto a efectos de reparto (con el resultado). */
  equity: number;
  /** Pérdidas que el beneficio ha de compensar antes de la reserva legal. */
  required_compensation: number;
  max_dividends: number;
  capitalization_minimum: number;
  /** Dividendo a cuenta ya repartido (557). */
  interim_dividend: number;
  suggested: Allocation;
  earliest: string;
  meeting_deadline: string;
  entry_id: number | null;
  warnings: string[];
}

/** Saldos simultáneos de la 4700 y la 4750 que dejaban las liquidaciones antiguas del IVA. */
export interface VatBalances {
  year: number;
  date: string;
  debtor: number;
  creditor: number;
  amount: number;
  lines: ProposedLine[];
}

export const closingVatBalances = (year: number) =>
  invoke<VatBalances | null>('closing_vat_balances', { year });
export const closingVatBalancesPost = (year: number) =>
  invoke<number>('closing_vat_balances_post', { year, confirmed: true });
export const closingChecklist = (year: number) =>
  invoke<ClosingChecklist>('closing_checklist', { year });
export const closingCorporateTax = (year: number) =>
  invoke<CorporateTaxProposal>('closing_corporate_tax', { year });
export const closingCorporateTaxPost = (year: number) =>
  invoke<number>('closing_corporate_tax_post', { year, confirmed: true });
export const closingCorporateTaxUndo = (year: number) =>
  invoke<void>('closing_corporate_tax_undo', { year, confirmed: true });
export const closingAllocation = (year: number) =>
  invoke<AllocationProposal>('closing_allocation', { year });
export const closingAllocationPost = (year: number, date: string, allocation: Allocation) =>
  invoke<number>('closing_allocation_post', { year, date, allocation, confirmed: true });
export const closingAllocationUndo = (year: number) =>
  invoke<void>('closing_allocation_undo', { year, confirmed: true });

// ---------- Presupuestos y proformas ----------

export type QuoteStatus = 'borrador' | 'enviado' | 'aceptado' | 'rechazado' | 'facturado';

export interface Quote {
  id: number;
  kind: 'presupuesto' | 'proforma';
  number: string;
  issue_date: string;
  valid_until: string | null;
  status: QuoteStatus;
  recipient_name: string | null;
  /** En euros (en moneda extranjera, el equivalente al tipo del presupuesto). */
  total: number;
  /** En moneda extranjera, el total en esa moneda. */
  currency_total?: number | null;
  /** Total a pagar en euros (el total menos la retención). */
  payable?: number;
  /** En moneda extranjera, el total a pagar en esa moneda. */
  currency_payable?: number | null;
  draft: InvoiceDraft;
  language: 'Es' | 'En' | null;
  invoice_id: number | null;
  invoice_number: string | null;
  /** Enviado (o en borrador) y ya caducado. */
  expired: boolean;
}

export interface QuoteInput {
  kind: 'presupuesto' | 'proforma';
  draft: InvoiceDraft;
  valid_until: string | null;
  language: 'Es' | 'En' | null;
}

export const quotesList = (year: number) => invoke<Quote[]>('quotes_list', { year });
export const quoteCreate = (input: QuoteInput) => invoke<Quote>('quote_create', { input });
export const quoteUpdate = (id: number, input: QuoteInput) =>
  invoke<Quote>('quote_update', { id, input });
export const quoteStatus = (id: number, status: QuoteStatus) =>
  invoke<void>('quote_status', { id, status });
export const quoteDelete = (id: number) => invoke<void>('quote_delete', { id });
export const quoteOpen = (id: number) => invoke<void>('quote_open', { id });
export const quoteInvoiceDraft = (id: number) =>
  invoke<{ draft: InvoiceDraft; language: 'Es' | 'En' | null }>('quote_invoice_draft', { id });
/** Emite la factura del presupuesto y lo deja facturado, de una vez (si falla, ni lo uno ni lo
 * otro). */
export const quoteIssueInvoice = (
  id: number,
  draft: InvoiceDraft,
  confirmed: boolean,
  language: 'Es' | 'En' | null = null,
) => invoke<IssuedInvoice>('quote_issue_invoice', { id, draft, confirmed, language });
export const quoteEmailCompose = (id: number) => invoke<EmailDraft>('quote_email_compose', { id });
export const quoteEmailOpen = (id: number, to: string | null, subject: string, body: string) =>
  invoke<void>('quote_email_open', { id, to, subject, body });

/** Pide dónde guardar el PDF del presupuesto. */
export async function quoteSave(id: number, number: string): Promise<boolean> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: `${number.replace(/[^\w.-]/g, '-')}.pdf`,
    filters: [{ name: 'PDF', extensions: ['pdf'] }],
  });
  if (!path) return false;
  await invoke('quote_save', { id, path });
  return true;
}

// ---------- VIES ----------

export interface ViesResult {
  country: string;
  vat_number: string;
  valid: boolean;
  name: string | null;
  address: string | null;
  request_id: string | null;
  request_date: string | null;
}

/** Comprueba un NIF-IVA en VIES (Comisión Europea) y guarda la prueba en la ficha. */
/** Comprueba el NIF-IVA en VIES y guarda la prueba en la ficha (si no la hay, la crea como
 * `role`: cliente o proveedor; una que ya existe conserva su tipo). */
export const viesCheck = (
  country: string,
  vat: string,
  name: string | null,
  role: 'cliente' | 'proveedor' | 'ambos' = 'cliente',
) => invoke<ViesResult>('vies_check', { country, vat, name, role });

// ---------- Segundo plano ----------

export const autostartGet = () => invoke<boolean>('autostart_get');
export const autostartSet = (enabled: boolean) => invoke<boolean>('autostart_set', { enabled });

// ---------- Memoria ----------

/** Pide dónde guardar el borrador de la memoria (RTF para editar o PDF) y lo guarda, con el
 * modelo `model` (si no se indica, el elegido en los ajustes). */
export async function memoriaSave(
  year: number,
  pdf: boolean,
  model?: AccountsModel,
): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: `Memoria ${year}.${pdf ? 'pdf' : 'rtf'}`,
    filters: [pdf ? { name: 'PDF', extensions: ['pdf'] } : { name: 'RTF', extensions: ['rtf'] }],
  });
  if (!path) return null;
  await invoke('memoria_save', { year, path, pdf, model: model ?? null });
  return path;
}

/** Textos de la memoria que no salen de la contabilidad. */
export interface MemoriaTextos {
  /** Nota fija de regímenes especiales (se repite cada año; p. ej. art. 86 LIS). */
  nota_fija: string;
  hechos_posteriores: string;
  /** Céntimos; 0 = no se audita; null = se deja el corchete. */
  honorarios_auditoria: number | null;
  /** Fecha de formulación (AAAA-MM-DD). */
  formulacion: string | null;
  firmantes: string[];
}
export const memoriaTextos = (year: number) => invoke<MemoriaTextos>('memoria_textos', { year });
export const memoriaTextosGuardar = (year: number, textos: MemoriaTextos) =>
  invoke<void>('memoria_textos_guardar', { year, textos });

/** Datos de un ejercicio para los límites de los arts. 257 y 258 LSC. */
export interface ModeloEjercicio {
  year: number;
  con_datos: boolean;
  activo: number;
  cifra_negocios: number;
  /** Plantilla media en décimas de persona. */
  plantilla_decimas: number;
  cumple_abreviado: number;
  cumple_pyg_abreviada: number;
  cumple_micro: number;
}
export interface ModeloAviso {
  year: number;
  ejercicios: ModeloEjercicio[];
  recomendado: AccountsModel;
  puede_abreviado: boolean | null;
  puede_pyg_abreviada: boolean | null;
  puede_micro: boolean | null;
  elegido: AccountsModel;
  explicacion: string[];
  aviso: string | null;
}
export const cuentasModeloAviso = (year: number) =>
  invoke<ModeloAviso>('cuentas_modelo_aviso', { year });
export const cuentasModeloElegir = (model: AccountsModel) =>
  invoke<void>('cuentas_modelo_elegir', { model });

export type EcpnFila =
  | 'SaldoInicial'
  | 'IngresosGastos'
  | 'AumentoCapital'
  | 'ReduccionCapital'
  | 'Dividendos'
  | 'ParticipacionesPropias'
  | 'OtrasVariaciones'
  | 'SaldoFinal';
/** Estado de cambios en el patrimonio neto. */
export interface Ecpn {
  year: number;
  model: AccountsModel;
  obligatorio: boolean;
  nota: string;
  ingresos_gastos: { label: string; amount: number }[];
  columnas: string[];
  filas: { fila: EcpnFila; label: string; importes: number[]; total: number }[];
}
export const ecpn = (year: number, model: AccountsModel) => invoke<Ecpn>('ecpn', { year, model });
/** Pide dónde guardar el ECPN (PDF o RTF) y lo guarda. */
export async function ecpnSave(
  year: number,
  pdf: boolean,
  model: AccountsModel,
): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: `ECPN ${year}.${pdf ? 'pdf' : 'rtf'}`,
    filters: [pdf ? { name: 'PDF', extensions: ['pdf'] } : { name: 'RTF', extensions: ['rtf'] }],
  });
  if (!path) return null;
  await invoke('ecpn_save', { year, path, pdf, model });
  return path;
}

// ---------- Dividendos, administradores y gastos de viaje ----------

/** Socio de la sociedad (reparto de dividendos y modelos 123 y 193). */
export interface Shareholder {
  id: number;
  nif: string;
  name: string;
  address: string;
  /** Provincia del domicilio (01-53); vacía si no es residente. */
  province: string;
  /** País de residencia (ISO, 2 letras). */
  country: string;
  /** Participación en millonésimas (100 % = 1.000.000). */
  pct: number;
  legal_person: boolean;
  resident: boolean;
  /** Sociedad con derecho a la exención del art. 21 LIS: sin retención (art. 128.4.d LIS). */
  exempt: boolean;
  active: boolean;
}

export interface PayoutInput {
  /** Fecha de exigibilidad (la de pago acordada por la junta). */
  date: string;
  gross: number;
  /** 57x o 551. */
  treasury: string;
  /** Acuerdo de reparto que se paga (si no, el más antiguo pendiente). */
  agreement?: number | null;
}

export interface PayoutLine {
  shareholder_id: number | null;
  nif: string;
  name: string;
  province: string;
  legal_person: boolean;
  resident: boolean;
  pct: number;
  gross: number;
  /** Tipo de retención en centésimas de punto (1900 = 19 %). */
  rate: number;
  withheld: number;
  net: number;
}

export interface ProposedLine {
  account: string;
  name: string;
  debit: number;
  credit: number;
}

export interface PayoutProposal {
  date: string;
  gross: number;
  /** Pendiente de pago en la 526 ese día. */
  available: number;
  lines: PayoutLine[];
  withheld: number;
  net: number;
  entry: ProposedLine[];
  /** Acuerdo de reparto que se paga. */
  agreement?: DividendAgreement | null;
  warnings: string[];
}

export interface DividendPayment {
  id: number;
  date: string;
  gross: number;
  withheld: number;
  net: number;
  treasury: string;
  entry_id: number | null;
  agreement_id?: number | null;
  lines: PayoutLine[];
}

/** Clase de acuerdo de dividendos. */
export type DividendAgreementKind = 'a_cuenta' | 'reservas' | 'resultado';

export const DIVIDEND_AGREEMENT_KINDS: Record<DividendAgreementKind, string> = {
  a_cuenta: 'A cuenta del resultado del ejercicio (art. 277 LSC)',
  reservas: 'Con cargo a reservas voluntarias o remanente (art. 273.2 LSC)',
  resultado: 'Con cargo al resultado del ejercicio cerrado (aplicación del resultado)',
};

export interface DividendAgreementInput {
  kind: DividendAgreementKind;
  /** Fecha del acuerdo. */
  date: string;
  amount: number;
  payment_date: string | null;
  /** A cuenta: fecha del estado contable de liquidez. */
  statement_date: string | null;
  /** A cuenta: tipo del IS estimado, en centésimas de punto (2500 = 25 %). */
  tax_rate: number | null;
  /** Con cargo a reservas: 113 o 120. */
  reserve_account: string | null;
  /** Con cargo al resultado: el ejercicio. */
  year: number | null;
  organ: MinuteKind | null;
}

/** Una comprobación legal del semáforo. */
export interface DividendCheck {
  code: string;
  title: string;
  light: 'verde' | 'ambar' | 'rojo';
  detail: string;
  norm: string;
}

export interface DividendAgreementCheck {
  kind: DividendAgreementKind;
  date: string;
  amount: number;
  account: string;
  checks: DividendCheck[];
  /** Algo en rojo: no se deja contabilizar. */
  blocked: boolean;
  liquidity: LiquidityStatement | null;
  entry: ProposedLine[];
  lines: PayoutLine[];
  withheld: number;
  organ: MinuteKind;
  warnings: string[];
}

export interface DividendAgreement {
  id: number;
  kind: DividendAgreementKind;
  date: string;
  amount: number;
  payment_date: string | null;
  account: string;
  allocation_year: number | null;
  entry_id: number | null;
  minute_id: number | null;
  minute_number: number | null;
  minute_approved: boolean;
  paid: number;
  pending: number;
}

export interface DividendAgreementRecorded {
  id: number;
  entry_id: number | null;
  minute_id: number;
}

export const shareholdersList = () => invoke<Shareholder[]>('shareholders_list');
export const shareholderSave = (shareholder: Shareholder) =>
  invoke<number>('shareholder_save', { shareholder });
export const shareholderDelete = (id: number) => invoke<void>('shareholder_delete', { id });
export const dividendPropose = (input: PayoutInput) =>
  invoke<PayoutProposal>('dividend_propose', { input });
export const dividendPay = (input: PayoutInput) =>
  invoke<number>('dividend_pay', { input, confirmed: true });
export const dividendPayments = (year: number) =>
  invoke<DividendPayment[]>('dividend_payments', { year });
export const dividendUndo = (id: number) => invoke<void>('dividend_undo', { id, confirmed: true });
export const dividendAgreementCheck = (input: DividendAgreementInput) =>
  invoke<DividendAgreementCheck>('dividend_agreement_check', { input });
export const dividendAgreementRecord = (input: DividendAgreementInput) =>
  invoke<DividendAgreementRecorded>('dividend_agreement_record', { input, confirmed: true });
export const dividendAgreements = () => invoke<DividendAgreement[]>('dividend_agreements');
export const dividendAgreementUndo = (id: number) =>
  invoke<void>('dividend_agreement_undo', { id, confirmed: true });

// ---------- Libros societarios (espejo de `taxpain_app::libros_societarios`) ----------

/** Movimiento del libro registro de socios (arts. 104-106 LSC). */
export type MovementKind =
  | 'constitucion'
  | 'ampliacion'
  | 'reduccion'
  | 'transmision'
  | 'carga'
  | 'cancelacion_carga';

export const MOVEMENT_KINDS: Record<MovementKind, string> = {
  constitucion: 'Constitución (titularidad originaria)',
  ampliacion: 'Ampliación de capital',
  reduccion: 'Reducción de capital (amortización)',
  transmision: 'Transmisión',
  carga: 'Constitución de carga o gravamen',
  cancelacion_carga: 'Cancelación de carga',
};

export interface ShareMovement {
  id: number;
  date: string;
  kind: MovementKind;
  /** Transmitente, o titular de las participaciones que se gravan, amortizan o liberan. */
  from_nif: string;
  from_name: string;
  /** Suscriptor, adquirente o beneficiario de la carga. */
  to_nif: string;
  to_name: string;
  to_address: string;
  to_nationality: string;
  number_from: number;
  number_to: number;
  /** Nominal de cada participación, en céntimos. */
  unit_nominal: number;
  price: number | null;
  charge: string;
  title: string;
  notary: string;
  protocol: string;
  title_date: string | null;
  notes: string;
}
export interface ShareCharge {
  charge: string;
  beneficiary_nif: string;
  beneficiary_name: string;
  date: string;
}
export interface ShareBlock {
  from: number;
  to: number;
  unit_nominal: number;
  charges: ShareCharge[];
}
export interface ShareHolding {
  nif: string;
  name: string;
  address: string;
  nationality: string;
  blocks: ShareBlock[];
  count: number;
  nominal: number;
  /** Millonésimas (100 % = 1.000.000). */
  pct: number;
}
export interface PartnersRegister {
  movements: ShareMovement[];
  situation: { count: number; capital: number; holders: ShareHolding[] };
  checks: string[];
  in_sync: boolean;
  capital_books: number | null;
}
export const partnersRegister = () => invoke<PartnersRegister>('partners_register');
export const shareMovementSave = (movement: ShareMovement) =>
  invoke<number>('share_movement_save', { movement });
export const shareMovementDelete = (id: number) =>
  invoke<void>('share_movement_delete', { id, confirmed: true });
export const partnersRegisterApply = () =>
  invoke<number>('partners_register_apply', { confirmed: true });

/** Contrato del socio único con la sociedad (art. 16 LSC). */
export interface SoleContract {
  id: number;
  date: string;
  object: string;
  amount: number | null;
  conditions: string;
  document: string;
  source: string | null;
}
export interface SoleContractProposals {
  /** [NIF, nombre] del socio único; null si la sociedad no es unipersonal. */
  sole_partner: [string, string] | null;
  proposals: SoleContract[];
}
export const soleContracts = () => invoke<SoleContract[]>('sole_contracts');
export const soleContractProposals = (year: number) =>
  invoke<SoleContractProposals>('sole_contract_proposals', { year });
export const soleContractSave = (contract: SoleContract) =>
  invoke<number>('sole_contract_save', { contract });
export const soleContractDelete = (id: number) =>
  invoke<void>('sole_contract_delete', { id, confirmed: true });

export type MinuteKind =
  | 'junta_ordinaria'
  | 'junta_extraordinaria'
  | 'junta_universal'
  | 'socio_unico'
  | 'consejo'
  | 'administrador';

export const MINUTE_KINDS: Record<MinuteKind, string> = {
  junta_ordinaria: 'Junta general ordinaria',
  junta_extraordinaria: 'Junta general extraordinaria',
  junta_universal: 'Junta general universal',
  socio_unico: 'Decisión del socio único',
  consejo: 'Consejo de administración',
  administrador: 'Decisión del órgano de administración',
};

export interface Minute {
  id: number;
  number: number | null;
  fiscal_year: number;
  date: string;
  place: string;
  kind: MinuteKind;
  template: string;
  attendees: string;
  chair: string;
  secretary: string;
  agreements: string;
  status: 'borrador' | 'aprobada';
  approved_at: string | null;
  /** Acuerdo de dividendos que documenta (lo pone TaxPain). */
  dividend_agreement?: number | null;
}

/** Plantilla de acta y sus datos (importes en céntimos; tipos en centésimas de punto). */
export type MinuteTemplate =
  | { template: 'cuentas'; year: number }
  | { template: 'dividendo'; amount: number; payment_date: string | null }
  | {
      template: 'dividendo_cuenta';
      amount: number;
      payment_date: string | null;
      statement_date: string;
      tax_rate: number | null;
    }
  | { template: 'nombramiento'; name: string; nif: string; address: string; position: string }
  | { template: 'cese'; name: string; nif: string; position: string }
  | { template: 'retribucion'; year: number; max_amount: number; concept: string }
  | { template: 'domicilio'; address: string }
  | { template: 'libre' };

/** Estado contable de liquidez del dividendo a cuenta (art. 277 LSC). */
export interface LiquidityStatement {
  date: string;
  period_start: string;
  result: number;
  prior_losses: number;
  legal_reserve: number;
  tax_rate: number;
  estimated_tax: number;
  already_distributed: number;
  limit: number;
  treasury: number;
  short_term_debts: number;
  amount: number;
  treasury_after: number;
}
export interface MinuteDraft {
  minute: Minute;
  warnings: string[];
  liquidity: LiquidityStatement | null;
}
export interface CertificateInput {
  name: string;
  position: string;
  place: string;
  date: string;
}
export type CorporateBook = 'actas' | 'socios' | 'contratos';

export const minutesList = (year: number) => invoke<Minute[]>('minutes_list', { year });
export const minuteTemplate = (template: MinuteTemplate, date: string) =>
  invoke<MinuteDraft>('minute_template', { template, date });
export const minuteSave = (minute: Minute) => invoke<number>('minute_save', { minute });
export const minuteDelete = (id: number) => invoke<void>('minute_delete', { id, confirmed: true });
export const minuteApprove = (id: number) =>
  invoke<number>('minute_approve', { id, confirmed: true });
export const minuteReopen = (id: number) => invoke<void>('minute_reopen', { id, confirmed: true });

async function savePath(name: string, pdf: boolean): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: `${name}.${pdf ? 'pdf' : 'rtf'}`,
    filters: [pdf ? { name: 'PDF', extensions: ['pdf'] } : { name: 'RTF', extensions: ['rtf'] }],
  });
  return path ?? null;
}

/** Guarda un acta suelta; devuelve la ruta o null si se cancela. */
export async function minuteDocumentSave(
  id: number,
  name: string,
  pdf: boolean,
): Promise<string | null> {
  const path = await savePath(name, pdf);
  if (!path) return null;
  await invoke('minute_document_save', { id, path, pdf });
  return path;
}

/** Guarda la certificación de los acuerdos de un acta aprobada. */
export async function minuteCertificateSave(
  id: number,
  certificate: CertificateInput,
  name: string,
  pdf: boolean,
): Promise<string | null> {
  const path = await savePath(name, pdf);
  if (!path) return null;
  await invoke('minute_certificate_save', { id, certificate, path, pdf });
  return path;
}

/** Guarda uno de los tres libros del ejercicio (PDF con folios o RTF). */
export async function corporateBookSave(
  book: CorporateBook,
  year: number | null,
  name: string,
  pdf: boolean,
): Promise<string | null> {
  const path = await savePath(name, pdf);
  if (!path) return null;
  await invoke('corporate_book_save', { book, year, path, pdf });
  return path;
}

/** Régimen de Seguridad Social de un administrador (decide la subclave E del 190). */
export type DirectorSs = 'Asimilado' | 'Autonomo' | 'Ninguno';

/** De dónde sale la cifra de negocios: el 200 presentado en la AEAT, uno guardado en TaxPain o la contabilidad. */
export type TurnoverSource = 'aeat' | 'guardado' | 'contabilidad';

export interface DirectorTurnover {
  /** Último ejercicio cerrado antes del pago. */
  year: number;
  turnover: number | null;
  /** Cifra de negocios inferior a 100.000 € (null: TaxPain no la sabe). */
  small: boolean | null;
  source: TurnoverSource;
}

/** «según …» para decir de dónde sale la cifra de negocios. */
export const TURNOVER_SOURCE: Record<TurnoverSource, string> = {
  aeat: 'según el 200 presentado en la AEAT',
  guardado: 'según el 200 guardado en TaxPain',
  contabilidad: 'según la contabilidad de TaxPain',
};

export const directorTurnover = (date: string) =>
  invoke<DirectorTurnover>('director_turnover', { date });

/** Subclave E del 190 (01-04) según el régimen y la cifra de negocios. */
export function directorSubkey(ss: DirectorSs, small: boolean): number {
  if (ss === 'Asimilado') return small ? 2 : 1;
  return ss === 'Autonomo' && small ? 3 : 4;
}

export interface Trip {
  date: string;
  employee_id: number | null;
  nif: string;
  name: string;
  /** Relación laboral con la sociedad (sin ella no se aplica el art. 9 RIRPF). */
  labor: boolean;
  origin: string;
  destination: string;
  reason: string;
  abroad: boolean;
  overnight: boolean;
  days: number;
  km: number;
  km_amount: number;
  meals: number;
  lodging: number;
  tolls_parking: number;
  transport: number;
  /** 551, 465 o 57x. */
  counter_account: string;
}

export interface TripCalc {
  total: number;
  /** Exceptuado (clave L.01 del 190). */
  exempt: number;
  /** Gastos por cuenta de la sociedad que no son renta. */
  not_income: number;
  /** Retribución sujeta a retención: va en la nómina. */
  taxable: number;
  km_limit: number;
  meals_limit: number;
  posted: number;
  warnings: string[];
}

export interface StoredTrip {
  id: number;
  trip: Trip;
  exempt: number;
  taxable: number;
  entry_id: number | null;
}

export const travelPreview = (trip: Trip) => invoke<TripCalc>('travel_preview', { trip });
export const travelRegister = (trip: Trip) => invoke<number>('travel_register', { trip });
export const travelList = (year: number) => invoke<StoredTrip[]>('travel_list', { year });
export const travelDelete = (id: number) => invoke<void>('travel_delete', { id, confirmed: true });

// ---------- Moneda extranjera ----------

/** Espejo de `taxpain_core::fx::ForeignCurrency`: 1 € = `rate` unidades de la moneda. */
export interface ForeignCurrency {
  code: string;
  /** Decimal con punto, como lo publica el BCE («1.1193»). */
  rate: string;
  /** Fecha del tipo (la de su publicación). */
  rate_date: string;
  source: 'bce' | 'manual';
}

/** Factura recibida en moneda extranjera: importes en la moneda (céntimos). */
export interface ReceivedFx {
  currency: ForeignCurrency;
  base: number;
  vat: number;
  withholding: number;
  total: number;
  /** IVA en euros que figura en la factura (si no, la base en euros por el tipo de IVA). */
  vat_eur?: number | null;
}

/** Tipo de referencia del BCE descargado con «Tipo del BCE». */
export interface BceRate {
  currency: string;
  requested: string;
  /** Día de publicación (el pedido o el último hábil anterior). */
  date: string;
  rate: string;
  cached: boolean;
}

/** Monedas admitidas (las de `taxpain_core::fx::CURRENCIES`); `ecb`: el BCE publica su tipo. */
export const CURRENCIES: { code: string; name: string; ecb: boolean }[] = [
  { code: 'USD', name: 'Dólar estadounidense', ecb: true },
  { code: 'GBP', name: 'Libra esterlina', ecb: true },
  { code: 'CHF', name: 'Franco suizo', ecb: true },
  { code: 'CAD', name: 'Dólar canadiense', ecb: true },
  { code: 'AUD', name: 'Dólar australiano', ecb: true },
  { code: 'NZD', name: 'Dólar neozelandés', ecb: true },
  { code: 'SEK', name: 'Corona sueca', ecb: true },
  { code: 'NOK', name: 'Corona noruega', ecb: true },
  { code: 'DKK', name: 'Corona danesa', ecb: true },
  { code: 'PLN', name: 'Esloti polaco', ecb: true },
  { code: 'CZK', name: 'Corona checa', ecb: true },
  { code: 'HUF', name: 'Forinto húngaro', ecb: true },
  { code: 'RON', name: 'Leu rumano', ecb: true },
  { code: 'TRY', name: 'Lira turca', ecb: true },
  { code: 'MXN', name: 'Peso mexicano', ecb: true },
  { code: 'BRL', name: 'Real brasileño', ecb: true },
  { code: 'CNY', name: 'Yuan chino', ecb: true },
  { code: 'HKD', name: 'Dólar de Hong Kong', ecb: true },
  { code: 'SGD', name: 'Dólar de Singapur', ecb: true },
  { code: 'INR', name: 'Rupia india', ecb: true },
  { code: 'ILS', name: 'Nuevo séquel israelí', ecb: true },
  { code: 'ZAR', name: 'Rand sudafricano', ecb: true },
  { code: 'THB', name: 'Bat tailandés', ecb: true },
  { code: 'MYR', name: 'Ringit malayo', ecb: true },
  { code: 'PHP', name: 'Peso filipino', ecb: true },
  { code: 'IDR', name: 'Rupia indonesia', ecb: true },
  { code: 'AED', name: 'Dírham de los EAU', ecb: false },
  { code: 'SAR', name: 'Riyal saudí', ecb: false },
  { code: 'MAD', name: 'Dírham marroquí', ecb: false },
  { code: 'COP', name: 'Peso colombiano', ecb: false },
  { code: 'PEN', name: 'Sol peruano', ecb: false },
];

/** Descarga (sólo al pedirlo) el tipo de referencia del BCE vigente el `date`. */
export const fxBceRate = (currency: string, date: string) =>
  invoke<BceRate>('fx_bce_rate', { currency, date });

/**
 * Tipo escrito a mano → millonésimas (entero), o null. Acepta «1,1193», «1.1193» y
 * «20.268,06» (miles con punto si también hay coma); como mucho 6 decimales.
 */
export function parseRate(s: string): number | null {
  const t = s.trim();
  let int: string;
  let frac: string;
  if (t.includes(',') && t.includes('.')) {
    const [i = '', f = ''] = t.split(',');
    int = i.replace(/\./g, '');
    frac = f;
  } else {
    const m = /^(\d*)(?:[.,](\d*))?$/.exec(t);
    if (!m) return null;
    int = m[1] ?? '';
    frac = m[2] ?? '';
  }
  if (!/^\d*$/.test(int) || !/^\d*$/.test(frac) || frac.length > 6 || int.length > 12) {
    return null;
  }
  const micros = Number(int || '0') * 1_000_000 + Number(frac.padEnd(6, '0') || '0');
  return micros > 0 && Number.isSafeInteger(micros) ? micros : null;
}

/** Millonésimas → «1.1193» (para el proceso nativo). */
export function rateDecimal(micros: number): string {
  const int = Math.floor(micros / 1_000_000);
  const frac = String(micros % 1_000_000)
    .padStart(6, '0')
    .replace(/0+$/, '');
  return frac ? `${int}.${frac}` : String(int);
}

/** Millonésimas → «1,1193» (para mostrar). */
export const rateText = (micros: number) => rateDecimal(micros).replace('.', ',');

/** Importe en la moneda (céntimos) → euros al tipo, como el proceso nativo: ÷ tipo, al céntimo
 * y la mitad alejándose de cero. Exacto (enteros grandes). */
export function toEur(cents: number, micros: number): number {
  const num = BigInt(cents) * 1_000_000n;
  const den = BigInt(micros);
  const neg = num < 0n;
  const abs = neg ? -num : num;
  let q = abs / den;
  if ((abs % den) * 2n >= den) q += 1n;
  return Number(neg ? -q : q);
}

/** «1.234,56 €» en euros o «1.234,56 USD» en otra moneda. */
export function money(cents: number, code?: string | null): string {
  return code && code !== 'EUR' ? euros(cents).replace(/ €$/, ` ${code}`) : euros(cents);
}

/** Aviso si el tipo no es el vigente al devengo `accrual` (como lo comprueba la emisión): ni
 * posterior ni más de 7 días anterior (fines de semana y festivos sin publicación). */
export function rateDateWarning(
  rateDate: string,
  accrual: string,
  oldRateOk = false,
): string | null {
  if (!rateDate || !accrual) return null;
  const d = daysBetween(rateDate, accrual);
  if (d < 0) return 'El tipo es posterior a la fecha de la operación: usa el vigente ese día.';
  if (d > 7 && !oldRateOk)
    return `El tipo es del ${formatDate(rateDate)}: usa el vigente a la fecha de la operación («Tipo del BCE»).`;
  return null;
}

// ---------- Moneda extranjera (cuentas en divisa y valoración al cierre) ----------

/** Euros de un movimiento de una cuenta en divisa. */
export interface Contravalor {
  amount_eur: number;
  rate: string;
  date: string;
  source: 'bce' | 'manual' | 'traspaso';
  stored: boolean;
}

/** Fija a mano los euros (o el tipo) de un movimiento en divisa; con los dos a `null`, lo quita. */
export const bankSetFx = (tx: number, eur: number | null, rate: string | null) =>
  invoke<Contravalor | null>('bank_set_fx', { tx, eur, rate });

/** Saldo de una cuenta del banco en su divisa y en euros. */
export interface BankBalance {
  name: string;
  account: string;
  currency: string;
  kind: string;
  /** En la divisa de la cuenta, según el extracto. */
  saldo: number | null;
  /** Saldo contable de la subcuenta (euros). */
  contable: number;
  /** Último tipo del BCE descargado (cuentas en divisa). */
  tipo: string | null;
  tipo_fecha: string | null;
  al_tipo: number | null;
  /** Movimientos sin contravalor (falta el tipo de su día). */
  sin_contravalor: number;
}

export const bankBalances = () => invoke<BankBalance[]>('bank_balances');

/** Lo tecleado para la valoración: tipos de cierre por divisa y saldos en divisa por subcuenta. */
export interface FxValuationInput {
  tipos: Record<string, string>;
  saldos: Record<string, number>;
}

export interface FxClosingRate {
  currency: string;
  rate: string | null;
  date: string | null;
  source: 'bce' | 'manual' | null;
}

export interface FxValuationItem {
  kind: 'banco' | 'cliente' | 'proveedor';
  account: string;
  label: string;
  currency: string;
  amount: number;
  amount_source: 'extracto' | 'tecleado' | 'factura';
  booked: number;
  valued: number | null;
  /** Positiva a favor (768), negativa en contra (668). */
  difference: number | null;
}

/** Valoración al cierre de las partidas monetarias en divisa (NRV 11.ª). */
export interface FxValuation {
  year: number;
  date: string;
  reversal_date: string;
  closed: boolean;
  rates: FxClosingRate[];
  items: FxValuationItem[];
  lines: ProposedLine[];
  gains: number;
  losses: number;
  entry_id: number | null;
  reversal_id: number | null;
  stale: boolean;
  input: FxValuationInput;
  warnings: string[];
}

export const closingFxValuation = (year: number, input?: FxValuationInput) =>
  invoke<FxValuation>('closing_fx_valuation', { year, input: input ?? null });
export const closingFxValuationPost = (year: number, input: FxValuationInput) =>
  invoke<number>('closing_fx_valuation_post', { year, input, confirmed: true });
export const closingFxValuationUndo = (year: number) =>
  invoke<void>('closing_fx_valuation_undo', { year, confirmed: true });

/** Espejo de `taxpain_app::censo` (modelo 036: cotejo del censo y asistente de cambios). */
export interface CensoTecleado {
  fecha: string | null;
  obligaciones: string[];
  epigrafes: string[];
  roi: boolean | null;
  sii: boolean | null;
  criterio_caja: boolean | null;
  domicilio_fiscal: string | null;
}
export interface SituacionCensal {
  origen: 'tecleado' | 'certificado' | 'ninguno';
  documento: string | null;
  fecha: string | null;
  obligaciones: string[];
  lineas: string[];
  epigrafes: string[];
  roi: boolean | null;
  sii: boolean | null;
  criterio_caja: boolean | null;
  domicilio_fiscal: string | null;
}
export interface FilaCenso {
  codigo: string;
  nombre: string;
  censo: boolean | null;
  perfil: boolean | null;
  contabilidad: string | null;
  presentado: number[];
}
export interface DiferenciaCenso {
  clave: string;
  gravedad: 'alta' | 'media' | 'info';
  titulo: string;
  detalle: string;
  que_hacer: string;
  supuesto: string | null;
}
export interface ComunicacionCenso {
  id: number;
  supuesto: string;
  titulo: string;
  fecha_hecho: string | null;
  vence: string;
  nota: string;
  creada: string;
  hecha: string | null;
}
export interface SupuestoCenso {
  id: string;
  titulo: string;
  grupo: string;
}
export interface CotejoCenso {
  hoy: string;
  situacion: SituacionCensal;
  tecleado: CensoTecleado | null;
  epigrafes_actividad: string[];
  filas: FilaCenso[];
  diferencias: DiferenciaCenso[];
  comunicaciones: ComunicacionCenso[];
  supuestos: SupuestoCenso[];
}
export interface GuiaCenso {
  id: string;
  titulo: string;
  resumen: string;
  por_036: boolean;
  causa: string;
  pasos: { apartado: string; que_hacer: string }[];
  plazo: {
    tipo: 'un_mes' | 'antes' | 'ventana' | 'en303';
    texto: string;
    desde: string | null;
    vence: string | null;
    pide_fecha: boolean;
    en_calendario: string | null;
  };
  documentacion: string[];
  norma: string;
  avisos: string[];
  sede: string | null;
  verificar: string;
}
export const censoCotejo = () => invoke<CotejoCenso>('censo_cotejo');
export const censoGuardar = (tecleado: CensoTecleado | null, epigrafesActividad: string[]) =>
  invoke<void>('censo_guardar', { tecleado, epigrafesActividad });
export const censoGuia = (id: string, fecha: string | null) =>
  invoke<GuiaCenso>('censo_guia', { id, fecha });
export const censoProgramar = (id: string, fecha: string | null, nota: string) =>
  invoke<ComunicacionCenso>('censo_programar', { id, fecha, nota });
export const censoPresentada = (id: number) => invoke<void>('censo_presentada', { id });
export const censoQuitar = (id: number, confirmed: boolean) =>
  invoke<void>('censo_quitar', { id, confirmed });
// ---------- Expedientes: requerimientos, liquidaciones, sanciones y recursos (G26, G27) ----------
// Espejo de `taxpain_app::expedientes`. Nada se presenta ni se envía.

export type ExpedienteOrganismo =
  | 'aeat'
  | 'dehu'
  | 'seguridad_social'
  | 'autonomica'
  | 'ayuntamiento'
  | 'otro';
export type ExpedienteTipo =
  | 'requerimiento'
  | 'propuesta_liquidacion'
  | 'audiencia'
  | 'propuesta_sancion'
  | 'liquidacion'
  | 'acuerdo_sancionador'
  | 'providencia_apremio'
  | 'diligencia_embargo'
  | 'otro';
export type ExpedienteEstado = 'abierto' | 'contestado' | 'recurrido' | 'cerrado';

export interface DocAportar {
  descripcion: string;
  listo: boolean;
}

export interface Expediente {
  id: number;
  organismo: ExpedienteOrganismo;
  organo: string;
  referencia: string;
  tipo: ExpedienteTipo;
  asunto: string;
  concepto: string;
  puesto_el: string | null;
  notificado_el: string | null;
  plazo_dias: number | null;
  vence_manual: string | null;
  /** Céntimos. */
  importe: number | null;
  /** Sanción sin reducciones, en céntimos. */
  sancion: number | null;
  fin_voluntario: string | null;
  estado: ExpedienteEstado;
  notice_id: string | null;
  document_id: number | null;
  documentos: DocAportar[];
  notas: string;
}

export interface ExpedientePlazo {
  clave: string;
  titulo: string;
  vence: string;
  computo: string;
  norma: string;
  manual: boolean;
}

export interface EscenarioRecargo {
  hasta: string | null;
  texto: string;
  porcentaje: number;
  recargo: number;
  intereses: number;
  total: number;
  hoy: boolean;
}

export interface ReduccionesSancion {
  sancion: number;
  conformidad: number;
  pronto_pago: number;
  ambas: number;
  notas: string[];
}

export interface ExpedienteCalculo {
  notificado: string | null;
  presunta: boolean;
  plazos: ExpedientePlazo[];
  proximo: ExpedientePlazo | null;
  dias: number | null;
  vencido: ExpedientePlazo | null;
  festivos_completos: boolean;
  recargo: EscenarioRecargo[];
  reducciones: ReduccionesSancion | null;
  avisos: string[];
}

export type TipoActuacion =
  | 'contestacion'
  | 'alegaciones'
  | 'recurso_reposicion'
  | 'reclamacion_tea'
  | 'aplazamiento'
  | 'ampliacion'
  | 'pago'
  | 'otro';

export interface ExpedienteActuacion {
  id: number;
  fecha: string;
  tipo: TipoActuacion;
  registro: string;
  descripcion: string;
}

export interface ExpedienteVista {
  expediente: Expediente;
  actuaciones: ExpedienteActuacion[];
  calculo: ExpedienteCalculo;
}

export type EscritoTipo =
  | 'contestacion'
  | 'alegaciones'
  | 'recurso_reposicion'
  | 'aplazamiento'
  | 'ampliacion';

export interface EscritoOpciones {
  motivo: string;
  plazos: number | null;
  lugar: string;
}

export interface AvisoExpediente {
  texto: string;
  fecha: string;
  dias: number;
  urgente: boolean;
}

export type TipoCertificadoAdmin =
  | 'corriente_aeat'
  | 'corriente_ss'
  | 'residencia_fiscal'
  | 'situacion_censal'
  | 'apoderamiento'
  | 'otro';

export interface CertificadoAdmin {
  id: number;
  tipo: TipoCertificadoAdmin;
  descripcion: string;
  emitido_el: string | null;
  caduca_el: string | null;
  document_id: number | null;
  notas: string;
}

export interface CertificadoAdminVista {
  certificado: CertificadoAdmin;
  dias: number | null;
  estado: 'vigente' | 'caduca_pronto' | 'caducado';
}

export const expedientesList = () => invoke<ExpedienteVista[]>('expedientes_list');
export const expedientePreview = (expediente: Expediente) =>
  invoke<ExpedienteCalculo>('expediente_preview', { expediente });
export const expedienteCreate = (expediente: Expediente) =>
  invoke<number>('expediente_create', { expediente });
export const expedienteUpdate = (id: number, expediente: Expediente) =>
  invoke<null>('expediente_update', { id, expediente });
export const expedienteDelete = (id: number, confirmed: boolean) =>
  invoke<null>('expediente_delete', { id, confirmed });
export const expedienteDesdeNotificacion = (noticeId: string) =>
  invoke<Expediente>('expediente_desde_notificacion', { noticeId });
export const expedienteDesdeDocumento = (documentId: number) =>
  invoke<Expediente>('expediente_desde_documento', { documentId });
export const expedienteActuacionAdd = (id: number, actuacion: ExpedienteActuacion) =>
  invoke<number>('expediente_actuacion_add', { id, actuacion });
export const expedienteActuacionDelete = (actuacion: number, confirmed: boolean) =>
  invoke<null>('expediente_actuacion_delete', { actuacion, confirmed });
export const expedientesAvisos = () => invoke<AvisoExpediente[]>('expedientes_avisos');
export const certificadosAdminList = () =>
  invoke<CertificadoAdminVista[]>('certificados_admin_list');
export const certificadoAdminSave = (certificado: CertificadoAdmin) =>
  invoke<number>('certificado_admin_save', { certificado });
export const certificadoAdminDesdeDocumento = (documentId: number) =>
  invoke<CertificadoAdmin>('certificado_admin_desde_documento', { documentId });
export const certificadoAdminDelete = (id: number, confirmed: boolean) =>
  invoke<null>('certificado_admin_delete', { id, confirmed });

/** Pide dónde guardar el borrador del escrito (RTF o PDF) y lo guarda. Nada se presenta. */
export async function expedienteEscritoSave(
  id: number,
  escrito: EscritoTipo,
  opciones: EscritoOpciones,
  pdf: boolean,
): Promise<string | null> {
  const nombre = await invoke<string>('expediente_escrito_nombre', { id, escrito, pdf });
  if (demoInvoke) {
    await invoke('expediente_escrito_save', { id, escrito, opciones, path: nombre });
    return `~/Documents/${nombre}`;
  }
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: nombre,
    filters: pdf ? [{ name: 'PDF', extensions: ['pdf'] }] : [{ name: 'RTF', extensions: ['rtf'] }],
  });
  if (!path) return null;
  await invoke('expediente_escrito_save', { id, escrito, opciones, path });
  return path;
}

// ---------- Modelos 720 y 721 (bienes y monedas virtuales en el extranjero) ----------

export type OrigenBienExtranjero = 'banco' | 'valores' | 'cripto';
export type CondicionDeclarante =
  | 'titular'
  | 'representante'
  | 'autorizado'
  | 'beneficiario'
  | 'usufructuario'
  | 'tomador'
  | 'poder_disposicion'
  | 'titular_real'
  | 'otra';
export type BloqueExtranjero = 'cuentas' | 'valores' | 'monedas';
export interface DatosBienExtranjero {
  origen: OrigenBienExtranjero;
  ref_id: number;
  condicion: CondicionDeclarante;
  porcentaje_bp: number;
  fecha_apertura: string | null;
  fecha_extincion: string | null;
  entidad: string;
  nif_entidad: string;
  bic: string;
  domicilio: string;
  pais: string;
  extranjero: boolean | null;
}
export interface RegistroExtranjero {
  origen: OrigenBienExtranjero;
  ref_id: number;
  bloque: BloqueExtranjero;
  clave: 'C' | 'V' | 'I' | 'M';
  descripcion: string;
  identificacion: string;
  entidad: string;
  nif_entidad: string;
  bic: string;
  domicilio: string;
  pais: string;
  condicion: CondicionDeclarante;
  porcentaje_bp: number;
  fecha_apertura: string | null;
  fecha_estimada: boolean;
  fecha_extincion: string | null;
  saldo: number;
  saldo_medio: number | null;
  unidades: string | null;
  moneda: string;
  alta: 'A' | 'M' | 'C';
  faltan: string[];
}
export interface DeclaradoExtranjero {
  id: number;
  modelo: string;
  year: number;
  bloque: BloqueExtranjero;
  valor: number;
  saldo_medio: number | null;
  bienes: string[];
  origen: string;
  creado: string;
}
export interface BloqueExtranjeroView {
  bloque: BloqueExtranjero;
  titulo: string;
  total: number;
  total_medio: number | null;
  supera: boolean;
  anterior: DeclaradoExtranjero | null;
  incremento: number | null;
  incremento_medio: number | null;
  extinguidos: string[];
  obligado: boolean;
  motivo: string;
  registros: RegistroExtranjero[];
}
export interface BienExtranjeroView {
  nombre: string;
  extranjero: boolean;
  iban: string;
  datos: DatosBienExtranjero;
}
export interface BorradorExtranjero {
  modelo: string;
  year: number;
  fecha: string;
  plazo: string;
  nif: string;
  nombre: string;
  persona_fisica: boolean;
  bloques: BloqueExtranjeroView[];
  obligado: boolean;
  exenta_por_contabilidad: boolean;
  requisitos: string[];
  presentados_aeat: number[];
  declarados: DeclaradoExtranjero[];
  bienes: BienExtranjeroView[];
  fichero: { disponible: boolean; nota: string };
  avisos: string[];
  notas: string[];
  norma: string;
}
export const extranjeroBorrador = (modelo: string, year: number) =>
  invoke<BorradorExtranjero>('extranjero_borrador', { modelo, year });
export const extranjeroGuardarBien = (datos: DatosBienExtranjero) =>
  invoke<void>('extranjero_guardar_bien', { datos });
export const extranjeroGuardarDeclarado = (declarado: DeclaradoExtranjero) =>
  invoke<void>('extranjero_guardar_declarado', { declarado });
export const extranjeroQuitarDeclarado = (id: number, confirmed: boolean) =>
  invoke<void>('extranjero_quitar_declarado', { id, confirmed });
export const extranjeroMarcarPresentado = (modelo: string, year: number, confirmed: boolean) =>
  invoke<DeclaradoExtranjero[]>('extranjero_marcar_presentado', { modelo, year, confirmed });
export const extranjeroExportar = (modelo: string, year: number, dir: string) =>
  invoke<string>('extranjero_exportar', { modelo, year, dir });
// ---------- Empresa que no abre, auditoría, periodos cerrados y archivo (G46, G38) ----------

export interface CopiaEncontrada {
  ruta: string;
  nombre: string;
  bytes: number;
  modificada: string | null;
}

export type TipoFallo =
  | 'sin_clave'
  | 'llavero_denegado'
  | 'danada'
  | 'version_nueva'
  | 'migracion'
  | 'auditoria_alterada'
  | 'no_esta'
  | 'otro';

/** Qué le pasa a una empresa que no abre y qué se puede hacer. */
export interface Diagnostico {
  id: string;
  empresa: string | null;
  tipo: TipoFallo;
  titulo: string;
  explicacion: string;
  detalle: string;
  copias: CopiaEncontrada[];
  rescate_legible: boolean;
}

export interface Rescate {
  fichero: string;
  originales: string[];
  tablas: number;
  filas: number;
  sin_volcado: string | null;
}

export const recuperacionEmpresas = () => invoke<Diagnostico[]>('recuperacion_empresas');
export const recuperacionRescatar = (id: string, path: string) =>
  invoke<Rescate>('recuperacion_rescatar', { id, path });
export const recuperacionRestaurar = (path: string, passphrase: string) =>
  invoke<void>('recuperacion_restaurar', { path, passphrase });

/** Dónde guardar un ZIP (diálogo nativo). */
export async function pickZipSave(defaultPath: string): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({ defaultPath, filters: [{ name: 'ZIP', extensions: ['zip'] }] });
  return path ?? null;
}

export interface AuditEntry {
  seq: number;
  at: string;
  actor: string;
  action: string;
  subject: string;
  details: string;
  prev_hash: string;
  hash: string;
}

export interface FiltroAuditoria {
  desde?: string | null;
  hasta?: string | null;
  accion?: string | null;
  actor?: string | null;
  texto?: string | null;
  antes_de?: number | null;
  limite?: number | null;
}

export interface PaginaAuditoria {
  entradas: AuditEntry[];
  total: number;
  siguiente: number | null;
}

export interface VerificacionAuditoria {
  ok: boolean;
  entradas: number;
  rota_en: number | null;
  mensaje: string;
}

export const auditoriaConsultar = (filtro: FiltroAuditoria) =>
  invoke<PaginaAuditoria>('auditoria_consultar', { filtro });
export const auditoriaAcciones = () => invoke<[string, number][]>('auditoria_acciones');
export const auditoriaVerificar = () => invoke<VerificacionAuditoria>('auditoria_verificar');
export const integridadEmpresa = () => invoke<string[]>('integridad_empresa');

export type TipoPeriodo = 'trimestre' | 'ejercicio';

export interface PeriodoCerrado {
  id: number;
  tipo: TipoPeriodo;
  ejercicio: number;
  trimestre: number | null;
  desde: string;
  hasta: string;
  motivo: string;
  cerrado_en: string;
  etiqueta: string;
}

export interface PeriodoSugerido {
  tipo: TipoPeriodo;
  ejercicio: number;
  trimestre: number | null;
  etiqueta: string;
  motivo: string;
}

export const periodosListar = () => invoke<PeriodoCerrado[]>('periodos_listar');
export const periodosSugeridos = () => invoke<PeriodoSugerido[]>('periodos_sugeridos');
export const periodoCerrar = (
  tipo: TipoPeriodo,
  ejercicio: number,
  trimestre: number | null,
  motivo: string,
) =>
  invoke<PeriodoCerrado>('periodo_cerrar', { tipo, ejercicio, trimestre, motivo, confirmed: true });
export const periodoReabrir = (id: number) => invoke<void>('periodo_reabrir', { id });

export interface Conservacion {
  ejercicio: number;
  cierre: string;
  fin_mercantil: string;
  fin_fiscal: string;
  fin_creditos: string | null;
  creditos_pendientes: boolean;
  conservar_hasta: string | null;
  destruible: boolean;
  motivo: string;
  archivado: { creado_en: string; fichero: string; sha256: string } | null;
}

export interface AvisoConservacion {
  destruibles: number[];
  conservar: Conservacion[];
  sin_archivo: number[];
}

export interface ParteArchivo {
  clave: string;
  titulo: string;
  estado: 'si' | 'parcial' | 'no';
  detalle: string;
  ficheros: number;
}

export interface ArchivoHecho {
  fichero: string;
  sha256: string;
  bytes: number;
  ficheros: number;
  partes: ParteArchivo[];
}

export interface ExportacionCompleta {
  fichero: string;
  sha256: string;
  bytes: number;
  tablas: number;
  filas: number;
  adjuntos: number;
  con_error: string[];
}

export const archivoConservacion = () => invoke<Conservacion[]>('archivo_conservacion');
export const archivoAviso = () => invoke<AvisoConservacion>('archivo_aviso');
export const archivoEjercicioContenido = (year: number) =>
  invoke<ParteArchivo[]>('archivo_ejercicio_contenido', { year });
export const archivoEjercicioExportar = (year: number, path: string) =>
  invoke<ArchivoHecho>('archivo_ejercicio_exportar', { year, path });
export const exportacionCompleta = (path: string) =>
  invoke<ExportacionCompleta>('exportacion_completa', { path });

// ---------- Subvenciones, donaciones y legados (NRV 18.ª) ----------

/** Por lo que financia: capital (130), donación (131), gastos futuros (132) o explotación (740). */
export type TipoSubvencion = 'capital' | 'donacion' | 'gastos' | 'explotacion';

/** Ficha de una subvención. Importes en céntimos; tipo impositivo en centésimas de punto. */
export interface Subvencion {
  id: number;
  concedente: string;
  concedente_nif: string;
  fecha_concesion: string;
  importe: number;
  finalidad: string;
  tipo: TipoSubvencion;
  condiciones: string;
  reintegrable: boolean;
  fixed_asset_id: number | null;
  efecto_impositivo: boolean;
  tipo_impositivo: number;
  minimis: boolean;
  cuenta_deudora: string;
  notas: string;
}

/** Asiento que se pide para una subvención. */
export type AccionSubvencion =
  | { accion: 'concesion' }
  | { accion: 'no_reintegrable'; fecha: string }
  | { accion: 'cobro'; fecha: string; importe: number; tesoreria: string }
  | { accion: 'imputacion'; year: number; importe: number | null };

export interface AsientoSubvencion {
  id: number;
  tipo: 'concesion' | 'no_reintegrable' | 'cobro' | 'imputacion';
  fecha: string;
  ejercicio: number;
  importe: number;
  entry_id: number | null;
}

export interface VistaSubvencion {
  subvencion: Subvencion;
  bien: string | null;
  asientos: AsientoSubvencion[];
  concedida: boolean;
  en_deuda: boolean;
  cobrado: number;
  pendiente_cobro: number;
  imputado: number;
  imputado_ejercicio: number;
  pendiente_imputar: number;
  saldo_pn: number;
  pasivo_diferido: number;
  propuesta_imputacion: number | null;
  avisos: string[];
}

export interface ResumenSubvenciones {
  year: number;
  subvenciones: VistaSubvencion[];
  imputado_ejercicio: number;
  saldo_pn: number;
  pasivo_diferido: number;
  minimis_tres_años: number;
  tope_minimis: number;
  notas: string[];
}

export interface PropuestaSubvencion {
  asiento: JournalEntry;
  importe: number;
}

export const subvenciones = (year: number) => invoke<ResumenSubvenciones>('subvenciones', { year });
export const subvencionGuardar = (subvencion: Subvencion) =>
  invoke<number>('subvencion_guardar', { subvencion });
export const subvencionBorrar = (id: number) =>
  invoke<void>('subvencion_borrar', { id, confirmed: true });
export const subvencionPropuesta = (id: number, accion: AccionSubvencion) =>
  invoke<PropuestaSubvencion>('subvencion_propuesta', { id, accion });
export const subvencionRegistrar = (id: number, accion: AccionSubvencion) =>
  invoke<number>('subvencion_registrar', { id, accion, confirmed: true });
export const subvencionDeshacer = (id: number) =>
  invoke<void>('subvencion_deshacer', { id, confirmed: true });

// ---------- Datos para la Renta y el Patrimonio del socio ----------

export interface PersonaSocio {
  nif: string;
  nombre: string;
  socio: boolean;
  administrador: boolean;
  /** Participación en millonésimas (100 % = 1.000.000). */
  pct: number;
}

export interface ConceptoSocio {
  apartado: string;
  concepto: string;
  /** Casilla de Renta WEB (orientativa). */
  casilla: string;
  importe: number;
  retencion: number;
  fuente: string;
  nota: string;
  estimado: boolean;
}

export type Semaforo = 'cumple' | 'dudoso' | 'no_cumple' | 'sin_datos';

export interface RequisitoExencion {
  titulo: string;
  estado: Semaforo;
  detalle: string;
  norma: string;
}

export interface PatrimonioSocio {
  ejercicio_balance: number;
  fecha_balance: string | null;
  patrimonio_neto: number | null;
  pct: number;
  valor_teorico: number | null;
  requisitos: RequisitoExencion[];
  exencion: Semaforo;
  /** Centésimas de punto. */
  proporcion_afecta: number | null;
  valor_exento_orientativo: number | null;
  notas: string[];
}

export interface InformeSocio {
  year: number;
  persona: PersonaSocio;
  empresa: string;
  nif_empresa: string;
  conceptos: ConceptoSocio[];
  patrimonio: PatrimonioSocio;
  avisos: string[];
  notas: string[];
}

export const rentaSocioPersonas = () => invoke<PersonaSocio[]>('renta_socio_personas');
export const rentaSocio = (year: number, nif: string, otros: number | null) =>
  invoke<InformeSocio>('renta_socio', { year, nif, otros });
/** Pide dónde guardar el informe en PDF y lo guarda. */
export async function rentaSocioSave(
  year: number,
  nif: string,
  otros: number | null,
): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: `Datos Renta y Patrimonio ${year} ${nif}.pdf`,
    filters: [{ name: 'PDF', extensions: ['pdf'] }],
  });
  if (!path) return null;
  await invoke('renta_socio_save', { year, nif, otros, path });
  return path;
}

// ---------- No residentes (216 y 296) e inmuebles arrendados (115 y 180) ----------

export type NaturalezaIrnr = 'dividendo' | 'interes' | 'canon' | 'servicio' | 'otros';
export type RegimenIrnr = 'general' | 'convenio' | 'exenta';
export type ExencionIrnr = 'art14_1_c' | 'art14_1_h' | 'art14_1_m' | 'otra';

export interface CertificadoResidencia {
  id: number;
  nif: string;
  nombre: string;
  pais: string;
  expedido: string;
  /** Sin indicar: un año desde la expedición. */
  valido_hasta: string | null;
  convenio: boolean;
  documento: string;
  notas: string;
}

export interface EstadoCertificadoResidencia extends CertificadoResidencia {
  valido_hasta: string;
  vigente: boolean;
  caduca_pronto: boolean;
}

export interface NuevaRentaIrnr {
  fecha: string;
  nif: string;
  nombre: string;
  pais: string;
  naturaleza: NaturalezaIrnr;
  /** Céntimos. */
  bruto: number;
  /** Centésimas de punto (500 = 5 %). */
  tipo_convenio: number | null;
  exencion: ExencionIrnr | null;
  concepto: string;
  recibida_id: number | null;
}

export interface PropuestaIrnr {
  regimen: RegimenIrnr;
  tipo: number;
  tipo_general: number;
  exencion: ExencionIrnr | null;
  motivo: string;
  avisos: string[];
}

export interface RentaIrnr {
  fecha: string;
  nif: string;
  nombre: string;
  pais: string;
  naturaleza: NaturalezaIrnr;
  bruto: number;
  tipo: number;
  retencion: number;
  regimen: RegimenIrnr;
  exencion: ExencionIrnr | null;
}

export interface RentaIrnrGuardada extends RentaIrnr {
  id: number;
  concepto: string;
  recibida_id: number | null;
}

export interface CandidataIrnr {
  recibida_id: number;
  fecha: string;
  nif: string;
  nombre: string;
  pais: string;
  numero: string;
  base: number;
}

export interface ConvenioIrnr {
  pais: string;
  nombre: string;
  dividendos_bp: number;
  intereses_bp: number;
  canones_bp: number;
  fuente: string;
}

export interface ResumenIrnr {
  year: number;
  certificados: EstadoCertificadoResidencia[];
  rentas: RentaIrnrGuardada[];
  dividendos: RentaIrnr[];
  candidatas: CandidataIrnr[];
  convenios: ConvenioIrnr[];
}

export const irnrResumen = (year: number) => invoke<ResumenIrnr>('irnr_resumen', { year });
export const irnrGuardarCertificado = (certificado: CertificadoResidencia) =>
  invoke<number>('irnr_guardar_certificado', { certificado });
export const irnrQuitarCertificado = (id: number, confirmed: boolean) =>
  invoke<void>('irnr_quitar_certificado', { id, confirmed });
export const irnrProponer = (renta: NuevaRentaIrnr) =>
  invoke<PropuestaIrnr>('irnr_proponer', { renta });
export const irnrApuntar = (renta: NuevaRentaIrnr, confirmed: boolean) =>
  invoke<number>('irnr_apuntar', { renta, confirmed });
export const irnrQuitarRenta = (id: number, confirmed: boolean) =>
  invoke<void>('irnr_quitar_renta', { id, confirmed });

export interface InmuebleArrendado {
  /** NIF del arrendador. */
  nif: string;
  /** 1 territorio común, 2 País Vasco, 3 Navarra, 4 sin referencia catastral. */
  situacion: number;
  ref_catastral: string;
  via: string;
  numero: string;
  municipio: string;
  provincia: string;
  codigo_postal: string;
  /** Hasta cuándo vale la acreditación del arrendador para no retener (art. 75.3.g RIRPF). */
  exonerado_hasta: string | null;
}

export const inmueblesList = () => invoke<InmuebleArrendado[]>('inmuebles_list');
export const inmuebleGuardar = (inmueble: InmuebleArrendado) =>
  invoke<void>('inmueble_guardar', { inmueble });
export const inmuebleQuitar = (nif: string, confirmed: boolean) =>
  invoke<void>('inmueble_quitar', { nif, confirmed });

// --- Ventanilla única (OSS, modelos 035 y 369) ---

export type ClaseOss = 'servicios' | 'bienes' | 'otro';

export interface AjustesOss {
  /** Fecha de efectos del alta en el régimen de la Unión (035). */
  alta_035: string | null;
  /** Opción por tributar en destino aunque no se supere el umbral (art. 73 LIVA). */
  opcion_destino: boolean;
  /** Ventas fuera de TaxPain por año (céntimos, sin IVA). */
  otras_ventas: Record<string, number>;
}

export interface UmbralOss {
  year: number;
  anterior: number;
  en_curso: number;
  superado_anterior: boolean;
  superado_el: string | null;
  destino_desde: string | null;
}

export interface EstadoOss {
  umbral: UmbralOss;
  limite: number;
  ajustes: AjustesOss;
  tributa_en_destino: boolean;
  destino_desde: string | null;
  sin_clasificar: number;
  sin_clasificar_base: number;
  con_iva_espanol: string[];
  falta_035: boolean;
  avisos: string[];
}

export interface VentaUe {
  invoice_id: number;
  number: string;
  date: string;
  cliente: string;
  pais: string;
  pais_nombre: string;
  base: number;
  iva_espanol: number;
  oss: OssVat | null;
  clase: ClaseOss | null;
  clase_fija: boolean;
  cuenta: boolean;
  rectificativa: boolean;
  rectifica: [string, string] | null;
}

export interface TipoPaisOss {
  pais: string;
  nombre: string;
  tabla: number;
  confirmado: number | null;
  confirmado_at: string | null;
}

export interface TiposOss {
  fecha: string;
  fuente: string;
  paises: TipoPaisOss[];
}

export interface OperacionOss {
  invoice_id: number;
  number: string;
  date: string;
  cliente: string;
  pais: string;
  pais_nombre: string;
  supply: OssSupply;
  rate: number;
  base: number;
  cuota: number;
  corrige: [number, number] | null;
}

export interface Linea369 {
  pais: string;
  pais_nombre: string;
  supply: OssSupply;
  rate: number;
  base: number;
  cuota: number;
}

export interface Correccion369 {
  year: number;
  quarter: number;
  pais: string;
  pais_nombre: string;
  cuota: number;
}

export interface Borrador369 {
  year: number;
  quarter: number;
  vencimiento: string | null;
  lineas: Linea369[];
  correcciones: Correccion369[];
  total: number;
  por_pais: [string, number][];
  asiento: { cuenta: string; concepto: string; debe: number; haber: number }[];
  avisos: string[];
}

export const ossEstado = (year: number) => invoke<EstadoOss>('oss_estado', { year });
export const ossVentas = (year: number) => invoke<VentaUe[]>('oss_ventas', { year });
export const ossClasificar = (invoiceId: number, clase: ClaseOss | null) =>
  invoke<void>('oss_clasificar', { invoiceId, clase });
export const ossTipos = () => invoke<TiposOss>('oss_tipos');
export const ossConfirmarTipo = (pais: string, tipoBp: number, confirmed: boolean) =>
  invoke<void>('oss_confirmar_tipo', { pais, tipoBp, confirmed });
export const ossAjustesGuardar = (ajustes: AjustesOss) =>
  invoke<void>('oss_ajustes_guardar', { ajustes });
export const ossLibro = (year: number, quarter: number) =>
  invoke<OperacionOss[]>('oss_libro', { year, quarter });
export const oss369 = (year: number, quarter: number) =>
  invoke<Borrador369>('oss_369', { year, quarter });
export const oss369Hoja = (year: number, quarter: number, dir: string) =>
  invoke<string>('oss_369_hoja', { year, quarter, dir });

// Operaciones vinculadas (art. 18 LIS, modelo 232) y participadas: espejo de
// `taxpain_app::vinculadas` y sus comandos. Importes en céntimos; participaciones en
// millonésimas (100 % = 1.000.000).

import { command } from './api';

export type Vinculo =
  | 'socio'
  | 'participada'
  | 'administrador'
  | 'familiar'
  | 'grupo'
  | 'administrador_grupo'
  | 'participada_indirecta'
  | 'socios_comunes'
  | 'establecimiento_permanente';

export const VINCULO_LABEL: Record<Vinculo, string> = {
  socio: 'Socio con el 25 % o más (art. 18.2.a)',
  participada: 'Participada con el 25 % o más (art. 18.2.a)',
  administrador: 'Administrador (art. 18.2.b)',
  familiar: 'Cónyuge o familiar hasta 3.er grado de socio o administrador (art. 18.2.c)',
  grupo: 'Entidad del grupo, art. 42 CCom (art. 18.2.d)',
  administrador_grupo: 'Administrador de otra entidad del grupo (art. 18.2.e)',
  participada_indirecta: 'Participada indirecta con el 25 % o más (art. 18.2.f)',
  socios_comunes: 'Entidad con los mismos socios, 25 % o más (art. 18.2.g)',
  establecimiento_permanente: 'Establecimiento permanente en el extranjero (art. 18.2.h)',
};

export type Concepto =
  | 'bienes_tangibles'
  | 'intangibles'
  | 'fondos_propios'
  | 'creditos_deuda'
  | 'financiacion'
  | 'servicios'
  | 'reparto_costes'
  | 'alquileres'
  | 'intereses'
  | 'trabajo'
  | 'otras'
  | 'dividendos'
  | 'retribucion_administrador';

/** Claves del tipo de operación del 232 (Orden HFP/816/2017, art. 3.1.f). */
export const CLAVES_232: [number, string][] = [
  [1, 'Bienes tangibles (compra o venta)'],
  [2, 'Intangibles (compra, venta o cesión de uso)'],
  [3, 'Participaciones y otros instrumentos de fondos propios'],
  [4, 'Derechos de crédito y valores de deuda'],
  [5, 'Operaciones financieras de deuda (préstamos, sin intereses)'],
  [6, 'Servicios'],
  [7, 'Acuerdos de reparto de costes'],
  [8, 'Alquileres de inmuebles'],
  [9, 'Intereses de préstamos y créditos'],
  [10, 'Rendimientos del trabajo y pensiones'],
  [11, 'Otras operaciones'],
];

export const CONCEPTO_LABEL: Record<Concepto, string> = {
  bienes_tangibles: 'Bienes tangibles',
  intangibles: 'Intangibles',
  fondos_propios: 'Participaciones (fondos propios)',
  creditos_deuda: 'Créditos y valores de deuda',
  financiacion: 'Préstamos y cuentas corrientes',
  servicios: 'Servicios',
  reparto_costes: 'Reparto de costes',
  alquileres: 'Alquileres',
  intereses: 'Intereses',
  trabajo: 'Trabajo',
  otras: 'Otras',
  dividendos: 'Dividendos',
  retribucion_administrador: 'Retribución del administrador',
};

export type Metodo =
  | 'precio_libre_comparable'
  | 'coste_incrementado'
  | 'precio_reventa'
  | 'distribucion_resultado'
  | 'margen_neto_operacional'
  | 'otros';

export const METODO_LABEL: Record<Metodo, string> = {
  precio_libre_comparable: 'Precio libre comparable',
  coste_incrementado: 'Coste incrementado',
  precio_reventa: 'Precio de reventa',
  distribucion_resultado: 'Distribución del resultado',
  margen_neto_operacional: 'Margen neto operacional',
  otros: 'Otros métodos aceptados',
};

export interface Vinculada {
  id: number;
  nif: string;
  name: string;
  legal_person: boolean;
  province: string;
  country: string;
  vinculo: Vinculo;
  detalle: string;
  pct: number | null;
  grupo_fiscal: boolean;
  clave_emitidas: number;
  clave_recibidas: number;
  active: boolean;
}

export interface Participada {
  id: number;
  nif: string;
  name: string;
  province: string;
  country: string;
  pct: number;
  acquired: string;
  cost: number;
  account: string;
  grupo_fiscal: boolean;
  notes: string;
  active: boolean;
}

export interface OperacionManual {
  id: number;
  year: number;
  nif: string;
  clave: number;
  ingreso: boolean;
  amount: number;
  especifica: boolean;
  description: string;
}

export interface Valoracion {
  year: number;
  nif: string;
  concepto: Concepto;
  ingreso: boolean;
  metodo: Metodo;
  descripcion: string;
  justificacion: string;
}

export interface Textos {
  year: number;
  actividad: string;
  justificacion: string;
}

export interface PersonaVinculada {
  nif: string;
  name: string;
  legal_person: boolean;
  province: string;
  country: string;
  vinculos: Vinculo[];
  detalle: string;
  pct: number | null;
  grupo_fiscal: boolean;
  ficha: Vinculada | null;
  socio: boolean;
  administrador: boolean;
  participada: boolean;
  cuentas: string[];
  total: number;
  supera_umbral: boolean;
}

export interface OperacionVinculada {
  nif: string;
  name: string;
  vinculo: Vinculo;
  concepto: Concepto;
  clave: number | null;
  ingreso: boolean;
  importe: number;
  metodo: Metodo;
  metodo_elegido: boolean;
  especifica: boolean;
  origen: string[];
  excluida: string | null;
  en_232: boolean;
  motivo: string | null;
  descripcion: string;
  justificacion: string;
}

export interface CuentaVinculada {
  account: string;
  name: string;
  debit: number;
  credit: number;
  nif: string | null;
}

export interface RegistroVinculadas {
  year: number;
  start: string;
  end: string;
  company: string;
  company_nif: string;
  incn: number | null;
  incn_fuente: string;
  personas: PersonaVinculada[];
  operaciones: OperacionVinculada[];
  manuales: OperacionManual[];
  cuentas: CuentaVinculada[];
  obligado: boolean;
  motivos: string[];
  avisos: string[];
  plazo: string;
  simplificada: boolean;
  documentar: string[];
  textos: Textos;
}

export const vinculadasRegistro = (year: number) =>
  command<RegistroVinculadas>('vinculadas_registro', { year });
export const vinculadaDe = (nif: string) => command<Vinculada | null>('vinculada_de', { nif });
export const vinculadaSave = (vinculada: Vinculada) =>
  command<number>('vinculada_save', { vinculada });
export const vinculadaDelete = (id: number) => command<void>('vinculada_delete', { id });
export const participadasList = () => command<Participada[]>('participadas_list');
export const participadaSave = (participada: Participada) =>
  command<number>('participada_save', { participada });
export const participadaDelete = (id: number) => command<void>('participada_delete', { id });
export const vinculadaCuentaSet = (account: string, nif: string | null) =>
  command<void>('vinculada_cuenta_set', { account, nif });
export const vinculadaOperacionSave = (operacion: OperacionManual) =>
  command<number>('vinculada_operacion_save', { operacion });
export const vinculadaOperacionDelete = (id: number) =>
  command<void>('vinculada_operacion_delete', { id });
export const vinculadaValoracionSave = (valoracion: Valoracion) =>
  command<void>('vinculada_valoracion_save', { valoracion });
export const vinculadasTextosSave = (textos: Textos) =>
  command<void>('vinculadas_textos_save', { textos });

/** Pide dónde guardar la documentación simplificada (RTF para editar o PDF) y la guarda. */
export async function vinculadasDocumentacionSave(
  year: number,
  pdf: boolean,
): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: `Operaciones vinculadas ${year}.${pdf ? 'pdf' : 'rtf'}`,
    filters: [pdf ? { name: 'PDF', extensions: ['pdf'] } : { name: 'RTF', extensions: ['rtf'] }],
  });
  if (!path) return null;
  await command('vinculadas_documentacion_save', { year, path, pdf });
  return path;
}

/** Pide dónde guardar el borrador del 232 (Excel) y lo guarda. */
export async function vinculadasHoja232Save(year: number): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: `Modelo 232 ${year}.xlsx`,
    filters: [{ name: 'Excel', extensions: ['xlsx'] }],
  });
  if (!path) return null;
  await command('vinculadas_hoja_232_save', { year, path });
  return path;
}

/** Millonésimas → «33,3333». */
export const showPct = (p: number) =>
  (p / 10000).toLocaleString('es-ES', { maximumFractionDigits: 4 });

/** «33,3333» → millonésimas; inválido → null. */
export function parsePct(s: string): number | null {
  const n = Number(s.trim().replace(',', '.'));
  if (!(n > 0 && n <= 100)) return null;
  return Math.round(n * 10000);
}

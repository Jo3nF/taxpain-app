// Ajustes de cierre: espejo de `taxpain_app::ajustes_cierre` y sus comandos. Importes en
// céntimos; fechas «AAAA-MM-DD».

import { command, type ProposedLine } from './api';

export type TipoAjuste =
  | 'gasto_anticipado'
  | 'ingreso_anticipado'
  | 'pendiente_recibir'
  | 'pendiente_emitir'
  | 'deterioro'
  | 'provision';

export type MotivoDeterioro =
  | 'seis_meses'
  | 'concurso'
  | 'concurso_liquidacion'
  | 'alzamiento'
  | 'judicial'
  | 'ninguno';

export type ExclusionDeterioro = 'ninguna' | 'publica' | 'garantia' | 'renovado' | 'vinculada';

export const MOTIVO_LABEL: Record<MotivoDeterioro, string> = {
  seis_meses: 'Más de seis meses vencido (art. 13.1.a)',
  concurso: 'Deudor en concurso (art. 13.1.b)',
  concurso_liquidacion: 'En concurso, con la fase de liquidación abierta',
  alzamiento: 'Deudor procesado por alzamiento de bienes (art. 13.1.c)',
  judicial: 'Reclamado judicialmente o en arbitraje (art. 13.1.d)',
  ninguno: 'Ninguna: sólo contable',
};

export const EXCLUSION_LABEL: Record<ExclusionDeterioro, string> = {
  ninguna: 'Ninguna',
  publica: 'Entidad de derecho público',
  garantia: 'Afianzado o garantizado (aval, seguro de crédito, derecho real)',
  renovado: 'Renovado o prorrogado',
  vinculada: 'Persona o entidad vinculada',
};

export interface Ajuste {
  id: number;
  year: number;
  tipo: TipoAjuste;
  descripcion: string;
  importe: number;
  cuenta: string;
  contrapartida: string;
  factura_kind: string | null;
  factura_id: number | null;
  vencimiento: number | null;
  servicio_desde: string | null;
  servicio_hasta: string | null;
  tercero_nif: string | null;
  tercero: string | null;
  deducible: boolean;
  motivo_fiscal: string;
  entry_id: number;
  fecha: string;
  lines: ProposedLine[];
  reversion_entry_id: number | null;
  reversion_fecha: string | null;
  reversion_year: number | null;
}

export interface Periodificable {
  kind: 'recibida' | 'emitida';
  factura_id: number;
  numero: string;
  tercero: string;
  fecha: string;
  cuentas: ProposedLine[];
  importe: number;
  desde: string | null;
  hasta: string | null;
  ajuste_id: number | null;
}

export interface Deteriorable {
  factura_id: number;
  vencimiento: number;
  numero: string;
  cliente: string;
  nif: string | null;
  cuenta: string | null;
  vence: string;
  dias: number;
  importe: number;
  vinculada: boolean;
}

export interface Sugerencia {
  kind: 'recibida' | 'emitida';
  factura_id: number;
  numero: string;
  tercero: string;
  nif: string | null;
  fecha: string;
  base: number;
  cuenta: string | null;
}

export interface ResumenAjustes {
  year: number;
  start: string;
  end: string;
  reversion: string;
  closed: boolean;
  ajustes: Ajuste[];
  anteriores: Ajuste[];
  periodificables: Periodificable[];
  deteriorables: Deteriorable[];
  sugerencias: Sugerencia[];
  deterioro_anterior: number;
  fiscal: { aumentos: number; disminuciones: number; detalle: string[] };
}

export interface NuevoAjuste {
  tipo: TipoAjuste;
  descripcion?: string;
  importe?: number | null;
  cuenta?: string | null;
  contrapartida?: string | null;
  factura_id?: number | null;
  factura_kind?: string | null;
  vencimiento?: number | null;
  desde?: string | null;
  hasta?: string | null;
  tercero_nif?: string | null;
  tercero?: string | null;
  deducible?: boolean | null;
  motivo?: MotivoDeterioro | null;
  exclusion?: ExclusionDeterioro | null;
}

export interface PropuestaAjuste {
  tipo: TipoAjuste;
  descripcion: string;
  importe: number;
  fecha: string;
  lines: ProposedLine[];
  reversion_fecha: string | null;
  reversion: ProposedLine[];
  deducible: boolean;
  motivo_fiscal: string;
  avisos: string[];
}

export interface Comprobacion {
  id: string;
  label: string;
  ok: boolean;
  detail: string;
  cuentas: { account: string; name: string; saldo: number }[];
  /** Vista de Contabilidad donde se arregla (`mayor`, `banco`, `impuestos`, `prestamos`…). */
  ir: string;
  cuenta: string | null;
}

export const ajustesCierre = (year: number, todas: boolean) =>
  command<ResumenAjustes>('ajustes_cierre', { year, todas });
export const ajustesCierreComprobaciones = (year: number) =>
  command<Comprobacion[]>('ajustes_cierre_comprobaciones', { year });
export const ajustesCierrePropuesta = (year: number, nuevo: NuevoAjuste) =>
  command<PropuestaAjuste>('ajustes_cierre_propuesta', { year, nuevo });
export const ajustesCierreCrear = (year: number, nuevo: NuevoAjuste) =>
  command<number>('ajustes_cierre_crear', { year, nuevo, confirmed: true });
export const ajustesCierreDeshacer = (id: number) =>
  command<null>('ajustes_cierre_deshacer', { id, confirmed: true });
export const ajustesCierreRevertir = (id: number, year: number) =>
  command<number>('ajustes_cierre_revertir', { id, year, confirmed: true });
export const ajustesCierreDeshacerReversion = (id: number) =>
  command<null>('ajustes_cierre_deshacer_reversion', { id, confirmed: true });

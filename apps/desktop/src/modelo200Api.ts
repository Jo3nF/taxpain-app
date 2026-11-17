// Modelo 200 completo (G05) y ficha fiscal de las participadas (G15): espejo de
// `taxpain_app::modelo200` y sus comandos. Importes en céntimos; participaciones en
// millonésimas (100 % = 1.000.000).

import { command } from './api';
import type { Participada } from './vinculadasApi';

export type Regimen200 = 'General' | 'Micro' | 'ReducidaDimension' | 'NuevaCreacion';

export const REGIMEN_LABEL: Record<Regimen200, string> = {
  General: 'Tipo general (25 %)',
  Micro: 'Cifra de negocios < 1 M€ (dos tramos)',
  ReducidaDimension: 'Reducida dimensión (cifra < 10 M€)',
  NuevaCreacion: 'Nueva creación (15 %)',
};

export interface Importe200 {
  casilla: number;
  importe: number;
}

export interface Linea200 {
  casilla: number | null;
  concepto: string;
  importe: number;
  origen: string;
  norma: string;
}

export type EstadoAjuste = 'propuesta' | 'sin_incluir' | 'editada' | 'descartada' | 'manual';

export interface Ajuste200 {
  clave: string;
  casilla: number;
  aumento: boolean;
  concepto: string;
  norma: string;
  importe: number;
  propuesto: number | null;
  origen: string;
  estado: EstadoAjuste;
  nota: string | null;
}

export interface DetalleArrastre {
  origen: number;
  inicio: number;
  aplicado: number;
  pendiente: number;
}

export type TipoArrastre = 'bin' | 'nivelacion' | 'capitalizacion' | 'doble_imposicion';

export const TIPO_ARRASTRE_LABEL: Record<TipoArrastre, string> = {
  bin: 'Base imponible negativa',
  nivelacion: 'Reserva de nivelación por adicionar',
  capitalizacion: 'Reserva de capitalización por aplicar',
  doble_imposicion: 'Doble imposición internacional pendiente',
};

export interface Arrastre200 {
  tipo: TipoArrastre;
  origen: number;
  ejercicio: number;
  pendiente: number;
  nota: string;
}

export interface PartidaFondos {
  cuenta: string;
  nombre: string;
  inicio: number;
  fin: number;
  computa: boolean;
  motivo: string | null;
}

export interface FondosPropios200 {
  inicio: number;
  fin: number;
  incremento: number;
  partidas: PartidaFondos[];
  avisos: string[];
}

export interface Dotacion200 {
  a_dotar: number;
  dotada: number | null;
  estado: 'dotada' | 'en_acta' | 'falta' | 'pendiente';
  texto: string;
}

export interface Reserva200 {
  lineas: Linea200[];
  detalle: DetalleArrastre[];
  dotacion: Dotacion200 | null;
}

export interface Correccion200 {
  casilla: number;
  aumento: boolean;
  pagina: string;
  concepto: string;
  norma: string;
  solo_erd: boolean;
}

export interface CasillaCuota200 {
  casilla: number;
  grupo: 'bonificacion' | 'doble_imposicion' | 'deduccion' | 'retencion';
  concepto: string;
  norma: string;
}

export interface Datos200 {
  aplicar_capitalizacion: boolean;
  incremento_fondos_propios: number | null;
  porcentaje_capitalizacion: number | null;
  nivelacion: number;
  bin_nueva_creacion: boolean;
  doble_imposicion: number | null;
  cuota: Importe200[];
  retenciones: Importe200[] | null;
}

export type OrigenParticipacion = 'compra' | 'constitucion' | 'aportacion' | 'canje';

export const ORIGEN_LABEL: Record<OrigenParticipacion, string> = {
  compra: 'Compra',
  constitucion: 'Constitución',
  aportacion: 'Aportación no dineraria (art. 87 LIS)',
  canje: 'Canje de valores (art. 80 LIS)',
};

export interface DatosFiscales {
  participada_id: number;
  nominal: number;
  valor_fiscal: number | null;
  origen: OrigenParticipacion;
  tributacion_minima: boolean;
}

export interface FondosParticipada {
  participada_id: number;
  year: number;
  capital: number;
  reservas: number;
  otras_pn: number;
  resultado: number;
}

export interface DividendoParticipada {
  id: number;
  participada_id: number;
  fecha_acuerdo: string;
  fecha_cobro: string | null;
  bruto: number;
  retencion: number;
  documento: string;
  notas: string;
}

export type EstadoArt21 = 'cumple' | 'condicionada' | 'no_cumple';

export interface DividendoVista {
  dividendo: DividendoParticipada;
  art21: {
    estado: EstadoArt21;
    ano_cumplido: string;
    exento_pct: number;
    motivos: string[];
  };
  exento: number;
  retencion_procede: number;
}

export interface ParticipadaVista {
  participada: Participada;
  fiscal: DatosFiscales;
  valor_fiscal: number;
  fondos: FondosParticipada | null;
  dividendos: DividendoVista[];
  bruto: number;
  retenido: number;
  exento: number;
  pn_proporcional: number | null;
  avisos: string[];
}

export interface Hoja200 {
  year: number;
  inicio: string;
  fin: string;
  regimen_guardado: Regimen200;
  regimen_propuesto: Regimen200;
  regimen_motivo: string;
  regimen_aplicado: Regimen200;
  patrimonial: boolean | null;
  cifra_negocios_anterior: number | null;
  cifra_negocios: number;
  resultado: Linea200[];
  ajustes: Ajuste200[];
  base_previa: Linea200[];
  bins: DetalleArrastre[];
  bin_generada: number;
  bins_origen: 'arrastres' | 'datos' | 'ninguna';
  base: Linea200[];
  fondos_propios: FondosPropios200;
  capitalizacion: Reserva200;
  nivelacion: Reserva200;
  cuota: Linea200[];
  retenciones: Linea200[];
  liquidacion: Linea200[];
  participadas: ParticipadaVista[];
  arrastres: Arrastre200[];
  siguiente: Arrastre200[];
  datos: Datos200;
  catalogo: Correccion200[];
  catalogo_cuota: CasillaCuota200[];
  notas: string[];
  avisos: string[];
}

export interface AjusteGuardado {
  clave: string;
  casilla: number;
  importe: number | null;
  concepto: string;
}

export const modelo200Hoja = (year: number) => command<Hoja200>('modelo200_hoja', { year });
export const modelo200AjusteGuardar = (year: number, ajuste: AjusteGuardado) =>
  command<string>('modelo200_ajuste_guardar', { year, ajuste });
export const modelo200AjusteQuitar = (year: number, clave: string) =>
  command<void>('modelo200_ajuste_quitar', { year, clave, confirmed: true });
export const modelo200DatosGuardar = (year: number, datos: Datos200) =>
  command<void>('modelo200_datos_guardar', { year, datos });
export const modelo200ArrastreGuardar = (arrastre: Arrastre200) =>
  command<void>('modelo200_arrastre_guardar', { arrastre });
export const modelo200ArrastreBorrar = (tipo: TipoArrastre, origen: number, ejercicio: number) =>
  command<void>('modelo200_arrastre_borrar', { tipo, origen, ejercicio, confirmed: true });
export const modelo200PasarSiguiente = (year: number) =>
  command<number>('modelo200_pasar_siguiente', { year, confirmed: true });
export const participadasFichas = (year: number) =>
  command<ParticipadaVista[]>('participadas_fichas', { year });
export const participadaFiscalGuardar = (datos: DatosFiscales) =>
  command<void>('participada_fiscal_guardar', { datos });
export const participadaFondosGuardar = (fondos: FondosParticipada) =>
  command<void>('participada_fondos_guardar', { fondos });
export const participadaDividendoGuardar = (dividendo: DividendoParticipada) =>
  command<number>('participada_dividendo_guardar', { dividendo });
export const participadaDividendoBorrar = (id: number) =>
  command<void>('participada_dividendo_borrar', { id, confirmed: true });

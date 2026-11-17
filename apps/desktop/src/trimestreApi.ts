// Cierre de trimestre guiado, guía «Dejar la gestoría» y obligaciones no fiscales: espejo de
// `taxpain_app::{trimestre, puesta_marcha, obligaciones}` y sus comandos. Importes en céntimos;
// fechas «AAAA-MM-DD».

import { command } from './api';

export type EstadoPasoTrimestre = 'ok' | 'pendiente' | 'aviso' | 'no_aplica';
export type MarcaTrimestre = 'hecho' | 'no_aplica';

export interface DestinoPaso {
  tab: string;
  view: string | null;
}

export interface PasoTrimestre {
  id: string;
  label: string;
  estado: EstadoPasoTrimestre;
  detalle: string;
  destino: DestinoPaso | null;
  manual: boolean;
  marca: MarcaTrimestre | null;
}

export interface CuadreModelo {
  concepto: string;
  modelo: number;
  diario: number;
  ok: boolean;
}

export interface ModeloTrimestre {
  model: string;
  period: string;
  result: number;
  plazo: string | null;
  domiciliacion: string | null;
  presentado: boolean;
  cuadres: CuadreModelo[];
  avisos: string[];
  error: string | null;
}

export interface CierreTrimestre {
  year: number;
  quarter: number;
  start: string;
  end: string;
  pasos: PasoTrimestre[];
  modelos: ModeloTrimestre[];
  a_ingresar: number;
  domiciliar_hasta: string | null;
  listo: boolean;
  presentado: boolean;
  sin_perfil: boolean;
}

export interface ResumenTrimestre {
  quarter: number;
  listo: boolean;
  presentado: boolean;
  pendientes: number;
  en_curso: boolean;
  futuro: boolean;
}

export const cierreTrimestre = (year: number, quarter: number) =>
  command<CierreTrimestre>('cierre_trimestre', { year, quarter });
export const cierreTrimestreResumen = (year: number) =>
  command<ResumenTrimestre[]>('cierre_trimestre_resumen', { year });
export const cierreTrimestreMarcar = (
  year: number,
  quarter: number,
  paso: string,
  marca: MarcaTrimestre | null,
) => command<void>('cierre_trimestre_marcar', { year, quarter, paso, marca });

// Guía «Dejar la gestoría»

export type EstadoPasoGuia = 'pendiente' | 'hecho' | 'no_aplica';

export interface Enlace {
  label: string;
  url: string;
}

export interface PasoGuia {
  id: string;
  titulo: string;
  texto: string;
  /** «importar», «contabilidad:apertura»… */
  destino: string | null;
  enlaces: Enlace[];
  estado: EstadoPasoGuia;
  fecha: string | null;
  nota: string;
  detectado: string | null;
}

export interface Guia {
  pasos: PasoGuia[];
  resueltos: number;
  empezada: boolean;
  correo: { asunto: string; cuerpo: string };
}

export const puestaMarcha = () => command<Guia>('puesta_marcha');
export const puestaMarchaMarcar = (
  paso: string,
  estado: EstadoPasoGuia,
  fecha: string | null,
  nota: string,
) => command<void>('puesta_marcha_marcar', { paso, estado, fecha, nota });

// Obligaciones no fiscales

export type EstadoObligacion = 'sin_revisar' | 'aplica' | 'no_aplica' | 'hecho';

export interface Obligacion {
  id: string;
  titulo: string;
  que_es: string;
  a_quien: string;
  plazo: string;
  norma: string;
  enlaces: Enlace[];
  con_empleados: boolean;
  estado: EstadoObligacion;
  fecha: string | null;
  nota: string;
}

export interface Obligaciones {
  lista: Obligacion[];
  hay_empleados: boolean;
}

export const obligacionesNoFiscales = () => command<Obligaciones>('obligaciones_no_fiscales');
export const obligacionNoFiscalMarcar = (
  id: string,
  estado: EstadoObligacion,
  fecha: string | null,
  nota: string,
) => command<void>('obligacion_no_fiscal_marcar', { id, estado, fecha, nota });

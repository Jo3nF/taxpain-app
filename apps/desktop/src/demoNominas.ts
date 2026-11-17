import type {
  Employee,
  FichaNomina,
  Finiquito,
  NominaCalculada,
  ResumenSeguros,
  TablaCotizacion,
} from './api';
import { autonomoHandlers } from './demoAutonomo';

/** Trabajador por cuenta ajena de la demo (datos inventados). */
export const trabajadorDemo: Employee = {
  id: 2,
  nif: '00000000T',
  name: 'Trabajador Inventado Demo',
  province: '46',
  birth_year: 1990,
  family: 3,
  spouse_nif: null,
  disability: 0,
  contract: 1,
  mobility: false,
  children_under3: 0,
  children_rest: 0,
  children_whole: false,
  active: true,
  administrator: false,
  ss_regime: 'Ninguno',
};

const ficha: FichaNomina = {
  employee_id: 2,
  naf: '46/00000000/00',
  categoria: 'Oficial administrativo',
  ccc: '46/0000000/00',
  antiguedad: '2025-03-01',
  cnae: '7111',
  grupo: 5,
  contrato: 'Indefinido',
  jornada: 10_000,
  salario_base: 180_000,
  complementos: [{ nombre: 'Plus de convenio', importe: 20_000, clase: 'Salarial' }],
  pagas: { numero: 2, importe: 180_000, prorrateadas: false, en_este_mes: 0 },
  at_it: 65,
  at_ims: 35,
  tipo_irpf: 1200,
};

/** La nómina de octubre de 2026 de la ficha de la demo (mismas cifras que el test de Rust). */
const nomina: NominaCalculada = {
  year: 2026,
  month: 10,
  grupo: 5,
  devengos: [
    {
      concepto: 'Salario base',
      importe: 180_000,
      clase: 'Salarial',
      extraordinaria: false,
      horas_extra: false,
    },
    {
      concepto: 'Plus de convenio',
      importe: 20_000,
      clase: 'Salarial',
      extraordinaria: false,
      horas_extra: false,
    },
  ],
  total_devengado: 200_000,
  remuneracion_mensual: 200_000,
  prorrata_pagas: 30_000,
  base_minima: 142_440,
  base_maxima: 510_120,
  base_comunes: 230_000,
  base_profesionales: 230_000,
  base_horas_extra: 0,
  tope: null,
  cuotas: [
    ['Comunes', 'Contingencias comunes', 2360, 54_280, 470, 10_810],
    ['Mei', 'Mecanismo de equidad intergeneracional (MEI)', 75, 1725, 15, 345],
    ['Desempleo', 'Desempleo', 550, 12_650, 155, 3565],
    ['Formacion', 'Formación profesional', 60, 1380, 10, 230],
    ['Fogasa', 'FOGASA', 20, 460, 0, 0],
    ['AtIt', 'AT y EP: incapacidad temporal', 65, 1495, 0, 0],
    ['AtIms', 'AT y EP: incapacidad permanente, muerte y supervivencia', 35, 805, 0, 0],
  ].map(([clave, concepto, te, e, tt, t]) => ({
    clave: clave as NominaCalculada['cuotas'][number]['clave'],
    concepto: concepto as string,
    base: 230_000,
    tipo_empresa: te as number,
    empresa: e as number,
    tipo_trabajador: tt as number,
    trabajador: t as number,
  })),
  total_trabajador: 14_950,
  total_empresa: 72_795,
  base_irpf: 200_000,
  tipo_irpf: 1200,
  retencion_irpf: 24_000,
  anticipos: 0,
  otras_deducciones: 0,
  total_deducciones: 38_950,
  liquido: 161_050,
  coste_empresa: 272_795,
  norma: 'Orden PJC/297/2026 (BOE-A-2026-7296) y RDL 3/2026; DT 43.ª y art. 19 bis TRLGSS',
  avisos: [
    'Sin cotejar con el BOE: Bases mínimas de los grupos 1, 2 y 3 de 2026 (art. 3 de la Orden PJC/297/2026): estimadas con la subida del SMI',
  ],
};

function seguros(year: number, month: number): ResumenSeguros {
  const vacio = month !== 10 || year !== 2026;
  return {
    year,
    month,
    conceptos: vacio
      ? []
      : nomina.cuotas.map((c) => ({
          clave: c.clave,
          concepto: c.concepto,
          empresa: c.empresa,
          trabajador: c.trabajador,
          total: c.empresa + c.trabajador,
        })),
    trabajadores: vacio
      ? []
      : [
          {
            payslip_id: 50,
            employee: trabajadorDemo.name,
            nif: trabajadorDemo.nif,
            base_comunes: 230_000,
            base_profesionales: 230_000,
            trabajador: 14_950,
            empresa: 72_795,
            sin_desglose: false,
          },
        ],
    total_trabajador: vacio ? 0 : 14_950,
    total_empresa: vacio ? 0 : 72_795,
    total: vacio ? 0 : 87_745,
    vence_nominal: '2026-11-30',
    vence: '2026-11-30',
    devengo: vacio
      ? []
      : [
          { cuenta: '640', nombre: 'Sueldos y salarios', debe: 200_000, haber: 0 },
          {
            cuenta: '642',
            nombre: 'Seguridad Social a cargo de la empresa',
            debe: 72_795,
            haber: 0,
          },
          {
            cuenta: '4751',
            nombre: 'H. P., acreedora por retenciones practicadas',
            debe: 0,
            haber: 24_000,
          },
          {
            cuenta: '476',
            nombre: 'Organismos de la Seguridad Social, acreedores',
            debe: 0,
            haber: 87_745,
          },
          { cuenta: '465', nombre: 'Remuneraciones pendientes de pago', debe: 0, haber: 161_050 },
        ],
    pago: null,
    avisos: [
      'Plazo de ingreso: hasta el 30/11/2026 (último día del mes siguiente, art. 56 RD 1415/2004). Fuera de plazo hay recargo (art. 30 TRLGSS).',
      'TaxPain no envía nada al Sistema RED ni a SILTRA: compara este total con el de la liquidación de cuotas (RLC) de la TGSS, que es el que se carga en el banco.',
    ],
  };
}

const tabla: TablaCotizacion = {
  year: 2026,
  base_maxima: 510_120,
  bases_minimas: [],
  norma: nomina.norma,
  pendiente: [],
};

const finiquito: Finiquito = {
  lineas: [
    {
      concepto: 'Salario de los días trabajados del mes',
      importe: 150_000,
      cotiza: true,
      detalle: '30 días × salario mensual / 30',
    },
    {
      concepto: 'Vacaciones devengadas y no disfrutadas',
      importe: 49_400,
      cotiza: true,
      detalle: '9,88 días × salario mensual / 30',
    },
  ],
  sujeto: 199_400,
  indemnizacion: 0,
  total: 199_400,
  vacaciones_pendientes_centesimas: 988,
  avisos: [],
};

export const nominasHandlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  employees: () => [...((autonomoHandlers.employees?.() as Employee[]) ?? []), trabajadorDemo],
  nomina_ficha: (a) => (Number(a?.employeeId) === 2 ? ficha : null),
  nomina_ficha_save: () => null,
  nomina_tabla: () => tabla,
  nomina_calcular: () => nomina,
  nomina_registrar: () => 50,
  nominas_calculadas: () => [],
  nomina_recibo_pdf: () => '/Users/demo/Documents/Recibo de salarios 00000000T 2026-10.pdf',
  seguros_sociales: (a) => seguros(Number(a?.year ?? 2026), Number(a?.month ?? 10)),
  seguros_sociales_pagar: () => 51,
  finiquito_calcular: () => finiquito,
};

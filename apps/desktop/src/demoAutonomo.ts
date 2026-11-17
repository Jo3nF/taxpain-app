// Modo demo de Nóminas → Autónomo societario: datos inventados de Carmen Española Española,
// administradora de la sociedad en el RETA (base de 1.000 €, cuota pagada por la sociedad).

import type { Employee, ResumenReta, StoredPayslip, TramoReta } from './api';

const carmen: Employee = {
  id: 1,
  nif: '12345678Z',
  name: 'Española Española Carmen',
  province: '46',
  birth_year: 0,
  family: 3,
  spouse_nif: null,
  disability: 0,
  contract: 1,
  mobility: false,
  children_under3: 0,
  children_rest: 0,
  children_whole: false,
  active: true,
  administrator: true,
  ss_regime: 'Autonomo',
};

function slips(year: number): StoredPayslip[] {
  return Array.from({ length: 10 }, (_, i) => ({
    id: i + 1,
    employee_id: 1,
    employee: carmen.name,
    nif: carmen.nif,
    date: `${year}-${String(i + 1).padStart(2, '0')}-28`,
    gross: 200_000,
    irpf: 38_000,
    ss_employee: 0,
    ss_company: 0,
    net: 162_000,
    entry_id: 100 + i,
    director_subkey: 3,
  }));
}

const t = (
  tabla: 'Reducida' | 'General',
  numero: number,
  desde: number,
  hasta: number | null,
  base_min: number,
  base_max: number,
): TramoReta => ({ tabla, numero, desde, hasta, base_min, base_max });

const TRAMOS: TramoReta[] = [
  t('Reducida', 1, 0, 67_000, 65_359, 71_894),
  t('Reducida', 2, 67_000, 90_000, 71_895, 90_000),
  t('Reducida', 3, 90_000, 116_669, 84_967, 116_670),
  t('General', 1, 116_669, 130_000, 95_098, 130_000),
  t('General', 2, 130_000, 150_000, 96_078, 150_000),
  t('General', 3, 150_000, 170_000, 96_078, 170_000),
  t('General', 4, 170_000, 185_000, 114_379, 185_000),
  t('General', 5, 185_000, 203_000, 120_915, 203_000),
  t('General', 6, 203_000, 233_000, 127_451, 233_000),
  t('General', 7, 233_000, 276_000, 135_621, 276_000),
  t('General', 8, 276_000, 319_000, 143_791, 319_000),
  t('General', 9, 319_000, 362_000, 151_961, 362_000),
  t('General', 10, 362_000, 405_000, 160_131, 405_000),
  t('General', 11, 405_000, 600_000, 173_203, 510_120),
  t('General', 12, 600_000, null, 192_810, 510_120),
];

function resumen(year: number): ResumenReta {
  const general7 = TRAMOS[9] as TramoReta;
  return {
    employee_id: 1,
    name: carmen.name,
    nif: carmen.nif,
    ficha: {
      employee_id: 1,
      year,
      base_elegida: 100_000,
      rendimiento_previsto: 120_000,
      meses_alta: 12,
      alta_societario: null,
      control_efectivo: true,
      pct_dividendos: 10_000,
      otros_rendimientos: 0,
      gastos_deducibles: 0,
      pagador: 'Sociedad',
      cuenta_gasto: '642',
      cuenta_tesoreria: '5720001',
      estatutos_retribuido: true,
      acuerdo_fecha: `${year - 1}-06-30`,
      acuerdo_importe: 3_000_000,
      acuerdo_nota: 'Junta ordinaria',
      guardada: true,
    },
    tabla: {
      year,
      tramos: TRAMOS,
      base_min_societarios: 142_440,
      tipo: 3150,
      norma:
        'RDL 3/2026, art. 3 (BOE-A-2026-2548); Orden PJC/297/2026, arts. 3, 18 y 37 (BOE-A-2026-7296)',
      pendiente:
        year > 2026
          ? ['la tabla del año: sin orden de cotización publicada, se usa la de 2026']
          : [],
    },
    dinerarias: 2_000_000,
    especie: 315_000,
    dividendos: 600_000,
    rendimiento: {
      dividendos_computados: 600_000,
      neto: 2_915_000,
      deduccion_pct: 300,
      deduccion: 87_450,
      computable: 2_827_550,
      mensual: 235_629,
    },
    dias_alta_societario: 365,
    noventa_dias: true,
    tramo_previsto: TRAMOS[3] as TramoReta,
    regularizacion: {
      tramo: general7,
      tramo_nombre: 'General 7',
      base_min: 142_440,
      base_max: 276_000,
      base_elegida: 100_000,
      base_correcta: 142_440,
      cuota_pagada: 378_000,
      cuota_correcta: 538_428,
      importe: 160_428,
      sentido: 'AIngresar',
      meses: 12,
    },
    base_recomendada: 142_440,
    cuota_mensual: 31_500,
    cuotas: Array.from({ length: 10 }, (_, i) => ({
      id: i + 1,
      employee_id: 1,
      date: `${year}-${String(i + 1).padStart(2, '0')}-28`,
      amount: 31_500,
      pagador: 'Sociedad' as const,
      entry_id: 200 + i,
    })),
    cuotas_total: 315_000,
    tipo_retencion: 1900,
    ingreso_a_cuenta: 59_850,
    retribucion_total: 2_315_000,
    acuerdo: {
      estatutos_retribuido: true,
      fecha: `${year - 1}-06-30`,
      importe_maximo: 3_000_000,
      fuente: 'ficha del RETA',
    },
    ventana_abierta: { desde: '2026-09-01', hasta: '2026-10-31', efectos: '2026-11-01' },
    proximas_ventanas: [
      { desde: '2026-09-01', hasta: '2026-10-31', efectos: '2026-11-01' },
      { desde: '2026-11-01', hasta: '2026-12-31', efectos: '2027-01-01' },
      { desde: '2027-01-01', hasta: '2027-02-28', efectos: '2027-03-01' },
    ],
    avisos: [
      'El rendimiento previsto (tramo General 1) no coincide con el que sale de TaxPain (tramo General 7): puedes cambiarlo hasta el 31/10/2026 con efectos del 01/11/2026',
      'Con estos rendimientos la TGSS reclamará unos 1.604,28 € al regularizar 2026 (la base tendría que ser de 1.424,40 € como poco)',
      'TaxPain todavía no lleva la retribución en especie (la cuota que paga la sociedad) ni su ingreso a cuenta al 111 (casillas 04 a 06) ni al 190: añádelos al revisar los borradores',
    ],
  };
}

export const autonomoHandlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  employees: () => [carmen],
  payslips: (a) => slips(Number(a?.year ?? 2026)),
  reta_resumen: (a) => resumen(Number(a?.year ?? 2026)),
  reta_ficha_save: () => null,
  reta_cuota_register: () => 11,
  reta_cuota_delete: () => null,
  admin_receipt_pdf: () => '/Users/demo/Documents/Recibo administrador 12345678Z 2026-10.pdf',
};

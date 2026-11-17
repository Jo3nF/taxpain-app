// Modo demo del modelo de cuentas, el ECPN y los textos de la memoria: ESTUDIO DEMO SL (datos
// inventados, importes redondos).

import type { AccountsModel, Ecpn, MemoriaTextos, ModeloAviso } from './api';

let elegido: AccountsModel = 'Pymes';
let textos: MemoriaTextos = {
  nota_fija: '',
  hechos_posteriores: '',
  honorarios_auditoria: null,
  formulacion: null,
  firmantes: [],
};

function aviso(year: number): ModeloAviso {
  const ej = (y: number, activo: number, cifra: number, plantilla: number) => ({
    year: y,
    con_datos: true,
    activo,
    cifra_negocios: cifra,
    plantilla_decimas: plantilla,
    cumple_abreviado: 3,
    cumple_pyg_abreviada: 3,
    cumple_micro: 3,
  });
  return {
    year,
    ejercicios: [ej(year, 12_000_000, 18_000_000, 20), ej(year - 1, 10_000_000, 15_000_000, 15)],
    recomendado: 'Pymes',
    puede_abreviado: true,
    puede_pyg_abreviada: true,
    puede_micro: true,
    elegido,
    explicacion: [
      'Reúne dos de las tres condiciones (activo hasta 4 millones, cifra de negocios hasta 8 millones y hasta 50 personas de media): puede formular el balance, el ECPN y la memoria abreviados (arts. 257 y 261 LSC) y aplicar el PGC de PYMES (RD 1515/2007), que es lo más sencillo.',
      'Puede aplicar los criterios específicos de microempresas (art. 4 RD 1515/2007: activo hasta 1 millón, cifra de negocios hasta 2 millones, hasta 10 personas).',
    ],
    aviso:
      elegido === 'Normal'
        ? 'Tienes elegido el modelo normal: es válido, pero por tamaño puedes usar el de PYMES o el abreviado, que piden menos información.'
        : null,
  };
}

const COLUMNAS = [
  'Capital',
  'Prima de emisión',
  'Reservas',
  'Participaciones propias',
  'Resultados de ejercicios anteriores',
  'Otras aportaciones de socios',
  'Resultado del ejercicio',
  'Dividendo a cuenta',
  'Otros instrumentos de patrimonio',
  'Ajustes por cambios de valor',
  'Subvenciones, donaciones y legados',
];

function fila(
  fila: Ecpn['filas'][number]['fila'],
  label: string,
  cap: number,
  res: number,
  resu: number,
) {
  const importes = COLUMNAS.map(() => 0);
  importes[0] = cap;
  importes[2] = res;
  importes[6] = resu;
  return { fila, label, importes, total: cap + res + resu };
}

function ecpn(year: number, model: AccountsModel): Ecpn {
  return {
    year,
    model,
    obligatorio: model !== 'Pymes',
    nota:
      model === 'Pymes'
        ? 'Voluntario en el PGC de PYMES desde el RD 602/2016: puedes incluirlo en las cuentas si lo quieres.'
        : 'Obligatorio con este modelo.',
    ingresos_gastos: [
      { label: 'A) Resultado de la cuenta de pérdidas y ganancias', amount: 2_000_000 },
      {
        label: 'B) Total ingresos y gastos imputados directamente en el patrimonio neto',
        amount: 0,
      },
      { label: 'C) Total transferencias a la cuenta de pérdidas y ganancias', amount: 0 },
      { label: 'TOTAL DE INGRESOS Y GASTOS RECONOCIDOS (A + B + C)', amount: 2_000_000 },
    ],
    columnas: COLUMNAS,
    filas: [
      fila('SaldoInicial', `Saldo al inicio del ejercicio ${year}`, 300_000, 500_000, 1_500_000),
      fila('IngresosGastos', 'I. Total ingresos y gastos reconocidos', 0, 0, 2_000_000),
      fila('Dividendos', 'II.3. Distribución de dividendos', 0, 0, -500_000),
      fila(
        'OtrasVariaciones',
        'III. Otras variaciones del patrimonio neto',
        0,
        1_000_000,
        -1_000_000,
      ),
      fila('SaldoFinal', `Saldo al final del ejercicio ${year}`, 300_000, 1_500_000, 2_000_000),
    ],
  };
}

export const cuentasModeloHandlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  cuentas_modelo_aviso: (args) => aviso(Number(args?.year ?? 2026)),
  cuentas_modelo_elegir: (args) => {
    elegido = (args?.model as AccountsModel) ?? 'Pymes';
    return null;
  },
  ecpn: (args) => ecpn(Number(args?.year ?? 2026), (args?.model as AccountsModel) ?? 'Pymes'),
  ecpn_save: () => null,
  memoria_textos: () => textos,
  memoria_textos_guardar: (args) => {
    textos = (args?.textos as MemoriaTextos) ?? textos;
    return null;
  },
};

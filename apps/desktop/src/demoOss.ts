// Modo demo de la ventanilla única (OSS): datos inventados de ESTUDIO DEMO SL, que vende cursos
// en línea a particulares de Francia y Alemania (Marie Exemple y Hans Beispiel, clientes
// inventados). Pasa de 10.000 € en septiembre de 2026 y desde entonces factura con el IVA del
// país del cliente.

import type {
  AjustesOss,
  Borrador369,
  ClaseOss,
  EstadoOss,
  OperacionOss,
  TiposOss,
  VentaUe,
} from './api';

const TABLA: [string, string, number][] = [
  ['AT', 'Austria', 2000],
  ['BE', 'Bélgica', 2100],
  ['BG', 'Bulgaria', 2000],
  ['CY', 'Chipre', 1900],
  ['CZ', 'Chequia', 2100],
  ['DE', 'Alemania', 1900],
  ['DK', 'Dinamarca', 2500],
  ['EE', 'Estonia', 2400],
  ['EL', 'Grecia', 2400],
  ['FI', 'Finlandia', 2550],
  ['FR', 'Francia', 2000],
  ['HR', 'Croacia', 2500],
  ['HU', 'Hungría', 2700],
  ['IE', 'Irlanda', 2300],
  ['IT', 'Italia', 2200],
  ['LT', 'Lituania', 2100],
  ['LU', 'Luxemburgo', 1700],
  ['LV', 'Letonia', 2100],
  ['MT', 'Malta', 1800],
  ['NL', 'Países Bajos', 2100],
  ['PL', 'Polonia', 2300],
  ['PT', 'Portugal', 2300],
  ['RO', 'Rumanía', 2100],
  ['SE', 'Suecia', 2500],
  ['SI', 'Eslovenia', 2200],
  ['SK', 'Eslovaquia', 2300],
  ['XI', 'Irlanda del Norte', 2000],
];

const confirmados: Record<string, number> = { FR: 2000, DE: 1900 };

let ajustes: AjustesOss = { alta_035: '2026-09-01', opcion_destino: false, otras_ventas: {} };

const clases: Record<number, ClaseOss> = { 901: 'servicios', 902: 'servicios', 903: 'otro' };

function ventas(year: number): VentaUe[] {
  if (year !== 2026) return [];
  const v = (
    id: number,
    number: string,
    date: string,
    cliente: string,
    pais: string,
    pais_nombre: string,
    base: number,
    iva: number,
    oss: VentaUe['oss'],
  ): VentaUe => {
    const clase = oss ? (oss.supply === 'Bienes' ? 'bienes' : 'servicios') : (clases[id] ?? null);
    return {
      invoice_id: id,
      number,
      date,
      cliente,
      pais,
      pais_nombre,
      base,
      iva_espanol: iva,
      oss,
      clase,
      clase_fija: oss !== null,
      cuenta: clase === 'servicios' || clase === 'bienes',
      rectificativa: false,
      rectifica: null,
    };
  };
  return [
    v(901, 'A2026-0021', '2026-03-10', 'Marie Exemple', 'FR', 'Francia', 600_000, 126_000, null),
    v(902, 'A2026-0034', '2026-05-12', 'Hans Beispiel', 'DE', 'Alemania', 400_000, 84_000, null),
    v(903, 'A2026-0040', '2026-06-01', 'Marie Exemple', 'FR', 'Francia', 30_000, 6_300, null),
    v(904, 'A2026-0051', '2026-09-01', 'Marie Exemple', 'FR', 'Francia', 100_000, 0, {
      country: 'FR',
      supply: 'Servicios',
      rate: 2000,
      base: 100_000,
      vat: 20_000,
    }),
    v(905, 'A2026-0057', '2026-10-05', 'Hans Beispiel', 'DE', 'Alemania', 50_000, 0, {
      country: 'DE',
      supply: 'Servicios',
      rate: 1900,
      base: 50_000,
      vat: 9_500,
    }),
  ];
}

function estado(year: number): EstadoOss {
  const vs = ventas(year);
  const en_curso = vs.filter((x) => x.cuenta).reduce((a, x) => a + x.base, 0);
  const destino = year === 2026 ? '2026-09-01' : null;
  return {
    umbral: {
      year,
      anterior: 0,
      en_curso,
      superado_anterior: false,
      superado_el: destino,
      destino_desde: destino,
    },
    limite: 1_000_000,
    ajustes,
    tributa_en_destino: destino !== null,
    destino_desde: destino,
    sin_clasificar: vs.filter((x) => x.clase === null).length,
    sin_clasificar_base: 0,
    con_iva_espanol: [],
    falta_035: destino !== null && !ajustes.alta_035,
    avisos: destino
      ? [
          'Pasaste de 10.000 € el 2026-09-01: esa venta y las siguientes tributan en el país del cliente (art. 73 LIVA).',
        ]
      : [],
  };
}

function libro(year: number, quarter: number): OperacionOss[] {
  return ventas(year)
    .filter((x) => x.oss && Math.floor((Number(x.date.slice(5, 7)) - 1) / 3) + 1 === quarter)
    .map((x) => ({
      invoice_id: x.invoice_id,
      number: x.number,
      date: x.date,
      cliente: x.cliente,
      pais: x.pais,
      pais_nombre: x.pais_nombre,
      supply: x.oss?.supply ?? 'Servicios',
      rate: x.oss?.rate ?? 0,
      base: x.oss?.base ?? 0,
      cuota: x.oss?.vat ?? 0,
      corrige: null,
    }));
}

const VENCE: Record<number, string> = {
  1: '04-30',
  2: '07-31',
  3: '10-31',
  4: '01-31',
};

function borrador(year: number, quarter: number): Borrador369 {
  const ops = libro(year, quarter);
  const lineas = ops.map((o) => ({
    pais: o.pais,
    pais_nombre: o.pais_nombre,
    supply: o.supply,
    rate: o.rate,
    base: o.base,
    cuota: o.cuota,
  }));
  const total = lineas.reduce((a, l) => a + l.cuota, 0);
  const cuenta: Record<string, string> = { FR: '4779011', DE: '4779006' };
  return {
    year,
    quarter,
    vencimiento: `${quarter === 4 ? year + 1 : year}-${VENCE[quarter]}`,
    lineas,
    correcciones: [],
    total,
    por_pais: lineas.map((l) => [l.pais, l.cuota]),
    asiento:
      total === 0
        ? []
        : [
            ...lineas.map((l) => ({
              cuenta: cuenta[l.pais] ?? '4779',
              concepto: `IVA ${l.pais_nombre} (${l.pais}) del 369 ${quarter}T ${year}`,
              debe: l.cuota,
              haber: 0,
            })),
            {
              cuenta: '572',
              concepto: `Pago del 369 ${quarter}T ${year}`,
              debe: 0,
              haber: total,
            },
          ],
    avisos: [
      'No hay diseño de registro del 369 en TaxPain: pasa estas cifras al formulario de la Sede (régimen de la Unión).',
    ],
  };
}

export const ossHandlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  oss_estado: (args) => estado(Number(args?.year ?? 2026)),
  oss_ventas: (args) => ventas(Number(args?.year ?? 2026)),
  oss_clasificar: (args) => {
    const id = Number(args?.invoiceId);
    const c = args?.clase as ClaseOss | null;
    if (c) clases[id] = c;
    else delete clases[id];
    return null;
  },
  oss_tipos: (): TiposOss => ({
    fecha: '2025-08-01',
    fuente:
      'Comisión Europea, «VAT rates applied in the Member States of the European Union» y base de datos TEDB (taxation-customs.ec.europa.eu)',
    paises: TABLA.map(([pais, nombre, bp]) => ({
      pais,
      nombre,
      tabla: bp,
      confirmado: confirmados[pais] ?? null,
      confirmado_at: confirmados[pais] ? '2026-09-01T09:00:00Z' : null,
    })),
  }),
  oss_confirmar_tipo: (args) => {
    confirmados[String(args?.pais)] = Number(args?.tipoBp);
    return null;
  },
  oss_ajustes_guardar: (args) => {
    ajustes = args?.ajustes as AjustesOss;
    return null;
  },
  oss_libro: (args) => libro(Number(args?.year ?? 2026), Number(args?.quarter ?? 1)),
  oss_369: (args) => borrador(Number(args?.year ?? 2026), Number(args?.quarter ?? 1)),
  oss_369_hoja: (args) =>
    `/Users/demo/Documents/369_${Number(args?.year ?? 2026)}_${Number(args?.quarter ?? 1)}T.xlsx`,
};

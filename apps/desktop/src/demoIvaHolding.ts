// Modo demo de IVA de holding y cartera: datos inventados de ESTUDIO DEMO SL (servicios a su
// participada, Participada Demo SL, con un NIF de prueba que no puede ser de nadie; préstamo a
// la participada y un local comprado en 2023 y vendido exento en 2026).

import type {
  AplicacionRegla,
  ComprobacionSectores,
  DiagnosticoHolding,
  LibroBienesInversion,
  ReglaAfectacion,
  SectorIva,
} from './api';

let reglas: ReglaAfectacion[] = [
  {
    id: 1,
    ambito: 'cuenta',
    clave: '623',
    pct: 82,
    sector_id: null,
    criterio:
      'Proporción de 2025: operaciones sujetas (servicios e intereses) sobre el total de ingresos con los dividendos (art. 93.Cinco LIVA).',
    desde: '2026-01-01',
  },
  {
    id: 2,
    ambito: 'proveedor',
    clave: 'A22222228',
    pct: 0,
    sector_id: null,
    criterio: 'Custodia y gestión de la cartera de participaciones: sólo la tenencia.',
    desde: '2026-01-01',
  },
];
let sectores: SectorIva[] = [
  {
    id: 1,
    nombre: 'Servicios de gestión a filiales',
    cnae: '7010',
    pct: 100,
    volumen: 18_000_000,
    notas: '',
  },
  {
    id: 2,
    nombre: 'Financiación al grupo',
    cnae: '6499',
    pct: 0,
    volumen: 4_000_000,
    notas: '',
  },
];
let separar = false;

function grupo(cnae: string) {
  return cnae.replace(/\D/g, '').slice(0, 3);
}

/** La misma regla que `comprobar_sectores` en Rust. */
function comprobar(lista: SectorIva[]): ComprobacionSectores {
  if (lista.length === 0)
    return { filas: [], hay_diferenciados: false, explicacion: 'No hay actividades anotadas.' };
  const p = lista.reduce((a, b) => (b.volumen > a.volumen ? b : a));
  const filas = lista.map((s) => {
    const diferencia = Math.abs(s.pct - p.pct);
    const g = grupo(s.cnae);
    const papel =
      s.id === p.id
        ? ('principal' as const)
        : g.length < 3 || grupo(p.cnae).length < 3
          ? ('sin_cnae' as const)
          : g === grupo(p.cnae) || diferencia <= 50
            ? ('mismo_sector' as const)
            : s.volumen * 100 <= p.volumen * 15
              ? ('accesoria' as const)
              : ('diferenciado' as const);
    return { ...s, grupo: g, diferencia, papel };
  });
  const hay = filas.some((f) => f.papel === 'diferenciado');
  return {
    filas,
    hay_diferenciados: hay,
    explicacion: hay
      ? `Hay actividades de otro grupo de la CNAE con un porcentaje de deducción a más de 50 puntos del de la principal («${p.nombre}», ${p.pct} %): forman otro sector diferenciado y deducen por separado (arts. 9.1.º.c.a' y 101 LIVA).`
      : `Con lo anotado no hay sectores diferenciados: todas las actividades siguen a la principal («${p.nombre}», ${p.pct} %) y se aplica un solo porcentaje.`,
  };
}

const aplicaciones = (): AplicacionRegla[] => [
  {
    recibida_id: 41,
    numero: 'AS-2026-07',
    proveedor: 'Asesoría Demo SL',
    nif: 'B12345674',
    registro: '2026-07-15',
    cuota: 63_000,
    pct_factura: 100,
    pct_aplicado: reglas.find((r) => r.id === 1)?.pct ?? 100,
    regla_id: 1,
    criterio: reglas.find((r) => r.id === 1)?.criterio ?? '',
    sector: null,
    no_deducido: Math.round((63_000 * (100 - (reglas.find((r) => r.id === 1)?.pct ?? 100))) / 100),
  },
  {
    recibida_id: 42,
    numero: 'CV-118',
    proveedor: 'Gestora Demo SA',
    nif: 'A22222228',
    registro: '2026-09-30',
    cuota: 42_000,
    pct_factura: 100,
    pct_aplicado: 0,
    regla_id: 2,
    criterio: 'Custodia y gestión de la cartera de participaciones: sólo la tenencia.',
    sector: null,
    no_deducido: 42_000,
  },
];

function informe(year: number): DiagnosticoHolding {
  const comprobacion = comprobar(sectores);
  const ap = reglas.length ? aplicaciones() : [];
  return {
    year,
    perfil: 'holding_mixta',
    perfil_texto:
      'Parece una holding mixta: tiene participaciones y factura servicios a sus participadas. Los servicios dan derecho a deducir; la tenencia, no. Los gastos comunes se reparten con un criterio razonable mantenido en el tiempo.',
    datos: {
      participadas: 1,
      coste_participadas: 15_000_000,
      dividendos: 4_000_000,
      financieros: 300_000,
      prestamos_participadas: 10_000_000,
      intereses_participadas: 300_000,
      ingresos_actividad: 18_000_000,
      servicios_participadas: 6_000_000,
    },
    senales: [
      {
        clave: 'participadas',
        titulo: 'Participadas',
        importe: 15_000_000,
        detalle: '1 en la ficha de Vinculadas (coste)',
      },
      {
        clave: 'dividendos',
        titulo: 'Dividendos (760)',
        importe: 4_000_000,
        detalle: 'No sujetos: fuera de la prorrata',
      },
      {
        clave: 'prestamos',
        titulo: 'Préstamos a participadas',
        importe: 10_000_000,
        detalle: 'Saldo vivo al cierre; intereses del año 3.000,00 €',
      },
      {
        clave: 'financieros',
        titulo: 'Ingresos financieros (761, 762, 769)',
        importe: 300_000,
        detalle: 'Operaciones financieras exentas',
      },
      {
        clave: 'servicios',
        titulo: 'Servicios facturados a participadas',
        importe: 6_000_000,
        detalle: 'Facturas emitidas a participadas y al grupo',
      },
      {
        clave: 'actividad',
        titulo: 'Ventas y servicios (70)',
        importe: 18_000_000,
        detalle: 'Ingresos de la actividad',
      },
    ],
    notas: [
      {
        titulo: 'Dividendos y tenencia de participaciones',
        texto:
          'Cobrar dividendos o tener participaciones no es una entrega de bienes ni una prestación de servicios: no lleva IVA, no entra en la prorrata y el IVA de los gastos destinados sólo a la tenencia no se deduce.',
        norma: 'Arts. 4, 5, 94.Uno y 104.Tres.5.º LIVA',
        por_cotejar: false,
      },
      {
        titulo: 'Gastos comunes a lo sujeto y lo no sujeto',
        texto:
          'Los gastos que sirven a la vez a la actividad y a la tenencia se deducen en la parte que corresponde a la actividad sujeta, con un criterio razonable y homogéneo que se mantiene en el tiempo.',
        norma:
          'Art. 93.Cinco LIVA (por cotejar: se refiere a las operaciones del art. 7.8.º; la DGT y el TJUE lo extienden a las holdings mixtas, STJUE Larentia + Minerva C-108/14)',
        por_cotejar: true,
      },
      {
        titulo: 'Préstamos e intereses de las participadas',
        texto:
          'Prestar dinero es una operación financiera exenta. Si no es tu actividad habitual, sus intereses no entran en la prorrata; si lo es, van al denominador y bajan el porcentaje de deducción.',
        norma: 'Arts. 20.Uno.18.º y 104.Tres.4.º LIVA',
        por_cotejar: false,
      },
    ],
    prorrata_facturas: 100,
    prorrata_con_financieros: 99,
    proporcion_93: 82,
    prorrata_configurada: null,
    prorrata_especial: false,
    config: { sectores_diferenciados: separar },
    reglas,
    sectores,
    comprobacion,
    aplicaciones: ap,
    no_deducido: ap.reduce((s, a) => s + a.no_deducido, 0),
    avisos:
      comprobacion.hay_diferenciados && !separar
        ? [
            'Con las actividades anotadas hay sectores diferenciados y la deducción no se separa: márcalo si lo aplicas y lleva las compras de cada sector con una regla.',
          ]
        : [],
  };
}

function libro(year: number): LibroBienesInversion {
  return {
    year,
    con_prorrata: false,
    bienes: [
      {
        asset_id: 1,
        received_id: 7,
        nombre: 'Equipos Demo SL · EQ-2024-3',
        factura: 'EQ-2024-3',
        nif: 'B12345674',
        fecha: '2024-04-10',
        inmueble: false,
        base: 600_000,
        cuota: 126_000,
        pct_factura: 100,
        prorrata_compra: 100,
        deduccion_inicial: 126_000,
        fin_periodo: 2028,
        anios: [2025, 2026]
          .filter((y) => y <= year)
          .map((y) => ({
            year: y,
            prorrata: null,
            diferencia: null,
            importe: 0,
            nota: 'Sin prorrata: no hay regularización anual',
          })),
        venta: null,
        avisos: [],
      },
      {
        asset_id: 2,
        received_id: 3,
        nombre: 'Local de la calle Mayor',
        factura: 'INM-2023-1',
        nif: 'A22222228',
        fecha: '2023-02-01',
        inmueble: true,
        base: 10_000_000,
        cuota: 2_100_000,
        pct_factura: 100,
        prorrata_compra: 100,
        deduccion_inicial: 2_100_000,
        fin_periodo: 2032,
        anios: [
          {
            year: 2024,
            prorrata: null,
            diferencia: null,
            importe: 0,
            nota: 'Sin prorrata: no hay regularización anual',
          },
          {
            year: 2025,
            prorrata: null,
            diferencia: null,
            importe: 0,
            nota: 'Sin prorrata: no hay regularización anual',
          },
          {
            year: 2026,
            prorrata: null,
            diferencia: null,
            importe: 0,
            nota: 'Entregado: se regulariza de una vez por la entrega',
          },
        ],
        venta: {
          fecha: '2026-06-15',
          tipo: 'exenta',
          iva: 0,
          regularizacion: -1_470_000,
          anios_restantes: 7,
          origen: 'factura de venta',
        },
        avisos: [],
      },
    ],
    total_anual: 0,
    total_ventas: year === 2026 ? -1_470_000 : 0,
    sin_datos: [
      { asset_id: 3, nombre: 'Mobiliario de oficina', cuenta: '216', adquirido: '2022-09-01' },
    ],
    avisos: [
      '1 elemento(s) del inmovilizado sin datos de IVA: si se dedujo su IVA, anota la cuota para seguir su regularización.',
    ],
  };
}

export const ivaHoldingHandlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  iva_holding_informe: (args) => informe(Number(args?.year ?? 2026)),
  iva_holding_libro: (args) => libro(Number(args?.year ?? 2026)),
  iva_holding_regla_guardar: (args) => {
    const r = args?.regla as ReglaAfectacion;
    if (r.id) {
      reglas = reglas.map((x) => (x.id === r.id ? r : x));
      return r.id;
    }
    const id = Math.max(0, ...reglas.map((x) => x.id)) + 1;
    reglas = [...reglas, { ...r, id }];
    return id;
  },
  iva_holding_regla_borrar: (args) => {
    reglas = reglas.filter((x) => x.id !== Number(args?.id));
  },
  iva_holding_sector_guardar: (args) => {
    const s = args?.sector as SectorIva;
    if (s.id) {
      sectores = sectores.map((x) => (x.id === s.id ? s : x));
      return s.id;
    }
    const id = Math.max(0, ...sectores.map((x) => x.id)) + 1;
    sectores = [...sectores, { ...s, id }];
    return id;
  },
  iva_holding_sector_borrar: (args) => {
    sectores = sectores.filter((x) => x.id !== Number(args?.id));
  },
  iva_holding_config: (args) => {
    separar = Boolean(
      (args?.config as { sectores_diferenciados: boolean })?.sectores_diferenciados,
    );
  },
  iva_holding_bien_guardar: () => undefined,
};

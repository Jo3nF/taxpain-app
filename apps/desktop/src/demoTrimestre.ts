// Modo demo del cierre de trimestre, de la guía «Dejar la gestoría» y de las obligaciones no
// fiscales: datos inventados de ESTUDIO DEMO SL (B12345674). Ningún importe es real.

import type {
  CierreTrimestre,
  EstadoObligacion,
  EstadoPasoGuia,
  Guia,
  MarcaTrimestre,
  Obligaciones,
  PasoTrimestre,
  ResumenTrimestre,
} from './trimestreApi';

const marcas = new Map<string, MarcaTrimestre>();

function pasos(quarter: number): PasoTrimestre[] {
  const pendiente = quarter === 4;
  const base: PasoTrimestre[] = [
    {
      id: 'recibidas',
      label: 'Facturas recibidas revisadas',
      estado: pendiente ? 'pendiente' : 'ok',
      detalle: pendiente
        ? 'Quedan 2 por revisar del trimestre (o sin fecha): sin revisar no cuentan en el 303 ni en el diario.'
        : 'Todas las del trimestre están revisadas.',
      destino: { tab: 'recibidas', view: null },
      manual: false,
      marca: null,
    },
    {
      id: 'cobros',
      label: 'Facturas emitidas vencidas cobradas',
      estado: 'aviso',
      detalle:
        '1 factura vencida sin cobrar (1.210,00 €). El IVA se declara igual aunque no se haya cobrado; si ya se cobró, concílialo con el banco.',
      destino: { tab: 'contabilidad', view: 'cartera' },
      manual: true,
      marca: null,
    },
    {
      id: 'banco',
      label: 'Banco conciliado',
      estado: pendiente ? 'pendiente' : 'ok',
      detalle: pendiente
        ? '3 movimientos sin conciliar.'
        : 'Todos los movimientos del trimestre tienen asiento.',
      destino: { tab: 'contabilidad', view: 'banco' },
      manual: false,
      marca: null,
    },
    {
      id: 'nominas',
      label: 'Nóminas del trimestre',
      estado: 'no_aplica',
      detalle: 'Sin empleados en Nóminas.',
      destino: { tab: 'contabilidad', view: 'nominas' },
      manual: true,
      marca: null,
    },
    {
      id: 'reta',
      label: 'Cuotas del RETA',
      estado: 'ok',
      detalle: 'Registrada la cuota de cada mes terminado.',
      destino: { tab: 'contabilidad', view: 'nominas' },
      manual: true,
      marca: null,
    },
    {
      id: 'periodicas',
      label: 'Facturas periódicas emitidas',
      estado: 'ok',
      detalle: 'Ninguna plantilla con una factura por emitir del trimestre.',
      destino: { tab: 'facturas', view: null },
      manual: true,
      marca: null,
    },
    {
      id: 'amortizaciones',
      label: 'Amortizaciones',
      estado: 'no_aplica',
      detalle:
        'TaxPain las contabiliza una vez al año, al cierre del ejercicio; no cambian los modelos del trimestre.',
      destino: { tab: 'contabilidad', view: 'inmovilizado' },
      manual: false,
      marca: null,
    },
    {
      id: 'prestamos',
      label: 'Cuotas de los préstamos',
      estado: 'ok',
      detalle: 'Registradas las cuotas vencidas del trimestre.',
      destino: { tab: 'contabilidad', view: 'prestamos' },
      manual: true,
      marca: null,
    },
    {
      id: 'liquidacion_iva',
      label: 'Liquidación del IVA contabilizada',
      estado: quarter < 3 ? 'ok' : 'pendiente',
      detalle:
        quarter < 3
          ? 'Hay asiento de liquidación del trimestre (477/472 a la 4750 o 4700).'
          : 'Cuando el 303 esté bien, contabiliza su liquidación (Contabilidad → IVA y libros): salda la 472 y la 477 del trimestre.',
      destino: { tab: 'contabilidad', view: 'impuestos' },
      manual: false,
      marca: null,
    },
  ];
  return base.map((p) => {
    const m = marcas.get(`${quarter}-${p.id}`);
    return m ? { ...p, marca: m, estado: m === 'hecho' ? 'ok' : 'no_aplica' } : p;
  });
}

function cierre(year: number, quarter: number): CierreTrimestre {
  const presentado = quarter < 3;
  const ps = pasos(quarter);
  const cuadra = quarter !== 4;
  const listo =
    ps.filter((p) => p.id !== 'liquidacion_iva').every((p) => p.estado !== 'pendiente') && cuadra;
  const fin = ['03-31', '06-30', '09-30', '12-31'][quarter - 1];
  const plazo =
    quarter === 4 ? `${year + 1}-01-30` : `${year}-${['04', '07', '10'][quarter - 1]}-20`;
  const dom = quarter === 4 ? `${year + 1}-01-26` : `${year}-${['04', '07', '10'][quarter - 1]}-15`;
  return {
    year,
    quarter,
    start: `${year}-${String(quarter * 3 - 2).padStart(2, '0')}-01`,
    end: `${year}-${fin}`,
    pasos: ps,
    modelos: [
      {
        model: '303',
        period: `${quarter}T`,
        result: 210_000,
        plazo,
        domiciliacion: dom,
        presentado,
        cuadres: [
          {
            concepto: 'IVA devengado [27] con la 477 del diario',
            modelo: 420_000,
            diario: cuadra ? 420_000 : 399_000,
            ok: cuadra,
          },
          {
            concepto: 'IVA deducible [45] con la 472 del diario',
            modelo: 210_000,
            diario: 210_000,
            ok: true,
          },
        ],
        avisos: [],
        error: null,
      },
      {
        model: '111',
        period: `${quarter}T`,
        result: 45_000,
        plazo,
        domiciliacion: dom,
        presentado,
        cuadres: [
          {
            concepto: 'Retenciones [28] con la 4751 del diario (trabajo y profesionales)',
            modelo: 45_000,
            diario: 45_000,
            ok: true,
          },
        ],
        avisos: [],
        error: null,
      },
      {
        model: '115',
        period: `${quarter}T`,
        result: 57_000,
        plazo,
        domiciliacion: dom,
        presentado,
        cuadres: [],
        avisos: [],
        error: null,
      },
    ],
    a_ingresar: presentado ? 0 : 312_000,
    domiciliar_hasta: presentado ? null : dom,
    listo,
    presentado,
    sin_perfil: false,
  };
}

function resumen(year: number): ResumenTrimestre[] {
  return [1, 2, 3, 4].map((q) => {
    const c = cierre(year, q);
    return {
      quarter: q,
      listo: c.listo,
      presentado: c.presentado,
      pendientes:
        c.pasos.filter((p) => p.estado === 'pendiente').length +
        c.modelos.filter((m) => !m.presentado && m.cuadres.some((x) => !x.ok)).length,
      en_curso: q === 4,
      futuro: false,
    };
  });
}

const guiaEstado = new Map<string, { estado: EstadoPasoGuia; fecha: string | null; nota: string }>([
  ['empresa', { estado: 'hecho', fecha: null, nota: '' }],
  ['certificado', { estado: 'hecho', fecha: null, nota: '' }],
]);

const PASOS_GUIA: Omit<Guia['pasos'][number], 'estado' | 'fecha' | 'nota'>[] = [
  {
    id: 'empresa',
    titulo: 'Datos de la empresa y perfil fiscal',
    texto: 'Pon los datos de facturación y el perfil fiscal: qué modelos presentas y cada cuánto.',
    destino: 'ajustes',
    enlaces: [],
    detectado: 'Tienes los datos de facturación y el perfil fiscal.',
  },
  {
    id: 'certificado',
    titulo: 'Certificado electrónico',
    texto: 'Instala el certificado de representante de la sociedad.',
    destino: 'ajustes',
    enlaces: [],
    detectado: null,
  },
  {
    id: 'pedir',
    titulo: 'Pedir a la gestoría lo que es tuyo',
    texto: 'Copia el correo de abajo y mándaselo tú (TaxPain no envía nada).',
    destino: null,
    enlaces: [],
    detectado: null,
  },
  {
    id: 'carpeta',
    titulo: 'Importar su carpeta',
    texto: 'Junta todo en una carpeta e impórtala.',
    destino: 'importar',
    enlaces: [],
    detectado: 'Hay 42 documentos de la empresa y 12 modelos importados.',
  },
  {
    id: 'saldos',
    titulo: 'Fecha de corte y saldos iniciales',
    texto: 'Elige la fecha desde la que llevas tú la contabilidad y crea los saldos iniciales.',
    destino: 'contabilidad:apertura',
    enlaces: [],
    detectado: null,
  },
  {
    id: 'revocar',
    titulo: 'Revocar el apoderamiento y la representación',
    texto:
      'En la Sede de la AEAT, en «Apoderamientos», consulta los poderes que diste a la gestoría y revócalos.',
    destino: null,
    enlaces: [
      {
        label: 'Apoderamientos (Sede de la AEAT)',
        url: 'https://sede.agenciatributaria.gob.es/static_files/common/html/selector_acceso/SelectorAccesos.html?ref=%2Fwlpl%2FITTI-APOD%2FSvApodPoderQuery&aut=CPE',
      },
    ],
    detectado: null,
  },
  {
    id: 'calendario',
    titulo: 'Quedarte con el calendario',
    texto: 'Revisa el calendario de los próximos doce meses y activa los avisos.',
    destino: 'calendario',
    enlaces: [],
    detectado: 'El calendario ya sale de tu perfil fiscal.',
  },
];

function guia(): Guia {
  const pasos = PASOS_GUIA.map((p) => ({
    ...p,
    ...(guiaEstado.get(p.id) ?? { estado: 'pendiente' as const, fecha: null, nota: '' }),
  }));
  return {
    pasos,
    resueltos: pasos.filter((p) => p.estado !== 'pendiente').length,
    empezada: guiaEstado.size > 0,
    correo: {
      asunto: 'ESTUDIO DEMO SL: documentación para el traspaso de la contabilidad',
      cuerpo:
        'Hola:\n\nA partir de ahora llevaremos nosotros la contabilidad y los impuestos de ESTUDIO DEMO SL (NIF B12345674). Os pedimos, por favor:\n\n1. El libro diario y el mayor.\n2. Los libros registro de IVA.\n3. Los modelos presentados con sus justificantes.\n\nUn saludo.\n',
    },
  };
}

const obligEstado = new Map<
  string,
  { estado: EstadoObligacion; fecha: string | null; nota: string }
>([['deposito_cuentas', { estado: 'hecho', fecha: '2026-07-20', nota: '' }]]);

function obligaciones(): Obligaciones {
  const base = [
    {
      id: 'deposito_cuentas',
      titulo: 'Depósito de las cuentas anuales',
      que_es: 'Las cuentas que aprueba la junta se depositan en el Registro Mercantil.',
      a_quien: 'Todas las sociedades de capital (SL y SA).',
      plazo: 'Un mes desde la aprobación.',
      norma: 'Arts. 164 y 279 LSC (RDLeg 1/2010)',
      enlaces: [
        {
          label: 'Ley de Sociedades de Capital (BOE)',
          url: 'https://www.boe.es/buscar/act.php?id=BOE-A-2010-10544',
        },
      ],
      con_empleados: false,
    },
    {
      id: 'lssi_web',
      titulo: 'Web: aviso legal y cookies',
      que_es: 'La web ha de mostrar la razón social, el NIF, el domicilio y los datos registrales.',
      a_quien: 'Quien tenga una web.',
      plazo: 'Permanente.',
      norma: 'Arts. 10 y 22.2 Ley 34/2002 (LSSI)',
      enlaces: [
        {
          label: 'Ley 34/2002 (BOE)',
          url: 'https://www.boe.es/buscar/act.php?id=BOE-A-2002-13758',
        },
      ],
      con_empleados: false,
    },
    {
      id: 'registro_horario',
      titulo: 'Registro de la jornada',
      que_es: 'Registro diario de la hora de entrada y salida de cada trabajador.',
      a_quien: 'Quien tenga trabajadores por cuenta ajena.',
      plazo: 'Cada día.',
      norma: 'Art. 34.9 Estatuto de los Trabajadores',
      enlaces: [],
      con_empleados: true,
    },
  ];
  return {
    hay_empleados: false,
    lista: base.map((o) => ({
      ...o,
      ...(obligEstado.get(o.id) ?? { estado: 'sin_revisar' as const, fecha: null, nota: '' }),
    })),
  };
}

export const trimestreHandlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  cierre_trimestre: (a) => cierre(Number(a?.year ?? 2026), Number(a?.quarter ?? 4)),
  cierre_trimestre_resumen: (a) => resumen(Number(a?.year ?? 2026)),
  cierre_trimestre_marcar: (a) => {
    const key = `${a?.quarter}-${a?.paso}`;
    const m = a?.marca as MarcaTrimestre | null;
    if (m) marcas.set(key, m);
    else marcas.delete(key);
    return null;
  },
  puesta_marcha: () => guia(),
  puesta_marcha_marcar: (a) => {
    guiaEstado.set(String(a?.paso), {
      estado: a?.estado as EstadoPasoGuia,
      fecha: (a?.fecha as string | null) ?? null,
      nota: String(a?.nota ?? ''),
    });
    return null;
  },
  obligaciones_no_fiscales: () => obligaciones(),
  obligacion_no_fiscal_marcar: (a) => {
    obligEstado.set(String(a?.id), {
      estado: a?.estado as EstadoObligacion,
      fecha: (a?.fecha as string | null) ?? null,
      nota: String(a?.nota ?? ''),
    });
    return null;
  },
};

// Modo demo de los ajustes de cierre: datos inventados de ESTUDIO DEMO SL (un seguro anual, una
// suscripción, un cliente moroso con NIF de prueba y la asesoría de diciembre que llega en enero).

import type {
  Ajuste,
  Comprobacion,
  NuevoAjuste,
  PropuestaAjuste,
  ResumenAjustes,
} from './ajustesCierreApi';
import type { ProposedLine } from './api';

const NOMBRES: Record<string, string> = {
  '480': 'Gastos anticipados',
  '485': 'Ingresos anticipados',
  '4009': 'Proveedores, facturas pendientes de recibir o de formalizar',
  '4309': 'Clientes, facturas pendientes de formalizar',
  '490': 'Deterioro de valor de créditos por operaciones comerciales',
  '694': 'Pérdidas por deterioro de créditos por operaciones comerciales',
  '142': 'Provisión para otras responsabilidades',
  '625': 'Primas de seguros',
  '629': 'Otros servicios',
  '623': 'Servicios de profesionales independientes',
  '678': 'Gastos excepcionales',
  '705': 'Prestaciones de servicios',
};

const l = (account: string, debit: number, credit: number): ProposedLine => ({
  account,
  name: NOMBRES[account] ?? '',
  debit,
  credit,
});

const espejo = (ls: ProposedLine[]) => ls.map((x) => ({ ...x, debit: x.credit, credit: x.debit }));

let ajustes: Ajuste[] = [];
let siguiente = 1;

/** Días entre dos fechas «AAAA-MM-DD». */
const dias = (a: string, b: string) => Math.round((Date.parse(b) - Date.parse(a)) / 86_400_000);

const FACTURAS: Record<number, { importe: number; cuenta: string; numero: string }> = {
  11: { importe: 120_000, cuenta: '625', numero: 'POL-2026-77' },
  12: { importe: 36_000, cuenta: '629', numero: 'SUS-0098' },
  21: { importe: 240_000, cuenta: '705', numero: 'A2026-0031' },
};

function propuesta(year: number, n: NuevoAjuste): PropuestaAjuste {
  const cierre = `${year}-12-31`;
  const base = {
    tipo: n.tipo,
    fecha: cierre,
    reversion_fecha: null as string | null,
    reversion: [] as ProposedLine[],
    deducible: true,
    motivo_fiscal: '',
    avisos: [] as string[],
  };
  if (n.tipo === 'gasto_anticipado' || n.tipo === 'ingreso_anticipado') {
    const f = FACTURAS[n.factura_id ?? 0];
    if (!f || !n.desde || !n.hasta) throw { message: 'indica el periodo de servicio' };
    if (n.hasta <= cierre) throw { message: 'el servicio acaba antes del cierre' };
    const total = dias(n.desde, n.hasta) + 1;
    const despues = Math.min(total, dias(cierre, n.hasta));
    const importe = Math.round((f.importe * despues) / total);
    const lines =
      n.tipo === 'gasto_anticipado'
        ? [l('480', importe, 0), l(f.cuenta, 0, importe)]
        : [l(f.cuenta, importe, 0), l('485', 0, importe)];
    return {
      ...base,
      descripcion: `${f.numero}: ${despues} de ${total} días del servicio son del ejercicio siguiente`,
      importe,
      lines,
      reversion_fecha: `${year + 1}-01-01`,
      reversion: espejo(lines),
    };
  }
  if (n.tipo === 'pendiente_recibir' || n.tipo === 'pendiente_emitir') {
    const importe = n.importe ?? 0;
    if (importe <= 0) throw { message: 'indica un importe mayor que cero' };
    if (!n.descripcion?.trim()) throw { message: 'describe el gasto o el ingreso devengado' };
    const cuenta = n.cuenta ?? '629';
    const lines =
      n.tipo === 'pendiente_recibir'
        ? [l(cuenta, importe, 0), l('4009', 0, importe)]
        : [l('4309', importe, 0), l(cuenta, 0, importe)];
    return {
      ...base,
      descripcion: n.descripcion,
      importe,
      lines,
      reversion_fecha: `${year + 1}-01-01`,
      reversion: espejo(lines),
    };
  }
  if (n.tipo === 'deterioro') {
    const importe = n.importe ?? 72_600;
    const deducible = n.motivo !== 'ninguno' && (n.exclusion ?? 'ninguna') === 'ninguna';
    return {
      ...base,
      descripcion: 'Deterioro del cobro de la A2026-0004 de Cliente Moroso Inventado SA',
      importe,
      lines: [l('694', importe, 0), l('490', 0, importe)],
      deducible,
      motivo_fiscal: deducible
        ? 'Deducible: más de seis meses vencido (art. 13.1.a LIS).'
        : 'No deducible (art. 13.1 LIS).',
      avisos: [
        'Si recuperas el IVA de la factura con una rectificativa por crédito incobrable (art. 80.Cuatro LIVA), el deterioro es sólo por la base.',
      ],
    };
  }
  const importe = n.importe ?? 0;
  if (importe <= 0) throw { message: 'indica un importe mayor que cero' };
  if (!n.descripcion?.trim()) throw { message: 'describe el riesgo o el litigio que provisionas' };
  return {
    ...base,
    descripcion: n.descripcion,
    importe,
    lines: [l(n.cuenta ?? '678', importe, 0), l(n.contrapartida ?? '142', 0, importe)],
    deducible: n.deducible ?? false,
    motivo_fiscal: n.deducible
      ? 'Marcada como deducible (art. 14 LIS): guarda la justificación.'
      : 'No deducible (art. 14 LIS): corrección en aumento en el 200 de este ejercicio.',
  };
}

function resumen(year: number): ResumenAjustes {
  const propios = ajustes.filter((a) => a.year === year);
  const hecha = (kind: string, id: number) =>
    propios.find((a) => a.factura_kind === kind && a.factura_id === id)?.id ?? null;
  const noDeducibles = propios.filter(
    (a) => !a.deducible && (a.tipo === 'deterioro' || a.tipo === 'provision'),
  );
  return {
    year,
    start: `${year}-01-01`,
    end: `${year}-12-31`,
    reversion: `${year + 1}-01-01`,
    closed: false,
    ajustes: propios,
    anteriores: [],
    periodificables: [
      {
        kind: 'recibida',
        factura_id: 11,
        numero: 'POL-2026-77',
        tercero: 'Seguros Inventados SA',
        fecha: `${year}-10-01`,
        cuentas: [l('625', 120_000, 0)],
        importe: 120_000,
        desde: null,
        hasta: null,
        ajuste_id: hecha('recibida', 11),
      },
      {
        kind: 'recibida',
        factura_id: 12,
        numero: 'SUS-0098',
        tercero: 'Programas de Prueba SL',
        fecha: `${year}-07-01`,
        cuentas: [l('629', 36_000, 0)],
        importe: 36_000,
        desde: null,
        hasta: null,
        ajuste_id: hecha('recibida', 12),
      },
      {
        kind: 'emitida',
        factura_id: 21,
        numero: 'A2026-0031',
        tercero: 'Estudio Gráfico Turia SL',
        fecha: `${year}-11-01`,
        cuentas: [l('705', 0, 240_000)],
        importe: 240_000,
        desde: null,
        hasta: null,
        ajuste_id: hecha('emitida', 21),
      },
    ],
    deteriorables: propios.some((a) => a.tipo === 'deterioro')
      ? []
      : [
          {
            factura_id: 4,
            vencimiento: 1,
            numero: 'A2026-0004',
            cliente: 'Cliente Moroso Inventado SA',
            nif: 'A22222228',
            cuenta: '4300004',
            vence: `${year}-03-15`,
            dias: 291,
            importe: 72_600,
            vinculada: false,
          },
        ],
    sugerencias: propios.some((a) => a.factura_id === 31)
      ? []
      : [
          {
            kind: 'recibida',
            factura_id: 31,
            numero: 'ASE-12',
            tercero: 'Asesoría Inventada SL',
            nif: '12345678Z',
            fecha: `${year + 1}-01-08`,
            base: 30_000,
            cuenta: '623',
          },
        ],
    deterioro_anterior: 0,
    fiscal: {
      aumentos: noDeducibles.reduce((s, a) => s + a.importe, 0),
      disminuciones: 0,
      detalle: noDeducibles.map((a) => `«${a.descripcion}»: aumento de ${a.importe / 100} €`),
    },
  };
}

const COMPROBACIONES: Comprobacion[] = [
  {
    id: 'proveedores',
    label: 'Proveedores y acreedores sin saldo deudor',
    ok: true,
    detail: 'Ningún proveedor ni acreedor tiene saldo deudor.',
    cuentas: [],
    ir: 'mayor',
    cuenta: null,
  },
  {
    id: 'clientes',
    label: 'Clientes sin saldo acreedor',
    ok: true,
    detail: 'Ningún cliente tiene saldo acreedor.',
    cuentas: [],
    ir: 'mayor',
    cuenta: null,
  },
  {
    id: 'bancos',
    label: 'Bancos y caja sin saldo negativo',
    ok: true,
    detail: 'Ninguna cuenta de tesorería queda en negativo.',
    cuentas: [],
    ir: 'banco',
    cuenta: null,
  },
  {
    id: 'partidas_pendientes',
    label: 'Partidas pendientes de aplicación (555) a cero',
    ok: false,
    detail:
      'La 555 es una cuenta puente: lo que quede hay que llevarlo a su cuenta (el cliente, el proveedor, el gasto) antes de cerrar.',
    cuentas: [{ account: '555', name: 'Partidas pendientes de aplicación', saldo: 15_000 }],
    ir: 'mayor',
    cuenta: '555',
  },
  {
    id: 'socios',
    label: 'Cuenta corriente con socios y administradores (551)',
    ok: false,
    detail:
      'Tiene saldo: si es un préstamo, dalo de alta en Préstamos (con su contrato e intereses de mercado, art. 18 LIS); si son gastos pagados por el socio, liquídalos.',
    cuentas: [
      { account: '551', name: 'Cuenta corriente con socios y administradores', saldo: -50_000 },
    ],
    ir: 'prestamos',
    cuenta: '551',
  },
  {
    id: 'iva',
    label: 'IVA soportado y repercutido liquidados (472 y 477)',
    ok: true,
    detail: 'La 472 y la 477 están a cero: el IVA del ejercicio está liquidado.',
    cuentas: [],
    ir: 'impuestos',
    cuenta: null,
  },
];

export const ajustesCierreHandlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  ajustes_cierre: (args) => resumen(Number(args?.year ?? 2026)),
  ajustes_cierre_comprobaciones: () => COMPROBACIONES,
  ajustes_cierre_propuesta: (args) =>
    propuesta(Number(args?.year ?? 2026), args?.nuevo as NuevoAjuste),
  ajustes_cierre_crear: (args) => {
    const year = Number(args?.year ?? 2026);
    const n = args?.nuevo as NuevoAjuste;
    const p = propuesta(year, n);
    const id = siguiente++;
    const auto = p.reversion_fecha !== null;
    ajustes = [
      ...ajustes,
      {
        id,
        year,
        tipo: n.tipo,
        descripcion: p.descripcion,
        importe: p.importe,
        cuenta: p.lines[0]?.account ?? '',
        contrapartida: p.lines[1]?.account ?? '',
        factura_kind:
          n.factura_kind ??
          (n.tipo === 'gasto_anticipado'
            ? 'recibida'
            : n.tipo === 'ingreso_anticipado' || n.tipo === 'deterioro'
              ? 'emitida'
              : null),
        factura_id: n.factura_id ?? null,
        vencimiento: n.vencimiento ?? null,
        servicio_desde: n.desde ?? null,
        servicio_hasta: n.hasta ?? null,
        tercero_nif: null,
        tercero: null,
        deducible: p.deducible,
        motivo_fiscal: p.motivo_fiscal,
        entry_id: 900 + id,
        fecha: p.fecha,
        lines: p.lines,
        reversion_entry_id: auto ? 950 + id : null,
        reversion_fecha: p.reversion_fecha,
        reversion_year: auto ? year + 1 : null,
      },
    ];
    return id;
  },
  ajustes_cierre_deshacer: (args) => {
    ajustes = ajustes.filter((a) => a.id !== Number(args?.id));
    return null;
  },
  ajustes_cierre_revertir: () => 999,
  ajustes_cierre_deshacer_reversion: () => null,
};

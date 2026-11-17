// Modo demo de Operaciones vinculadas: datos inventados de ESTUDIO DEMO SL (socia única y
// administradora Carmen Española Española y una participada al 50 %, Participada Demo SL, con
// un NIF de prueba que no puede ser de nadie).

import type {
  OperacionManual,
  OperacionVinculada,
  Participada,
  PersonaVinculada,
  RegistroVinculadas,
  Vinculada,
} from './vinculadasApi';

const CARMEN = '12345678Z';
const PARTICIPADA = 'B00000000';

let participadas: Participada[] = [
  {
    id: 1,
    nif: PARTICIPADA,
    name: 'Participada Demo SL',
    province: '46',
    country: 'ES',
    pct: 500_000,
    acquired: '2024-03-01',
    cost: 15_000_000,
    account: '2403',
    grupo_fiscal: false,
    notes: '',
    active: true,
  },
];
let fichas: Vinculada[] = [];
let manuales: OperacionManual[] = [];
const cuentasAsignadas: Record<string, string> = { '5510001': CARMEN, '5320001': PARTICIPADA };

function op(
  nif: string,
  name: string,
  over: Partial<OperacionVinculada> & Pick<OperacionVinculada, 'concepto' | 'importe'>,
): OperacionVinculada {
  return {
    nif,
    name,
    vinculo: nif === CARMEN ? 'socio' : 'participada',
    clave: null,
    ingreso: false,
    metodo: 'precio_libre_comparable',
    metodo_elegido: false,
    especifica: false,
    origen: [],
    excluida: null,
    en_232: false,
    motivo: null,
    descripcion: '',
    justificacion: '',
    ...over,
  };
}

function registro(year: number): RegistroVinculadas {
  const conParticipada = participadas.some((p) => p.nif === PARTICIPADA && p.pct >= 250_000);
  const motivo = 'más de 250.000 € con Participada Demo SL (429.000,00 €)';
  const operaciones: OperacionVinculada[] = [
    ...(conParticipada
      ? [
          op(PARTICIPADA, 'Participada Demo SL', {
            concepto: 'financiacion',
            clave: 5,
            importe: 30_000_000,
            origen: ['subcuenta 5320001'],
            en_232: true,
            motivo,
          }),
          op(PARTICIPADA, 'Participada Demo SL', {
            concepto: 'servicios',
            clave: 6,
            ingreso: true,
            importe: 12_000_000,
            origen: ['12 facturas emitidas'],
            en_232: true,
            motivo,
            metodo: 'coste_incrementado',
            metodo_elegido: true,
            descripcion: 'Servicios de administración y diseño a la participada, por meses.',
            justificacion: 'Coste del personal dedicado más un margen del 5 %.',
          }),
          op(PARTICIPADA, 'Participada Demo SL', {
            concepto: 'intereses',
            clave: 9,
            ingreso: true,
            importe: 900_000,
            origen: ['subcuenta 7620001'],
            en_232: true,
            motivo,
          }),
        ]
      : []),
    op(CARMEN, 'Carmen Española Española', {
      concepto: 'financiacion',
      clave: 5,
      importe: 500_000,
      origen: ['subcuenta 5510001'],
    }),
    op(CARMEN, 'Carmen Española Española', {
      concepto: 'financiacion',
      clave: 5,
      ingreso: true,
      importe: 200_000,
      origen: ['subcuenta 5510001'],
    }),
    op(CARMEN, 'Carmen Española Española', {
      concepto: 'retribucion_administrador',
      importe: 3_600_000,
      origen: ['12 nóminas'],
      excluida:
        'Retribución por el ejercicio del cargo de administrador: fuera del régimen de vinculadas (art. 18.2.b LIS)',
    }),
    op(CARMEN, 'Carmen Española Española', {
      concepto: 'dividendos',
      importe: 2_000_000,
      origen: ['1 pago de dividendos'],
      excluida:
        'El reparto de dividendos no es una operación del 232 (no tiene clave): sólo va a la memoria',
    }),
  ];
  const personas: PersonaVinculada[] = [
    {
      nif: CARMEN,
      name: 'Carmen Española Española',
      legal_person: false,
      province: '46',
      country: 'ES',
      vinculos: ['socio', 'administrador'],
      detalle: 'Socia única y administradora única',
      pct: 1_000_000,
      grupo_fiscal: false,
      ficha: fichas.find((f) => f.nif === CARMEN) ?? null,
      socio: true,
      administrador: true,
      participada: false,
      cuentas: ['5510001'],
      total: 700_000,
      supera_umbral: false,
    },
    ...participadas
      .filter((p) => p.active && p.pct >= 250_000)
      .map(
        (p): PersonaVinculada => ({
          nif: p.nif,
          name: p.name,
          legal_person: true,
          province: p.province,
          country: p.country,
          vinculos: ['participada'],
          detalle: '',
          pct: p.pct,
          grupo_fiscal: p.grupo_fiscal,
          ficha: null,
          socio: false,
          administrador: false,
          participada: true,
          cuentas: p.nif === PARTICIPADA ? ['5320001', '7620001'] : [],
          total: p.nif === PARTICIPADA ? 42_900_000 : 0,
          supera_umbral: p.nif === PARTICIPADA,
        }),
      ),
    ...fichas
      .filter((f) => f.nif !== CARMEN && !participadas.some((p) => p.nif === f.nif))
      .map(
        (f): PersonaVinculada => ({
          nif: f.nif,
          name: f.name,
          legal_person: f.legal_person,
          province: f.province,
          country: f.country,
          vinculos: [f.vinculo],
          detalle: f.detalle,
          pct: f.pct,
          grupo_fiscal: f.grupo_fiscal,
          ficha: f,
          socio: false,
          administrador: false,
          participada: false,
          cuentas: [],
          total: 0,
          supera_umbral: false,
        }),
      ),
  ];
  return {
    year,
    start: `${year}-01-01`,
    end: `${year}-12-31`,
    company: 'ESTUDIO DEMO SL',
    company_nif: 'B12345674',
    incn: 48_000_000,
    incn_fuente: 'la contabilidad de TaxPain',
    personas,
    operaciones,
    manuales: manuales.filter((m) => m.year === year),
    cuentas: [
      {
        account: '5320001',
        name: 'Préstamo a Participada Demo SL',
        debit: 30_000_000,
        credit: 0,
        nif: cuentasAsignadas['5320001'] ?? null,
      },
      {
        account: '5510001',
        name: 'Cuenta corriente con Carmen Española',
        debit: 500_000,
        credit: 200_000,
        nif: cuentasAsignadas['5510001'] ?? null,
      },
      {
        account: '7620001',
        name: 'Intereses del préstamo a la participada',
        debit: 0,
        credit: 900_000,
        nif: cuentasAsignadas['7620001'] ?? PARTICIPADA,
      },
    ],
    obligado: conParticipada,
    motivos: conParticipada ? [motivo] : [],
    avisos: [
      'La retribución de un administrador que además es socio con el 25 % o más se deja fuera del 232 por el art. 18.2.b LIS (por el ejercicio de sus funciones): si cobra por otros trabajos, tecléalo como rendimientos del trabajo (clave 10).',
    ],
    plazo: `noviembre de ${year + 1}`,
    simplificada: true,
    documentar: conParticipada ? ['Participada Demo SL'] : [],
    textos: {
      year,
      actividad: 'Estudio de diseño gráfico; tiene el 50 % de Participada Demo SL.',
      justificacion: '',
    },
  };
}

export const vinculadasHandlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  vinculadas_registro: (args) => registro(Number(args?.year ?? 2026)),
  vinculada_de: (args) => fichas.find((f) => f.nif === String(args?.nif ?? '')) ?? null,
  vinculada_save: (args) => {
    const f = args?.vinculada as Vinculada;
    const id = f.id || fichas.length + 10;
    fichas = [...fichas.filter((x) => x.id !== f.id || f.id === 0), { ...f, id }];
    return id;
  },
  vinculada_delete: (args) => {
    fichas = fichas.filter((f) => f.id !== Number(args?.id));
    return null;
  },
  participadas_list: () => participadas,
  participada_save: (args) => {
    const p = args?.participada as Participada;
    const id = p.id || participadas.length + 10;
    participadas = [...participadas.filter((x) => x.id !== p.id || p.id === 0), { ...p, id }];
    return id;
  },
  participada_delete: (args) => {
    participadas = participadas.filter((p) => p.id !== Number(args?.id));
    return null;
  },
  vinculada_cuenta_set: (args) => {
    const account = String(args?.account ?? '');
    const nif = args?.nif as string | null;
    if (nif) cuentasAsignadas[account] = nif;
    else delete cuentasAsignadas[account];
    return null;
  },
  vinculada_operacion_save: (args) => {
    const m = args?.operacion as OperacionManual;
    const id = manuales.length + 1;
    manuales = [...manuales, { ...m, id }];
    return id;
  },
  vinculada_operacion_delete: (args) => {
    manuales = manuales.filter((m) => m.id !== Number(args?.id));
    return null;
  },
  vinculada_valoracion_save: () => null,
  vinculadas_textos_save: () => null,
  vinculadas_documentacion_save: () => null,
  vinculadas_hoja_232_save: () => null,
};

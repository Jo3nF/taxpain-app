// Modo demo de las rentas a no residentes (216 y 296) y de los inmuebles arrendados (115 y 180):
// un licenciante francés con certificado de residencia, un prestamista alemán exento y la
// oficina alquilada a Inmuebles Ejemplo SA. Datos inventados.
import type {
  CertificadoResidencia,
  InmuebleArrendado,
  ModelDraft,
  NuevaRentaIrnr,
  PropuestaIrnr,
  ResumenIrnr,
} from './api';

let inmuebles: InmuebleArrendado[] = [
  {
    nif: 'A22222228',
    situacion: 1,
    ref_catastral: '9872023VH5797S0001WX',
    via: 'Calle Ejemplo',
    numero: '10',
    municipio: 'València',
    provincia: '46',
    codigo_postal: '46001',
    exonerado_hasta: null,
  },
];

const resumen: ResumenIrnr = {
  year: 2026,
  certificados: [
    {
      id: 1,
      nif: 'FR00000000001',
      nombre: 'Licencias Exemple SARL',
      pais: 'FR',
      expedido: '2026-01-10',
      valido_hasta: '2027-01-09',
      convenio: true,
      documento: 'certificado-residencia-FR.pdf',
      notas: '',
      vigente: true,
      caduca_pronto: false,
    },
    {
      id: 2,
      nif: 'DE000000002',
      nombre: 'Darlehen Beispiel GmbH',
      pais: 'DE',
      expedido: '2025-12-01',
      valido_hasta: '2026-11-30',
      convenio: true,
      documento: '',
      notas: '',
      vigente: true,
      caduca_pronto: true,
    },
  ],
  rentas: [
    {
      id: 1,
      fecha: '2026-04-20',
      nif: 'FR00000000001',
      nombre: 'Licencias Exemple SARL',
      pais: 'FR',
      naturaleza: 'canon',
      bruto: 200_000,
      tipo: 500,
      retencion: 10_000,
      regimen: 'convenio',
      exencion: null,
      concepto: 'Licencia de software 2T',
      recibida_id: null,
    },
    {
      id: 2,
      fecha: '2026-06-30',
      nif: 'DE000000002',
      nombre: 'Darlehen Beispiel GmbH',
      pais: 'DE',
      naturaleza: 'interes',
      bruto: 50_000,
      tipo: 0,
      retencion: 0,
      regimen: 'exenta',
      exencion: 'art14_1_c',
      concepto: 'Intereses del préstamo',
      recibida_id: null,
    },
  ],
  dividendos: [],
  candidatas: [
    {
      recibida_id: 7,
      fecha: '2026-09-15',
      nif: 'IE0000000T',
      nombre: 'Cloud Ejemplo Ltd',
      pais: 'IE',
      numero: 'INV-0042',
      base: 30_000,
    },
  ],
  convenios: [
    {
      pais: 'FR',
      nombre: 'Francia',
      dividendos_bp: 1500,
      intereses_bp: 1000,
      canones_bp: 500,
      fuente: 'Convenio de 10-10-1995 (BOE 12-6-1997), arts. 10-12',
    },
  ],
};

export const draft216: ModelDraft = {
  model: '216',
  year: 2026,
  quarter: 2,
  boxes: [
    [2, 200_000],
    [3, 10_000],
    [5, 50_000],
    [6, 0],
    [7, 10_000],
  ],
  payees: [
    [1, 1],
    [4, 1],
  ],
  details: [
    ['2026-04-20 · Licencias Exemple SARL · FR00000000001 · FR · Cánones al 5 %', 10_000],
    ['2026-06-30 · Darlehen Beispiel GmbH · DE000000002 · DE · Intereses al 0 %', 0],
  ],
  declaration: 'I',
  result: 10_000,
  warnings: [
    'TaxPain no tiene el diseño de registro del 216: no genera el fichero. Teclea estas casillas en el formulario de la Sede.',
  ],
  file_name: 'B12345674_2026_2T_216.txt',
  file: '',
};

export const draft296: ModelDraft = {
  ...draft216,
  model: '296',
  quarter: 0,
  boxes: [],
  payees: [],
  details: [
    [
      'Licencias Exemple SARL · FR00000000001 · FR · Cánones · convenio 5 % · 1 renta(s), íntegro 2.000,00 €',
      10_000,
    ],
    [
      'Darlehen Beispiel GmbH · DE000000002 · DE · Intereses · exenta 0 % · 1 renta(s), íntegro 500,00 €',
      0,
    ],
  ],
  declaration: '',
  warnings: [
    'TaxPain no tiene el diseño de registro del 296: no genera el fichero. Presenta el 296 en el formulario de la Sede con estos registros.',
  ],
  file_name: 'B12345674_2026_0A_296.txt',
};

const propuesta = (r?: NuevaRentaIrnr): PropuestaIrnr => {
  const general = r?.naturaleza === 'interes' ? 1900 : 2400;
  return r?.pais === 'FR'
    ? {
        regimen: 'convenio',
        tipo: r.naturaleza === 'interes' ? 1000 : 500,
        tipo_general: 1900,
        exencion: null,
        motivo: 'tipo general del convenio con Francia',
        avisos: [],
      }
    : {
        regimen: 'general',
        tipo: general,
        tipo_general: general,
        exencion: null,
        motivo: 'tipo general (art. 25.1 TRLIRNR)',
        avisos: ['Sin certificado de residencia fiscal del perceptor: se aplica el tipo general.'],
      };
};

export const noResidentesHandlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  irnr_resumen: (args) => ({ ...resumen, year: Number(args?.year ?? 2026) }),
  irnr_guardar_certificado: (args) => {
    const c = args?.certificado as CertificadoResidencia | undefined;
    return c?.id || resumen.certificados.length + 1;
  },
  irnr_quitar_certificado: () => undefined,
  irnr_proponer: (args) => propuesta(args?.renta as NuevaRentaIrnr | undefined),
  irnr_apuntar: () => resumen.rentas.length + 1,
  irnr_quitar_renta: () => undefined,
  inmuebles_list: () => inmuebles.map((i) => ({ ...i })),
  inmueble_guardar: (args) => {
    const i = args?.inmueble as InmuebleArrendado | undefined;
    if (i) inmuebles = [...inmuebles.filter((x) => x.nif !== i.nif.toUpperCase()), i];
    return undefined;
  },
  inmueble_quitar: (args) => {
    inmuebles = inmuebles.filter((x) => x.nif !== String(args?.nif ?? ''));
    return undefined;
  },
};

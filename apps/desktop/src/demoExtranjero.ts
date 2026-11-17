// Modo demo de los modelos 720 y 721: una persona física inventada con un neobanco y un bróker
// del extranjero y criptomonedas en un exchange de fuera. Datos inventados.
import type {
  BienExtranjeroView,
  BloqueExtranjeroView,
  BorradorExtranjero,
  DatosBienExtranjero,
  DeclaradoExtranjero,
} from './api';

const datos = (
  origen: DatosBienExtranjero['origen'],
  ref_id: number,
  extra: Partial<DatosBienExtranjero> = {},
): DatosBienExtranjero => ({
  origen,
  ref_id,
  condicion: 'titular',
  porcentaje_bp: 10000,
  fecha_apertura: null,
  fecha_extincion: null,
  entidad: '',
  nif_entidad: '',
  bic: '',
  domicilio: '',
  pais: '',
  extranjero: null,
  ...extra,
});

let bienes: BienExtranjeroView[] = [
  {
    nombre: 'Neobanco',
    extranjero: true,
    iban: 'LT121000011101001000',
    datos: datos('banco', 1, { fecha_apertura: '2023-04-12', bic: 'DEMOLT2X' }),
  },
  {
    nombre: 'Banco Demo',
    extranjero: false,
    iban: 'ES9121000418450200051332',
    datos: datos('banco', 2),
  },
  { nombre: 'Bróker Demo', extranjero: true, iban: '', datos: datos('valores', 1) },
  { nombre: 'Exchange Demo', extranjero: true, iban: '', datos: datos('cripto', 1) },
];

let declarados: DeclaradoExtranjero[] = [
  {
    id: 1,
    modelo: '720',
    year: 2024,
    bloque: 'cuentas',
    valor: 5_200_000,
    saldo_medio: 5_000_000,
    bienes: ['LT121000011101001000'],
    origen: 'tecleado',
    creado: '2026-11-17T10:00:00Z',
  },
];
let siguiente = 1;

function anterior(modelo: string, bloque: string, year: number) {
  return (
    declarados
      .filter((d) => d.modelo === modelo && d.bloque === bloque && d.year < year)
      .sort((a, b) => b.year - a.year)[0] ?? null
  );
}

function bloque(
  b: BloqueExtranjeroView['bloque'],
  titulo: string,
  year: number,
  total: number,
  medio: number | null,
  registros: BloqueExtranjeroView['registros'],
): BloqueExtranjeroView {
  const modelo = b === 'monedas' ? '721' : '720';
  const a = anterior(modelo, b, year);
  const supera = total > 5_000_000 || (medio ?? 0) > 5_000_000;
  const incremento = a ? total - a.valor : null;
  const obligado = a ? (incremento ?? 0) > 2_000_000 : supera;
  const motivo = a
    ? obligado
      ? `Ha subido más de 20.000 € respecto del ${modelo} de ${a.year}: se vuelve a presentar.`
      : `Ya se declaró en el ${modelo} de ${a.year} y no ha subido más de 20.000 €: no hay que volver a presentar.`
    : supera
      ? 'Supera 50.000 € y no hay declaración anterior de este bloque: se presenta.'
      : 'No pasa de 50.000 €: no se presenta.';
  return {
    bloque: b,
    titulo,
    total,
    total_medio: medio,
    supera,
    anterior: a,
    incremento,
    incremento_medio: null,
    extinguidos: [],
    obligado,
    motivo,
    registros: registros.map((r) => ({
      ...r,
      alta: a?.bienes.includes(r.identificacion) ? 'M' : 'A',
    })),
  };
}

function borrador(modelo: string, year: number): BorradorExtranjero {
  const d = (n: string) => bienes.find((b) => b.nombre === n)?.datos ?? datos('banco', 0);
  const neo = d('Neobanco');
  const broker = d('Bróker Demo');
  const exchange = d('Exchange Demo');
  const base = {
    nif_entidad: '',
    bic: '',
    domicilio: '',
    porcentaje_bp: 10000,
    fecha_extincion: null,
    saldo_medio: null,
    alta: 'A' as const,
  };
  const bloques =
    modelo === '720'
      ? [
          bloque('cuentas', 'Cuentas en entidades financieras', year, 7_850_000, 7_420_000, [
            {
              ...base,
              origen: 'banco',
              ref_id: 1,
              bloque: 'cuentas',
              clave: 'C',
              descripcion: 'Neobanco',
              identificacion: 'LT121000011101001000',
              entidad: neo.entidad || 'Neobanco Demo UAB',
              bic: neo.bic,
              domicilio: neo.domicilio,
              nif_entidad: neo.nif_entidad,
              pais: neo.pais || 'LT',
              condicion: neo.condicion,
              fecha_apertura: neo.fecha_apertura ?? '2023-04-12',
              fecha_estimada: !neo.fecha_apertura,
              saldo: 7_850_000,
              saldo_medio: 7_420_000,
              unidades: null,
              moneda: 'EUR',
              faltan: neo.domicilio ? [] : ['domicilio de la entidad'],
            },
          ]),
          bloque('valores', 'Valores, derechos e IIC', year, 4_100_000, null, [
            {
              ...base,
              origen: 'valores',
              ref_id: 1,
              bloque: 'valores',
              clave: 'I',
              descripcion: 'Fondo Indexado Demo (FIDX)',
              identificacion: 'IE00B0000001',
              entidad: broker.entidad || 'Bróker Demo',
              nif_entidad: broker.nif_entidad,
              pais: broker.pais || 'IE',
              condicion: broker.condicion,
              fecha_apertura: '2024-02-05',
              fecha_estimada: true,
              saldo: 4_100_000,
              unidades: '120',
              moneda: 'EUR',
              faltan: broker.nif_entidad ? [] : ['NIF de la entidad', 'domicilio de la entidad'],
            },
          ]),
        ]
      : [
          bloque('monedas', 'Monedas virtuales', year, 6_000_000, null, [
            {
              ...base,
              origen: 'cripto',
              ref_id: 1,
              bloque: 'monedas',
              clave: 'M',
              descripcion: 'BTC',
              identificacion: 'BTC',
              entidad: exchange.entidad || 'Exchange Demo',
              nif_entidad: exchange.nif_entidad,
              pais: exchange.pais || 'US',
              condicion: exchange.condicion,
              fecha_apertura: '2024-06-01',
              fecha_estimada: true,
              saldo: 6_000_000,
              unidades: '0,75',
              moneda: 'EUR',
              faltan: exchange.nif_entidad ? [] : ['NIF del custodio', 'domicilio del custodio'],
            },
          ]),
        ];
  return {
    modelo,
    year,
    fecha: `${year}-12-31`,
    plazo: `${year + 1}-03-31`,
    nif: '12345678Z',
    nombre: 'ESPAÑOLA ESPAÑOLA CARMEN',
    persona_fisica: true,
    bloques,
    obligado: bloques.some((b) => b.obligado),
    exenta_por_contabilidad: false,
    requisitos: [],
    presentados_aeat: modelo === '720' ? [2024] : [],
    declarados: declarados.filter((x) => x.modelo === modelo),
    bienes: bienes.filter((b) =>
      modelo === '720' ? b.datos.origen !== 'cripto' : b.datos.origen === 'cripto',
    ),
    fichero: {
      disponible: false,
      nota:
        modelo === '720'
          ? 'TaxPain no tiene el diseño de registro del 720 (Orden HAP/72/2013, de 30 de enero, y sus modificaciones), así que no genera el fichero: pasa los registros de la hoja al formulario del 720 en la Sede de la AEAT.'
          : 'TaxPain no tiene el diseño de registro del 721 (Orden HFP/886/2023, de 26 de julio), así que no genera el fichero: pasa los registros de la hoja al formulario del 721 en la Sede de la AEAT.',
    },
    avisos: [],
    notas: [
      `Se presenta del 1 de enero al 31 de marzo de ${year + 1}. Cada bloque va por separado: la primera vez, si pasa de 50.000 €; después, si sube más de 20.000 € respecto de la última declaración o se extingue algún bien declarado.`,
    ],
    norma:
      modelo === '720'
        ? 'DA 18.ª LGT; arts. 42 bis y 42 ter RGAT; Orden HAP/72/2013'
        : 'DA 18.ª LGT; art. 42 quater RGAT; Orden HFP/886/2023',
  };
}

export const extranjeroHandlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  extranjero_borrador: (args) =>
    borrador(String(args?.modelo ?? '720'), Number(args?.year ?? 2025)),
  extranjero_guardar_bien: (args) => {
    const d = args?.datos as DatosBienExtranjero;
    bienes = bienes.map((b) =>
      b.datos.origen === d.origen && b.datos.ref_id === d.ref_id ? { ...b, datos: d } : b,
    );
  },
  extranjero_guardar_declarado: (args) => {
    const d = args?.declarado as DeclaradoExtranjero;
    siguiente += 1;
    declarados = [
      ...declarados.filter(
        (x) => !(x.modelo === d.modelo && x.year === d.year && x.bloque === d.bloque),
      ),
      { ...d, id: siguiente, origen: 'tecleado', creado: '2026-11-17T10:00:00Z' },
    ];
  },
  extranjero_quitar_declarado: (args) => {
    declarados = declarados.filter((x) => x.id !== Number(args?.id));
  },
  extranjero_marcar_presentado: (args) => {
    const modelo = String(args?.modelo ?? '720');
    const year = Number(args?.year ?? 2025);
    const hechos = borrador(modelo, year)
      .bloques.filter((b) => b.obligado)
      .map((b) => {
        siguiente += 1;
        return {
          id: siguiente,
          modelo,
          year,
          bloque: b.bloque,
          valor: b.total,
          saldo_medio: b.total_medio,
          bienes: b.registros.map((r) => r.identificacion),
          origen: 'borrador',
          creado: '2026-11-17T10:00:00Z',
        };
      });
    declarados = [
      ...declarados.filter((x) => !(x.modelo === modelo && x.year === year)),
      ...hechos,
    ];
    return hechos;
  },
  extranjero_exportar: (args) =>
    `${String(args?.dir ?? '')}/Modelo ${String(args?.modelo ?? '720')} ${Number(args?.year ?? 2025)} - registros.xlsx`,
};

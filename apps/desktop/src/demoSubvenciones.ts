// Modo demo de Contabilidad → Subvenciones y de Cierre → Datos para la Renta y el Patrimonio
// del socio: ESTUDIO DEMO SL con una subvención de capital inventada de 6.000 € que financia una
// máquina de 10.000 € al 20 % (1.200 € al año a la 746, 25 % de efecto impositivo) y la socia
// administradora de prueba (12345678Z) con el 60 %.

import type {
  InformeSocio,
  PersonaSocio,
  PropuestaSubvencion,
  ResumenSubvenciones,
  Subvencion,
} from './api';

const ficha = (year: number): Subvencion => ({
  id: 1,
  concedente: 'Ayuntamiento de Ejemplo',
  concedente_nif: '',
  fecha_concesion: `${year}-01-15`,
  importe: 600_000,
  finalidad: 'comprar la maquinaria',
  tipo: 'capital',
  condiciones: 'mantener la inversión cinco años',
  reintegrable: false,
  fixed_asset_id: 1,
  efecto_impositivo: true,
  tipo_impositivo: 2500,
  minimis: true,
  cuenta_deudora: '4708',
  notas: '',
});

function resumen(year: number): ResumenSubvenciones {
  return {
    year,
    subvenciones: [
      {
        subvencion: ficha(year),
        bien: 'Máquina demo',
        asientos: [
          {
            id: 1,
            tipo: 'concesion',
            fecha: `${year}-01-15`,
            ejercicio: year,
            importe: 600_000,
            entry_id: 501,
          },
          {
            id: 2,
            tipo: 'cobro',
            fecha: `${year}-03-01`,
            ejercicio: year,
            importe: 600_000,
            entry_id: 502,
          },
        ],
        concedida: true,
        en_deuda: false,
        cobrado: 600_000,
        pendiente_cobro: 0,
        imputado: 0,
        imputado_ejercicio: 0,
        pendiente_imputar: 600_000,
        saldo_pn: 450_000,
        pasivo_diferido: 150_000,
        propuesta_imputacion: 120_000,
        avisos: [],
      },
    ],
    imputado_ejercicio: 0,
    saldo_pn: 450_000,
    pasivo_diferido: 150_000,
    minimis_tres_años: 600_000,
    tope_minimis: 30_000_000,
    notas: [
      'Las subvenciones no reintegrables de capital van al patrimonio neto (130, 131) y a resultados (746) al ritmo de la amortización del bien; las de gastos futuros (132) a la 747 cuando se devengan; las de explotación, a la 740 (PGC y PGC de PYMES, NRV 18.ª).',
      'Fiscalmente tributan cuando se imputan a resultados (art. 19 LIS): no hay ajuste en el 200. La parte del impuesto que corresponde a lo que queda en el patrimonio neto es un pasivo por impuesto diferido (479).',
    ],
  };
}

function propuesta(year: number): PropuestaSubvencion {
  const l = (account: string, debit: number, credit: number) => ({
    account,
    debit,
    credit,
    description: 'Ayuntamiento de Ejemplo',
  });
  return {
    asiento: {
      date: `${year}-12-31`,
      concept: 'Imputación a resultados de subvención: Ayuntamiento de Ejemplo',
      origin: { Subvencion: 1 },
      lines: [l('130', 90_000, 0), l('479', 30_000, 0), l('746', 0, 120_000)],
    },
    importe: 120_000,
  };
}

const carmen: PersonaSocio = {
  nif: '12345678Z',
  nombre: 'Española Española Carmen',
  socio: true,
  administrador: true,
  pct: 600_000,
};

function informe(year: number): InformeSocio {
  return {
    year,
    persona: carmen,
    empresa: 'ESTUDIO DEMO SL',
    nif_empresa: 'B12345674',
    conceptos: [
      {
        apartado: 'Rendimientos del trabajo',
        concepto: 'Retribuciones dinerarias (cargo de administrador y trabajo)',
        casilla: '0001',
        importe: 2_400_000,
        retencion: 456_000,
        fuente: '12 nóminas · 190, clave E, subclave 03',
        nota: 'La retención va en la casilla de retenciones del trabajo (0596).',
        estimado: false,
      },
      {
        apartado: 'Rendimientos del trabajo',
        concepto: 'Retribución en especie: cuota del RETA pagada por la sociedad',
        casilla: '0004 / 0005',
        importe: 360_000,
        retencion: 68_400,
        fuente: 'Nóminas → Autónomo societario (cuotas pagadas)',
        nota: '',
        estimado: false,
      },
      {
        apartado: 'Rendimientos del capital mobiliario',
        concepto: 'Dividendos y participaciones en beneficios',
        casilla: '0029',
        importe: 600_000,
        retencion: 114_000,
        fuente: 'Cierre → Socios y dividendos (193)',
        nota: 'Base del ahorro.',
        estimado: false,
      },
    ],
    patrimonio: {
      ejercicio_balance: year - 1,
      fecha_balance: `${year - 1}-12-31`,
      patrimonio_neto: 10_000_000,
      pct: 600_000,
      valor_teorico: 6_000_000,
      requisitos: [
        {
          titulo: 'La sociedad no gestiona un patrimonio mobiliario o inmobiliario',
          estado: 'cumple',
          detalle:
            '0 de 4 trimestres cerrados con más de la mitad del activo en valores o no afecto.',
          norma: 'Art. 4.Ocho.Dos.a LIP',
        },
        {
          titulo: 'Participación de al menos el 5 % (20 % con su familia)',
          estado: 'cumple',
          detalle: 'Participación individual del 60,00 %.',
          norma: 'Art. 4.Ocho.Dos.b LIP',
        },
        {
          titulo: 'Funciones de dirección con más del 50 % de sus rendimientos',
          estado: 'sin_datos',
          detalle:
            'Remuneración de la sociedad 27.600,00 €. Indica sus otros rendimientos del trabajo y de actividades para comprobar que es más del 50 %.',
          norma: 'Art. 4.Ocho.Dos.c LIP; RD 1704/1999',
        },
      ],
      exencion: 'dudoso',
      proporcion_afecta: 9000,
      valor_exento_orientativo: 5_400_000,
      notas: [
        'Participaciones no cotizadas: valor teórico del último balance aprobado (art. 16.Uno LIP).',
      ],
    },
    avisos: [],
    notas: [
      'Junta lo que la sociedad sabe y ha declarado (190, 193, 123 y 115); no calcula el IRPF ni el Impuesto sobre el Patrimonio de la persona ni le dice qué hacer.',
      'Las casillas son las de Renta WEB de los últimos ejercicios y pueden cambiar: cotéjalas con el borrador de la AEAT.',
    ],
  };
}

export const subvencionesHandlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  subvenciones: (a) => resumen(Number(a?.year ?? 2026)),
  subvencion_guardar: () => 2,
  subvencion_borrar: () => null,
  subvencion_propuesta: () => propuesta(2026),
  subvencion_registrar: () => 503,
  subvencion_deshacer: () => null,
  renta_socio_personas: () => [carmen],
  renta_socio: (a) => informe(Number(a?.year ?? 2026)),
  renta_socio_save: () => null,
};

// Modo demo del Censo (modelo 036): datos inventados de ESTUDIO DEMO SL (B12345674). El
// certificado de situación censal de la demo no tiene el 111 y la contabilidad tiene facturas
// de profesionales con retención; constan un epígrafe de alquiler que no se usa y el ROI de baja
// con facturas intracomunitarias.

import type {
  CensoTecleado,
  ComunicacionCenso,
  CotejoCenso,
  GuiaCenso,
  SupuestoCenso,
} from './api';

const SUPUESTOS: SupuestoCenso[] = [
  {
    id: 'alta-roi',
    titulo: 'Alta en el Registro de operadores intracomunitarios (ROI)',
    grupo: 'IVA y operaciones intracomunitarias',
  },
  {
    id: 'criterio-caja-renuncia',
    titulo: 'Renunciar al criterio de caja del IVA',
    grupo: 'IVA y operaciones intracomunitarias',
  },
  {
    id: 'alta-111',
    titulo: 'Alta en las retenciones de trabajo y profesionales (111)',
    grupo: 'Retenciones',
  },
  { id: 'baja-115', titulo: 'Baja en las retenciones de alquileres (115)', grupo: 'Retenciones' },
  {
    id: 'opcion-202-base',
    titulo: 'Optar por el 202 por la base del periodo (art. 40.3 LIS)',
    grupo: 'Impuesto sobre Sociedades',
  },
  {
    id: 'baja-epigrafe',
    titulo: 'Dejar una actividad (baja de un epígrafe)',
    grupo: 'Actividad y datos de la empresa',
  },
  {
    id: 'domicilio-fiscal',
    titulo: 'Cambio de domicilio fiscal',
    grupo: 'Actividad y datos de la empresa',
  },
  {
    id: 'administrador',
    titulo: 'Cambio de administrador o de representante',
    grupo: 'Actividad y datos de la empresa',
  },
];

let tecleado: CensoTecleado | null = null;
let actividad: string[] = ['844'];
let comunicaciones: ComunicacionCenso[] = [];
let siguiente = 0;

function cotejo(): CotejoCenso {
  return {
    hoy: '2026-11-16',
    situacion: {
      origen: tecleado ? 'tecleado' : 'certificado',
      documento: tecleado ? null : 'Certificado de situación censal',
      fecha: tecleado?.fecha ?? '2026-10-01',
      obligaciones: tecleado?.obligaciones ?? ['115', '200', '303'],
      lineas: [],
      epigrafes: tecleado?.epigrafes ?? ['844', '861.2'],
      roi: tecleado ? tecleado.roi : false,
      sii: tecleado?.sii ?? null,
      criterio_caja: tecleado?.criterio_caja ?? null,
      domicilio_fiscal: tecleado?.domicilio_fiscal ?? 'Calle Inventada 1, 46001, Valencia',
    },
    tecleado,
    epigrafes_actividad: actividad,
    filas: [
      {
        codigo: '303',
        nombre: 'IVA (303 y 390)',
        censo: true,
        perfil: true,
        contabilidad: null,
        presentado: [2025, 2026],
      },
      {
        codigo: '111',
        nombre: 'Retenciones de trabajo y profesionales (111 y 190)',
        censo: false,
        perfil: true,
        contabilidad: '4 facturas de profesionales con retención (600,00 € de retención)',
        presentado: [],
      },
      {
        codigo: '115',
        nombre: 'Retenciones de alquileres (115 y 180)',
        censo: true,
        perfil: false,
        contabilidad: null,
        presentado: [],
      },
      {
        codigo: '349',
        nombre: 'Operaciones intracomunitarias (349)',
        censo: false,
        perfil: false,
        contabilidad: '3 facturas emitidas que van al 349',
        presentado: [],
      },
      {
        codigo: '200',
        nombre: 'Impuesto sobre Sociedades (200 y 202)',
        censo: true,
        perfil: true,
        contabilidad: null,
        presentado: [2025],
      },
    ],
    diferencias: [
      {
        clave: '111-sin-alta',
        gravedad: 'alta',
        titulo:
          'Retenciones de trabajo y profesionales (111 y 190): no consta en el censo y la contabilidad tiene 4 facturas de profesionales con retención (600,00 € de retención)',
        detalle:
          'Según el certificado de situación censal del 01/10/2026, la empresa no tiene la obligación del 111.',
        que_hacer:
          'Comunica en el 036 el alta de la obligación del 111 con la fecha de la primera operación.',
        supuesto: 'alta-111',
      },
      {
        clave: 'roi-sin-alta',
        gravedad: 'alta',
        titulo: 'Operaciones intracomunitarias sin alta en el ROI',
        detalle:
          'No estás en el Registro de operadores intracomunitarios y hay 3 facturas intracomunitarias.',
        que_hacer: 'Pide el alta en el ROI con el 036 cuanto antes.',
        supuesto: 'alta-roi',
      },
      {
        clave: 'epigrafe-sobra-861.2',
        gravedad: 'media',
        titulo: 'Consta el epígrafe 861.2 y no es de lo que haces',
        detalle:
          'Estás de alta en el epígrafe 861.2, que no está entre las actividades que has dicho que haces.',
        que_hacer: 'Si ya no haces esa actividad, comunica la baja del epígrafe en el 036.',
        supuesto: 'baja-epigrafe',
      },
      {
        clave: '115-sin-uso',
        gravedad: 'media',
        titulo: 'Retenciones de alquileres (115 y 180): consta en el censo y no hay operaciones',
        detalle: 'Mientras conste, la AEAT espera el 115 cada periodo.',
        que_hacer:
          'Si ya no pagas alquiler con retención, comunica la baja de la obligación del 115 en el 036.',
        supuesto: 'baja-115',
      },
    ],
    comunicaciones,
    supuestos: SUPUESTOS,
  };
}

function guia(id: string, fecha: string | null): GuiaCenso {
  const s = SUPUESTOS.find((x) => x.id === id) ?? (SUPUESTOS[0] as SupuestoCenso);
  const ventana = id === 'opcion-202-base';
  let vence: string | null = null;
  if (ventana) vence = '2027-03-01';
  else if (fecha) {
    const d = new Date(`${fecha}T12:00:00Z`);
    d.setUTCMonth(d.getUTCMonth() + 1);
    vence = d.toISOString().slice(0, 10);
  }
  return {
    id: s.id,
    titulo: s.titulo,
    resumen: 'Ejemplo de la demo: en la app, la guía sale de la norma de cada supuesto.',
    por_036: true,
    causa: 'Modificación (de datos de la declaración censal ya presentada)',
    pasos: [
      { apartado: 'Causas de presentación', que_hacer: 'Marca la modificación que corresponde.' },
      {
        apartado: ventana ? 'Impuesto sobre Sociedades' : 'Retenciones e ingresos a cuenta',
        que_hacer: 'Marca el alta o la baja con su fecha.',
      },
    ],
    plazo: {
      tipo: ventana ? 'ventana' : 'un_mes',
      texto: ventana
        ? 'En febrero del año en que debe surtir efecto: hasta el 01/03/2027.'
        : 'Un mes desde el hecho (art. 10 RGAT).',
      desde: ventana ? '2027-02-01' : fecha,
      vence,
      pide_fecha: !ventana && !fecha,
      en_calendario: ventana ? '036-202' : null,
    },
    documentacion: [],
    norma: ventana ? 'LIS art. 40.3' : 'RGAT arts. 9 a 14',
    avisos: [],
    sede: 'https://sede.agenciatributaria.gob.es/Sede/procedimientoini/G322.shtml',
    verificar:
      'Los apartados se nombran por su título: los números de casilla no se han cotejado con la Orden HAC/1526/2024.',
  };
}

export const censoHandlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  censo_cotejo: () => cotejo(),
  censo_guardar: (args) => {
    tecleado = (args?.tecleado as CensoTecleado | null) ?? null;
    actividad = (args?.epigrafesActividad as string[] | undefined) ?? [];
  },
  censo_guia: (args) => guia(String(args?.id ?? ''), (args?.fecha as string | null) ?? null),
  censo_programar: (args) => {
    const g = guia(String(args?.id ?? ''), (args?.fecha as string | null) ?? null);
    if (!g.plazo.vence) throw new Error('pon la fecha del hecho para calcular el plazo');
    siguiente += 1;
    const c: ComunicacionCenso = {
      id: siguiente,
      supuesto: g.id,
      titulo: g.titulo,
      fecha_hecho: (args?.fecha as string | null) ?? null,
      vence: g.plazo.vence,
      nota: String(args?.nota ?? ''),
      creada: '2026-11-16T10:00:00Z',
      hecha: null,
    };
    comunicaciones = [...comunicaciones, c];
    return c;
  },
  censo_presentada: (args) => {
    comunicaciones = comunicaciones.map((c) =>
      c.id === Number(args?.id) ? { ...c, hecha: '2026-11-16T10:00:00Z' } : c,
    );
  },
  censo_quitar: (args) => {
    comunicaciones = comunicaciones.filter((c) => c.id !== Number(args?.id));
  },
};

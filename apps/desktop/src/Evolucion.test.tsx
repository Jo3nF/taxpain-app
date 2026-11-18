import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import {
  type AssistantTopic,
  type ConversationSummary,
  type Evolucion,
  type Revision,
  setDemoInvoke,
} from './api';
import { EvolucionPage } from './Evolucion';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const evolucion: Evolucion = {
  años: [2024, 2025],
  sin_comprobar: false,
  sin_casillas: 1,
  grupos: [
    {
      titulo: 'IVA',
      filas: [
        {
          id: 'volumen',
          titulo: 'Volumen de operaciones',
          ayuda: 'Lo facturado en el año.',
          recuento: false,
          valores: [
            {
              año: 2024,
              valor: 10_000_000,
              fuente: '390 [108]',
              diferencia: null,
              variacion: null,
            },
            {
              año: 2025,
              valor: 15_000_000,
              fuente: '390 [108]',
              diferencia: 5_000_000,
              variacion: 500,
            },
          ],
        },
        {
          id: 'iva_resultado',
          titulo: 'Resultado del IVA del año',
          ayuda: '',
          recuento: false,
          valores: [
            { año: 2024, valor: null, fuente: null, diferencia: null, variacion: null },
            {
              año: 2025,
              valor: -19_371,
              fuente: 'suma de los 303 [64]',
              diferencia: null,
              variacion: null,
            },
          ],
        },
      ],
    },
    {
      titulo: 'Declaraciones',
      filas: [
        {
          id: 'presentadas',
          titulo: 'Declaraciones presentadas',
          ayuda: '',
          recuento: true,
          valores: [
            { año: 2024, valor: 12, fuente: 'AEAT', diferencia: null, variacion: null },
            { año: 2025, valor: 9, fuente: 'AEAT', diferencia: -3, variacion: -250 },
          ],
        },
      ],
    },
    {
      titulo: 'Actividad y cuentas',
      filas: [
        {
          id: 'cifra_negocios',
          titulo: 'Cifra de negocios',
          ayuda: '',
          recuento: false,
          valores: [
            {
              año: 2024,
              valor: 10_000_000,
              fuente: '200 [00255]',
              diferencia: null,
              variacion: null,
            },
            {
              año: 2025,
              valor: 5_500_000,
              fuente: 'avance con los 303 1T y 2T (sin 200)',
              diferencia: null,
              variacion: null,
              avance: true,
            },
          ],
        },
      ],
    },
  ],
};

const hallazgo = (
  codigo: string,
  gravedad: 'error' | 'aviso',
  ejercicio: number,
  titulo: string,
) => ({
  codigo,
  area: 'modelos' as const,
  gravedad,
  responsable: 'gestoria' as const,
  ejercicio,
  titulo,
  detalle: 'Detalle.',
  ficheros: [`AEAT · modelo 390 ${ejercicio} 0A · justificante 3900000000001`],
  importe: null,
  rectificable_hasta: `${ejercicio + 4}-04-20`,
});

const revision: Revision = {
  generada: '2026-10-28',
  empresa: 'EMPRESA DEMO SL',
  nif: 'B12345674',
  carpeta: '',
  resumen: {
    ficheros: 0,
    con_detalle: 0,
    sin_leer: 0,
    no_admitidos: 0,
    por_tipo: [],
    errores: 1,
    avisos: 1,
    info: 0,
    recuperable: 0,
  },
  hallazgos: [
    hallazgo('cruces.390_303', 'aviso', 2024, 'El 390 de 2024 no cuadra con sus 303'),
    hallazgo('modelos.faltan', 'error', 2025, 'Falta el 202 del 3P 2025'),
  ],
  tablas: [],
  pedir: [],
};

/** Los temas que se abren con la IA (para comprobar cuál pide cada botón). */
const abiertos: AssistantTopic[] = [];

const demo = (e: Evolucion, r: Revision, guardadas: ConversationSummary[] = []) =>
  setDemoInvoke(async (cmd, args) => {
    if (cmd === 'company_evolution') return e;
    if (cmd === 'company_review') return r;
    if (cmd === 'assistant_conversations') return guardadas;
    if (cmd === 'assistant_status')
      return {
        settings: { provider: { kind: 'Anthropic', model: 'claude-opus-5-5' }, consent: true },
        has_api_key: true,
        remote: true,
      };
    if (cmd === 'assistant_topic_open') {
      const topic = args?.topic as AssistantTopic;
      abiertos.push(topic);
      return {
        header: {
          group: 'Qué revisar · 2024',
          title: topic.kind === 'hallazgo' ? topic.title : '',
          facts: [],
          intro: 'Al comparar lo que tiene la AEAT con lo que sabe TaxPain sale esto: Detalle.',
          suggestions: ['¿Qué significa y es grave?'],
          notice: null,
        },
        conversation: null,
        changed: false,
      };
    }
    throw { message: `sin datos: ${cmd}` };
  });

describe('EvolucionPage', () => {
  it('enseña cada cifra con su variación y de dónde sale, y «sin dato» donde no hay', async () => {
    demo(evolucion, revision);
    render(<EvolucionPage />);
    const iva = await screen.findByRole('table', { name: 'IVA' });
    const fila = within(iva).getByRole('row', { name: /Volumen de operaciones/ });
    expect(fila.textContent).toContain('+50,0 %');
    expect(fila.textContent).toContain('390 [108]');
    const res = within(iva).getByRole('row', { name: /Resultado del IVA/ });
    expect(res.textContent).toContain('sin dato');
    expect(res.textContent).toContain('suma de los 303 [64]');
    // Los recuentos, sin euros.
    const decl = screen.getByRole('table', { name: 'Declaraciones' });
    expect(within(decl).getByRole('row', { name: /presentadas/ }).textContent).toContain('−25,0 %');
    expect(screen.getByText(/1 declaración no tiene sus casillas leídas/)).toBeTruthy();
    // Las tablas, en el orden en que llegan (el de importancia lo decide el backend).
    const tablas = screen.getAllByRole('table').map((t) => t.getAttribute('aria-label'));
    expect(tablas.slice(0, 2)).toEqual(['IVA', 'Declaraciones']);
  });

  it('el año sin 200 sale como avance «a medias», sin compararlo', async () => {
    demo(evolucion, revision);
    render(<EvolucionPage />);
    const t = await screen.findByRole('table', { name: 'Actividad y cuentas' });
    const fila = within(t).getByRole('row', { name: /Cifra de negocios/ });
    expect(within(fila).getByText('a medias')).toBeTruthy();
    expect(fila.textContent).toContain('avance con los 303 1T y 2T (sin 200)');
    expect(fila.textContent).not.toContain('%');
  });

  it('qué revisar: avisos con quién y hasta cuándo, filtrables', async () => {
    demo(evolucion, revision);
    render(<EvolucionPage />);
    expect(await screen.findByText('El 390 de 2024 no cuadra con sus 303')).toBeTruthy();
    expect(screen.getByText('Falta el 202 del 3P 2025')).toBeTruthy();
    expect(screen.getAllByText(/Se puede rectificar hasta el 20\/04\/2028/).length).toBe(1);
    fireEvent.change(screen.getByLabelText('Importancia'), { target: { value: 'error' } });
    expect(screen.queryByText('El 390 de 2024 no cuadra con sus 303')).toBeNull();
    expect(screen.getByText('Falta el 202 del 3P 2025')).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Importancia'), { target: { value: 'todas' } });
    fireEvent.change(screen.getByLabelText('Ejercicio'), { target: { value: '2024' } });
    expect(screen.queryByText('Falta el 202 del 3P 2025')).toBeNull();
  });

  it('qué revisar: cada aviso abre su conversación con la IA', async () => {
    demo(evolucion, revision, [
      {
        id: 5,
        topic: {
          kind: 'hallazgo',
          origin: 'aeat',
          code: 'modelos.faltan',
          year: 2025,
          title: 'Falta el 202 del 3P 2025',
        },
        title: 'Falta el 202 del 3P 2025',
        updated_at: '2026-10-30T10:00:00Z',
        questions: 2,
        materials: 0,
      },
    ]);
    render(<EvolucionPage />);
    expect(
      await screen.findByRole('button', {
        name: 'Seguir con la IA 2 preguntas: Falta el 202 del 3P 2025',
      }),
    ).toBeTruthy();
    const ask = screen.getByRole('button', {
      name: 'Preguntar a la IA: El 390 de 2024 no cuadra con sus 303',
    });
    fireEvent.click(ask);
    const dialog = await screen.findByRole('dialog', {
      name: 'Conversación con la IA: El 390 de 2024 no cuadra con sus 303',
    });
    expect(abiertos.at(-1)).toEqual({
      kind: 'hallazgo',
      origin: 'aeat',
      code: 'cruces.390_303',
      year: 2024,
      title: 'El 390 de 2024 no cuadra con sus 303',
    });
    fireEvent.keyDown(dialog, { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(document.activeElement).toBe(ask);
  });

  it('sin nada comprobado, lleva a Modelos', async () => {
    demo(
      { años: [], grupos: [], sin_comprobar: true, sin_casillas: 0 },
      { ...revision, hallazgos: [] },
    );
    let fue = '';
    render(
      <EvolucionPage
        onNavigate={(t) => {
          fue = t;
        }}
      />,
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Ir a Modelos' }));
    expect(fue).toBe('modelos');
    expect(screen.getByText('No hay nada que revisar con lo que tiene la AEAT.')).toBeTruthy();
  });
});

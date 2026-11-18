import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Ahorro, Recomendacion } from './api';

const api = vi.hoisted(() => ({
  taxSavings: vi.fn(),
  taxSavingsMark: vi.fn(),
  openSede: vi.fn(),
  assistantConversations: vi.fn(),
  assistantStatus: vi.fn(),
  assistantTopicOpen: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { AhorroPage } = await import('./Ahorro');

beforeEach(() => {
  api.assistantConversations.mockResolvedValue([]);
  api.assistantStatus.mockResolvedValue({
    settings: { provider: { kind: 'Anthropic', model: 'claude-opus-5-5' }, consent: true },
    has_api_key: true,
    remote: true,
  });
  api.assistantTopicOpen.mockImplementation((topic) =>
    Promise.resolve({
      header: {
        group: 'Ahorrar impuestos · 2026',
        title:
          topic.kind === 'ahorro_plan' ? 'Tu plan de ahorro de 2026' : 'Reserva de capitalización',
        facts: [],
        intro: 'Con tus datos, te conviene.',
        suggestions: [],
        notice: null,
      },
      conversation: null,
      changed: false,
    }),
  );
});

afterEach(() => {
  cleanup();
  for (const f of Object.values(api)) f.mockReset();
});

const base: Omit<Recomendacion, 'id' | 'titulo' | 'encaje' | 'ahorro'> = {
  area: 'beneficio',
  resumen: 'Qué es.',
  motivo: 'Por qué te aplica.',
  aplazado: false,
  calculo: null,
  plazo: null,
  requisitos: ['Requisito.'],
  pasos: ['Paso uno.', 'Paso dos.'],
  riesgos: [],
  norma: 'Art. 25 de la Ley 27/2014',
  enlace: 'https://www.boe.es/',
  pantalla: 'modelos',
  marca: { estado: 'pendiente', pasos: [] },
};

const DATA: Ahorro = {
  situacion: {
    año: 2026,
    cierre: '2026-12-31',
    persona_fisica: false,
    regimen: 'Microempresa (cifra de negocios menor de 1 M€)',
    cifra_negocios: { valor: 40_000_000, fuente: '200 de 2025 [00255]' },
    base: { valor: 10_000_000, fuente: 'borrador del 200 de 2026' },
    cuota: { valor: 2_000_000, fuente: 'borrador' },
    tipo: 21,
    trabajadores: false,
    cnae: '6201',
    notas: ['Las cifras de 2026 son las de la contabilidad hasta hoy.'],
  },
  recomendaciones: [
    {
      ...base,
      id: 'capitalizacion',
      titulo: 'Reserva de capitalización',
      encaje: 'conviene',
      ahorro: 252_000,
      plazo: '2026-12-31',
      calculo: '20 % del aumento',
    },
    {
      ...base,
      id: 'nivelacion',
      titulo: 'Reserva de nivelación',
      encaje: 'conviene',
      ahorro: 210_000,
      aplazado: true,
    },
    { ...base, id: 'patent_box', titulo: 'Patent box', encaje: 'no_aplica', ahorro: null },
  ],
  ahorro_posible: 252_000,
  aplazado_posible: 210_000,
};

describe('AhorroPage', () => {
  it('enseña el ahorro posible, la situación y lo que conviene', async () => {
    api.taxSavings.mockResolvedValue(DATA);
    render(<AhorroPage />);
    expect(
      await screen.findByRole('heading', { name: /podrías ahorrarte unos 2\.520,00 €/ }),
    ).toBeTruthy();
    expect(screen.getByText(/aplazar unos 2\.100,00 €/)).toBeTruthy();
    expect(screen.getByText('Microempresa (cifra de negocios menor de 1 M€)')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Reserva de capitalización' })).toBeTruthy();
    expect(
      screen.getByText('Cuenta lo que hagas hasta el cierre del ejercicio (31/12/2026).'),
    ).toBeTruthy();
    // Lo que no aplica no sale en «Para ti», sí en «Todas».
    expect(screen.queryByRole('heading', { name: 'Patent box' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Todas (3)' }));
    expect(screen.getByRole('heading', { name: 'Patent box' })).toBeTruthy();
  });

  it('marcar un paso la pone en marcha y guarda la marca', async () => {
    api.taxSavings.mockResolvedValue(DATA);
    api.taxSavingsMark.mockResolvedValue(undefined);
    render(<AhorroPage />);
    await screen.findByRole('heading', { name: 'Reserva de capitalización' });
    fireEvent.click(screen.getAllByLabelText(/Paso dos\./)[0] as HTMLElement);
    await waitFor(() =>
      expect(api.taxSavingsMark).toHaveBeenCalledWith('capitalizacion', {
        estado: 'en_marcha',
        pasos: [1],
      }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'No me interesa: Reserva de nivelación' }));
    await waitFor(() =>
      expect(api.taxSavingsMark).toHaveBeenCalledWith('nivelacion', {
        estado: 'descartado',
        pasos: [],
      }),
    );
    // Sale de «Para ti»: se anuncia dónde está y el foco pasa a otra tarjeta.
    expect((await screen.findByRole('status')).textContent).toBe(
      '«Reserva de nivelación»: no me interesa. Ahora está en «Descartadas».',
    );
    await waitFor(() =>
      expect(document.activeElement?.textContent).toBe('Reserva de capitalización'),
    );
  });

  it('al marcar «Hecho» quita su ahorro del total y lo confirma con el motor', async () => {
    api.taxSavings
      .mockResolvedValueOnce(DATA)
      .mockResolvedValue({ ...DATA, ahorro_posible: 0, aplazado_posible: 210_000 });
    api.taxSavingsMark.mockResolvedValue(undefined);
    render(<AhorroPage />);
    await screen.findByRole('heading', { name: /podrías ahorrarte unos 2\.520,00 €/ });
    fireEvent.click(screen.getByRole('button', { name: 'Hecho: Reserva de capitalización' }));
    expect(
      await screen.findByRole('heading', { name: 'Lo que puedes aprovechar en 2026' }),
    ).toBeTruthy();
    // El impuesto aplazado (nivelación) sigue.
    expect(screen.getByText(/aplazar unos 2\.100,00 €/)).toBeTruthy();
    await waitFor(() => expect(api.taxSavings).toHaveBeenCalledTimes(2));
  });

  it('lleva a la pantalla que ayuda', async () => {
    api.taxSavings.mockResolvedValue(DATA);
    const go = vi.fn();
    render(<AhorroPage onNavigate={go} />);
    await screen.findByRole('heading', { name: 'Reserva de capitalización' });
    fireEvent.click(screen.getAllByRole('button', { name: 'Abrir Modelos' })[0] as HTMLElement);
    expect(go).toHaveBeenCalledWith({ tab: 'modelos' });
  });

  it('cada recomendación abre su conversación con la IA', async () => {
    api.taxSavings.mockResolvedValue(DATA);
    render(<AhorroPage />);
    const ask = await screen.findByRole('button', {
      name: 'Preguntar a la IA: Reserva de capitalización',
    });
    fireEvent.click(ask);
    const dialog = await screen.findByRole('dialog', {
      name: 'Conversación con la IA: Reserva de capitalización',
    });
    expect(api.assistantTopicOpen).toHaveBeenCalledWith({
      kind: 'ahorro',
      id: 'capitalizacion',
      year: 2026,
    });
    // Al cerrarla, el foco vuelve a su botón.
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cerrar' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(document.activeElement).toBe(ask);
  });

  it('con una conversación guardada dice «Seguir con la IA» y cuántas preguntas', async () => {
    api.taxSavings.mockResolvedValue(DATA);
    api.assistantConversations.mockResolvedValue([
      {
        id: 3,
        topic: { kind: 'ahorro', id: 'capitalizacion', year: 2026 },
        title: 'Reserva de capitalización',
        updated_at: '2026-10-30T10:00:00Z',
        questions: 2,
        materials: 1,
      },
      // La de otro ejercicio no cuenta.
      {
        id: 4,
        topic: { kind: 'ahorro', id: 'nivelacion', year: 2025 },
        title: 'Reserva de nivelación',
        updated_at: '2025-10-30T10:00:00Z',
        questions: 1,
        materials: 0,
      },
    ]);
    render(<AhorroPage />);
    expect(
      await screen.findByRole('button', {
        name: 'Seguir con la IA 2 preguntas: Reserva de capitalización',
      }),
    ).toBeTruthy();
    expect(
      screen.getByRole('button', { name: 'Preguntar a la IA: Reserva de nivelación' }),
    ).toBeTruthy();
  });

  it('el resumen abre la conversación del plan del año', async () => {
    api.taxSavings.mockResolvedValue(DATA);
    render(<AhorroPage />);
    fireEvent.click(
      await screen.findByRole('button', { name: 'Preguntar a la IA: tu plan de ahorro de 2026' }),
    );
    expect(
      await screen.findByRole('dialog', {
        name: 'Conversación con la IA: Tu plan de ahorro de 2026',
      }),
    ).toBeTruthy();
    expect(api.assistantTopicOpen).toHaveBeenCalledWith({ kind: 'ahorro_plan', year: 2026 });
  });

  it('si no se pueden leer las conversaciones, los botones salen igual', async () => {
    api.taxSavings.mockResolvedValue(DATA);
    api.assistantConversations.mockRejectedValue({ message: 'sin almacén' });
    render(<AhorroPage />);
    expect(
      await screen.findByRole('button', { name: 'Preguntar a la IA: Reserva de capitalización' }),
    ).toBeTruthy();
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('dice el error si no se puede calcular', async () => {
    api.taxSavings.mockRejectedValue(new Error('sin empresa'));
    render(<AhorroPage />);
    expect((await screen.findByRole('alert')).textContent).toContain('sin empresa');
  });
});

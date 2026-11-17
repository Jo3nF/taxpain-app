import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  periodosListar: vi.fn(),
  periodosSugeridos: vi.fn(),
  periodoCerrar: vi.fn(),
  periodoReabrir: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));

const { Periodos } = await import('./Periodos');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Periodos', () => {
  it('cierra el sugerido con confirmación y reabre con confirmación', async () => {
    api.periodosListar.mockResolvedValue([
      {
        id: 1,
        tipo: 'trimestre',
        ejercicio: 2026,
        trimestre: 3,
        desde: '2026-07-01',
        hasta: '2026-09-30',
        motivo: '303 presentado',
        cerrado_en: '2026-10-20T09:15:00Z',
        etiqueta: '3T 2026',
      },
    ]);
    api.periodosSugeridos.mockResolvedValue([
      {
        tipo: 'trimestre',
        ejercicio: 2026,
        trimestre: 2,
        etiqueta: '2T 2026',
        motivo: '303 presentado',
      },
    ]);
    api.periodoCerrar.mockResolvedValue({});
    api.periodoReabrir.mockResolvedValue(undefined);
    render(<Periodos now={new Date('2026-11-17')} />);
    expect(await screen.findByText('3T 2026')).toBeTruthy();
    expect(screen.getByText(/del 01\/07\/2026 al 30\/09\/2026/)).toBeTruthy();
    fireEvent.click(await screen.findByRole('button', { name: 'Cerrar el 2T 2026' }));
    expect(api.periodoCerrar).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Cerrar el periodo' }));
    await vi.waitFor(() =>
      expect(api.periodoCerrar).toHaveBeenCalledWith('trimestre', 2026, 2, '303 presentado'),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Reabrir el 3T 2026' }));
    expect(api.periodoReabrir).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Reabrir' }));
    await vi.waitFor(() => expect(api.periodoReabrir).toHaveBeenCalledWith(1));
  });
});

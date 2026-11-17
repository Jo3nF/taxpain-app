import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({ financialRatios: vi.fn() }));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Ratios, show } = await import('./Ratios');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Ratios', () => {
  it('formatea cada unidad', () => {
    expect(show('veces', 153)).toBe('1,53');
    expect(show('porcentaje', 2800)).toBe('28 %');
    expect(show('dias', 7300)).toBe('73 días');
    expect(show('euros', 4_400_000)).toBe('44.000,00 €');
    expect(show('veces', null)).toBe('—');
  });

  it('agrupa, compara con el año anterior y marca lo que está fuera de rango', async () => {
    api.financialRatios.mockResolvedValue([
      {
        id: 'liquidez',
        group: 'Liquidez',
        name: 'Liquidez general',
        formula: 'AC / PC',
        explanation: 'x',
        unit: 'veces',
        value: 540,
        previous: 180,
        range: [150, 200],
        status: 'atencion',
      },
      {
        id: 'pmp',
        group: 'Actividad',
        name: 'Periodo medio de pago',
        formula: 'p',
        explanation: 'y',
        unit: 'dias',
        value: 4500,
        previous: null,
        range: [0, 6000],
        status: 'bien',
      },
    ]);
    render(<Ratios year={2026} />);
    expect(await screen.findByText('Liquidez general')).toBeTruthy();
    expect(screen.getByText('5,4')).toBeTruthy();
    expect(screen.getByText('1,8')).toBeTruthy();
    expect(screen.getByText('1,5 – 2')).toBeTruthy();
    expect(screen.getByText('≤ 60 días')).toBeTruthy();
    expect(screen.getByText('(fuera del rango)')).toBeTruthy();
  });
});

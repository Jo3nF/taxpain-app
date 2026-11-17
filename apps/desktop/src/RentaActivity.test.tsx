import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  rentaActivity: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { RentaActivity } = await import('./RentaActivity');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const draft = {
  year: 2025,
  regime: 'Simplificada',
  result: {
    year: 2025,
    regime: 'Simplificada',
    income: 3000000,
    expenses: 1100000,
    net_before: 2000000,
    hard_to_justify: 100000,
    net: 1900000,
    reduction_32_2: 0,
    reduction_start: 0,
    net_reduced: 1900000,
    withholdings: 450000,
    installments: 80000,
    boxes: [
      { code: '0180', label: 'Total ingresos computables', amount: 3000000 },
      {
        code: '0222',
        label: 'Provisiones deducibles y gastos de difícil justificación',
        amount: 100000,
      },
      { code: null, label: 'Gastos financieros', amount: 0 },
    ],
    warnings: [],
  },
  missing_130: ['4T'],
  check: [
    {
      topic: 'pago_130',
      label: 'Pagos fraccionados del 130',
      aeat: 120000,
      taxpain: 80000,
      matches: false,
    },
  ],
  check_date: '20260402',
  notes: ['No constan presentados estos 130 de 2025: 4T.'],
};

describe('RentaActivity', () => {
  it('muestra el rendimiento, las casillas y el cotejo', async () => {
    api.rentaActivity.mockResolvedValue(draft);
    render(<RentaActivity year={2025} />);
    expect(await screen.findByText('0222')).toBeTruthy();
    expect(api.rentaActivity).toHaveBeenCalledWith(2025);
    expect(screen.getByText('simplificada')).toBeTruthy();
    // La tarjeta dice a qué casilla corresponde (la 0226 es otra cifra).
    expect(screen.getByText('Rendimiento neto reducido total (casilla 0235)')).toBeTruthy();
    // Rendimiento neto y el 5 % de difícil justificación.
    expect(screen.getAllByText('19.000,00 €').length).toBeGreaterThan(0);
    expect(screen.getByText('1.000,00 €')).toBeTruthy();
    // Casilla sin número confirmado.
    expect(screen.getByText('—')).toBeTruthy();
    expect(screen.getByText(/02\/04\/2026/)).toBeTruthy();
    expect(screen.getByText('≠')).toBeTruthy();
    expect(screen.getByText(/No constan presentados/)).toBeTruthy();
  });

  it('cambia de ejercicio y enseña el error si no es persona física', async () => {
    api.rentaActivity.mockResolvedValueOnce(draft);
    api.rentaActivity.mockRejectedValueOnce('la Renta (modelo 100) es de personas físicas');
    render(<RentaActivity year={2025} />);
    await screen.findByText('0222');
    fireEvent.change(screen.getByLabelText('Ejercicio'), { target: { value: '2024' } });
    expect(await screen.findByRole('alert')).toBeTruthy();
    expect(api.rentaActivity).toHaveBeenLastCalledWith(2024);
  });
});

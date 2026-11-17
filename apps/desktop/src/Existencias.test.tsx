import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({ stockLines: vi.fn(), stockRegularize: vi.fn() }));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Existencias } = await import('./Existencias');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Existencias', () => {
  it('regulariza con el inventario final y una cuenta nueva', async () => {
    api.stockLines.mockResolvedValue([
      { account: '300', name: 'Mercaderías', initial: 500000, final_amount: 0 },
    ]);
    api.stockRegularize.mockResolvedValue(7);
    render(<Existencias year={2026} />);
    fireEvent.change(await screen.findByLabelText('Existencias finales de 300'), {
      target: { value: '7.000,00' },
    });
    fireEvent.change(screen.getByLabelText(/Añadir cuenta/), { target: { value: '350' } });
    fireEvent.click(screen.getByRole('button', { name: 'Añadir' }));
    fireEvent.change(screen.getByLabelText('Existencias finales de 350'), {
      target: { value: '1000' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Regularizar existencias' }));
    await vi.waitFor(() =>
      expect(api.stockRegularize).toHaveBeenCalledWith(2026, [
        { account: '300', amount: 700000 },
        { account: '350', amount: 100000 },
      ]),
    );
    expect(await screen.findByText('Existencias regularizadas.')).toBeTruthy();
  });
});

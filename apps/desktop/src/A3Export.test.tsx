import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  a3Options: vi.fn(),
  exportA3: vi.fn(),
  pickFolder: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));

const { A3Export } = await import('./A3Export');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('A3Export', () => {
  it('exporta con el código de empresa y los dígitos de a3', async () => {
    api.a3Options.mockResolvedValue({ company: 3, digits: 8, format: 'F5', invoices: true });
    api.pickFolder.mockResolvedValue('/tmp/a3');
    api.exportA3.mockResolvedValue({
      path: '/tmp/a3/SUENLACE.DAT',
      entries: 12,
      invoices: 5,
      accounts: 4,
      warnings: ['las facturas pasan como asientos'],
    });
    render(<A3Export year={2026} />);
    await vi.waitFor(() =>
      expect((screen.getByLabelText('Código de empresa en a3') as HTMLInputElement).value).toBe(
        '3',
      ),
    );
    fireEvent.change(screen.getByLabelText('Dígitos de las cuentas'), { target: { value: '10' } });
    fireEvent.click(screen.getByRole('button', { name: 'Exportar diario 2026 para a3…' }));
    expect(
      await screen.findByText(/12 asientos \(5 como facturas con IVA\) y 4 clientes/),
    ).toBeTruthy();
    expect(api.exportA3).toHaveBeenCalledWith(
      2026,
      { company: 3, digits: 10, format: 'F5', invoices: true },
      '/tmp/a3',
    );
    expect(screen.getByText('las facturas pasan como asientos')).toBeTruthy();
  });
});

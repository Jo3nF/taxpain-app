import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  actaSave: vi.fn(),
  dividendPayments: vi.fn(),
  dividendCertificates: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Actas } = await import('./Actas');

const payment = {
  id: 1,
  date: '2026-06-01',
  gross: 200_000,
  withheld: 38_000,
  net: 162_000,
  treasury: '5720001',
  entry_id: 40,
  lines: [],
};
const cert = {
  year: 2026,
  nif: '12345678Z',
  name: 'Ejemplo Uno, Ana',
  resident: true,
  lines: [{ date: '2026-06-01', gross: 120_000, rate: 1900, withheld: 22_800, net: 97_200 }],
  gross: 120_000,
  withheld: 22_800,
  net: 97_200,
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Actas', () => {
  it('guarda el acta de cuentas, la de dividendos, la de reservas y los certificados', async () => {
    api.dividendPayments.mockResolvedValue([payment]);
    api.dividendCertificates.mockResolvedValue([cert]);
    api.actaSave.mockResolvedValue('/docs/acta.rtf');
    render(<Actas year={2026} />);

    fireEvent.click(screen.getByRole('button', { name: 'Acta de cuentas (RTF)…' }));
    expect(await screen.findByText('Guardado en /docs/acta.rtf.')).toBeTruthy();
    expect(api.actaSave).toHaveBeenLastCalledWith(
      { kind: 'cuentas', year: 2026 },
      'Acta cuentas 2026',
      false,
    );

    fireEvent.click(await screen.findByRole('button', { name: 'Acta (PDF)…' }));
    expect(api.actaSave).toHaveBeenLastCalledWith(
      { kind: 'dividendo', payment_id: 1 },
      'Acta dividendos 2026-06-01',
      true,
    );

    // Certificados: uno por socio, con su retención.
    expect(await screen.findByText('Ejemplo Uno, Ana · 12345678Z')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Todos los certificados (PDF)…' }));
    expect(api.actaSave).toHaveBeenLastCalledWith(
      { kind: 'certificados', year: 2026, nif: null },
      'Certificados retenciones 2026',
      true,
    );

    // Reservas: sin importe no guarda.
    const calls = api.actaSave.mock.calls.length;
    fireEvent.click(screen.getByRole('button', { name: 'Acta de reservas (RTF)…' }));
    expect(await screen.findByText('Pon el importe del traspaso.')).toBeTruthy();
    expect(api.actaSave.mock.calls.length).toBe(calls);
    fireEvent.change(screen.getByLabelText('Importe'), { target: { value: '500' } });
    fireEvent.change(screen.getByLabelText('A la cuenta'), { target: { value: '1149' } });
    fireEvent.click(screen.getByRole('button', { name: 'Acta de reservas (RTF)…' }));
    expect(api.actaSave).toHaveBeenLastCalledWith(
      { kind: 'reservas', date: null, from: '113', to: '1149', amount: 50_000, reason: '' },
      'Acta reservas 2026',
      false,
    );
  });
});

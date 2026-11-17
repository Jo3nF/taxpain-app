import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  rentaCrossCheck: vi.fn(),
  rentaDownload: vi.fn(),
  listCertificates: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { RentaCheck } = await import('./RentaCheck');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('RentaCheck', () => {
  it('descarga con el certificado y marca las diferencias', async () => {
    api.rentaCrossCheck.mockResolvedValue(null);
    api.listCertificates.mockResolvedValue([
      {
        fingerprint_sha256: 'abc',
        holder_name: 'CARMEN',
        holder_nif: '12345678Z',
        subject_dn: 'x',
      },
    ]);
    api.rentaDownload.mockResolvedValue({
      year: 2025,
      process_date: '20260402',
      lines: [
        {
          topic: 'ingresos',
          label: 'CLIENTE UNO SL (B12345674): ingresos',
          aeat: 100000,
          taxpain: 100000,
          matches: true,
        },
        {
          topic: 'cotizaciones',
          label: 'Cotizaciones de autónomo (RETA)',
          aeat: 354000,
          taxpain: 0,
          matches: false,
        },
      ],
      notes: ['Las cuotas de autónomo son gasto deducible'],
    });
    render(<RentaCheck year={2025} />);
    const button = await screen.findByRole('button', { name: 'Descargar y cotejar' });
    await vi.waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(button);
    expect(await screen.findByText(/1 diferencia\(s\) que revisar/)).toBeTruthy();
    expect(api.rentaDownload).toHaveBeenCalledWith('abc', 2025);
    expect(screen.getByText('3.540,00 €')).toBeTruthy();
    expect(screen.getByText(/02\/04\/2026/)).toBeTruthy();
  });
});

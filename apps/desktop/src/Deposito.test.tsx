import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({ depositBuild: vi.fn(), pickFolder: vi.fn(), pickPdf: vi.fn() }));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Deposito } = await import('./Deposito');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Deposito', () => {
  it('prepara el ZIP para el D2 con los firmantes y la memoria', async () => {
    api.pickFolder.mockResolvedValue('/tmp');
    api.pickPdf.mockResolvedValue('/docs/memoria.pdf');
    api.depositBuild.mockResolvedValue({
      path: '/tmp/46030B1234567420251.ZIP',
      filename: '46030B1234567420251.ZIP',
      huella: 'x',
      claves: 42,
      warnings: ['completa en el D2 la hoja de titular real'],
    });
    render(<Deposito year={2025} />);
    fireEvent.change(screen.getByLabelText(/Código del Registro/), { target: { value: '46030' } });
    fireEvent.change(screen.getByLabelText('CNAE-2025'), { target: { value: '7410' } });
    fireEvent.change(screen.getByLabelText(/Administradores/), {
      target: { value: 'ANA UNO\n\nLUIS DOS ' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Adjuntar memoria (PDF)…' }));
    expect(await screen.findByText('memoria.pdf')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Preparar depósito 2025…' }));
    expect(await screen.findByText(/42 datos/)).toBeTruthy();
    expect(screen.getByText('completa en el D2 la hoja de titular real')).toBeTruthy();
    const [req, memoria, dir] = api.depositBuild.mock.calls[0] as [
      Record<string, unknown>,
      string,
      string,
    ];
    expect(req).toMatchObject({
      year: 2025,
      registry_code: '46030',
      cnae: '7410',
      signers: ['ANA UNO', 'LUIS DOS'],
    });
    expect([memoria, dir]).toEqual(['/docs/memoria.pdf', '/tmp']);
  });
});

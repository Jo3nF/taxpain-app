import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  auditoriaAcciones: vi.fn(),
  auditoriaConsultar: vi.fn(),
  auditoriaVerificar: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));

const { Auditoria } = await import('./Auditoria');

const ENTRADA = {
  seq: 7,
  at: '2026-10-15T11:00:00Z',
  actor: 'user',
  action: 'invoice.issue',
  subject: 'F-2026-0007',
  details: '{"total":121000}',
  prev_hash: 'a'.repeat(64),
  hash: 'b'.repeat(64),
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Auditoria', () => {
  it('lista, filtra por tipo de acción y verifica la cadena', async () => {
    api.auditoriaAcciones.mockResolvedValue([
      ['invoice.issue', 1],
      ['journal.manual', 2],
    ]);
    api.auditoriaConsultar.mockResolvedValue({ entradas: [ENTRADA], total: 1, siguiente: null });
    api.auditoriaVerificar.mockResolvedValue({
      ok: false,
      entradas: 0,
      rota_en: 5,
      mensaje: 'La entrada 5 no cuadra con su huella.',
    });
    render(<Auditoria />);
    expect(await screen.findByText('F-2026-0007')).toBeTruthy();
    await screen.findByRole('option', { name: 'invoice.issue (1)' });
    fireEvent.change(screen.getByLabelText('Tipo de acción'), { target: { value: 'invoice' } });
    fireEvent.click(screen.getByRole('button', { name: 'Buscar' }));
    await vi.waitFor(() =>
      expect(api.auditoriaConsultar).toHaveBeenLastCalledWith(
        expect.objectContaining({ accion: 'invoice', antes_de: null }),
      ),
    );
    fireEvent.click(screen.getByText('F-2026-0007'));
    expect(screen.getByText(/Huella: b+/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: /Verificar la cadena/ }));
    expect((await screen.findByRole('alert')).textContent).toContain('La entrada 5 no cuadra');
  });
});

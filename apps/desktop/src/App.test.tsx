import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({ appInfo: vi.fn(), ack: vi.fn() }));
vi.mock('./api', async (orig) => {
  const real = await orig<typeof import('./api')>();
  return {
    ...real,
    appInfo: (...a: []) => api.appInfo(...a) ?? real.appInfo(),
    auditRollbackAcknowledge: () => api.ack(),
  };
});

const { App } = await import('./App');

afterEach(() => {
  cleanup();
  api.appInfo.mockReset();
});

describe('App', () => {
  it('muestra el nombre y el modo vista previa fuera de Tauri', async () => {
    render(<App />);
    expect(screen.getByRole('heading', { name: 'TaxPain' })).toBeTruthy();
    expect(await screen.findByText(/Vista previa/)).toBeTruthy();
  });

  it('avisa si los datos son más antiguos que la última auditoría vista', async () => {
    api.appInfo.mockResolvedValue({
      version: '0.1.0',
      keys_secure: true,
      audit_rollback: { anchored_seq: 40, current_seq: 31 },
    });
    render(<App />);
    expect(await screen.findByText(/Tus datos son más antiguos/)).toBeTruthy();
    expect(screen.getByRole('alert').textContent).toContain('entrada 40');
  });

  it('sin ancla, otro texto; «Entendido» lo reconoce y lo quita', async () => {
    api.appInfo.mockResolvedValue({
      version: '0.1.0',
      keys_secure: true,
      audit_warning: {
        kind: 'anchor_missing',
        anchored_seq: null,
        current_seq: 12,
        detected_at: '2026-01-05T10:00:00Z',
      },
    });
    api.ack.mockResolvedValue(true);
    render(<App />);
    expect(await screen.findByText(/No se ha podido comprobar que el registro/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Entendido' }));
    expect(api.ack).toHaveBeenCalledTimes(1);
    await vi.waitFor(() => expect(screen.queryByText(/No se ha podido comprobar/)).toBeNull());
  });
});

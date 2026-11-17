import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({ pickBackupFile: vi.fn() }));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { setDemoInvoke } = await import('./api');
const { Backups } = await import('./Backups');

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
  vi.clearAllMocks();
});

const status = {
  has_passphrase: true,
  settings: { folder: null, auto: false, every_days: 7, keep: 10 },
  last: null,
  last_error: null,
  days_since: null,
  remind: true,
  suggested_folder: null,
  suggested_name: 'Ejemplo-SL-2026-10-24.taxpain',
  restored: null,
  min_passphrase: 10,
};

describe('Restaurar una copia', () => {
  it('pide confirmación si ya hay una empresa con el mismo NIF', async () => {
    api.pickBackupFile.mockResolvedValue('/Users/u/Ejemplo.taxpain');
    const calls: string[] = [];
    setDemoInvoke(async (cmd) => {
      calls.push(cmd);
      if (cmd === 'backup_status') return status;
      if (cmd === 'backup_inspect')
        return {
          created_at: '2026-10-20T18:30:00Z',
          app_version: '0.1.0',
          company: 'Ejemplo SL',
          nif: 'B12345674',
          invoices_issued: 4,
          invoices_received: 0,
          journal_entries: 4,
          last_invoice_date: '2026-10-15',
          newer_version: false,
          same_nif_company: 'Ejemplo SL',
        };
      if (cmd === 'backup_restore')
        return { id: '0123456789abcdef', name: 'Ejemplo SL', nif: 'B12345674', active: false };
      return null;
    });
    render(<Backups />);
    fireEvent.click(await screen.findByRole('button', { name: 'Elegir copia…' }));
    fireEvent.change(await screen.findByLabelText('Contraseña de la copia'), {
      target: { value: 'una contraseña larga' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Abrir la copia' }));
    expect(await screen.findByText(/ya tienes «Ejemplo SL» con el mismo NIF/)).toBeTruthy();
    const restore = screen.getByRole('button', {
      name: 'Restaurar como empresa nueva',
    }) as HTMLButtonElement;
    expect(restore.disabled).toBe(true);
    fireEvent.click(screen.getByRole('checkbox', { name: 'Entendido: no facturaré con las dos.' }));
    expect(restore.disabled).toBe(false);
    fireEvent.click(restore);
    expect(await screen.findByText(/Restaurada como empresa nueva/)).toBeTruthy();
    expect(calls).toContain('backup_restore');
  });
});

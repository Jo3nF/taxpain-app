import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { setDemoInvoke } from './api';
import { Backups, RestoreBanner } from './Backups';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const base = {
  has_passphrase: false,
  settings: { folder: null, auto: false, every_days: 7, keep: 10 },
  last: null,
  last_error: null,
  days_since: null,
  remind: true,
  suggested_folder: '/Users/u/iCloud/TaxPain/Copias de seguridad',
  suggested_name: 'Ejemplo-SL-2026-10-24.taxpain',
  restored: null,
  min_passphrase: 10,
  recovery_code: false,
  setup_required: false,
};

describe('Backups', () => {
  it('pide la contraseña antes de nada y comprueba que coincide', async () => {
    const calls: string[] = [];
    let st = base;
    setDemoInvoke(async (cmd, args) => {
      calls.push(cmd);
      if (cmd === 'backup_status') return st;
      if (cmd === 'backup_set_passphrase') {
        expect(args).toEqual({ passphrase: 'una frase bien larga' });
        st = { ...base, has_passphrase: true, recovery_code: true };
        return null;
      }
      return null;
    });
    render(<Backups />);
    const pass = await screen.findByLabelText('Contraseña de las copias');
    expect(screen.getByText(/todavía no tiene código de recuperación/)).toBeTruthy();
    fireEvent.change(pass, { target: { value: 'corta' } });
    fireEvent.change(screen.getByLabelText('Repítela'), { target: { value: 'corta' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar la contraseña' }));
    expect((await screen.findByRole('alert')).textContent).toContain('al menos 10');
    fireEvent.change(pass, { target: { value: 'una frase bien larga' } });
    fireEvent.change(screen.getByLabelText('Repítela'), {
      target: { value: 'otra frase distinta' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar la contraseña' }));
    expect((await screen.findByRole('alert')).textContent).toContain('no coinciden');
    fireEvent.change(screen.getByLabelText('Repítela'), {
      target: { value: 'una frase bien larga' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar la contraseña' }));
    expect(await screen.findByRole('button', { name: 'Hacer copia ahora' })).toBeTruthy();
    expect(screen.getByText('Todavía no has hecho ninguna copia.')).toBeTruthy();
    expect(screen.getByText(/Código de recuperación guardado junto a tus datos/)).toBeTruthy();
    expect(calls).toContain('backup_set_passphrase');
  });

  it('no deja activar las automáticas sin guardar los cambios', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'backup_status')
        return {
          ...base,
          has_passphrase: true,
          remind: false,
          days_since: 2,
          last: {
            at: '2026-10-22T10:00:00Z',
            path: '/x/Ejemplo.taxpain',
            bytes: 2048,
            auto: false,
          },
        };
      return null;
    });
    render(<Backups />);
    expect(await screen.findByText(/Última copia hace 2 días/)).toBeTruthy();
    const save = screen.getByRole('button', { name: 'Guardar' }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: /^Usar / }));
    expect(save.disabled).toBe(false);
  });
});

describe('RestoreBanner', () => {
  it('enseña la última factura y pide confirmación antes de emitir', async () => {
    const calls: string[] = [];
    setDemoInvoke(async (cmd) => {
      calls.push(cmd);
      if (cmd === 'restore_check')
        return {
          restored: {
            at: '2026-10-24T10:00:00Z',
            copy_created_at: '2026-10-20T10:00:00Z',
            acknowledged_at: null,
          },
          last_by_series: [['A', 'A-2026-0042', '2026-10-15']],
          last_verifactu: ['A-2026-0042', '15-10-2026', 'ABCDEF0123456789ABCDEF0123456789'],
        };
      return null;
    });
    render(<RestoreBanner />);
    expect(await screen.findByText('A-2026-0042')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Son las últimas: ya puedo emitir' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, son las últimas' }));
    await waitFor(() => expect(screen.queryByText('A-2026-0042')).toBeNull());
    expect(calls).toContain('restore_acknowledge');
  });
});

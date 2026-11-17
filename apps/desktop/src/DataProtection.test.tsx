import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  vaultLocked: vi.fn(),
  vaultRecover: vi.fn(),
  backupStatus: vi.fn(),
  backupSetPassphrase: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));

const { DataGate, RecoverVault } = await import('./DataProtection');

const PASS = 'caballo grapa batería';

function status(setup_required: boolean) {
  return { setup_required, min_passphrase: 10, has_passphrase: !setup_required };
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('DataGate', () => {
  it('entra en la app si los datos están abiertos y protegidos', async () => {
    api.vaultLocked.mockResolvedValue(null);
    api.backupStatus.mockResolvedValue(status(false));
    render(
      <DataGate>
        <p>la app</p>
      </DataGate>,
    );
    expect(await screen.findByText('la app')).toBeTruthy();
  });

  it('en el alta pide la contraseña de las copias antes de empezar', async () => {
    api.vaultLocked.mockResolvedValue(null);
    api.backupStatus.mockResolvedValue(status(true));
    api.backupSetPassphrase.mockResolvedValue(undefined);
    render(
      <DataGate>
        <p>la app</p>
      </DataGate>,
    );
    expect(await screen.findByText('Antes de empezar: protege tus datos')).toBeTruthy();
    expect(screen.getByText('Sin esta contraseña no se puede recuperar nada.')).toBeTruthy();
    expect(screen.queryByText('la app')).toBeNull();
    fireEvent.change(screen.getByLabelText('Contraseña de las copias'), {
      target: { value: PASS },
    });
    fireEvent.change(screen.getByLabelText('Repítela'), { target: { value: 'otra' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar la contraseña' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/no coinciden/);
    fireEvent.change(screen.getByLabelText('Repítela'), { target: { value: PASS } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar la contraseña' }));
    expect(await screen.findByText('la app')).toBeTruthy();
    expect(api.backupSetPassphrase).toHaveBeenCalledWith(PASS);
  });

  it('sin la clave en el llavero pide la contraseña para recuperar los datos', async () => {
    api.vaultLocked.mockResolvedValue({ several: false });
    render(
      <DataGate>
        <p>la app</p>
      </DataGate>,
    );
    expect(await screen.findByText('Recupera tus datos')).toBeTruthy();
    expect(screen.getByText(/el llavero no tiene la clave/)).toBeTruthy();
    expect(api.backupStatus).not.toHaveBeenCalled();
  });
});

describe('DataGate sin datos', () => {
  it('si no se han podido abrir los datos lo explica sin entrar en la app', async () => {
    api.vaultLocked.mockResolvedValue({ several: false, failed: 'El almacén está dañado.' });
    render(
      <DataGate>
        <p>la app</p>
      </DataGate>,
    );
    expect(await screen.findByText('TaxPain no puede abrir tus datos')).toBeTruthy();
    expect(screen.getByText('El almacén está dañado.')).toBeTruthy();
    expect(screen.queryByText('la app')).toBeNull();
    expect(api.backupStatus).not.toHaveBeenCalled();
  });
});

describe('RecoverVault', () => {
  it('recupera con la contraseña y avisa si no es', async () => {
    const onRecovered = vi.fn();
    api.vaultRecover.mockRejectedValueOnce({ message: 'No es la contraseña de las copias' });
    api.vaultRecover.mockResolvedValueOnce(undefined);
    render(<RecoverVault several onRecovered={onRecovered} />);
    expect(screen.getByText(/usa la de cualquiera de ellas/)).toBeTruthy();
    const button = screen.getByRole('button', { name: 'Recuperar mis datos' }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText('Contraseña de las copias'), {
      target: { value: 'mala' },
    });
    fireEvent.click(button);
    expect((await screen.findByRole('alert')).textContent).toMatch(/No es la contraseña/);
    expect(onRecovered).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText('Contraseña de las copias'), {
      target: { value: PASS },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Recuperar mis datos' }));
    await vi.waitFor(() => expect(onRecovered).toHaveBeenCalled());
    expect(api.vaultRecover).toHaveBeenLastCalledWith(PASS);
  });
});

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  companiesList: vi.fn(),
  companyCreate: vi.fn(),
  companySwitch: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));

const { COMPANIES_CHANGED, CompanySwitcher, initials } = await import('./CompanySwitcher');

beforeEach(() => {
  api.companiesList.mockResolvedValue([
    { id: 'principal', name: 'Carmen', nif: '12345678Z', active: true },
    { id: '0a1b2c3d4e5f6071', name: 'Estudio SL', nif: 'B12345674', active: false },
  ]);
  api.companySwitch.mockResolvedValue(undefined);
});
const PASS = 'caballo grapa batería';

/** Rellena la contraseña de las copias de la empresa nueva. */
function fillPassphrase(again = PASS) {
  fireEvent.change(screen.getByLabelText('Contraseña de las copias'), { target: { value: PASS } });
  fireEvent.change(screen.getByLabelText('Repítela'), { target: { value: again } });
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('CompanySwitcher', () => {
  it('iniciales sin la forma jurídica', () => {
    expect(initials('Estudio Norte SL')).toBe('EN');
    expect(initials('Carmen Española Española')).toBe('CE');
    expect(initials('Panadería, S.L.')).toBe('P');
  });

  it('cambia de empresa y recarga', async () => {
    const onSwitched = vi.fn();
    render(<CompanySwitcher onSwitched={onSwitched} />);
    expect(await screen.findByText('Carmen')).toBeTruthy();
    expect(screen.getByText('12345678Z')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: /^Empresa/ }));
    expect(screen.getByRole('menuitemradio', { name: /Carmen/ }).getAttribute('aria-checked')).toBe(
      'true',
    );
    fireEvent.click(screen.getByRole('menuitemradio', { name: /Estudio SL/ }));
    await vi.waitFor(() => expect(onSwitched).toHaveBeenCalled());
    expect(api.companySwitch).toHaveBeenCalledWith('0a1b2c3d4e5f6071');
  });

  it('no crea dos empresas con un doble envío', async () => {
    let finish: (c: unknown) => void = () => {};
    api.companyCreate.mockReturnValue(new Promise((r) => (finish = r)));
    const onSwitched = vi.fn();
    render(<CompanySwitcher onSwitched={onSwitched} />);
    await screen.findByText('Carmen');
    fireEvent.click(screen.getByRole('button', { name: /^Empresa/ }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Añadir empresa…' }));
    fireEvent.change(screen.getByLabelText('Nombre o razón social'), {
      target: { value: 'Nueva SL' },
    });
    fillPassphrase();
    const create = screen.getByRole('button', { name: 'Crear y abrir' }) as HTMLButtonElement;
    fireEvent.click(create);
    fireEvent.click(create);
    expect(api.companyCreate).toHaveBeenCalledTimes(1);
    expect(create.disabled).toBe(true);
    finish({ id: 'ffffffffffffffff', name: 'Nueva SL', nif: null, active: false });
    await vi.waitFor(() => expect(onSwitched).toHaveBeenCalledTimes(1));
    expect(api.companySwitch).toHaveBeenCalledTimes(1);
  });

  it('da de alta una empresa y la abre', async () => {
    api.companyCreate.mockResolvedValue({
      id: 'ffffffffffffffff',
      name: 'Nueva SL',
      nif: null,
      active: false,
    });
    const onSwitched = vi.fn();
    render(<CompanySwitcher onSwitched={onSwitched} />);
    await screen.findByText('Carmen');
    fireEvent.click(screen.getByRole('button', { name: /^Empresa/ }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Añadir empresa…' }));
    fireEvent.change(screen.getByLabelText('Nombre o razón social'), {
      target: { value: 'Nueva SL' },
    });
    fillPassphrase();
    fireEvent.click(screen.getByRole('button', { name: 'Crear y abrir' }));
    await vi.waitFor(() => expect(onSwitched).toHaveBeenCalled());
    expect(api.companyCreate).toHaveBeenCalledWith({ name: 'Nueva SL', nif: null }, PASS);
    expect(api.companySwitch).toHaveBeenCalledWith('ffffffffffffffff');
  });

  it('abre «Modificar o borrar empresas…» y relee la lista al cambiar', async () => {
    render(<CompanySwitcher onSwitched={vi.fn()} />);
    await screen.findByText('Carmen');
    fireEvent.click(screen.getByRole('button', { name: /^Empresa/ }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Modificar o borrar empresas…' }));
    const dialog = screen.getByRole('dialog', { name: 'Gestionar empresas' });
    expect(dialog.textContent).toMatch(/Estudio SL/);
    expect(screen.getByRole('button', { name: 'Borrar Estudio SL' })).toBeTruthy();
    expect(api.companiesList).toHaveBeenCalledTimes(1);
    window.dispatchEvent(new Event(COMPANIES_CHANGED));
    await vi.waitFor(() => expect(api.companiesList).toHaveBeenCalledTimes(2));
    fireEvent.click(screen.getByRole('button', { name: 'Cerrar' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole('button', { name: /^Empresa/ }));
  });

  it('pide la contraseña de las copias con confirmación', async () => {
    render(<CompanySwitcher onSwitched={vi.fn()} />);
    await screen.findByText('Carmen');
    fireEvent.click(screen.getByRole('button', { name: /^Empresa/ }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Añadir empresa…' }));
    fireEvent.change(screen.getByLabelText('Nombre o razón social'), {
      target: { value: 'Nueva SL' },
    });
    expect(screen.getByText(/no se puede recuperar nada/)).toBeTruthy();
    fillPassphrase('otra cosa distinta');
    fireEvent.submit(screen.getByRole('form', { name: 'Nueva empresa' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/no coinciden/);
    fireEvent.change(screen.getByLabelText('Contraseña de las copias'), {
      target: { value: 'corta' },
    });
    fireEvent.submit(screen.getByRole('form', { name: 'Nueva empresa' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/al menos 10/);
    expect(api.companyCreate).not.toHaveBeenCalled();
  });
});

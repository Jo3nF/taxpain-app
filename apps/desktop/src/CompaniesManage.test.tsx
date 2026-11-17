import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  companyUpdateId: vi.fn(),
  companyDelete: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));

const { CompaniesManage, confirmsDelete } = await import('./CompaniesManage');

const MI = { id: 'principal', name: 'Mi empresa', nif: null, active: true };
const DEMO = { id: '0a1b2c3d4e5f6071', name: 'EMPRESA DEMO SL', nif: 'B12345674', active: false };

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function show(list = [MI, DEMO]) {
  const props = { onClose: vi.fn(), onChanged: vi.fn(), onReload: vi.fn() };
  render(<CompaniesManage list={list} {...props} />);
  return props;
}

describe('CompaniesManage', () => {
  it('confirma con el nombre o el NIF', () => {
    expect(confirmsDelete(DEMO, ' empresa  demo sl ')).toBe(true);
    expect(confirmsDelete(DEMO, 'b12345674')).toBe(true);
    expect(confirmsDelete(DEMO, 'EMPRESA')).toBe(false);
    expect(confirmsDelete(MI, '')).toBe(false);
  });

  it('lista las empresas en un diálogo con nombre', () => {
    show();
    const dialog = screen.getByRole('dialog', { name: 'Gestionar empresas' });
    expect(dialog.textContent).toMatch(/Mi empresa/);
    expect(dialog.textContent).toMatch(/B12345674/);
    expect(screen.getByRole('button', { name: 'Modificar EMPRESA DEMO SL' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Borrar Mi empresa' })).toBeTruthy();
  });

  it('modifica una empresa que no está abierta y avisa a las listas', async () => {
    api.companyUpdateId.mockResolvedValue({ name: 'OTRA DEMO SL', nif: 'B98765431' });
    const props = show();
    fireEvent.click(screen.getByRole('button', { name: 'Modificar EMPRESA DEMO SL' }));
    const form = screen.getByRole('form', { name: 'Modificar EMPRESA DEMO SL' });
    expect((screen.getByLabelText('NIF (opcional)') as HTMLInputElement).value).toBe('B12345674');
    fireEvent.change(screen.getByLabelText('Nombre o razón social'), {
      target: { value: 'OTRA DEMO SL' },
    });
    fireEvent.change(screen.getByLabelText('NIF (opcional)'), { target: { value: 'B98765431' } });
    fireEvent.submit(form);
    await vi.waitFor(() => expect(props.onChanged).toHaveBeenCalled());
    expect(api.companyUpdateId).toHaveBeenCalledWith('0a1b2c3d4e5f6071', {
      name: 'OTRA DEMO SL',
      nif: 'B98765431',
    });
    expect(screen.queryByRole('form', { name: /Modificar/ })).toBeNull();
  });

  it('muestra el error al guardar', async () => {
    api.companyUpdateId.mockRejectedValue({ message: 'NIF: no es válido' });
    show();
    fireEvent.click(screen.getByRole('button', { name: 'Modificar EMPRESA DEMO SL' }));
    fireEvent.submit(screen.getByRole('form', { name: 'Modificar EMPRESA DEMO SL' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/NIF/);
  });

  it('no borra hasta escribir el nombre', async () => {
    api.companyDelete.mockResolvedValue(false);
    const props = show();
    fireEvent.click(screen.getByRole('button', { name: 'Borrar EMPRESA DEMO SL' }));
    const form = screen.getByRole('form', { name: 'Borrar «EMPRESA DEMO SL»' });
    expect(form.textContent).toMatch(/Papelera del Mac/);
    expect(form.textContent).toMatch(/copia de seguridad/);
    const button = screen.getByRole('button', { name: 'Borrar empresa' }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    const input = screen.getByLabelText(/Para confirmar/);
    fireEvent.change(input, { target: { value: 'EMPRESA' } });
    expect(button.disabled).toBe(true);
    fireEvent.change(input, { target: { value: 'EMPRESA DEMO SL' } });
    expect(button.disabled).toBe(false);
    fireEvent.click(button);
    await vi.waitFor(() => expect(props.onChanged).toHaveBeenCalled());
    expect(api.companyDelete).toHaveBeenCalledWith('0a1b2c3d4e5f6071', 'EMPRESA DEMO SL');
    expect(props.onReload).not.toHaveBeenCalled();
  });

  it('al borrar la abierta avisa de cuál se abre y recarga', async () => {
    api.companyDelete.mockResolvedValue(true);
    const props = show();
    fireEvent.click(screen.getByRole('button', { name: 'Borrar Mi empresa' }));
    expect(screen.getByText(/se abre «EMPRESA DEMO SL»/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText(/Para confirmar/), { target: { value: 'mi empresa' } });
    fireEvent.click(screen.getByRole('button', { name: 'Borrar empresa' }));
    await vi.waitFor(() => expect(props.onReload).toHaveBeenCalled());
  });

  it('muestra el error si no se puede borrar', async () => {
    api.companyDelete.mockRejectedValue({ message: 'no se pudo mover la empresa a la Papelera' });
    const props = show();
    fireEvent.click(screen.getByRole('button', { name: 'Borrar EMPRESA DEMO SL' }));
    fireEvent.change(screen.getByLabelText(/Para confirmar/), { target: { value: 'B12345674' } });
    fireEvent.click(screen.getByRole('button', { name: 'Borrar empresa' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/Papelera/);
    expect(props.onChanged).not.toHaveBeenCalled();
  });

  it('con una sola empresa no se ofrece borrar', () => {
    show([MI]);
    expect(screen.queryByRole('button', { name: /^Borrar/ })).toBeNull();
    expect(screen.getByText(/única empresa/)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Modificar Mi empresa' })).toBeTruthy();
  });

  it('Escape cierra el diálogo', () => {
    const props = show();
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(props.onClose).toHaveBeenCalled();
  });
});

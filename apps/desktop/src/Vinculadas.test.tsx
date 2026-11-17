import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { setDemoInvoke } from './api';
import { vinculadasHandlers } from './demoVinculadas';
import { Vinculadas, VinculoContacto } from './Vinculadas';
import { parsePct } from './vinculadasApi';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

function demo(calls: [string, Record<string, unknown> | undefined][] = []) {
  setDemoInvoke(async (cmd, args) => {
    calls.push([cmd, args]);
    const h = vinculadasHandlers[cmd];
    if (!h) throw new Error(`sin demo: ${cmd}`);
    return h(args);
  });
  return calls;
}

describe('Vinculadas', () => {
  it('convierte porcentajes', () => {
    expect(parsePct('50')).toBe(500_000);
    expect(parsePct('0')).toBeNull();
  });

  it('dice si toca el 232, lista vinculadas y operaciones y deja asignar subcuentas', async () => {
    const calls = demo();
    render(<Vinculadas year={2026} />);
    expect(
      await screen.findByText('Hay que presentar el modelo 232 de 2026 en noviembre de 2027.'),
    ).toBeTruthy();
    expect(screen.getAllByText('Carmen Española Española').length).toBeGreaterThan(0);
    // La retribución del administrador se ve, pero no va.
    expect(screen.getByText(/No va \(Retribución por el ejercicio del cargo/)).toBeTruthy();
    // Valoración de una operación.
    fireEvent.click(screen.getAllByRole('button', { name: 'Valoración' })[0] as HTMLElement);
    const form = screen.getByRole('form', { name: 'Valoración' });
    fireEvent.change(within(form).getByLabelText(/Comparables/), {
      target: { value: 'Tarifa a terceros' },
    });
    fireEvent.click(within(form).getByRole('button', { name: 'Guardar valoración' }));
    await screen.findByText('Hay que presentar el modelo 232 de 2026 en noviembre de 2027.');
    const saved = calls.find(([c]) => c === 'vinculada_valoracion_save');
    expect((saved?.[1]?.valoracion as { justificacion: string } | undefined)?.justificacion).toBe(
      'Tarifa a terceros',
    );
    // Subcuenta sin asignar.
    fireEvent.change(screen.getByLabelText('De quién es la subcuenta 5510001'), {
      target: { value: '' },
    });
    expect(calls.some(([c, a]) => c === 'vinculada_cuenta_set' && a?.nif === null)).toBe(true);
  });

  it('añade una vinculada a mano y pide confirmación para quitar una participada', async () => {
    const calls = demo();
    render(<Vinculadas year={2026} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Añadir vinculada' }));
    const form = screen.getByRole('form', { name: 'Ficha de vinculada' });
    fireEvent.change(within(form).getByLabelText('NIF'), { target: { value: '00000000T' } });
    fireEvent.change(within(form).getByLabelText('Apellidos y nombre o razón social'), {
      target: { value: 'Hermano Demo' },
    });
    fireEvent.click(within(form).getByRole('button', { name: 'Guardar vinculada' }));
    expect((await screen.findAllByText('Hermano Demo')).length).toBeGreaterThan(0);
    expect(calls.some(([c]) => c === 'vinculada_save')).toBe(true);
    fireEvent.click(
      screen.getByRole('button', { name: 'Quitar la participada Participada Demo SL' }),
    );
    expect(calls.some(([c]) => c === 'participada_delete')).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'Sí, quitar' }));
    await screen.findByText(/Ninguna\. Si la sociedad tiene participaciones/);
    expect(calls.some(([c]) => c === 'participada_delete')).toBe(true);
  });

  it('en la ficha de un contacto deja marcarlo como vinculado', async () => {
    const calls = demo();
    render(<VinculoContacto taxId="B98765431" name="Cliente Demo SL" country="ES" />);
    fireEvent.click(await screen.findByRole('button', { name: 'Marcar como vinculada' }));
    const form = screen.getByRole('form', { name: 'Ficha de vinculada' });
    expect((within(form).getByLabelText('NIF') as HTMLInputElement).value).toBe('B98765431');
    fireEvent.change(within(form).getByLabelText('Vínculo (art. 18.2 LIS)'), {
      target: { value: 'grupo' },
    });
    fireEvent.click(within(form).getByRole('button', { name: 'Guardar vinculada' }));
    expect(await screen.findByText('Entidad del grupo, art. 42 CCom (art. 18.2.d)')).toBeTruthy();
    const saved = calls.find(([c]) => c === 'vinculada_save');
    const v = saved?.[1]?.vinculada as { legal_person: boolean } | undefined;
    expect(v?.legal_person).toBe(true);
  });
});

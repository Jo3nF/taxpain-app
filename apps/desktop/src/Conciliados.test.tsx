import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  bankReconciled: vi.fn(),
  bankUnreconcile: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Conciliados, undoQuestion } = await import('./Conciliados');

const tx = (id: number, description: string, amount: number) => ({
  id,
  bank_account: 'Banco Demo',
  op_date: '2026-10-01',
  amount,
  description,
  matched: 'emitida',
  currency: 'EUR',
});

const factura = {
  tx: tx(1, 'TRANSFERENCIA CLIENTE DEMO', 121000),
  account: '5720001',
  entry_id: 7,
  kind: 'emitida' as const,
  counterpart: 'Factura A2026-0009',
  party: 'Cliente Demo SL',
  reconciled_at: '2026-10-02T09:00:00Z',
  rule: null,
  blocked: null,
};
const comision = {
  ...factura,
  tx: tx(2, 'COMISION MANTENIMIENTO', -1250),
  entry_id: 8,
  kind: 'cuenta' as const,
  counterpart: '626',
  party: 'Comisión',
  rule: 'comision mantenimiento',
  blocked: 'el modelo 303 del 4T de 2026 ya está presentado y cambiaría',
};

beforeEach(() => {
  api.bankReconciled.mockResolvedValue({
    accounts: [{ account: '5720001', name: 'Banco Demo' }],
    items: [factura, comision],
  });
  api.bankUnreconcile.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Conciliados', () => {
  it('lista, filtra y deshace con confirmación', async () => {
    const onDone = vi.fn();
    render(<Conciliados version={0} onDone={onDone} />);
    expect(api.bankReconciled).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Ver los movimientos conciliados…' }));
    expect(await screen.findByText(/Factura A2026-0009 · Cliente Demo SL/)).toBeTruthy();
    expect(api.bankReconciled).toHaveBeenCalledWith(null, null, null);
    // La bloqueada dice por qué y no tiene botón.
    expect(screen.getByText(/No se puede deshacer: el modelo 303/)).toBeTruthy();
    expect(screen.getAllByRole('button', { name: 'Deshacer conciliación' })).toHaveLength(1);

    fireEvent.change(screen.getByLabelText('Cuenta'), { target: { value: '5720001' } });
    fireEvent.change(screen.getByLabelText('Desde'), { target: { value: '2026-10-01' } });
    await vi.waitFor(() =>
      expect(api.bankReconciled).toHaveBeenLastCalledWith('5720001', '2026-10-01', null),
    );

    fireEvent.click(await screen.findByRole('button', { name: 'Deshacer conciliación' }));
    expect(api.bankUnreconcile).not.toHaveBeenCalled();
    expect(screen.getByRole('alertdialog').textContent).toContain('pendiente de cobro');
    fireEvent.click(screen.getByRole('button', { name: 'Sí, deshacer' }));
    await vi.waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(api.bankUnreconcile).toHaveBeenCalledWith(1, true);
  });

  it('explica qué pasa según con qué se concilió', () => {
    expect(undoQuestion({ ...factura, kind: 'traspaso' })).toContain('los dos movimientos');
    expect(
      undoQuestion({ ...factura, kind: 'impuesto', counterpart: 'Modelo 303 3T 2026' }),
    ).toContain('modelo 303 3T 2026 vuelve a estar pendiente');
  });
});

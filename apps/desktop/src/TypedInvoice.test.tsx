import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({ receivedRegisterTyped: vi.fn() }));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { TypedInvoice } = await import('./TypedInvoice');

afterEach(cleanup);

describe('TypedInvoice', () => {
  it('calcula IVA, retención y total y registra la factura', async () => {
    api.receivedRegisterTyped.mockResolvedValue(1);
    const saved = vi.fn();
    render(<TypedInvoice onClose={() => {}} onSaved={saved} />);
    const type = (label: string, value: string) =>
      fireEvent.change(screen.getByLabelText(label), { target: { value } });
    type('NIF del proveedor', 'b12345674');
    type('Nombre', 'Asesoría SL');
    type('Número', 'A-1');
    type('Base', '1000');
    type('Retención', '1500');
    expect(screen.getByText(/Total: 1\.?060,00/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Registrar la factura' }));
    await screen.findByText(/A-1 registrada/);
    expect(api.receivedRegisterTyped).toHaveBeenCalledWith(
      expect.objectContaining({
        supplier_nif: 'B12345674',
        base: 100000,
        vat: 21000,
        withholding: 15000,
        total: 106000,
      }),
      null,
    );
    expect(saved).toHaveBeenCalled();
  });

  it('tras registrar vuelve a «Sin retención», «Compra o servicio» y 100 % deducible', async () => {
    api.receivedRegisterTyped.mockResolvedValue(1);
    render(<TypedInvoice onClose={() => {}} onSaved={() => {}} />);
    const type = (label: string, value: string) =>
      fireEvent.change(screen.getByLabelText(label), { target: { value } });
    type('NIF del proveedor', 'B12345674');
    type('Número', 'A-2');
    type('Base', '500');
    type('Retención', '1900');
    type('Naturaleza', 'alquiler');
    type('IVA deducible %', '50');
    fireEvent.click(screen.getByRole('button', { name: 'Registrar la factura' }));
    await screen.findByText(/A-2 registrada/);
    expect((screen.getByLabelText('Retención') as HTMLSelectElement).value).toBe('0');
    expect((screen.getByLabelText('Naturaleza') as HTMLSelectElement).value).toBe('general');
    expect((screen.getByLabelText('IVA deducible %') as HTMLInputElement).value).toBe('100');
    expect((screen.getByLabelText('NIF del proveedor') as HTMLInputElement).value).toBe(
      'B12345674',
    );
  });

  it('avisa de que el IVA de una factura de un trimestre anterior se deduce ahora', () => {
    render(<TypedInvoice onClose={() => {}} onSaved={() => {}} />);
    expect(screen.queryByText(/trimestre anterior/)).toBeNull();
    fireEvent.change(screen.getByLabelText('Fecha'), { target: { value: '2020-01-15' } });
    expect(screen.getByText(/trimestre anterior/)).toBeTruthy();
  });
});

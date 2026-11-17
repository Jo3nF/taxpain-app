import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  carteraList: vi.fn(),
  paymentTerms: vi.fn(),
  setPaymentTerms: vi.fn(),
  setDueDate: vi.fn(),
  settleInvoice: vi.fn(),
  unsettleInvoice: vi.fn(),
  setInstallments: vi.fn(),
  partyTermsList: vi.fn(),
  setPartyTerms: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Cartera } = await import('./Cartera');

const item = (over: Record<string, unknown>) => ({
  kind: 'emitida',
  invoice_id: 1,
  seq: 1,
  installments: 1,
  party_account: '4300001',
  invoice_amount: 121000,
  method: null,
  treasury: null,
  fixed: false,
  number: 'A2026-0001',
  party: 'Cliente SA',
  issue_date: '2026-08-01',
  due_date: '2026-08-31',
  amount: 121000,
  status: 'pendiente',
  settled_on: null,
  overdue_days: 43,
  ...over,
});

beforeEach(() => {
  api.carteraList.mockResolvedValue([
    item({}),
    item({
      invoice_id: 2,
      number: 'A2026-0002',
      due_date: '2026-12-31',
      amount: 60500,
      invoice_amount: 60500,
      overdue_days: 0,
    }),
    item({
      kind: 'recibida',
      invoice_id: 7,
      number: 'P-7',
      party: 'Proveedor SA',
      amount: 24200,
      invoice_amount: 24200,
      overdue_days: 0,
      status: 'registrada',
      settled_on: '2026-10-13',
    }),
  ]);
  api.paymentTerms.mockResolvedValue({ customers_days: 30, suppliers_days: 30 });
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Cartera', () => {
  it('resume lo pendiente y lo vencido y registra un cobro en caja', async () => {
    api.settleInvoice.mockResolvedValue(5);
    render(<Cartera />);
    expect(await screen.findByText('Vencida hace 43 d')).toBeTruthy();
    expect(screen.getByText('1.815,00 €')).toBeTruthy(); // pendiente de cobro
    expect(screen.getByText('1.210,00 €', { selector: 'dd' })).toBeTruthy(); // cobros vencidos
    // Por defecto sólo pendientes: el pago ya registrado no sale.
    expect(screen.queryByText('P-7')).toBeNull();
    fireEvent.click(screen.getAllByRole('button', { name: 'Registrar cobro…' })[0] as HTMLElement);
    fireEvent.change(screen.getByLabelText('Cuenta de tesorería de A2026-0001'), {
      target: { value: '5720001' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Registrar cobro' }));
    await vi.waitFor(() =>
      expect(api.settleInvoice).toHaveBeenCalledWith(
        'emitida',
        1,
        1,
        expect.any(String),
        '5720001',
        null,
        'transferencia',
      ),
    );
    expect(await screen.findByText('Cobro de A2026-0001 registrado.')).toBeTruthy();
  });

  it('en caja propone «efectivo» como forma de cobro (lo que el 347 declara en metálico)', async () => {
    api.settleInvoice.mockResolvedValue(6);
    render(<Cartera />);
    await screen.findByText('Vencida hace 43 d');
    fireEvent.click(screen.getAllByRole('button', { name: 'Registrar cobro…' })[0] as HTMLElement);
    const forma = screen.getByLabelText('Forma de cobro de A2026-0001') as HTMLSelectElement;
    expect(forma.value).toBe('efectivo');
    fireEvent.change(forma, { target: { value: 'tarjeta' } });
    fireEvent.click(screen.getByRole('button', { name: 'Registrar cobro' }));
    await vi.waitFor(() =>
      expect(api.settleInvoice).toHaveBeenCalledWith(
        'emitida',
        1,
        1,
        expect.any(String),
        '570',
        null,
        'tarjeta',
      ),
    );
  });

  it('avisa del límite de pagos en efectivo al cobrar en caja 1.000 € o más', async () => {
    api.carteraList.mockResolvedValue([item({ cash_limited: true })]);
    render(<Cartera />);
    await screen.findByText('Vencida hace 43 d');
    fireEvent.click(screen.getByRole('button', { name: 'Registrar cobro…' }));
    // La cuenta por defecto es la 570 (caja): aviso no bloqueante con la norma.
    expect(screen.getByRole('note').textContent).toMatch(/Ley 7\/2012/);
    expect(screen.getByRole('button', { name: 'Registrar cobro' })).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Cuenta de tesorería de A2026-0001'), {
      target: { value: '5720001' },
    });
    expect(screen.queryByRole('note')).toBeNull();
  });

  it('muestra todas, deshace un pago y cambia un vencimiento', async () => {
    api.unsettleInvoice.mockResolvedValue(undefined);
    api.setDueDate.mockResolvedValue(undefined);
    render(<Cartera />);
    await screen.findByText('Vencida hace 43 d');
    fireEvent.change(screen.getByLabelText('Mostrar'), { target: { value: 'todas' } });
    expect(screen.getByText('P-7')).toBeTruthy();
    expect(screen.getByText(/Pagada 13\/10\/2026/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Deshacer' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, deshacer' }));
    await vi.waitFor(() =>
      expect(api.unsettleInvoice).toHaveBeenCalledWith('recibida', 7, 1, true),
    );
    fireEvent.change(screen.getByLabelText('Vencimiento de A2026-0002'), {
      target: { value: '2027-01-15' },
    });
    await vi.waitFor(() =>
      expect(api.setDueDate).toHaveBeenCalledWith('emitida', 2, 1, '2027-01-15'),
    );
  });

  it('fracciona una factura en dos vencimientos', async () => {
    api.setInstallments.mockResolvedValue(undefined);
    render(<Cartera />);
    await screen.findByText('Vencida hace 43 d');
    expect(screen.getByRole('table', { name: 'Antigüedad de lo vencido' })).toBeTruthy();
    fireEvent.click(screen.getAllByRole('button', { name: 'Fraccionar…' })[0] as HTMLElement);
    const dialog = screen.getByRole('dialog', { name: 'Vencimientos de A2026-0001' });
    expect(dialog).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: '2' }));
    expect(screen.getByText('Suma 1.210,00 € de 1.210,00 €')).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Importe del vencimiento 1'), {
      target: { value: '600' },
    });
    expect(
      (screen.getByRole('button', { name: 'Guardar vencimientos' }) as HTMLButtonElement).disabled,
    ).toBe(true);
    fireEvent.change(screen.getByLabelText('Importe del vencimiento 2'), {
      target: { value: '610,00' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar vencimientos' }));
    await vi.waitFor(() =>
      expect(api.setInstallments).toHaveBeenCalledWith('emitida', 1, [
        { due_date: '2026-08-31', amount: 60000 },
        { due_date: '2026-09-30', amount: 61000 },
      ]),
    );
    expect(await screen.findByText('Vencimientos de A2026-0001 guardados.')).toBeTruthy();
  });

  it('pone condiciones de pago a un cliente', async () => {
    api.partyTermsList.mockResolvedValue([
      { account: '4300001', role: 'cliente', tax_id: 'B12345674', name: 'Cliente SA', terms: null },
    ]);
    api.setPartyTerms.mockResolvedValue(undefined);
    render(<Cartera />);
    fireEvent.click(screen.getByRole('button', { name: 'Condiciones de pago' }));
    expect(await screen.findByText('Plazo general')).toBeTruthy();
    expect(screen.getByText(/Ninguno todavía/)).toBeTruthy(); // sin proveedores
    fireEvent.click(screen.getByRole('button', { name: 'Poner condiciones' }));
    fireEvent.change(screen.getByLabelText(/Días de cada plazo/), { target: { value: '30, 60' } });
    fireEvent.change(screen.getByLabelText(/Día de pago/), { target: { value: '10' } });
    fireEvent.change(screen.getByLabelText(/Forma/), { target: { value: 'recibo' } });
    fireEvent.change(screen.getByLabelText(/Cuenta/), { target: { value: '5720001' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    await vi.waitFor(() =>
      expect(api.setPartyTerms).toHaveBeenCalledWith('4300001', {
        days: [30, 60],
        pay_day: 10,
        method: 'recibo',
        treasury: '5720001',
        iban: null,
        mandate_ref: null,
        mandate_date: null,
        key_190: null,
      }),
    );
    expect(await screen.findByText('Condiciones de Cliente SA guardadas.')).toBeTruthy();
  });

  it('fija la clave del 190 de un proveedor profesional', async () => {
    api.partyTermsList.mockResolvedValue([
      {
        account: '4100001',
        role: 'proveedor',
        tax_id: '12345678Z',
        name: 'Abogada Ruiz',
        terms: null,
      },
    ]);
    api.setPartyTerms.mockResolvedValue(undefined);
    render(<Cartera />);
    fireEvent.click(screen.getByRole('button', { name: 'Condiciones de pago' }));
    await screen.findByText('Plazo general');
    fireEvent.click(screen.getByRole('button', { name: 'Poner condiciones' }));
    fireEvent.change(screen.getByLabelText(/Clave del 190/), { target: { value: 'G03' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    await vi.waitFor(() =>
      expect(api.setPartyTerms).toHaveBeenCalledWith(
        '4100001',
        expect.objectContaining({ key_190: 'G03' }),
      ),
    );
    expect(await screen.findByText('Condiciones de Abogada Ruiz guardadas.')).toBeTruthy();
  });
});

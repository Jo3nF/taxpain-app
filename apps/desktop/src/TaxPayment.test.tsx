import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  taxPayment: vi.fn(),
  taxPay: vi.fn(),
  taxPayBank: vi.fn(),
  taxUnpay: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { TaxPaymentPanel } = await import('./TaxPayment');

const cargo = {
  id: 7,
  bank_account: 'Banco',
  op_date: '2026-10-20',
  amount: -75_600,
  description: 'AEAT MOD 303',
  matched: null,
};

beforeEach(() => {
  api.taxPayment.mockResolvedValue({ payable: true, payment: null, candidates: [cargo] });
  api.taxPay.mockResolvedValue({});
  api.taxPayBank.mockResolvedValue({});
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('TaxPaymentPanel', () => {
  it('ofrece el cargo del banco por el importe exacto y el pago a mano', async () => {
    render(<TaxPaymentPanel model="303" year={2026} quarter={3} amount={75_600} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Es este cargo' }));
    expect(api.taxPayBank).toHaveBeenCalledWith('303', 2026, 3, 7);
    // Mientras se registra, los botones esperan (sin dobles pagos).
    await screen.findByText('Pago registrado y movimiento conciliado.');
    fireEvent.change(screen.getByLabelText('Cuenta de pago'), { target: { value: '570' } });
    fireEvent.click(screen.getByRole('button', { name: 'Registrar pago' }));
    expect(api.taxPay).toHaveBeenCalledWith('303', 2026, 3, expect.any(String), '570');
  });

  it('un doble clic registra el pago una sola vez', async () => {
    render(<TaxPaymentPanel model="303" year={2026} quarter={3} amount={75_600} />);
    const pay = await screen.findByRole('button', { name: 'Registrar pago' });
    fireEvent.click(pay);
    fireEvent.click(pay);
    await screen.findByText('Pago registrado.');
    expect(api.taxPay).toHaveBeenCalledTimes(1);
  });

  it('deshacer el pago pide confirmación', async () => {
    api.taxPayment.mockResolvedValue({
      payable: true,
      payment: {
        date: '2026-10-20',
        amount: 75_600,
        account: '4750',
        treasury: '572',
        bank_tx: null,
      },
      candidates: [],
    });
    api.taxUnpay.mockResolvedValue({});
    render(<TaxPaymentPanel model="303" year={2026} quarter={3} amount={75_600} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Deshacer' }));
    expect(api.taxUnpay).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Sí, deshacer el pago' }));
    await vi.waitFor(() => expect(api.taxUnpay).toHaveBeenCalledWith('303', 2026, 3));
  });

  it('nada si el modelo sale a compensar o no se paga', async () => {
    api.taxPayment.mockResolvedValue({ payable: false, payment: null, candidates: [] });
    const { container } = render(
      <TaxPaymentPanel model="390" year={2026} quarter={4} amount={100} />,
    );
    await Promise.resolve();
    expect(container.textContent).toBe('');
  });
});

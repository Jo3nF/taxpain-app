import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { type BankTx, setDemoInvoke } from './api';
import { ContravalorMovimiento, SaldosCuentas } from './CuentasDivisa';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const tx: BankTx = {
  id: 5,
  bank_account: 'Cuenta en dólares',
  op_date: '2026-04-01',
  amount: -50_000,
  description: 'SAAS TOOLS',
  matched: null,
  currency: 'USD',
  amount_eur: null,
  fx_rate: null,
  fx_source: null,
};

describe('CuentasDivisa', () => {
  it('un movimiento en euros no lleva contravalor', () => {
    const { container } = render(
      <ContravalorMovimiento m={{ ...tx, currency: 'EUR' }} onDone={() => {}} />,
    );
    expect(container.textContent).toBe('');
  });

  it('guarda los euros del extracto con el signo del movimiento', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    let done = 0;
    setDemoInvoke(async (cmd, args) => {
      calls.push({ cmd, args });
      return null;
    });
    render(<ContravalorMovimiento m={tx} onDone={() => done++} />);
    expect(screen.getByText(/Sin contravalor en euros/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Euros de -500,00 USD'), {
      target: { value: '415,00' },
    });
    fireEvent.click(screen.getByText('Guardar euros'));
    await screen.findByText(/Sin contravalor/);
    await new Promise((r) => setTimeout(r, 0));
    expect(calls[0]).toEqual({ cmd: 'bank_set_fx', args: { tx: 5, eur: -41_500, rate: null } });
    expect(done).toBe(1);
  });

  it('enseña el contravalor y los saldos en divisa', async () => {
    render(
      <ContravalorMovimiento
        m={{ ...tx, amount_eur: -41_667, fx_rate: '1.2', fx_source: 'bce' }}
        onDone={() => {}}
      />,
    );
    expect(screen.getByText(/1 € = 1,2 USD, tipo del BCE/)).toBeTruthy();
    setDemoInvoke(async () => [
      {
        name: 'Cuenta en dólares',
        account: '5730001',
        currency: 'USD',
        kind: 'cuenta',
        saldo: 200_000,
        contable: 160_000,
        tipo: '1.1',
        tipo_fecha: '2026-11-13',
        al_tipo: 181_818,
        sin_contravalor: 2,
      },
    ]);
    render(<SaldosCuentas version={0} />);
    expect(await screen.findByText('2.000,00 USD')).toBeTruthy();
    expect(screen.getByText('2 movimientos sin contravalor en euros')).toBeTruthy();
  });
});

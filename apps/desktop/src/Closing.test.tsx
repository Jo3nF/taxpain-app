import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setDemoInvoke } from './api';
import { GuidedClosing } from './Closing';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

describe('GuidedClosing', () => {
  it('con el ejercicio cerrado propone la aplicación y la contabiliza', async () => {
    const posted: unknown[] = [];
    setDemoInvoke(async (cmd, args) => {
      if (cmd === 'closing_checklist')
        return {
          year: 2026,
          start: '2026-01-01',
          end: '2026-12-31',
          closed: true,
          company: true,
          lsc: true,
          items: [{ id: 'cierre', label: 'Ejercicio cerrado', ok: true, detail: 'Cerrado.' }],
          deadlines: [],
        };
      if (cmd === 'closing_allocation')
        return {
          year: 2026,
          result: 1_000_000,
          capital: 300_000,
          legal_reserve: 0,
          legal_minimum: 60_000,
          losses: 0,
          equity: 1_300_000,
          required_compensation: 0,
          max_dividends: 1_000_000,
          capitalization_minimum: 50_000,
          interim_dividend: 0,
          suggested: {
            legal: 60_000,
            losses: 0,
            voluntary: 890_000,
            dividends: 0,
            remanente: 0,
            capitalization: 50_000,
          },
          earliest: '2027-01-01',
          meeting_deadline: '2027-06-30',
          entry_id: null,
          warnings: ['La 129 trae 100,00 € de ejercicios anteriores sin aplicar'],
        };
      if (cmd === 'closing_allocation_post') {
        posted.push(args);
        return 1;
      }
      if (cmd === 'shareholders_list' || cmd === 'dividend_payments') return [];
      throw { message: `inesperado ${cmd}` };
    });
    render(
      <GuidedClosing year={2026}>
        <p>cerrar</p>
      </GuidedClosing>,
    );
    expect(await screen.findByText(/Beneficio de 10\.000,00 €/)).toBeTruthy();
    expect(screen.getByText(/Mínimo a la reserva legal: 600,00 €/)).toBeTruthy();
    expect(screen.getByText(/reserva de capitalización, al menos 500,00 €/)).toBeTruthy();
    expect(screen.getByText(/Dividendos: como mucho 10\.000,00 €/)).toBeTruthy();
    expect(screen.getByText(/La 129 trae 100,00 €/)).toBeTruthy();
    expect(
      (screen.getByLabelText('Reserva de capitalización (1149)') as HTMLInputElement).value,
    ).toBe('500,00');
    // 2.000 € de dividendos salen de las voluntarias.
    fireEvent.change(screen.getByLabelText('Dividendos (526)'), { target: { value: '2.000' } });
    fireEvent.change(screen.getByLabelText('Reservas voluntarias (113)'), {
      target: { value: '6.900' },
    });
    expect(screen.getByText('Suma 10.000,00 € de 10.000,00 €.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Contabilizar la aplicación' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, contabilizar' }));
    await vi.waitFor(() => expect(posted).toHaveLength(1));
    expect(posted[0]).toEqual({
      year: 2026,
      date: '2027-06-30',
      allocation: {
        legal: 60_000,
        losses: 0,
        capitalization: 50_000,
        voluntary: 690_000,
        dividends: 200_000,
        remanente: 0,
      },
      confirmed: true,
    });
  });

  it('con la 4700 y la 4750 con saldo a la vez propone el ajuste y lo contabiliza al confirmar', async () => {
    const posted: unknown[] = [];
    setDemoInvoke(async (cmd, args) => {
      if (cmd === 'closing_checklist')
        return {
          year: 2026,
          start: '2026-01-01',
          end: '2026-12-31',
          closed: false,
          company: false,
          lsc: false,
          items: [
            {
              id: 'iva_saldos',
              label: 'Saldos de la 4700 y la 4750',
              ok: false,
              detail: 'La 4700 tiene 53,00 € y la 4750 53,00 € a la vez.',
            },
          ],
          deadlines: [],
        };
      if (cmd === 'closing_vat_balances')
        return posted.length
          ? null
          : {
              year: 2026,
              date: '2026-12-31',
              debtor: 5_300,
              creditor: 5_300,
              amount: 5_300,
              lines: [
                { account: '4750', name: 'HP, acreedora por IVA', debit: 5_300, credit: 0 },
                { account: '4700', name: 'HP, deudora por IVA', debit: 0, credit: 5_300 },
              ],
            };
      if (cmd === 'closing_vat_balances_post') {
        posted.push(args);
        return 7;
      }
      throw { message: `inesperado ${cmd}` };
    });
    render(
      <GuidedClosing year={2026}>
        <p>cerrar</p>
      </GuidedClosing>,
    );
    expect(await screen.findByText(/La 4700 \(IVA a compensar\) tiene 53,00 €/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Contabilizar el ajuste' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, contabilizar' }));
    await vi.waitFor(() => expect(posted).toHaveLength(1));
    expect(posted[0]).toEqual({ year: 2026, confirmed: true });
    // Una vez contabilizado, el ajuste deja de proponerse.
    await vi.waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Contabilizar el ajuste' })).toBeNull(),
    );
  });
});

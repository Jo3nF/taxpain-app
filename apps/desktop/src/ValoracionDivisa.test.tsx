import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { setDemoInvoke } from './api';
import { ValoracionDivisa } from './ValoracionDivisa';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const proposal = (posted: boolean) => ({
  year: 2026,
  date: '2026-12-31',
  reversal_date: '2027-01-01',
  closed: false,
  rates: [{ currency: 'USD', rate: posted ? '1.1' : null, date: null, source: null }],
  items: [
    {
      kind: 'banco',
      account: '5730001',
      label: 'Cuenta en dólares',
      currency: 'USD',
      amount: 200_000,
      amount_source: 'extracto',
      booked: 160_000,
      valued: posted ? 181_818 : null,
      difference: posted ? 21_818 : null,
    },
  ],
  lines: posted
    ? [
        { account: '5730001', name: 'Cuenta en dólares', debit: 21_818, credit: 0 },
        { account: '768', name: 'Diferencias positivas de cambio', debit: 0, credit: 21_818 },
      ]
    : [],
  gains: posted ? 21_818 : 0,
  losses: 0,
  entry_id: null as number | null,
  reversal_id: null,
  stale: false,
  input: { tipos: {}, saldos: {} },
  warnings: posted ? [] : ['Falta el tipo de cierre del USD.'],
});

describe('ValoracionDivisa', () => {
  it('sin partidas en divisa no enseña nada', async () => {
    setDemoInvoke(async () => ({ ...proposal(false), items: [] }));
    const { container } = render(<ValoracionDivisa year={2026} onChanged={() => {}} />);
    await new Promise((r) => setTimeout(r, 0));
    expect(container.textContent).toBe('');
  });

  it('con el tipo tecleado propone el asiento y lo contabiliza', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    let posted = false;
    setDemoInvoke(async (cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'closing_fx_valuation') {
        const p = proposal(Boolean((args as { input?: unknown })?.input) || posted);
        return posted ? { ...p, entry_id: 7, reversal_id: 8 } : p;
      }
      if (cmd === 'closing_fx_valuation_post') {
        posted = true;
        return 7;
      }
      return null;
    });
    render(<ValoracionDivisa year={2026} onChanged={() => {}} />);
    expect(await screen.findByText('Falta el tipo de cierre del USD.')).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Tipo de cierre del USD'), {
      target: { value: '1,10' },
    });
    fireEvent.click(screen.getByText('Recalcular con lo tecleado'));
    expect(await screen.findByText('Contabilizar la valoración')).toBeTruthy();
    const recalc = calls.filter((c) => c.cmd === 'closing_fx_valuation').at(-1);
    expect(recalc?.args).toMatchObject({ input: { tipos: { USD: '1.10' } } });
    fireEvent.click(screen.getByText('Contabilizar la valoración'));
    fireEvent.click(await screen.findByText('Sí, contabilizar'));
    expect(await screen.findByText('Contabilizada')).toBeTruthy();
    const post = calls.find((c) => c.cmd === 'closing_fx_valuation_post');
    expect(post?.args).toMatchObject({ year: 2026, confirmed: true });
  });
});

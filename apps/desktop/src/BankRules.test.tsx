import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  bankRuleProposals: vi.fn(),
  bankRules: vi.fn(),
  bankRuleApply: vi.fn(),
  bankRuleDelete: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { BankRules } = await import('./BankRules');

const tx = (id: number, description: string) => ({
  id,
  bank_account: 'Banco',
  op_date: '2026-10-01',
  amount: -1200,
  description,
  matched: null,
});

beforeEach(() => {
  api.bankRuleProposals.mockResolvedValue([
    { tx: tx(1, 'COMISION MANTENIMIENTO 1012'), account: '626', pattern: 'comision mantenimiento' },
    { tx: tx(2, 'COMISION MANTENIMIENTO 1112'), account: '626', pattern: 'comision mantenimiento' },
  ]);
  api.bankRules.mockResolvedValue([
    { id: 9, pattern: 'comision mantenimiento', sign: 'cargo', account: '626', hits: 3 },
  ]);
  api.bankRuleApply.mockResolvedValue(1);
  api.bankRuleDelete.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('BankRules', () => {
  it('contabiliza sólo los marcados y borra reglas', async () => {
    const onDone = vi.fn();
    render(<BankRules version={0} onDone={onDone} />);
    const boxes = await screen.findAllByRole('checkbox');
    fireEvent.click(boxes[1] as HTMLElement);
    fireEvent.click(screen.getByRole('button', { name: 'Contabilizar los marcados (1)' }));
    await vi.waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(api.bankRuleApply).toHaveBeenCalledWith([1]);
    fireEvent.click(screen.getByRole('button', { name: 'Borrar la regla comision mantenimiento' }));
    expect(api.bankRuleDelete).toHaveBeenCalledWith(9);
  });
});

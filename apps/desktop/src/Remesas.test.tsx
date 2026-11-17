import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  remittanceCandidates: vi.fn(),
  remittanceBanks: vi.fn(),
  remittanceList: vi.fn(),
  remittanceCreate: vi.fn(),
  remittanceSave: vi.fn(),
  remittanceUndo: vi.fn(),
  remittanceReturn: vi.fn(),
  remittanceUndoReturn: vi.fn(),
  sepaSettings: vi.fn(),
  saveSepaSettings: vi.fn(),
  pickFolder: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Remesas } = await import('./Remesas');

const due = (over: Record<string, unknown>) => ({
  kind: 'emitida',
  invoice_id: 1,
  seq: 1,
  installments: 1,
  number: 'A2026-0001',
  party: 'Cliente SA',
  party_account: '4300001',
  issue_date: '2026-09-01',
  due_date: '2026-10-01',
  amount: 121000,
  invoice_amount: 121000,
  status: 'pendiente',
  settled_on: null,
  overdue_days: 0,
  method: 'recibo',
  treasury: null,
  fixed: false,
  remittance: null,
  ...over,
});

const remittance = {
  id: 3,
  kind: 'cobros',
  reference: 'REM-2026-0001',
  bank_iban: 'ES9121000418450200051332',
  charge_date: '2026-10-20',
  total: 121000,
  in_bank: true,
  created_at: '2026-10-18T10:00:00Z',
  items: [
    {
      kind: 'emitida',
      invoice_id: 1,
      seq: 1,
      number: 'A2026-0001',
      party: 'Cliente SA',
      amount: 121000,
      returned_on: null,
    },
  ],
};

beforeEach(() => {
  api.remittanceCandidates.mockResolvedValue([
    {
      item: due({}),
      iban: 'ES7921000813610123456789',
      mandate_ref: 'M-1',
      mandate_date: '2026-01-15',
      problem: null,
    },
    {
      item: due({ invoice_id: 2, number: 'A2026-0002', party: 'Sin Mandato SL', amount: 5000 }),
      iban: 'ES7921000813610123456789',
      mandate_ref: null,
      mandate_date: null,
      problem: 'falta el mandato SEPA de Sin Mandato SL (referencia y fecha de firma)',
    },
  ]);
  api.remittanceBanks.mockResolvedValue([
    { name: 'Banco (1332)', iban: 'ES9121000418450200051332', account: '5720001' },
  ]);
  api.remittanceList.mockResolvedValue([remittance]);
  api.sepaSettings.mockResolvedValue({ creditor_suffix: '000', b2b: false });
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Remesas', () => {
  it('genera una remesa con los recibos que tienen mandato y guarda el fichero', async () => {
    api.remittanceCreate.mockResolvedValue({ ...remittance, id: 4, reference: 'REM-2026-0002' });
    api.pickFolder.mockResolvedValue('/tmp/x');
    api.remittanceSave.mockResolvedValue('/tmp/x/REM-2026-0002.xml');
    render(<Remesas />);
    expect(await screen.findByText(/falta el mandato SEPA/)).toBeTruthy();
    expect((screen.getByLabelText('Incluir A2026-0002') as HTMLInputElement).disabled).toBe(true);
    const go = await screen.findByRole('button', {
      name: /Generar remesa \(1 recibo, 1\.210,00 €\)/,
    });
    // Espera a que se carguen las cuentas del banco (sin IBAN el botón está desactivado).
    await vi.waitFor(() => expect((go as HTMLButtonElement).disabled).toBe(false));
    expect(screen.getByLabelText(/Cuenta de la empresa/)).toBeTruthy();
    fireEvent.click(go);
    await vi.waitFor(() =>
      expect(api.remittanceCreate).toHaveBeenCalledWith(
        'cobros',
        [{ invoice_id: 1, seq: 1 }],
        'ES9121000418450200051332',
        expect.any(String),
      ),
    );
    expect(await screen.findByText(/REM-2026-0002 generada.*REM-2026-0002\.xml/)).toBeTruthy();
    expect(api.remittanceSave).toHaveBeenCalledWith(4, '/tmp/x');
  });

  it('anota la devolución de un recibo de una remesa ya abonada', async () => {
    api.remittanceReturn.mockResolvedValue(9);
    render(<Remesas />);
    fireEvent.click(await screen.findByRole('button', { name: /REM-2026-0001/ }));
    expect(screen.getByText('Abonada por el banco')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Deshacer' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Devuelto…' }));
    expect(
      (screen.getByLabelText('Cuenta del banco de la devolución de A2026-0001') as HTMLInputElement)
        .value,
    ).toBe('5720001');
    fireEvent.click(screen.getByRole('button', { name: 'Anotar devolución' }));
    await vi.waitFor(() =>
      expect(api.remittanceReturn).toHaveBeenCalledWith(3, 1, 1, expect.any(String), '5720001'),
    );
    expect(await screen.findByText('A2026-0001 vuelve a estar pendiente.')).toBeTruthy();
  });

  it('deshace una devolución anotada por error', async () => {
    api.remittanceList.mockResolvedValue([
      {
        ...remittance,
        total: 0,
        items: [{ ...remittance.items[0], returned_on: '2026-10-22' }],
      },
    ]);
    api.remittanceUndoReturn.mockResolvedValue(undefined);
    render(<Remesas />);
    fireEvent.click(await screen.findByRole('button', { name: /REM-2026-0001/ }));
    expect(screen.getByText('Devuelto el 22/10/2026')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Deshacer devolución' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, deshacer' }));
    await vi.waitFor(() => expect(api.remittanceUndoReturn).toHaveBeenCalledWith(3, 1, 1, true));
    expect(await screen.findByText('A2026-0001 vuelve a la remesa REM-2026-0001.')).toBeTruthy();
  });
});

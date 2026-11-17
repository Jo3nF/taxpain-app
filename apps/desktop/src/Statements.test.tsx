import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  annualAccounts: vi.fn(),
  bankMovements: vi.fn(),
  bankSuggestions: vi.fn(),
  bankReconcileInvoice: vi.fn(),
  bankReconcileAccount: vi.fn(),
  bankReconcileReturn: vi.fn(),
  bankImport: vi.fn(),
  pickStatementFiles: vi.fn(),
  settleVat: vi.fn(),
  redoVat: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
vi.mock('./A3Export', () => ({ A3Export: () => null }));
vi.mock('./CuentasModelo', () => ({
  ModeloCuentas: () => null,
  MemoriaDraft: () => null,
  EcpnView: () => null,
}));
vi.mock('./ImportEntries', () => ({ ImportEntries: () => null }));
vi.mock('./Legalizacion', () => ({ Legalizacion: () => null }));
const { AnnualAccountsView, Bank, TaxActions, describeImport, importWarnings } = await import(
  './Statements'
);

describe('Statements', () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('avisa de cuentas sin partida y muestra las partidas con importe', async () => {
    api.annualAccounts.mockImplementation(async (y: number) => ({
      balanced: true,
      income: {
        lines: [
          {
            id: '1',
            label: '1. Importe neto de la cifra de negocios.',
            amount: y === 2026 ? 100000 : 80000,
            leaf: true,
          },
        ],
        unmapped: [],
      },
      balance: {
        lines: [
          { id: 'A', label: 'A) ACTIVO NO CORRIENTE', amount: 0, leaf: false },
          { id: 'A.I', label: 'I. Inmovilizado intangible.', amount: 0, leaf: true },
        ],
        unmapped: y === 2026 ? [{ account: '473', debit: 15000, credit: 0 }] : [],
      },
    }));
    render(<AnnualAccountsView year={2026} />);
    expect((await screen.findByRole('alert')).textContent).toContain('473');
    expect(screen.getByText('1.000,00 €')).toBeTruthy();
    // Comparativo con el año anterior.
    expect(await screen.findByText('800,00 €')).toBeTruthy();
    expect(screen.getByText('2025')).toBeTruthy();
    expect(screen.queryByText('I. Inmovilizado intangible.')).toBeNull();
  });

  it('propone la factura y concilia', async () => {
    api.bankMovements.mockResolvedValue([
      {
        id: 5,
        bank_account: 'Banco',
        op_date: '2026-10-01',
        amount: 121000,
        description: 'TRANSFERENCIA CLIENTE',
        matched: null,
      },
    ]);
    api.bankSuggestions.mockResolvedValue([
      {
        kind: 'emitida',
        id: 9,
        number: 'A2026-0009',
        party: 'Cliente SL',
        date: '2026-09-20',
        due_date: '2026-10-20',
        seq: 2,
        installments: 2,
        amount: 121000,
      },
    ]);
    api.bankReconcileInvoice.mockResolvedValue(1);
    render(<Bank />);
    fireEvent.click(await screen.findByText(/TRANSFERENCIA CLIENTE/));
    fireEvent.click(await screen.findByText(/Cobro de A2026-0009 \(plazo 2\/2\)/));
    await vi.waitFor(() =>
      expect(api.bankReconcileInvoice).toHaveBeenCalledWith(5, 'emitida', 9, 2),
    );
  });
  it('propone el recibo devuelto ya anotado en Remesas y lo casa con su asiento', async () => {
    api.bankMovements.mockResolvedValue([
      {
        id: 6,
        bank_account: 'Banco',
        op_date: '2026-10-05',
        amount: -60500,
        description: 'DEVOLUCION RECIBO',
        matched: null,
      },
    ]);
    api.bankSuggestions.mockResolvedValue([
      {
        kind: 'devolucion',
        id: 31,
        number: 'REM-2026-0001',
        party: 'Devolución recibo A2026-0003 · remesa REM-2026-0001',
        date: '2026-10-05',
        due_date: '2026-10-05',
        seq: 0,
        installments: 0,
        amount: 60500,
      },
    ]);
    api.bankReconcileReturn.mockResolvedValue(undefined);
    render(<Bank />);
    fireEvent.click(await screen.findByText(/DEVOLUCION RECIBO/));
    fireEvent.click(
      await screen.findByText(
        'Devolución recibo A2026-0003 · remesa REM-2026-0001 (anotada el 05/10/2026)',
      ),
    );
    await vi.waitFor(() => expect(api.bankReconcileReturn).toHaveBeenCalledWith(6, 31));
  });

  it('importa extractos de cualquier formato y deja forzar los que no cuadran', async () => {
    api.bankMovements.mockResolvedValue([
      {
        id: 7,
        bank_account: 'Cuenta en dólares',
        op_date: '2026-10-03',
        amount: -2500,
        description: 'SOFTWARE DEMO',
        matched: null,
        currency: 'USD',
      },
    ]);
    api.pickStatementFiles.mockResolvedValue(['/x/enero.pdf', '/x/febrero.xlsx']);
    const extracto = {
      cuenta: 'Banco Demo …1332 EUR',
      cuenta_id: 1,
      cuenta_nueva: false,
      subcuenta: '5720001',
      formato: 'PDF',
      clase: 'Cuenta' as const,
      divisa: 'EUR',
      desde: '2026-01-01',
      hasta: '2026-01-31',
      movimientos: 12,
      movimientos_nuevos: 10,
      ya_estaban: 2,
      saldos_completados: 0,
      ya_contabilizados: 0,
      cuadre: { estado: 'cuadra' as const },
      importado: true,
      motivo: '',
      avisos: [],
    };
    api.bankImport.mockResolvedValueOnce([
      {
        fichero: 'enero.pdf',
        resumen: {
          fichero: 'enero.pdf',
          extractos: [extracto],
          movimientos_nuevos: 10,
          ya_estaban: 2,
        },
        error: null,
      },
      {
        fichero: 'febrero.xlsx',
        resumen: {
          fichero: 'febrero.xlsx',
          movimientos_nuevos: 0,
          ya_estaban: 0,
          extractos: [
            {
              ...extracto,
              movimientos_nuevos: 0,
              ya_estaban: 0,
              importado: false,
              motivo: 'no cuadra con sus saldos',
              cuadre: { estado: 'no_cuadra' as const, diferencia: -500, fila: 3, detalle: 'falta' },
            },
          ],
        },
        error: null,
      },
    ]);
    api.bankImport.mockResolvedValueOnce([]);
    render(<Bank />);
    expect(await screen.findByText('-25,00 USD')).toBeTruthy();
    fireEvent.click(screen.getByText('Importar extractos del banco…'));
    expect(
      await screen.findByText(
        'enero.pdf: Banco Demo …1332 EUR: 10 movimientos nuevos, 2 ya estaban',
      ),
    ).toBeTruthy();
    expect(
      screen.getByText(
        'febrero.xlsx: Banco Demo …1332 EUR: no se ha importado (no cuadra con sus saldos)',
      ),
    ).toBeTruthy();
    expect(api.bankImport).toHaveBeenCalledWith(['/x/enero.pdf', '/x/febrero.xlsx'], false);
    fireEvent.click(screen.getByText('Importar igualmente los que no cuadran'));
    await vi.waitFor(() =>
      expect(api.bankImport).toHaveBeenLastCalledWith(['/x/febrero.xlsx'], true),
    );
  });

  it('describe un fichero que no se ha podido leer', () => {
    expect(describeImport({ fichero: 'raro.pdf', resumen: null, error: 'no es un extracto' })).toBe(
      'raro.pdf: no es un extracto',
    );
    expect(
      importWarnings({ fichero: 'raro.pdf', resumen: null, error: 'no es un extracto' }),
    ).toEqual([]);
  });

  it('dice «1 ya estaba» en singular', () => {
    const e = {
      cuenta: 'Banco Demo …1332 EUR',
      cuenta_id: 1,
      cuenta_nueva: false,
      subcuenta: '5720001',
      formato: 'Norma 43',
      clase: 'Cuenta' as const,
      divisa: 'EUR',
      desde: '2026-10-01',
      hasta: '2026-10-31',
      movimientos: 2,
      movimientos_nuevos: 1,
      ya_estaban: 1,
      saldos_completados: 0,
      ya_contabilizados: 0,
      cuadre: { estado: 'cuadra' as const },
      importado: true,
      motivo: '',
      avisos: [],
    };
    expect(
      describeImport({
        fichero: 'oct.n43',
        resumen: { fichero: 'oct.n43', extractos: [e], movimientos_nuevos: 1, ya_estaban: 1 },
        error: null,
      }),
    ).toBe('oct.n43: Banco Demo …1332 EUR: 1 movimiento nuevo, 1 ya estaba');
  });

  it('enseña los avisos de la importación debajo de cada fichero', async () => {
    api.bankMovements.mockResolvedValue([]);
    api.pickStatementFiles.mockResolvedValue(['/x/tarjeta.xlsx']);
    const base = {
      cuenta_id: 1,
      cuenta_nueva: false,
      subcuenta: '5720001',
      formato: 'Hoja de cálculo',
      clase: 'Cuenta' as const,
      divisa: 'EUR',
      desde: '2026-01-01',
      hasta: '2026-01-31',
      movimientos: 3,
      movimientos_nuevos: 3,
      ya_estaban: 0,
      saldos_completados: 0,
      ya_contabilizados: 0,
      cuadre: { estado: 'sin_saldos' as const },
      importado: true,
      motivo: '',
    };
    api.bankImport.mockResolvedValueOnce([
      {
        fichero: 'tarjeta.xlsx',
        resumen: {
          fichero: 'tarjeta.xlsx',
          movimientos_nuevos: 6,
          ya_estaban: 0,
          extractos: [
            {
              ...base,
              cuenta: 'Banco Demo …1332 EUR',
              avisos: [
                'El extracto no trae saldos: no se ha podido comprobar que estén todos los movimientos.',
                'Posible duplicado: el movimiento del 05/01/2026 de 12,00 € ya estaba.',
              ],
            },
            {
              ...base,
              cuenta: 'Tarjeta de crédito Banco Demo …1234 EUR',
              clase: 'TarjetaCredito' as const,
              avisos: [
                'La tarjeta daba las compras en positivo: se les ha dado la vuelta para que resten.',
              ],
            },
          ],
        },
        error: null,
      },
    ]);
    render(<Bank />);
    fireEvent.click(screen.getByText('Importar extractos del banco…'));
    const duplicado = await screen.findByText(
      'Banco Demo …1332 EUR: Posible duplicado: el movimiento del 05/01/2026 de 12,00 € ya estaba.',
    );
    expect(duplicado.className).toContain('text-amber-800');
    const saldos = screen.getByText(/no trae saldos/);
    expect(saldos.className).not.toContain('text-amber-800');
    expect(
      screen.getByText(
        'Tarjeta de crédito Banco Demo …1234 EUR: La tarjeta daba las compras en positivo: se les ha dado la vuelta para que resten.',
      ).className,
    ).toContain('text-amber-800');
  });

  it('ofrece rehacer la liquidación del IVA del trimestre liquidado', async () => {
    api.settleVat.mockResolvedValue(7);
    api.redoVat.mockResolvedValue(8);
    render(<TaxActions year={2026} />);
    fireEvent.click(screen.getByRole('button', { name: '1T' }));
    expect(await screen.findByText('IVA del 1T liquidado.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Rehacer la liquidación del 1T' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, rehacer' }));
    expect(await screen.findByText('Liquidación del 1T rehecha con el 303 de ahora.')).toBeTruthy();
    expect(api.redoVat).toHaveBeenCalledWith(2026, 1);
  });
});

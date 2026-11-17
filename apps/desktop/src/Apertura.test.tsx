import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { OpeningDraft, OpeningSummary } from './api';

const api = vi.hoisted(() => ({
  aperturaDraft: vi.fn(),
  aperturaSave: vi.fn(),
  aperturaDiscard: vi.fn(),
  aperturaApplied: vi.fn(),
  aperturaReadBalance: vi.fn(),
  aperturaReadPending: vi.fn(),
  aperturaBanks: vi.fn(),
  aperturaSummary: vi.fn(),
  aperturaApply: vi.fn(),
  aperturaUndo: vi.fn(),
  fiscalYearInfo: vi.fn(),
  pickOpeningFile: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Apertura } = await import('./Apertura');

const draft = (patch: Partial<OpeningDraft> = {}): OpeningDraft => ({
  date: '2026-01-01',
  step: 1,
  source: null,
  lines: [
    { account: '1000000', name: 'Capital social', debit: 0, credit: 300_000 },
    { account: '5720001', name: 'Banco', debit: 300_000, credit: 0 },
  ],
  customers: [],
  suppliers: [],
  assets: [],
  banks: [],
  tax: { vat_refund: 0, vat_due: 0, withholdings: 0 },
  ...patch,
});

const summary = (patch: Partial<OpeningSummary> = {}): OpeningSummary => ({
  fiscal_year: 2026,
  fiscal_year_label: '2026',
  fiscal_year_start: '2026-01-01',
  lines: [
    {
      account: '1000000',
      name: 'Capital social',
      debit: 0,
      credit: 300_000,
      from: 'balance',
      new_account: false,
    },
    {
      account: '5720001',
      name: 'Banco',
      debit: 300_000,
      credit: 0,
      from: 'balance',
      new_account: false,
    },
  ],
  debit: 300_000,
  credit: 300_000,
  difference: 0,
  errors: [],
  warnings: [],
  duplicate: null,
  also: [],
  ...patch,
});

function setup(d: OpeningDraft) {
  api.aperturaDraft.mockResolvedValue(d);
  api.aperturaApplied.mockResolvedValue([]);
  api.aperturaSave.mockResolvedValue(undefined);
  api.aperturaBanks.mockResolvedValue([]);
  api.fiscalYearInfo.mockResolvedValue({
    year: 2026,
    start: '2026-01-01',
    end: '2026-12-31',
    label: '2026',
    natural: true,
    closed: false,
  });
  render(<Apertura year={2026} />);
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Apertura', () => {
  it('sigue en el paso guardado, suma el balance y guarda al cambiar de paso', async () => {
    setup(draft());
    expect(await screen.findByText(/Paso 2 de 7: Balance del gestor/)).toBeTruthy();
    expect(screen.getByText(/cuadra\./)).toBeTruthy();

    // Un importe que no cuadra: se ve la diferencia.
    const haber = screen.getByLabelText('Haber de la línea 1');
    fireEvent.change(haber, { target: { value: '2.500,00' } });
    fireEvent.blur(haber);
    expect(await screen.findByText(/diferencia 500,00 €/, {}, { timeout: 5000 })).toBeTruthy();
    expect(screen.getByText('Hay cambios sin guardar.')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Siguiente' }));
    await waitFor(() => expect(api.aperturaSave).toHaveBeenCalled());
    const saved = api.aperturaSave.mock.calls[0]?.[0] as OpeningDraft;
    expect(saved.step).toBe(2);
    expect(saved.lines[0]?.credit).toBe(250_000);
    expect(await screen.findByText(/Paso 3 de 7: Clientes pendientes/)).toBeTruthy();
  });

  it('importa el balance del gestor', async () => {
    setup(draft({ lines: [], date: null }));
    api.pickOpeningFile.mockResolvedValue('/tmp/sumas.xlsx');
    api.aperturaReadBalance.mockResolvedValue({
      kind: 'sumas_saldos',
      lines: draft().lines,
      date: null,
      warnings: ['fila 9: el importe de la cuenta 4300001 no se entiende; no entra'],
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Importar del gestor…' }));
    expect(await screen.findByText('Leído: balance de sumas y saldos, 2 cuentas.')).toBeTruthy();
    expect(screen.getByText(/fila 9/)).toBeTruthy();
    expect((screen.getByLabelText('Cuenta de la línea 2') as HTMLInputElement).value).toBe(
      '5720001',
    );
    expect(screen.getByText('Del fichero «sumas.xlsx».')).toBeTruthy();
  });

  it('del libro de facturas sólo añade las marcadas como pendientes', async () => {
    setup(
      draft({
        step: 2,
        lines: [
          ...draft().lines,
          { account: '4300007', name: 'EMPRESA DEMO SL', debit: 121_000, credit: 0 },
        ],
      }),
    );
    api.pickOpeningFile.mockResolvedValue('/tmp/emitidas.xlsx');
    const inv = (number: string, amount: number) => ({
      account: null,
      tax_id: 'B12345674',
      name: 'EMPRESA DEMO SL',
      number,
      issue_date: '2025-12-01',
      due_date: '2025-12-31',
      amount,
    });
    api.aperturaReadPending.mockResolvedValue({
      items: [inv('F-1', 50_000), inv('F-2', 121_000)],
      from_book: true,
      warnings: [],
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Importar listado o libro…' }));
    const second = await screen.findByLabelText(/F-2/);
    fireEvent.click(second);
    fireEvent.click(screen.getByRole('button', { name: 'Añadir las marcadas (1)' }));
    expect((screen.getByLabelText('Factura del pendiente 1') as HTMLInputElement).value).toBe(
      'F-2',
    );
    // La subcuenta del balance con el mismo nombre.
    expect((screen.getByLabelText('Subcuenta del pendiente 1') as HTMLInputElement).value).toBe(
      '4300007',
    );
    expect(screen.queryByLabelText('Factura del pendiente 2')).toBeNull();
    expect(api.aperturaReadPending).toHaveBeenCalledWith('emitida', '/tmp/emitidas.xlsx');
  });

  it('no deja contabilizar con errores y pide confirmar otra apertura', async () => {
    setup(draft({ step: 6 }));
    api.aperturaSummary.mockResolvedValueOnce(
      summary({ errors: ['no cuadra: el debe suma 3.000,00 € y el haber 2.500,00 €'] }),
    );
    expect(await screen.findByText(/no cuadra/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Contabilizar la apertura' })).toBeNull();

    api.aperturaSummary.mockResolvedValueOnce(
      summary({ duplicate: 'ya hay un asiento de apertura en 2026: nº 1 del 01/01/2026' }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Volver a comprobar' }));
    expect(await screen.findByText(/Otro asiento de apertura sumaría/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Contabilizar la apertura' })).toBeNull();
    fireEvent.click(screen.getByLabelText(/quiero hacer otro asiento de apertura/));

    api.aperturaApply.mockResolvedValue({
      id: 2,
      fiscal_year: 2026,
      date: '2026-01-01',
      entry_id: 9,
      entry_number: 3,
      pending: 1,
      assets: 0,
      created_at: '2026-10-26T10:00:00Z',
    });
    api.aperturaDraft.mockResolvedValue(draft({ step: 0, lines: [], date: null }));
    fireEvent.click(screen.getByRole('button', { name: 'Contabilizar la apertura' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, contabilizar' }));
    expect(await screen.findByText(/Apertura contabilizada: asiento nº 3/)).toBeTruthy();
    expect(api.aperturaApply).toHaveBeenCalledWith(expect.objectContaining({ step: 6 }), true);
  });

  it('dice que la 4700 pasará a compensar en el 303', async () => {
    setup(draft({ step: 6 }));
    api.aperturaSummary.mockResolvedValueOnce(
      summary({
        also: [
          'la 4700 (350,00 €) se anotará en Modelos › Ajustes como cuota a compensar al empezar 2026: el 303 del 1T la descontará en la casilla [110]',
        ],
      }),
    );
    expect(await screen.findByText('Además, al contabilizar:')).toBeTruthy();
    expect(screen.getByText(/casilla \[110\]/)).toBeTruthy();
  });
});

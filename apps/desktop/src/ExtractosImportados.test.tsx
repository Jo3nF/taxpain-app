import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  bankStatements: vi.fn(),
  bankStatementUndo: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { ExtractosImportados, undoStatementQuestion } = await import('./ExtractosImportados');

const agosto = {
  id: 1,
  cuenta_id: 1,
  cuenta: 'Banco Demo',
  divisa: 'EUR',
  fichero: 'agosto.xlsx',
  formato: 'Hoja de cálculo',
  desde: '2026-08-01',
  hasta: '2026-08-31',
  saldo_inicial: 100_000,
  saldo_final: 110_000,
  movimientos: 3,
  nuevos: 3,
  propios: 3,
  conciliados: 1,
  importado_en: '2026-09-01T08:00:00Z',
  heredado: false,
};
const octubre = {
  ...agosto,
  id: 2,
  fichero: 'octubre.pdf',
  formato: 'PDF',
  desde: '2026-10-01',
  hasta: '2026-10-31',
  saldo_inicial: 115_000,
  saldo_final: 120_000,
  movimientos: 2,
  nuevos: 1,
  propios: 1,
  conciliados: 0,
};

beforeEach(() => {
  api.bankStatements.mockResolvedValue({
    extractos: [agosto, octubre],
    huecos: [
      {
        cuenta_id: 1,
        cuenta: 'Banco Demo',
        desde: '2026-09-01',
        hasta: '2026-09-30',
        seguro: true,
        detalle: 'Los saldos no enlazan.',
      },
    ],
  });
  api.bankStatementUndo.mockResolvedValue({ borrados: 3, desconciliados: 1, compartidos: 0 });
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('ExtractosImportados', () => {
  it('lista los extractos con el hueco y deshace con confirmación', async () => {
    const onDone = vi.fn();
    render(<ExtractosImportados version={0} onDone={onDone} />);
    expect(api.bankStatements).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Ver los extractos importados…' }));
    expect(await screen.findByText('agosto.xlsx')).toBeTruthy();
    expect(screen.getByText(/del 01\/08\/2026 al 31\/08\/2026/)).toBeTruthy();
    expect(screen.getByText(/Faltan movimientos del 01\/09\/2026 al 30\/09\/2026/)).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Deshacer la importación de agosto.xlsx' }));
    expect(api.bankStatementUndo).not.toHaveBeenCalled();
    expect(screen.getByRole('alertdialog').textContent).toContain('Uno está conciliado');
    fireEvent.click(screen.getByRole('button', { name: 'Sí, desconciliar y deshacer' }));
    await vi.waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(api.bankStatementUndo).toHaveBeenCalledWith(1, true);
    expect(await screen.findByText(/3 movimientos borrados; 1 desconciliado antes/)).toBeTruthy();
  });

  it('explica qué se borra y qué se queda', () => {
    expect(undoStatementQuestion(octubre)).toContain(
      'los que también trae otro extracto se quedan',
    );
    expect(undoStatementQuestion({ ...octubre, propios: 0 })).toContain('sólo sale de la lista');
  });
});

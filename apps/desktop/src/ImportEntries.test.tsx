import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  importPreview: vi.fn(),
  importEntries: vi.fn(),
  importBatches: vi.fn(),
  importUndo: vi.fn(),
  pickImportFile: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { ImportEntries } = await import('./ImportEntries');

const preview = {
  source: 'tabla',
  entries: 2,
  invoices: 0,
  accounts: 3,
  first_date: '2026-01-02',
  last_date: '2026-01-15',
  total: 350050,
  sample: [
    {
      date: '2026-01-02',
      concept: 'Capital',
      origin: 'Manual',
      lines: [
        { account: '5720001', debit: 300000, credit: 0, description: null },
        { account: '100', debit: 0, credit: 300000, description: null },
      ],
    },
  ],
  warnings: [],
  already: null,
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('ImportEntries', () => {
  it('enseña la vista previa, importa y deshace el lote', async () => {
    api.importBatches.mockResolvedValueOnce([]).mockResolvedValue([
      {
        id: 4,
        source: 'tabla',
        filename: 'diario.xlsx',
        entries: 2,
        created_at: '2026-10-20T10:00:00Z',
      },
    ]);
    api.pickImportFile.mockResolvedValue('/tmp/diario.xlsx');
    api.importPreview.mockResolvedValue(preview);
    api.importEntries.mockResolvedValue(preview);
    api.importUndo.mockResolvedValue(2);
    render(<ImportEntries />);
    fireEvent.click(screen.getByRole('button', { name: 'Elegir fichero…' }));
    expect(await screen.findByText(/Tabla de asientos: 2 asientos, 3 cuentas/)).toBeTruthy();
    expect(screen.getByText('Capital')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Importar 2 asientos' }));
    expect(await screen.findByText('2 asientos importados.')).toBeTruthy();
    expect(api.importEntries).toHaveBeenCalledWith('/tmp/diario.xlsx');
    expect(await screen.findByText(/diario\.xlsx · 2 asientos/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Deshacer' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, deshacer' }));
    await vi.waitFor(() => expect(api.importUndo).toHaveBeenCalledWith(4, true));
  });

  it('no deja importar dos veces el mismo fichero', async () => {
    api.importBatches.mockResolvedValue([]);
    api.pickImportFile.mockResolvedValue('/tmp/d.xlsx');
    api.importPreview.mockResolvedValue({
      ...preview,
      already: 'd.xlsx (importado el 2026-10-19)',
    });
    render(<ImportEntries />);
    fireEvent.click(screen.getByRole('button', { name: 'Elegir fichero…' }));
    expect(await screen.findByText(/Este fichero ya se importó/)).toBeTruthy();
    expect(
      (screen.getByRole('button', { name: 'Importar 2 asientos' }) as HTMLButtonElement).disabled,
    ).toBe(true);
  });
});

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { FixedAssetInventoryPreview, OpeningAsset } from './api';

const api = vi.hoisted(() => ({
  pickOpeningFile: vi.fn(),
  fixedAssetInventoryPreview: vi.fn(),
  fixedAssetInventoryImport: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { InventarioGestor } = await import('./InventarioGestor');

const bien = (patch: Partial<OpeningAsset>): OpeningAsset => ({
  name: 'Portátil (2)',
  account: '21700001',
  depreciation_account: '2817',
  expense_account: '681',
  acquired_on: '2023-07-01',
  cost: 120_000,
  rate: 2500,
  method: 'lineal',
  accumulated: 75_000,
  ...patch,
});

const plan: FixedAssetInventoryPreview = {
  items: [
    {
      asset: bien({ name: 'Solar (1)', account: '21000001', rate: 0, cost: 2_000_000 }),
      existing: null,
      skip: 'es un terreno: no se amortiza, así que no va en Inmovilizado',
    },
    { asset: bien({}), existing: null, skip: null },
    {
      asset: bien({ name: 'Silla (4)', account: '21600001', rate: 1000 }),
      existing: 7,
      skip: null,
    },
  ],
  until: '2025-12-31',
  warnings: ['lo ya amortizado es adquisición − valor contable del listado'],
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('InventarioGestor', () => {
  it('da de alta los bienes nuevos con lo ya amortizado, sin ningún asiento', async () => {
    api.pickOpeningFile.mockResolvedValue('/tmp/Inventario 2025.xlsx');
    api.fixedAssetInventoryPreview.mockResolvedValue(plan);
    api.fixedAssetInventoryImport.mockResolvedValue({
      added: 1,
      existing: 1,
      skipped: 0,
      warnings: [],
    });
    const onDone = vi.fn();
    render(<InventarioGestor year={2026} onDone={onDone} />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Dar de alta el inventario del gestor en Inmovilizado…' }),
    );
    expect(await screen.findByText(/No entra: es un terreno/)).toBeTruthy();
    expect(screen.getByText('Ya está en Inmovilizado')).toBeTruthy();
    // Las cuentas de amortización y de gasto con las que entra el nuevo.
    expect(screen.getByText('amortización 2817 · gasto 681')).toBeTruthy();
    // Lo ya amortizado llega hasta el cierre del listado; el coeficiente se puede cambiar.
    expect((screen.getByLabelText('Amortizado hasta el') as HTMLInputElement).value).toBe(
      '2025-12-31',
    );
    fireEvent.change(screen.getByLabelText('Coeficiente anual de Portátil (2)'), {
      target: { value: '20' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Dar de alta 1 bien' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Sí, dar de alta' }));
    await waitFor(() => expect(api.fixedAssetInventoryImport).toHaveBeenCalled());
    // Van todos, para que TaxPain los case igual que en la vista previa; el nuevo, con su
    // coeficiente.
    expect(api.fixedAssetInventoryImport).toHaveBeenCalledWith(
      [plan.items[0]?.asset, bien({ rate: 2000 }), plan.items[2]?.asset],
      '2025-12-31',
    );
    const status = await screen.findByRole('status');
    expect(status.textContent).toMatch(/1 bien dado de alta en Inmovilizado/);
    expect(status.textContent).toMatch(/sin ningún asiento/);
    expect(status.textContent).toMatch(/1 ya estaba/);
    expect(onDone).toHaveBeenCalled();
  });

  it('sin la fecha del listado propone el cierre del año anterior; si falla, lo dice', async () => {
    api.pickOpeningFile.mockResolvedValue('/tmp/Inventario.xlsx');
    api.fixedAssetInventoryPreview.mockResolvedValueOnce({ ...plan, until: null });
    render(<InventarioGestor year={2026} onDone={() => {}} />);
    fireEvent.click(
      screen.getByRole('button', { name: 'Dar de alta el inventario del gestor en Inmovilizado…' }),
    );
    expect(((await screen.findByLabelText('Amortizado hasta el')) as HTMLInputElement).value).toBe(
      '2025-12-31',
    );
    api.fixedAssetInventoryPreview.mockRejectedValueOnce('no lo reconozco como inventario');
    fireEvent.click(
      screen.getByRole('button', { name: 'Dar de alta el inventario del gestor en Inmovilizado…' }),
    );
    expect((await screen.findByRole('status')).textContent).toMatch(/no lo reconozco/);
  });
});

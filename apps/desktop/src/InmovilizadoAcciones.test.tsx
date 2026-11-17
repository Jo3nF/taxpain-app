import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  assetEdit: vi.fn(),
  assetDelete: vi.fn(),
  assetUndispose: vi.fn(),
  assetSetIncentive: vi.fn(),
  assetIncentiveUsage: vi.fn(),
  depreciationPreview: vi.fn(),
  depreciationUndo: vi.fn(),
  depreciationDifferences: vi.fn(),
  postDepreciation: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { AmortizacionesEjercicio, DeshacerBaja, EditarBien, IncentivoFiscal, usefulLife } =
  await import('./InmovilizadoAcciones');

const portatil = {
  id: 7,
  name: 'Portátil demo',
  account: '217',
  depreciation_account: '2817',
  expense_account: '681',
  acquired_on: '2026-07-01',
  cost: 120_000,
  residual: 0,
  rate: 2500,
  disposed_on: null,
  method: 'lineal' as const,
  tax_rate: null,
};

beforeEach(() => {
  api.assetEdit.mockResolvedValue(['La amortización ya registrada (2026) no cambia.']);
  api.assetUndispose.mockResolvedValue({
    disposal_entry: 9,
    restored: [8],
    removed: null,
    warnings: [],
  });
  api.assetSetIncentive.mockResolvedValue(['Sólo para elementos nuevos.']);
  api.assetIncentiveUsage.mockResolvedValue({
    year: 2026,
    escaso_valor: 30_000,
    escaso_valor_tope: 2_500_000,
    erd_empleo: 0,
    erd_empleo_tope: null,
  });
  api.depreciationPreview.mockResolvedValue({
    year: 2026,
    pendientes: [{ asset_id: 7, name: 'Portátil demo', amount: 15_123, entry_id: null }],
    total: 15_123,
    registradas: [{ asset_id: 8, name: 'Mesa demo', amount: 10_000, entry_id: 3 }],
    total_registrado: 10_000,
    cerrado: false,
  });
  api.depreciationDifferences.mockResolvedValue([
    {
      asset_id: 8,
      name: 'Teclado demo',
      accounting: 6_000,
      tax: 30_000,
      difference: -24_000,
      reversal: false,
    },
  ]);
  api.depreciationUndo.mockResolvedValue({ borrados: 1, total: 10_000, saltados: [] });
  api.postDepreciation.mockResolvedValue(1);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('InmovilizadoAcciones', () => {
  it('edita el elemento con confirmación', async () => {
    const onDone = vi.fn();
    const onMsg = vi.fn();
    render(<EditarBien asset={portatil} onDone={onDone} onMsg={onMsg} />);
    fireEvent.change(screen.getByLabelText('Coeficiente % anual'), { target: { value: '20' } });
    expect(screen.getByText('Vida útil: 5 años')).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Valor residual'), { target: { value: '200' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar los cambios' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, guardar' }));
    await vi.waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(api.assetEdit).toHaveBeenCalledWith(7, {
      name: 'Portátil demo',
      acquired_on: '2026-07-01',
      rate: 2000,
      residual: 20_000,
    });
    expect(onMsg.mock.calls[0]?.[0]).toContain('no cambia');
  });

  it('deshace una baja', async () => {
    const onDone = vi.fn();
    render(
      <DeshacerBaja
        asset={{ ...portatil, disposed_on: '2026-10-01' }}
        onDone={onDone}
        onMsg={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Deshacer la baja' }));
    expect(screen.getByRole('alertdialog').textContent).toContain('01/10/2026');
    fireEvent.click(screen.getByRole('button', { name: 'Sí, deshacer' }));
    await vi.waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(api.assetUndispose).toHaveBeenCalledWith(7);
  });

  it('pone el incentivo de escaso valor', async () => {
    const onMsg = vi.fn();
    render(<IncentivoFiscal asset={portatil} incentivo={null} onDone={vi.fn()} onMsg={onMsg} />);
    fireEvent.change(screen.getByLabelText('Incentivo fiscal'), {
      target: { value: 'escaso_valor' },
    });
    expect(screen.queryByLabelText(/Coeficiente fiscal/)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Guardar el incentivo' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, guardar' }));
    await vi.waitFor(() => expect(onMsg).toHaveBeenCalled());
    expect(api.assetSetIncentive).toHaveBeenCalledWith(7, { tipo: 'escaso_valor' }, null);
    expect(onMsg.mock.calls[0]?.[0]).toContain('escaso valor');
  });

  it('registra las amortizaciones con su total y las deshace', async () => {
    const onMsg = vi.fn();
    render(<AmortizacionesEjercicio year={2026} version={0} onDone={vi.fn()} onMsg={onMsg} />);
    const registrar = await screen.findByRole('button', {
      name: 'Registrar amortizaciones de 2026',
    });
    await vi.waitFor(() => expect((registrar as HTMLButtonElement).disabled).toBe(false));
    expect(screen.getByText(/Teclado demo/)).toBeTruthy();
    expect(screen.getByText(/240,00 € disminución/)).toBeTruthy();
    fireEvent.click(registrar);
    expect(screen.getByRole('alertdialog').textContent).toContain('151,23 €');
    fireEvent.click(screen.getByRole('button', { name: 'Sí, registrar' }));
    await vi.waitFor(() => expect(api.postDepreciation).toHaveBeenCalledWith(2026));

    fireEvent.click(screen.getByRole('button', { name: 'Deshacer la amortización de 2026' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, deshacer' }));
    await vi.waitFor(() => expect(api.depreciationUndo).toHaveBeenCalledWith(2026));
  });

  it('vida útil por el coeficiente', () => {
    expect(usefulLife(2500)).toBe(4);
    expect(usefulLife(1200)).toBe(9);
  });
});

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { GestorLot, GestorPreview } from './api';

const api = vi.hoisted(() => ({
  gestorLots: vi.fn(),
  gestorPreview: vi.fn(),
  gestorImport: vi.fn(),
  gestorUndo: vi.fn(),
  gestorCompareBalance: vi.fn(),
  gestorCompareModels: vi.fn(),
  pickOpeningFile: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { EjercicioGestor } = await import('./EjercicioGestor');

const preview = (patch: Partial<GestorPreview> = {}): GestorPreview => ({
  source: 'diario',
  filename: 'Diario 2025.xlsx',
  fiscal_year: 2025,
  fiscal_year_label: '2025',
  entries: 12,
  lines: 37,
  first_date: '2025-01-01',
  last_date: '2025-12-31',
  total: 2_678_000,
  result: -285_000,
  has_opening: true,
  has_regularization: true,
  has_closing: false,
  next_opening: false,
  accounts: 20,
  new_accounts: [
    { code: '43000001', name: 'CLIENTE UNO SL' },
    { code: '57200001', name: 'BANCO DEMO' },
  ],
  sample: [],
  errors: [],
  warnings: ['no viene el asiento de cierre (concepto «Cierre»): el ejercicio queda abierto'],
  ...patch,
});

const lot: GestorLot = {
  id: 1,
  fiscal_year: 2025,
  source: 'diario',
  filename: 'Diario 2025.xlsx',
  entries: 10,
  lines: 37,
  accounts_created: 20,
  has_opening: true,
  has_regularization: true,
  has_closing: false,
  next_opening: false,
  closed_year: false,
  created_at: '2026-10-26T10:00:00Z',
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('EjercicioGestor', () => {
  it('enseña la vista previa e importa al confirmar', async () => {
    api.gestorLots.mockResolvedValue([]);
    api.pickOpeningFile.mockResolvedValue('/tmp/Diario 2025.xlsx');
    api.gestorPreview.mockResolvedValue(preview());
    api.gestorImport.mockResolvedValue({ ...lot, accounts_created: 20 });
    render(<EjercicioGestor year={2026} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Elegir el diario del gestor…' }));
    expect(await screen.findByText(/12 asientos con 37 apuntes; todos cuadran/)).toBeTruthy();
    expect(screen.getByText(/Pérdida de/)).toBeTruthy();
    expect(screen.getByText(/la apertura, la regularización/)).toBeTruthy();
    expect(screen.getByText(/no viene el asiento de cierre/)).toBeTruthy();
    // Las cuentas nuevas se despliegan.
    fireEvent.click(screen.getByRole('button', { name: /2 nuevas que se darán de alta/ }));
    expect(screen.getByText('CLIENTE UNO SL')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Importar el ejercicio 2025' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, importar' }));
    await waitFor(() => expect(api.gestorImport).toHaveBeenCalled());
    expect(api.gestorImport).toHaveBeenCalledWith('/tmp/Diario 2025.xlsx', 2025, {
      next_opening: true,
    });
    const status = await screen.findByRole('status');
    expect(status.textContent).toMatch(/Ejercicio 2025 importado/);
    expect(status.textContent).toMatch(/queda abierto/);
    expect(api.gestorLots).toHaveBeenCalledTimes(2);
  });

  it('si el diario del ejercicio siguiente trae su apertura, la del cierre no se marca', async () => {
    api.gestorLots.mockResolvedValue([]);
    api.pickOpeningFile.mockResolvedValue('/tmp/2024/Diario 2024.xlsx');
    api.gestorPreview.mockResolvedValue(
      preview({
        filename: 'Diario 2024.xlsx',
        fiscal_year: 2024,
        fiscal_year_label: '2024',
        has_closing: true,
        next_opening: true,
        next_ledger: '«2025/Diario 2025.xlsx», junto al fichero elegido',
        warnings: [
          'el diario trae el cierre: el ejercicio 2024 quedará cerrado. La apertura de 2025 no se marca',
        ],
      }),
    );
    api.gestorImport.mockResolvedValue({ ...lot, fiscal_year: 2024, closed_year: true });
    render(<EjercicioGestor year={2024} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Elegir el diario del gestor…' }));
    const box = (await screen.findByRole('checkbox', {
      name: /Crear la apertura de 2025/,
    })) as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect(box.disabled).toBe(false);
    expect(
      screen.getByText(/no se marca: 2025 tiene su propio diario con su apertura/),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Importar el ejercicio 2024' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, importar' }));
    await waitFor(() =>
      expect(api.gestorImport).toHaveBeenCalledWith('/tmp/2024/Diario 2024.xlsx', 2024, {
        next_opening: false,
      }),
    );
  });

  it('con errores no deja importar', async () => {
    api.gestorLots.mockResolvedValue([]);
    api.pickOpeningFile.mockResolvedValue('/tmp/Diario 2025.xlsx');
    api.gestorPreview.mockResolvedValue(
      preview({ errors: ['el ejercicio 2025 ya tiene 3 asiento(s) en TaxPain'] }),
    );
    render(<EjercicioGestor year={2025} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Elegir el diario del gestor…' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/ya tiene 3 asiento/);
    const b = screen.getByRole('button', { name: /No se puede importar/ });
    expect((b as HTMLButtonElement).disabled).toBe(true);
    expect(screen.queryByRole('button', { name: /Importar el ejercicio/ })).toBeNull();
  });

  it('lista el lote, coteja con los modelos y lleva al cierre', async () => {
    api.gestorLots.mockResolvedValue([lot]);
    api.gestorCompareModels.mockResolvedValue({
      fiscal_year: 2025,
      rows: [
        {
          model: '303',
          period: '1T',
          box: 27,
          label: 'IVA devengado (477)',
          ours: 21_000,
          theirs: 21_000,
          diff: 0,
        },
        {
          model: '303',
          period: '1T',
          box: 45,
          label: 'IVA deducible (472)',
          ours: 10_500,
          theirs: 9_000,
          diff: 1_500,
        },
      ],
      mismatches: 1,
      warnings: ['las diferencias del 303 suelen ser IVA no deducible'],
    });
    const onClose = vi.fn();
    render(<EjercicioGestor year={2025} onClose={onClose} />);
    expect(await screen.findByText(/Ejercicio 2025: «Diario 2025.xlsx»/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Cotejar con los modelos presentados' }));
    expect(await screen.findByText(/1 cifra no coinciden/)).toBeTruthy();
    expect(screen.getByText(/\[45\] IVA deducible/)).toBeTruthy();
    expect(screen.getByText(/IVA no deducible/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Cerrar 2025 con TaxPain' }));
    expect(onClose).toHaveBeenCalled();
    // Deshacer con confirmación.
    api.gestorUndo.mockResolvedValue(12);
    fireEvent.click(screen.getByRole('button', { name: 'Deshacer' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, deshacer' }));
    await waitFor(() => expect(api.gestorUndo).toHaveBeenCalledWith(1, true));
    expect((await screen.findByRole('status')).textContent).toMatch(/12 asientos borrados/);
  });
});

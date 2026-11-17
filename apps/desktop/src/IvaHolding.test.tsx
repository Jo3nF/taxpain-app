import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ivaHoldingHandlers } from './demoIvaHolding';

const api = vi.hoisted(() => ({
  ivaHoldingInforme: vi.fn(),
  ivaHoldingLibro: vi.fn(),
  ivaHoldingReglaGuardar: vi.fn(),
  ivaHoldingReglaBorrar: vi.fn(),
  ivaHoldingSectorGuardar: vi.fn(),
  ivaHoldingSectorBorrar: vi.fn(),
  ivaHoldingConfig: vi.fn(),
  ivaHoldingBienGuardar: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { IvaHolding } = await import('./IvaHolding');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function conDemo() {
  api.ivaHoldingInforme.mockImplementation(async (year: number) =>
    ivaHoldingHandlers.iva_holding_informe?.({ year }),
  );
  api.ivaHoldingLibro.mockImplementation(async (year: number) =>
    ivaHoldingHandlers.iva_holding_libro?.({ year }),
  );
  api.ivaHoldingReglaGuardar.mockResolvedValue(3);
  api.ivaHoldingConfig.mockResolvedValue(undefined);
  api.ivaHoldingReglaBorrar.mockResolvedValue(undefined);
}

describe('IvaHolding', () => {
  it('enseña el perfil, las prorratas, los sectores y el libro de bienes', async () => {
    conDemo();
    render(<IvaHolding year={2026} />);
    expect(await screen.findByText(/Parece una holding mixta/)).toBeTruthy();
    expect(screen.getByText('Servicios facturados a participadas')).toBeTruthy();
    expect(screen.getByText(/art\. 93\.Cinco\)/)).toBeTruthy();
    expect(screen.getAllByText('82 %').length).toBeGreaterThan(0);
    // Sectores: la financiación del grupo forma otro sector y se avisa de que no se separa.
    expect(screen.getByText(/Sector diferenciado \(100 puntos\)/)).toBeTruthy();
    expect(screen.getAllByRole('alert')[0]?.textContent).toMatch(/sectores diferenciados/);
    // Libro: el local vendido exento regulariza de una vez.
    expect(screen.getByText(/Regularización única por 7 años/)).toBeTruthy();
    expect(screen.getByText('Inmovilizado sin datos de IVA')).toBeTruthy();
    // Lo aplicado con las reglas queda documentado.
    expect(screen.getByText(/de IVA que no se deduce/)).toBeTruthy();
  });

  it('añade una regla con su criterio y la propuesta del 93.Cinco', async () => {
    conDemo();
    render(<IvaHolding year={2026} />);
    await screen.findByText(/Parece una holding mixta/);
    fireEvent.change(screen.getByLabelText(/Cuenta \(6230/), { target: { value: '629' } });
    fireEvent.change(screen.getByLabelText(/Criterio/), {
      target: { value: 'Proporción de ingresos sujetos de 2025' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Añadir regla' }));
    await waitFor(() => expect(api.ivaHoldingReglaGuardar).toHaveBeenCalledTimes(1));
    const r = api.ivaHoldingReglaGuardar.mock.calls[0]?.[0];
    expect(r).toMatchObject({ ambito: 'cuenta', clave: '629', pct: 82, sector_id: null });
    expect(await screen.findByText('Regla añadida.')).toBeTruthy();
  });

  it('separar por sectores y borrar una regla piden confirmación', async () => {
    conDemo();
    render(<IvaHolding year={2026} />);
    await screen.findByText(/Parece una holding mixta/);
    fireEvent.click(screen.getByRole('button', { name: 'Separar la deducción por sectores' }));
    expect(api.ivaHoldingConfig).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Sí, separar' }));
    await waitFor(() =>
      expect(api.ivaHoldingConfig).toHaveBeenCalledWith({ sectores_diferenciados: true }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Borrar la regla de 623' }));
    expect(api.ivaHoldingReglaBorrar).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Sí, borrarla' }));
    await waitFor(() => expect(api.ivaHoldingReglaBorrar).toHaveBeenCalledWith(1, true));
  });

  it('dice el error si no carga', async () => {
    api.ivaHoldingInforme.mockRejectedValue('sin empresa');
    api.ivaHoldingLibro.mockResolvedValue(null);
    render(<IvaHolding year={2026} />);
    expect((await screen.findByRole('alert')).textContent).toMatch(/No se pudo cargar/);
  });
});

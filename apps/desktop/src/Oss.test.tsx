import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ossHandlers } from './demoOss';

const api = vi.hoisted(() => ({
  ossEstado: vi.fn(),
  ossVentas: vi.fn(),
  ossTipos: vi.fn(),
  ossClasificar: vi.fn(),
  ossConfirmarTipo: vi.fn(),
  ossAjustesGuardar: vi.fn(),
  oss369: vi.fn(),
  oss369Hoja: vi.fn(),
  pickFolder: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Oss } = await import('./Oss');
const { parsePct, pctText } = await import('./OssFactura');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function conDemo() {
  api.ossEstado.mockImplementation(async (year: number) => ossHandlers.oss_estado?.({ year }));
  api.ossVentas.mockImplementation(async (year: number) => ossHandlers.oss_ventas?.({ year }));
  api.ossTipos.mockImplementation(async () => ossHandlers.oss_tipos?.());
  api.oss369.mockImplementation(async (year: number, quarter: number) =>
    ossHandlers.oss_369?.({ year, quarter }),
  );
  api.ossClasificar.mockResolvedValue(undefined);
  api.ossConfirmarTipo.mockResolvedValue(undefined);
  api.ossAjustesGuardar.mockResolvedValue(undefined);
  api.oss369Hoja.mockResolvedValue('/tmp/369_2026_3T.xlsx');
  api.pickFolder.mockResolvedValue('/tmp');
}

describe('Oss', () => {
  it('muestra el umbral superado, las facturas y el 369 por país y tipo', async () => {
    conDemo();
    render(<Oss year={2026} />);
    expect(await screen.findByText(/Tributas en el país del cliente desde el/)).toBeTruthy();
    expect(screen.getByText('A2026-0051')).toBeTruthy();
    // El 3T: Francia al 20 %.
    fireEvent.change(screen.getByLabelText('Trimestre'), { target: { value: '3' } });
    await waitFor(() => expect(api.oss369).toHaveBeenLastCalledWith(2026, 3));
    expect(await screen.findByText('Total a ingresar')).toBeTruthy();
    expect(screen.getAllByText('Francia (FR)').length).toBeGreaterThan(1);
    expect(screen.getByText('4779011')).toBeTruthy();
    fireEvent.click(screen.getByText('Guardar la hoja del 369 (XLSX)…'));
    expect(await screen.findByText(/Hoja del 369 guardada/)).toBeTruthy();
    expect(api.oss369Hoja).toHaveBeenCalledWith(2026, 3, '/tmp');
  });

  it('clasifica una factura y confirma el tipo de un país con confirmación', async () => {
    conDemo();
    render(<Oss year={2026} />);
    const sel = await screen.findByLabelText('Clase de la factura A2026-0040');
    fireEvent.change(sel, { target: { value: 'bienes' } });
    await waitFor(() => expect(api.ossClasificar).toHaveBeenCalledWith(903, 'bienes'));
    fireEvent.click(screen.getByText(/Tipos generales de IVA por país/));
    fireEvent.click(screen.getByRole('button', { name: 'Confirmar el tipo de Italia' }));
    expect(api.ossConfirmarTipo).not.toHaveBeenCalled();
    fireEvent.click(await screen.findByRole('button', { name: 'Sí, confirmar' }));
    await waitFor(() => expect(api.ossConfirmarTipo).toHaveBeenCalledWith('IT', 2200, true));
  });

  it('guarda el alta del 035', async () => {
    conDemo();
    render(<Oss year={2026} />);
    const alta = await screen.findByLabelText(/modelo 035/);
    fireEvent.change(alta, { target: { value: '2026-09-15' } });
    fireEvent.click(screen.getByText('Guardar ajustes'));
    await waitFor(() =>
      expect(api.ossAjustesGuardar).toHaveBeenCalledWith({
        alta_035: '2026-09-15',
        opcion_destino: false,
        otras_ventas: {},
      }),
    );
  });

  it('lee y escribe los tipos', () => {
    expect(parsePct('20')).toBe(2000);
    expect(parsePct('25,5')).toBe(2550);
    expect(parsePct('25,5 %')).toBe(2550);
    expect(parsePct('50')).toBeNull();
    expect(parsePct('x')).toBeNull();
    expect(pctText(2550)).toBe('25,5 %');
    expect(pctText(1900)).toBe('19 %');
  });
});

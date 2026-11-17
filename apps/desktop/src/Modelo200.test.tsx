import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { hoja200 } from './demoModelo200';

const api = vi.hoisted(() => ({
  modelo200Hoja: vi.fn(),
  modelo200AjusteGuardar: vi.fn(),
  modelo200AjusteQuitar: vi.fn(),
  modelo200DatosGuardar: vi.fn(),
  modelo200ArrastreGuardar: vi.fn(),
  modelo200ArrastreBorrar: vi.fn(),
  modelo200PasarSiguiente: vi.fn(),
  participadaDividendoBorrar: vi.fn(),
  participadaDividendoGuardar: vi.fn(),
  participadaFiscalGuardar: vi.fn(),
  participadaFondosGuardar: vi.fn(),
}));
vi.mock('./modelo200Api', async (orig) => ({
  ...(await orig<typeof import('./modelo200Api')>()),
  ...api,
}));
const { Modelo200 } = await import('./Modelo200');

beforeEach(() => {
  api.modelo200Hoja.mockResolvedValue(hoja200(2025));
  for (const f of Object.values(api)) if (f !== api.modelo200Hoja) f.mockResolvedValue(undefined);
  api.modelo200PasarSiguiente.mockResolvedValue(1);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Modelos → 200', () => {
  it('enseña cada bloque con su casilla, su origen y su norma', async () => {
    render(<Modelo200 year={2025} onChange={() => {}} />);
    await screen.findByRole('heading', { name: /Impuesto sobre Sociedades 2025/ });
    for (const t of [
      '1. Resultado contable',
      '2. Correcciones al resultado contable',
      '3. Base imponible previa',
      '4. Bases imponibles negativas por ejercicio de origen',
      '5. Base imponible',
      '6. Reservas de capitalización y de nivelación',
      '7. Cuota, bonificaciones y deducciones',
      '8. Retenciones y pagos fraccionados',
      '9. Resultado',
    ]) {
      expect(screen.getByRole('heading', { name: t })).toBeTruthy();
    }
    const base = screen.getByRole('table', { name: 'Base imponible' });
    const fila = within(base).getByRole('row', { name: /Base imponible después de la reserva/ });
    expect(fila.textContent).toContain('[01330]');
    expect(fila.textContent).toContain('115.600,00 €');
    expect(fila.textContent).toContain('art. 105 LIS');
    const resultado = screen.getByRole('table', { name: 'Resultado de la autoliquidación' });
    expect(
      within(resultado).getByRole('row', { name: /Resultado de la autoliquidación/ }).textContent,
    ).toContain('13.644,00 €');
    // Reserva de capitalización al 20 % desde 2025 (art. 25.1 LIS): 100.000 × 20 % = 20.000.
    expect(screen.getByText('Derecho del ejercicio (20 %)')).toBeTruthy();
    expect(screen.queryByText('Derecho del ejercicio (15 %)')).toBeNull();
    // Participadas con la comprobación del art. 21 y el aviso de la retención indebida.
    const ficha = screen.getByRole('article', { name: 'Participada Demo SL' });
    expect(within(ficha).getByText('Cumple')).toBeTruthy();
    expect(ficha.textContent).toContain('no procedía retención');
    expect(
      screen
        .getAllByRole('status')
        .some((s) => s.textContent?.includes('reserva de capitalización')),
    ).toBe(true);
  });

  it('descarta una propuesta y añade una corrección a mano', async () => {
    render(<Modelo200 year={2025} onChange={() => {}} />);
    fireEvent.click(
      await screen.findByRole('button', { name: 'Descartar: Multas, sanciones y recargos' }),
    );
    expect(api.modelo200AjusteGuardar).toHaveBeenCalledWith(2025, {
      clave: 'multas',
      casilla: 1815,
      importe: null,
      concepto: 'Multas, sanciones y recargos',
    });
    const nueva = screen.getByRole('group', { name: 'Añadir una corrección a mano' });
    fireEvent.change(within(nueva).getByLabelText('Casilla'), { target: { value: '413' } });
    fireEvent.change(within(nueva).getByLabelText('Importe'), { target: { value: '1.000' } });
    fireEvent.click(within(nueva).getByRole('button', { name: 'Añadir' }));
    await vi.waitFor(() =>
      expect(api.modelo200AjusteGuardar).toHaveBeenCalledWith(2025, {
        clave: '',
        casilla: 413,
        importe: 100_000,
        concepto: '',
      }),
    );
  });

  it('pasa lo pendiente al ejercicio siguiente con confirmación', async () => {
    render(<Modelo200 year={2025} onChange={() => {}} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Pasar a 2026' }));
    expect(api.modelo200PasarSiguiente).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Pasar' }));
    await screen.findByText('1 pendientes guardados en 2026.');
    expect(api.modelo200PasarSiguiente).toHaveBeenCalledWith(2025);
  });

  it('guarda una base negativa de un ejercicio anterior', async () => {
    render(<Modelo200 year={2025} onChange={() => {}} />);
    const g = await screen.findByRole('group', { name: /Añadir un pendiente de otro ejercicio/ });
    fireEvent.change(within(g).getByLabelText('Ejercicio de origen'), {
      target: { value: '2021' },
    });
    fireEvent.change(within(g).getByLabelText('Pendiente'), { target: { value: '5.000' } });
    fireEvent.click(within(g).getByRole('button', { name: 'Guardar' }));
    await vi.waitFor(() =>
      expect(api.modelo200ArrastreGuardar).toHaveBeenCalledWith({
        tipo: 'bin',
        origen: 2021,
        ejercicio: 2025,
        pendiente: 500_000,
        nota: '',
      }),
    );
  });

  it('muestra el error si no es una sociedad', async () => {
    api.modelo200Hoja.mockRejectedValue('el modelo 200 es para sociedades');
    render(<Modelo200 year={2025} onChange={() => {}} />);
    expect((await screen.findByRole('alert')).textContent).toContain('sociedades');
  });
});

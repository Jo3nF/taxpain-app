import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Simulacion202 } from './api';

const api = vi.hoisted(() => ({
  model202Simulation: vi.fn(),
  model202SetModality: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Pago202 } = await import('./Pago202');

function sim(over: Partial<Simulacion202['modalidad']> = {}): Simulacion202 {
  return {
    ejercicio: 2027,
    modalidad: {
      ejercicio: 2027,
      elegida: null,
      efectiva: 'Cuota',
      obligatoria: false,
      cifra_negocios: 200_000_000,
      aviso: null,
      ...over,
    },
    periodos: [
      {
        period: 1,
        hasta: '2027-03-31',
        cuota: 540_000,
        base_imponible: 1_000_000,
        base: 170_000,
        fuente: 'ano_anterior',
      },
      {
        period: 2,
        hasta: '2027-09-30',
        cuota: 540_000,
        base_imponible: 3_000_000,
        base: 340_000,
        fuente: 'presupuesto',
      },
      {
        period: 3,
        hasta: '2027-11-30',
        cuota: null,
        base_imponible: 4_000_000,
        base: 170_000,
        fuente: 'sin_datos',
      },
    ],
    total_cuota: null,
    total_base: 680_000,
    opcion_desde: '2027-02-01',
    opcion_hasta: '2027-02-28',
    conclusion: 'Falta la cuota del Impuesto sobre Sociedades para comparar.',
    notas: ['Es una previsión.'],
  };
}

beforeEach(() => {
  api.model202Simulation.mockResolvedValue(sim());
  api.model202SetModality.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Pago202', () => {
  it('compara las dos modalidades pago a pago con su fuente y el plazo del 036', async () => {
    render(<Pago202 year={2027} onChange={() => {}} />);
    const table = await screen.findByRole('table');
    const rows = within(table).getAllByRole('row');
    const row = (i: number) => rows[i] as HTMLElement;
    // Cabecera, tres pagos y total.
    expect(rows).toHaveLength(5);
    expect(within(row(1)).getByText('Mismos meses del año anterior')).toBeTruthy();
    expect(within(row(2)).getByText('Presupuesto')).toBeTruthy();
    expect(within(row(3)).getByText('—')).toBeTruthy();
    expect(within(row(4)).getByText(/6\.800,00/)).toBeTruthy();
    expect(screen.getByText(/Falta la cuota/)).toBeTruthy();
    expect(screen.getByText(/Plazo de la opción/).textContent).toContain('01/02/2027');
    expect(screen.getByText('Es una previsión.')).toBeTruthy();
  });

  it('guarda la modalidad de la base tras confirmar y recuerda el 036', async () => {
    const onChange = vi.fn();
    render(<Pago202 year={2027} onChange={onChange} />);
    fireEvent.click(await screen.findByLabelText(/Por la base imponible del periodo/));
    fireEvent.click(screen.getByRole('button', { name: 'Guardar la modalidad' }));
    expect(screen.getByText(/modelo 036/)).toBeTruthy();
    expect(api.model202SetModality).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    await screen.findByText('Modalidad guardada.');
    expect(api.model202SetModality).toHaveBeenCalledWith(2027, 'Base');
    expect(onChange).toHaveBeenCalled();
  });

  it('con más de 6 millones la base es obligatoria y no se puede cambiar', async () => {
    api.model202Simulation.mockResolvedValue(
      sim({
        efectiva: 'Base',
        obligatoria: true,
        aviso: 'El 202 de 2027 va obligatoriamente por la base del periodo (art. 40.3 LIS)',
      }),
    );
    render(<Pago202 year={2027} onChange={() => {}} />);
    expect(await screen.findByText(/obligatoriamente/)).toBeTruthy();
    const base = screen.getByLabelText(/Por la base imponible del periodo/) as HTMLInputElement;
    expect(base.checked).toBe(true);
    expect(base.disabled).toBe(true);
    expect(screen.queryByRole('button', { name: 'Guardar la modalidad' })).toBeNull();
  });
});

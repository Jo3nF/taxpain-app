import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  nominaFicha: vi.fn(),
  nominaFichaSave: vi.fn(),
  nominaCalcular: vi.fn(),
  nominaRegistrar: vi.fn(),
  segurosSociales: vi.fn(),
  segurosSocialesPagar: vi.fn(),
  finiquitoCalcular: vi.fn(),
  openSede: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Nominas, pct } = await import('./Nominas');
const { nominasHandlers, trabajadorDemo } = await import('./demoNominas');
const h = (cmd: string, a?: Record<string, unknown>) => nominasHandlers[cmd]?.(a);

beforeEach(() => {
  api.nominaFicha.mockResolvedValue(h('nomina_ficha', { employeeId: 2 }));
  api.nominaCalcular.mockResolvedValue(h('nomina_calcular'));
  api.nominaRegistrar.mockResolvedValue(50);
  api.segurosSociales.mockImplementation(async (year: number, month: number) =>
    h('seguros_sociales', { year, month }),
  );
  api.segurosSocialesPagar.mockResolvedValue(51);
  api.finiquitoCalcular.mockResolvedValue(h('finiquito_calcular'));
  api.openSede.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Nominas', () => {
  it('formatea los tipos', () => {
    expect(pct(470)).toBe('4,7 %');
    expect(pct(2360)).toBe('23,6 %');
  });

  it('calcula la nómina con la ficha y la registra tras confirmar', async () => {
    const onRegistered = vi.fn();
    render(<Nominas year={2026} staff={[trabajadorDemo]} onRegistered={onRegistered} />);
    const sec = screen.getByRole('region', { name: 'Calcular la nómina' });
    expect((await within(sec).findAllByDisplayValue('1.800,00')).length).toBe(2);
    fireEvent.change(within(sec).getByLabelText('Mes'), { target: { value: '10' } });
    fireEvent.click(within(sec).getByRole('button', { name: 'Calcular nómina' }));
    expect(await within(sec).findByText(/Líquido a percibir: 1\.610,50 €/)).toBeTruthy();
    const datos = api.nominaCalcular.mock.calls[0]?.[0];
    expect(datos).toMatchObject({
      year: 2026,
      month: 10,
      grupo: 5,
      salario_base: 180_000,
      retencion: { modo: 'Manual', tipo: 1200 },
      pagas: { numero: 2, en_este_mes: 0 },
    });
    // Sin confirmar no se registra.
    fireEvent.click(within(sec).getByRole('button', { name: 'Registrar nómina' }));
    expect(api.nominaRegistrar).not.toHaveBeenCalled();
    fireEvent.click(await within(sec).findByRole('button', { name: 'Registrar' }));
    await within(sec).findByText('Nómina registrada con su asiento');
    expect(api.nominaRegistrar).toHaveBeenCalledWith({
      employee_id: 2,
      fecha: '2026-10-31',
      datos,
    });
    expect(onRegistered).toHaveBeenCalled();
  });

  it('abre el programa de retenciones de la AEAT', async () => {
    render(<Nominas year={2026} staff={[trabajadorDemo]} onRegistered={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: 'Programa de cálculo de retenciones' }));
    expect(api.openSede).toHaveBeenCalledWith(expect.stringContaining('calculo-retenciones'));
  });

  it('resume los seguros sociales del mes y registra el pago', async () => {
    render(<Nominas year={2026} staff={[trabajadorDemo]} onRegistered={() => {}} />);
    const sec = screen.getByRole('region', { name: 'Seguros sociales del mes' });
    fireEvent.change(within(sec).getByLabelText('Mes de devengo'), { target: { value: '10' } });
    expect((await within(sec).findAllByText('877,45 €')).length).toBeGreaterThan(0);
    expect(within(sec).getByText(/hasta el 30\/11\/2026/)).toBeTruthy();
    fireEvent.click(within(sec).getByRole('button', { name: 'Registrar el pago' }));
    fireEvent.click(await within(sec).findByRole('button', { name: 'Registrar' }));
    await vi.waitFor(() =>
      expect(api.segurosSocialesPagar).toHaveBeenCalledWith({
        year: 2026,
        month: 10,
        fecha: '2026-11-30',
        importe: 87_745,
        cuenta: '572',
      }),
    );
  });

  it('calcula el finiquito y sin trabajadores lo dice', async () => {
    render(<Nominas year={2026} staff={[]} onRegistered={() => {}} />);
    expect(screen.getByText(/Da de alta un empleado que no sea administrador/)).toBeTruthy();
    const sec = screen.getByRole('region', { name: 'Finiquito' });
    fireEvent.change(within(sec).getByLabelText('Fecha de alta'), {
      target: { value: '2026-01-01' },
    });
    fireEvent.change(within(sec).getByLabelText('Salario mensual'), {
      target: { value: '1.500' },
    });
    fireEvent.click(within(sec).getByRole('button', { name: 'Calcular finiquito' }));
    expect(await within(sec).findByText('1.994,00 €')).toBeTruthy();
    expect(api.finiquitoCalcular.mock.calls[0]?.[0]).toMatchObject({
      alta: '2026-01-01',
      salario_mensual: 150_000,
      pagas: [],
    });
  });
});

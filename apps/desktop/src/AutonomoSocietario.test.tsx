import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Employee, ResumenReta } from './api';
import { autonomoHandlers } from './demoAutonomo';

const api = vi.hoisted(() => ({
  retaResumen: vi.fn(),
  retaFichaSave: vi.fn(),
  retaCuotaRegister: vi.fn(),
  retaCuotaDelete: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { AutonomoSocietario } = await import('./AutonomoSocietario');

const demo = (cmd: string, args?: Record<string, unknown>) => {
  const h = autonomoHandlers[cmd];
  if (!h) throw new Error(`sin demo: ${cmd}`);
  return h(args);
};
const carmen = (demo('employees') as Employee[])[0] as Employee;
const resumen = () => demo('reta_resumen', { year: 2026 }) as ResumenReta;

beforeEach(() => {
  api.retaResumen.mockResolvedValue(resumen());
  api.retaFichaSave.mockResolvedValue(undefined);
  api.retaCuotaRegister.mockResolvedValue(11);
  api.retaCuotaDelete.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('AutonomoSocietario', () => {
  it('sin administrador en el RETA explica cómo activarlo', () => {
    render(<AutonomoSocietario year={2026} staff={[{ ...carmen, ss_regime: 'Asimilado' }]} />);
    expect(screen.getByText(/marca «Autónomos\s+\(RETA\)» en su ficha/)).toBeTruthy();
    expect(api.retaResumen).not.toHaveBeenCalled();
  });

  it('muestra el tramo y la regularización, sin aviso con la tabla del año publicada', async () => {
    render(<AutonomoSocietario year={2026} staff={[carmen]} />);
    await screen.findByText(/Ficha del RETA de Española Española Carmen en 2026/);
    expect(api.retaResumen).toHaveBeenCalledWith(1, 2026);
    expect(screen.queryByText(/sin norma publicada todavía/)).toBeNull();
    expect(screen.getAllByText('General 7').length).toBeGreaterThan(0);
    expect(screen.getByText(/1\.604,28 € a ingresar a la TGSS/)).toBeTruthy();
    expect(screen.getByText(/Base mínima de los autónomos societarios: 1\.424,40 €/)).toBeTruthy();
    expect(
      screen.getByText(/Deducción por gastos genéricos \(3 %; 365 días de alta como societario\)/),
    ).toBeTruthy();
    expect(screen.getByLabelText(/Fecha de alta como societario/)).toBeTruthy();
    expect(screen.getByText(/Ingreso a cuenta de la especie: 598,50 € \(19 %\)/)).toBeTruthy();
  });

  it('avisa de las cifras de un año sin orden de cotización', async () => {
    api.retaResumen.mockResolvedValue(demo('reta_resumen', { year: 2027 }) as ResumenReta);
    render(<AutonomoSocietario year={2027} staff={[carmen]} />);
    expect(await screen.findByText(/sin norma publicada todavía/)).toBeTruthy();
    expect(screen.getByText(/sin orden de cotización publicada/)).toBeTruthy();
  });

  it('guarda la ficha con los importes en céntimos', async () => {
    render(<AutonomoSocietario year={2026} staff={[carmen]} />);
    const base = (await screen.findByLabelText('Base de cotización mensual')) as HTMLInputElement;
    expect(base.value).toBe('1000,00');
    fireEvent.change(base, { target: { value: '1.356,21' } });
    fireEvent.change(screen.getByLabelText('Parte de los dividendos que computa (%)'), {
      target: { value: '50' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar la ficha de 2026' }));
    await vi.waitFor(() => expect(api.retaFichaSave).toHaveBeenCalled());
    expect(api.retaFichaSave.mock.calls[0]?.[0]).toMatchObject({
      base_elegida: 135_621,
      pct_dividendos: 5_000,
      rendimiento_previsto: 120_000,
      acuerdo_importe: 3_000_000,
    });
  });

  it('registrar una cuota pide confirmación y lleva el asiento', async () => {
    render(<AutonomoSocietario year={2026} staff={[carmen]} />);
    fireEvent.change(await screen.findByLabelText('Fecha del cargo'), {
      target: { value: '2026-11-30' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Registrar la cuota' }));
    expect(api.retaCuotaRegister).not.toHaveBeenCalled();
    expect(screen.getByText(/¿Registrar la cuota de 315,00 € del 30\/11\/2026 y su asiento\?/));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, registrar' }));
    await vi.waitFor(() =>
      expect(api.retaCuotaRegister).toHaveBeenCalledWith({
        employee_id: 1,
        date: '2026-11-30',
        amount: 31_500,
        pagador: 'Sociedad',
        asiento: true,
      }),
    );
  });
});

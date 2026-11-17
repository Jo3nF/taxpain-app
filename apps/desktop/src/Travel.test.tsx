import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  travelList: vi.fn(),
  travelPreview: vi.fn(),
  travelRegister: vi.fn(),
  travelDelete: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Travel, kmRateCents } = await import('./Travel');

const luis = {
  id: 7,
  nif: '00000000T',
  name: 'Administrador Luis',
  province: '46',
  birth_year: 0,
  family: 3,
  spouse_nif: null,
  disability: 0,
  contract: 1,
  mobility: false,
  children_under3: 0,
  children_rest: 0,
  children_whole: false,
  active: true,
  administrator: true,
  ss_regime: 'Autonomo' as const,
};

beforeEach(() => {
  api.travelList.mockResolvedValue([]);
  api.travelRegister.mockResolvedValue(1);
  api.travelPreview.mockResolvedValue({
    total: 14_300,
    exempt: 0,
    not_income: 2_200,
    taxable: 12_100,
    km_limit: 9_100,
    meals_limit: 2_667,
    posted: 2_200,
    warnings: ['Sin relación laboral no se aplican las dietas exentas'],
  });
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Travel', () => {
  it('tarifa por km según la fecha', () => {
    expect(kmRateCents('2023-07-16')).toBe(19);
    expect(kmRateCents('2026-02-12')).toBe(26);
  });

  it('no registra dos veces el viaje con un doble clic', async () => {
    let finish: (v: number) => void = () => {};
    api.travelRegister.mockReturnValue(new Promise<number>((r) => (finish = r)));
    render(<Travel year={2026} staff={[luis]} />);
    fireEvent.change(screen.getByLabelText('Quién viaja'), { target: { value: '7' } });
    fireEvent.change(screen.getByLabelText('Fecha del viaje'), { target: { value: '2026-02-12' } });
    fireEvent.change(screen.getByLabelText('Destino'), { target: { value: 'Barcelona' } });
    fireEvent.change(screen.getByLabelText('Motivo'), { target: { value: 'Taller' } });
    fireEvent.change(screen.getByLabelText('Peajes y aparcamiento'), { target: { value: '22' } });
    fireEvent.click(screen.getByRole('button', { name: 'Calcular' }));
    const register = screen.getByRole('button', { name: 'Registrar viaje' }) as HTMLButtonElement;
    await vi.waitFor(() => expect(register.disabled).toBe(false));
    fireEvent.click(register);
    fireEvent.click(register);
    expect(api.travelRegister).toHaveBeenCalledTimes(1);
    expect(register.disabled).toBe(true);
    finish(1);
    expect(await screen.findByText('Viaje registrado.')).toBeTruthy();
  });

  it('un administrador sin contrato laboral: calcula, avisa y registra', async () => {
    render(<Travel year={2026} staff={[luis]} />);
    fireEvent.change(screen.getByLabelText('Quién viaja'), { target: { value: '7' } });
    expect((screen.getByLabelText('Con contrato laboral') as HTMLInputElement).checked).toBe(false);
    expect((screen.getByLabelText('Contrapartida') as HTMLInputElement).value).toBe('551');
    fireEvent.change(screen.getByLabelText('Fecha del viaje'), { target: { value: '2026-02-12' } });
    fireEvent.change(screen.getByLabelText('Destino'), { target: { value: 'Barcelona' } });
    fireEvent.change(screen.getByLabelText('Motivo'), { target: { value: 'Taller' } });
    fireEvent.change(screen.getByLabelText('Kilómetros'), { target: { value: '350' } });
    fireEvent.click(screen.getByRole('button', { name: 'A 0,26 €/km' }));
    expect((screen.getByLabelText('Kilometraje pagado') as HTMLInputElement).value).toBe('91,00');
    fireEvent.change(screen.getByLabelText('Peajes y aparcamiento'), { target: { value: '22' } });
    const register = screen.getByRole('button', { name: 'Registrar viaje' }) as HTMLButtonElement;
    expect(register.disabled).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Calcular' }));
    await screen.findByText(/Sin relación laboral/);
    expect(api.travelPreview).toHaveBeenCalledWith(
      expect.objectContaining({
        employee_id: 7,
        nif: '00000000T',
        labor: false,
        km: 350,
        km_amount: 9_100,
        tolls_parking: 2_200,
        counter_account: '551',
      }),
    );
    fireEvent.click(register);
    await vi.waitFor(() => expect(api.travelRegister).toHaveBeenCalled());
    await screen.findByText('Viaje registrado.');
  });
});

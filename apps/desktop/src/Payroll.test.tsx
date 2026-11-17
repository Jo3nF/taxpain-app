import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  employees: vi.fn(),
  employeeSave: vi.fn(),
  payslips: vi.fn(),
  payslipRegister: vi.fn(),
  payslipDelete: vi.fn(),
  directorTurnover: vi.fn(),
  travelList: vi.fn(),
  retaResumen: vi.fn(),
  adminReceiptPdf: vi.fn(),
  pickFolder: vi.fn(),
  nominasCalculadas: vi.fn(),
  nominaFicha: vi.fn(),
  segurosSociales: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Payroll } = await import('./Payroll');
const { autonomoHandlers } = await import('./demoAutonomo');
const { nominasHandlers } = await import('./demoNominas');

const ana = {
  id: 1,
  nif: '12345678Z',
  name: 'Española Española Ana',
  province: '46',
  birth_year: 1990,
  family: 3,
  spouse_nif: null,
  disability: 0,
  contract: 1,
  mobility: false,
  children_under3: 0,
  children_rest: 0,
  children_whole: false,
  active: true,
};

beforeEach(() => {
  api.employees.mockResolvedValue([ana]);
  api.payslips.mockResolvedValue([]);
  api.payslipRegister.mockResolvedValue(1);
  api.travelList.mockResolvedValue([]);
  api.nominasCalculadas.mockResolvedValue([]);
  api.nominaFicha.mockResolvedValue(null);
  api.segurosSociales.mockResolvedValue(
    nominasHandlers.seguros_sociales?.({ year: 2026, month: 1 }),
  );
  api.retaResumen.mockResolvedValue(autonomoHandlers.reta_resumen?.({ year: 2026 }));
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Payroll', () => {
  it('no registra dos veces la nómina con un doble clic', async () => {
    let finish: (v: number) => void = () => {};
    api.payslipRegister.mockReturnValue(new Promise<number>((r) => (finish = r)));
    render(<Payroll year={2026} />);
    await screen.findByText(/Española Española Ana · 12345678Z/);
    fireEvent.change(screen.getByLabelText('Fecha'), { target: { value: '2026-10-31' } });
    fireEvent.change(screen.getByLabelText('Bruto'), { target: { value: '2.000,00' } });
    const submit = screen.getByRole('button', { name: 'Registrar nómina' }) as HTMLButtonElement;
    fireEvent.click(submit);
    fireEvent.click(submit);
    expect(api.payslipRegister).toHaveBeenCalledTimes(1);
    expect(submit.disabled).toBe(true);
    finish(1);
    await vi.waitFor(() => expect(submit.disabled).toBe(false));
    expect((screen.getByLabelText('Bruto') as HTMLInputElement).value).toBe('');
  });

  it('registra una nómina con el líquido calculado', async () => {
    render(<Payroll year={2026} />);
    await screen.findByText(/Española Española Ana · 12345678Z/);
    fireEvent.change(screen.getByLabelText('Fecha'), { target: { value: '2026-10-31' } });
    fireEvent.change(screen.getByLabelText('Bruto'), { target: { value: '2.000,00' } });
    fireEvent.change(screen.getByLabelText('Retención IRPF'), { target: { value: '240' } });
    fireEvent.change(screen.getByLabelText('SS trabajador'), { target: { value: '130' } });
    fireEvent.change(screen.getByLabelText('SS empresa'), { target: { value: '640' } });
    expect(screen.getByText(/Líquido: 1\.630,00/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Registrar nómina' }));
    await vi.waitFor(() =>
      expect(api.payslipRegister).toHaveBeenCalledWith({
        employee_id: 1,
        date: '2026-10-31',
        gross: 200000,
        irpf: 24000,
        ss_employee: 13000,
        ss_company: 64000,
        small_entity: null,
      }),
    );
  });

  it('propone la retención de un administrador y su subclave E', async () => {
    api.employees.mockResolvedValue([
      {
        ...ana,
        name: 'Administradora Ana',
        administrator: true,
        ss_regime: 'Autonomo',
      },
    ]);
    api.directorTurnover.mockResolvedValue({
      year: 2025,
      turnover: 8_000_000,
      small: true,
      source: 'aeat',
    });
    render(<Payroll year={2026} />);
    await screen.findByText(/Administradora Ana · 12345678Z · administrador/);
    fireEvent.change(screen.getByLabelText('Fecha'), { target: { value: '2026-03-31' } });
    fireEvent.change(screen.getByLabelText('Bruto'), { target: { value: '3.000' } });
    await screen.findByText(/retención del 19 % y subclave E\.03/);
    expect(screen.getByText(/según el 200 presentado en la AEAT/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Ponerla' }));
    expect((screen.getByLabelText('Retención IRPF') as HTMLInputElement).value).toBe('570,00');
    fireEvent.click(screen.getByRole('button', { name: 'Registrar nómina' }));
    await vi.waitFor(() =>
      expect(api.payslipRegister).toHaveBeenCalledWith(
        expect.objectContaining({ gross: 300000, irpf: 57000, small_entity: null }),
      ),
    );
  });

  it('sin la contabilidad del año anterior pregunta la cifra de negocios', async () => {
    api.employees.mockResolvedValue([{ ...ana, administrator: true, ss_regime: 'Asimilado' }]);
    api.directorTurnover.mockResolvedValue({
      year: 2025,
      turnover: null,
      small: null,
      source: 'contabilidad',
    });
    render(<Payroll year={2026} />);
    await screen.findByText(/· administrador/);
    fireEvent.change(screen.getByLabelText('Fecha'), { target: { value: '2026-03-31' } });
    fireEvent.change(screen.getByLabelText('Bruto'), { target: { value: '1.000' } });
    const box = await screen.findByLabelText(/inferior a 100\.000 €/);
    fireEvent.click(box);
    fireEvent.change(screen.getByLabelText('Retención IRPF'), { target: { value: '190' } });
    fireEvent.click(screen.getByRole('button', { name: 'Registrar nómina' }));
    await vi.waitFor(() =>
      expect(api.payslipRegister).toHaveBeenCalledWith(
        expect.objectContaining({ irpf: 19000, small_entity: true }),
      ),
    );
  });

  it('no registra la nómina de un administrador mientras se consulta la cifra de negocios', async () => {
    api.employees.mockResolvedValue([{ ...ana, administrator: true, ss_regime: 'Autonomo' }]);
    let answer: (t: unknown) => void = () => {};
    api.directorTurnover.mockReturnValue(
      new Promise((resolve) => {
        answer = resolve;
      }),
    );
    render(<Payroll year={2026} />);
    await screen.findByText(/· administrador/);
    fireEvent.change(screen.getByLabelText('Fecha'), { target: { value: '2026-01-31' } });
    fireEvent.change(screen.getByLabelText('Bruto'), { target: { value: '1.000' } });
    fireEvent.change(screen.getByLabelText('Retención IRPF'), { target: { value: '190' } });
    const submit = screen.getByRole('button', { name: 'Registrar nómina' }) as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    expect(screen.getByText(/Consultando la cifra de negocios/)).toBeTruthy();
    fireEvent.submit(submit.closest('form') as HTMLFormElement);
    expect(api.payslipRegister).not.toHaveBeenCalled();
    // Llega una cifra que no cubre todo el ejercicio: la decide el usuario.
    answer({ year: 2025, turnover: 4_000_000, small: null, source: 'contabilidad' });
    expect(await screen.findByText(/constan 40\.000,00 € de 2025/)).toBeTruthy();
    expect(submit.disabled).toBe(false);
    fireEvent.click(submit);
    await vi.waitFor(() =>
      expect(api.payslipRegister).toHaveBeenCalledWith(
        expect.objectContaining({ irpf: 19000, small_entity: false }),
      ),
    );
  });

  it('genera el recibo de retribución del administrador en PDF', async () => {
    api.employees.mockResolvedValue(autonomoHandlers.employees?.());
    api.payslips.mockResolvedValue(autonomoHandlers.payslips?.({ year: 2026 }));
    api.pickFolder.mockResolvedValue('/tmp/recibos');
    api.adminReceiptPdf.mockResolvedValue('/tmp/recibos/Recibo.pdf');
    render(<Payroll year={2026} />);
    const button = await screen.findByRole('button', {
      name: 'Recibo de retribución de Española Española Carmen del 28/03/2026 (PDF)',
    });
    fireEvent.click(button);
    await screen.findByText('Recibo guardado en /tmp/recibos/Recibo.pdf');
    expect(api.adminReceiptPdf).toHaveBeenCalledWith(3, expect.any(String), '/tmp/recibos');
    // La sección del autónomo societario sale con su ficha.
    expect(await screen.findByText(/Ficha del RETA de Española Española Carmen/)).toBeTruthy();
  });
});

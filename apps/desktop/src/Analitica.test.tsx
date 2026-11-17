import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  costCenters: vi.fn(),
  costCenterSave: vi.fn(),
  costCenterDelete: vi.fn(),
  costRules: vi.fn(),
  costRuleSet: vi.fn(),
  analyticLines: vi.fn(),
  analyticLineSet: vi.fn(),
  analyticReport: vi.fn(),
  budgetList: vi.fn(),
  budgetSet: vi.fn(),
  budgetFromActuals: vi.fn(),
  budgetReport: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Analitica, parseShares } = await import('./Analitica');

beforeEach(() => {
  api.costCenters.mockResolvedValue([
    { code: 'TIENDA', name: 'Tienda' },
    { code: 'WEB', name: 'Online' },
  ]);
  api.costRules.mockResolvedValue([]);
  api.analyticLines.mockResolvedValue([
    {
      line_id: 9,
      date: '2026-02-15',
      concept: 'Hosting',
      account: '629',
      amount: 30000,
      shares: [],
      source: 'sin',
    },
  ]);
  api.analyticReport.mockResolvedValue([
    { center: 'TIENDA', name: 'Tienda', income: 240000, expenses: 90000, result: 150000 },
    { center: '', name: 'Sin repartir', income: 0, expenses: 30000, result: -30000 },
  ]);
  api.budgetList.mockResolvedValue([]);
  api.budgetReport.mockResolvedValue([
    {
      account: '629',
      name: 'Otros servicios',
      center: 'WEB',
      budget: 20000,
      actual: 30000,
      deviation: 10000,
      deviation_bp: 5000,
      unfavourable: true,
    },
  ]);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Analitica', () => {
  it('lee los repartos «TIENDA 75, WEB 25»', () => {
    expect(parseShares('TIENDA 75, web 25')).toEqual([
      { center: 'TIENDA', pct_bp: 7500 },
      { center: 'WEB', pct_bp: 2500 },
    ]);
    expect(parseShares('A 33,33; B 66,67')).toEqual([
      { center: 'A', pct_bp: 3333 },
      { center: 'B', pct_bp: 6667 },
    ]);
    expect(parseShares('TIENDA')).toBeNull();
  });

  it('anuncia los errores como alerta y los éxitos como estado', async () => {
    api.costRuleSet.mockRejectedValueOnce(new Error('cuenta 99 no existe'));
    api.budgetSet.mockResolvedValue(undefined);
    render(<Analitica year={2026} />);
    await screen.findAllByText('TIENDA · Tienda');
    fireEvent.change(
      screen.getByLabelText(/Cuenta o grupo/, {
        selector: 'section[aria-label="Reparto por cuenta"] input',
      }),
      { target: { value: '99' } },
    );
    fireEvent.change(screen.getByLabelText(/^Reparto/, { selector: 'input' }), {
      target: { value: 'TIENDA 100' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar reparto' }));
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('cuenta 99 no existe');
    expect(screen.queryByRole('status')).toBeNull();

    // Un éxito del presupuesto va en su propio «status», no como alerta.
    fireEvent.change(screen.getByLabelText(/Importe anual/), { target: { value: '1200' } });
    fireEvent.change(
      screen.getByLabelText(/Cuenta o grupo/, {
        selector: 'section[aria-label="Presupuesto"] input',
      }),
      { target: { value: '705' } },
    );
    fireEvent.click(screen.getByRole('button', { name: 'Guardar línea' }));
    const status = await screen.findByRole('status');
    expect(status.textContent).toContain('Presupuesto de 705 guardado');
  });

  it('guarda una regla, asigna una línea y enseña resultados y desviaciones', async () => {
    api.costRuleSet.mockResolvedValue(undefined);
    api.analyticLineSet.mockResolvedValue(undefined);
    api.budgetSet.mockResolvedValue(undefined);
    render(<Analitica year={2026} />);
    expect((await screen.findAllByText('TIENDA · Tienda')).length).toBeGreaterThan(0);
    expect(screen.getByText('Sin repartir')).toBeTruthy();
    expect(await screen.findByText('50 %')).toBeTruthy();

    fireEvent.change(
      screen.getByLabelText(/Cuenta o grupo/, {
        selector: 'section[aria-label="Reparto por cuenta"] input',
      }),
      {
        target: { value: '62' },
      },
    );
    fireEvent.change(screen.getByLabelText(/^Reparto/, { selector: 'input' }), {
      target: { value: 'TIENDA 75, WEB 25' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar reparto' }));
    await vi.waitFor(() =>
      expect(api.costRuleSet).toHaveBeenCalledWith('62', [
        { center: 'TIENDA', pct_bp: 7500 },
        { center: 'WEB', pct_bp: 2500 },
      ]),
    );

    fireEvent.click(await screen.findByRole('button', { name: 'Centro de Hosting' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'WEB · Online' }));
    await vi.waitFor(() =>
      expect(api.analyticLineSet).toHaveBeenCalledWith(9, [{ center: 'WEB', pct_bp: 10000 }]),
    );

    fireEvent.change(screen.getByLabelText(/Importe anual/), { target: { value: '1200' } });
    fireEvent.change(
      screen.getByLabelText(/Cuenta o grupo/, {
        selector: 'section[aria-label="Presupuesto"] input',
      }),
      { target: { value: '705' } },
    );
    fireEvent.click(screen.getByRole('button', { name: 'Guardar línea' }));
    await vi.waitFor(() =>
      expect(api.budgetSet).toHaveBeenCalledWith(2026, {
        account: '705',
        center: '',
        months: Array(12).fill(10000),
      }),
    );
  });
});

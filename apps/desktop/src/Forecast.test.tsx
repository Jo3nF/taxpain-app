import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { type CashForecast, type ForecastBucket, setDemoInvoke } from './api';
import { Forecast } from './Forecast';

const bucket = (
  start: string,
  end: string,
  opening: number,
  inflow: number,
  outflow: number,
): ForecastBucket => ({
  start,
  end,
  opening,
  inflows: inflow ? { collections: inflow } : {},
  outflows: outflow ? { payments: outflow } : {},
  total_in: inflow,
  total_out: outflow,
  closing: opening + inflow - outflow,
});

const points = [
  { date: '2026-10-24', balance: 100_000 },
  { date: '2026-10-24', balance: 221_000 },
  { date: '2026-11-02', balance: -79_000 },
];

const data: CashForecast = {
  today: '2026-10-24',
  opening: 100_000,
  weekly: {
    buckets: [
      bucket('2026-10-24', '2026-10-25', 100_000, 121_000, 0),
      bucket('2026-10-26', '2026-11-01', 221_000, 0, 0),
      bucket('2026-11-02', '2026-11-08', 221_000, 0, 300_000),
    ],
    end: '2026-11-08',
    points,
    lowest: -79_000,
    lowest_date: '2026-11-02',
    first_negative: '2026-11-02',
  },
  monthly: {
    buckets: [
      bucket('2026-10-24', '2026-10-31', 100_000, 121_000, 0),
      bucket('2026-11-01', '2026-11-30', 221_000, 0, 300_000),
    ],
    end: '2026-11-30',
    points,
    lowest: -79_000,
    lowest_date: '2026-11-02',
    first_negative: '2026-11-02',
  },
  not_yours: { vat: 52_500, withholdings: 30_000, total: 82_500 },
  items: [
    {
      date: '2026-10-24',
      due_date: '2026-08-31',
      concept: 'Cobro A2026-0001 · Cliente Ejemplo SA',
      amount: 121_000,
      category: 'collections',
      source: 'cartera',
      overdue: true,
      estimated: false,
    },
    {
      date: '2026-11-02',
      due_date: '2026-11-02',
      concept: 'Pago P-7 · Proveedor Ejemplo SL',
      amount: -300_000,
      category: 'payments',
      source: 'cartera',
      overdue: false,
      estimated: false,
    },
  ],
  warnings: [
    {
      kind: 'negative',
      text: 'Saldo negativo desde la semana del 2 de noviembre; mínimo de -790,00 € el 02/11/2026.',
    },
    {
      kind: 'overdue',
      text: 'Cobros vencidos: 1 por 1.210,00 €. Se cuentan como si se cobraran esta semana.',
    },
  ],
  notes: ['Impuestos: el periodo en curso y los terminados sin presentar.'],
};

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

describe('Forecast', () => {
  it('muestra el saldo previsto, el mínimo, los avisos y el detalle', async () => {
    const calls: string[] = [];
    setDemoInvoke(async (cmd) => {
      calls.push(cmd);
      return data;
    });
    render(<Forecast />);
    expect(await screen.findByText('Previsión de tesorería')).toBeTruthy();
    expect(calls).toEqual(['cash_forecast']);
    // Saldo negativo: aviso en rojo; el resto de avisos, en ámbar.
    const red = screen.getByText(/Saldo negativo desde la semana del 2 de noviembre/).closest('p');
    expect(red?.className).toContain('bg-red-50');
    expect(screen.getByText(/Cobros vencidos: 1 por 1.210,00 €/)).toBeTruthy();
    // Cifras: mínimo en rojo y el dinero de Hacienda con su desglose.
    const min = screen.getByText('-790,00 €', { selector: 'dd' });
    expect(min.className).toContain('text-red-700');
    expect(screen.getByText('825,00 €')).toBeTruthy();
    expect(screen.getByText('IVA 525,00 € y retenciones 300,00 € por ingresar')).toBeTruthy();
    // Tabla por semanas (la vista accesible del gráfico) y movimientos.
    const weeks = screen.getByRole('table', { name: 'Saldo previsto por semanas' });
    expect(within(weeks).getAllByRole('row')).toHaveLength(4);
    expect(within(weeks).getByText('Semana del 2 nov')).toBeTruthy();
    const moves = screen.getByRole('table', { name: 'Cobros y pagos previstos' });
    expect(within(moves).getByText('vencido')).toBeTruthy();
    expect(within(moves).getByText('+1.210,00 €')).toBeTruthy();
    expect(within(moves).getByText('-3.000,00 €')).toBeTruthy();
    expect(screen.getByText('Movimientos previstos (2)')).toBeTruthy();
  });

  it('cambia a 12 meses y recorre los periodos con el teclado', async () => {
    setDemoInvoke(async () => data);
    render(<Forecast />);
    const months = await screen.findByRole('button', { name: '12 meses' });
    expect(screen.getByRole('button', { name: '13 semanas' }).getAttribute('aria-pressed')).toBe(
      'true',
    );
    fireEvent.click(months);
    expect(months.getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByText('Ver por meses')).toBeTruthy();
    expect(screen.getByText('Octubre de 2026')).toBeTruthy();

    const chart = screen.getByRole('figure', { name: /Saldo previsto por meses/ });
    fireEvent.keyDown(chart, { key: 'ArrowRight' });
    fireEvent.keyDown(chart, { key: 'ArrowRight' });
    expect(
      screen.getByText(
        'Noviembre de 2026: entradas 0,00 €, salidas 3.000,00 €, saldo final -790,00 €.',
      ),
    ).toBeTruthy();
    fireEvent.keyDown(chart, { key: 'Escape' });
    expect(screen.queryByText(/Noviembre de 2026: entradas/)).toBeNull();
  });

  it('si no se puede calcular, lo dice sin romper el panel', async () => {
    setDemoInvoke(async () => {
      throw { message: 'almacén bloqueado' };
    });
    render(<Forecast />);
    expect(
      await screen.findByText(
        'No se ha podido calcular la previsión de tesorería: almacén bloqueado',
      ),
    ).toBeTruthy();
  });
});

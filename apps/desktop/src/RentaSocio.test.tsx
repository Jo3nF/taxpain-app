import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { setDemoInvoke } from './api';
import { installDemo } from './demo';
import { RentaSocio } from './RentaSocio';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

describe('RentaSocio', () => {
  it('muestra los conceptos con su casilla y el semáforo de la exención', async () => {
    installDemo();
    render(<RentaSocio year={2026} />);
    const t = await screen.findByRole('table', { name: 'Conceptos para la Renta' });
    const nominas = within(t).getByRole('row', { name: /Retribuciones dinerarias/ });
    expect(nominas.textContent).toContain('0001');
    expect(nominas.textContent).toContain('24.000,00 €');
    expect(nominas.textContent).toContain('4.560,00 €');
    expect(within(t).getByRole('row', { name: /Dividendos/ }).textContent).toContain('0029');
    const valor = screen.getByRole('table', { name: 'Valor de las participaciones' });
    expect(valor.textContent).toContain('60.000,00 €');
    expect(valor.textContent).toContain('60 %');
    expect(screen.getByText('Dudoso: revísalo')).toBeTruthy();
    const req = screen.getByRole('list', { name: 'Requisitos de la exención' });
    expect(within(req).getAllByRole('listitem')).toHaveLength(3);
    expect(screen.getByText(/no calcula el IRPF/)).toBeTruthy();
  });

  it('pide el informe con los otros rendimientos tecleados', async () => {
    const calls: [string, unknown][] = [];
    installDemo();
    const demo = (await import('./demoSubvenciones')).subvencionesHandlers;
    setDemoInvoke(async (cmd, args) => {
      calls.push([cmd, args]);
      return (demo[cmd] ?? (() => null))(args);
    });
    render(<RentaSocio year={2026} />);
    await screen.findByRole('table', { name: 'Conceptos para la Renta' });
    fireEvent.change(screen.getByLabelText(/Otros rendimientos/), {
      target: { value: '10.000,00' },
    });
    await waitFor(() =>
      expect(
        calls.some(
          ([c, a]) =>
            c === 'renta_socio' &&
            (a as { otros: number | null; nif: string }).otros === 1_000_000 &&
            (a as { nif: string }).nif === '12345678Z',
        ),
      ).toBe(true),
    );
  });

  it('sin socios ni administradores explica dónde anotarlos', async () => {
    setDemoInvoke(async () => []);
    render(<RentaSocio year={2026} />);
    expect(await screen.findByText(/Anota a los socios personas físicas/)).toBeTruthy();
  });
});

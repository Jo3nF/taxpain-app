import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { setDemoInvoke } from './api';
import { installDemo } from './demo';
import { Subvenciones } from './Subvenciones';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

describe('Subvenciones', () => {
  it('muestra la subvención de la demo con su efecto impositivo', async () => {
    installDemo();
    render(<Subvenciones year={2026} />);
    const t = await screen.findByRole('article', { name: 'Ayuntamiento de Ejemplo' });
    expect(t.textContent).toContain('financia «Máquina demo»');
    const estado = within(t).getByRole('table', { name: /Estado de/ });
    expect(estado.textContent).toContain('4.500,00 €');
    expect(estado.textContent).toContain('1.500,00 €');
    const totales = screen.getByRole('table', { name: 'Totales de las subvenciones' });
    expect(totales.textContent).toContain('300.000,00 €');
    expect(screen.getByText(/art\. 19 LIS/)).toBeTruthy();
  });

  it('enseña el asiento de la imputación y lo graba con confirmación', async () => {
    const calls: [string, unknown][] = [];
    installDemo();
    const demo = (await import('./demoSubvenciones')).subvencionesHandlers;
    setDemoInvoke(async (cmd, args) => {
      calls.push([cmd, args]);
      const h = demo[cmd] ?? (() => []);
      return h(args);
    });
    render(<Subvenciones year={2026} />);
    const t = await screen.findByRole('article', { name: 'Ayuntamiento de Ejemplo' });
    // La imputación propuesta por la amortización: 1.200 €.
    expect(within(t).getByDisplayValue('1200,00')).toBeTruthy();
    fireEvent.click(within(t).getByRole('button', { name: /Ver el asiento: imputar/ }));
    const asiento = await within(t).findByRole('table', { name: 'Asiento propuesto' });
    expect(asiento.textContent).toContain('746');
    expect(asiento.textContent).toContain('479');
    fireEvent.click(within(t).getByRole('button', { name: 'Imputar a resultados' }));
    fireEvent.click(await within(t).findByRole('button', { name: 'Contabilizar' }));
    expect(await screen.findByText('Asiento contabilizado.')).toBeTruthy();
    const reg = calls.find(([c]) => c === 'subvencion_registrar');
    expect(reg?.[1]).toEqual({
      id: 1,
      accion: { accion: 'imputacion', year: 2026, importe: 120_000 },
      confirmed: true,
    });
  });

  it('da de alta una subvención', async () => {
    const calls: [string, unknown][] = [];
    setDemoInvoke(async (cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === 'subvenciones')
        return {
          year: 2026,
          subvenciones: [],
          imputado_ejercicio: 0,
          saldo_pn: 0,
          pasivo_diferido: 0,
          minimis_tres_años: 0,
          tope_minimis: 30_000_000,
          notas: [],
        };
      if (cmd === 'fixed_assets') return [];
      if (cmd === 'subvencion_guardar') return 3;
      return null;
    });
    render(<Subvenciones year={2026} />);
    expect(await screen.findByText('No hay subvenciones anotadas.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Nueva subvención' }));
    fireEvent.change(screen.getByLabelText('Concedente'), {
      target: { value: 'Diputación de Ejemplo' },
    });
    fireEvent.change(screen.getByLabelText('Importe concedido'), { target: { value: '2.000,00' } });
    fireEvent.change(screen.getByLabelText('Tipo'), { target: { value: 'explotacion' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    expect(await screen.findByText('Subvención anotada.')).toBeTruthy();
    const g = calls.find(([c]) => c === 'subvencion_guardar')?.[1] as {
      subvencion: { importe: number; tipo: string; concedente: string };
    };
    expect(g.subvencion).toMatchObject({
      importe: 200_000,
      tipo: 'explotacion',
      concedente: 'Diputación de Ejemplo',
    });
  });
});

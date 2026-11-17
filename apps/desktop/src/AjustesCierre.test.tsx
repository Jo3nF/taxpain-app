import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { AjustesCierre } from './AjustesCierre';
import { setDemoInvoke } from './api';
import { ajustesCierreHandlers } from './demoAjustesCierre';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

function demo(calls: { cmd: string; args?: Record<string, unknown> }[]) {
  setDemoInvoke(async (cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    const h = ajustesCierreHandlers[cmd];
    if (!h) throw { message: `inesperado ${cmd}` };
    return h(args as Record<string, unknown>);
  });
}

describe('AjustesCierre', () => {
  it('periodifica un seguro con su reversión y lo deshace', async () => {
    const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
    demo(calls);
    const onGo = vi.fn();
    render(<AjustesCierre year={2026} onGo={onGo} />);
    expect(await screen.findByRole('navigation', { name: 'Pasos del cierre' })).toBeTruthy();
    // Comprobaciones con enlace a donde se arreglan.
    expect(await screen.findByText(/555 Partidas pendientes de aplicación: 150,00 €/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Abrir el mayor de la 555' }));
    expect(onGo).toHaveBeenCalledWith('mayor', '555');
    fireEvent.click(screen.getByRole('button', { name: /reclasificación al corto plazo/ }));
    expect(onGo).toHaveBeenCalledWith('prestamos');

    // Seguro del 1/10/2026 propuesto hasta el 30/9/2027.
    const hasta = (await screen.findByLabelText(
      'Servicio hasta (POL-2026-77 de Seguros Inventados SA)',
    )) as HTMLInputElement;
    expect(hasta.value).toBe('2027-09-30');
    fireEvent.click(screen.getByRole('button', { name: 'Periodificar POL-2026-77' }));
    expect(await screen.findByText(/273 de 365 días/)).toBeTruthy();
    expect(screen.getByText(/reversión automática el 01\/01\/2027/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Contabilizar' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, contabilizar' }));
    await vi.waitFor(() => expect(calls.some((c) => c.cmd === 'ajustes_cierre_crear')).toBe(true));
    const crear = calls.find((c) => c.cmd === 'ajustes_cierre_crear');
    expect(crear?.args?.nuevo).toMatchObject({
      tipo: 'gasto_anticipado',
      factura_id: 11,
      desde: '2026-10-01',
      hasta: '2027-09-30',
    });
    expect(await screen.findByText('Periodificada en este cierre (abajo).')).toBeTruthy();
    const seccion = screen.getByRole('region', { name: 'Periodificaciones de gastos e ingresos' });
    fireEvent.click(within(seccion).getByRole('button', { name: /^Deshacer:/ }));
    fireEvent.click(within(seccion).getByRole('button', { name: 'Sí, quitarlo' }));
    await vi.waitFor(() =>
      expect(calls.some((c) => c.cmd === 'ajustes_cierre_deshacer')).toBe(true),
    );
  });

  it('usa una factura de enero como pendiente de recibir y deteriora un cliente', async () => {
    const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
    demo(calls);
    render(<AjustesCierre year={2026} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Usar ASE-12' }));
    expect((screen.getByLabelText('Importe (base)') as HTMLInputElement).value).toBe('300,00');
    const pendientes = screen.getByRole('region', {
      name: 'Facturas pendientes de recibir o de emitir',
    });
    fireEvent.click(within(pendientes).getByRole('button', { name: 'Ver el asiento' }));
    expect(
      await within(pendientes).findAllByText(/facturas pendientes de recibir o de formalizar/),
    ).toHaveLength(2);
    fireEvent.click(within(pendientes).getByRole('button', { name: 'Contabilizar' }));
    fireEvent.click(within(pendientes).getByRole('button', { name: 'Sí, contabilizar' }));
    await vi.waitFor(() => expect(calls.some((c) => c.cmd === 'ajustes_cierre_crear')).toBe(true));
    expect(calls.find((c) => c.cmd === 'ajustes_cierre_crear')?.args?.nuevo).toMatchObject({
      tipo: 'pendiente_recibir',
      importe: 30_000,
      cuenta: '623',
      factura_kind: 'recibida',
      factura_id: 31,
    });

    // Deterioro: con garantía no es deducible.
    const det = screen.getByRole('region', { name: 'Deterioro de clientes' });
    fireEvent.change(within(det).getByLabelText('Crédito excluido'), {
      target: { value: 'garantia' },
    });
    fireEvent.click(within(det).getByRole('button', { name: 'Deteriorar A2026-0004' }));
    expect(await within(det).findByText('No deducible (art. 13.1 LIS).')).toBeTruthy();
  });

  it('una provisión sin descripción no se propone', async () => {
    demo([]);
    render(<AjustesCierre year={2026} />);
    const prov = await screen.findByRole('region', { name: 'Provisiones' });
    fireEvent.change(within(prov).getByLabelText('Importe estimado'), {
      target: { value: '4.000' },
    });
    fireEvent.click(within(prov).getByRole('button', { name: 'Ver el asiento' }));
    expect((await within(prov).findByRole('alert')).textContent).toMatch(/describe el riesgo/);
  });
});

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setDemoInvoke } from './api';
import { CierreTrimestre, trimestrePorCerrar } from './CierreTrimestre';
import { trimestreHandlers } from './demoTrimestre';
import { ObligacionesNoFiscales } from './ObligacionesNoFiscales';
import { PuestaEnMarcha } from './PuestaEnMarcha';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

type Call = { cmd: string; args?: Record<string, unknown> };

function demo(calls: Call[]) {
  setDemoInvoke(async (cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    if (cmd === 'list_certificates') return [];
    const h = trimestreHandlers[cmd];
    if (!h) throw { message: `inesperado ${cmd}` };
    return h(args as Record<string, unknown>);
  });
}

describe('trimestrePorCerrar', () => {
  it('en el primer mes de un trimestre, el anterior', () => {
    expect(trimestrePorCerrar(new Date(2026, 9, 13))).toEqual({ year: 2026, quarter: 3 });
    expect(trimestrePorCerrar(new Date(2026, 10, 16))).toEqual({ year: 2026, quarter: 4 });
    expect(trimestrePorCerrar(new Date(2027, 0, 10))).toEqual({ year: 2026, quarter: 4 });
  });
});

describe('CierreTrimestre', () => {
  it('dice qué falta, lleva a cada sitio, marca a mano y cambia de trimestre', async () => {
    const calls: Call[] = [];
    demo(calls);
    const onGo = vi.fn();
    render(<CierreTrimestre onGo={onGo} now={new Date(2026, 10, 16)} />);
    expect(await screen.findByText(/El 4T de 2026 aún no está listo/)).toBeTruthy();
    expect(screen.getByText(/diferencia de 210,00/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Ir: Banco conciliado' }));
    expect(onGo).toHaveBeenCalledWith('contabilidad', 'banco');
    fireEvent.click(
      screen.getByRole('button', {
        name: 'Marcar como hecho: Facturas emitidas vencidas cobradas',
      }),
    );
    await vi.waitFor(() =>
      expect(calls.find((c) => c.cmd === 'cierre_trimestre_marcar')?.args).toMatchObject({
        quarter: 4,
        paso: 'cobros',
        marca: 'hecho',
      }),
    );
    expect(await screen.findByText(/Marcado a mano: hecho/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: /^3T/ }));
    expect(await screen.findByText('El 3T de 2026 está listo para presentar.')).toBeTruthy();
    expect(screen.getByText(/A ingresar:/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: /^1T/ }));
    expect(await screen.findByText('El 1T de 2026 está presentado.')).toBeTruthy();
  });
});

describe('PuestaEnMarcha', () => {
  it('marca el progreso, enseña el correo y guarda la fecha de corte', async () => {
    const calls: Call[] = [];
    demo(calls);
    const onGo = vi.fn();
    render(<PuestaEnMarcha onGo={onGo} />);
    expect(await screen.findByText('2 de 7 pasos resueltos')).toBeTruthy();
    const correo = screen.getByLabelText('Texto del correo a la gestoría') as HTMLTextAreaElement;
    expect(correo.value).toMatch(/ESTUDIO DEMO SL \(NIF B12345674\)/);
    fireEvent.click(screen.getByRole('button', { name: 'Ir: Fecha de corte y saldos iniciales' }));
    expect(onGo).toHaveBeenCalledWith('contabilidad', 'apertura');
    fireEvent.change(screen.getByLabelText('Fecha de corte'), { target: { value: '2026-10-01' } });
    fireEvent.blur(screen.getByLabelText('Fecha de corte'));
    await vi.waitFor(() =>
      expect(calls.find((c) => c.cmd === 'puesta_marcha_marcar')?.args).toMatchObject({
        paso: 'saldos',
        fecha: '2026-10-01',
      }),
    );
    const revocar = screen.getByRole('group', {
      name: 'Estado: Revocar el apoderamiento y la representación',
    });
    fireEvent.click(
      revocar.querySelector('button[aria-pressed="false"]:nth-of-type(2)') as Element,
    );
    expect(await screen.findByText('3 de 7 pasos resueltos')).toBeTruthy();
    // Nada se envía: no hay ninguna orden de correo.
    expect(calls.some((c) => /correo|email|send/.test(c.cmd))).toBe(false);
  });
});

describe('ObligacionesNoFiscales', () => {
  it('guarda si aplica y avisa de las que piden empleados', async () => {
    const calls: Call[] = [];
    demo(calls);
    render(<ObligacionesNoFiscales />);
    expect(await screen.findByText('1 de 3 revisadas (hechas o que no aplican).')).toBeTruthy();
    expect(screen.getByText('No tienes empleados en Nóminas.')).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Estado: Registro de la jornada'), {
      target: { value: 'no_aplica' },
    });
    expect(await screen.findByText('2 de 3 revisadas (hechas o que no aplican).')).toBeTruthy();
    expect(calls.find((c) => c.cmd === 'obligacion_no_fiscal_marcar')?.args).toMatchObject({
      id: 'registro_horario',
      estado: 'no_aplica',
    });
  });
});

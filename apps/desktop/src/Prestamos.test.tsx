import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { type PrestamoVista, setDemoInvoke } from './api';
import { installDemo } from './demo';
import { Prestamos, parsePct } from './Prestamos';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const cuota = {
  n: 1,
  fecha: '2026-02-28',
  intereses: 10_000,
  capital: 100_000,
  capitalizados: 0,
  pago: 110_000,
  pendiente: 2_300_000,
  estado: 'pendiente' as const,
  retencion: 1_900,
  neto: 108_100,
  devengado: 0,
  bank_tx: null,
};

const vista = (alta: PrestamoVista['alta']): PrestamoVista => ({
  prestamo: {
    id: 7,
    nombre: 'Préstamo de la socia',
    sentido: 'recibido',
    contraparte: {
      origen: 'texto',
      ref_id: null,
      nif: '12345678Z',
      nombre: 'Carmen Española Española',
      provincia: '46',
      persona_fisica: true,
      entidad_credito: false,
      vinculo: 'otra',
    },
    inicio: '2026-01-31',
    principal: 2_400_000,
    tipo: 0,
    sistema: 'lineal',
    periodicidad: 1,
    plazo: 24,
    carencia: 0,
    carencia_tipo: 'capital',
    cuentas: { largo: '1635001', corto: '5135001', intereses: '662', devengo: '5145' },
    retencion: 1900,
    notas: '',
  },
  alta,
  alta_bank_tx: null,
  cuotas: [cuota, { ...cuota, n: 2, fecha: '2026-03-31' }],
  devengos: [],
  saldo_vivo: 2_400_000,
  corto_plazo: 1_200_000,
  largo_plazo: 1_200_000,
  vencidas: 2,
  avisos: ['Préstamo sin intereses con una parte vinculada: … (art. 18.1 LIS).'],
});

describe('Prestamos', () => {
  it('lee porcentajes', () => {
    expect(parsePct('4,75')).toBe(475);
    expect(parsePct('19')).toBe(1900);
    expect(parsePct('101')).toBeNull();
    expect(parsePct('abc')).toBeNull();
  });

  it('contabiliza el alta con el movimiento del banco y registra la cuota con confirmación', async () => {
    const calls: [string, unknown][] = [];
    let alta: PrestamoVista['alta'] = 'pendiente';
    setDemoInvoke(async (cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === 'prestamos_list') return [vista(alta)];
      if (cmd === 'shareholders_list') return [];
      if (cmd === 'prestamo_propuesta')
        return {
          cuentas: { largo: '1700001', corto: '5200001', intereses: '662', devengo: '527' },
          retencion: 0,
          motivo: 'Sin retención.',
        };
      if (cmd === 'prestamo_bank_candidates') {
        const n = (args as { n: number }).n;
        return n === 0
          ? [
              {
                id: 31,
                fecha: '2026-01-31',
                importe: 2_400_000,
                concepto: 'TRANSFERENCIA SOCIA',
                cuenta: '5720001',
                banco: 'Banco',
              },
            ]
          : [];
      }
      if (cmd === 'prestamo_post_start') {
        alta = 'contabilizada';
        return 5;
      }
      if (cmd === 'prestamo_post_installment') return 6;
      throw { message: `inesperado ${cmd}` };
    });
    render(<Prestamos year={2026} />);
    expect(await screen.findByText(/Préstamo sin intereses con una parte vinculada/)).toBeTruthy();
    expect(screen.getByText(/2 cuotas vencidas sin registrar/)).toBeTruthy();
    // El movimiento del banco que cuadra sale propuesto.
    expect(await screen.findByText(/TRANSFERENCIA SOCIA/)).toBeTruthy();
    fireEvent.click(
      screen.getByRole('button', { name: 'Contabilizar la recepción del principal' }),
    );
    const dialog = screen.getByRole('alertdialog');
    expect(dialog.textContent).toMatch(/conciliado con el movimiento del banco/);
    expect(calls.some(([c]) => c === 'prestamo_post_start')).toBe(false);
    fireEvent.click(within(dialog).getByRole('button', { name: 'Sí, contabilizar' }));
    await vi.waitFor(() =>
      expect(calls.find(([c]) => c === 'prestamo_post_start')?.[1]).toEqual({
        id: 7,
        pago: { tesoreria: null, bank_tx: 31 },
        confirmed: true,
      }),
    );
    // Con el alta hecha, la siguiente cuota: sin movimiento del banco, contra la 572.
    fireEvent.click(await screen.findByRole('button', { name: 'Registrar cuota 1' }));
    const d2 = screen.getByRole('alertdialog');
    expect(d2.textContent).toMatch(/retención 19,00 € a la 4751/);
    fireEvent.click(within(d2).getByRole('button', { name: 'Sí, registrar la cuota' }));
    await vi.waitFor(() =>
      expect(calls.find(([c]) => c === 'prestamo_post_installment')?.[1]).toEqual({
        id: 7,
        n: 1,
        pago: { tesoreria: '572', bank_tx: null },
        confirmed: true,
      }),
    );
    await vi.waitFor(() => expect(screen.getByRole('status').textContent).toMatch(/123/));
  });

  it('da de alta un préstamo con las cuentas propuestas', async () => {
    const calls: [string, unknown][] = [];
    setDemoInvoke(async (cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === 'prestamos_list') return [];
      if (cmd === 'shareholders_list') return [];
      if (cmd === 'prestamo_propuesta')
        return {
          cuentas: { largo: '1700001', corto: '5200001', intereses: '662', devengo: '527' },
          retencion: 0,
          motivo: 'Los intereses que se pagan a un banco no llevan retención.',
        };
      if (cmd === 'prestamo_create') return 1;
      throw { message: `inesperado ${cmd}` };
    });
    render(<Prestamos year={2026} />);
    expect(await screen.findByText('No hay préstamos dados de alta.')).toBeTruthy();
    await vi.waitFor(() =>
      expect((screen.getByLabelText('Cuenta a largo plazo') as HTMLInputElement).value).toBe(
        '1700001',
      ),
    );
    fireEvent.change(screen.getByLabelText('Nombre del préstamo'), {
      target: { value: 'Préstamo del banco' },
    });
    fireEvent.change(screen.getByLabelText('Nombre de la contraparte'), {
      target: { value: 'Banco Inventado' },
    });
    fireEvent.change(screen.getByLabelText('Principal'), { target: { value: '60.000' } });
    fireEvent.change(screen.getByLabelText('Interés nominal anual %'), {
      target: { value: '4,5' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Dar de alta' }));
    await vi.waitFor(() => expect(calls.some(([c]) => c === 'prestamo_create')).toBe(true));
    const args = calls.find(([c]) => c === 'prestamo_create')?.[1] as
      | { prestamo: Record<string, unknown> }
      | undefined;
    const p = args?.prestamo ?? {};
    expect(p.principal).toBe(6_000_000);
    expect(p.tipo).toBe(450);
    expect(p.cuentas).toEqual({
      largo: '1700001',
      corto: '5200001',
      intereses: '662',
      devengo: '527',
    });
    await vi.waitFor(() => expect(screen.getByRole('status').textContent).toMatch(/dado de alta/));
  });

  it('en modo demo enseña los préstamos inventados', async () => {
    installDemo();
    render(<Prestamos year={2026} />);
    expect(await screen.findByText('Préstamo a Participada Inventada')).toBeTruthy();
    expect(screen.getByText('Préstamo del Banco Inventado')).toBeTruthy();
    expect(screen.getByText('Préstamo de la socia')).toBeTruthy();
    expect(screen.getAllByText(/Vinculada/)).toHaveLength(2);
  });
});

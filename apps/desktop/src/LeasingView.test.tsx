import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setDemoInvoke } from './api';
import { LeasingView } from './LeasingView';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const contrato = {
  id: 1,
  name: 'Furgoneta',
  start: '2026-01-15',
  principal: 3_000_000,
  months: 48,
  rate: 500,
  purchase_option: 60_000,
  vat: 2100,
  asset_account: '218',
  debt_account: '5240001',
};

const cuota = {
  n: 1,
  date: '2026-02-15',
  interest: 12_500,
  capital: 55_456,
  vat: 14_271,
  amount: 67_956,
  outstanding: 2_944_544,
  posted: false,
};

describe('LeasingView', () => {
  it('pide el arrendador de un contrato antiguo y después registra las cuotas como facturas', async () => {
    const calls: [string, unknown][] = [];
    let lessor: unknown = null;
    setDemoInvoke(async (cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === 'leasing_list')
        return [{ leasing: { ...contrato, lessor }, installments: [cuota] }];
      if (cmd === 'leasing_set_lessor') {
        lessor = (args as { lessor: unknown }).lessor;
        return null;
      }
      if (cmd === 'leasing_post_due') return { posted: 1, linked: [] };
      throw { message: `inesperado ${cmd}` };
    });
    render(<LeasingView year={2026} />);
    expect(await screen.findByText(/Indica el arrendador/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: /Registrar las/ })).toBeNull();
    // El primero es el del contrato (el segundo, el del alta de uno nuevo).
    const [nif] = screen.getAllByLabelText('NIF del arrendador');
    if (!nif) throw new Error('falta el NIF del arrendador');
    fireEvent.change(nif, { target: { value: 'B98765431' } });
    fireEvent.change(screen.getByLabelText('Nombre'), { target: { value: 'EMPRESA DEMO SL' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar arrendador' }));
    await vi.waitFor(() =>
      expect(calls.find(([c]) => c === 'leasing_set_lessor')?.[1]).toEqual({
        id: 1,
        lessor: { nif: 'B98765431', name: 'EMPRESA DEMO SL', deductible_pct: 100 },
      }),
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Registrar las 1 cuotas vencidas' }));
    await vi.waitFor(() => expect(screen.getByRole('status').textContent).toMatch(/303/));
    const post = calls.find(([c]) => c === 'leasing_post_due')?.[1] as Record<string, unknown>;
    expect(post.id).toBe(1);
    expect(post).not.toHaveProperty('treasury');
  });

  it('ofrece regularizar las cuotas antiguas con confirmación y explica las bloqueadas', async () => {
    const calls: [string, unknown][] = [];
    const lessor = { nif: 'B98765431', name: 'EMPRESA DEMO SL', deductible_pct: 100 };
    let regularized = false;
    setDemoInvoke(async (cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === 'leasing_list')
        return [
          {
            leasing: { ...contrato, lessor },
            installments: [
              {
                ...cuota,
                posted: true,
                legacy: true,
                blocked: 'el 303 del 1T de 2026 ya está presentado',
              },
              { ...cuota, n: 2, date: '2026-04-15', posted: true, legacy: !regularized },
            ],
          },
        ];
      if (cmd === 'leasing_regularize') {
        regularized = true;
        return { done: 1, blocked: ['cuota 1/48: el 303 del 1T de 2026 ya está presentado'] };
      }
      throw { message: `inesperado ${cmd}` };
    });
    render(<LeasingView year={2026} />);
    expect(await screen.findByText(/Hay 2 cuotas registradas con el sistema antiguo/)).toBeTruthy();
    expect(screen.getByText(/el 303 del 1T de 2026 ya está presentado/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Regularizar 1 cuota' }));
    // Pide confirmación y explica qué hará antes de llamar.
    const dialog = screen.getByRole('alertdialog');
    expect(dialog.textContent).toMatch(/se borra su asiento antiguo/);
    expect(calls.some(([c]) => c === 'leasing_regularize')).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'Sí, regularizar' }));
    await vi.waitFor(() =>
      expect(calls.find(([c]) => c === 'leasing_regularize')?.[1]).toEqual({
        id: 1,
        confirmed: true,
      }),
    );
    await vi.waitFor(() =>
      expect(screen.getByRole('status').textContent).toMatch(/1 cuota regularizada/),
    );
    expect(await screen.findByText(/Hay 1 cuota registrada con el sistema antiguo/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: /^Regularizar/ })).toBeNull();
  });

  it('avisa de los vencimientos a +30 días y los corrige', async () => {
    const calls: [string, unknown][] = [];
    const lessor = { nif: 'B98765431', name: 'EMPRESA DEMO SL', deductible_pct: 100 };
    let fixed = false;
    setDemoInvoke(async (cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === 'leasing_list')
        return [
          {
            leasing: { ...contrato, lessor },
            installments: [
              { ...cuota, posted: true, due_mismatch: !fixed },
              { ...cuota, n: 2, date: '2026-03-15', posted: true, due_mismatch: !fixed },
              { ...cuota, n: 3, date: '2026-04-15', posted: true, due_mismatch: false },
            ],
          },
        ];
      if (cmd === 'leasing_fix_due_dates') {
        fixed = true;
        return 2;
      }
      throw { message: `inesperado ${cmd}` };
    });
    render(<LeasingView year={2026} />);
    expect(await screen.findByText(/Hay 2 cuotas sin pagar cuyo vencimiento/)).toBeTruthy();
    // Con cuotas contabilizadas no se ofrece borrar el contrato.
    expect(screen.queryByRole('button', { name: 'Borrar el contrato' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Corregir vencimientos' }));
    await vi.waitFor(() =>
      expect(calls.find(([c]) => c === 'leasing_fix_due_dates')?.[1]).toEqual({ id: 1 }),
    );
    await vi.waitFor(() =>
      expect(screen.getByRole('status').textContent).toMatch(/2 vencimientos corregidos/),
    );
    expect(screen.queryByText(/cuyo vencimiento/)).toBeNull();
  });

  it('borra un contrato sin cuotas con confirmación y cuenta las cuotas enlazadas', async () => {
    const calls: [string, unknown][] = [];
    const lessor = { nif: 'B98765431', name: 'EMPRESA DEMO SL', deductible_pct: 100 };
    let deleted = false;
    setDemoInvoke(async (cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === 'leasing_list')
        return deleted ? [] : [{ leasing: { ...contrato, lessor }, installments: [cuota] }];
      if (cmd === 'leasing_post_due')
        return {
          posted: 0,
          linked: ['cuota 1/48: enlazada con la factura «L-2026-0001» que ya estaba en Recibidas'],
        };
      if (cmd === 'leasing_delete') {
        deleted = true;
        return null;
      }
      throw { message: `inesperado ${cmd}` };
    });
    render(<LeasingView year={2026} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Registrar las 1 cuotas vencidas' }));
    await vi.waitFor(() =>
      expect(screen.getByRole('status').textContent).toMatch(
        /Ninguna factura nueva de Furgoneta. Una cuota ya tenía su factura en Recibidas .*L-2026-0001/,
      ),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Borrar el contrato' }));
    const dialog = screen.getByRole('alertdialog');
    expect(dialog.textContent).toMatch(/dado de alta por error/);
    expect(calls.some(([c]) => c === 'leasing_delete')).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'Sí, borrar el contrato' }));
    await vi.waitFor(() =>
      expect(calls.find(([c]) => c === 'leasing_delete')?.[1]).toEqual({ id: 1, confirmed: true }),
    );
    await vi.waitFor(() =>
      expect(screen.getByRole('status').textContent).toMatch(/«Furgoneta» borrado/),
    );
    expect(screen.queryByText(/Furgoneta ·/)).toBeNull();
  });
});

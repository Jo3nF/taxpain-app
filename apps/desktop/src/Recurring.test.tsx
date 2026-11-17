import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { type InvoiceTemplate, setDemoInvoke } from './api';
import { Recurring } from './Recurring';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const draft = {
  kind: 'Completa' as const,
  series: null,
  issue_date: '2026-09-01',
  operation_date: null,
  recipient: { id: { Nif: 'B12345674' }, name: 'Cliente SL', address: null, person: null },
  lines: [
    {
      description: 'Cuota {mes}',
      quantity_milli: 1000,
      unit_price: 10_000,
      discount: 0,
      vat: { Taxed: { rate: 2100 } },
      surcharge: null,
      withholding: null,
    },
  ],
  rectified: [],
  rectified_amounts: null,
  description: 'Cuota {mes}',
  notes: null,
};

const t: InvoiceTemplate = {
  id: 'abc',
  name: 'Cuota Cliente',
  draft,
  language: null,
  every_months: 1,
  start: '2026-10-01',
  issued: 0,
  end: null,
  active: true,
  last: ['A-1', '2026-09-01'],
  next_date: '2026-10-01',
  due: true,
  client: 'Cliente SL',
};

describe('Recurring', () => {
  it('enseña lo que se va a emitir y pide confirmación', async () => {
    const calls: string[] = [];
    setDemoInvoke(async (cmd, args) => {
      calls.push(`${cmd}:${JSON.stringify(args)}`);
      if (cmd === 'recurring_preview')
        return {
          draft: {
            ...draft,
            issue_date: '2026-10-03',
            operation_date: '2026-10-01',
            lines: [{ ...draft.lines[0], description: 'Cuota octubre 2026' }],
          },
          totals: { invoice_total: 12_100 },
        };
      if (cmd === 'recurring_issue') return { id: 2, number: 'A-2' };
      return null;
    });
    const msg = vi.fn();
    const changed = vi.fn();
    render(<Recurring templates={[t]} onChanged={changed} onMessage={msg} />);
    expect(screen.getByText('1 toca')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Emitir…' }));
    expect(await screen.findByText(/Cuota octubre 2026/)).toBeTruthy();
    expect(screen.getByText(/operación del 01\/10\/2026/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Emitir esta factura' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, emitir' }));
    await vi.waitFor(() => expect(msg).toHaveBeenCalledWith('Emitida A-2.'));
    expect(calls).toContain('recurring_issue:{"id":"abc","confirmed":true}');
    expect(changed).toHaveBeenCalled();
  });

  it('en dólares pide el tipo del día de la operación', async () => {
    const calls: string[] = [];
    setDemoInvoke(async (cmd, args) => {
      calls.push(`${cmd}:${JSON.stringify(args)}`);
      if (cmd === 'recurring_preview')
        return {
          draft: {
            ...draft,
            issue_date: '2026-10-03',
            operation_date: '2026-10-01',
            currency: { code: 'USD', rate: '1.1193', rate_date: '2026-09-01', source: 'bce' },
          },
          totals: { invoice_total: 12_100 },
        };
      if (cmd === 'fx_bce_rate')
        return {
          currency: 'USD',
          requested: '2026-10-01',
          date: '2026-10-01',
          rate: '1.1208',
          cached: false,
        };
      if (cmd === 'recurring_issue') return { id: 2, number: 'A-2' };
      return null;
    });
    const msg = vi.fn();
    render(<Recurring templates={[t]} onChanged={vi.fn()} onMessage={msg} />);
    fireEvent.click(screen.getByRole('button', { name: 'Emitir…' }));
    expect(await screen.findByText('121,00 USD')).toBeTruthy();
    // El tipo guardado es el de la factura anterior: se avisa.
    expect(screen.getByText(/El tipo es del 01\/09\/2026/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Tipo del BCE' }));
    expect(await screen.findByDisplayValue('1,1208')).toBeTruthy();
    expect(calls).toContain('fx_bce_rate:{"currency":"USD","date":"2026-10-01"}');
    fireEvent.click(screen.getByRole('button', { name: 'Emitir esta factura' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, emitir' }));
    await vi.waitFor(() => expect(msg).toHaveBeenCalledWith('Emitida A-2.'));
    expect(calls).toContain(
      'recurring_issue:{"id":"abc","confirmed":true,"currency":{"code":"USD","rate":"1.1208","rate_date":"2026-10-01","source":"bce"}}',
    );
  });

  it('editar sin tocar la fecha no vuelve a empezar la serie', async () => {
    const calls: string[] = [];
    setDemoInvoke(async (cmd, args) => {
      calls.push(`${cmd}:${JSON.stringify(args)}`);
      return null;
    });
    const done = { ...t, next_date: null, due: false, issued: 5, end: '2026-06-30' };
    const changed = vi.fn();
    render(<Recurring templates={[done]} onChanged={changed} onMessage={vi.fn()} />);
    expect(screen.getByText(/terminada/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Editar' }));
    expect(screen.getByText(/pon una fecha sólo para volver a empezar/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Nombre'), { target: { value: 'Cuota 2026' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    await vi.waitFor(() => expect(changed).toHaveBeenCalled());
    const update = calls.find((c) => c.startsWith('recurring_update:')) ?? '';
    expect(update).toContain('"name":"Cuota 2026"');
    expect(update).toContain('"next_date":null');
  });

  it('cambiar la fecha a propósito la manda', async () => {
    const calls: string[] = [];
    setDemoInvoke(async (cmd, args) => {
      calls.push(`${cmd}:${JSON.stringify(args)}`);
      return null;
    });
    const changed = vi.fn();
    render(<Recurring templates={[t]} onChanged={changed} onMessage={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Editar' }));
    fireEvent.change(screen.getByLabelText(/Siguiente/), { target: { value: '2026-11-15' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    await vi.waitFor(() => expect(changed).toHaveBeenCalled());
    expect(calls.find((c) => c.startsWith('recurring_update:'))).toContain(
      '"next_date":"2026-11-15"',
    );
  });

  it('atrasada de un trimestre ya cerrado: pregunta cómo emitirla', async () => {
    const calls: string[] = [];
    setDemoInvoke(async (cmd, args) => {
      calls.push(`${cmd}:${JSON.stringify(args)}`);
      if (cmd === 'recurring_preview')
        return {
          draft: {
            ...draft,
            issue_date: '2026-10-25',
            operation_date: '2026-07-01',
            lines: [{ ...draft.lines[0], description: 'Cuota julio 2026' }],
          },
          totals: { invoice_total: 12_100 },
          late_vat:
            'el plazo del 303 del 3T de 2026, donde se declara su IVA, terminó el 20 de octubre de 2026',
        };
      if (cmd === 'recurring_issue') return { id: 3, number: 'A-3' };
      return null;
    });
    const msg = vi.fn();
    render(<Recurring templates={[t]} onChanged={vi.fn()} onMessage={msg} />);
    fireEvent.click(screen.getByRole('button', { name: 'Emitir…' }));
    expect(await screen.findByText(/terminó el 20 de octubre de 2026/)).toBeTruthy();
    // Sin elegir, no se puede emitir.
    expect(
      (screen.getByRole('button', { name: 'Emitir esta factura' }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(screen.getByText(/operación del 01\/07\/2026\)/)).toBeTruthy();
    fireEvent.click(screen.getByLabelText(/Sin fecha de la operación/));
    expect(screen.queryByText(/operación del 01\/07\/2026\)/)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Emitir esta factura' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, emitir' }));
    await vi.waitFor(() => expect(msg).toHaveBeenCalledWith('Emitida A-3.'));
    expect(calls).toContain('recurring_issue:{"id":"abc","confirmed":true,"late":"today"}');
  });

  it('no sale nada sin plantillas', () => {
    const { container } = render(
      <Recurring templates={[]} onChanged={() => {}} onMessage={() => {}} />,
    );
    expect(container.textContent).toBe('');
  });
});

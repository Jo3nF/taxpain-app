import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setDemoInvoke } from './api';
import { InvoiceEmailPanel } from './Email';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

describe('InvoiceEmailPanel', () => {
  it('propone el recordatorio, deja editarlo y lo abre en Mail', async () => {
    const opened: Record<string, unknown>[] = [];
    setDemoInvoke(async (cmd, args) => {
      if (cmd === 'invoice_email_compose')
        return {
          to: null,
          subject: 'Recordatorio de pago: factura A-7',
          body: 'Hola:\n\nSigue pendiente.\n',
          attachments: ['A-7.pdf'],
          language: 'Es',
          reminder: 2,
          last_reminder: '2026-10-01T10:00:00Z',
          pending: 121_000,
          overdue_days: 30,
        };
      if (cmd === 'invoice_email_open') {
        opened.push(args ?? {});
        return null;
      }
      throw { message: `inesperado ${cmd}` };
    });
    const done = vi.fn();
    render(
      <InvoiceEmailPanel invoiceId={7} number="A-7" reminder onClose={() => {}} onDone={done} />,
    );
    expect(await screen.findByText(/2\.º recordatorio/)).toBeTruthy();
    expect(screen.getByText(/Vencida hace 30 días/)).toBeTruthy();
    expect(screen.getByText('A-7.pdf')).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Para'), { target: { value: 'pagos@cliente.example' } });
    fireEvent.click(screen.getByRole('button', { name: 'Abrir en Mail' }));
    await vi.waitFor(() => expect(done).toHaveBeenCalled());
    expect(opened[0]).toEqual({
      id: 7,
      reminder: true,
      to: 'pagos@cliente.example',
      subject: 'Recordatorio de pago: factura A-7',
      body: 'Hola:\n\nSigue pendiente.\n',
    });
  });

  it('en plazos dice lo vencido y lo pendiente', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'invoice_email_compose')
        return {
          to: null,
          subject: 'Recordatorio de pago: factura A-8',
          body: 'Hola:\n',
          attachments: ['A-8.pdf'],
          language: 'Es',
          reminder: 1,
          last_reminder: null,
          pending: 121_000,
          overdue: 40_333,
          overdue_days: 10,
        };
      throw { message: `inesperado ${cmd}` };
    });
    render(
      <InvoiceEmailPanel invoiceId={8} number="A-8" reminder onClose={() => {}} onDone={vi.fn()} />,
    );
    expect(await screen.findByText(/vencido 403,33 € de 1\.210,00 € pendiente/)).toBeTruthy();
  });
});

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { type InvoicingSettings, setDemoInvoke } from './api';
import { Quotes } from './Quotes';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const settings: InvoicingSettings = {
  issuer: { id: { Nif: 'B98765431' }, name: 'Agencia SL', address: null, person: null },
  default_series: 'A',
  rectifying_series: 'R',
  simplified_series: 'S',
  language: 'Es',
  default_vat: 2100,
  default_withholding: null,
  outputs: { pdf: true, facturae: false },
  destination: null,
  rounding: 'PerRate',
  verifactu: { enabled: true, installation_id: 'TP-x', production: false },
  footer: null,
};

const draft = {
  kind: 'Completa' as const,
  series: null,
  issue_date: '2026-10-20',
  operation_date: null,
  recipient: { id: { Nif: '' }, name: 'Prospecto SL', address: null, person: null },
  lines: [
    {
      description: 'Web',
      quantity_milli: 1000,
      unit_price: 100_000,
      discount: 0,
      vat: { Taxed: { rate: 2100 } },
      surcharge: null,
      withholding: null,
    },
  ],
  rectified: [],
  rectified_amounts: null,
  description: 'Web',
  notes: null,
};

describe('Quotes', () => {
  it('la lista enseña el total a pagar (sin la retención)', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'quotes_list')
        return [
          {
            id: 9,
            kind: 'presupuesto',
            number: 'P2026-0009',
            issue_date: '2026-10-20',
            valid_until: null,
            status: 'enviado',
            recipient_name: 'EMPRESA DEMO SL',
            total: 121_000,
            payable: 106_000,
            draft,
            language: null,
            invoice_id: null,
            invoice_number: null,
            expired: false,
          },
        ];
      return null;
    });
    render(<Quotes settings={settings} />);
    expect(await screen.findByText(/1\.?060,00/)).toBeTruthy();
    expect(screen.queryByText(/1\.?210,00/)).toBeNull();
  });

  it('crea un presupuesto sin NIF y lo factura desde la lista', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    setDemoInvoke(async (cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'quotes_list')
        return [
          {
            id: 7,
            kind: 'presupuesto',
            number: 'P2026-0007',
            issue_date: '2026-10-20',
            valid_until: '2026-11-19',
            status: 'aceptado',
            recipient_name: 'Prospecto SL',
            total: 121_000,
            draft,
            language: null,
            invoice_id: null,
            invoice_number: null,
            expired: false,
          },
        ];
      if (cmd === 'quote_create') return { number: 'P2026-0008' };
      if (cmd === 'quote_invoice_draft') return { draft, language: null };
      if (cmd === 'contacts_search') return [];
      return null;
    });
    render(<Quotes settings={settings} />);
    expect(await screen.findByText(/presupuesto aceptado por facturar/)).toBeTruthy();

    // Nuevo presupuesto para un cliente sin NIF todavía.
    fireEvent.click(screen.getByRole('button', { name: 'Nuevo presupuesto' }));
    fireEvent.change(screen.getByLabelText('Nombre o razón social'), {
      target: { value: 'Nuevo Cliente SL' },
    });
    fireEvent.change(screen.getByLabelText('Concepto 1'), { target: { value: 'Logo' } });
    fireEvent.change(screen.getByLabelText('Precio 1'), { target: { value: '500' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar presupuesto' }));
    await vi.waitFor(() => expect(calls.some((c) => c.cmd === 'quote_create')).toBe(true));
    const created = calls.find((c) => c.cmd === 'quote_create');
    if (!created) throw new Error('sin quote_create');
    const input = (
      created.args as {
        input: {
          kind: string;
          valid_until: string | null;
          draft: { recipient: { name: string; id: unknown } };
        };
      }
    ).input;
    expect(input.kind).toBe('presupuesto');
    expect(input.valid_until).not.toBeNull();
    expect(input.draft.recipient.name).toBe('Nuevo Cliente SL');
    expect(input.draft.recipient.id).toEqual({ Nif: '' });

    // «Facturar» abre la factura con sus datos.
    fireEvent.click(await screen.findByRole('button', { name: 'Facturar' }));
    expect(await screen.findByText(/Factura de P2026-0007/)).toBeTruthy();
    expect((screen.getByLabelText('Concepto 1') as HTMLInputElement).value).toBe('Web');
  });

  it('la factura del presupuesto se emite con él (una sola orden) y sólo se borran borradores', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    const quote = (id: number, status: string) => ({
      id,
      kind: 'presupuesto',
      number: `P2026-000${id}`,
      issue_date: '2026-10-20',
      valid_until: null,
      status,
      recipient_name: 'Prospecto SL',
      total: 121_000,
      draft,
      language: null,
      invoice_id: null,
      invoice_number: null,
      expired: false,
    });
    setDemoInvoke(async (cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'quotes_list') return [quote(1, 'borrador'), quote(2, 'enviado')];
      if (cmd === 'quote_invoice_draft') return { draft, language: null };
      if (cmd === 'invoice_preview')
        return {
          totals: {
            base: 100_000,
            vat: 21_000,
            surcharge: 0,
            withholding: 0,
            tax_total: 21_000,
            invoice_total: 121_000,
            payable: 121_000,
          },
          opened: false,
        };
      if (cmd === 'quote_issue_invoice') return { id: 9, number: 'A2026-0009', export_error: null };
      if (cmd === 'contacts_search') return [];
      return null;
    });
    render(<Quotes settings={settings} />);
    await screen.findByText('P2026-0001');
    // Borrar: sólo el borrador (el enviado conserva su número).
    expect(screen.getAllByRole('button', { name: 'Borrar' })).toHaveLength(1);

    fireEvent.click(screen.getAllByRole('button', { name: 'Facturar' })[1] as HTMLElement);
    await screen.findByText(/Factura de P2026-0002/);
    fireEvent.change(screen.getByLabelText('NIF del cliente (vacío = simplificada)'), {
      target: { value: 'B12345674' },
    });
    fireEvent.click(screen.getByText('Emitir factura…'));
    fireEvent.click(await screen.findByText('Sí, emitir'));
    expect(await screen.findByText('Factura emitida desde P2026-0002.')).toBeTruthy();
    const issued = calls.filter((c) => c.cmd === 'quote_issue_invoice');
    expect(issued).toHaveLength(1);
    expect(issued[0]?.args).toMatchObject({ id: 2, confirmed: true });
    expect(calls.some((c) => c.cmd === 'invoice_issue' || c.cmd === 'quote_mark_invoiced')).toBe(
      false,
    );
  });
});

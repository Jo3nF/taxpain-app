import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { type InvoicingSettings, setDemoInvoke } from './api';
import { Invoices, NewInvoice } from './Invoices';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const settings: InvoicingSettings = {
  issuer: { id: { Nif: '12345678Z' }, name: 'Carmen', address: null, person: null },
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

describe('Duplicar', () => {
  it('saca la copia en el idioma en que se emitió la factura', async () => {
    const calls: string[] = [];
    setDemoInvoke(async (cmd, args) => {
      calls.push(`${cmd}:${JSON.stringify(args ?? {})}`);
      if (cmd === 'invoices_list')
        return [
          {
            id: 4,
            number: 'A2026-0004',
            issue_date: '2026-10-01',
            kind: 'F1',
            recipient_name: 'Acme Ltd',
            base: 10000,
            vat: 0,
            total: 10000,
            payable: 10000,
            status: 'emitida',
            verifactu_status: null,
            exported_path: null,
            has_facturae: false,
            has_ubl: false,
            origin: 'app',
          },
        ];
      if (cmd === 'verifactu_issues' || cmd === 'list_certificates' || cmd === 'recurring_list')
        return [];
      if (cmd === 'invoice_draft_of')
        return {
          kind: 'Completa',
          series: null,
          issue_date: '2026-10-01',
          operation_date: null,
          recipient: {
            id: { Foreign: { country: 'GB', id_type: 'NifIva', id: 'GB123456789' } },
            name: 'Acme Ltd',
            address: null,
            person: null,
          },
          lines: [
            {
              description: 'Maintenance',
              quantity_milli: 1000,
              unit_price: 10000,
              discount: 0,
              vat: { NotSubject: 'N2' },
              surcharge: null,
              withholding: null,
            },
          ],
          rectified: [],
          rectified_amounts: null,
          description: 'Maintenance',
          notes: null,
        };
      if (cmd === 'invoice_language_of') return 'En';
      throw { message: `sin datos: ${cmd}` };
    });
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Más acciones de A2026-0004' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Duplicar' }));
    const lang = (await screen.findByLabelText('Idioma de la factura')) as HTMLSelectElement;
    expect(lang.value).toBe('En');
    expect(calls).toContain('invoice_language_of:{"id":4}');
  });
});

describe('Rectificar', () => {
  it('trae el NIF y la dirección del cliente de la factura rectificada', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'invoices_list')
        return [
          {
            id: 7,
            number: 'A2026-0007',
            issue_date: '2026-10-02',
            kind: 'F1',
            recipient_name: 'EMPRESA DEMO SL',
            base: 10000,
            vat: 2100,
            total: 12100,
            payable: 12100,
            status: 'emitida',
            verifactu_status: null,
            exported_path: null,
            has_facturae: false,
            has_ubl: false,
            origin: 'app',
          },
        ];
      if (cmd === 'verifactu_issues' || cmd === 'list_certificates' || cmd === 'recurring_list')
        return [];
      if (cmd === 'dashboard_get') return { pending: { verifactu_unsent: 0 } };
      if (cmd === 'invoice_draft_of')
        return {
          kind: 'Completa',
          series: null,
          issue_date: '2026-10-02',
          operation_date: null,
          recipient: {
            id: { Nif: 'B12345674' },
            name: 'EMPRESA DEMO SL',
            address: {
              street: 'Calle Mayor 1',
              post_code: '28001',
              town: 'Madrid',
              province: 'Madrid',
              country: 'ES',
            },
            person: null,
          },
          lines: [],
          rectified: [],
          rectified_amounts: null,
          description: 'Consultoría',
          notes: null,
        };
      if (cmd === 'invoice_language_of') return null;
      throw { message: `sin datos: ${cmd}` };
    });
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Rectificar' }));
    expect(await screen.findByDisplayValue('B12345674')).toBeTruthy();
    expect(screen.getByDisplayValue('EMPRESA DEMO SL')).toBeTruthy();
    expect(screen.getByDisplayValue('Calle Mayor 1')).toBeTruthy();
    expect(screen.getByDisplayValue('28001')).toBeTruthy();
    expect(screen.getByText(/Rectificativa de/)).toBeTruthy();
  });
});

describe('Retención', () => {
  it('explica por qué no se puede marcar con «Sin retención»', () => {
    render(<NewInvoice settings={settings} onIssued={() => {}} onCancel={() => {}} />);
    const box = screen.getByLabelText('Retención IRPF 1') as HTMLInputElement;
    expect(box.disabled).toBe(true);
    expect(box.getAttribute('aria-description')).toMatch(/Ajustes → Facturación/);
  });
});

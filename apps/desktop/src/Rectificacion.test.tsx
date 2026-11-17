import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CreditoIncobrable, InvoiceDraft, InvoicingSettings, IssuedSummary } from './api';

const api = vi.hoisted(() => ({
  invoiceNumbering: vi.fn(),
  invoicePreview: vi.fn(),
  invoiceIssue: vi.fn(),
  incobrablesList: vi.fn(),
  incobrableOpen: vi.fn(),
  incobrableClaim: vi.fn(),
  incobrableIssue: vi.fn(),
  incobrableClose: vi.fn(),
  incobrableCommunication: vi.fn(),
  incobrableCommunicated: vi.fn(),
  contactsSearch: vi.fn(() => Promise.resolve([])),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));

const { NewInvoice, toDraft } = await import('./Invoices');
const { NumberingCheck } = await import('./Rectificacion');
const { Incobrables } = await import('./Incobrables');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const settings: InvoicingSettings = {
  issuer: { id: { Nif: 'B12345674' }, name: 'Estudio Demo SL', address: null, person: null },
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
} as InvoicingSettings;

const issued = (id: number, number: string): IssuedSummary =>
  ({
    id,
    number,
    issue_date: '2026-10-01',
    kind: 'F1',
    recipient_name: 'Cliente Inventado SA',
    base: 100_000,
    vat: 21_000,
    total: 121_000,
    payable: 121_000,
    status: 'emitida',
    verifactu_status: null,
    exported_path: null,
    has_facturae: false,
    has_ubl: false,
  }) as IssuedSummary;

const original: InvoiceDraft = {
  kind: 'Completa',
  series: null,
  issue_date: '2026-10-01',
  operation_date: null,
  recipient: {
    id: { Nif: 'A22222228' },
    name: 'Cliente Inventado SA',
    address: null,
    person: null,
  },
  lines: [
    {
      description: 'Diseño de la web',
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
  description: 'Diseño de la web',
  notes: null,
};

describe('rectificativa con motivo y tipo', () => {
  it('toDraft lleva el motivo, la sustitución y las facturas agrupadas', () => {
    const d = toDraft(
      {
        clientName: 'Cliente Inventado SA',
        clientNif: 'A22222228',
        street: '',
        postCode: '',
        town: '',
        province: '',
        date: '2026-10-13',
        description: 'Corrección',
        lines: [{ description: 'Diseño', quantity: '1', price: '800', vat: 0, withholding: false }],
      },
      settings,
      issued(1, 'A2026-0001'),
      false,
      false,
      { code: 4, method: 'Sustitucion', also: [issued(2, 'A2026-0002')] },
    );
    if (typeof d === 'string') throw new Error(d);
    expect(d.kind).toEqual({ Rectificativa: { code: 4, method: 'Sustitucion' } });
    expect(d.rectified).toEqual([
      ['A2026-0001', '2026-10-01'],
      ['A2026-0002', '2026-10-01'],
    ]);
    // Lo sustituido lo calcula TaxPain con las facturas guardadas.
    expect(d.rectified_amounts).toBeNull();
  });

  it('por sustitución parte de las líneas de la factura y emite con su motivo', async () => {
    api.invoicePreview.mockResolvedValue({
      totals: {
        base: 80_000,
        vat: 16_800,
        surcharge: 0,
        withholding: 0,
        tax_total: 16_800,
        invoice_total: 96_800,
        payable: 96_800,
      },
      eur_totals: null,
    });
    api.invoiceIssue.mockResolvedValue({ id: 9, number: 'R2026-0001', export_error: null });
    render(
      <NewInvoice
        settings={settings}
        rectifies={issued(1, 'A2026-0001')}
        rectifiesDraft={original}
        rectifyOthers={[issued(2, 'A2026-0002')]}
        onIssued={() => {}}
        onCancel={() => {}}
      />,
    );
    expect(screen.getByText(/Rectificativa de A2026-0001/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Motivo'), { target: { value: '4' } });
    expect(screen.getByText(/Cualquier otra corrección/)).toBeTruthy();
    fireEvent.click(screen.getByLabelText('Por sustitución'));
    expect((screen.getByLabelText('Concepto 1') as HTMLInputElement).value).toBe(
      'Diseño de la web',
    );
    expect(screen.getByText(/sustituye a A2026-0001/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Precio 1'), { target: { value: '800' } });
    fireEvent.click(screen.getByText('Emitir factura…'));
    expect(await screen.findByText(/se contabiliza y se declara la diferencia/)).toBeTruthy();
    fireEvent.click(screen.getByText('Sí, emitir'));
    await vi.waitFor(() => expect(api.invoiceIssue).toHaveBeenCalled());
    const d = api.invoiceIssue.mock.calls[0]?.[0] as InvoiceDraft;
    expect(d.kind).toEqual({ Rectificativa: { code: 4, method: 'Sustitucion' } });
    expect(d.lines[0]?.unit_price).toBe(80_000);
  });

  it('la R2 no admite sustitución y la R3 se manda a Cobros y pagos', () => {
    render(
      <NewInvoice
        settings={settings}
        rectifies={issued(1, 'A2026-0001')}
        rectifiesDraft={original}
        onIssued={() => {}}
        onCancel={() => {}}
      />,
    );
    expect(screen.getByText(/Créditos incobrables/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Motivo'), { target: { value: '2' } });
    expect((screen.getByLabelText('Por sustitución') as HTMLInputElement).disabled).toBe(true);
  });
});

describe('control de numeración', () => {
  it('enseña los huecos de una serie', async () => {
    api.invoiceNumbering.mockResolvedValue([
      {
        serie: 'A',
        clase: 'facturas completas',
        year: 2026,
        emitidas: 3,
        anuladas: 0,
        primera: 'A2026-0001',
        ultima: 'A2026-0005',
        huecos: [3, 4],
        fuera_de_orden: [],
        otra_clase: [],
      },
    ]);
    render(<NumberingCheck year={2026} />);
    const details = screen.getByText('Control de numeración 2026').closest('details');
    if (!details) throw new Error('sin details');
    details.open = true;
    fireEvent(details, new Event('toggle'));
    expect(await screen.findByText('Faltan los números 3, 4.')).toBeTruthy();
    expect(api.invoiceNumbering).toHaveBeenCalledWith(2026);
  });
});

const candidata: CreditoIncobrable = {
  id: null,
  invoice_id: 3,
  number: 'A2026-0003',
  cliente: 'Cliente Inventado SA',
  nif: 'A22222228',
  issue_date: '2026-02-10',
  devengo: '2026-02-10',
  total: 121_000,
  pendiente: 121_000,
  base_pendiente: 100_000,
  iva_pendiente: 21_000,
  plazo_meses: 6,
  estado: 'candidata',
  exclusion: 'ninguna',
  reclamacion_fecha: null,
  reclamacion_medio: null,
  reclamacion_ref: null,
  rectificativa: null,
  rectificativa_fecha: null,
  comunicada_el: null,
  comunicacion_ref: null,
  notas: '',
  calendario: {
    inicio: '2026-02-10',
    desde: '2026-08-10',
    limite_rectificar: '2027-02-10',
    limite_comunicar: null,
  },
  requisitos: [{ clave: 'reclamacion', cumplido: false, texto: 'Falta reclamar el cobro.' }],
  puede_rectificar: false,
  avisos: [],
  deterioro: null,
};

describe('créditos incobrables', () => {
  it('abre el seguimiento con confirmación', async () => {
    api.incobrablesList.mockResolvedValue([candidata]);
    api.incobrableOpen.mockResolvedValue(1);
    render(<Incobrables />);
    expect(await screen.findByText(/A2026-0003 · Cliente Inventado SA/)).toBeTruthy();
    expect(screen.getByText(/Falta reclamar el cobro/)).toBeTruthy();
    fireEvent.click(screen.getByText('Seguir como incobrable…'));
    fireEvent.click(await screen.findByText('Sí, seguirla'));
    await vi.waitFor(() => expect(api.incobrableOpen).toHaveBeenCalledWith(3, 6, 'ninguna', true));
  });

  it('sin los requisitos no deja emitir la R3', async () => {
    api.incobrablesList.mockResolvedValue([{ ...candidata, id: 1, estado: 'abierto' }]);
    render(<Incobrables />);
    const b = (await screen.findByText('Emitir la rectificativa R3…')) as HTMLButtonElement;
    expect(b.closest('button')?.disabled).toBe(true);
  });

  it('con la R3 emitida, prepara la comunicación', async () => {
    api.incobrablesList.mockResolvedValue([
      {
        ...candidata,
        id: 1,
        estado: 'rectificado',
        rectificativa: 'R2026-0002',
        rectificativa_fecha: '2026-11-10',
      },
    ]);
    api.incobrableCommunication.mockResolvedValue({
      campos: [['Factura original', 'A2026-0003']],
      adjuntos: ['Copia de la factura rectificativa R2026-0002.'],
      plazo: '2026-12-10',
      donde: 'Sede electrónica de la AEAT. TaxPain no la envía.',
    });
    render(<Incobrables />);
    fireEvent.click(await screen.findByText('Datos para comunicarla a la AEAT'));
    expect(await screen.findByText(/Plazo: hasta el/)).toBeTruthy();
    expect(screen.getByText('Copia de la factura rectificativa R2026-0002.')).toBeTruthy();
  });
});

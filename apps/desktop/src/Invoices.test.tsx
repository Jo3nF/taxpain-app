import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { InvoicingSettings } from './api';
import { dueIn, euros, kindLabel, parseEuros, parseQuantity, plural, sendStateLabel } from './api';

const settings: InvoicingSettings = {
  issuer: { id: { Nif: '12345678Z' }, name: 'Carmen', address: null, person: null },
  default_series: 'A',
  rectifying_series: 'R',
  simplified_series: 'S',
  language: 'Es',
  default_vat: 2100,
  default_withholding: 1500,
  outputs: { pdf: true, facturae: false },
  destination: null,
  rounding: 'PerRate',
  verifactu: { enabled: true, installation_id: 'TP-x', production: false },
  footer: null,
};

const totals = {
  base: 10000,
  vat: 2100,
  surcharge: 0,
  withholding: 1500,
  tax_total: 2100,
  invoice_total: 12100,
  payable: 10600,
};

const api = vi.hoisted(() => ({
  invoicesList: vi.fn(),
  listCertificates: vi.fn(),
  invoicePreview: vi.fn(),
  invoiceIssue: vi.fn(),
  invoiceOpen: vi.fn(),
  invoiceSave: vi.fn(),
  invoiceCancel: vi.fn(),
  verifactuSend: vi.fn(),
  verifactuIssues: vi.fn(),
  verifactuResubmit: vi.fn(),
  dashboardGet: vi.fn(),
  fxBceRate: vi.fn(),
  viesCheck: vi.fn(),
  quoteIssueInvoice: vi.fn(),
  contactsSearch: vi.fn(),
  // Por defecto, como una factura importada sin borrador: el formulario se abre vacío.
  invoiceDraftOf: vi.fn(
    (_id: number): Promise<unknown> => Promise.reject(new Error('sin borrador')),
  ),
  invoiceLanguageOf: vi.fn((_id: number): Promise<unknown> => Promise.resolve(null)),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));

const { Invoices, NewInvoice, fitVat, fromDraft, toDraft, withClient } = await import('./Invoices');

describe('importes', () => {
  it('formatea y analiza euros', () => {
    expect(euros(123456)).toBe('1.234,56 €');
    expect(euros(-5)).toBe('-0,05 €');
    expect(parseEuros('1.234,56')).toBe(123456);
    expect(parseEuros('80')).toBe(8000);
    expect(parseEuros('12.5')).toBe(1250);
    expect(parseEuros('-0,05')).toBe(-5);
    expect(parseEuros('abc')).toBeNull();
    expect(parseEuros('1,234')).toBeNull();
  });

  it('acepta miles con punto y decimales con coma o con punto', () => {
    expect(parseEuros('12.000')).toBe(1200000);
    expect(parseEuros('1.234.567')).toBe(123456700);
    expect(parseEuros('1.234.567,8')).toBe(123456780);
    expect(parseEuros('1234,56')).toBe(123456);
    expect(parseEuros('1234.56')).toBe(123456);
    expect(parseEuros('1.50')).toBe(150);
    expect(parseEuros('1.5')).toBe(150);
    expect(parseEuros(' 1.234,56 € ')).toBe(123456);
    expect(parseEuros('-12.000')).toBe(-1200000);
    expect(parseEuros('-0')).toBe(0);
    // Un 0 delante no es de miles: «0.125» no son 125 € (con tres decimales, no vale).
    expect(parseEuros('0.125')).toBeNull();
    expect(parseEuros('-0.050')).toBeNull();
    expect(parseEuros('0.50')).toBe(50);
    expect(parseQuantity('0.250')).toBe(250);
    expect(parseQuantity('1.000')).toBe(1000000);
  });

  it('rechaza importes ambiguos o mal escritos', () => {
    expect(parseEuros('')).toBeNull();
    expect(parseEuros('-')).toBeNull();
    expect(parseEuros('1234.567')).toBeNull();
    expect(parseEuros('1.234.56')).toBeNull();
    expect(parseEuros('12.00.000')).toBeNull();
    expect(parseEuros('1,2,3')).toBeNull();
    expect(parseEuros('1.234,567')).toBeNull();
    expect(parseEuros('12,')).toBeNull();
  });
});

describe('textos', () => {
  it('traduce los códigos del registro VERI*FACTU', () => {
    expect(kindLabel('F1')).toBe('Completa');
    expect(kindLabel('F2')).toBe('Simplificada');
    expect(kindLabel('F3')).toBe('Sustitutiva de simplificadas');
    expect(kindLabel('R1')).toBe('Rectificativa (R1)');
    expect(kindLabel('R5')).toBe('Rectificativa (R5)');
    expect(sendStateLabel('Correcto')).toBe('aceptado');
    expect(sendStateLabel('ParcialmenteCorrecto')).toBe('aceptado con algún error');
    expect(sendStateLabel('Incorrecto')).toBe('rechazado');
  });

  it('concuerda el plural', () => {
    expect(plural(1, 'fichero')).toBe('1 fichero');
    expect(plural(0, 'fichero')).toBe('0 ficheros');
    expect(plural(3, 'registro VERI*FACTU', 'registros VERI*FACTU')).toBe('3 registros VERI*FACTU');
  });

  it('dice cuándo vence un plazo', () => {
    expect(dueIn(0)).toBe('vence hoy');
    expect(dueIn(1)).toBe('vence mañana');
    expect(dueIn(5)).toBe('faltan 5 días');
    expect(dueIn(-1)).toBe('plazo terminado');
  });
});

describe('cantidades', () => {
  it('admite miles con punto y hasta tres decimales; lo ambiguo no pasa', () => {
    expect(parseQuantity('1.000')).toBe(1_000_000);
    expect(parseQuantity('1,5')).toBe(1500);
    expect(parseQuantity('2.5')).toBe(2500);
    expect(parseQuantity('0,250')).toBe(250);
    expect(parseQuantity('1.000,5')).toBe(1_000_500);
    expect(parseQuantity('-3')).toBe(-3000);
    expect(parseQuantity('1.234.5')).toBeNull();
    expect(parseQuantity('0x10')).toBeNull();
    expect(parseQuantity('1e3')).toBeNull();
    expect(parseQuantity('1,2345')).toBeNull();
    expect(parseQuantity('')).toBeNull();
  });
});

describe('toDraft', () => {
  const form = {
    clientName: 'Cliente SL',
    clientNif: 'b12345674',
    street: 'Calle 1',
    postCode: '46001',
    town: 'València',
    province: 'Valencia',
    date: '2026-10-13',
    description: '',
    lines: [
      { description: 'Consultoría', quantity: '2,5', price: '80,00', vat: 0, withholding: true },
    ],
  };

  it('genera un borrador en céntimos y milésimas', () => {
    const d = toDraft(form, settings);
    if (typeof d === 'string') throw new Error(d);
    expect(d.kind).toBe('Completa');
    expect(d.recipient?.id).toEqual({ Nif: 'B12345674' });
    expect(d.lines[0]).toMatchObject({
      quantity_milli: 2500,
      unit_price: 8000,
      vat: { Taxed: { rate: 2100 } },
      withholding: 1500,
    });
    expect(d.description).toBe('Consultoría');
  });

  it('«1.000» unidades son mil, como en el precio (no una)', () => {
    const d = toDraft(
      {
        ...form,
        lines: [
          { description: 'Km', quantity: '1.000', price: '0,26', vat: 0, withholding: false },
        ],
      },
      settings,
    );
    if (typeof d === 'string') throw new Error(d);
    expect(d.lines[0]?.quantity_milli).toBe(1_000_000);
    expect(
      toDraft(
        {
          ...form,
          lines: [
            { description: 'Km', quantity: '1.234.5', price: '1', vat: 0, withholding: false },
          ],
        },
        settings,
      ),
    ).toBe('Revisa la línea 1');
  });

  it('cliente de la UE con NIF-IVA y de fuera con su identificador', () => {
    const fr = toDraft(
      {
        ...form,
        country: 'FR',
        clientNif: 'fr 12345678901',
        operationDate: '2026-09-30',
        lines: form.lines.map((l) => ({ ...l, vat: 4, withholding: false })),
      },
      settings,
    );
    if (typeof fr === 'string') throw new Error(fr);
    expect(fr.recipient?.id).toEqual({
      Foreign: { country: 'FR', id_type: 'NifIva', id: 'FR12345678901' },
    });
    expect(fr.recipient?.address?.country).toBe('FR');
    expect(fr.operation_date).toBe('2026-09-30');
    expect(fr.lines[0]?.vat).toEqual({ NotSubject: 'N2' });
    const us = toDraft({ ...form, country: 'US', clientNif: '12-3456789' }, settings);
    if (typeof us === 'string') throw new Error(us);
    expect(us.recipient?.id).toEqual({
      Foreign: { country: 'US', id_type: 'DocumentoOficial', id: '123456789' },
    });
    // De fuera, sin identificador no hay factura (no puede ser simplificada).
    expect(toDraft({ ...form, country: 'DE', clientNif: '' }, settings)).toContain('NIF-IVA');
  });

  it('duplicar: del borrador al formulario y vuelta', () => {
    const d = toDraft({ ...form, country: 'FR', clientNif: 'FR12345678901' }, settings);
    if (typeof d === 'string') throw new Error(d);
    const back = fromDraft(d, 0);
    expect(back.country).toBe('FR');
    expect(back.clientNif).toBe('FR12345678901');
    expect(back.lines[0]).toMatchObject({ quantity: '2,5', price: '80,00', vat: 0 });
    const again = toDraft(back, settings);
    if (typeof again === 'string') throw new Error(again);
    expect(again.lines).toEqual(d.lines);
  });

  it('sin NIF es simplificada y una línea mala da error', () => {
    const d = toDraft({ ...form, clientName: '', clientNif: '' }, settings);
    expect(typeof d !== 'string' && d.kind).toBe('Simplificada');
    expect(typeof d !== 'string' && d.recipient).toBeNull();
    expect(
      toDraft({ ...form, lines: form.lines.map((l) => ({ ...l, price: 'x' })) }, settings),
    ).toBe('Revisa la línea 1');
  });

  it('un nombre sin NIF no se convierte en una simplificada sin cliente', () => {
    const e = toDraft({ ...form, clientNif: '' }, settings);
    expect(e).toContain('Pon el NIF del cliente o borra su nombre');
    // La factura de un presupuesto exige el NIF aunque se borre el nombre.
    expect(
      toDraft({ ...form, clientName: '', clientNif: '' }, settings, undefined, false, true),
    ).toContain('presupuesto');
    const ok = toDraft(form, settings, undefined, false, true);
    expect(typeof ok !== 'string' && ok.kind).toBe('Completa');
  });

  it('un particular de la UE: su documento, IVA español y, sin documento, simplificada', () => {
    const base = { ...form, country: 'FR', consumer: true, clientNif: 'AB 123456' };
    const d = toDraft(base, settings);
    if (typeof d === 'string') throw new Error(d);
    expect(d.kind).toBe('Completa');
    expect(d.recipient?.id).toEqual({
      Foreign: { country: 'FR', id_type: 'DocumentoOficial', id: 'AB123456' },
    });
    expect(d.lines[0]?.vat).toEqual({ Taxed: { rate: 2100 } });
    const anon = toDraft({ ...base, clientName: '', clientNif: '' }, settings);
    expect(typeof anon !== 'string' && anon.kind).toBe('Simplificada');
    expect(toDraft({ ...base, clientNif: '' }, settings)).toContain('documento de identidad');
    // Una empresa de la UE sin NIF-IVA, no.
    expect(toDraft({ ...base, consumer: false, clientNif: '' }, settings)).toContain('NIF-IVA');
  });

  it('un particular de la UE con el IVA de su país (ventanilla única)', () => {
    const base = {
      ...form,
      country: 'FR',
      consumer: true,
      clientNif: 'AB 123456',
      oss: { rate: '20', supply: 'Servicios' as const },
    };
    const d = toDraft(base, settings);
    if (typeof d === 'string') throw new Error(d);
    // Sin IVA español ni retención en las líneas; el IVA francés va aparte.
    expect(d.lines[0]?.vat).toEqual({ NotSubject: 'N2' });
    expect(d.lines[0]?.withholding).toBeNull();
    expect(d.oss).toEqual({ rate: 2000, supply: 'Servicios' });
    expect(toDraft({ ...base, oss: { rate: 'x', supply: 'Servicios' } }, settings)).toContain(
      'tipo de IVA del país',
    );
    // Vuelve al formulario con el tipo, y al pasar a empresa o a otro país se quita.
    const back = fromDraft(d, 0);
    expect(back.oss).toEqual({ rate: '20', supply: 'Servicios' });
    expect(withClient(back, 'FR', false, 0).oss).toBeUndefined();
    expect(withClient(back, 'DE', true, 0).oss).toBeUndefined();
    expect(withClient(back, 'FR', true, 0).oss).toEqual(back.oss);
    // A una empresa (sin «particular»), no lleva IVA de destino aunque quede en el formulario.
    const empresa = toDraft({ ...base, consumer: false, clientNif: 'FR12345678901' }, settings);
    expect(typeof empresa !== 'string' && empresa.oss).toBeFalsy();
  });

  it('el IVA de las líneas se adapta al cliente', () => {
    // Opciones: 0 21 %, 4 servicio UE, 5 servicio fuera UE, 6 bienes UE, 7 exportación, 8 ISP.
    for (const v of [4, 5, 6, 7]) expect(fitVat(v, 'ES', false, 0)).toBe(0);
    expect(fitVat(8, 'ES', false, 0)).toBe(8);
    expect(fitVat(0, 'FR', false, 0)).toBe(4);
    expect(fitVat(7, 'FR', false, 0)).toBe(6);
    expect(fitVat(8, 'FR', false, 0)).toBe(4);
    expect(fitVat(6, 'US', false, 0)).toBe(7);
    expect(fitVat(0, 'US', false, 0)).toBe(5);
    // Irlanda del Norte: bienes UE sí; un servicio es de fuera de la UE.
    expect(fitVat(6, 'XI', false, 0)).toBe(6);
    expect(fitVat(0, 'XI', false, 0)).toBe(5);
    // Particular de la UE: IVA español.
    for (const v of [4, 6, 7, 8]) expect(fitVat(v, 'DE', true, 0)).toBe(0);
    const f = withClient(
      {
        ...form,
        lines: [
          { description: 'Bienes', quantity: '1', price: '10', vat: 7, withholding: true },
          { description: 'Servicio', quantity: '1', price: '10', vat: 0, withholding: true },
        ],
      },
      'FR',
      false,
      0,
    );
    expect(f.lines.map((l) => [l.vat, l.withholding])).toEqual([
      [6, false],
      [4, false],
    ]);
    expect(f.language).toBe('En');
    expect(withClient(f, 'FR', true, 0).lines.map((l) => l.vat)).toEqual([0, 0]);
    expect(withClient(f, 'ES', true, 0).consumer).toBe(false);
  });

  it('editar un presupuesto conserva su fecha', () => {
    const d = toDraft(form, settings);
    if (typeof d === 'string') throw new Error(d);
    expect(fromDraft(d, 0, true).date).toBe('2026-10-13');
    expect(fromDraft(d, 0).date).not.toBe('2026-10-13');
    // Un particular de la UE se reconoce al volver al formulario.
    const p = toDraft({ ...form, country: 'IT', consumer: true, clientNif: 'X1' }, settings);
    if (typeof p === 'string') throw new Error(p);
    expect(fromDraft(p, 0).consumer).toBe(true);
  });
});

describe('NewInvoice', () => {
  const quoteDraft = {
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
  beforeEach(() => {
    api.invoicePreview.mockResolvedValue({ totals, opened: false });
    api.contactsSearch.mockResolvedValue([]);
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('la factura de un presupuesto pide el NIF y se emite con el presupuesto', async () => {
    const onIssued = vi.fn();
    api.quoteIssueInvoice.mockRejectedValueOnce({ message: 'P2026-0007 ya está facturado' });
    render(
      <NewInvoice
        settings={settings}
        initial={quoteDraft}
        fromQuote={7}
        onIssued={onIssued}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByText('Emitir factura…'));
    expect((await screen.findByRole('alert')).textContent).toContain('Pon el NIF del cliente');
    expect(api.invoicePreview).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText('NIF del cliente (vacío = simplificada)'), {
      target: { value: 'B12345674' },
    });
    fireEvent.click(screen.getByText('Emitir factura…'));
    await screen.findByRole('dialog', { name: 'Confirmar emisión' });
    fireEvent.click(screen.getByText('Sí, emitir'));
    // Si falla (ya facturado), se ve el error y no se cierra.
    expect((await screen.findByRole('alert')).textContent).toContain('ya está facturado');
    expect(onIssued).not.toHaveBeenCalled();
    expect(api.invoiceIssue).not.toHaveBeenCalled();
    const [id, draft, confirmed] = api.quoteIssueInvoice.mock.calls[0] ?? [];
    expect(id).toBe(7);
    expect(confirmed).toBe(true);
    expect(draft).toMatchObject({ kind: 'Completa', recipient: { id: { Nif: 'B12345674' } } });
  });

  it('la comprobación de VIES se borra al cambiar el NIF y no pisa lo escrito después', async () => {
    let answer: (r: unknown) => void = () => {};
    api.viesCheck.mockImplementation(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );
    render(<NewInvoice settings={settings} onIssued={() => {}} onCancel={() => {}} />);
    fireEvent.change(screen.getByLabelText('País del cliente'), { target: { value: 'FR' } });
    const nif = screen.getByLabelText('NIF-IVA del cliente');
    fireEvent.change(nif, { target: { value: 'FR12345678901' } });
    fireEvent.click(screen.getByRole('button', { name: 'Comprobar el NIF-IVA en VIES' }));
    expect(api.viesCheck).toHaveBeenCalledWith('FR', 'FR12345678901', null, 'cliente');
    // Mientras VIES contesta, se cambia el NIF: la respuesta ya no vale.
    fireEvent.change(nif, { target: { value: 'FR99999999999' } });
    answer({ valid: true, name: 'ACME SAS', request_id: 'W1' });
    await vi.waitFor(() =>
      expect(screen.getByRole('button', { name: 'Comprobar el NIF-IVA en VIES' })).toBeTruthy(),
    );
    expect(screen.queryByText(/Válido en VIES/)).toBeNull();
    expect((screen.getByLabelText('Nombre o razón social') as HTMLInputElement).value).toBe('');

    // Una comprobación buena se borra al cambiar el NIF.
    api.viesCheck.mockResolvedValueOnce({ valid: true, name: 'OTRA SAS', request_id: 'W2' });
    fireEvent.click(screen.getByRole('button', { name: 'Comprobar el NIF-IVA en VIES' }));
    expect(await screen.findByText(/Válido en VIES/)).toBeTruthy();
    expect((screen.getByLabelText('Nombre o razón social') as HTMLInputElement).value).toBe(
      'OTRA SAS',
    );
    fireEvent.change(nif, { target: { value: 'FR11111111111' } });
    expect(screen.queryByText(/Válido en VIES/)).toBeNull();
  });

  it('el código postal rellena la provincia (en España)', () => {
    render(<NewInvoice settings={settings} onIssued={() => {}} onCancel={() => {}} />);
    const province = screen.getByLabelText('Provincia') as HTMLInputElement;
    expect(province.getAttribute('list')).toBeTruthy();
    fireEvent.change(screen.getByLabelText('CP'), { target: { value: '46001' } });
    expect(province.value).toBe('Valencia/València');
    fireEvent.change(province, { target: { value: 'València' } });
    fireEvent.change(screen.getByLabelText('CP'), { target: { value: '28013' } });
    expect(province.value).toBe('València');
    // Fuera de España, ni sugerencias ni relleno.
    fireEvent.change(screen.getByLabelText('País del cliente'), { target: { value: 'FR' } });
    const region = screen.getByLabelText('Región') as HTMLInputElement;
    expect(region.getAttribute('list')).toBeNull();
  });

  it('un particular de la UE no ve VIES ni la inversión del sujeto pasivo', () => {
    render(<NewInvoice settings={settings} onIssued={() => {}} onCancel={() => {}} />);
    fireEvent.change(screen.getByLabelText('País del cliente'), { target: { value: 'DE' } });
    expect((screen.getByLabelText('IVA 1') as HTMLSelectElement).value).toBe('4');
    fireEvent.click(screen.getByLabelText(/Es un particular/));
    expect((screen.getByLabelText('IVA 1') as HTMLSelectElement).value).toBe('0');
    expect(screen.queryByRole('button', { name: 'Comprobar el NIF-IVA en VIES' })).toBeNull();
    expect(screen.getByLabelText('Documento de identidad (vacío = simplificada)')).toBeTruthy();
    expect(screen.getByText(/lleva IVA español/)).toBeTruthy();
  });
});

describe('Invoices', () => {
  beforeEach(() => {
    api.dashboardGet.mockRejectedValue({ message: 'sin datos' });
    api.verifactuIssues.mockResolvedValue([]);
    api.invoicesList.mockResolvedValue([]);
    api.listCertificates.mockResolvedValue([]);
    api.invoicePreview.mockResolvedValue({ totals, opened: false });
    api.invoiceIssue.mockResolvedValue({
      id: 1,
      number: 'A2026-0001',
      totals,
      verifactu_huella: 'H',
      qr_url: null,
      exported_to: null,
      export_error: null,
    });
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('pide confirmación antes de emitir', async () => {
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByText('Nueva factura'));
    fireEvent.change(screen.getByLabelText('Concepto 1'), { target: { value: 'Consultoría' } });
    fireEvent.change(screen.getByLabelText('Precio 1'), { target: { value: '100' } });
    fireEvent.click(screen.getByText('Emitir factura…'));
    const dialog = await screen.findByRole('dialog', { name: 'Confirmar emisión' });
    expect(dialog.textContent).toContain('106,00 €');
    expect(dialog.textContent).toContain('VERI*FACTU');
    expect(api.invoiceIssue).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText('Sí, emitir'));
    await vi.waitFor(() => expect(api.invoiceIssue).toHaveBeenCalledTimes(1));
    expect(api.invoiceIssue.mock.calls[0]?.[1]).toBe(true);
  });

  it('editar el formulario tras pedir confirmación la retira (no se emite lo no confirmado)', async () => {
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByText('Nueva factura'));
    fireEvent.change(screen.getByLabelText('Concepto 1'), { target: { value: 'X' } });
    fireEvent.change(screen.getByLabelText('Precio 1'), { target: { value: '100' } });
    fireEvent.click(screen.getByText('Emitir factura…'));
    await screen.findByRole('dialog', { name: 'Confirmar emisión' });
    fireEvent.change(screen.getByLabelText('Precio 1'), { target: { value: '900' } });
    expect(screen.queryByRole('dialog', { name: 'Confirmar emisión' })).toBeNull();
    expect(api.invoiceIssue).not.toHaveBeenCalled();
  });

  it('quien ya emitía simplificadas en la serie ordinaria ve el cambio de numeración', async () => {
    const f2 = {
      id: 3,
      number: 'A2026-0003',
      issue_date: '2026-10-01',
      kind: 'F2',
      recipient_name: null,
      base: 10000,
      vat: 2100,
      total: 12100,
      payable: 12100,
      status: 'emitida' as const,
      verifactu_status: null,
      exported_path: null,
      has_facturae: false,
      has_ubl: false,
    };
    api.invoicesList.mockResolvedValue([f2]);
    render(<Invoices settings={settings} />);
    await screen.findAllByText(/A2026-0003/);
    fireEvent.click(screen.getByText('Nueva factura'));
    fireEvent.change(screen.getByLabelText('Concepto 1'), { target: { value: 'Ticket' } });
    fireEvent.change(screen.getByLabelText('Precio 1'), { target: { value: '100' } });
    fireEvent.click(screen.getByText('Emitir factura…'));
    const dialog = await screen.findByRole('dialog', { name: 'Confirmar emisión' });
    expect(dialog.textContent).toContain('siguiente número de la serie S');
    expect(dialog.textContent).toContain('Cambio de numeración');
    cleanup();

    // Con una simplificada ya en la serie S, sin aviso.
    api.invoicesList.mockResolvedValue([f2, { ...f2, id: 4, number: 'S2026-0001' }]);
    render(<Invoices settings={settings} />);
    await screen.findAllByText(/S2026-0001/);
    fireEvent.click(screen.getByText('Nueva factura'));
    fireEvent.change(screen.getByLabelText('Concepto 1'), { target: { value: 'Ticket' } });
    fireEvent.change(screen.getByLabelText('Precio 1'), { target: { value: '100' } });
    fireEvent.click(screen.getByText('Emitir factura…'));
    const again = await screen.findByRole('dialog', { name: 'Confirmar emisión' });
    expect(again.textContent).not.toContain('Cambio de numeración');
  });

  it('anular pide confirmación en la propia interfaz', async () => {
    api.invoicesList.mockResolvedValue([
      {
        id: 3,
        number: 'A2026-0003',
        issue_date: '2026-10-01',
        kind: 'F1',
        recipient_name: 'Cliente SL',
        base: 10000,
        vat: 2100,
        total: 12100,
        payable: 12100,
        status: 'emitida',
        verifactu_status: null,
        exported_path: null,
        has_facturae: false,
        has_ubl: true,
      },
    ]);
    api.invoiceCancel.mockResolvedValue(undefined);
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Anular' }));
    // El tipo del registro (F1) se lee en palabras.
    expect(screen.getByText(/01\/10\/2026 · Completa/)).toBeTruthy();
    expect(api.invoiceCancel).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Sí, anular A2026-0003' }));
    await vi.waitFor(() => expect(api.invoiceCancel).toHaveBeenCalledWith(3, true));
  });

  it('si anular falla, el error se queda en la confirmación (no parece anulada)', async () => {
    api.invoicesList.mockResolvedValue([
      {
        id: 3,
        number: 'A2026-0003',
        issue_date: '2026-10-01',
        kind: 'F1',
        recipient_name: 'Cliente SL',
        base: 10000,
        vat: 2100,
        total: 12100,
        payable: 12100,
        status: 'emitida',
        verifactu_status: null,
        exported_path: null,
        has_facturae: false,
        has_ubl: false,
      },
    ]);
    api.invoiceCancel.mockRejectedValue({ message: 'la AEAT no responde' });
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Anular' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, anular A2026-0003' }));
    const dialog = await screen.findByRole('alertdialog');
    expect((await within(dialog).findByRole('alert')).textContent).toContain('la AEAT no responde');
    expect(screen.getByRole('button', { name: 'Sí, anular A2026-0003' })).toBeTruthy();
  });

  it('guarda el UBL desde la lista', async () => {
    api.invoicesList.mockResolvedValue([
      {
        id: 4,
        number: 'A2026-0004',
        issue_date: '2026-10-01',
        kind: 'F1',
        recipient_name: 'Cliente SL',
        base: 10000,
        vat: 2100,
        total: 12100,
        payable: 12100,
        status: 'emitida',
        verifactu_status: null,
        exported_path: null,
        has_facturae: false,
        has_ubl: true,
      },
    ]);
    api.invoiceSave.mockResolvedValue(true);
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Guardar A2026-0004 como' }));
    expect(screen.queryByRole('menuitem', { name: 'Facturae (XML)' })).toBeNull();
    fireEvent.click(screen.getByRole('menuitem', { name: 'UBL (XML)' }));
    await vi.waitFor(() => expect(api.invoiceSave).toHaveBeenCalledWith(4, 'A2026-0004', 'ubl'));
    expect(await screen.findByText('A2026-0004 guardada.')).toBeTruthy();
  });

  it('avisa de los registros rechazados y genera la subsanación tras confirmar', async () => {
    api.verifactuIssues.mockResolvedValue([
      {
        id: 9,
        invoice_id: 3,
        num_serie: 'A2026-0003',
        record_type: 'alta',
        send_status: 'incorrecto',
        error_code: '1100',
        error_text: 'Valor no permitido',
      },
    ]);
    api.verifactuResubmit.mockResolvedValue(10);
    render(<Invoices settings={settings} />);
    expect(await screen.findByText(/A2026-0003 · alta · rechazado/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Subsanar…' }));
    expect(api.verifactuResubmit).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Sí, generar la subsanación' }));
    await vi.waitFor(() => expect(api.verifactuResubmit).toHaveBeenCalledWith(9));
  });

  it('la fecha por defecto es la local', async () => {
    const { today } = await import('./Invoices');
    expect(today(new Date(2027, 0, 1, 0, 30))).toBe('2027-01-01');
  });

  it('la fecha no admite el futuro ni una operación posterior a la factura', async () => {
    const { today } = await import('./Invoices');
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByText('Nueva factura'));
    const fecha = screen.getByLabelText('Fecha') as HTMLInputElement;
    expect(fecha.max).toBe(today());
    fireEvent.change(fecha, { target: { value: '2026-10-05' } });
    expect((screen.getByLabelText('Fecha de la operación') as HTMLInputElement).max).toBe(
      '2026-10-05',
    );
  });

  it('la rectificativa se puede cerrar y cancelar', async () => {
    api.invoicesList.mockResolvedValue([
      {
        id: 5,
        number: 'A2026-0005',
        issue_date: '2026-10-01',
        kind: 'F1',
        recipient_name: 'Cliente SL',
        base: 10000,
        vat: 2100,
        total: 12100,
        payable: 12100,
        status: 'emitida',
        verifactu_status: null,
        exported_path: null,
        has_facturae: false,
        has_ubl: false,
      },
    ]);
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Rectificar' }));
    expect(await screen.findByText(/Rectificativa de/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Cerrar' }));
    expect(screen.queryByText(/Rectificativa de/)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Rectificar' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Cancelar' }));
    expect(screen.queryByText(/Rectificativa de/)).toBeNull();
    expect(screen.getByRole('button', { name: 'Nueva factura' })).toBeTruthy();
  });

  it('la rectificativa parte del IVA y la retención de la factura que rectifica', async () => {
    api.invoicesList.mockResolvedValue([
      {
        id: 6,
        number: 'A2026-0006',
        issue_date: '2026-10-01',
        kind: 'F1',
        recipient_name: 'Client SARL',
        base: 10000,
        vat: 0,
        total: 10000,
        payable: 10000,
        status: 'emitida',
        verifactu_status: null,
        exported_path: null,
        has_facturae: false,
        has_ubl: false,
      },
    ]);
    api.invoiceDraftOf.mockResolvedValueOnce({
      kind: 'Completa',
      series: null,
      issue_date: '2026-10-01',
      operation_date: null,
      recipient: {
        id: { Foreign: { country: 'FR', id_type: 'NifIva', id: 'FR12345678901' } },
        name: 'Client SARL',
        address: null,
        person: null,
      },
      lines: [
        {
          description: 'Servicio',
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
      description: 'Servicio',
      notes: null,
    });
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Rectificar' }));
    await screen.findByText(/Rectificativa de/);
    // Ni IVA español ni IRPF a una empresa francesa: lo de la factura que rectifica.
    expect((screen.getByLabelText('Retención IRPF 1') as HTMLInputElement).checked).toBe(false);
    const iva = screen.getByLabelText('IVA 1') as HTMLSelectElement;
    expect(iva.selectedOptions[0]?.textContent).not.toMatch(/21/);
  });

  it('tras emitir, el aviso de la exportación queda a la vista', async () => {
    api.invoiceIssue.mockResolvedValue({
      id: 1,
      number: 'A2026-0001',
      totals,
      verifactu_huella: 'H',
      qr_url: null,
      exported_to: null,
      export_error: 'no se pudo copiar a la carpeta',
    });
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByText('Nueva factura'));
    fireEvent.change(screen.getByLabelText('Concepto 1'), { target: { value: 'X' } });
    fireEvent.change(screen.getByLabelText('Precio 1'), { target: { value: '100' } });
    fireEvent.click(screen.getByText('Emitir factura…'));
    fireEvent.click(await screen.findByText('Sí, emitir'));
    expect(
      await screen.findByText('Emitida A2026-0001, con avisos: no se pudo copiar a la carpeta'),
    ).toBeTruthy();
    expect(screen.queryByLabelText('Concepto 1')).toBeNull();
  });

  it('cuenta todos los registros pendientes, no sólo los del año que se ve', async () => {
    api.dashboardGet.mockResolvedValue({ pending: { verifactu_unsent: 3 } });
    render(<Invoices settings={settings} />);
    expect(await screen.findByText(/3 registros VERI\*FACTU pendientes/)).toBeTruthy();
  });

  it('cancelar la confirmación no emite', async () => {
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByText('Nueva factura'));
    fireEvent.change(screen.getByLabelText('Concepto 1'), { target: { value: 'X' } });
    fireEvent.change(screen.getByLabelText('Precio 1'), { target: { value: '10' } });
    fireEvent.click(screen.getByText('Emitir factura…'));
    const dialog = await screen.findByRole('dialog', { name: 'Confirmar emisión' });
    fireEvent.click(within(dialog).getByText('Cancelar'));
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(api.invoiceIssue).not.toHaveBeenCalled();
  });
});

describe('rectificativa', () => {
  it('toDraft genera R1 por diferencias con la factura rectificada', () => {
    const issued = {
      id: 7,
      number: 'A2026-0007',
      issue_date: '2026-10-01',
      kind: 'F1',
      recipient_name: 'Cliente SL',
      base: 10000,
      vat: 2100,
      total: 12100,
      payable: 12100,
      status: 'emitida' as const,
      verifactu_status: null,
      exported_path: null,
      has_facturae: false,
      has_ubl: false,
    };
    const d = toDraft(
      {
        clientName: 'Cliente SL',
        clientNif: 'B12345674',
        street: '',
        postCode: '',
        town: '',
        province: '',
        date: '2026-10-13',
        description: 'Descuento',
        lines: [
          { description: 'Descuento', quantity: '1', price: '-10', vat: 0, withholding: false },
        ],
      },
      settings,
      issued,
    );
    if (typeof d === 'string') throw new Error(d);
    expect(d.kind).toEqual({ Rectificativa: { code: 1, method: 'Diferencias' } });
    expect(d.rectified).toEqual([['A2026-0007', '2026-10-01']]);
    expect(d.lines[0]?.unit_price).toBe(-1000);
  });

  it('la rectificativa de una simplificada (R5) no admite cliente', () => {
    // Validaciones VERI*FACTU v1.2.2, 3.1.3.13: con F2 o R5, Destinatarios no puede ir.
    const simplified = {
      id: 8,
      number: 'A2026-0008',
      issue_date: '2026-10-02',
      kind: 'F2',
      recipient_name: null,
      base: 2000,
      vat: 420,
      total: 2420,
      payable: 2420,
      status: 'emitida' as const,
      verifactu_status: null,
      exported_path: null,
      has_facturae: false,
      has_ubl: false,
    };
    const form = {
      clientName: '',
      clientNif: '',
      street: '',
      postCode: '',
      town: '',
      province: '',
      date: '2026-10-13',
      description: 'Abono',
      lines: [{ description: 'Abono', quantity: '1', price: '-5', vat: 0, withholding: false }],
    };
    const withClient = toDraft(
      { ...form, clientName: 'Cliente SL', clientNif: 'B12345674' },
      settings,
      simplified,
    );
    expect(withClient).toMatch(/R5.*no lleva los datos del cliente/);
    const d = toDraft(form, settings, simplified);
    if (typeof d === 'string') throw new Error(d);
    expect(d.kind).toEqual({ Rectificativa: { code: 5, method: 'Diferencias' } });
    expect(d.recipient).toBeNull();
  });
});

describe('moneda extranjera', () => {
  const form = {
    clientName: 'Client Inc.',
    clientNif: '12-3456789',
    country: 'US',
    street: '',
    postCode: '',
    town: '',
    province: '',
    date: '2026-10-24',
    description: '',
    lines: [
      { description: 'Desarrollo', quantity: '10', price: '120', vat: 5, withholding: false },
    ],
  };
  beforeEach(() => {
    api.dashboardGet.mockRejectedValue({ message: 'sin datos' });
    api.verifactuIssues.mockResolvedValue([]);
    api.invoicesList.mockResolvedValue([]);
    api.listCertificates.mockResolvedValue([]);
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('toDraft lleva la moneda y el tipo; sin tipo no hay factura', () => {
    expect(toDraft({ ...form, currency: 'USD' }, settings)).toContain('tipo de cambio');
    const d = toDraft(
      {
        ...form,
        currency: 'USD',
        fx: { rate: '1,1193', rateDate: '2026-10-23', source: 'bce' },
      },
      settings,
    );
    if (typeof d === 'string') throw new Error(d);
    expect(d.currency).toEqual({
      code: 'USD',
      rate: '1.1193',
      rate_date: '2026-10-23',
      source: 'bce',
    });
    expect(d.lines[0]?.unit_price).toBe(12000);
    // Ida y vuelta (Duplicar, presupuesto → factura).
    const back = fromDraft(d, 0);
    expect(back.currency).toBe('USD');
    expect(back.fx).toEqual({ rate: '1,1193', rateDate: '2026-10-23', source: 'bce' });
    // En euros no lleva moneda.
    const eur = toDraft({ ...form, currency: 'EUR' }, settings);
    expect(typeof eur !== 'string' && 'currency' in eur).toBe(false);
  });

  it('el tipo del BCE, el equivalente en euros y la confirmación en las dos monedas', async () => {
    api.fxBceRate.mockResolvedValue({
      currency: 'USD',
      requested: '2026-10-24',
      date: '2026-10-23',
      rate: '1.1193',
      cached: false,
    });
    const usdTotals = { ...totals, withholding: 0, tax_total: 0, vat: 0, base: 120000 };
    api.invoicePreview.mockResolvedValue({
      totals: { ...usdTotals, invoice_total: 120000, payable: 120000 },
      eur_totals: { ...usdTotals, base: 107210, invoice_total: 107210, payable: 107210 },
      opened: false,
    });
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByText('Nueva factura'));
    fireEvent.change(screen.getByLabelText('País del cliente'), { target: { value: 'US' } });
    fireEvent.change(screen.getByLabelText('Identificador fiscal'), {
      target: { value: '12-3456789' },
    });
    fireEvent.change(screen.getByLabelText('Nombre o razón social'), {
      target: { value: 'Client Inc.' },
    });
    fireEvent.change(screen.getByLabelText('Moneda'), { target: { value: 'USD' } });
    fireEvent.change(screen.getByLabelText('Concepto 1'), { target: { value: 'Desarrollo' } });
    fireEvent.change(screen.getByLabelText('Precio 1'), { target: { value: '1200' } });
    fireEvent.click(screen.getByText('Tipo del BCE'));
    await vi.waitFor(() => expect(api.fxBceRate).toHaveBeenCalledTimes(1));
    expect(api.fxBceRate.mock.calls[0]?.[0]).toBe('USD');
    expect(await screen.findByDisplayValue('1,1193')).toBeTruthy();
    // Mientras se escribe: 1.200,00 USD ≈ 1.072,10 €.
    expect(screen.getByText(/Total aproximado: 1\.200,00 USD ≈ 1\.072,10 €/)).toBeTruthy();
    fireEvent.click(screen.getByText('Emitir factura…'));
    const dialog = await screen.findByRole('dialog', { name: 'Confirmar emisión' });
    expect(dialog.textContent).toContain('1.200,00 USD');
    expect(dialog.textContent).toContain('1.072,10 €');
    const draft = api.invoicePreview.mock.calls[0]?.[0];
    expect(draft.currency).toMatchObject({ code: 'USD', rate: '1.1193', rate_date: '2026-10-23' });
    // Otra moneda: el tipo del dólar (y su «BCE») no se queda para las libras.
    fireEvent.change(screen.getByLabelText('Moneda'), { target: { value: 'GBP' } });
    expect(screen.queryByDisplayValue('1,1193')).toBeNull();
    expect((screen.getByLabelText('Tipo (1 € = … GBP)') as HTMLInputElement).value).toBe('');
  });

  it('la rectificativa de una factura en dólares parte de su moneda y su tipo', async () => {
    api.invoicesList.mockResolvedValue([
      {
        id: 7,
        number: 'A2026-0007',
        issue_date: '2026-10-01',
        kind: 'F1',
        recipient_name: 'Client Inc.',
        base: 90909,
        vat: 0,
        total: 90909,
        payable: 90909,
        status: 'emitida',
        verifactu_status: null,
        exported_path: null,
        has_facturae: false,
        has_ubl: false,
        currency: 'USD',
        currency_payable: 100000,
        currency_rate: { code: 'USD', rate: '1.1', rate_date: '2026-10-01', source: 'bce' },
      },
    ]);
    render(<Invoices settings={settings} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Rectificar' }));
    expect(((await screen.findByLabelText('Moneda')) as HTMLSelectElement).value).toBe('USD');
    expect(screen.getByDisplayValue('1,1')).toBeTruthy();
    expect(screen.getByDisplayValue('2026-10-01')).toBeTruthy();
    // Un tipo de hace semanas es el de la original: no se empuja al del día.
    expect(screen.queryByText(/usa el vigente a la fecha de la operación/)).toBeNull();
    expect(screen.getByText(/al tipo de la factura que se rectifica/)).toBeTruthy();
  });
});

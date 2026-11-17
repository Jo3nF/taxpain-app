import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { money, parseRate, rateDateWarning, rateDecimal, rateText, toEur } from './api';

const api = vi.hoisted(() => ({
  receivedList: vi.fn(),
  watchedFolders: vi.fn(),
  setWatchedFolders: vi.fn(),
  receivedReview: vi.fn(),
  // La comprobación previa a confirmar: aquí, un resumen sin avisos.
  receivedCheck: vi.fn(async (_id: number, r: { total: number }) => ({
    correccion: false,
    despues: {
      proveedor: '',
      proveedor_nif: '',
      numero: '',
      fecha: '2026-10-01',
      base: 0,
      iva: 0,
      retencion: 0,
      total: r.total,
      deducible_pct: 100,
      regimen: 'interior',
      moneda: null,
      total_moneda: null,
    },
    antes: null,
    trimestre: '4T 2026',
    avisos: [],
  })),
  fxBceRate: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { euroVat, receivedInEuros, toForeignCurrency } = await import('./CurrencyRate');
const { Received } = await import('./Received');

describe('tipo de cambio', () => {
  it('lee y escribe tipos sin coma flotante', () => {
    expect(parseRate('1,1193')).toBe(1_119_300);
    expect(parseRate('1.1193')).toBe(1_119_300);
    expect(parseRate('0.83728')).toBe(837_280);
    expect(parseRate('20.268,06')).toBe(20_268_060_000);
    for (const bad of ['', '0', 'abc', '1,1234567', '-1', '1,2,3']) {
      expect(parseRate(bad), bad).toBeNull();
    }
    expect(rateDecimal(1_119_300)).toBe('1.1193');
    expect(rateDecimal(7_000_000)).toBe('7');
    expect(rateText(837_280)).toBe('0,83728');
  });

  it('pasa a euros como el proceso nativo (mitad alejándose de cero)', () => {
    expect(toEur(100_000, 1_119_300)).toBe(89_342); // 1.000,00 USD
    expect(toEur(-100_000, 1_119_300)).toBe(-89_342);
    expect(toEur(5, 2_000_000)).toBe(3); // 0,025 → 0,03
    expect(money(144_998, 'USD')).toBe('1.449,98 USD');
    expect(money(144_998, null)).toBe('1.449,98 €');
  });

  it('avisa si el tipo no es el vigente al devengo', () => {
    expect(rateDateWarning('2026-10-23', '2026-10-24')).toBeNull();
    expect(rateDateWarning('2026-10-10', '2026-10-24')).toContain('10/10/2026');
    expect(rateDateWarning('2026-10-25', '2026-10-24')).toContain('posterior');
    // Rectificativa al tipo de la original: el antiguo vale; el posterior, no.
    expect(rateDateWarning('2026-10-01', '2026-10-24', true)).toBeNull();
    expect(rateDateWarning('2026-10-25', '2026-10-24', true)).toContain('posterior');
  });

  it('el IVA en euros es el de la base en euros por el tipo (art. 12 RD 1619/2012)', () => {
    // 100,02 USD + 21,00 USD a 1,1193: base 89,36 € × 21 % = 18,77 € (21,00 / 1,1193 = 18,76).
    expect(euroVat(10_002, 2_100, 1_119_300)).toBe(1_877);
    expect(toEur(2_100, 1_119_300)).toBe(1_876);
    // Una cuota que no es la de un tipo español: la convertida.
    expect(euroVat(10_000, 1_234, 1_119_300)).toBe(toEur(1_234, 1_119_300));
    expect(euroVat(10_000, 0, 1_119_300)).toBe(0);
    const a = { base: 10_002, vat: 2_100, withholding: 0, total: 12_102 };
    expect(receivedInEuros(a, 1_119_300)).toEqual({
      base: 8_936,
      vat: 1_877,
      withholding: 0,
      total: 10_813,
    });
    // El que dice la factura, si se escribe.
    expect(receivedInEuros(a, 1_119_300, 1_876)?.vat).toBe(1_876);
  });

  it('formulario → moneda y recibidas en euros', () => {
    expect(
      toForeignCurrency('USD', { rate: '', rateDate: '2026-10-23', source: 'manual' }),
    ).toContain('tipo de cambio');
    expect(toForeignCurrency('USD', { rate: '1,12', rateDate: '', source: 'manual' })).toContain(
      'fecha',
    );
    expect(
      toForeignCurrency('USD', { rate: '1,12', rateDate: '2026-10-23', source: 'manual' }),
    ).toEqual({ code: 'USD', rate: '1.12', rate_date: '2026-10-23', source: 'manual' });
    expect(
      receivedInEuros({ base: 10_000, vat: 0, withholding: 0, total: 10_000 }, 1_119_300),
    ).toEqual({ base: 8_934, vat: 0, withholding: 0, total: 8_934 });
    expect(receivedInEuros({ base: 10_000, vat: 0, withholding: 0, total: 9_000 }, 1_119_300)).toBe(
      null,
    );
  });
});

describe('recibida en dólares', () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('se revisa en la moneda y se envía en euros con la moneda aparte', async () => {
    const field = (value: unknown) => ({ value, confidence: 'Alta' });
    api.watchedFolders.mockResolvedValue([]);
    api.receivedReview.mockResolvedValue(undefined);
    api.fxBceRate.mockResolvedValue({
      currency: 'USD',
      requested: '2026-10-23',
      date: '2026-10-23',
      rate: '1.1193',
      cached: true,
    });
    api.receivedList.mockResolvedValue([
      {
        id: 9,
        filename: 'saas.pdf',
        status: 'pendiente',
        supplier_nif: null,
        supplier_name: 'SaaS Tools Inc.',
        number: 'INV-1',
        issue_date: '2026-10-23',
        total: 10_000,
        extraction: {
          source: 'texto',
          supplier_nif: null,
          supplier_name: field('SaaS Tools Inc.'),
          number: field('INV-1'),
          issue_date: field('2026-10-23'),
          base: field(10_000),
          vat: field(0),
          withholding: null,
          total: field(10_000),
          warnings: [],
        },
      },
    ]);
    render(<Received />);
    fireEvent.click(await screen.findByText('SaaS Tools Inc.'));
    fireEvent.change(screen.getByLabelText('Moneda'), { target: { value: 'USD' } });
    expect(screen.getByLabelText('Total (USD)')).toBeTruthy();
    fireEvent.click(screen.getByText('Tipo del BCE'));
    await vi.waitFor(() => expect(api.fxBceRate).toHaveBeenCalledWith('USD', '2026-10-23'));
    expect(await screen.findByText(/base 89,34 €/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Confirmar datos' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Sí, confirmar' }));
    await vi.waitFor(() => expect(api.receivedReview).toHaveBeenCalled());
    const [, reviewed, fx] = api.receivedReview.mock.calls[0] ?? [];
    expect(reviewed).toMatchObject({ base: 8_934, vat: 0, total: 8_934 });
    expect(fx).toMatchObject({
      currency: { code: 'USD', rate: '1.1193', rate_date: '2026-10-23', source: 'bce' },
      base: 10_000,
      total: 10_000,
    });
  });

  it('una Facturae en dólares llega con la moneda, el tipo y el IVA en euros', async () => {
    const field = (value: unknown) => ({ value, confidence: 'Alta' });
    api.watchedFolders.mockResolvedValue([]);
    api.receivedReview.mockResolvedValue(undefined);
    api.receivedList.mockResolvedValue([
      {
        id: 10,
        filename: 'factura.xml',
        status: 'pendiente',
        supplier_nif: 'B98765431',
        supplier_name: 'Proveedor Ejemplo SL',
        number: 'F-1',
        issue_date: '2026-10-23',
        total: 12_102,
        extraction: {
          source: 'facturae',
          supplier_nif: field('B98765431'),
          supplier_name: field('Proveedor Ejemplo SL'),
          number: field('F-1'),
          issue_date: field('2026-10-23'),
          base: field(10_002),
          vat: field(2_100),
          withholding: null,
          total: field(12_102),
          warnings: ['la factura está en USD'],
          fx: { code: 'USD', rate: '1.1193', rate_date: '2026-10-22', vat_eur: 1_876 },
        },
      },
    ]);
    render(<Received />);
    fireEvent.click(await screen.findByText('Proveedor Ejemplo SL'));
    expect((screen.getByLabelText('Moneda') as HTMLSelectElement).value).toBe('USD');
    expect(screen.getByDisplayValue('1,1193')).toBeTruthy();
    expect(screen.getByDisplayValue('2026-10-22')).toBeTruthy();
    const vatEur = screen.getByLabelText('IVA en euros (el de la factura)') as HTMLInputElement;
    expect(vatEur.value).toBe('18,76');
    // Vacío, se propone la base en euros por el 21 %.
    fireEvent.change(vatEur, { target: { value: '' } });
    expect(vatEur.placeholder).toBe('18,77 €');
    fireEvent.change(vatEur, { target: { value: '18,76' } });
    fireEvent.click(screen.getByRole('button', { name: 'Confirmar datos' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Sí, confirmar' }));
    await vi.waitFor(() => expect(api.receivedReview).toHaveBeenCalled());
    const [, reviewed, fx] = api.receivedReview.mock.calls[0] ?? [];
    expect(reviewed).toMatchObject({ base: 8_936, vat: 1_876, total: 8_936 + 1_876 });
    expect(fx).toMatchObject({
      currency: { code: 'USD', rate: '1.1193', rate_date: '2026-10-22', source: 'manual' },
      vat_eur: 1_876,
    });
  });

  it('al cambiar de moneda no se queda el tipo de la anterior', async () => {
    const field = (value: unknown) => ({ value, confidence: 'Alta' });
    api.watchedFolders.mockResolvedValue([]);
    api.fxBceRate.mockResolvedValue({
      currency: 'USD',
      requested: '2026-10-23',
      date: '2026-10-23',
      rate: '1.1193',
      cached: true,
    });
    api.receivedList.mockResolvedValue([
      {
        id: 11,
        filename: 'saas.pdf',
        status: 'pendiente',
        supplier_nif: null,
        supplier_name: 'SaaS Tools Inc.',
        number: 'INV-2',
        issue_date: '2026-10-23',
        total: 10_000,
        extraction: {
          source: 'texto',
          supplier_nif: null,
          supplier_name: field('SaaS Tools Inc.'),
          number: field('INV-2'),
          issue_date: field('2026-10-23'),
          base: field(10_000),
          vat: field(0),
          withholding: null,
          total: field(10_000),
          warnings: [],
        },
      },
    ]);
    render(<Received />);
    fireEvent.click(await screen.findByText('SaaS Tools Inc.'));
    fireEvent.change(screen.getByLabelText('Moneda'), { target: { value: 'USD' } });
    fireEvent.click(screen.getByText('Tipo del BCE'));
    expect(await screen.findByDisplayValue('1,1193')).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Moneda'), { target: { value: 'GBP' } });
    expect(screen.queryByDisplayValue('1,1193')).toBeNull();
    expect((screen.getByLabelText('Tipo (1 € = … GBP)') as HTMLInputElement).value).toBe('');
  });
});

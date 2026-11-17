import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  receivedList: vi.fn(),
  watchedFolders: vi.fn(),
  setWatchedFolders: vi.fn(),
  receivedReview: vi.fn(),
  receivedAiExtract: vi.fn(),
  receivedAiFillPending: vi.fn(),
  receivedDiscard: vi.fn(),
  receivedUndiscard: vi.fn(),
  receivedCheck: vi.fn(),
  receivedCorrect: vi.fn(),
  receivedUnreviewCheck: vi.fn(),
  receivedUnreview: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Received } = await import('./Received');

const field = (value: unknown) => ({ value, confidence: 'Alta' });

const coche = (extra: Record<string, unknown> = {}) => ({
  id: 12,
  filename: 'coche.pdf',
  status: 'pendiente',
  supplier_nif: 'B12345674',
  supplier_name: 'Concesionario SL',
  number: 'C-7',
  issue_date: '2026-08-01',
  total: 1210000,
  extraction: {
    source: 'texto',
    supplier_nif: field('B12345674'),
    supplier_name: field('Concesionario SL'),
    number: field('C-7'),
    issue_date: field('2026-08-01'),
    base: field(1000000),
    vat: field(210000),
    withholding: null,
    total: field(1210000),
    warnings: [],
  },
  ...extra,
});

/** Lo que devuelve la comprobación de Rust para unos datos (en la prueba, sin avisos). */
function check(reviewed: Record<string, unknown>, extra: Record<string, unknown> = {}) {
  const resumen = (r: Record<string, unknown>) => ({
    proveedor_nif: r.supplier_nif,
    proveedor: r.supplier_name,
    numero: r.number,
    fecha: r.issue_date,
    base: r.base,
    iva: r.vat,
    retencion: r.withholding,
    total: r.total,
    deducible_pct: r.deductible_pct,
    regimen: r.vat_regime ?? 'interior',
    moneda: null,
    total_moneda: null,
  });
  return {
    correccion: false,
    despues: resumen(reviewed),
    antes: null,
    trimestre: '4T 2026',
    avisos: [],
    ...extra,
  };
}

/** «Confirmar datos» enseña el resumen; «Sí, confirmar» graba. */
async function confirmar() {
  fireEvent.click(screen.getByRole('button', { name: 'Confirmar datos' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Sí, confirmar' }));
}

describe('Received', () => {
  beforeEach(() => {
    api.receivedCheck.mockImplementation(async (_id: number, r: Record<string, unknown>) =>
      check(r),
    );
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('envía la naturaleza del gasto y si es un bien de inversión', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedReview.mockResolvedValue(undefined);
    api.receivedList.mockResolvedValue([
      {
        id: 7,
        filename: 'alquiler.pdf',
        status: 'pendiente',
        supplier_nif: 'B12345674',
        supplier_name: 'Inmobiliaria SL',
        number: 'A-1',
        issue_date: '2026-08-01',
        total: 102000,
        extraction: {
          source: 'texto',
          supplier_nif: field('B12345674'),
          supplier_name: field('Inmobiliaria SL'),
          number: field('A-1'),
          issue_date: field('2026-08-01'),
          base: field(100000),
          vat: field(21000),
          withholding: field(19000),
          total: field(102000),
          warnings: [],
        },
      },
    ]);
    render(<Received />);
    fireEvent.click(await screen.findByText('Inmobiliaria SL'));
    // Con retención, la sugerencia inicial es «profesional»; se cambia a alquiler.
    const select = screen.getByLabelText('Tipo de gasto') as HTMLSelectElement;
    expect(select.value).toBe('profesional');
    fireEvent.change(select, { target: { value: 'alquiler' } });
    await confirmar();
    await vi.waitFor(() => expect(api.receivedReview).toHaveBeenCalled());
    expect(api.receivedReview.mock.calls[0]?.[1]).toMatchObject({
      nature: 'alquiler',
      capital_good: false,
      asset_class: null,
      withholding: 19000,
    });
  });

  it('un bien de inversión lleva su clase de inmovilizado', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedReview.mockResolvedValue(undefined);
    api.receivedList.mockResolvedValue([
      {
        id: 8,
        filename: 'portatil.pdf',
        status: 'pendiente',
        supplier_nif: 'A22222228',
        supplier_name: 'Tienda SA',
        number: 'T-9',
        issue_date: '2026-08-01',
        total: 484000,
        extraction: {
          source: 'texto',
          supplier_nif: field('A22222228'),
          supplier_name: field('Tienda SA'),
          number: field('T-9'),
          issue_date: field('2026-08-01'),
          base: field(400000),
          vat: field(84000),
          withholding: null,
          total: field(484000),
          warnings: [],
        },
      },
    ]);
    render(<Received />);
    fireEvent.click(await screen.findByText('Tienda SA'));
    fireEvent.click(screen.getByLabelText(/Es un bien de inversión/));
    fireEvent.change(screen.getByLabelText(/Tipo de bien/), { target: { value: 'mobiliario' } });
    await confirmar();
    await vi.waitFor(() => expect(api.receivedReview).toHaveBeenCalled());
    expect(api.receivedReview.mock.calls[0]?.[1]).toMatchObject({
      capital_good: true,
      asset_class: 'mobiliario',
      nature: 'general',
    });
  });

  it('por debajo de 3.005,06 € no se admite como bien de inversión (art. 108 LIVA)', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedReview.mockResolvedValue(undefined);
    api.receivedList.mockResolvedValue([
      coche({
        id: 9,
        filename: 'raton.pdf',
        supplier_name: 'Tienda SA',
        total: 4840,
        extraction: {
          source: 'texto',
          supplier_nif: field('B12345674'),
          supplier_name: field('Tienda SA'),
          number: field('T-10'),
          issue_date: field('2026-08-01'),
          base: field(4000),
          vat: field(840),
          withholding: null,
          total: field(4840),
          warnings: [],
        },
      }),
    ]);
    render(<Received />);
    fireEvent.click(await screen.findByText('Tienda SA'));
    const box = screen.getByLabelText(/Es un bien de inversión/);
    expect(box.closest('label')?.textContent).toContain('3.005,06 €');
    fireEvent.click(box);
    fireEvent.click(screen.getByRole('button', { name: 'Confirmar datos' }));
    expect((await screen.findByRole('alert')).textContent).toContain('3.005,06 € sin IVA');
    expect(api.receivedReview).not.toHaveBeenCalled();
    // Sin la casilla, es un gasto corriente y se confirma.
    fireEvent.click(box);
    await confirmar();
    await vi.waitFor(() => expect(api.receivedReview).toHaveBeenCalled());
    expect(api.receivedReview.mock.calls[0]?.[1]).toMatchObject({ capital_good: false });
  });

  it('una compra a un proveedor de la UE se autorrepercute sin IVA en la factura', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedReview.mockResolvedValue(undefined);
    api.receivedList.mockResolvedValue([
      {
        id: 9,
        filename: 'ads.pdf',
        status: 'pendiente',
        supplier_nif: 'IE1234567T',
        supplier_name: 'Ads Ireland Ltd',
        number: 'IE-1',
        issue_date: '2026-08-01',
        total: 10000,
        extraction: {
          source: 'texto',
          supplier_nif: field('IE1234567T'),
          supplier_name: field('Ads Ireland Ltd'),
          number: field('IE-1'),
          issue_date: field('2026-08-01'),
          base: field(10000),
          vat: field(2100),
          withholding: null,
          total: field(10000),
          warnings: [],
        },
      },
    ]);
    render(<Received />);
    fireEvent.click(await screen.findByText('Ads Ireland Ltd'));
    fireEvent.change(screen.getByLabelText('IVA de la compra'), {
      target: { value: 'intracomunitaria' },
    });
    fireEvent.change(screen.getByLabelText('País (p. ej. IE)'), { target: { value: 'ie' } });
    await confirmar();
    await vi.waitFor(() => expect(api.receivedReview).toHaveBeenCalled());
    expect(api.receivedReview.mock.calls[0]?.[1]).toMatchObject({
      vat: 0,
      vat_regime: 'intracomunitaria',
      supplier_country: 'IE',
      self_assessed_rate: 2100,
      intra_goods: false,
    });
  });

  it('envía el porcentaje de IVA deducible y lo valida', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedReview.mockResolvedValue(undefined);
    api.receivedList.mockResolvedValue([coche()]);
    render(<Received />);
    fireEvent.click(await screen.findByText('Concesionario SL'));
    const pct = screen.getByLabelText('IVA deducible (%)');
    fireEvent.change(pct, { target: { value: '150' } });
    fireEvent.click(screen.getByRole('button', { name: 'Confirmar datos' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/entre 0 y 100/);
    expect(api.receivedReview).not.toHaveBeenCalled();
    fireEvent.change(pct, { target: { value: '50' } });
    await confirmar();
    await vi.waitFor(() => expect(api.receivedReview).toHaveBeenCalled());
    expect(api.receivedReview.mock.calls[0]?.[1]).toMatchObject({ deductible_pct: 50 });
  });

  it('la factura del arrendador de una cuota de leasing ya registrada se rechaza con el motivo', async () => {
    api.watchedFolders.mockResolvedValue([]);
    const motivo =
      'Esta factura es la cuota 2/48 del leasing «Furgoneta» (vence el 15/03/2026, 822,27 € con IVA), que ya está registrada desde Contabilidad → Leasing como la factura «Leasing 1 cuota 2/48» de EMPRESA DEMO SL: registrarla otra vez deduciría su IVA dos veces y duplicaría el gasto y la deuda con el arrendador. Descarta esta; si quieres guardar el PDF, adjúntalo al asiento de esa cuota en el Diario.';
    api.receivedCheck.mockRejectedValue({ message: motivo });
    api.receivedList.mockResolvedValue([coche()]);
    render(<Received />);
    fireEvent.click(await screen.findByText('Concesionario SL'));
    fireEvent.click(screen.getByRole('button', { name: 'Confirmar datos' }));
    expect((await screen.findByRole('alert')).textContent).toContain(
      'ya está registrada desde Contabilidad → Leasing',
    );
    // El formulario sigue abierto: nada se ha registrado.
    expect(screen.getByRole('button', { name: 'Confirmar datos' })).toBeTruthy();
    expect(api.receivedReview).not.toHaveBeenCalled();
  });

  it('sin fecha no llama al proceso nativo', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedList.mockResolvedValue([coche()]);
    render(<Received />);
    fireEvent.click(await screen.findByText('Concesionario SL'));
    fireEvent.change(screen.getByLabelText('Fecha'), { target: { value: '' } });
    fireEvent.click(screen.getByRole('button', { name: 'Confirmar datos' }));
    expect((await screen.findByRole('alert')).textContent).toBe('Falta la fecha de la factura');
    expect(api.receivedReview).not.toHaveBeenCalled();
  });

  it('si el asistente completa la factura abierta, el formulario toma sus datos', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedList.mockResolvedValue([
      coche({
        supplier_name: null,
        total: null,
        extraction: { ...coche().extraction, total: null },
      }),
    ]);
    api.receivedAiFillPending.mockResolvedValue({ filled: 1, failed: [] });
    render(<Received />);
    fireEvent.click(await screen.findByText('coche.pdf'));
    expect((screen.getByLabelText('Total') as HTMLInputElement).value).toBe('');
    api.receivedList.mockResolvedValue([coche()]);
    fireEvent.click(screen.getByRole('button', { name: 'Completar con el asistente' }));
    await screen.findByText(/ha completado 1 factura\./);
    await vi.waitFor(() =>
      expect((screen.getByLabelText('Total') as HTMLInputElement).value).toBe('12100,00'),
    );
  });

  it('el asistente rellena el formulario pero no confirma nada', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedReview.mockResolvedValue(undefined);
    api.receivedList.mockResolvedValue([
      {
        id: 11,
        filename: 'raro.pdf',
        status: 'pendiente',
        supplier_nif: null,
        supplier_name: null,
        number: null,
        issue_date: null,
        total: null,
        extraction: {
          source: 'texto',
          supplier_nif: null,
          supplier_name: null,
          number: null,
          issue_date: null,
          base: null,
          vat: null,
          withholding: null,
          total: null,
          warnings: [],
        },
      },
    ]);
    api.receivedAiExtract.mockResolvedValue({
      source: 'asistente',
      supplier_nif: field('B12345674'),
      supplier_name: field('Suministros Levante SL'),
      number: field('FV-118'),
      issue_date: field('2026-10-05'),
      base: field(4500),
      vat: field(945),
      withholding: null,
      total: field(5445),
      warnings: ['datos propuestos por el asistente: revísalos antes de confirmar'],
    });
    render(<Received />);
    fireEvent.click(await screen.findByText('raro.pdf'));
    fireEvent.click(screen.getByRole('button', { name: 'Rellenar con el asistente' }));
    expect(await screen.findByDisplayValue('Suministros Levante SL')).toBeTruthy();
    expect(screen.getByDisplayValue('54,45')).toBeTruthy();
    expect(api.receivedReview).not.toHaveBeenCalled();
    expect(screen.getByText(/propuestos por el asistente/)).toBeTruthy();
  });

  it('descartar pide confirmación y las descartadas se recuperan', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedDiscard.mockResolvedValue(undefined);
    api.receivedUndiscard.mockResolvedValue(undefined);
    api.receivedList.mockResolvedValue([
      coche(),
      coche({ id: 13, filename: 'repetida.pdf', supplier_name: null, status: 'descartada' }),
    ]);
    render(<Received />);
    fireEvent.click(await screen.findByText('Concesionario SL'));
    // Un clic en «Descartar» no descarta: abre la confirmación en la propia interfaz.
    fireEvent.click(screen.getByRole('button', { name: 'Descartar' }));
    expect(api.receivedDiscard).not.toHaveBeenCalled();
    expect(screen.getByRole('alertdialog').textContent).toContain('coche.pdf');
    fireEvent.click(screen.getByRole('button', { name: 'Sí, descartar' }));
    await vi.waitFor(() => expect(api.receivedDiscard).toHaveBeenCalledWith(12));
    // Las descartadas se ven y se recuperan (vuelven a pendientes).
    expect(screen.getByText('Descartadas (1)')).toBeTruthy();
    expect(screen.getByText('repetida.pdf')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Recuperar' }));
    await vi.waitFor(() => expect(api.receivedUndiscard).toHaveBeenCalledWith(13));
    expect(await screen.findByText('«repetida.pdf» vuelve a pendientes.')).toBeTruthy();
    expect(api.receivedList.mock.calls.length).toBeGreaterThanOrEqual(3);
  });
  it('«Confirmar datos» enseña el resumen con el trimestre y los avisos antes de grabar', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedReview.mockResolvedValue(undefined);
    api.receivedList.mockResolvedValue([coche()]);
    api.receivedCheck.mockImplementation(async (_id: number, r: Record<string, unknown>) =>
      check(r, {
        trimestre: '4T 2026',
        avisos: [
          'La factura es del 3T 2026 pero se registra hoy: su IVA se deduce en el 303 del 4T 2026',
        ],
      }),
    );
    render(<Received />);
    fireEvent.click(await screen.findByText('Concesionario SL'));
    fireEvent.click(screen.getByRole('button', { name: 'Confirmar datos' }));
    const panel = await screen.findByRole('alertdialog', { name: 'Confirmar los datos' });
    expect(api.receivedCheck).toHaveBeenCalledWith(
      12,
      expect.objectContaining({ base: 1000000 }),
      null,
    );
    for (const t of ['Concesionario SL (B12345674)', 'C-7', '01/08/2026', '4T 2026', 'Retención']) {
      expect(panel.textContent).toContain(t);
    }
    expect(panel.textContent).toContain('12.100,00');
    expect(panel.textContent).toContain('se registra hoy');
    // Volver no graba nada.
    fireEvent.click(screen.getByRole('button', { name: 'Volver a los datos' }));
    expect(api.receivedReview).not.toHaveBeenCalled();
    expect(screen.queryByRole('alertdialog')).toBeNull();
    await confirmar();
    await vi.waitFor(() => expect(api.receivedReview).toHaveBeenCalledTimes(1));
  });

  const revisada = () =>
    coche({
      status: 'revisada',
      reviewed: {
        supplier_nif: 'B12345674',
        supplier_name: 'Concesionario SL',
        number: 'C-7',
        issue_date: '2026-08-01',
        base: 1000000,
        vat: 210000,
        withholding: 0,
        total: 1210000,
        deductible_pct: 50,
        capital_good: false,
        nature: 'general',
        asset_class: null,
        vat_regime: 'interior',
        supplier_country: null,
        self_assessed_rate: null,
        intra_goods: false,
        cash_basis: false,
        import_dua: false,
        prorata_use: 'comun',
      },
      fx: null,
      registered_on: '2026-08-03',
    });

  it('«Corregir…» abre los datos confirmados, enseña el antes y el después y rehace', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedList.mockResolvedValue([revisada()]);
    api.receivedCorrect.mockResolvedValue({
      avisos: [],
      ejercicio_anterior: null,
      punteos_quitados: 1,
    });
    api.receivedCheck.mockImplementation(async (_id: number, r: Record<string, unknown>) => {
      const now = check(r);
      return {
        ...now,
        correccion: true,
        antes: { ...now.despues, base: 1000000, iva: 210000, total: 1210000 },
        trimestre: '3T 2026',
        avisos: [
          'El 303 del 3T de 2026 ya está presentado (o la AEAT lo tiene): TaxPain no presenta nada.',
        ],
      };
    });
    render(<Received />);
    fireEvent.click(
      await screen.findByRole('button', { name: 'Corregir la factura C-7 de Concesionario SL' }),
    );
    // Empieza con lo confirmado (también lo que no está en la extracción: el 50 % deducible).
    expect((screen.getByLabelText('Base') as HTMLInputElement).value).toBe('10000,00');
    expect((screen.getByLabelText('IVA deducible (%)') as HTMLInputElement).value).toBe('50');
    fireEvent.change(screen.getByLabelText('Base'), { target: { value: '1000,00' } });
    fireEvent.change(screen.getByLabelText('IVA'), { target: { value: '210,00' } });
    fireEvent.change(screen.getByLabelText('Total'), { target: { value: '1210,00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Revisar la corrección' }));
    const panel = await screen.findByRole('alertdialog', { name: 'Confirmar la corrección' });
    expect(panel.textContent).toContain('10.000,00');
    expect(panel.textContent).toContain('1.000,00');
    expect(panel.textContent).toContain('ya está presentado');
    expect(panel.textContent).toContain('rectificativa');
    expect(api.receivedCorrect).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Sí, corregir' }));
    await vi.waitFor(() => expect(api.receivedCorrect).toHaveBeenCalledTimes(1));
    expect(api.receivedCorrect.mock.calls[0]?.[0]).toBe(12);
    expect(api.receivedCorrect.mock.calls[0]?.[1]).toMatchObject({
      base: 100000,
      vat: 21000,
      total: 121000,
      deductible_pct: 50,
    });
    expect(api.receivedReview).not.toHaveBeenCalled();
    expect(
      await screen.findByText(/corregida y su asiento rehecho.*1 línea punteada/),
    ).toBeTruthy();
  });

  it('tras corregir una del 303 presentado ofrece preparar su rectificativa', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedList.mockResolvedValue([revisada()]);
    api.receivedCorrect.mockResolvedValue({
      avisos: ['El 303 del 3T de 2026 ya está presentado (o la AEAT lo tiene).'],
      rectificar: [{ modelo: '303', ejercicio: 2026, trimestre: 3 }],
      ejercicio_anterior: null,
      punteos_quitados: 0,
    });
    api.receivedCheck.mockImplementation(async (_id: number, r: Record<string, unknown>) => ({
      ...check(r),
      correccion: true,
      trimestre: '3T 2026',
      avisos: ['El 303 del 3T de 2026 ya está presentado (o la AEAT lo tiene).'],
      rectificar: [{ modelo: '303', ejercicio: 2026, trimestre: 3 }],
    }));
    const onRectificar = vi.fn();
    render(<Received onRectificar={onRectificar} />);
    fireEvent.click(
      await screen.findByRole('button', { name: 'Corregir la factura C-7 de Concesionario SL' }),
    );
    fireEvent.change(screen.getByLabelText('Base'), { target: { value: '1000,00' } });
    fireEvent.change(screen.getByLabelText('IVA'), { target: { value: '210,00' } });
    fireEvent.change(screen.getByLabelText('Total'), { target: { value: '1210,00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Revisar la corrección' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Sí, corregir' }));
    fireEvent.click(
      await screen.findByRole('button', {
        name: 'Preparar la rectificativa del 303 del 3T de 2026',
      }),
    );
    expect(onRectificar).toHaveBeenCalledWith({ modelo: '303', ejercicio: 2026, trimestre: 3 });
  });

  it('una corrección que no se puede (pagada) dice por qué y no graba', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedList.mockResolvedValue([revisada()]);
    api.receivedCheck.mockRejectedValue({
      message:
        'ya está pagada (conciliada con el banco): para cambiar el total, deshaz antes el pago',
    });
    render(<Received />);
    fireEvent.click(
      await screen.findByRole('button', { name: 'Corregir la factura C-7 de Concesionario SL' }),
    );
    fireEvent.change(screen.getByLabelText('Total'), { target: { value: '12000,00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Revisar la corrección' }));
    expect((await screen.findByRole('alert')).textContent).toContain('ya está pagada');
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(api.receivedCorrect).not.toHaveBeenCalled();
    // Cancelar cierra el formulario.
    fireEvent.click(screen.getByRole('button', { name: 'Cancelar' }));
    expect(screen.queryByLabelText('Base')).toBeNull();
  });

  it('deshacer la revisión pregunta con los avisos y la devuelve a pendientes', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedList.mockResolvedValue([revisada()]);
    api.receivedUnreviewCheck.mockResolvedValue([
      'Vuelve a «Pendientes de revisar» y se borra su asiento: deja de contar en el 303 del 3T 2026',
    ]);
    api.receivedUnreview.mockResolvedValue(undefined);
    render(<Received />);
    fireEvent.click(
      await screen.findByRole('button', {
        name: 'Deshacer la revisión de la factura C-7 de Concesionario SL',
      }),
    );
    const panel = await screen.findByRole('alertdialog', { name: 'Deshacer la revisión' });
    expect(panel.textContent).toContain('se borra su asiento');
    expect(api.receivedUnreview).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Sí, deshacer' }));
    await vi.waitFor(() => expect(api.receivedUnreview).toHaveBeenCalledWith(12));
    expect(await screen.findByText(/vuelve a «Pendientes de revisar»/)).toBeTruthy();
  });

  it('si no se puede deshacer la revisión, dice por qué', async () => {
    api.watchedFolders.mockResolvedValue([]);
    api.receivedList.mockResolvedValue([revisada()]);
    api.receivedUnreviewCheck.mockRejectedValue({
      message: 'está conciliada con el banco: deshaz antes la conciliación del pago',
    });
    render(<Received />);
    fireEvent.click(
      await screen.findByRole('button', {
        name: 'Deshacer la revisión de la factura C-7 de Concesionario SL',
      }),
    );
    expect((await screen.findByRole('alert')).textContent).toContain('conciliada con el banco');
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(api.receivedUnreview).not.toHaveBeenCalled();
  });
});

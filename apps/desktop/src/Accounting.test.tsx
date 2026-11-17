import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  journal: vi.fn(),
  trialBalance: vi.fn(),
  ledger: vi.fn(),
  fixedAssets: vi.fn(),
  addFixedAsset: vi.fn(),
  postDepreciation: vi.fn(),
  closeYear: vi.fn(),
  chart: vi.fn(),
  pgcAccounts: vi.fn(),
  setAccount: vi.fn(),
  manualEntry: vi.fn(),
  templatesList: vi.fn(),
  templatesDue: vi.fn(),
  templateSave: vi.fn(),
  templatePost: vi.fn(),
  templateDelete: vi.fn(),
  updateEntry: vi.fn(),
  setTick: vi.fn(),
  deleteEntry: vi.fn(),
  pickFolder: vi.fn(),
  exportListings: vi.fn(),
  fixedAssetPriors: vi.fn(),
  assetSales: vi.fn(),
  assetSaleCandidates: vi.fn(),
  assetDispose: vi.fn(),
  assetSaleLink: vi.fn(),
  assetSaleUnlink: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Accounting, filterEntries, ledgerRange } = await import('./Accounting');

describe('Accounting', () => {
  beforeEach(() => {
    api.chart.mockResolvedValue([
      {
        code: '4300001',
        name: 'Cliente Uno SL',
        source: 'tercero',
        nif: 'B12345674',
        debit: 121000,
        credit: 0,
      },
      { code: '5720002', name: 'Banco Sabadell', source: 'propia', nif: null, debit: 0, credit: 0 },
    ]);
    api.templatesList.mockResolvedValue([]);
    api.templatesDue.mockResolvedValue([]);
    api.fixedAssetPriors.mockResolvedValue([]);
    api.assetSales.mockResolvedValue([]);
    api.assetSaleCandidates.mockResolvedValue([]);
    api.pgcAccounts.mockResolvedValue([
      ['572', 'Bancos e instituciones de crédito c/c vista, euros'],
      ['629', 'Otros servicios'],
      ['705', 'Prestaciones de servicios'],
    ]);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('muestra el diario y el cuadre de sumas y saldos', async () => {
    api.journal.mockResolvedValue([
      {
        id: 1,
        number: 1,
        entry: {
          date: '2026-03-10',
          concept: 'Factura emitida A2026-0001',
          origin: { FacturaEmitida: 1 },
          lines: [
            { account: '4300001', debit: 106000, credit: 0, description: null },
            { account: '473', debit: 15000, credit: 0, description: null },
            { account: '705', debit: 0, credit: 100000, description: null },
            { account: '477', debit: 0, credit: 21000, description: null },
          ],
        },
      },
    ]);
    api.trialBalance.mockResolvedValue([
      { account: '430', debit: 106000, credit: 0 },
      { account: '705', debit: 0, credit: 106000 },
    ]);
    render(<Accounting />);
    expect(await screen.findByText(/Factura emitida A2026-0001 · Factura emitida/)).toBeTruthy();
    expect(screen.getByText('1.060,00 €')).toBeTruthy();
    fireEvent.click(screen.getByText('Sumas y saldos'));
    expect(await screen.findByText('Cuadra')).toBeTruthy();
  });

  it('no cierra sin confirmar en la propia interfaz', async () => {
    api.journal.mockResolvedValue([]);
    api.closeYear.mockResolvedValue({ result: 0 });
    // `window.confirm` no se usa: en la app es asíncrono y siempre «verdadero».
    const spy = vi.spyOn(window, 'confirm');
    render(<Accounting />);
    fireEvent.click(screen.getByText('Cierre'));
    fireEvent.click(await screen.findByRole('button', { name: /^Cerrar el ejercicio/ }));
    expect(api.closeYear).not.toHaveBeenCalled();
    // Dice que se puede reabrir y avisa de los movimientos del banco sin conciliar.
    expect(screen.getByText(/se puede reabrir con «Reabrir/)).toBeTruthy();
    expect(screen.getByText(/dejarán de salir como\s+pendientes/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Cancelar' }));
    expect(api.closeYear).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: /^Cerrar el ejercicio/ }));
    fireEvent.click(screen.getByRole('button', { name: /^Sí, cerrar/ }));
    await waitFor(() => expect(api.closeYear).toHaveBeenCalledTimes(1));
    expect(spy).not.toHaveBeenCalled();
  });

  it('cuadra contra una contrapartida, Intro pasa de campo y ⌥N abre un asiento', async () => {
    api.journal.mockResolvedValue([]);
    render(<Accounting />);
    await screen.findByRole('button', { name: 'Nuevo asiento' });
    fireEvent.keyDown(window, { key: 'n', code: 'KeyN', altKey: true });
    const cuenta1 = await screen.findByLabelText('Cuenta 1');
    fireEvent.change(cuenta1, { target: { value: '629' } });
    fireEvent.keyDown(cuenta1, { key: 'Enter' });
    expect(document.activeElement).toBe(screen.getByLabelText('Descripción 1'));
    fireEvent.change(screen.getByLabelText('Debe 1'), { target: { value: '100' } });
    fireEvent.change(screen.getByLabelText('Contrapartida'), { target: { value: '570' } });
    fireEvent.click(screen.getByRole('button', { name: 'Cuadrar contra ella' }));
    expect((screen.getByLabelText('Cuenta 2') as HTMLInputElement).value).toBe('570');
    expect((screen.getByLabelText('Haber 2') as HTMLInputElement).value).toBe('100,00');
    expect(screen.getByLabelText('Descuadre').textContent).toBe('Cuadra');
  });

  it('registra un asiento manual cuadrado desde el diario', async () => {
    api.journal.mockResolvedValue([]);
    api.manualEntry.mockResolvedValue(9);
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Nuevo asiento' }));
    fireEvent.change(screen.getByLabelText('Concepto'), { target: { value: 'Comisión' } });
    fireEvent.change(screen.getByLabelText('Cuenta 1'), { target: { value: '626' } });
    fireEvent.change(screen.getByLabelText('Debe 1'), { target: { value: '12,50' } });
    fireEvent.change(screen.getByLabelText('Cuenta 2'), { target: { value: '5720002' } });
    // El nombre de la cuenta aparece bajo el código (además de en la lista de sugerencias).
    await vi.waitFor(async () =>
      expect((await screen.findAllByText('Banco Sabadell')).some((e) => e.tagName === 'SPAN')).toBe(
        true,
      ),
    );
    expect(screen.getByLabelText('Descuadre').textContent).toContain('12,50');
    fireEvent.click(screen.getByRole('button', { name: 'Cuadrar con la línea 2' }));
    expect((screen.getByLabelText('Haber 2') as HTMLInputElement).value).toBe('12,50');
    expect(screen.getByLabelText('Descuadre').textContent).toBe('Cuadra');
    fireEvent.click(screen.getByRole('button', { name: /Registrar asiento/ }));
    await vi.waitFor(() => expect(api.manualEntry).toHaveBeenCalled());
    const entry = api.manualEntry.mock.calls[0]?.[0];
    expect(entry.concept).toBe('Comisión');
    expect(entry.origin).toBe('Manual');
    expect(entry.lines).toEqual([
      { account: '626', debit: 1250, credit: 0, description: null },
      { account: '5720002', debit: 0, credit: 1250, description: null },
    ]);
    expect(await screen.findByText('Asiento «Comisión» registrado.')).toBeTruthy();
  });

  it('no registra un asiento descuadrado', async () => {
    api.journal.mockResolvedValue([]);
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Nuevo asiento' }));
    fireEvent.change(screen.getByLabelText('Concepto'), { target: { value: 'X' } });
    fireEvent.change(screen.getByLabelText('Cuenta 1'), { target: { value: '626' } });
    fireEvent.change(screen.getByLabelText('Debe 1'), { target: { value: '10' } });
    fireEvent.change(screen.getByLabelText('Cuenta 2'), { target: { value: '572' } });
    fireEvent.change(screen.getByLabelText('Haber 2'), { target: { value: '9' } });
    fireEvent.click(screen.getByRole('button', { name: /Registrar asiento/ }));
    const err = await screen.findByText(/El asiento no cuadra/);
    // En rojo y anunciado como error, no como un aviso cualquiera.
    expect(err.getAttribute('role')).toBe('alert');
    expect(api.manualEntry).not.toHaveBeenCalled();
  });

  it('plan de cuentas: busca, da de alta y abre el mayor', async () => {
    api.ledger.mockResolvedValue([]);
    api.setAccount.mockResolvedValue(undefined);
    render(<Accounting />);
    fireEvent.click(screen.getByRole('button', { name: 'Plan de cuentas' }));
    expect(await screen.findByText('Cliente Uno SL')).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Buscar cuenta'), { target: { value: 'b1234' } });
    expect(screen.queryByText('Banco Sabadell')).toBeNull();
    fireEvent.change(screen.getByLabelText('Código'), { target: { value: '5720003' } });
    fireEvent.change(screen.getByLabelText('Nombre'), { target: { value: 'Banco BBVA' } });
    fireEvent.click(screen.getByRole('button', { name: 'Dar de alta o renombrar' }));
    await vi.waitFor(() => expect(api.setAccount).toHaveBeenCalledWith('5720003', 'Banco BBVA'));
    fireEvent.click(screen.getByRole('button', { name: '4300001' }));
    await vi.waitFor(() =>
      expect(api.ledger).toHaveBeenCalledWith(new Date().getFullYear(), '4300001'),
    );
    expect(await screen.findByText('Cliente Uno SL')).toBeTruthy();
  });

  it('periódico pendiente: se revisa, se completa y se registra con su plantilla', async () => {
    const rent = {
      id: 4,
      name: 'Alquiler oficina',
      concept: 'Alquiler nº #',
      lines: [
        { account: '621', description: '$', debit: true, amount: 100000 },
        { account: '4751', description: '', debit: false, amount: 19000 },
        { account: '572?', description: '', debit: false, amount: null },
      ],
      every_months: 1,
      next_date: '2026-03-31',
      counter: 2,
    };
    api.journal.mockResolvedValue([]);
    api.templatesDue.mockResolvedValue([rent]);
    api.templatesList.mockResolvedValue([rent]);
    api.templatePost.mockResolvedValue(11);
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Revisar y registrar' }));
    await vi.waitFor(() =>
      expect((screen.getByLabelText('Concepto') as HTMLInputElement).value).toBe('Alquiler nº 3'),
    );
    expect((screen.getByLabelText('Fecha') as HTMLInputElement).value).toBe('2026-03-31');
    expect((screen.getByLabelText('Debe 1') as HTMLInputElement).value).toBe('1000,00');
    expect((screen.getByLabelText('Cuenta 3') as HTMLInputElement).value).toBe('572');
    expect(screen.getByText(/Completa las subcuentas/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Cuenta 3'), { target: { value: '5720002' } });
    fireEvent.click(screen.getByRole('button', { name: 'Cuadrar con la línea 3' }));
    fireEvent.click(screen.getByRole('button', { name: /Registrar asiento/ }));
    await vi.waitFor(() => expect(api.templatePost).toHaveBeenCalled());
    const [id, entry] = api.templatePost.mock.calls[0] ?? [];
    expect(id).toBe(4);
    expect(entry.date).toBe('2026-03-31');
    expect(entry.lines[2]).toEqual({
      account: '5720002',
      debit: 0,
      credit: 81000,
      description: null,
    });
    expect(api.manualEntry).not.toHaveBeenCalled();
  });

  it('guarda el asiento como plantilla periódica', async () => {
    api.journal.mockResolvedValue([]);
    api.templateSave.mockResolvedValue(1);
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Nuevo asiento' }));
    fireEvent.change(screen.getByLabelText('Concepto'), { target: { value: 'Cuota autónomos' } });
    fireEvent.change(screen.getByLabelText('Cuenta 1'), { target: { value: '642' } });
    fireEvent.change(screen.getByLabelText('Debe 1'), { target: { value: '300' } });
    fireEvent.change(screen.getByLabelText('Cuenta 2'), { target: { value: '5720001' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar como plantilla…' }));
    fireEvent.change(screen.getByLabelText('Nombre de la plantilla'), {
      target: { value: 'RETA' },
    });
    fireEvent.change(screen.getByLabelText('Repetir'), { target: { value: '1' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar plantilla' }));
    await vi.waitFor(() => expect(api.templateSave).toHaveBeenCalled());
    const t = api.templateSave.mock.calls[0]?.[0];
    expect(t.name).toBe('RETA');
    expect(t.every_months).toBe(1);
    expect(t.next_date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(t.lines).toEqual([
      { account: '642', description: '', debit: true, amount: 30000 },
      { account: '5720001', description: '', debit: false, amount: null },
    ]);
  });

  it('busca, modifica y borra asientos manuales', async () => {
    const manual = {
      id: 21,
      number: 2,
      entry: {
        date: '2026-05-02',
        concept: 'Comisión banco',
        origin: 'Manual',
        lines: [
          { account: '626', debit: 1250, credit: 0, description: null },
          { account: '5720001', debit: 0, credit: 1250, description: null },
        ],
      },
    };
    const invoice = {
      id: 20,
      number: 1,
      entry: {
        date: '2026-03-10',
        concept: 'Factura A2026-0001',
        origin: { FacturaEmitida: 1 },
        lines: [
          { account: '4300001', debit: 121000, credit: 0, description: null },
          { account: '705', debit: 0, credit: 100000, description: null },
          { account: '477', debit: 0, credit: 21000, description: null },
        ],
      },
    };
    api.journal.mockResolvedValue([invoice, manual]);
    api.updateEntry.mockResolvedValue(undefined);
    api.deleteEntry.mockResolvedValue(undefined);
    render(<Accounting />);
    // Sólo los manuales se modifican o borran.
    expect(await screen.findByRole('button', { name: 'Modificar el asiento 2' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Modificar el asiento 1' })).toBeNull();
    fireEvent.change(screen.getByLabelText('Buscar en el diario'), { target: { value: '12,50' } });
    expect(screen.queryByText(/Factura A2026-0001/)).toBeNull();
    expect(screen.getByText(/Comisión banco/)).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Modificar el asiento 2' }));
    expect(screen.getByText('Modificar el asiento nº 2')).toBeTruthy();
    expect((screen.getByLabelText('Debe 1') as HTMLInputElement).value).toBe('12,50');
    fireEvent.change(screen.getByLabelText('Debe 1'), { target: { value: '15' } });
    fireEvent.change(screen.getByLabelText('Haber 2'), { target: { value: '15' } });
    fireEvent.click(screen.getByRole('button', { name: /Guardar cambios/ }));
    await vi.waitFor(() => expect(api.updateEntry).toHaveBeenCalled());
    expect(api.updateEntry.mock.calls[0]?.[0]).toBe(21);
    expect(api.updateEntry.mock.calls[0]?.[1].lines[0].debit).toBe(1500);

    fireEvent.click(await screen.findByRole('button', { name: 'Borrar' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, borrar el 2' }));
    await vi.waitFor(() => expect(api.deleteEntry).toHaveBeenCalledWith(21, true));
  });

  it('⌥N no abre un asiento mientras se escribe (en el Mac es la tecla de la «ñ»)', async () => {
    api.journal.mockResolvedValue([]);
    render(<Accounting />);
    const search = await screen.findByLabelText('Buscar en el diario');
    fireEvent.keyDown(search, { key: 'Dead', code: 'KeyN', altKey: true });
    expect(screen.queryByLabelText('Cuenta 1')).toBeNull();
    fireEvent.keyDown(window, { key: 'n', code: 'KeyN', altKey: true, metaKey: true });
    expect(screen.queryByLabelText('Cuenta 1')).toBeNull();
    fireEvent.keyDown(window, { key: 'n', code: 'KeyN', altKey: true });
    expect(await screen.findByLabelText('Cuenta 1')).toBeTruthy();
  });

  it('no graba dos veces el asiento con un doble envío', async () => {
    api.journal.mockResolvedValue([]);
    let finish: (v: number) => void = () => {};
    api.manualEntry.mockReturnValue(new Promise<number>((r) => (finish = r)));
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Nuevo asiento' }));
    fireEvent.change(screen.getByLabelText('Concepto'), { target: { value: 'Comisión' } });
    fireEvent.change(screen.getByLabelText('Cuenta 1'), { target: { value: '626' } });
    fireEvent.change(screen.getByLabelText('Debe 1'), { target: { value: '10' } });
    fireEvent.change(screen.getByLabelText('Cuenta 2'), { target: { value: '572' } });
    fireEvent.change(screen.getByLabelText('Haber 2'), { target: { value: '10' } });
    const submit = screen.getByRole('button', { name: /Registrar asiento/ });
    fireEvent.click(submit);
    fireEvent.keyDown(screen.getByLabelText('Concepto'), { key: 'Enter', metaKey: true });
    fireEvent.click(submit);
    expect(api.manualEntry).toHaveBeenCalledTimes(1);
    expect((submit as HTMLButtonElement).disabled).toBe(true);
    finish(9);
    expect(await screen.findByText('Asiento «Comisión» registrado.')).toBeTruthy();
    expect((submit as HTMLButtonElement).disabled).toBe(false);
  });

  it('un error al borrar se avisa sin ocultar el diario', async () => {
    api.journal.mockResolvedValue([
      {
        id: 21,
        number: 2,
        entry: {
          date: '2026-05-02',
          concept: 'Comisión banco',
          origin: 'Manual',
          lines: [
            { account: '626', debit: 1250, credit: 0, description: null },
            { account: '5720001', debit: 0, credit: 1250, description: null },
          ],
        },
      },
    ]);
    api.deleteEntry.mockRejectedValue({ message: 'el ejercicio está cerrado' });
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Borrar' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, borrar el 2' }));
    expect((await screen.findByRole('alert')).textContent).toBe('el ejercicio está cerrado');
    expect(screen.getAllByText('12,50 €')).toHaveLength(2);
    expect(screen.getByRole('button', { name: 'Nuevo asiento' })).toBeTruthy();
  });

  it('inmovilizado: el tipo propone su coeficiente máximo (art. 12 LIS) y avisa del exceso', async () => {
    api.journal.mockResolvedValue([]);
    api.fixedAssets.mockResolvedValue([]);
    api.addFixedAsset.mockResolvedValue(undefined);
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Inmovilizado' }));
    const rate = (await screen.findByLabelText('Coeficiente %')) as HTMLInputElement;
    expect(rate.value).toBe('25');
    // El «Tipo» del alta (el otro es el de la baja o el plan de un bien).
    const type = screen
      .getAllByLabelText('Tipo')
      .find((el) => (el as HTMLSelectElement).value === '217') as HTMLSelectElement;
    fireEvent.change(type, { target: { value: '216' } });
    expect(rate.value).toBe('10');
    fireEvent.change(screen.getByLabelText('Elemento'), { target: { value: 'Mesa' } });
    fireEvent.change(screen.getByLabelText('Coste'), { target: { value: '3.000' } });
    fireEvent.change(rate, { target: { value: '25' } });
    // Una vez tocado, cambiar el tipo no pisa el coeficiente.
    fireEvent.change(type, { target: { value: '218' } });
    expect(rate.value).toBe('25');
    fireEvent.change(type, { target: { value: '216' } });
    fireEvent.click(screen.getByRole('button', { name: 'Añadir' }));
    expect(await screen.findByText(/máximo fiscal para mobiliario es el 10 %/)).toBeTruthy();
    expect(api.addFixedAsset).not.toHaveBeenCalled();
    // Con el coeficiente fiscal dentro del máximo sí: el ajuste lo hacen el 130/200.
    fireEvent.change(screen.getByLabelText(/Coeficiente fiscal/), { target: { value: '10' } });
    fireEvent.click(screen.getByRole('button', { name: 'Añadir' }));
    await vi.waitFor(() => expect(api.addFixedAsset).toHaveBeenCalled());
    expect(api.addFixedAsset.mock.calls[0]?.[0]).toMatchObject({
      account: '216',
      rate: 2500,
      tax_rate: 1000,
      cost: 300000,
    });
    // Recuerda que la compra se contabiliza con su factura.
    expect(await screen.findByText(/La compra no se contabiliza aquí/)).toBeTruthy();
  });

  // Venta del inmovilizado con su factura (CONTABILIDAD-08).
  const envasadora = {
    id: 1,
    name: 'Envasadora',
    account: '213',
    depreciation_account: '2813',
    expense_account: '681',
    acquired_on: '2024-01-01',
    cost: 1200000,
    residual: 0,
    rate: 1000,
    disposed_on: null,
    method: 'lineal' as const,
    tax_rate: null,
  };
  const factura = {
    invoice_id: 7,
    number: 'A2026-0050',
    issue_date: '2026-06-30',
    operation_date: null,
    recipient_name: 'Cliente Uno SL',
    base: 1000000,
    open: 1000000,
    suggested: 1000000,
    rectifies: null,
    cancels_sale: false,
  };
  const vendida = {
    asset_id: 1,
    date: '2026-06-30',
    price: 1000000,
    invoiced: 0,
    unbilled: 1000000,
    result: 99507,
    invoices: [],
    pending: [],
  };

  it('inmovilizado: al vender con su factura, ésta rellena precio y fecha y va con la baja', async () => {
    api.journal.mockResolvedValue([]);
    api.fixedAssets.mockResolvedValue([envasadora]);
    api.assetSaleCandidates.mockResolvedValue([factura]);
    api.assetDispose.mockResolvedValue({
      depreciation_entry: 3,
      depreciation_removed: 60493,
      disposal_entry: 4,
      sale_entry: 5,
      result: 99507,
      warnings: [],
    });
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Inmovilizado' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Dar de baja o vender' }));
    const select = (await screen.findByLabelText(
      'Factura de venta (opcional)',
    )) as HTMLSelectElement;
    expect(select.getAttribute('aria-describedby')).toBe('venta-ayuda-1');
    expect(
      await screen.findByRole('option', {
        name: 'A2026-0050 · 30/06/2026 · Cliente Uno SL · 10.000,00 €',
      }),
    ).toBeTruthy();
    fireEvent.change(select, { target: { value: '7' } });
    const price = screen.getByLabelText(
      'Precio de venta (sin IVA; 0 si se tira)',
    ) as HTMLInputElement;
    expect(price.value).toBe('10000,00');
    // (El otro «Fecha» es el del leasing, debajo.)
    const panel = select.closest('div') as HTMLElement;
    expect((within(panel).getByLabelText('Fecha') as HTMLInputElement).value).toBe('2026-06-30');
    fireEvent.click(screen.getByRole('button', { name: 'Registrar la baja' }));
    expect(screen.getByRole('alertdialog').textContent).toContain(
      'la base de A2026-0050 pasa de ventas a la 543',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Sí, registrar' }));
    await vi.waitFor(() =>
      expect(api.assetDispose).toHaveBeenCalledWith(1, '2026-06-30', 1000000, 7),
    );
    // (El otro aviso es el del leasing, que aquí no tiene datos.)
    const status = (await screen.findByText(/^Baja registrada/)).textContent;
    expect(status).toContain('beneficio de 995,07 €');
    expect(status).toContain('La factura A2026-0050 ya no cuenta como ventas.');
    expect(status).toContain('(604,93 € menos)');
  });

  it('inmovilizado: sin factura, el precio sigue en la 543 y la factura se vincula después', async () => {
    api.journal.mockResolvedValue([]);
    api.fixedAssets.mockResolvedValue([{ ...envasadora, disposed_on: '2026-06-30' }]);
    api.assetSales.mockResolvedValue([vendida]);
    api.assetSaleCandidates.mockResolvedValue([
      { ...factura, issue_date: '2026-07-05', base: 1050000, open: 1050000 },
    ]);
    api.assetSaleLink.mockResolvedValue({
      entry_id: 9,
      amount: 1050000,
      result_change: 50000,
      warnings: [],
    });
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Inmovilizado' }));
    expect(
      await screen.findByText(/vendido sin factura vinculada: 10\.000,00 € en la 543/),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Vincular la factura de venta' }));
    const select = await screen.findByLabelText('Factura de venta');
    expect(select.getAttribute('aria-describedby')).toBe('vincular-ayuda-1');
    fireEvent.change(select, { target: { value: '7' } });
    const amount = screen.getByLabelText('Importe de este elemento (sin IVA)') as HTMLInputElement;
    expect(amount.value).toBe('10000,00');
    fireEvent.change(amount, { target: { value: '10.500' } });
    fireEvent.click(screen.getByRole('button', { name: 'Vincular' }));
    const question = screen.getByRole('alertdialog').textContent;
    expect(question).toContain(
      'El precio de la baja era 10.000,00 €: la diferencia (500,00 €) va al resultado de la venta.',
    );
    expect(question).toContain('Si ya lo corregiste con un asiento manual, no la vincules.');
    fireEvent.click(screen.getByRole('button', { name: 'Sí, vincular' }));
    await vi.waitFor(() => expect(api.assetSaleLink).toHaveBeenCalledWith(1, 7, 1050000));
    const status = (await screen.findByText(/^A2026-0050 vinculada a la venta de Envasadora/))
      .textContent;
    expect(status).toContain('El resultado de la venta sube 500,00 €.');
  });

  it('inmovilizado: sin facturas que vincular, dice cuáles salen y qué hacer', async () => {
    api.journal.mockResolvedValue([]);
    api.fixedAssets.mockResolvedValue([{ ...envasadora, disposed_on: '2026-06-30' }]);
    api.assetSales.mockResolvedValue([vendida]);
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Inmovilizado' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Vincular la factura de venta' }));
    expect(await screen.findByText('No hay facturas que se puedan vincular.')).toBeTruthy();
    // No manda emitir una que quizá ya existe: dice cuáles salen y qué hacer con las demás.
    expect(screen.queryByText(/emítela en Facturas y vuelve/)).toBeNull();
    const help = document.getElementById('vincular-ayuda-1')?.textContent ?? '';
    expect(help).toContain('de un ejercicio abierto');
    expect(help).toContain('reábrelo desde Cierre');
    expect(help).toContain('salda la 543 con un asiento manual');
  });

  it('inmovilizado: busca la factura por número, cliente o NIF', async () => {
    api.journal.mockResolvedValue([]);
    api.fixedAssets.mockResolvedValue([{ ...envasadora, disposed_on: '2026-06-30' }]);
    api.assetSales.mockResolvedValue([vendida]);
    api.assetSaleCandidates.mockImplementation((_id: number, q = '') =>
      Promise.resolve(q === 'nadie' ? [] : [factura]),
    );
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Inmovilizado' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Vincular la factura de venta' }));
    await screen.findByLabelText('Factura de venta');
    expect(api.assetSaleCandidates).toHaveBeenLastCalledWith(1, '');
    const search = screen.getByLabelText('Buscar la factura');
    expect(search.getAttribute('aria-describedby')).toBe('vincular-ayuda-1');
    fireEvent.change(search, { target: { value: ' uno ' } });
    await vi.waitFor(() => expect(api.assetSaleCandidates).toHaveBeenLastCalledWith(1, 'uno'));
    expect(await screen.findByText('1 factura encontrada')).toBeTruthy();
    fireEvent.change(search, { target: { value: 'nadie' } });
    expect(
      await screen.findByText('Ninguna factura que se pueda vincular lleva «nadie».'),
    ).toBeTruthy();
    expect(screen.getByText('0 facturas encontradas')).toBeTruthy();
  });

  it('inmovilizado: si no puede buscar las facturas, lo dice (no que no las haya)', async () => {
    api.journal.mockResolvedValue([]);
    api.fixedAssets.mockResolvedValue([{ ...envasadora, disposed_on: '2026-06-30' }]);
    api.assetSales.mockResolvedValue([vendida]);
    api.assetSaleCandidates.mockRejectedValue(new Error('la base de datos está ocupada'));
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Inmovilizado' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Vincular la factura de venta' }));
    const error = 'No se han podido buscar las facturas: la base de datos está ocupada';
    expect((await screen.findAllByText(error)).length).toBeGreaterThan(0);
    expect(screen.queryByText('No hay facturas que se puedan vincular.')).toBeNull();
  });

  it('inmovilizado: sin factura y con la 543 ya saldada, no dice que el precio siga en ella', async () => {
    api.journal.mockResolvedValue([]);
    api.fixedAssets.mockResolvedValue([{ ...envasadora, disposed_on: '2026-06-30' }]);
    api.assetSales.mockResolvedValue([{ ...vendida, unbilled: 0 }]);
    api.assetSaleCandidates.mockResolvedValue([factura]);
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Inmovilizado' }));
    expect(
      await screen.findByText(/vendido por 10\.000,00 € sin factura vinculada; la 543 ya no tiene/),
    ).toBeTruthy();
    expect(screen.queryByText(/€ en la 543/)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Vincular la factura de venta' }));
    expect(await screen.findByText(/La 543 ya no tiene todo el precio de esta venta/)).toBeTruthy();
  });

  it('inmovilizado: la factura vinculada se desvincula y la rectificativa pendiente se pasa', async () => {
    api.journal.mockResolvedValue([]);
    api.fixedAssets.mockResolvedValue([{ ...envasadora, disposed_on: '2026-06-30' }]);
    const rect = {
      ...factura,
      invoice_id: 8,
      number: 'R2026-0003',
      issue_date: '2026-07-15',
      base: -50000,
      open: -50000,
      suggested: -50000,
      rectifies: 'A2026-0050',
    };
    api.assetSales.mockResolvedValue([
      {
        ...vendida,
        invoiced: 1000000,
        unbilled: 0,
        invoices: [
          {
            invoice_id: 7,
            number: 'A2026-0050',
            issue_date: '2026-06-30',
            amount: 1000000,
            rectifies: null,
            entry_id: 5,
          },
        ],
        pending: [rect],
      },
    ]);
    api.assetSaleCandidates.mockResolvedValue([rect]);
    api.assetSaleUnlink.mockResolvedValue(undefined);
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: 'Inmovilizado' }));
    expect(
      await screen.findByText(/vendido por 10\.000,00 € \(beneficio de 995,07 €\)/),
    ).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Vincular la factura de venta' })).toBeNull();
    // Rebaja parte del precio: no «anula la venta».
    expect(screen.getByText(/Rectificativa R2026-0003 de A2026-0050 sin pasar/)).toBeTruthy();
    expect(screen.queryByText(/anula la venta/)).toBeNull();
    // La rectificativa pendiente abre el panel con ella elegida y su importe.
    fireEvent.click(screen.getByRole('button', { name: 'Pasar R2026-0003 a la venta' }));
    const select = (await screen.findByLabelText(
      'Rectificativa de la factura de venta',
    )) as HTMLSelectElement;
    await vi.waitFor(() => expect(select.value).toBe('8'));
    expect(
      (screen.getByLabelText('Importe de este elemento (sin IVA)') as HTMLInputElement).value,
    ).toBe('-500,00');
    fireEvent.click(screen.getByRole('button', { name: 'Desvincular A2026-0050' }));
    const question = screen.getByRole('alertdialog').textContent;
    expect(question).toContain('vuelve a contar como ventas');
    // El asiento borrado no se lleva sus adjuntos: se dice adónde van.
    expect(question).toContain('sus documentos y observaciones del Diario');
    fireEvent.click(screen.getByRole('button', { name: 'Sí, desvincular' }));
    await vi.waitFor(() => expect(api.assetSaleUnlink).toHaveBeenCalledWith(1, 7));
  });

  it('filtros puros del diario y del mayor', () => {
    const e = (date: string, concept: string, account: string, amount: number) => ({
      id: 1,
      number: 1,
      entry: {
        date,
        concept,
        origin: 'Manual' as const,
        lines: [
          { account, debit: amount, credit: 0, description: null },
          { account: '572', debit: 0, credit: amount, description: null },
        ],
      },
    });
    const all = [e('2026-01-10', 'Luz', '628', 5000), e('2026-02-10', 'Agua', '628', 3000)];
    const name = (c: string) => (c === '628' ? 'Suministros' : undefined);
    expect(filterEntries(all, 'suministros', '', '', name)).toHaveLength(2);
    expect(filterEntries(all, '30', '', '', name)).toHaveLength(1);
    expect(filterEntries(all, '', '2026-02-01', '', name)).toHaveLength(1);
    expect(filterEntries(all, '', '', '2026-01-31', name)[0]?.entry.concept).toBe('Luz');
    const rows = [
      {
        number: 1,
        date: '2026-01-01',
        concept: 'Apertura',
        account: '572',
        debit: 1000,
        credit: 0,
        balance: 1000,
        line_id: 1,
        ticked: false,
      },
      {
        number: 2,
        date: '2026-02-01',
        concept: 'Pago',
        account: '572',
        debit: 0,
        credit: 300,
        balance: 700,
        line_id: 2,
        ticked: true,
      },
      {
        number: 3,
        date: '2026-03-01',
        concept: 'Cobro',
        account: '572',
        debit: 500,
        credit: 0,
        balance: 1200,
        line_id: 3,
        ticked: false,
      },
    ];
    const r = ledgerRange(rows, '2026-02-01', '2026-02-28');
    expect(r.opening).toBe(1000);
    expect(r.shown.map((x) => x.number)).toEqual([2]);
    expect(ledgerRange(rows, '', '').opening).toBe(0);
  });

  it('puntea líneas del mayor y filtra las pendientes', async () => {
    api.ledger.mockResolvedValue([
      {
        number: 1,
        date: '2026-01-05',
        concept: 'Comisión',
        account: '5720001',
        debit: 0,
        credit: 1000,
        balance: -1000,
        line_id: 11,
        ticked: false,
      },
      {
        number: 2,
        date: '2026-02-05',
        concept: 'Cobro',
        account: '5720001',
        debit: 5000,
        credit: 0,
        balance: 4000,
        line_id: 12,
        ticked: true,
      },
    ]);
    api.setTick.mockResolvedValue(undefined);
    render(<Accounting />);
    fireEvent.click(screen.getByRole('button', { name: 'Libro mayor' }));
    const box = (await screen.findByLabelText('Puntear Comisión 05/01/2026')) as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect(screen.getByText('punteado: 50,00 €')).toBeTruthy();
    fireEvent.click(box);
    await vi.waitFor(() => expect(api.setTick).toHaveBeenCalledWith(11, true));
    expect(screen.getByText('punteado: 40,00 €')).toBeTruthy();
    fireEvent.click(screen.getByLabelText('Puntear Cobro 05/02/2026'));
    fireEvent.change(screen.getByLabelText('Punteo'), { target: { value: 'sin' } });
    expect(screen.getByText('Cobro')).toBeTruthy();
    expect(screen.queryByText('Comisión')).toBeNull();
  });

  it('un error al puntear se avisa sin ocultar el mayor', async () => {
    api.ledger.mockResolvedValue([
      {
        number: 1,
        date: '2026-01-05',
        concept: 'Comisión',
        account: '5720001',
        debit: 0,
        credit: 1000,
        balance: -1000,
        line_id: 11,
        ticked: false,
      },
    ]);
    api.setTick.mockRejectedValue({ message: 'almacén bloqueado' });
    render(<Accounting />);
    fireEvent.click(screen.getByRole('button', { name: 'Libro mayor' }));
    const box = (await screen.findByLabelText('Puntear Comisión 05/01/2026')) as HTMLInputElement;
    fireEvent.click(box);
    expect((await screen.findByRole('alert')).textContent).toContain('almacén bloqueado');
    expect(box.checked).toBe(false);
    expect(screen.getByLabelText('Cuenta del mayor')).toBeTruthy();
  });
});

describe('frequentConcepts', () => {
  it('ordena los conceptos manuales por uso', async () => {
    const { frequentConcepts } = await import('./Accounting');
    const e = (concept: string, origin: unknown = 'Manual') =>
      ({ id: 1, number: 1, entry: { date: '2026-01-01', concept, origin, lines: [] } }) as never;
    expect(
      frequentConcepts([e('Alquiler'), e('Luz'), e('Alquiler'), e('Factura', { Banco: 1 })]),
    ).toEqual(['Alquiler', 'Luz']);
  });

  it('el aviso de «Guardado en …xlsx» se quita al cambiar de pestaña', async () => {
    api.journal.mockResolvedValue([]);
    api.fixedAssets.mockResolvedValue([]);
    api.pickFolder.mockResolvedValue('/tmp');
    api.exportListings.mockResolvedValue('/tmp/Contabilidad 2026.xlsx');
    render(<Accounting />);
    fireEvent.click(await screen.findByRole('button', { name: /Exportar a Excel/ }));
    expect(await screen.findByText(/Guardado en .*xlsx/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Inmovilizado' }));
    expect(screen.queryByText(/Guardado en .*xlsx/)).toBeNull();
  });
});

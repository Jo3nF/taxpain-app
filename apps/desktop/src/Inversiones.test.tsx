import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CuentaView, OpValores, ValoresReport } from './valores';

const api = vi.hoisted(() => ({
  valoresReport: vi.fn(),
  valoresOps: vi.fn(),
  valoresSecurities: vi.fn(),
  valoresPrices: vi.fn(),
  valoresIbkrConnect: vi.fn(),
  valoresIbkrSync: vi.fn(),
  valoresReadFiles: vi.fn(),
  valoresImport: vi.fn(),
  valoresSaveOp: vi.fn(),
  valoresSaveSettings: vi.fn(),
  valoresBce: vi.fn(),
  valoresPostEntries: vi.fn(),
  valoresUndoEntries: vi.fn(),
  pickValoresFiles: vi.fn(),
}));
vi.mock('./valores', async (orig) => ({ ...(await orig<typeof import('./valores')>()), ...api }));
const { Inversiones, eurPorUnidad } = await import('./Inversiones');
const { parseDecimalEs } = await import('./valores');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const cuenta = (
  c: Partial<CuentaView> & Pick<CuentaView, 'id' | 'broker' | 'name'>,
): CuentaView => ({
  number: '',
  currency: 'EUR',
  country: '',
  abroad: true,
  account: '',
  flex_query: '',
  last_sync: null,
  last_sync_note: '',
  token: false,
  ops: 0,
  conector: 'manual',
  ...c,
});

const report: ValoresReport = {
  year: 2026,
  inicio: '2026-01-01',
  fecha: '2026-12-31',
  persona_fisica: false,
  metodo: 'pmp',
  settings: {
    cartera: 'coste',
    plazo: 'corto',
    cuentas: {
      corto_patrimonio: '540',
      corto_deuda: '541',
      largo_patrimonio: '250',
      largo_deuda: '251',
      tesoreria: '5720090',
      beneficio: '766',
      perdida: '666',
      beneficio_vr: '7630',
      perdida_vr: '6630',
      dividendos: '760',
      intereses: '769',
      comisiones: '669',
      retencion_espana: '473',
      impuesto_extranjero: '6300',
      tributos: '631',
      deterioro_corto: '698',
      correctora_corto: '596',
      reversion_corto: '798',
      deterioro_largo: '6963',
      correctora_largo: '296',
      reversion_largo: '7963',
    },
  },
  cuentas: [
    cuenta({
      id: 1,
      broker: 'ibkr',
      name: 'IBKR con token',
      number: 'U0000001',
      token: true,
      flex_query: '123456',
      conector: 'api',
      ops: 9,
      last_sync: '2026-11-02 09:00:00 UTC',
      last_sync_note: 'Sincronizado con el Flex Web Service · 9 nuevas, 0 ya estaban',
    }),
    cuenta({ id: 2, broker: 'ibkr', name: 'IBKR sin token', conector: 'fichero' }),
    cuenta({ id: 3, broker: 'xtb', name: 'XTB 12345678', number: '12345678', conector: 'pdf' }),
  ],
  posiciones: [
    {
      security_id: 1,
      ticker: 'ACME',
      name: 'Acme',
      isin: '',
      kind: 'accion',
      account_id: 1,
      account: 'IBKR con token',
      abroad: true,
      country: 'IE',
      cantidad: '20',
      coste: 90_090,
      precio: '60',
      divisa: 'USD',
      precio_fecha: null,
      cambio: '0.95',
      valor: 114_000,
      latente: 23_910,
    },
  ],
  coste_total: 90_090,
  valor_total: 114_000,
  latente_total: 23_910,
  resultado: {
    ventas: 1,
    ganancias: 3_825,
    perdidas: 0,
    neto: 3_825,
    derivados: 0,
    ventas_derivados: 0,
    dividendos: 1_840,
    retenciones_extranjero: 276,
    retenciones_espana: 0,
    intereses: 450,
    comisiones: 150,
    impuestos: 0,
  },
  realizados: [],
  doble_imposicion: {
    tipo_pct: 25,
    paises: [
      {
        pais: 'US',
        rentas: 1_840,
        retenido: 276,
        convenio_pct: 15,
        convenio_comprobado: true,
        tope_convenio: 276,
        cuota_espanola: 460,
        deducible: 276,
        exceso: 0,
      },
    ],
    retenido: 276,
    deducible: 276,
    nota: 'Deducción por doble imposición jurídica internacional (art. 31 LIS).',
  },
  m720: {
    year: 2026,
    fecha: '2026-12-31',
    total_extranjero: 114_000,
    umbral: 5_000_000,
    supera: false,
    cuentas: [],
    sin_precio: [],
    persona_fisica: false,
    exenta_por_contabilidad: true,
    nota: 'Los valores en el extranjero no llegan a 50.000 €.',
  },
  asientos: [],
  ops_ejercicio: 1,
  sin_euros: 0,
  avisos: ['Sin precio de cierre a 2026-12-31 de MMKT.DE.'],
  notas: ['Coste medio ponderado por valor (NRV 9.ª).'],
};

const op: OpValores = {
  id: 7,
  account_id: 1,
  to_account_id: null,
  security_id: 1,
  date: '2026-03-10',
  time: '15:30:00',
  kind: 'compra',
  qty: '10',
  price: '100',
  currency: 'USD',
  amount: 100_000,
  fee: 100,
  fx: '0.9',
  eur: 90_000,
  fee_eur: 90,
  country: '',
  note: '',
  source: 'ibkr',
  fingerprint: 'ibkr:U0000001:t:21',
  entry_id: null,
};

function setup() {
  api.valoresReport.mockResolvedValue(report);
  api.valoresOps.mockResolvedValue([op]);
  api.valoresSecurities.mockResolvedValue([
    {
      id: 1,
      isin: '',
      ticker: 'ACME',
      name: 'Acme',
      currency: 'USD',
      kind: 'accion',
      country: 'US',
      account: '',
    },
  ]);
  api.valoresPrices.mockResolvedValue([]);
}

describe('Inversiones en valores', () => {
  it('enseña las cuentas con su conector, las posiciones, el resultado, las retenciones y el 720', async () => {
    setup();
    render(<Inversiones year={2026} />);
    const lista = within(await screen.findByRole('list', { name: 'Cuentas de bróker' }));
    expect(
      lista.getByText(/Conectada por API \(Flex Web Service\) · Query ID 123456/),
    ).toBeTruthy();
    expect(lista.getByText(/Sin conectar: importa el Flex a mano/)).toBeTruthy();
    expect(lista.getByText(/Por el informe en PDF o Excel \(XTB no tiene API\)/)).toBeTruthy();
    expect(lista.getByRole('button', { name: 'Sincronizar 2026' })).toBeTruthy();
    expect(
      lista.getByRole('button', { name: 'Importar informe de XTB (PDF o Excel)…' }),
    ).toBeTruthy();
    // Sin opciones en el año, no hay línea de derivados; sin operaciones sin euros, no hay botón.
    expect(screen.queryByRole('rowheader', { name: 'Derivados (opciones)' })).toBeNull();
    expect(screen.queryByRole('button', { name: /Pasar a euros con el BCE/ })).toBeNull();
    expect(screen.getByText('Sin precio de cierre a 2026-12-31 de MMKT.DE.')).toBeTruthy();
    expect(screen.getAllByText('1.140,00 €')).toHaveLength(2); // el valor y el total
    expect(screen.getByRole('rowheader', { name: 'Neto de las ventas' })).toBeTruthy();
    expect(screen.getByText('Los valores en el extranjero no llegan a 50.000 €.')).toBeTruthy();
    expect(screen.getByText('15 %')).toBeTruthy();
    // La compra en dólares: importe en USD y en euros.
    expect(screen.getByText('1.000,00 USD')).toBeTruthy();
    expect(screen.getByText('900,00 €')).toBeTruthy();
  });

  it('conecta Interactive Brokers con el token y el Query ID, y sincroniza el año', async () => {
    setup();
    api.valoresIbkrConnect.mockResolvedValue({});
    api.valoresIbkrSync.mockResolvedValue({
      nuevas: 2,
      duplicadas: 41,
      sustituidas: 0,
      precios: 3,
      cuentas_nuevas: 0,
      avisos: [],
    });
    render(<Inversiones year={2026} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Conectar por API…' }));
    const form = within(screen.getByRole('form', { name: 'Conectar IBKR sin token por API' }));
    expect(form.getByText(/Flex Web Service Configuration/)).toBeTruthy();
    fireEvent.change(form.getByLabelText('Token del Flex Web Service'), {
      target: { value: ' abcdef0123456789 ' },
    });
    fireEvent.change(form.getByLabelText('Query ID de la Flex Query'), {
      target: { value: '654321' },
    });
    fireEvent.click(form.getByRole('button', { name: 'Guardar y conectar' }));
    await vi.waitFor(() =>
      expect(api.valoresIbkrConnect).toHaveBeenCalledWith(2, 'abcdef0123456789', '654321'),
    );
    expect(await screen.findByText('Conectada: ya puedes sincronizar.')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Sincronizar 2026' }));
    await vi.waitFor(() => expect(api.valoresIbkrSync).toHaveBeenCalledWith(1, 2026));
    expect(
      await screen.findByText('Sincronizado 2026: 2 operaciones nuevas, 41 ya estaban, 3 precios.'),
    ).toBeTruthy();
  });

  it('lee varios ficheros, enseña lo leído e importa', async () => {
    setup();
    api.pickValoresFiles.mockResolvedValue(['/tmp/xtb.pdf', '/tmp/flex.xml']);
    api.valoresReadFiles.mockResolvedValue({
      ficheros: [
        {
          nombre: 'xtb.pdf',
          formato: 'XTB (PDF)',
          cuenta: '12345678',
          resumen: '3 × Compra, 3 × Venta',
          desde: '2025-03-01',
          hasta: '2025-10-03',
          error: null,
        },
        {
          nombre: 'flex.xml',
          formato: '',
          cuenta: '',
          resumen: '',
          desde: null,
          hasta: null,
          error: 'el XML no trae ningún extracto',
        },
      ],
      extractos: [{}],
      nuevas: 6,
      duplicadas: 5,
      precios: 1,
      avisos: ['1 filas de CFD no se importan'],
    });
    api.valoresImport.mockResolvedValue({
      nuevas: 6,
      duplicadas: 5,
      sustituidas: 0,
      precios: 1,
      cuentas_nuevas: 1,
      avisos: [],
    });
    render(<Inversiones year={2026} />);
    fireEvent.click(
      await screen.findByRole('button', {
        name: 'Importar informe de XTB (PDF o Excel) o Flex de IBKR…',
      }),
    );
    await vi.waitFor(() =>
      expect(api.valoresReadFiles).toHaveBeenCalledWith(['/tmp/xtb.pdf', '/tmp/flex.xml']),
    );
    const leido = within(await screen.findByRole('region', { name: 'Lo leído de los ficheros' }));
    expect(leido.getByText(/XTB \(PDF\) · cuenta 12345678 · 3 × Compra, 3 × Venta/)).toBeTruthy();
    expect(leido.getByText(/no se puede leer: el XML no trae ningún extracto/)).toBeTruthy();
    expect(leido.getByText(/6 operaciones nuevas y 5 que ya estaban/)).toBeTruthy();
    expect(leido.getByText('1 filas de CFD no se importan')).toBeTruthy();
    fireEvent.click(leido.getByRole('button', { name: 'Importar' }));
    await vi.waitFor(() => expect(api.valoresImport).toHaveBeenCalled());
    expect(
      await screen.findByText(
        'Importado: 6 operaciones nuevas, 5 ya estaban, 1 precio, 1 cuenta nueva.',
      ),
    ).toBeTruthy();
  });

  it('una opción vencida va en la línea de derivados y la cuenta en dólares se pasa a euros con el BCE', async () => {
    setup();
    api.valoresReport.mockResolvedValue({
      ...report,
      resultado: { ...report.resultado, derivados: -7_650, ventas_derivados: 1 },
      realizados: [
        {
          op: 9,
          fecha: '2026-10-09',
          security_id: 5,
          ticker: 'DEMO.US_261009_P_40',
          name: 'Demo (opción)',
          account: 'XTB 87654321',
          cantidad: '3',
          transmision: 0,
          coste: 7_650,
          resultado: -7_650,
          dos_meses: false,
          derivado: true,
        },
      ],
      sin_euros: 12,
      avisos: ['12 operaciones del ejercicio en otra divisa sin importe en euros.'],
    });
    api.valoresBce.mockResolvedValue({ divisas: ['USD'], tipos: 200, operaciones: 12 });
    render(<Inversiones year={2026} />);
    const fila = (await screen.findByRole('rowheader', { name: 'Derivados (opciones)' }))
      .parentElement as HTMLElement;
    expect(within(fila).getByText('-76,50 €')).toBeTruthy();
    expect(screen.getByText('(derivado)')).toBeTruthy();
    fireEvent.click(
      screen.getByRole('button', { name: 'Pasar a euros con el BCE (12 operaciones)' }),
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Sí, descargar del BCE' }));
    await vi.waitFor(() => expect(api.valoresBce).toHaveBeenCalled());
    expect(
      await screen.findByText('Tipos del BCE de USD: 12 operaciones pasadas a euros.'),
    ).toBeTruthy();
  });

  it('contabiliza los asientos de las operaciones tras confirmar y dice cuántos', async () => {
    setup();
    api.valoresReport.mockResolvedValue({
      ...report,
      asientos: [
        {
          fecha: '2026-02-10',
          concepto: 'Compra de 10 ACME (IBKR con token)',
          op: 1,
          entry_id: null,
          lineas: [
            { cuenta: '5400001', debe: 90_000, haber: 0 },
            { cuenta: '5720090', debe: 0, haber: 90_000 },
          ],
        },
      ],
    });
    api.valoresPostEntries.mockResolvedValue({ asientos: 1, borrados: 0, saltadas: 0, avisos: [] });
    render(<Inversiones year={2026} />);
    expect(await screen.findByText('Asientos (0 de 1 grabados)')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Contabilizar' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, contabilizar' }));
    expect(await screen.findByText('1 asiento grabado.')).toBeTruthy();
    expect(api.valoresPostEntries).toHaveBeenCalledWith(2026, false);
  });

  it('lee los números en español: «1.000» títulos son mil y el precio con coma', async () => {
    setup();
    api.valoresSaveOp.mockResolvedValue(8);
    render(<Inversiones year={2026} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Nueva operación' }));
    const f = within(screen.getByRole('form', { name: 'Operación' }));
    fireEvent.change(f.getByLabelText('Títulos'), { target: { value: '1.000' } });
    fireEvent.change(f.getByLabelText('Precio por título'), { target: { value: '10,5' } });
    fireEvent.change(f.getByLabelText('Importe (sin comisión)'), { target: { value: '10.500' } });
    fireEvent.click(f.getByRole('button', { name: 'Añadir operación' }));
    await vi.waitFor(() =>
      expect(api.valoresSaveOp).toHaveBeenCalledWith(
        expect.objectContaining({ qty: '1000', price: '10.5', amount: 1_050_000 }),
      ),
    );
  });

  it('no guarda un precio que no se entiende sin decirlo', async () => {
    setup();
    render(<Inversiones year={2026} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Nueva operación' }));
    const f = within(screen.getByRole('form', { name: 'Operación' }));
    fireEvent.change(f.getByLabelText('Títulos'), { target: { value: '10' } });
    fireEvent.change(f.getByLabelText('Precio por título'), { target: { value: '12.345.6' } });
    fireEvent.change(f.getByLabelText('Importe (sin comisión)'), { target: { value: '100' } });
    fireEvent.click(f.getByRole('button', { name: 'Añadir operación' }));
    expect(await screen.findByText('Revisa el precio por título.')).toBeTruthy();
    expect(api.valoresSaveOp).not.toHaveBeenCalled();
  });

  it('«Sincronizar» sigue sin poder pulsarse mientras dura, aunque acabe otra orden', async () => {
    setup();
    api.valoresIbkrSync.mockReturnValue(new Promise(() => {}));
    api.valoresSaveSettings.mockResolvedValue(undefined);
    render(<Inversiones year={2026} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Sincronizar 2026' }));
    await vi.waitFor(() => expect(api.valoresIbkrSync).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole('button', { name: 'Guardar los ajustes' }));
    await vi.waitFor(() => expect(api.valoresSaveSettings).toHaveBeenCalled());
    await vi.waitFor(() => expect(api.valoresReport).toHaveBeenCalledTimes(2));
    const boton = screen.getByRole('button', { name: /Sincroniz/ }) as HTMLButtonElement;
    expect(boton.disabled).toBe(true);
    fireEvent.click(boton);
    expect(api.valoresIbkrSync).toHaveBeenCalledTimes(1);
  });

  it('parseDecimalEs: miles con punto, decimales con coma o con punto', () => {
    expect(parseDecimalEs('1.000', 18)).toBe('1000');
    expect(parseDecimalEs('1.234,5', 18)).toBe('1234.5');
    expect(parseDecimalEs('0,5', 18)).toBe('0.5');
    expect(parseDecimalEs('12.5', 8)).toBe('12.5');
    expect(parseDecimalEs('0.001', 8)).toBe('0.001');
    expect(parseDecimalEs('-10', 18)).toBe('-10');
    expect(parseDecimalEs('0,123456789', 8)).toBeNull();
    expect(parseDecimalEs('12.345.6', 8)).toBeNull();
    expect(parseDecimalEs('abc', 8)).toBeNull();
    expect(parseDecimalEs('0', 8)).toBeNull();
  });

  it('pasa el tipo del BCE a euros por unidad sin coma flotante', () => {
    expect(eurPorUnidad('1.25')).toBe('0.8');
    expect(eurPorUnidad('1.1193')).toBe('0.89341553');
    expect(eurPorUnidad('0')).toBeNull();
    expect(eurPorUnidad('abc')).toBeNull();
  });
});

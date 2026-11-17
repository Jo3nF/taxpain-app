import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CriptoOp, CriptoPlatform, CriptoReport } from './api';

const api = vi.hoisted(() => ({
  criptoReport: vi.fn(),
  criptoOps: vi.fn(),
  criptoPlatforms: vi.fn(),
  criptoPrices: vi.fn(),
  criptoSaveOp: vi.fn(),
  criptoSaveSettings: vi.fn(),
  criptoPostEntries: vi.fn(),
  criptoDeleteOp: vi.fn(),
  criptoReadFile: vi.fn(),
  criptoImport: vi.fn(),
  pickCriptoFile: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Criptomonedas } = await import('./Criptomonedas');
const { parseCriptoQty, criptoQty } = await import('./api');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const platforms: CriptoPlatform[] = [
  { id: 1, name: 'Kraken', kind: 'exchange', country: 'US', abroad: true, account: '' },
];
const op: CriptoOp = {
  id: 7,
  date: '2026-03-10',
  time: '10:00:00',
  kind: 'venta',
  platform_id: 1,
  to_platform_id: null,
  ticker: 'BTC',
  qty: '1',
  ticker2: null,
  qty2: null,
  eur: 5_000_000,
  fee_eur: 5_000,
  ref_price: null,
  note: '',
  source: 'manual',
  fingerprint: null,
  entry_id: null,
};
const report: CriptoReport = {
  year: 2026,
  fecha: '2026-12-31',
  settings: {
    metodo: 'pmp',
    naturaleza: 'existencias',
    cuentas: {
      existencias: '309',
      intangible: '207',
      tesoreria: '5720001',
      puente: '5550001',
      ventas: '700',
      variacion: '610',
      beneficio: '771',
      perdida: '671',
      comisiones: '626',
      recompensas: '769',
      deterioro_gasto: '693',
      deterioro: '390',
      reversion: '793',
    },
  },
  persona_fisica: false,
  metodo_declaracion: 'pmp',
  posiciones: [
    {
      ticker: 'BTC',
      platform_id: 1,
      platform: 'Kraken',
      abroad: true,
      cantidad: '1',
      coste: 3_000_000,
      precio: null,
      precio_fecha: null,
      valor: null,
      latente: null,
    },
  ],
  valor_total: 0,
  coste_total: 3_000_000,
  resumen_pmp: {
    metodo: 'pmp',
    year: 2026,
    ganancias: 1_994_500,
    perdidas: 0,
    neto: 1_994_500,
    recompensas: 0,
    comisiones: 0,
    transmisiones: 1,
  },
  resumen_fifo: {
    metodo: 'fifo',
    year: 2026,
    ganancias: 2_994_000,
    perdidas: 0,
    neto: 2_994_000,
    recompensas: 0,
    comisiones: 0,
    transmisiones: 1,
  },
  realizados: [],
  m721: {
    year: 2026,
    fecha: '2026-12-31',
    total_extranjero: 0,
    umbral: 5_000_000,
    supera: false,
    plataformas: [],
    sin_precio: ['BTC'],
    sin_plataforma: [],
    en_perfil: false,
    exenta_por_contabilidad: true,
    nota: 'Faltan precios de cierre a 2026-12-31 de BTC.',
  },
  ops_ejercicio: 1,
  ops_sin_asiento: 1,
  ops_con_asiento: 0,
  deterioro_registrado: null,
  avisos: ['Sin precio de cierre a 2026-12-31 de BTC.'],
  inicio: null,
  antes_del_inicio: false,
  ops_historial: 0,
  huecos: 0,
};

function setup() {
  api.criptoReport.mockResolvedValue(report);
  api.criptoOps.mockResolvedValue([op]);
  api.criptoPlatforms.mockResolvedValue(platforms);
  api.criptoPrices.mockResolvedValue([]);
}

describe('Criptomonedas', () => {
  it('enseña los dos métodos con el elegido marcado, las posiciones y los avisos', async () => {
    setup();
    render(<Criptomonedas year={2026} />);
    expect(await screen.findByText('Precio medio ✓')).toBeTruthy();
    expect(screen.getByRole('columnheader', { name: 'FIFO' })).toBeTruthy();
    expect(screen.getAllByText('19.945,00 €')).toHaveLength(2); // ganancias y neto
    expect(screen.getAllByText('29.940,00 €')).toHaveLength(2);
    expect(screen.getByText('Sin precio de cierre a 2026-12-31 de BTC.')).toBeTruthy();
    expect(screen.getByText('(extranjero)')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Contabilizar 1 operaciones' })).toBeTruthy();
    expect(screen.getByText('Faltan precios de cierre a 2026-12-31 de BTC.')).toBeTruthy();
  });

  it('añade una compra con cantidad exacta y euros en céntimos', async () => {
    setup();
    api.criptoSaveOp.mockResolvedValue(9);
    render(<Criptomonedas year={2026} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Nueva operación' }));
    const form = within(screen.getByRole('form', { name: 'Operación' }));
    fireEvent.change(form.getByLabelText('Fecha'), { target: { value: '2026-05-02' } });
    fireEvent.change(form.getByLabelText('Activo'), { target: { value: 'btc' } });
    fireEvent.change(form.getByLabelText('Cantidad'), { target: { value: '0,12345678' } });
    fireEvent.change(form.getByLabelText('Euros pagados o cobrados'), {
      target: { value: '1.234,56' },
    });
    fireEvent.change(form.getByLabelText('Comisión en euros'), { target: { value: '2,5' } });
    fireEvent.click(screen.getByRole('button', { name: 'Añadir operación' }));
    await vi.waitFor(() => expect(api.criptoSaveOp).toHaveBeenCalled());
    const saved = api.criptoSaveOp.mock.calls[0]?.[0] as CriptoOp;
    expect(saved).toMatchObject({
      id: 0,
      date: '2026-05-02',
      kind: 'compra',
      platform_id: 1,
      ticker: 'BTC',
      qty: '0.12345678',
      eur: 123_456,
      fee_eur: 250,
    });
    expect(await screen.findByText('Operación añadida.')).toBeTruthy();
  });

  it('importa un fichero tras la vista previa', async () => {
    setup();
    api.pickCriptoFile.mockResolvedValue('/tmp/trades.csv');
    api.criptoReadFile.mockResolvedValue({
      kind: 'kraken',
      ops: [{ ...op, id: 0, platform: 'Kraken', to_platform: null, fingerprint: 'abc' }],
      prices: [],
      duplicates: 0,
      new_platforms: [],
      warnings: ['1 operaciones no traen valor en euros'],
    });
    api.criptoImport.mockResolvedValue([1, 0, 0]);
    render(<Criptomonedas year={2026} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Importar fichero…' }));
    expect(await screen.findByText(/Ledgers de Kraken: 1 operaciones/)).toBeTruthy();
    expect(screen.getByText('1 operaciones no traen valor en euros')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Importar' }));
    await vi.waitFor(() => expect(api.criptoImport).toHaveBeenCalled());
    expect(
      await screen.findByText('Importadas 1 operaciones (0 duplicadas) y 0 precios.'),
    ).toBeTruthy();
  });
});

describe('Criptomonedas con punto de partida', () => {
  it('un ejercicio anterior sale como calculado por la gestoría y sus operaciones son historial', async () => {
    api.criptoReport.mockResolvedValue({
      ...report,
      year: 2025,
      fecha: '2025-12-31',
      realizados: [],
      ops_ejercicio: 1,
      ops_sin_asiento: 0,
      ops_historial: 1,
      antes_del_inicio: true,
      inicio: {
        fecha: '2025-12-31',
        cuenta: '',
        origen: 'Cálculos de la gestoría',
        nota: '',
        posiciones: [{ ticker: 'BTC', platform_id: 1, cantidad: '1', coste: 3_000_000 }],
      },
    });
    api.criptoOps.mockResolvedValue([{ ...op, date: '2025-03-10' }]);
    api.criptoPlatforms.mockResolvedValue(platforms);
    api.criptoPrices.mockResolvedValue([]);
    render(<Criptomonedas year={2025} />);
    expect(await screen.findByText('Calculado por la gestoría.')).toBeTruthy();
    expect(
      screen.getByRole('heading', { name: 'Posiciones a 31/12/2025 (punto de partida)' }),
    ).toBeTruthy();
    expect(screen.queryByText('Precio medio ✓')).toBeNull();
    expect(screen.getByText('historial')).toBeTruthy();
    expect(
      (screen.getByRole('button', { name: 'Contabilizar 0 operaciones' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    expect(screen.getByRole('region', { name: 'Punto de partida' })).toBeTruthy();
  });
});

describe('cantidades de cripto', () => {
  it('lee cantidades con coma o punto y las devuelve como texto decimal', () => {
    expect(parseCriptoQty('0,12345678')).toBe('0.12345678');
    expect(parseCriptoQty('1.234,5')).toBe('1234.5');
    // Un punto sin coma detrás es el decimal (no se multiplica por mil).
    expect(parseCriptoQty('0.001')).toBe('0.001');
    expect(parseCriptoQty('0.500')).toBe('0.5');
    expect(parseCriptoQty('1.234')).toBe('1.234');
    expect(parseCriptoQty('2')).toBe('2');
    expect(parseCriptoQty('0.000000000000000001')).toBe('0.000000000000000001');
    expect(parseCriptoQty('0')).toBeNull();
    expect(parseCriptoQty('1e3')).toBeNull();
    expect(parseCriptoQty('0,0000000000000000001')).toBeNull();
    expect(criptoQty('1234.5')).toBe('1.234,5');
    expect(criptoQty('0.25')).toBe('0,25');
    expect(criptoQty(null)).toBe('');
  });
});

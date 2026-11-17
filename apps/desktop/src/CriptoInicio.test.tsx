import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CriptoPlatform, CriptoPropuestaInicio, CriptoInicio as Inicio } from './api';

const api = vi.hoisted(() => ({
  criptoPropuestaInicio: vi.fn(),
  criptoSaveInicio: vi.fn(),
  criptoDeleteInicio: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { CriptoInicio } = await import('./CriptoInicio');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const platforms: CriptoPlatform[] = [
  { id: 1, name: 'Kraken', kind: 'exchange', country: 'IE', abroad: true, account: '' },
];

// Cifras inventadas.
const propuesta: CriptoPropuestaInicio = {
  fecha: '2025-12-31',
  ejercicio: 2025,
  motivo: '2025 es el último con el 200 presentado',
  posiciones: [
    { ticker: 'BTC', platform_id: null, cantidad: '1', coste: 0 },
    { ticker: 'ETH', platform_id: null, cantidad: '2', coste: 0 },
    { ticker: 'ZEC', platform_id: null, cantidad: '1.5', coste: 0 },
  ],
  cuenta: '2050004',
  cuentas: [{ cuenta: '2050004', nombre: 'Criptoactivos', saldo: 3_500_000 }],
  coste_diario: 3_500_000,
  casilla_102: null,
  hoja: 'control.xlsx',
  fecha_cantidades: '2025-12-01',
  cantidades_de_operaciones: false,
  reparto_seguro: false,
  por_repartir: 3_500_000,
  origen: 'Cálculos de la gestoría: diario a 31/12/2025',
  notas: ['Falta repartir 35.000,00 € del diario entre las monedas.'],
  polvo: [],
};

const guardado: Inicio = {
  fecha: '2025-12-31',
  cuenta: '2050004',
  origen: 'Cálculos de la gestoría',
  nota: '',
  posiciones: [{ ticker: 'BTC', platform_id: 1, cantidad: '0.5', coste: 1_500_000 }],
};

describe('Punto de partida de Criptomonedas', () => {
  it('rellena la propuesta de la gestoría, se reparte el coste y se guarda', async () => {
    api.criptoPropuestaInicio.mockResolvedValue(propuesta);
    api.criptoSaveInicio.mockResolvedValue(undefined);
    const onSaved = vi.fn();
    render(
      <CriptoInicio inicio={null} platforms={platforms} year={2026} huecos={3} onSaved={onSaved} />,
    );
    expect(screen.getByText(/Faltan operaciones: 3 ventas o traspasos/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Usar los cálculos de la gestoría' }));
    const form = within(await screen.findByRole('form', { name: 'Punto de partida' }));
    expect((form.getByLabelText('Fecha de corte') as HTMLInputElement).value).toBe('2025-12-31');
    expect((form.getByLabelText('Cuenta del diario') as HTMLInputElement).value).toBe('2050004');
    expect((form.getByLabelText('Cantidad 3') as HTMLInputElement).value).toBe('1,5');
    expect(form.getByText('Falta repartir 35.000,00 € del diario entre las monedas.')).toBeTruthy();
    expect(form.getByText(/No se puede deducir con seguridad/)).toBeTruthy();
    const cuadre = () => form.getByRole('status', { name: 'Cuadre con el diario' }).textContent;
    expect(cuadre()).toContain('Faltan 35.000,00 € por repartir.');
    fireEvent.change(form.getByLabelText('Coste 1'), { target: { value: '30.000' } });
    fireEvent.change(form.getByLabelText('Coste 2'), { target: { value: '5000' } });
    expect(cuadre()).toContain('Cuadra.');
    // Coste por unidad del ETH: 5.000 / 2.
    expect(form.getByText('2.500,00 €')).toBeTruthy();
    fireEvent.change(form.getByLabelText('Plataforma 1'), { target: { value: '1' } });
    fireEvent.click(form.getByRole('button', { name: 'Guardar el punto de partida' }));
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(api.criptoSaveInicio).toHaveBeenCalledWith({
      fecha: '2025-12-31',
      cuenta: '2050004',
      origen: 'Cálculos de la gestoría: diario a 31/12/2025',
      nota: '',
      posiciones: [
        { ticker: 'BTC', platform_id: 1, cantidad: '1', coste: 3_000_000 },
        { ticker: 'ETH', platform_id: null, cantidad: '2', coste: 500_000 },
        { ticker: 'ZEC', platform_id: null, cantidad: '1.5', coste: 0 },
      ],
    });
    expect(await screen.findByText('Punto de partida guardado.')).toBeTruthy();
  });

  it('vuelve a pedir los cálculos con la cuenta del diario que se escribe', async () => {
    api.criptoPropuestaInicio.mockResolvedValue({ ...propuesta, cuenta: '', coste_diario: null });
    render(
      <CriptoInicio inicio={null} platforms={platforms} year={2026} huecos={0} onSaved={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Usar los cálculos de la gestoría' }));
    const form = within(await screen.findByRole('form', { name: 'Punto de partida' }));
    expect(api.criptoPropuestaInicio).toHaveBeenLastCalledWith(undefined);
    expect(form.queryByRole('status', { name: 'Cuadre con el diario' })).toBeNull();
    api.criptoPropuestaInicio.mockResolvedValue({ ...propuesta, cuenta: '2060009' });
    fireEvent.change(form.getByLabelText('Cuenta del diario'), { target: { value: '2060009' } });
    fireEvent.click(form.getByRole('button', { name: 'Usar los cálculos de la gestoría' }));
    await waitFor(() => expect(api.criptoPropuestaInicio).toHaveBeenLastCalledWith('2060009'));
    expect(await form.findByRole('status', { name: 'Cuadre con el diario' })).toBeTruthy();
  });

  it('a mano: no guarda una cantidad que no se entiende y se quita una fila', async () => {
    render(
      <CriptoInicio inicio={null} platforms={platforms} year={2026} huecos={0} onSaved={vi.fn()} />,
    );
    expect(screen.queryByText(/Faltan operaciones/)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Ponerlo a mano' }));
    const form = within(screen.getByRole('form', { name: 'Punto de partida' }));
    expect((form.getByLabelText('Fecha de corte') as HTMLInputElement).value).toBe('2025-12-31');
    fireEvent.change(form.getByLabelText('Moneda 1'), { target: { value: 'btc' } });
    fireEvent.change(form.getByLabelText('Cantidad 1'), { target: { value: '1e3' } });
    fireEvent.click(form.getByRole('button', { name: 'Guardar el punto de partida' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/^BTC: revisa la cantidad/);
    expect(form.getByLabelText('Cantidad 1').getAttribute('aria-invalid')).toBe('true');
    expect(api.criptoSaveInicio).not.toHaveBeenCalled();
    fireEvent.click(form.getByRole('button', { name: 'Añadir moneda' }));
    expect(form.getByLabelText('Moneda 2')).toBeTruthy();
    fireEvent.click(form.getByRole('button', { name: 'Quitar la fila 1' }));
    expect(form.queryByLabelText('Moneda 2')).toBeNull();
  });

  it('enseña el guardado y se puede quitar', async () => {
    api.criptoDeleteInicio.mockResolvedValue(undefined);
    const onSaved = vi.fn();
    render(
      <CriptoInicio
        inicio={guardado}
        platforms={platforms}
        year={2026}
        huecos={0}
        onSaved={onSaved}
      />,
    );
    expect(screen.getByText('31/12/2025')).toBeTruthy();
    expect(screen.getByRole('cell', { name: 'Kraken' })).toBeTruthy();
    expect(screen.getByRole('cell', { name: '0,5' })).toBeTruthy();
    expect(screen.getAllByText('15.000,00 €')).toHaveLength(2); // la fila y el total
    expect(screen.getByText('30.000,00 €')).toBeTruthy(); // por unidad
    expect(
      screen.getByText(/una vez elegido, se mantiene \(principio\s+de uniformidad\)/),
    ).toBeTruthy();
    expect(screen.getByText(/arts\. 66\s+bis\.1 y 115\.1 LGT/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Quitar el punto de partida' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Sí, quitarlo' }));
    await waitFor(() => expect(api.criptoDeleteInicio).toHaveBeenCalled());
    expect(onSaved).toHaveBeenCalled();
    // Cambiarlo abre el formulario con lo guardado.
    fireEvent.click(screen.getByRole('button', { name: 'Cambiar' }));
    const form = within(screen.getByRole('form', { name: 'Punto de partida' }));
    expect((form.getByLabelText('Coste 1') as HTMLInputElement).value).toBe('15000,00');
    expect((form.getByLabelText('Plataforma 1') as HTMLSelectElement).value).toBe('1');
  });

  it('una cantidad de miles se edita sin puntos y se guarda igual; los avisos se enseñan', async () => {
    api.criptoSaveInicio.mockResolvedValue([
      '1 operación de después del corte ya tiene asiento: rehaz los asientos de 2026.',
    ]);
    render(
      <CriptoInicio
        inicio={{
          ...guardado,
          posiciones: [{ ticker: 'FLR', platform_id: null, cantidad: '5000', coste: 12_345 }],
        }}
        platforms={platforms}
        year={2026}
        huecos={0}
        onSaved={vi.fn()}
      />,
    );
    expect(screen.getByRole('cell', { name: '5.000' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Cambiar' }));
    const form = within(screen.getByRole('form', { name: 'Punto de partida' }));
    expect((form.getByLabelText('Cantidad 1') as HTMLInputElement).value).toBe('5000');
    fireEvent.click(form.getByRole('button', { name: 'Guardar el punto de partida' }));
    await waitFor(() => expect(api.criptoSaveInicio).toHaveBeenCalled());
    expect(api.criptoSaveInicio.mock.calls[0]?.[0].posiciones[0].cantidad).toBe('5000');
    expect(await screen.findByText(/rehaz los asientos de 2026/)).toBeTruthy();
  });

  it('una fecha de corte de hoy o futura no se guarda y el foco va al campo', async () => {
    render(
      <CriptoInicio
        inicio={guardado}
        platforms={platforms}
        year={2026}
        huecos={0}
        onSaved={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Cambiar' }));
    const form = within(screen.getByRole('form', { name: 'Punto de partida' }));
    const fecha = form.getByLabelText('Fecha de corte') as HTMLInputElement;
    fireEvent.change(fecha, { target: { value: '2999-12-31' } });
    fireEvent.click(form.getByRole('button', { name: 'Guardar el punto de partida' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/anterior a hoy/);
    expect(fecha.getAttribute('aria-invalid')).toBe('true');
    expect(document.activeElement).toBe(fecha);
    expect(api.criptoSaveInicio).not.toHaveBeenCalled();
  });

  it('el polvo no se propone, pero se puede añadir', async () => {
    api.criptoPropuestaInicio.mockResolvedValue({
      ...propuesta,
      polvo: [{ ticker: 'PEPE', platform_id: null, cantidad: '12345.5', coste: 0 }],
    });
    render(
      <CriptoInicio inicio={null} platforms={platforms} year={2026} huecos={0} onSaved={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Usar los cálculos de la gestoría' }));
    const form = within(await screen.findByRole('form', { name: 'Punto de partida' }));
    expect(form.getByText(/PEPE 12\.345,5/)).toBeTruthy();
    expect(form.queryByLabelText('Moneda 4')).toBeNull();
    fireEvent.click(form.getByRole('button', { name: 'Añadir el polvo' }));
    expect((form.getByLabelText('Moneda 4') as HTMLInputElement).value).toBe('PEPE');
    expect((form.getByLabelText('Cantidad 4') as HTMLInputElement).value).toBe('12345,5');
    expect(form.queryByRole('button', { name: 'Añadir el polvo' })).toBeNull();
  });
});

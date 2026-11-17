import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Ecpn, ModeloAviso } from './api';

const api = vi.hoisted(() => ({
  cuentasModeloAviso: vi.fn(),
  cuentasModeloElegir: vi.fn(),
  ecpn: vi.fn(),
  ecpnSave: vi.fn(),
  memoriaSave: vi.fn(),
  memoriaTextos: vi.fn(),
  memoriaTextosGuardar: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { EcpnView, MemoriaDraft, ModeloCuentas } = await import('./CuentasModelo');

const aviso = (elegido: ModeloAviso['elegido']): ModeloAviso => ({
  year: 2026,
  ejercicios: [
    {
      year: 2026,
      con_datos: true,
      activo: 12_000_000,
      cifra_negocios: 18_000_000,
      plantilla_decimas: 25,
      cumple_abreviado: 3,
      cumple_pyg_abreviada: 3,
      cumple_micro: 3,
    },
  ],
  recomendado: 'Pymes',
  puede_abreviado: true,
  puede_pyg_abreviada: true,
  puede_micro: true,
  elegido,
  explicacion: ['Reúne dos de las tres condiciones.'],
  aviso: elegido === 'Normal' ? 'Tienes elegido el modelo normal.' : null,
});

describe('CuentasModelo', () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('pone el modelo elegido y ofrece guardar otro con confirmación', async () => {
    api.cuentasModeloAviso.mockResolvedValue(aviso('Normal'));
    api.cuentasModeloElegir.mockResolvedValue(undefined);
    const onModel = vi.fn();
    const { rerender } = render(<ModeloCuentas year={2026} model="Pymes" onModel={onModel} />);
    expect((await screen.findByRole('alert')).textContent).toContain('normal');
    expect(onModel).toHaveBeenCalledWith('Normal');
    expect(screen.getByText('2,5')).toBeTruthy();
    rerender(<ModeloCuentas year={2026} model="Abreviado" onModel={onModel} />);
    fireEvent.click(screen.getByRole('button', { name: /Usar el modelo abreviado/ }));
    expect(api.cuentasModeloElegir).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Sí, usarlo' }));
    await vi.waitFor(() => expect(api.cuentasModeloElegir).toHaveBeenCalledWith('Abreviado'));
  });

  it('guarda los textos y la memoria con el modelo de la vista', async () => {
    api.memoriaTextos.mockResolvedValue({
      nota_fija: '',
      hechos_posteriores: '',
      honorarios_auditoria: null,
      formulacion: null,
      firmantes: [],
    });
    api.memoriaTextosGuardar.mockResolvedValue(undefined);
    api.memoriaSave.mockResolvedValue('/tmp/Memoria 2026.pdf');
    render(<MemoriaDraft year={2026} model="Abreviado" />);
    fireEvent.click(await screen.findByText(/Lo que no sale de la contabilidad/));
    fireEvent.change(screen.getByLabelText(/Honorarios de auditoría/), {
      target: { value: '1.500,00' },
    });
    fireEvent.change(screen.getByLabelText(/Administradores que firman/), {
      target: { value: 'Carmen Española Española\n' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar textos' }));
    await vi.waitFor(() =>
      expect(api.memoriaTextosGuardar).toHaveBeenCalledWith(
        2026,
        expect.objectContaining({
          honorarios_auditoria: 150_000,
          firmantes: ['Carmen Española Española'],
        }),
      ),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Guardar en PDF…' }));
    await vi.waitFor(() => expect(api.memoriaSave).toHaveBeenCalledWith(2026, true, 'Abreviado'));
  });

  it('muestra el ECPN sólo con las columnas que tienen importes', async () => {
    const importes = (cap: number, res: number) => [cap, 0, 0, 0, 0, 0, res];
    const e: Ecpn = {
      year: 2026,
      model: 'Abreviado',
      obligatorio: true,
      nota: 'Obligatorio en el modelo abreviado.',
      ingresos_gastos: [{ label: 'A) Resultado', amount: 370_000 }],
      columnas: ['Capital', 'Prima', 'Reservas', 'x', 'y', 'z', 'Resultado del ejercicio'],
      filas: [
        {
          fila: 'SaldoInicial',
          label: 'Saldo inicial',
          importes: importes(300_000, 0),
          total: 300_000,
        },
        { fila: 'Dividendos', label: 'Dividendos', importes: importes(0, 0), total: 0 },
        {
          fila: 'SaldoFinal',
          label: 'Saldo final',
          importes: importes(300_000, 370_000),
          total: 670_000,
        },
      ],
    };
    api.ecpn.mockResolvedValue(e);
    render(<EcpnView year={2026} model="Abreviado" />);
    expect(await screen.findByText('Obligatorio en el modelo abreviado.')).toBeTruthy();
    expect(screen.getByRole('columnheader', { name: 'Capital' })).toBeTruthy();
    expect(screen.queryByRole('columnheader', { name: 'Prima' })).toBeNull();
    expect(screen.queryByText('Dividendos')).toBeNull();
    expect(screen.getByText('6.700,00 €')).toBeTruthy();
  });
});

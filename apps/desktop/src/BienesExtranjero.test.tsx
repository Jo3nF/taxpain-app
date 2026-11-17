import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { BorradorExtranjero } from './api';
import { extranjeroHandlers } from './demoExtranjero';

const api = vi.hoisted(() => ({
  extranjeroBorrador: vi.fn(),
  extranjeroGuardarBien: vi.fn(),
  extranjeroGuardarDeclarado: vi.fn(),
  extranjeroQuitarDeclarado: vi.fn(),
  extranjeroMarcarPresentado: vi.fn(),
  extranjeroExportar: vi.fn(),
  pickFolder: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { BienesExtranjero } = await import('./BienesExtranjero');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function demo(modelo: string): BorradorExtranjero {
  return extranjeroHandlers.extranjero_borrador?.({ modelo, year: 2025 }) as BorradorExtranjero;
}

describe('BienesExtranjero', () => {
  it('muestra los bloques del 720, la decisión y que no hay fichero', async () => {
    api.extranjeroBorrador.mockResolvedValue(demo('720'));
    render(<BienesExtranjero model="720" year={2025} />);
    expect(await screen.findByText(/Cuentas en entidades financieras: se presenta/)).toBeTruthy();
    // La cuenta se declaró en 2024 con 52.000 € y ahora tiene 78.500 €: sube más de 20.000 €.
    expect(screen.getByText(/Ha subido más de 20.000 €/)).toBeTruthy();
    expect(screen.getByText(/Valores, derechos e IIC: no se presenta/)).toBeTruthy();
    expect(screen.getByRole('note').textContent).toContain('Orden HAP/72/2013');
    const cuentas = screen.getByRole('table', { name: /Cuentas en entidades/ });
    expect(within(cuentas).getByText('LT121000011101001000')).toBeTruthy();
    expect(within(cuentas).getByText('Ya declarado')).toBeTruthy();
    expect(screen.getByText(/consta presentado en 2024/)).toBeTruthy();
  });

  it('apunta el borrador como presentado tras confirmar', async () => {
    api.extranjeroBorrador.mockResolvedValue(demo('721'));
    api.extranjeroMarcarPresentado.mockResolvedValue([]);
    render(<BienesExtranjero model="721" year={2025} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Ya lo he presentado' }));
    expect(api.extranjeroMarcarPresentado).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Apuntar' }));
    await waitFor(() =>
      expect(api.extranjeroMarcarPresentado).toHaveBeenCalledWith('721', 2025, true),
    );
  });

  it('guarda lo declarado otro año y los datos de un bien', async () => {
    api.extranjeroBorrador.mockResolvedValue(demo('720'));
    api.extranjeroGuardarDeclarado.mockResolvedValue(undefined);
    api.extranjeroGuardarBien.mockResolvedValue(undefined);
    render(<BienesExtranjero model="720" year={2025} />);
    fireEvent.change(await screen.findByLabelText('Valor declarado'), {
      target: { value: '60.000' },
    });
    fireEvent.change(screen.getByLabelText('Saldo medio 4T declarado'), {
      target: { value: '55.000,50' },
    });
    fireEvent.change(screen.getByLabelText(/IBAN o ISIN declarados/), {
      target: { value: 'LT121000011101001000' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Añadir lo declarado' }));
    await waitFor(() =>
      expect(api.extranjeroGuardarDeclarado).toHaveBeenCalledWith(
        expect.objectContaining({
          modelo: '720',
          year: 2024,
          bloque: 'cuentas',
          valor: 6_000_000,
          saldo_medio: 5_500_050,
          bienes: ['LT121000011101001000'],
        }),
      ),
    );
    const neobanco = screen.getByText(/Neobanco · Cuenta bancaria/).closest('details');
    if (!neobanco) throw new Error('sin el bien');
    fireEvent.change(within(neobanco).getByLabelText('Condición del declarante'), {
      target: { value: 'autorizado' },
    });
    fireEvent.click(within(neobanco).getByRole('button', { name: 'Guardar datos' }));
    await waitFor(() =>
      expect(api.extranjeroGuardarBien).toHaveBeenCalledWith(
        expect.objectContaining({ origen: 'banco', ref_id: 1, condicion: 'autorizado' }),
      ),
    );
  });
});

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Diagnostico } from './api';

const api = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  recuperacionRestaurar: vi.fn(),
  recuperacionRescatar: vi.fn(),
  pickBackupFile: vi.fn(),
  pickZipSave: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));

const { RecuperacionEmpresa, PantallaRecuperacion } = await import('./Recuperacion');

const DIAG: Diagnostico = {
  id: 'principal',
  empresa: 'Estudio Demo SL',
  tipo: 'danada',
  titulo: 'Los datos están dañados',
  explicacion: 'El fichero está dañado.\n\nTaxPain no ha cambiado ni borrado nada.',
  detalle: 'el almacén está dañado o la clave no corresponde',
  copias: [
    {
      ruta: '/Users/demo/Copias/Estudio-2026-11-01.taxpain',
      nombre: 'Estudio-2026-11-01.taxpain',
      bytes: 2_000_000,
      modificada: '2026-11-01T10:00:00Z',
    },
  ],
  rescate_legible: true,
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('RecuperacionEmpresa', () => {
  it('explica qué ha pasado y abre la última copia con su contraseña', async () => {
    const onRestored = vi.fn();
    api.recuperacionRestaurar.mockResolvedValue(undefined);
    render(<RecuperacionEmpresa diag={DIAG} onRestored={onRestored} />);
    expect(screen.getByText(/Estudio Demo SL: Los datos están dañados/)).toBeTruthy();
    expect(screen.getByText(/no ha cambiado ni borrado nada/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Elegir' }));
    fireEvent.change(screen.getByLabelText('Contraseña de la copia'), {
      target: { value: 'caballo grapa batería' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Restaurar y abrir' }));
    await vi.waitFor(() => expect(onRestored).toHaveBeenCalled());
    expect(api.recuperacionRestaurar).toHaveBeenCalledWith(
      DIAG.copias[0]?.ruta,
      'caballo grapa batería',
    );
  });

  it('avisa de que el rescate sale sin cifrar antes de exportarlo', async () => {
    api.pickZipSave.mockResolvedValue('/Users/demo/rescate.zip');
    api.recuperacionRescatar.mockResolvedValue({
      fichero: 'rescate.zip',
      originales: ['taxpain.db'],
      tablas: 80,
      filas: 500,
      sin_volcado: null,
    });
    render(<RecuperacionEmpresa diag={DIAG} onRestored={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: /Exportar a un ZIP/ }));
    expect(api.pickZipSave).not.toHaveBeenCalled();
    expect(screen.getByText(/sin cifrar/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: /Entendido/ }));
    expect(await screen.findByText(/Guardado rescate.zip/)).toBeTruthy();
    expect(api.recuperacionRescatar).toHaveBeenCalledWith('principal', '/Users/demo/rescate.zip');
  });
});

describe('PantallaRecuperacion', () => {
  it('sin diagnósticos enseña el texto del arranque', () => {
    render(
      <PantallaRecuperacion
        text="El Llavero no ha dado la clave."
        broken={[]}
        onRestored={vi.fn()}
      />,
    );
    expect(screen.getByText('El Llavero no ha dado la clave.')).toBeTruthy();
  });
});

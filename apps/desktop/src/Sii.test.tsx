import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  siiEstado: vi.fn(),
  siiPlazos: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { SiiPanel } = await import('./Sii');

const base = {
  obligatorio: false,
  voluntario: false,
  motivos: [],
  avisos: [],
  formato_sii: false,
  norma: 'Arts. 62.6 y 69 bis RIVA',
};

describe('SiiPanel', () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('sin SII ni avisos no enseña nada', async () => {
    api.siiEstado.mockResolvedValue(base);
    const { container } = render(<SiiPanel />);
    await Promise.resolve();
    expect(container.textContent).toBe('');
    expect(api.siiPlazos).not.toHaveBeenCalled();
  });

  it('avisa de que el año que viene tocará el SII', async () => {
    api.siiEstado.mockResolvedValue({
      ...base,
      avisos: ['Desde enero de 2027 tendrás que llevar los libros por el SII.'],
    });
    render(<SiiPanel />);
    expect(
      await screen.findByText('Desde enero de 2027 tendrás que llevar los libros por el SII.'),
    ).toBeTruthy();
    expect(screen.getByText('Todavía no tienes que llevar los libros por el SII.')).toBeTruthy();
  });

  it('obligatorio: los plazos de las facturas, con los vencidos marcados', async () => {
    api.siiEstado.mockResolvedValue({
      ...base,
      obligatorio: true,
      motivos: ['Inscrita en el REDEME: IVA mensual y SII.'],
    });
    api.siiPlazos.mockResolvedValue([
      {
        libro: 'recibida',
        numero: 'R-1',
        nif: 'B12345674',
        nombre: 'Estudio Demo SL',
        fecha: '2026-11-02',
        limite: '2026-11-06',
        dias: -1,
        aproximado: false,
      },
    ]);
    render(<SiiPanel />);
    expect(await screen.findByText('Inscrita en el REDEME: IVA mensual y SII.')).toBeTruthy();
    expect(await screen.findByText('R-1 · Estudio Demo SL')).toBeTruthy();
    expect(screen.getByText(/vencido/)).toBeTruthy();
    expect(api.siiPlazos).toHaveBeenCalledTimes(1);
  });
});

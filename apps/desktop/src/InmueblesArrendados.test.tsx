import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  inmueblesList: vi.fn(),
  inmuebleGuardar: vi.fn(),
  inmuebleQuitar: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { InmueblesArrendados, arrendadoresDe } = await import('./InmueblesArrendados');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('InmueblesArrendados', () => {
  it('saca los arrendadores del detalle del 180', () => {
    expect(arrendadoresDe([['Inmuebles Ejemplo SA · A22222228', 1]])).toEqual([
      { nif: 'A22222228', name: 'Inmuebles Ejemplo SA' },
    ]);
  });

  it('avisa del arrendador sin inmueble y lo guarda', async () => {
    api.inmueblesList.mockResolvedValue([]);
    api.inmuebleGuardar.mockResolvedValue(undefined);
    const onChange = vi.fn();
    render(
      <InmueblesArrendados
        arrendadores={[{ nif: 'A22222228', name: 'Inmuebles Ejemplo SA' }]}
        onChange={onChange}
      />,
    );
    expect(
      await screen.findByText(/Inmuebles Ejemplo SA \(A22222228\): sin inmueble/),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Añadir su inmueble' }));
    const form = screen.getByRole('form', { name: 'Datos del inmueble' });
    const put = (label: RegExp | string, value: string) =>
      fireEvent.change(within(form).getByLabelText(label), { target: { value } });
    put(/Referencia catastral/, '9872023VH5797S0001WX');
    put('Vía', 'Calle Ejemplo');
    put('Número', '10');
    put('Municipio', 'València');
    put('Código de provincia', '46');
    put('Código postal', '46001');
    fireEvent.click(within(form).getByRole('button', { name: 'Guardar inmueble' }));
    await waitFor(() =>
      expect(api.inmuebleGuardar).toHaveBeenCalledWith(
        expect.objectContaining({
          nif: 'A22222228',
          situacion: 1,
          ref_catastral: '9872023VH5797S0001WX',
          exonerado_hasta: null,
        }),
      ),
    );
    await waitFor(() => expect(onChange).toHaveBeenCalled());
  });

  it('quita un inmueble tras confirmar', async () => {
    api.inmueblesList.mockResolvedValue([
      {
        nif: 'A22222228',
        situacion: 1,
        ref_catastral: '9872023VH5797S0001WX',
        via: 'Calle Ejemplo',
        numero: '10',
        municipio: 'València',
        provincia: '46',
        codigo_postal: '46001',
        exonerado_hasta: '2026-12-31',
      },
    ]);
    api.inmuebleQuitar.mockResolvedValue(undefined);
    render(<InmueblesArrendados />);
    fireEvent.click(await screen.findByRole('button', { name: 'Quitar el inmueble de A22222228' }));
    fireEvent.click(screen.getByRole('button', { name: 'Quitar' }));
    await waitFor(() => expect(api.inmuebleQuitar).toHaveBeenCalledWith('A22222228', true));
  });
});

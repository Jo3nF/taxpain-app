import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ResumenIrnr } from './api';
import { noResidentesHandlers } from './demoNoResidentes';

const api = vi.hoisted(() => ({
  irnrResumen: vi.fn(),
  irnrGuardarCertificado: vi.fn(),
  irnrQuitarCertificado: vi.fn(),
  irnrProponer: vi.fn(),
  irnrApuntar: vi.fn(),
  irnrQuitarRenta: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { NoResidentes } = await import('./NoResidentes');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const demo = () => noResidentesHandlers.irnr_resumen?.({ year: 2026 }) as ResumenIrnr;

describe('NoResidentes', () => {
  it('enseña certificados, rentas, candidatas y convenios', async () => {
    api.irnrResumen.mockResolvedValue(demo());
    render(<NoResidentes year={2026} />);
    const certs = await screen.findByRole('table', { name: 'Certificados de residencia' });
    expect(within(certs).getByText('Caduca pronto')).toBeTruthy();
    const rentas = screen.getByRole('table', { name: 'Rentas a no residentes' });
    expect(within(rentas).getByText('Convenio')).toBeTruthy();
    expect(within(rentas).getByText('Exenta')).toBeTruthy();
    expect(screen.getByText(/INV-0042/)).toBeTruthy();
    expect(screen.getByRole('table', { name: 'Tipos de convenio' })).toBeTruthy();
  });

  it('propone el tipo y apunta la renta tras confirmar', async () => {
    api.irnrResumen.mockResolvedValue(demo());
    api.irnrProponer.mockResolvedValue({
      regimen: 'convenio',
      tipo: 500,
      tipo_general: 1900,
      exencion: null,
      motivo: 'tipo general del convenio con Francia',
      avisos: ['Comprueba el convenio.'],
    });
    api.irnrApuntar.mockResolvedValue(3);
    const onChange = vi.fn();
    render(<NoResidentes year={2026} onChange={onChange} />);
    fireEvent.click(
      await screen.findByRole('button', { name: 'Apuntar como renta la factura INV-0042' }),
    );
    const form = screen.getByRole('form', { name: 'Renta a un no residente' });
    expect((within(form).getByLabelText('Importe íntegro') as HTMLInputElement).value).toBe(
      '300,00',
    );
    fireEvent.change(within(form).getByLabelText(/Tipo del convenio/), {
      target: { value: '5' },
    });
    fireEvent.click(within(form).getByRole('button', { name: 'Calcular el tipo' }));
    await waitFor(() =>
      expect(api.irnrProponer).toHaveBeenCalledWith(
        expect.objectContaining({
          nif: 'IE0000000T',
          bruto: 30_000,
          tipo_convenio: 500,
          recibida_id: 7,
        }),
      ),
    );
    expect(await screen.findByText('Comprueba el convenio.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Apuntar la renta' }));
    expect(api.irnrApuntar).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Apuntar' }));
    await waitFor(() =>
      expect(api.irnrApuntar).toHaveBeenCalledWith(
        expect.objectContaining({ pais: 'IE', naturaleza: 'canon' }),
        true,
      ),
    );
    await waitFor(() => expect(onChange).toHaveBeenCalled());
  });

  it('guarda un certificado de residencia', async () => {
    api.irnrResumen.mockResolvedValue(demo());
    api.irnrGuardarCertificado.mockResolvedValue(3);
    render(<NoResidentes year={2026} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Añadir un certificado' }));
    const form = screen.getByRole('form', { name: 'Certificado de residencia' });
    fireEvent.change(within(form).getByLabelText('Identificador fiscal del perceptor'), {
      target: { value: 'US-1' },
    });
    fireEvent.change(within(form).getByLabelText('Nombre o razón social'), {
      target: { value: 'Ejemplo Inc' },
    });
    fireEvent.change(within(form).getByLabelText(/País de residencia/), {
      target: { value: 'us' },
    });
    fireEvent.click(within(form).getByRole('button', { name: 'Guardar certificado' }));
    await waitFor(() =>
      expect(api.irnrGuardarCertificado).toHaveBeenCalledWith(
        expect.objectContaining({ nif: 'US-1', pais: 'US', valido_hasta: null }),
      ),
    );
  });
});

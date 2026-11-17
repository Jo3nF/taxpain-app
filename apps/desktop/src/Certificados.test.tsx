import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  modelCertificatesPrepare: vi.fn(),
  modelCertificatesSave: vi.fn(),
  modelCertificateEmail: vi.fn(),
  pickFolder: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Certificados } = await import('./Certificados');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const prep = (model: string) => ({
  model,
  year: 2026,
  payees: [
    {
      nif: '12345678Z',
      name: 'ESPAÑOLA ESPAÑOLA CARMEN',
      concept: model === '180' ? 'arrendamiento' : 'clave E.02',
      gross: 600_000,
      withheld: 114_000,
      email: 'carmen@example.com',
    },
    {
      nif: 'A22222228',
      name: 'Ejemplo Servicios SA',
      concept: model === '180' ? 'arrendamiento' : 'clave G.01',
      gross: 300_000,
      withheld: 45_000,
      email: null,
    },
  ],
  place: 'València',
  signer: 'ESPAÑOLA ESPAÑOLA CARMEN',
  signer_role: 'Administrador',
  notes: ['TaxPain no registra retribuciones en especie'],
});

describe('Certificados', () => {
  it('elige perceptores, lugar y fecha y guarda un PDF por perceptor y otro con todos', async () => {
    api.modelCertificatesPrepare.mockResolvedValue(prep('190'));
    api.pickFolder.mockResolvedValue('/tmp/cert');
    api.modelCertificatesSave.mockResolvedValue(['/tmp/cert/a.pdf', '/tmp/cert/b.pdf']);
    render(<Certificados model="190" year={2026} />);
    fireEvent.click(screen.getByRole('button', { name: 'Certificados para los perceptores…' }));
    expect(await screen.findByText('clave E.02')).toBeTruthy();
    expect(screen.getByText('1.140,00 €')).toBeTruthy();
    expect(screen.getByText(/en especie/)).toBeTruthy();
    expect((screen.getByLabelText('Lugar de expedición') as HTMLInputElement).value).toBe(
      'València',
    );
    fireEvent.change(screen.getByLabelText('Fecha de expedición'), {
      target: { value: '2027-01-20' },
    });
    fireEvent.change(screen.getByLabelText('Cargo'), {
      target: { value: 'Administradora única' },
    });
    fireEvent.click(screen.getByLabelText('Incluir a Ejemplo Servicios SA'));
    fireEvent.click(screen.getByLabelText('Un PDF con todos'));
    fireEvent.click(screen.getByRole('button', { name: 'Guardar los certificados…' }));
    expect(await screen.findByText('2 PDF guardados en /tmp/cert')).toBeTruthy();
    expect(api.modelCertificatesSave).toHaveBeenCalledWith(
      '190',
      2026,
      {
        place: 'València',
        date: '2027-01-20',
        signer: 'ESPAÑOLA ESPAÑOLA CARMEN',
        signer_role: 'Administradora única',
        nifs: ['12345678Z'],
        properties: {},
      },
      '/tmp/cert',
      true,
      true,
    );
  });

  it('en el 180 pide el inmueble y prepara el correo con el certificado', async () => {
    api.modelCertificatesPrepare.mockResolvedValue(prep('180'));
    api.modelCertificateEmail.mockResolvedValue(undefined);
    render(<Certificados model="180" year={2026} />);
    fireEvent.click(screen.getByRole('button', { name: 'Certificados para los perceptores…' }));
    fireEvent.change(await screen.findByLabelText('Inmueble de Ejemplo Servicios SA'), {
      target: { value: 'Calle Inventada 3' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Preparar correo a Ejemplo Servicios SA' }));
    expect(await screen.findByText(/sin enviar/)).toBeTruthy();
    const [model, year, options, nif, to] = api.modelCertificateEmail.mock.calls[0] as unknown[];
    expect([model, year, nif, to]).toEqual(['180', 2026, 'A22222228', null]);
    expect(options).toMatchObject({
      nifs: ['A22222228'],
      properties: { A22222228: 'Calle Inventada 3' },
    });
  });

  it('enseña el error si no se puede preparar', async () => {
    api.modelCertificatesPrepare.mockRejectedValue({ message: 'no configurado' });
    render(<Certificados model="193" year={2026} />);
    fireEvent.click(screen.getByRole('button', { name: 'Certificados para los perceptores…' }));
    expect(await screen.findByText('no configurado')).toBeTruthy();
  });
});

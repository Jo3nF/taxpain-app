import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CertificateInfo } from './api';

const cert: CertificateInfo = {
  fingerprint_sha256: 'ab'.repeat(32),
  subject_dn: 'CN=12345678Z CARMEN ESPAÑOLA (R: B12345674)',
  issuer_dn: 'CN=AC FNMT Usuarios PRUEBAS',
  serial: '01',
  valid_from: '2026-01-01T00:00:00Z',
  valid_to: '2099-01-01T00:00:00Z',
  kind: 'representante_pj',
  issuer_kind: 'fnmt',
  holder_name: 'CARMEN ESPAÑOLA',
  holder_nif: '12345678Z',
  entity_nif: 'B12345674',
  entity_name: 'EMPRESA DE PRUEBA SL',
  key_algorithm: 'RSA 2048',
  policy_oids: [],
};

const api = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  listCertificates: vi.fn(),
  pickCertificateFile: vi.fn(),
  importCertificate: vi.fn(),
  deleteCertificate: vi.fn(),
}));

vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));

const { Certificates } = await import('./Certificates');

describe('Certificates', () => {
  beforeEach(() => {
    api.isTauri.mockReturnValue(true);
    api.listCertificates.mockResolvedValue([]);
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('lista los certificados con titular, entidad y caducidad', async () => {
    api.listCertificates.mockResolvedValue([cert]);
    render(<Certificates />);
    expect(await screen.findByText('CARMEN ESPAÑOLA')).toBeTruthy();
    expect(screen.getByText(/Representante de persona jurídica · NIF 12345678Z/)).toBeTruthy();
    expect(screen.getByText(/EMPRESA DE PRUEBA SL \(B12345674\)/)).toBeTruthy();
    expect(screen.getByText(/Caduca el 01\/01\/2099/)).toBeTruthy();
  });

  it('importa con la ruta elegida y la contraseña, y la borra del formulario', async () => {
    api.pickCertificateFile.mockResolvedValue('/Users/x/cert.p12');
    api.importCertificate.mockResolvedValue(cert);
    render(<Certificates />);
    fireEvent.click(await screen.findByText(/Importar certificado/));
    const input = (await screen.findByLabelText('Contraseña del certificado')) as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'Prueba-1234' } });
    fireEvent.click(screen.getByText('Importar'));
    await vi.waitFor(() =>
      expect(api.importCertificate).toHaveBeenCalledWith('/Users/x/cert.p12', 'Prueba-1234'),
    );
    await vi.waitFor(() => expect(api.listCertificates).toHaveBeenCalledTimes(2));
    await vi.waitFor(() =>
      expect(screen.queryByLabelText('Contraseña del certificado')).toBeNull(),
    );
  });

  it('muestra el error del proceso nativo', async () => {
    api.pickCertificateFile.mockResolvedValue('/x/cert.p12');
    api.importCertificate.mockRejectedValue({
      message: 'la contraseña del certificado no es correcta',
    });
    render(<Certificates />);
    fireEvent.click(await screen.findByText(/Importar certificado/));
    fireEvent.change(await screen.findByLabelText('Contraseña del certificado'), {
      target: { value: 'mal' },
    });
    fireEvent.click(screen.getByText('Importar'));
    expect((await screen.findByRole('alert')).textContent).toContain('contraseña');
  });

  it('fuera de la app de escritorio no ofrece importar', async () => {
    api.isTauri.mockReturnValue(false);
    render(<Certificates />);
    expect(await screen.findByText('Disponible en la app de escritorio.')).toBeTruthy();
  });
});

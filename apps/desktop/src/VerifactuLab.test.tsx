import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { LabStatus } from './api';

const api = vi.hoisted(() => ({
  verifactuLabStatus: vi.fn(),
  verifactuLabCreate: vi.fn(),
  verifactuLabGenerate: vi.fn(),
  verifactuLabCancel: vi.fn(),
  verifactuLabQuery: vi.fn(),
  verifactuLabUseCertNif: vi.fn(),
  verifactuSend: vi.fn(),
  listCertificates: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { VerifactuLab } = await import('./VerifactuLab');

afterEach(() => {
  cleanup();
  for (const f of Object.values(api)) f.mockReset();
});

const base: LabStatus = {
  is_lab: true,
  info: { created_at: '', invoices: [] },
  records: [],
  checks: [
    { id: 'empresa', label: 'Empresa de pruebas', ok: true, blocking: true, detail: 'ok' },
    {
      id: 'nif',
      label: 'El certificado es del emisor',
      ok: false,
      blocking: true,
      detail: 'El emisor es B y el certificado es de A',
    },
  ],
};

describe('VerifactuLab', () => {
  it('guía los pasos: emisor del certificado, facturas, envío y consulta', async () => {
    api.listCertificates.mockResolvedValue([{ fingerprint_sha256: 'fp' }]);
    api.verifactuLabStatus.mockResolvedValue(base);
    api.verifactuLabUseCertNif.mockResolvedValue(undefined);
    api.verifactuLabGenerate.mockResolvedValue(['PRU2026-0001', 'PRU2026-0002']);
    render(<VerifactuLab />);
    expect(await screen.findByText(/El emisor es B/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Usar el NIF del certificado' }));
    await screen.findByText(/titular del certificado/);
    fireEvent.click(screen.getByRole('button', { name: 'Generar las facturas de prueba' }));
    await screen.findByText(/Emitidas: PRU2026-0001/);

    api.verifactuLabStatus.mockResolvedValue({
      ...base,
      checks: base.checks.map((c) => ({ ...c, ok: true })),
      info: { created_at: '', invoices: ['PRU2026-0001'] },
      records: [
        {
          num_serie: 'PRU2026-0001',
          record_type: 'alta',
          send_status: 'pendiente',
          error: null,
          csv: null,
          sent_at: null,
        },
      ],
    });
    fireEvent.click(screen.getByRole('button', { name: 'Volver a comprobar' }));
    api.verifactuSend.mockResolvedValue({
      estado_envio: 'Correcto',
      registros: 1,
      tiempo_espera_envio: 60,
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Enviar 1 registro' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Sí, enviar a pruebas' }));
    await screen.findByText(/Envío aceptado \(1 registro\)/);
    expect(api.verifactuSend).toHaveBeenCalledWith('fp', true);
  });

  it('no deja enviar con comprobaciones en rojo, fuera de pruebas o sin certificado', async () => {
    const pendiente = {
      num_serie: 'PRU2026-0001',
      record_type: 'alta' as const,
      send_status: 'pendiente',
      error: null,
      csv: null,
      sent_at: null,
    };
    const disabled = async () =>
      ((await screen.findByRole('button', { name: 'Enviar 1 registro' })) as HTMLButtonElement)
        .disabled;

    api.listCertificates.mockResolvedValue([{ fingerprint_sha256: 'fp' }]);
    api.verifactuLabStatus.mockResolvedValue({ ...base, records: [pendiente] });
    render(<VerifactuLab />);
    expect(await disabled()).toBe(true);
    expect(screen.getByText(/resuelve las comprobaciones/)).toBeTruthy();
    cleanup();

    const ok = base.checks.map((c) => ({ ...c, ok: true }));
    api.verifactuLabStatus.mockResolvedValue({
      ...base,
      is_lab: false,
      checks: ok,
      records: [pendiente],
    });
    render(<VerifactuLab />);
    expect(await disabled()).toBe(true);
    expect(screen.getByText(/desde la empresa de pruebas/)).toBeTruthy();
    cleanup();

    api.listCertificates.mockResolvedValue([]);
    api.verifactuLabStatus.mockResolvedValue({ ...base, checks: ok, records: [pendiente] });
    render(<VerifactuLab />);
    expect(await disabled()).toBe(true);
    expect(api.verifactuSend).not.toHaveBeenCalled();
  });

  it('fuera de la empresa de pruebas ofrece crearla', async () => {
    api.listCertificates.mockResolvedValue([]);
    api.verifactuLabStatus.mockResolvedValue({ ...base, is_lab: false, info: null });
    render(<VerifactuLab onSwitched={() => {}} />);
    expect(await screen.findByRole('button', { name: 'Crear la empresa de pruebas' })).toBeTruthy();
    expect(
      (screen.getByRole('button', { name: 'Generar las facturas de prueba' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });
});

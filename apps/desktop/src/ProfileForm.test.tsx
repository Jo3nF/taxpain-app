import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { TaxpayerProfile } from './api';

const api = vi.hoisted(() => ({
  profilePresets: vi.fn(),
  ccaaList: vi.fn(),
  setProfile: vi.fn(),
  deducedProfile: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { ProfileForm } = await import('./ProfileForm');

const profile: TaxpayerProfile = {
  persona: 'Fisica',
  irpf_method: 'EstimacionDirecta',
  exento_pagos_fraccionados: false,
  iva: { periodicity: 'Trimestral', sii: false },
  retiene_trabajo_profesionales: false,
  retiene_alquileres: false,
  retiene_capital_mobiliario: false,
  retiene_no_residentes: false,
  gran_empresa: false,
  intracomunitarias: null,
  obligado_347: false,
  cierre_ejercicio: ['December', 31],
  pagos_fraccionados_is: false,
  ccaa: null,
  festivos_locales: [],
  bienes_extranjero: false,
  monedas_virtuales_extranjero: false,
  operaciones_vinculadas: false,
  renta_fraccionada: false,
  iae_desde: null,
  oss: false,
  ioss: false,
  opciones_censales: false,
  cotiza_reta: false,
  tiene_trabajadores: false,
};

describe('ProfileForm', () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('los festivos locales se guardan aunque se envíe con Intro sin salir del campo', async () => {
    api.profilePresets.mockResolvedValue([]);
    api.ccaaList.mockResolvedValue([]);
    api.setProfile.mockResolvedValue(undefined);
    const onSaved = vi.fn();
    render(<ProfileForm initial={profile} onSaved={onSaved} />);
    const input = screen.getByLabelText(/Festivos locales/);
    fireEvent.change(input, { target: { value: '2026-06-24, 2026-10-09, no-es-fecha' } });
    // Intro en el campo: el formulario se envía sin que haya habido blur.
    fireEvent.submit(input.closest('form') as HTMLFormElement);
    await vi.waitFor(() => expect(api.setProfile).toHaveBeenCalled());
    expect(api.setProfile.mock.calls[0]?.[0].festivos_locales).toEqual([
      '2026-06-24',
      '2026-10-09',
    ]);
    expect(onSaved).toHaveBeenCalled();
  });

  it('el autónomo marca RETA, 720 y Renta en dos plazos; la SL ve el 232 y el IAE', async () => {
    api.profilePresets.mockResolvedValue([]);
    api.ccaaList.mockResolvedValue([]);
    api.setProfile.mockResolvedValue(undefined);
    render(<ProfileForm initial={profile} onSaved={vi.fn()} />);
    expect(screen.queryByText(/\(232\)/)).toBeNull();
    expect(screen.queryByLabelText(/pagas el IAE/)).toBeNull();
    fireEvent.click(screen.getByLabelText(/cuota de autónomos/));
    fireEvent.click(screen.getByLabelText(/\(720\)/));
    fireEvent.click(screen.getByLabelText(/Renta en dos plazos/));
    fireEvent.submit(screen.getByText('Guardar perfil').closest('form') as HTMLFormElement);
    await vi.waitFor(() => expect(api.setProfile).toHaveBeenCalled());
    expect(api.setProfile.mock.calls[0]?.[0]).toMatchObject({
      cotiza_reta: true,
      bienes_extranjero: true,
      renta_fraccionada: true,
    });
    cleanup();
    render(<ProfileForm initial={{ ...profile, persona: 'Juridica' }} onSaved={vi.fn()} />);
    expect(screen.queryByLabelText(/cuota de autónomos/)).toBeNull();
    fireEvent.click(screen.getByLabelText(/autónomo societario/));
    fireEvent.click(screen.getByLabelText(/\(232\)/));
    fireEvent.change(screen.getByLabelText(/pagas el IAE/), { target: { value: '2027' } });
    fireEvent.submit(screen.getByText('Guardar perfil').closest('form') as HTMLFormElement);
    await vi.waitFor(() => expect(api.setProfile).toHaveBeenCalledTimes(2));
    expect(api.setProfile.mock.calls[1]?.[0]).toMatchObject({
      cotiza_reta: true,
      operaciones_vinculadas: true,
      iae_desde: 2027,
    });
  });

  it('sin perfil, «Rellenar con lo presentado» trae el deducido y se guarda al confirmar', async () => {
    api.profilePresets.mockResolvedValue([]);
    api.ccaaList.mockResolvedValue([]);
    api.setProfile.mockResolvedValue(undefined);
    api.deducedProfile.mockResolvedValue({
      perfil: {
        ...profile,
        persona: 'Juridica',
        irpf_method: null,
        retiene_trabajo_profesionales: true,
        pagos_fraccionados_is: true,
      },
      motivos: ['el NIF es de una sociedad', '303 trimestral (2024 a 2025)'],
    });
    const onSaved = vi.fn();
    render(<ProfileForm initial={null} onSaved={onSaved} />);
    fireEvent.click(screen.getByText('Rellenar con lo presentado'));
    await screen.findByText(/303 trimestral \(2024 a 2025\)/);
    expect(api.setProfile).not.toHaveBeenCalled();
    fireEvent.submit(screen.getByText('Guardar perfil').closest('form') as HTMLFormElement);
    await vi.waitFor(() => expect(api.setProfile).toHaveBeenCalled());
    expect(api.setProfile.mock.calls[0]?.[0]).toMatchObject({
      persona: 'Juridica',
      retiene_trabajo_profesionales: true,
      pagos_fraccionados_is: true,
    });
    expect(onSaved).toHaveBeenCalled();
  });

  it('con perfil guardado no sale el botón', () => {
    api.profilePresets.mockResolvedValue([]);
    api.ccaaList.mockResolvedValue([]);
    render(<ProfileForm initial={profile} onSaved={vi.fn()} />);
    expect(screen.queryByText('Rellenar con lo presentado')).toBeNull();
  });
});

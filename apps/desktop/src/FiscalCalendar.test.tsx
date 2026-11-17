import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CalendarView, TaxpayerProfile } from './api';

const autonomo: TaxpayerProfile = {
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

const view: CalendarView = {
  year: 2026,
  today: '2026-04-13',
  holidays_loaded: false,
  aeat_calendar_loaded: true,
  local_holidays: 1,
  deadlines: [
    {
      model: '303',
      title: 'Modelo 303',
      description: 'IVA: declaración del periodo',
      period: '1T',
      fiscal_year: 2026,
      starts_on: '2026-04-01',
      nominal_end: '2026-04-20',
      due_on: '2026-04-20',
      domiciliation_end: '2026-04-15',
      domiciliation_official: true,
      tentative: false,
      rule_ref: 'RIVA art. 71.4',
    },
    {
      model: '303',
      title: 'Modelo 303',
      description: 'IVA: declaración del periodo',
      period: '2T',
      fiscal_year: 2026,
      starts_on: '2026-07-01',
      nominal_end: '2026-07-20',
      due_on: '2026-07-20',
      domiciliation_end: '2026-07-15',
      domiciliation_official: false,
      tentative: false,
      rule_ref: 'RIVA art. 71.4',
    },
  ],
};

const api = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  getProfile: vi.fn(),
  setProfile: vi.fn(),
  profilePresets: vi.fn(),
  ccaaList: vi.fn(),
  fiscalCalendar: vi.fn(),
  exportIcal: vi.fn(),
  thresholdAlerts: vi.fn(async () => [] as unknown[]),
}));

vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));

const { FiscalCalendar } = await import('./FiscalCalendar');

describe('FiscalCalendar', () => {
  beforeEach(() => {
    api.profilePresets.mockResolvedValue([['Autónomo (estimación directa)', autonomo]]);
    api.fiscalCalendar.mockResolvedValue(view);
    api.ccaaList.mockResolvedValue([['ComunitatValenciana', 'Comunitat Valenciana']]);
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('sin perfil pide configurarlo y lo guarda', async () => {
    api.getProfile.mockResolvedValueOnce(null).mockResolvedValue(autonomo);
    api.setProfile.mockResolvedValue(undefined);
    render(<FiscalCalendar />);
    fireEvent.click(await screen.findByText('Autónomo (estimación directa)'));
    fireEvent.click(await screen.findByText(/Pago nóminas/));
    fireEvent.click(screen.getByText('Guardar perfil'));
    await vi.waitFor(() =>
      expect(api.setProfile).toHaveBeenCalledWith({
        ...autonomo,
        retiene_trabajo_profesionales: true,
      }),
    );
    expect(await screen.findByText('Modelo 303 · 1T 2026')).toBeTruthy();
  });

  it('el IVA mensual implica SII y desactiva el 347', async () => {
    api.getProfile.mockResolvedValueOnce(null).mockResolvedValue(autonomo);
    api.setProfile.mockResolvedValue(undefined);
    render(<FiscalCalendar />);
    fireEvent.click(await screen.findByText('Autónomo (estimación directa)'));
    const m347 = screen.getByLabelText(/3\.005,06/) as HTMLInputElement;
    fireEvent.click(m347);
    expect(m347.checked).toBe(true);
    fireEvent.change(screen.getByLabelText('Declaración de IVA'), {
      target: { value: 'Mensual-sii' },
    });
    expect(m347.disabled).toBe(true);
    expect(m347.checked).toBe(false);
    expect(screen.getByText(/con SII no se presenta el 347/)).toBeTruthy();
    fireEvent.click(screen.getByText('Guardar perfil'));
    await vi.waitFor(() =>
      expect(api.setProfile).toHaveBeenCalledWith({
        ...autonomo,
        iva: { periodicity: 'Mensual', sii: true },
        obligado_347: false,
      }),
    );
  });

  it('muestra vencimientos por mes, urgencia y aviso de festivos', async () => {
    api.getProfile.mockResolvedValue(autonomo);
    render(<FiscalCalendar />);
    expect(await screen.findByText('Modelo 303 · 1T 2026')).toBeTruthy();
    expect(screen.getByText('Faltan 7 días')).toBeTruthy();
    expect(screen.getByText('Abre el 01/07/2026')).toBeTruthy();
    expect(screen.getByText('Abril')).toBeTruthy();
    expect(screen.getByText(/domiciliación hasta el 15\/04\/2026/).textContent).not.toContain(
      'estimada',
    );
    expect(screen.getByText(/domiciliación hasta el 15\/07\/2026 \(estimada\)/)).toBeTruthy();
    expect(screen.getByRole('alert').textContent).toContain('festivos oficiales');
  });

  it('en 2027 avisa del calendario de la AEAT y de los festivos locales pendientes', async () => {
    api.getProfile.mockResolvedValue(autonomo);
    api.fiscalCalendar.mockResolvedValue({
      ...view,
      year: 2027,
      holidays_loaded: true,
      aeat_calendar_loaded: false,
      local_holidays: 0,
      deadlines: [],
    });
    render(<FiscalCalendar />);
    expect(await screen.findByText(/calendario del contribuyente de/)).toBeTruthy();
    expect(screen.getByText(/Festivos locales de .*pendientes de publicar/)).toBeTruthy();
    expect(screen.queryByText(/festivos oficiales/)).toBeNull();
  });

  it('muestra los avisos de umbral con lo que hay que hacer', async () => {
    api.getProfile.mockResolvedValue(autonomo);
    api.thresholdAlerts.mockResolvedValueOnce([
      {
        code: '347',
        severity: 'warning',
        title: 'Ya superas 3.005,06 € con 1 cliente o proveedor en 2026',
        detail: 'Según las facturas de TaxPain…',
        action: 'Marca la casilla del 347 en el perfil fiscal.',
        rule: 'RGAT arts. 31 a 33',
      },
    ]);
    render(<FiscalCalendar />);
    expect(await screen.findByText('Avisos según tus datos')).toBeTruthy();
    expect(screen.getByText(/Ya superas 3\.005,06 €/)).toBeTruthy();
    expect(screen.getByText(/Marca la casilla del 347/)).toBeTruthy();
  });
});

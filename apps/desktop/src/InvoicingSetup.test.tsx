import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { InvoicingSettings } from './api';

const api = vi.hoisted(() => ({
  detectDestinations: vi.fn(),
  saveInvoicingSettings: vi.fn(),
  newInstallationId: vi.fn(),
  companiesList: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { InvoicingSetup } = await import('./InvoicingSetup');

const settings: InvoicingSettings = {
  issuer: {
    id: { Nif: '12345678Z' },
    name: 'Carmen Española Española',
    address: {
      street: 'Calle 1',
      post_code: '46001',
      town: 'València',
      province: 'Valencia',
      country: 'ES',
    },
    person: { name: 'Carmen', first_surname: 'Española', second_surname: 'Española' },
  },
  default_series: 'A',
  rectifying_series: 'R',
  simplified_series: 'S',
  language: 'Es',
  default_vat: 2100,
  default_withholding: null,
  outputs: { pdf: true, facturae: false },
  destination: null,
  rounding: 'PerRate',
  verifactu: { enabled: true, installation_id: 'TP-x', production: false },
  footer: null,
};

describe('InvoicingSetup', () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('al dar de alta la empresa VERI*FACTU va desactivado y la serie de simplificadas es S', async () => {
    api.detectDestinations.mockResolvedValue([]);
    api.newInstallationId.mockResolvedValue('TP-nuevo');
    render(<InvoicingSetup initial={null} onSaved={vi.fn()} />);
    const vf = (await screen.findByRole('switch')) as HTMLInputElement;
    // Orden HAC/1177/2024 art. 20: sin envío real no hay QR ni leyenda; mejor desactivado.
    expect(vf.checked).toBe(false);
    expect(screen.getByText(/todavía no envía los registros a la AEAT real/)).toBeTruthy();
    expect((screen.getByLabelText('Serie de simplificadas') as HTMLInputElement).value).toBe('S');
    expect((screen.getByLabelText('Serie de rectificativas') as HTMLInputElement).value).toBe('R');
  });

  it('sin datos del emisor trae el nombre y el NIF de la empresa añadida', async () => {
    api.detectDestinations.mockResolvedValue([]);
    api.newInstallationId.mockResolvedValue('TP-nuevo');
    api.companiesList.mockResolvedValue([
      { id: 'a', name: 'Otra SL', nif: 'B98765431', active: false },
      { id: 'b', name: 'EMPRESA DEMO SL', nif: 'B12345674', active: true },
    ]);
    render(<InvoicingSetup initial={null} onSaved={vi.fn()} />);
    const nif = (await screen.findByDisplayValue(
      'B12345674',
      {},
      { timeout: 5000 },
    )) as HTMLInputElement;
    expect(nif).toBeTruthy();
    expect(screen.getByDisplayValue('EMPRESA DEMO SL')).toBeTruthy();
  });

  it('no pisa los datos del emisor que ya hay', async () => {
    api.detectDestinations.mockResolvedValue([]);
    api.companiesList.mockResolvedValue([
      { id: 'b', name: 'EMPRESA DEMO SL', nif: 'B12345674', active: true },
    ]);
    render(<InvoicingSetup initial={settings} onSaved={vi.fn()} />);
    await screen.findByDisplayValue('12345678Z');
    expect(screen.queryByDisplayValue('B12345674')).toBeNull();
  });

  it('la serie de simplificadas se guarda en mayúsculas', () => {
    api.detectDestinations.mockResolvedValue([]);
    api.saveInvoicingSettings.mockResolvedValue(undefined);
    render(<InvoicingSetup initial={settings} onSaved={vi.fn()} />);
    fireEvent.change(screen.getByLabelText('Serie de simplificadas'), { target: { value: 't' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    expect(api.saveInvoicingSettings.mock.calls[0]?.[0].simplified_series).toBe('T');
  });

  it('cada apellido va en su campo y no se guarda dos veces con un doble clic', async () => {
    api.detectDestinations.mockResolvedValue([]);
    let finish: () => void = () => {};
    api.saveInvoicingSettings.mockReturnValue(new Promise<void>((r) => (finish = r)));
    const onSaved = vi.fn();
    render(<InvoicingSetup initial={settings} onSaved={onSaved} />);
    expect((screen.getByLabelText('Primer apellido') as HTMLInputElement).value).toBe('Española');
    fireEvent.change(screen.getByLabelText('Primer apellido'), {
      target: { value: 'De la Fuente' },
    });
    fireEvent.change(screen.getByLabelText('Segundo apellido'), { target: { value: 'García' } });
    const save = screen.getByRole('button', { name: 'Guardar' }) as HTMLButtonElement;
    fireEvent.click(save);
    fireEvent.click(save);
    expect(api.saveInvoicingSettings).toHaveBeenCalledTimes(1);
    expect(save.disabled).toBe(true);
    expect(api.saveInvoicingSettings.mock.calls[0]?.[0].issuer.person).toEqual({
      name: 'Carmen',
      first_surname: 'De la Fuente',
      second_surname: 'García',
    });
    finish();
    await vi.waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(save.disabled).toBe(false);
  });

  it('al guardar dice «Guardado» y avisa al selector de empresas', async () => {
    api.detectDestinations.mockResolvedValue([]);
    api.saveInvoicingSettings.mockResolvedValue(undefined);
    const changed = vi.fn();
    window.addEventListener('taxpain:empresas', changed);
    render(<InvoicingSetup initial={settings} onSaved={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    expect((await screen.findByRole('status')).textContent).toBe('Guardado');
    expect(changed).toHaveBeenCalledTimes(1);
    window.removeEventListener('taxpain:empresas', changed);
    // Al cambiar algo deja de decirlo.
    fireEvent.change(screen.getByLabelText('Serie de simplificadas'), { target: { value: 'T' } });
    expect(screen.queryByRole('status')).toBeNull();
  });
});

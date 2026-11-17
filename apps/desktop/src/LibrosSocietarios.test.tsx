import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Minute, PartnersRegister } from './api';

const api = vi.hoisted(() => ({
  minutesList: vi.fn(),
  minuteTemplate: vi.fn(),
  minuteSave: vi.fn(),
  minuteApprove: vi.fn(),
  minuteDelete: vi.fn(),
  minuteReopen: vi.fn(),
  partnersRegister: vi.fn(),
  partnersRegisterApply: vi.fn(),
  shareMovementSave: vi.fn(),
  shareMovementDelete: vi.fn(),
  soleContracts: vi.fn(),
  soleContractProposals: vi.fn(),
  soleContractSave: vi.fn(),
  soleContractDelete: vi.fn(),
  corporateBookSave: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { LibrosSocietarios } = await import('./LibrosSocietarios');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const SOCIA = 'Carmen Española Española';

const draft: Minute = {
  id: 0,
  number: null,
  fiscal_year: 2026,
  date: '2026-12-01',
  place: 'València',
  kind: 'socio_unico',
  template: 'dividendo_cuenta',
  attendees: `${SOCIA} (NIF 12345678Z), socio único.`,
  chair: '',
  secretary: '',
  agreements: 'PRIMERO. Distribuir a cuenta 5.000,00 €.',
  status: 'borrador',
  approved_at: null,
};

describe('LibrosSocietarios', () => {
  it('prepara un dividendo a cuenta con su estado de liquidez y lo guarda en borrador', async () => {
    api.minutesList.mockResolvedValue([]);
    api.minuteTemplate.mockResolvedValue({
      minute: draft,
      warnings: ['Contabiliza el acuerdo en el Diario (557 a 526).'],
      liquidity: {
        date: '2026-11-30',
        period_start: '2026-01-01',
        result: 1_000_000,
        prior_losses: 0,
        legal_reserve: 60_000,
        tax_rate: 2500,
        estimated_tax: 250_000,
        already_distributed: 0,
        limit: 690_000,
        treasury: 1_300_000,
        short_term_debts: 0,
        amount: 500_000,
        treasury_after: 800_000,
      },
    });
    api.minuteSave.mockResolvedValue(1);
    render(<LibrosSocietarios year={2026} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Nueva acta' }));
    const form = screen.getByRole('form', { name: 'Nueva acta' });
    fireEvent.change(within(form).getByLabelText('Plantilla'), {
      target: { value: 'dividendo_cuenta' },
    });
    fireEvent.change(within(form).getByLabelText('Fecha del acta'), {
      target: { value: '2026-12-01' },
    });
    fireEvent.change(within(form).getByLabelText('Dividendo bruto (€)'), {
      target: { value: '5.000' },
    });
    fireEvent.change(within(form).getByLabelText('Fecha del estado contable'), {
      target: { value: '2026-11-30' },
    });
    fireEvent.change(within(form).getByLabelText('Tipo del IS estimado (%)'), {
      target: { value: '23' },
    });
    fireEvent.click(within(form).getByRole('button', { name: 'Preparar el borrador' }));
    expect(await screen.findByText('Máximo distribuible (art. 277.b)')).toBeTruthy();
    expect(screen.getByText('6.900,00 €')).toBeTruthy();
    expect(screen.getByText(/557 a 526/)).toBeTruthy();
    expect(api.minuteTemplate).toHaveBeenCalledWith(
      {
        template: 'dividendo_cuenta',
        amount: 500_000,
        payment_date: null,
        statement_date: '2026-11-30',
        tax_rate: 2300,
      },
      '2026-12-01',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Guardar borrador' }));
    expect(await screen.findByText('Borrador guardado.')).toBeTruthy();
    expect(api.minuteSave).toHaveBeenCalledWith(draft);
  });

  it('aprueba un borrador con confirmación', async () => {
    api.minutesList.mockResolvedValue([draft]);
    api.minuteApprove.mockResolvedValue(3);
    render(<LibrosSocietarios year={2026} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Aprobar' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, aprobar' }));
    expect(await screen.findByText('Acta aprobada con el nº 3.')).toBeTruthy();
    expect(api.minuteApprove).toHaveBeenCalledWith(0);
  });

  it('enseña la situación del registro de socios y pasa los porcentajes a la ficha', async () => {
    api.minutesList.mockResolvedValue([]);
    const r: PartnersRegister = {
      movements: [],
      situation: {
        count: 3000,
        capital: 300_000,
        holders: [
          {
            nif: '12345678Z',
            name: SOCIA,
            address: '',
            nationality: '',
            blocks: [{ from: 1, to: 3000, unit_nominal: 100, charges: [] }],
            count: 3000,
            nominal: 300_000,
            pct: 1_000_000,
          },
        ],
      },
      checks: [`${SOCIA}: la ficha de socios dice 50 % y el libro, 100 %.`],
      in_sync: false,
      capital_books: 300_000,
    };
    r.movements = [
      {
        id: 1,
        date: '2024-01-15',
        kind: 'constitucion',
        from_nif: '',
        from_name: '',
        to_nif: '12345678Z',
        to_name: SOCIA,
        to_address: '',
        to_nationality: '',
        number_from: 1,
        number_to: 3000,
        unit_nominal: 100,
        price: null,
        charge: '',
        title: 'Escritura de constitución',
        notary: '',
        protocol: '',
        title_date: null,
        notes: '',
      },
    ];
    api.partnersRegister.mockResolvedValue(r);
    api.partnersRegisterApply.mockResolvedValue(1);
    render(<LibrosSocietarios year={2026} />);
    fireEvent.click(screen.getByRole('button', { name: 'Registro de socios' }));
    expect(await screen.findByText(/Situación actual: 3\.?000 participaciones/)).toBeTruthy();
    expect(screen.getByText(/la ficha de socios dice 50 %/)).toBeTruthy();
    fireEvent.click(
      screen.getByRole('button', { name: 'Pasar los porcentajes a la ficha de socios' }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Sí, actualizar' }));
    expect(await screen.findByText('Un socio actualizado.')).toBeTruthy();
  });

  it('propone transcribir lo que ya sabe del socio único', async () => {
    api.minutesList.mockResolvedValue([]);
    api.soleContracts.mockResolvedValue([]);
    api.soleContractProposals.mockResolvedValue({
      sole_partner: ['12345678Z', SOCIA],
      proposals: [
        {
          id: 0,
          date: '2026-01-31',
          object: `Retribución de ${SOCIA}, socio único, por el ejercicio del cargo de administrador`,
          amount: 2_400_000,
          conditions: '12 recibos',
          document: '',
          source: 'nominas:2026:12345678Z',
        },
      ],
    });
    api.soleContractSave.mockResolvedValue(1);
    render(<LibrosSocietarios year={2026} />);
    fireEvent.click(screen.getByRole('button', { name: 'Contratos con el socio único' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Revisar y transcribir' }));
    const form = screen.getByRole('form', { name: 'Contrato con el socio único' });
    expect((within(form).getByLabelText('Importe (€)') as HTMLInputElement).value).toBe(
      '24.000,00',
    );
    fireEvent.click(within(form).getByRole('button', { name: 'Transcribir al libro' }));
    await vi.waitFor(() => expect(api.soleContractSave).toHaveBeenCalled());
    expect(api.soleContractSave.mock.calls[0]?.[0]).toMatchObject({
      amount: 2_400_000,
      source: 'nominas:2026:12345678Z',
    });
  });
});

import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  shareholdersList: vi.fn(),
  shareholderSave: vi.fn(),
  shareholderDelete: vi.fn(),
  dividendPropose: vi.fn(),
  dividendPay: vi.fn(),
  dividendPayments: vi.fn(),
  dividendUndo: vi.fn(),
  dividendAgreements: vi.fn(),
  dividendAgreementCheck: vi.fn(),
  dividendAgreementRecord: vi.fn(),
  dividendAgreementUndo: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Dividends, parsePct, quarterOf } = await import('./Dividends');

const ana = {
  id: 1,
  nif: '12345678Z',
  name: 'Socia Ana',
  address: '',
  province: '46',
  country: 'ES',
  pct: 750_000,
  legal_person: false,
  resident: true,
  exempt: false,
  active: true,
};

const proposal = {
  date: '2026-07-15',
  gross: 1_000_000,
  available: 1_000_000,
  lines: [
    {
      shareholder_id: 1,
      nif: '12345678Z',
      name: 'Socia Ana',
      province: '46',
      legal_person: false,
      resident: true,
      pct: 750_000,
      gross: 750_000,
      rate: 1900,
      withheld: 142_500,
      net: 607_500,
    },
  ],
  withheld: 142_500,
  net: 857_500,
  entry: [
    { account: '526', name: 'Dividendo activo a pagar', debit: 1_000_000, credit: 0 },
    { account: '4751', name: 'HP acreedora por retenciones', debit: 0, credit: 142_500 },
    { account: '572', name: 'Bancos', debit: 0, credit: 857_500 },
  ],
  warnings: ['Las participaciones de los socios activos suman 75,0000 %'],
};

beforeEach(() => {
  api.shareholdersList.mockResolvedValue([ana]);
  api.dividendPayments.mockResolvedValue([]);
  api.dividendPropose.mockResolvedValue(proposal);
  api.dividendPay.mockResolvedValue(1);
  api.shareholderSave.mockResolvedValue(2);
  api.dividendAgreements.mockResolvedValue([]);
  api.dividendAgreementRecord.mockResolvedValue({ id: 1, entry_id: 5, minute_id: 3 });
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Dividends', () => {
  it('convierte porcentajes y dice el trimestre del 123', () => {
    expect(parsePct('33,3333')).toBe(333_333);
    expect(parsePct('0')).toBeNull();
    expect(parsePct('101')).toBeNull();
    expect(quarterOf('2026-07-15')).toBe('3T de 2026 (hasta el 20 de octubre)');
    expect(quarterOf('2026-12-01')).toBe('4T de 2026 (hasta el 20 de enero de 2027)');
  });

  it('reparte, enseña el asiento y pide confirmación para contabilizar', async () => {
    render(<Dividends year={2025} />);
    await screen.findByText('Socia Ana');
    expect(screen.getByText(/suman 75 %/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Fecha de pago acordada'), {
      target: { value: '2026-07-15' },
    });
    fireEvent.change(screen.getByLabelText('Dividendo bruto total'), {
      target: { value: '10.000' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Calcular el reparto' }));
    await screen.findByText(/Pendiente de pago en la 526/);
    expect(api.dividendPropose).toHaveBeenCalledWith({
      date: '2026-07-15',
      gross: 1_000_000,
      treasury: '572',
      agreement: null,
    });
    expect(screen.getByText('4751')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Contabilizar el pago' }));
    expect(screen.getByText(/123 del 3T de 2026/)).toBeTruthy();
    expect(api.dividendPay).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Sí, contabilizar' }));
    await vi.waitFor(() => expect(api.dividendPay).toHaveBeenCalled());
    await screen.findByText(/Pago contabilizado/);
  });

  it('da de alta un socio con su participación', async () => {
    api.shareholdersList.mockResolvedValue([]);
    render(<Dividends year={2025} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Añadir socio' }));
    fireEvent.change(screen.getByLabelText('NIF'), { target: { value: 'B12345674' } });
    fireEvent.change(screen.getByLabelText('Apellidos y nombre o razón social'), {
      target: { value: 'Matriz SL' },
    });
    fireEvent.change(screen.getByLabelText('Participación (%)'), { target: { value: '25' } });
    fireEvent.change(screen.getByLabelText(/Provincia/), { target: { value: '46' } });
    fireEvent.click(screen.getByLabelText('Es una sociedad'));
    fireEvent.click(screen.getByLabelText(/exención del art\. 21 LIS/));
    fireEvent.click(screen.getByRole('button', { name: 'Guardar socio' }));
    await vi.waitFor(() =>
      expect(api.shareholderSave).toHaveBeenCalledWith(
        expect.objectContaining({ pct: 250_000, legal_person: true, exempt: true }),
      ),
    );
  });

  it('acuerda un dividendo a cuenta con el semáforo y crea el acta', async () => {
    const check = {
      kind: 'a_cuenta',
      date: '2026-10-15',
      amount: 2_000_000,
      account: '557',
      checks: [
        {
          code: 'limite_277',
          title: 'Límite del dividendo a cuenta',
          light: 'verde',
          detail: 'Cantidad máxima distribuible a cuenta: 30.000,00 €',
          norm: 'Art. 277.b LSC',
        },
        {
          code: 'entidad_patrimonial',
          title: 'Entidad patrimonial',
          light: 'ambar',
          detail: 'Puede ser entidad patrimonial',
          norm: 'Art. 5.2 LIS',
        },
      ],
      blocked: false,
      liquidity: {
        date: '2026-09-30',
        period_start: '2026-01-01',
        result: 4_000_000,
        prior_losses: 0,
        legal_reserve: 0,
        tax_rate: 2500,
        estimated_tax: 1_000_000,
        already_distributed: 0,
        limit: 3_000_000,
        treasury: 9_000_000,
        short_term_debts: 0,
        amount: 2_000_000,
        treasury_after: 7_000_000,
      },
      entry: [
        { account: '557', name: 'Dividendo activo a cuenta', debit: 2_000_000, credit: 0 },
        { account: '526', name: 'Dividendo activo a pagar', debit: 0, credit: 2_000_000 },
      ],
      lines: [],
      withheld: 380_000,
      organ: 'junta_universal',
      warnings: [],
    };
    api.dividendAgreementCheck.mockResolvedValue(check);
    render(<Dividends year={2025} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Acordar un dividendo' }));
    const form = screen.getByRole('region', { name: 'Acordar un dividendo' });
    const inForm = within(form);
    fireEvent.change(inForm.getByLabelText('Fecha del acuerdo'), {
      target: { value: '2026-10-15' },
    });
    fireEvent.change(inForm.getByLabelText('Dividendo bruto'), { target: { value: '20.000' } });
    fireEvent.change(inForm.getByLabelText('Fecha del estado contable'), {
      target: { value: '2026-09-30' },
    });
    fireEvent.click(inForm.getByRole('button', { name: 'Comprobar' }));
    await inForm.findByText(/Cantidad máxima distribuible a cuenta: 30/);
    expect(api.dividendAgreementCheck).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: 'a_cuenta',
        amount: 2_000_000,
        statement_date: '2026-09-30',
        tax_rate: 2500,
      }),
    );
    expect(inForm.getByText(/Revisar · Entidad patrimonial/)).toBeTruthy();
    expect(inForm.getByText(/Estado contable de liquidez/)).toBeTruthy();
    fireEvent.click(
      inForm.getByRole('button', { name: 'Contabilizar el acuerdo y crear el acta' }),
    );
    expect(api.dividendAgreementRecord).not.toHaveBeenCalled();
    fireEvent.click(inForm.getByRole('button', { name: 'Sí, acordar' }));
    await vi.waitFor(() => expect(api.dividendAgreementRecord).toHaveBeenCalled());
    await screen.findByText(/El acta está en borrador/);
  });

  it('con algo en rojo no deja acordar', async () => {
    api.dividendAgreementCheck.mockResolvedValue({
      kind: 'reservas',
      date: '2026-03-01',
      amount: 300_000,
      account: '113',
      checks: [
        {
          code: 'patrimonio_neto',
          title: 'Patrimonio neto frente al capital',
          light: 'rojo',
          detail: 'Tras el reparto el patrimonio neto sería 8.000,00 €',
          norm: 'Art. 273.2 LSC',
        },
      ],
      blocked: true,
      liquidity: null,
      entry: [],
      lines: [],
      withheld: 0,
      organ: 'socio_unico',
      warnings: [],
    });
    render(<Dividends year={2025} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Acordar un dividendo' }));
    const inForm = within(screen.getByRole('region', { name: 'Acordar un dividendo' }));
    fireEvent.change(inForm.getByLabelText('Clase de dividendo'), {
      target: { value: 'reservas' },
    });
    fireEvent.change(inForm.getByLabelText('Fecha del acuerdo'), {
      target: { value: '2026-03-01' },
    });
    fireEvent.change(inForm.getByLabelText('Dividendo bruto'), { target: { value: '3.000' } });
    fireEvent.click(inForm.getByRole('button', { name: 'Comprobar' }));
    await inForm.findByText(/No se cumple · Patrimonio neto/);
    expect(inForm.getByRole('alert').textContent).toMatch(/no se puede acordar/);
    expect(inForm.queryByRole('button', { name: /Contabilizar el acuerdo/ })).toBeNull();
  });

  it('lista los acuerdos con su acta y lo pendiente', async () => {
    api.dividendAgreements.mockResolvedValue([
      {
        id: 1,
        kind: 'a_cuenta',
        date: '2026-10-15',
        amount: 2_000_000,
        payment_date: null,
        account: '557',
        allocation_year: null,
        entry_id: 5,
        minute_id: 3,
        minute_number: null,
        minute_approved: false,
        paid: 0,
        pending: 2_000_000,
      },
    ]);
    render(<Dividends year={2025} />);
    await screen.findByText('Acta en borrador');
    expect(screen.getByRole('option', { name: /A cuenta/ })).toBeTruthy();
  });
});

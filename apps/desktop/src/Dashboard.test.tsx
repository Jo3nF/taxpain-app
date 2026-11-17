import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setDemoInvoke } from './api';
import { certificateAlert, Dashboard } from './Dashboard';

const data = {
  year: 2026,
  months: Array.from({ length: 12 }, (_, i) => ({
    month: i + 1,
    income: i === 2 ? 100_000 : 0,
    expenses: i === 4 ? 20_000 : 0,
  })),
  income: 100_000,
  expenses: 20_000,
  profit: 80_000,
  cash: 275_800,
  receivables: 159_000,
  payables: 0,
  estimates: [{ model: '303', quarter: 2, result: -10_500 }],
  pending: { received_to_review: 3, bank_unreconciled: 0, notices_pending: 1, verifactu_unsent: 0 },
};

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

describe('Dashboard', () => {
  it('muestra cifras, estimaciones y lleva a lo pendiente', async () => {
    const calls: string[] = [];
    setDemoInvoke(async (cmd, args) => {
      calls.push(`${cmd}:${JSON.stringify(args)}`);
      if (cmd === 'dashboard_get') return data;
      throw { message: 'sin perfil' };
    });
    const go = vi.fn();
    render(<Dashboard onNavigate={go} now={new Date(2026, 4, 20)} />);
    expect(await screen.findByText('Tu negocio en 2026')).toBeTruthy();
    expect(calls).toContain('dashboard_get:{"year":2026,"month":5}');
    expect(screen.getByText('1.000,00 €', { selector: 'p' })).toBeTruthy();
    expect(screen.getByText('105,00 € a tu favor')).toBeTruthy();
    // La tabla accesible sólo llega hasta el mes en curso.
    expect(screen.getAllByRole('row')).toHaveLength(6);
    fireEvent.click(screen.getByRole('button', { name: '3 facturas recibidas por revisar' }));
    expect(go).toHaveBeenCalledWith('recibidas');
    expect(
      screen.getByRole('button', { name: '1 notificación de la AEAT sin abrir' }),
    ).toBeTruthy();
  });

  it('enseña arriba lo urgente de la última revisión de la carpeta', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'dashboard_get') return data;
      if (cmd === 'folder_review_last')
        return {
          hallazgos: [
            {
              codigo: 'cruces.pago_no_esta',
              area: 'bancos',
              gravedad: 'aviso',
              responsable: 'empresa',
              ejercicio: 2026,
              titulo: 'No se ve el pago del 111 del 2T 2026 en el banco',
              detalle: '',
              ficheros: [],
              importe: null,
              urgente: {
                en_juego: 100_000,
                vencio: '2026-07-20',
                porcentaje_hoy: 5,
                recargo_hoy: 5_000,
                siguiente_desde: null,
                siguiente_porcentaje: 10,
                siguiente_recargo: 10_000,
                nota: 'Periodo ejecutivo.',
              },
            },
          ],
        };
      throw { message: 'no' };
    });
    const go = vi.fn();
    render(<Dashboard onNavigate={go} now={new Date(2026, 10, 16)} />);
    const bloque = await screen.findByRole('region', { name: /Urgente: puede haber deuda/ });
    expect(bloque.textContent).toContain('No se ve el pago del 111 del 2T 2026 en el banco');
    fireEvent.click(screen.getByRole('button', { name: 'Ver la revisión de la carpeta' }));
    expect(go).toHaveBeenCalledWith('importar');
  });

  it('en diciembre avisa de lo que hay que revisar del año nuevo', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'dashboard_get') return data;
      if (cmd === 'aviso_cambio_ejercicio')
        return {
          year: 2027,
          comprobaciones: [
            {
              clave: 'festivos_nacionales',
              titulo: 'Festivos nacionales de 2027',
              estado: 'listo',
              detalle: '9 días, de la resolución de fiestas laborales del BOE.',
            },
            {
              clave: 'calendario_aeat',
              titulo: 'Calendario del contribuyente 2027 (AEAT)',
              estado: 'supuesto',
              detalle: 'Aún no está en TaxPain.',
            },
            {
              clave: 'festivos_locales',
              titulo: 'Festivos locales de 2027',
              estado: 'pendiente',
              detalle: 'Añádelos cuando se publiquen.',
            },
          ],
        };
      throw { message: 'no' };
    });
    render(<Dashboard onNavigate={vi.fn()} now={new Date(2026, 11, 2)} />);
    const card = await screen.findByRole('region', {
      name: 'Cambio de año: qué revisar para 2027',
    });
    expect(card.textContent).toContain('Calendario del contribuyente 2027 (AEAT)');
    expect(card.textContent).toContain('Se usa el del año anterior');
    expect(card.textContent).toContain('Pendiente de publicar');
    expect(card.textContent).toContain('Ya cargado para 2027 (1)');
  });

  it('fuera de diciembre y enero no hay aviso de cambio de año', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'dashboard_get') return data;
      if (cmd === 'aviso_cambio_ejercicio') return null;
      throw { message: 'no' };
    });
    render(<Dashboard onNavigate={vi.fn()} now={new Date(2026, 4, 20)} />);
    expect(await screen.findByText('Tu negocio en 2026')).toBeTruthy();
    expect(screen.queryByText(/Cambio de año/)).toBeNull();
  });

  it('recuerda hacer una copia de seguridad', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'dashboard_get') return data;
      if (cmd === 'backup_status')
        return { remind: true, days_since: null, settings: { auto: false }, last_error: null };
      if (cmd === 'list_certificates') return [{ valid_to: '2026-06-01T00:00:00Z' }];
      throw { message: 'sin datos' };
    });
    const go = vi.fn();
    render(<Dashboard onNavigate={go} now={new Date(2026, 4, 20)} />);
    fireEvent.click(
      await screen.findByRole('button', { name: 'Haz tu primera copia de seguridad' }),
    );
    expect(go).toHaveBeenCalledWith('ajustes');
    expect(
      await screen.findByRole('button', {
        name: 'Tu certificado caduca el 01/06/2026: renuévalo en la Sede de la FNMT (sólo en los 60 días anteriores)',
      }),
    ).toBeTruthy();
  });

  it('avisa si la copia automática falla aunque la última sea reciente', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'dashboard_get') return data;
      if (cmd === 'backup_status')
        return {
          remind: false,
          days_since: 2,
          settings: { auto: true },
          last_error: { at: '2026-05-20T08:00:00Z', message: 'no existe la carpeta' },
        };
      throw { message: 'sin datos' };
    });
    render(<Dashboard onNavigate={() => {}} now={new Date(2026, 4, 20)} />);
    expect(
      await screen.findByRole('button', {
        name: 'La copia de seguridad automática no se ha podido hacer',
      }),
    ).toBeTruthy();
  });

  it('vencidas sin abonos y presupuestos aceptados de cualquier año', async () => {
    const calls: string[] = [];
    setDemoInvoke(async (cmd) => {
      calls.push(cmd);
      if (cmd === 'dashboard_get') return data;
      if (cmd === 'dashboard_receivables')
        return {
          overdue_invoices: 2,
          overdue_amount: 150_000,
          accepted_quotes: 1,
          accepted_total: 242_000,
        };
      throw { message: 'sin datos' };
    });
    const go = vi.fn();
    render(<Dashboard onNavigate={go} now={new Date(2026, 4, 20)} />);
    fireEvent.click(
      await screen.findByRole('button', {
        name: '2 facturas vencidas sin cobrar (1.500,00 €)',
      }),
    );
    expect(go).toHaveBeenCalledWith('contabilidad', 'cartera');
    expect(
      screen.getByRole('button', { name: '1 presupuesto aceptado por facturar (2.420,00 €)' }),
    ).toBeTruthy();
    // Ya no se calcula aquí con la cartera ni con los presupuestos de este año.
    expect(calls).not.toContain('cartera_list');
    expect(calls).not.toContain('quotes_list');
  });

  it('el 202 de más de 6 millones sale sin importe', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'dashboard_get')
        return {
          ...data,
          estimates: [
            {
              model: '202',
              quarter: 2,
              period: '2P',
              result: 0,
              note: 'más de 6 millones de cifra de negocios: se calcula sobre la base del periodo (art. 40.3 LIS), en la Sede',
            },
          ],
        };
      throw { message: 'sin datos' };
    });
    render(<Dashboard onNavigate={() => {}} now={new Date(2026, 9, 20)} />);
    const note = await screen.findByText(/Sin estimar: más de 6 millones/);
    expect(note.closest('li')?.textContent).not.toContain('€');
  });

  it('sin contabilidad del año: el aviso en vez de ceros y lo presentado a la AEAT', async () => {
    const empty = {
      ...data,
      months: data.months.map((m) => ({ ...m, income: 0, expenses: 0 })),
      income: 0,
      expenses: 0,
      profit: 0,
      cash: 0,
      receivables: 0,
      payables: 0,
      estimates: [{ model: '303', quarter: 4, result: 0 }],
      sin_contabilidad: true,
      presentado: [
        {
          id: 'iva_devengado',
          titulo: 'IVA devengado',
          valor: 310_000,
          fuente: 'suma de los 303 [27]',
          periodos: '1T a 3T',
        },
        {
          id: 'iva_a_compensar',
          titulo: 'IVA a compensar o devolver',
          valor: 5_000,
          fuente: 'suma de los 303 [71] negativos',
          periodos: '3T',
        },
        {
          id: 'pagos_fraccionados',
          titulo: 'Pagos fraccionados del Impuesto sobre Sociedades',
          valor: 120_000,
          fuente: 'suma de los 202 [03]',
          periodos: '1P y 2P',
        },
      ],
    };
    setDemoInvoke(async (cmd) => {
      if (cmd === 'dashboard_get') return empty;
      if (cmd === 'fiscal_calendar')
        return {
          today: '2026-10-28',
          deadlines: [
            {
              model: '303',
              period: '4T',
              fiscal_year: 2026,
              title: 'IVA trimestral',
              due_on: '2027-01-30',
            },
          ],
        };
      throw { message: 'sin datos' };
    });
    const go = vi.fn();
    render(<Dashboard onNavigate={go} now={new Date(2026, 9, 28)} />);
    expect(
      await screen.findByRole('heading', { name: 'TaxPain no tiene la contabilidad de 2026' }),
    ).toBeTruthy();
    // Ni las seis cifras a cero ni el gráfico vacío ni la estimación a cero.
    expect(screen.queryByText('Ingresos')).toBeNull();
    expect(screen.queryByText('Pendiente de cobro')).toBeNull();
    expect(screen.queryByText('Ingresos y gastos por mes')).toBeNull();
    expect(screen.queryByText('0,00 €')).toBeNull();
    expect(
      screen.getByText('Sin la contabilidad de 2026, TaxPain no puede estimarlos.'),
    ).toBeTruthy();
    // El próximo plazo sigue.
    expect(await screen.findByText('Próximo plazo')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Ir a Importar' }));
    expect(go).toHaveBeenCalledWith('importar');

    expect(
      screen.getByRole('heading', { name: 'Según lo presentado a la AEAT en 2026' }),
    ).toBeTruthy();
    expect(screen.getByText('3.100,00 €')).toBeTruthy();
    expect(screen.getByText('IVA a compensar o devolver')).toBeTruthy();
    expect(screen.getByText('50,00 €')).toBeTruthy();
    expect(screen.getByText('suma de los 202 [03] · 1P y 2P')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Ver la evolución' }));
    expect(go).toHaveBeenCalledWith('evolucion');
  });

  it('sin contabilidad ni declaraciones leídas: sólo el aviso', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'dashboard_get') return { ...data, sin_contabilidad: true, presentado: [] };
      throw { message: 'sin datos' };
    });
    render(<Dashboard onNavigate={() => {}} now={new Date(2026, 9, 28)} />);
    expect(
      await screen.findByRole('heading', { name: 'TaxPain no tiene la contabilidad de 2026' }),
    ).toBeTruthy();
    expect(screen.queryByText(/Según lo presentado a la AEAT/)).toBeNull();
    expect(screen.queryByText('Ingresos y gastos por mes')).toBeNull();
  });

  it('con contabilidad: las cifras de siempre, sin el aviso', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'dashboard_get') return { ...data, sin_contabilidad: false, presentado: [] };
      throw { message: 'sin datos' };
    });
    render(<Dashboard onNavigate={() => {}} now={new Date(2026, 4, 20)} />);
    expect(await screen.findByText('Pendiente de cobro')).toBeTruthy();
    expect(screen.getAllByText('Ingresos y gastos por mes').length).toBeGreaterThan(0);
    expect(screen.queryByText(/no tiene la contabilidad/)).toBeNull();
    expect(screen.queryByText(/Según lo presentado a la AEAT/)).toBeNull();
  });

  it('muestra el error si no se puede cargar', async () => {
    setDemoInvoke(async () => {
      throw { message: 'almacén bloqueado' };
    });
    render(<Dashboard onNavigate={() => {}} />);
    expect((await screen.findByRole('alert')).textContent).toBe('almacén bloqueado');
  });
});

describe('certificateAlert', () => {
  const cert = (
    valid_to: string,
    holder_nif = '12345678Z',
    entity_nif: string | null = null,
    kind: 'persona_fisica' | 'representante_pj' | 'otro' = entity_nif
      ? 'representante_pj'
      : 'persona_fisica',
  ) => ({
    valid_to,
    holder_nif,
    entity_nif,
    kind,
  });

  it('avisa si el único certificado caduca pronto o ya caducó', () => {
    expect(certificateAlert([cert('2026-06-01T00:00:00Z')], '2026-05-20')).toBe(
      'Tu certificado caduca el 01/06/2026: renuévalo en la Sede de la FNMT (sólo en los 60 días anteriores)',
    );
    expect(certificateAlert([cert('2026-05-01T00:00:00Z')], '2026-05-20')).toBe(
      'Tu certificado caducó el 01/05/2026: renuévalo en la Sede de la FNMT (sólo en los 60 días anteriores)',
    );
    expect(certificateAlert([cert('2027-05-01T00:00:00Z')], '2026-05-20')).toBeNull();
  });

  it('avisa con 60 días, no 30, y el de representante pide uno nuevo', () => {
    // A 50 días ya avisa (la FNMT abre la renovación 60 días antes); a 70, todavía no.
    expect(certificateAlert([cert('2026-07-09T00:00:00Z')], '2026-05-20')).toMatch(/renuévalo/);
    expect(certificateAlert([cert('2026-07-29T00:00:00Z')], '2026-05-20')).toBeNull();
    // El de representante de la sociedad no se renueva: hay que solicitar uno nuevo.
    expect(
      certificateAlert([cert('2026-06-10T00:00:00Z', '12345678Z', 'B12345674')], '2026-05-20'),
    ).toBe(
      'El certificado de representante caduca el 10/06/2026: no se renueva, solicita uno nuevo en la FNMT (lleva días)',
    );
  });

  it('renovado: el viejo que sigue guardado no avisa', () => {
    expect(
      certificateAlert([cert('2026-05-01T00:00:00Z'), cert('2030-05-01T00:00:00Z')], '2026-05-20'),
    ).toBeNull();
    // El de otro titular (la sociedad) sí cuenta aparte.
    expect(
      certificateAlert(
        [
          cert('2026-05-01T00:00:00Z'),
          cert('2030-05-01T00:00:00Z'),
          cert('2026-06-10T00:00:00Z', '12345678Z', 'B12345674'),
        ],
        '2026-05-20',
      ),
    ).toMatch(/^El certificado de representante caduca el 10\/06\/2026/);
  });
});

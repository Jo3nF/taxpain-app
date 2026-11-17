import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  modelDraft: vi.fn(),
  modelExport: vi.fn(),
  modelSettings: vi.fn(),
  saveModelSettings: vi.fn(),
  pickFolder: vi.fn(),
  listCertificates: vi.fn(),
  modelPrevalidate: vi.fn(),
  modelFileTesting: vi.fn(),
  filedReturns: vi.fn(),
  getProfile: vi.fn(),
  aeatCheckReport: vi.fn(),
  aeatCheckSummaries: vi.fn(),
  aeatCheckRunning: vi.fn(),
  assistantConversations: vi.fn(),
  assistantStatus: vi.fn(),
  assistantTopicOpen: vi.fn(),
  rectificativaAnterior: vi.fn(),
  rectificativaPreparar: vi.fn(),
  rectificativaExportar: vi.fn(),
  modelMonthly: vi.fn(),
  siiEstado: vi.fn(),
  siiPlazos: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Models, isRates, regimeLabels } = await import('./Models');

const draft303 = {
  model: '303',
  year: 2026,
  quarter: 3,
  boxes: [
    [7, 600000],
    [9, 126000],
    [46, 86100],
    [65, 10000],
    [71, 75600],
  ],
  payees: [],
  details: [],
  declaration: 'I',
  result: 75600,
  warnings: ['A1: tipo de IVA del 5 % sin casilla en este cálculo; revísalo a mano'],
  file_name: '12345678Z_2026_3T_303.txt',
  file: '<T303…',
};

describe('Models', () => {
  beforeEach(() => {
    api.filedReturns.mockResolvedValue([]);
    api.getProfile.mockResolvedValue(null);
    // «Comprobar lo presentado», debajo del borrador: sin nada comprobado.
    api.aeatCheckReport.mockImplementation(async (year: number) => ({
      year,
      nif: '',
      checked_at: null,
      without_profile: false,
      rows: [],
      none_filed: [],
      errors: [],
      empty_answers: [],
      unavailable: [],
    }));
    api.aeatCheckSummaries.mockResolvedValue([]);
    api.rectificativaAnterior.mockResolvedValue(null);
    api.modelMonthly.mockResolvedValue(null);
    api.siiEstado.mockResolvedValue({
      obligatorio: false,
      voluntario: false,
      motivos: [],
      avisos: [],
      formato_sii: false,
      norma: 'RIVA',
    });
    api.siiPlazos.mockResolvedValue([]);
    api.aeatCheckRunning.mockResolvedValue(false);
    api.assistantConversations.mockResolvedValue([]);
    api.assistantStatus.mockResolvedValue({
      settings: { provider: { kind: 'Anthropic', model: 'claude-opus-5-5' }, consent: true },
      has_api_key: true,
      remote: true,
    });
    api.assistantTopicOpen.mockResolvedValue({
      header: {
        group: 'Modelos · 2026',
        title: 'Borrador del modelo 303 · 3T 2026',
        facts: [['Resultado', '756,00 € · a ingresar']],
        intro:
          'Con tus facturas y tu contabilidad, el borrador del modelo 303 del 3T 2026 sale 756,00 €.',
        suggestions: ['¿Por qué me sale a ingresar?'],
        notice: null,
      },
      conversation: null,
      changed: false,
    });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('el borrador abre su conversación con la IA', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    render(<Models />);
    const ask = await screen.findByRole('button', {
      name: 'Preguntar a la IA sobre este borrador: modelo 303 · 3T 2026',
    });
    fireEvent.click(ask);
    const dialog = await screen.findByRole('dialog', {
      name: 'Conversación con la IA: Borrador del modelo 303 · 3T 2026',
    });
    expect(api.assistantTopicOpen).toHaveBeenCalledWith({
      kind: 'modelo',
      model: '303',
      year: 2026,
      period: 3,
    });
    fireEvent.keyDown(dialog, { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(document.activeElement).toBe(ask);
  });

  it('con una conversación guardada dice «Seguir con la IA sobre este borrador»', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.assistantConversations.mockResolvedValue([
      {
        id: 9,
        topic: { kind: 'modelo', model: '303', year: 2026, period: 3 },
        title: 'Borrador del modelo 303 · 3T 2026',
        updated_at: '2026-10-30T10:00:00Z',
        questions: 1,
        materials: 0,
      },
    ]);
    render(<Models />);
    const b = await screen.findByRole('button', {
      name: 'Seguir con la IA sobre este borrador: modelo 303 · 3T 2026, 1 pregunta',
    });
    expect(b.textContent).toBe('Seguir con la IA sobre este borrador1');
  });

  it('muestra el borrador casilla a casilla con avisos y el resultado', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    render(<Models />);
    expect((await screen.findAllByText('756,00 €')).length).toBe(2); // resultado y casilla [71]
    expect(screen.getByText('A ingresar')).toBeTruthy();
    expect(screen.getByText('Cuota al 21 %')).toBeTruthy();
    expect(screen.getByText('[07]')).toBeTruthy();
    expect(screen.getByText('100,00 %')).toBeTruthy();
    expect(screen.getByRole('alert').textContent).toContain('sin casilla');
  });

  it('cambia de modelo y guarda el fichero en la carpeta elegida', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.pickFolder.mockResolvedValue('/tmp/x');
    api.modelExport.mockResolvedValue('/tmp/x/12345678Z_2026_3T_303.txt');
    render(<Models />);
    await screen.findAllByText('756,00 €');
    fireEvent.change(screen.getByLabelText('Modelo'), { target: { value: '130' } });
    expect(api.modelDraft).toHaveBeenLastCalledWith('130', expect.any(Number), expect.any(Number));
    fireEvent.click(await screen.findByRole('button', { name: 'Guardar fichero para la Sede…' }));
    expect(await screen.findByText(/Fichero guardado en/)).toBeTruthy();
    expect(api.modelExport).toHaveBeenCalledWith(
      '130',
      expect.any(Number),
      expect.any(Number),
      '/tmp/x',
    );
  });

  it('en un periodo presentado prepara la rectificativa y guarda su fichero', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.rectificativaAnterior.mockResolvedValue({
      fuente: 'aeat',
      justificante: '3030000000017',
      presentada: '2026-10-20',
      declaracion: 'I',
      casillas: [
        [9, 115000],
        [71, 65000],
      ],
      resultado: 65000,
    });
    api.rectificativaPreparar.mockResolvedValue({
      model: '303',
      year: 2026,
      quarter: 3,
      period: '3T',
      cauce: 'rectificativa',
      cauce_texto: 'Autoliquidación rectificativa: sale 106,00 € más a ingresar.',
      anterior: null,
      justificante: '3030000000017',
      filas: [
        { casilla: 9, presentado: 115000, ahora: 126000, diferencia: 11000 },
        { casilla: 70, presentado: 0, ahora: 65000, diferencia: 65000 },
        { casilla: 71, presentado: 65000, ahora: 10600, diferencia: -54400 },
      ],
      ingresado_antes: 65000,
      devuelto_antes: 0,
      total: 75600,
      resultado: 10600,
      a_ingresar: 10600,
      a_devolver: 0,
      a_compensar: 0,
      plazo: '2026-10-20',
      fecha: '2026-12-21',
      recargo: {
        porcentaje: 3,
        importe: 318,
        meses: 2,
        intereses: 0,
        intereses_desde: null,
        reducido: 239,
      },
      intereses_devolucion: 0,
      total_a_pagar: 10918,
      declaracion: 'I',
      avisos: ['Fuera de plazo y sin requerimiento: recargo del 3 %.'],
      file_name: '12345678Z_2026_3T_303_rectificativa.txt',
      file: '<T303…',
      calendario: ['Desde el 1 de abril de 2025', 'Art. 120.3 LGT. TODO(VERIFY)'],
    });
    api.pickFolder.mockResolvedValue('/tmp/x');
    api.rectificativaExportar.mockResolvedValue('/tmp/x/12345678Z_2026_3T_303_rectificativa.txt');
    render(<Models />);
    expect(await screen.findByText(/justificante 3030000000017/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Preparar rectificativa…' }));
    const panel = await screen.findByRole('region', { name: /Rectificar el 303/ });
    expect(await within(panel).findByText(/106,00 € más a ingresar/)).toBeTruthy();
    expect(api.rectificativaPreparar).toHaveBeenCalledWith(
      '303',
      expect.any(Number),
      expect.any(Number),
      expect.objectContaining({ justificante: '3030000000017' }),
    );
    // Lo presentado al lado de lo de ahora, con la diferencia y el recargo.
    expect(within(panel).getByText('1.150,00 €')).toBeTruthy();
    expect(within(panel).getByText('110,00 €')).toBeTruthy();
    expect(within(panel).getByText(/3 % · 3,18 €/)).toBeTruthy();
    expect(within(panel).getByText('109,18 €')).toBeTruthy();
    fireEvent.click(
      within(panel).getByRole('button', { name: 'Guardar el fichero de la rectificativa…' }),
    );
    expect(
      await within(panel).findByText(/Fichero guardado en.*Nada se ha presentado/),
    ).toBeTruthy();
    expect(api.rectificativaExportar.mock.calls[0]?.[4]).toBe('/tmp/x');
    // Cerrar vuelve a ofrecerla.
    fireEvent.click(within(panel).getByRole('button', { name: 'Cerrar' }));
    expect(screen.getByRole('button', { name: 'Preparar rectificativa…' })).toBeTruthy();
  });

  it('abre con la rectificativa del periodo que pide Recibidas', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.rectificativaAnterior.mockResolvedValue({
      fuente: 'gestor',
      justificante: null,
      presentada: null,
      declaracion: null,
      casillas: [[71, 65000]],
      resultado: 65000,
    });
    api.rectificativaPreparar.mockRejectedValue({ message: 'demo' });
    render(<Models rectificar={{ modelo: '303', ejercicio: 2025, trimestre: 2 }} />);
    expect(
      await screen.findByRole('region', { name: /Rectificar el 303 de 2T 2025/ }),
    ).toBeTruthy();
    expect(api.modelDraft).toHaveBeenCalledWith('303', 2025, 2);
    expect(api.rectificativaAnterior).toHaveBeenCalledWith('303', 2025, 2);
  });

  it('el 349 lista los operadores y su total', async () => {
    api.modelDraft.mockResolvedValue({
      ...draft303,
      model: '349',
      boxes: [],
      details: [['Kunde GmbH · DE123456789 · clave E', 150000]],
      declaration: '',
      result: 150000,
      warnings: [],
    });
    render(<Models />);
    expect(await screen.findByText('Kunde GmbH · DE123456789 · clave E')).toBeTruthy();
    expect(screen.getByText('Total de operaciones intracomunitarias')).toBeTruthy();
  });

  it('con el IVA mensual ofrece los meses y pide el borrador del mes', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.modelMonthly.mockResolvedValue(true);
    render(<Models />);
    const periodo = await screen.findByLabelText('Periodo');
    fireEvent.change(periodo, { target: { value: '107' } });
    await waitFor(() =>
      expect(api.modelDraft).toHaveBeenLastCalledWith('303', expect.any(Number), 107),
    );
    expect(screen.getByRole('option', { name: 'Julio' })).toBeTruthy();
  });

  it('avisa del SII cuando es obligatorio, con el plazo de cada factura', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.siiEstado.mockResolvedValue({
      obligatorio: true,
      voluntario: false,
      motivos: ['Gran empresa: IVA mensual y SII.'],
      avisos: [],
      formato_sii: false,
      norma: 'RIVA',
    });
    api.siiPlazos.mockResolvedValue([
      {
        libro: 'emitida',
        numero: 'F-1',
        nif: 'A22222228',
        nombre: 'Cliente Demo SA',
        fecha: '2026-11-16',
        limite: '2026-11-20',
        dias: 3,
        aproximado: false,
      },
    ]);
    render(<Models />);
    expect(
      await screen.findByText('Tienes que llevar los libros registro del IVA por el SII.'),
    ).toBeTruthy();
    expect(await screen.findByText('F-1 · Cliente Demo SA')).toBeTruthy();
  });

  it('el 390 es anual: sin trimestre', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    render(<Models />);
    await screen.findAllByText('756,00 €');
    fireEvent.change(screen.getByLabelText('Modelo'), { target: { value: '390' } });
    expect(api.modelDraft).toHaveBeenLastCalledWith('390', expect.any(Number), 0);
    expect(screen.queryByLabelText('Trimestre')).toBeNull();
  });

  it('prevalida sólo tras confirmar y muestra lo que dice la AEAT', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.listCertificates.mockResolvedValue([
      {
        fingerprint_sha256: 'abc',
        holder_name: 'CARMEN',
        holder_nif: '12345678Z',
        subject_dn: 'x',
      },
    ]);
    api.modelPrevalidate.mockResolvedValue({
      valid: true,
      messages: ['Compruebe la casilla 28'],
      pdf_path: null,
    });
    render(<Models />);
    fireEvent.click(await screen.findByRole('button', { name: 'Prevalidar con la AEAT…' }));
    expect(api.modelPrevalidate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByLabelText('Guardar el borrador oficial en PDF'));
    fireEvent.click(await screen.findByRole('button', { name: 'Sí, prevalidar' }));
    expect(await screen.findByText('La AEAT no ha encontrado errores.')).toBeTruthy();
    expect(api.modelPrevalidate).toHaveBeenCalledWith(
      '303',
      expect.any(Number),
      expect.any(Number),
      'abc',
      null,
    );
    expect(screen.getByText('Compruebe la casilla 28')).toBeTruthy();
  });

  it('presentar exige conformidad y NRC si sale a ingresar; muestra el CSV', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.listCertificates.mockResolvedValue([
      {
        fingerprint_sha256: 'abc',
        holder_name: 'CARMEN',
        holder_nif: '12345678Z',
        subject_dn: 'x',
      },
    ]);
    api.modelFileTesting.mockResolvedValue({
      accepted: true,
      errors: [],
      receipt: {
        csv: 'AAAAAAAAAAAAAAAA',
        receipt_number: '3030000000001',
        date: '2026-10-14',
        time: '10:00',
        pdf_url: 'https://www2.agenciatributaria.gob.es/x',
        notices: [],
        warnings: [],
      },
    });
    render(<Models />);
    fireEvent.click(
      await screen.findByRole('button', { name: 'Presentar en el entorno de pruebas…' }),
    );
    const send = await screen.findByRole('button', { name: 'Presentar (pruebas)' });
    expect((send as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(
      screen.getByLabelText('He revisado el borrador y estoy conforme con sus datos'),
    );
    expect((send as HTMLButtonElement).disabled).toBe(true); // falta el NRC
    fireEvent.change(screen.getByLabelText('NRC del pago (te lo da tu banco al pagar)'), {
      target: { value: '3030000000000000000001' },
    });
    expect((send as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(send);
    expect(await screen.findByText(/CSV AAAAAAAAAAAAAAAA/)).toBeTruthy();
    expect(api.modelFileTesting).toHaveBeenCalledWith(
      '303',
      2026,
      3,
      'abc',
      '3030000000000000000001',
      draft303.file,
      false,
    );
    // Aceptada: no se puede volver a presentar desde el mismo diálogo.
    expect((send as HTMLButtonElement).disabled).toBe(true);
  });

  it('el diálogo de prevalidar enfoca su primer control, se cierra con Esc y nombra la sociedad', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.listCertificates.mockResolvedValue([
      {
        fingerprint_sha256: 'abc',
        holder_name: 'CARMEN',
        holder_nif: '12345678Z',
        entity_name: 'EMPRESA SL',
        entity_nif: 'B12345674',
        subject_dn: 'x',
      },
    ]);
    render(<Models />);
    fireEvent.click(await screen.findByRole('button', { name: 'Prevalidar con la AEAT…' }));
    const select = await screen.findByLabelText('Certificado');
    expect(select.textContent).toBe('EMPRESA SL · B12345674');
    await vi.waitFor(() => expect(document.activeElement).toBe(select));
    fireEvent.keyDown(select, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(
      screen.getByRole('button', { name: 'Prevalidar con la AEAT…' }),
    );
  });

  it('avisa si no puede leer las presentaciones', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.filedReturns.mockRejectedValue({ message: 'almacén bloqueado' });
    render(<Models />);
    expect(
      await screen.findByText(/No se pudieron leer las presentaciones.*almacén bloqueado/),
    ).toBeTruthy();
  });

  it('el cotejo con los datos fiscales de Renta sólo sale a las personas físicas', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.getProfile.mockResolvedValue({ persona: 'Juridica' });
    render(<Models />);
    await screen.findByText('A ingresar');
    expect(screen.queryByText('Cotejo con los datos fiscales de la AEAT (Renta)')).toBeNull();
    cleanup();
    api.getProfile.mockResolvedValue({ persona: 'Fisica' });
    render(<Models />);
    expect(
      await screen.findByText('Cotejo con los datos fiscales de la AEAT (Renta)'),
    ).toBeTruthy();
  });

  it('enseña el error si el modelo no aplica', async () => {
    api.modelDraft.mockRejectedValue({
      message: 'el modelo 130 es para personas físicas (autónomos)',
    });
    render(<Models />);
    expect((await screen.findByRole('alert')).textContent).toContain('personas físicas');
  });

  it('guarda los datos del 202 y el 200 en su ejercicio y conserva los de la versión antigua', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.modelSettings.mockResolvedValue({
      iban: null,
      domiciliar: false,
      vat_compensation_carried: 0,
      request_refund: false,
      exempt_from_390: false,
      previous_year_net: null,
      corporate_tax_base: 500_000,
      corporate_tax_base_1p: null,
      cnae: '6201',
      corporate_tax_rate: '25',
      turnover_over_6m: false,
      turnover_under_1m: true,
      corporate_regime: 'Micro',
      corporate_fines: 0,
      corporate_increases: 0,
      corporate_decreases: 0,
      capitalization_reserve: 0,
      pending_losses: 0,
    });
    api.saveModelSettings.mockResolvedValue(undefined);
    render(<Models />);
    fireEvent.click(await screen.findByRole('button', { name: 'Datos de los modelos' }));
    // La configuración antigua (sin años) sale en el ejercicio que se ve, con aviso.
    const base = await screen.findByLabelText(/Cuota del IS de \d{4} \(202 de \d{4}, 2P y 3P\)/);
    expect((base as HTMLInputElement).value).toBe('5.000,00 €');
    expect(screen.getByText(/configuración antigua, sin ejercicio/)).toBeTruthy();
    const year = String(new Date().getFullYear());
    // Cambiar otro ajuste no convierte los antiguos en los de este año.
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    await vi.waitFor(() => expect(api.saveModelSettings).toHaveBeenCalledTimes(1));
    let saved = api.saveModelSettings.mock.calls[0]?.[0];
    expect(saved.years[year]).toBeUndefined();
    expect(saved.corporate_tax_base).toBe(500_000);
    // Tocar los del ejercicio sí los guarda para ese año; los antiguos siguen para los demás.
    fireEvent.change(base, { target: { value: '6.000,00' } });
    // Con cambios sin guardar no se cambia de ejercicio.
    fireEvent.click(
      within(screen.getByRole('group', { name: 'Ejercicio de estos datos' })).getByRole('button', {
        name: 'Año siguiente',
      }),
    );
    expect(await screen.findByText(`Guarda o descarta antes los cambios de ${year}.`)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    await vi.waitFor(() => expect(api.saveModelSettings).toHaveBeenCalledTimes(2));
    saved = api.saveModelSettings.mock.calls[1]?.[0];
    expect(saved.corporate_tax_base).toBe(500_000);
    expect(saved.years[year].corporate_tax_base).toBe(600_000);
  });

  it('guarda la modalidad de estimación directa y las reducciones de la Renta del ejercicio', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.modelSettings.mockResolvedValue({
      iban: null,
      domiciliar: false,
      vat_compensation_carried: 0,
      request_refund: false,
      exempt_from_390: false,
      previous_year_net: null,
      corporate_tax_base: null,
      cnae: '',
      corporate_tax_rate: '',
      turnover_over_6m: false,
      turnover_under_1m: false,
      corporate_regime: 'General',
      corporate_fines: 0,
      corporate_increases: 0,
      corporate_decreases: 0,
      capitalization_reserve: 0,
      pending_losses: 0,
    });
    api.saveModelSettings.mockResolvedValue(undefined);
    render(<Models />);
    fireEvent.click(await screen.findByRole('button', { name: 'Datos de los modelos' }));
    const regime = await screen.findByLabelText(/Estimación directa del IRPF en \d{4}/);
    expect((regime as HTMLSelectElement).value).toBe('');
    fireEvent.change(regime, { target: { value: 'Normal' } });
    fireEvent.change(screen.getByLabelText(/Reducción por la actividad en la Renta/), {
      target: { value: 'RentasBajas' },
    });
    fireEvent.click(screen.getByLabelText(/inicio de actividad/));
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    await vi.waitFor(() => expect(api.saveModelSettings).toHaveBeenCalledTimes(1));
    const saved = api.saveModelSettings.mock.calls[0]?.[0];
    const year = String(new Date().getFullYear());
    expect(saved.years[year].irpf_regime).toBe('Normal');
    expect(saved.years[year].activity_reduction).toBe('RentasBajas');
    expect(saved.years[year].activity_start).toBe(true);
    expect(saved.years[year].other_income_over_6500).toBe(false);
  });

  it('a un autónomo no le enseña los datos del IS, ni a una sociedad los del IRPF', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.modelSettings.mockResolvedValue({
      iban: null,
      domiciliar: false,
      vat_compensation_carried: 0,
      request_refund: false,
      exempt_from_390: false,
      previous_year_net: null,
      corporate_tax_base: null,
      cnae: '',
      corporate_tax_rate: '',
      turnover_over_6m: false,
      turnover_under_1m: false,
      corporate_regime: 'General',
      corporate_fines: 0,
      corporate_increases: 0,
      corporate_decreases: 0,
      capitalization_reserve: 0,
      pending_losses: 0,
    });
    const hidden = (label: RegExp) => screen.getByLabelText(label).closest('[hidden]') !== null;
    api.getProfile.mockResolvedValue({ persona: 'Fisica' });
    render(<Models />);
    fireEvent.click(await screen.findByRole('button', { name: 'Datos de los modelos' }));
    await screen.findByLabelText(/Estimación directa del IRPF/);
    expect(hidden(/Estimación directa del IRPF/)).toBe(false);
    expect(hidden(/Cuota del IS de \d{4} \(202 de \d{4}, 2P/)).toBe(true);
    expect(hidden(/CNAE de la actividad principal/)).toBe(true);
    expect(screen.getByText(/Datos de cada ejercicio \(\s*130 y Renta\)/)).toBeTruthy();
    expect(screen.queryByText(/La cuota del IS también puede salir del 200/)).toBeNull();
    cleanup();
    api.getProfile.mockResolvedValue({ persona: 'Juridica' });
    render(<Models />);
    fireEvent.click(await screen.findByRole('button', { name: 'Datos de los modelos' }));
    await screen.findByLabelText(/Estimación directa del IRPF/);
    expect(hidden(/Estimación directa del IRPF/)).toBe(true);
    expect(hidden(/Rendimiento neto de \d{4}/)).toBe(true);
    expect(hidden(/Cuota del IS de \d{4} \(202 de \d{4}, 2P/)).toBe(false);
    expect(screen.getByText(/La cuota del IS también puede salir del 200/)).toBeTruthy();
  });
});

describe('tipos del IS por ejercicio', () => {
  beforeEach(() => {
    api.filedReturns.mockResolvedValue([]);
    api.getProfile.mockResolvedValue(null);
  });

  it('isRates sigue la rebaja gradual del art. 29.1 y la DT 44.ª LIS (Ley 7/2024)', () => {
    // Espejo de small_company_rates (crates/models/src/m200.rs).
    expect(isRates(2025)).toEqual({ general: 25, reduced: 24, micro: [21, 22], startup: 15 });
    expect(isRates(2026)).toEqual({ general: 25, reduced: 23, micro: [19, 21], startup: 15 });
    expect(isRates(2027)).toEqual({ general: 25, reduced: 22, micro: [17, 20], startup: 15 });
    expect(isRates(2028).reduced).toBe(21);
    expect(isRates(2029).reduced).toBe(20);
    expect(isRates(2024)).toEqual({ general: 25, reduced: 25, micro: [23, 23], startup: 15 });
    expect(isRates(2022).micro).toEqual([25, 25]);
  });

  it('las etiquetas dicen el tipo del ejercicio y la nueva creación es un 15 % plano', () => {
    expect(regimeLabels(2025)).toEqual({
      General: 'General (25 %)',
      ReducidaDimension: 'Reducida dimensión (24 % en 2025)',
      Micro: 'Cifra de negocios inferior a 1 millón (21 % hasta 50.000 €, 22 % el resto)',
      NuevaCreacion: 'Nueva creación (15 %)',
    });
    expect(regimeLabels(2026).ReducidaDimension).toBe('Reducida dimensión (23 % en 2026)');
    expect(regimeLabels(2026).Micro).toBe(
      'Cifra de negocios inferior a 1 millón (19 % hasta 50.000 €, 21 % el resto)',
    );
    // Sin ejercicio (el régimen por defecto) no se afirma ningún porcentaje que cambie.
    expect(regimeLabels().ReducidaDimension).toBe('Reducida dimensión (según el ejercicio)');
    expect(regimeLabels().NuevaCreacion).toBe('Nueva creación (15 %)');
  });

  it('el desplegable del ejercicio cambia los porcentajes al cambiar de año', async () => {
    api.modelDraft.mockResolvedValue(draft303);
    api.modelSettings.mockResolvedValue({
      iban: null,
      domiciliar: false,
      vat_compensation_carried: 0,
      request_refund: false,
      exempt_from_390: false,
      previous_year_net: null,
      corporate_tax_base: null,
      corporate_tax_base_1p: null,
      cnae: '6201',
      corporate_tax_rate: '25',
      turnover_over_6m: false,
      turnover_under_1m: true,
      corporate_regime: 'Micro',
      corporate_fines: 0,
      corporate_increases: 0,
      corporate_decreases: 0,
      capitalization_reserve: 0,
      pending_losses: 0,
    });
    render(<Models />);
    fireEvent.click(await screen.findByRole('button', { name: 'Datos de los modelos' }));
    const year = new Date().getFullYear();
    const texts = (el: HTMLElement) => Array.from((el as HTMLSelectElement).options, (o) => o.text);
    const ofYear = await screen.findByLabelText(/Tipo de gravamen del IS de \d{4} \(200\)/);
    expect(texts(ofYear)).toContain(`Reducida dimensión (${isRates(year).reduced} % en ${year})`);
    fireEvent.click(
      within(screen.getByRole('group', { name: 'Ejercicio de estos datos' })).getByRole('button', {
        name: 'Año anterior',
      }),
    );
    expect(texts(ofYear)).toContain(
      `Reducida dimensión (${isRates(year - 1).reduced} % en ${year - 1})`,
    );
    expect(texts(ofYear)).not.toContain(
      `Reducida dimensión (${isRates(year).reduced} % en ${year})`,
    );
    const byDefault = screen.getByLabelText('Tipo de gravamen del IS (200), por defecto');
    expect(texts(byDefault)).toEqual([
      'General (25 %)',
      'Reducida dimensión (según el ejercicio)',
      'Cifra de negocios inferior a 1 millón (según el ejercicio)',
      'Nueva creación (15 %)',
    ]);
  });
});

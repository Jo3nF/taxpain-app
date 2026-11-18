import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AeatCheckReport, AeatCheckRow, AeatCheckTarget } from './api';

const api = vi.hoisted(() => ({
  aeatCheckReport: vi.fn(),
  aeatCheckYear: vi.fn(),
  aeatCheckYears: vi.fn(),
  aeatCheckTarget: vi.fn(),
  aeatCheckSave: vi.fn(),
  aeatCheckSaveYears: vi.fn(),
  aeatCheckSummaries: vi.fn(),
  onAeatCheckProgress: vi.fn(),
  aeatCheckRunning: vi.fn(),
  onAeatCheckDone: vi.fn(),
  pickFolder: vi.fn(),
  listCertificates: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { PresentadoAeat } = await import('./PresentadoAeat');

const base: AeatCheckRow = {
  model: '303',
  period: '1T',
  status: 'Presented',
  expected: true,
  due_on: '2025-04-21',
  presented_at: '2025-04-15T10:00:00',
  original_presented_at: null,
  late: false,
  late_amendment: false,
  receipt: '3030000000001',
  replaces: [],
  has_pdf: true,
  boxes_read: true,
  mismatches: 0,
  filed_model_id: 1,
  note: null,
};

const report: AeatCheckReport = {
  year: 2025,
  nif: 'B12345674',
  checked_at: '2026-10-20T10:00:00Z',
  without_profile: false,
  rows: [
    base,
    {
      ...base,
      period: '2T',
      late: false,
      late_amendment: true,
      original_presented_at: '2025-07-15T10:00:00',
      receipt: '3030000000003',
      replaces: ['3030000000002'],
      mismatches: 2,
      presented_at: '2025-09-01T10:00:00',
    },
    {
      ...base,
      period: '3T',
      status: 'Missing',
      due_on: '2025-10-20',
      presented_at: null,
      receipt: null,
      has_pdf: false,
      boxes_read: false,
      mismatches: null,
    },
    {
      ...base,
      model: '111',
      period: '2T',
      status: 'NotFound',
      due_on: '2025-07-21',
      presented_at: null,
      receipt: null,
      has_pdf: false,
      boxes_read: false,
      mismatches: null,
      note: 'Sólo hacía falta si hubo pagos con retención.',
    },
  ],
  none_filed: ['349'],
  errors: ['390: la AEAT no respondió'],
  empty_answers: [{ text: 'Usted no ha presentado la declaración solicitada', models: ['349'] }],
  unavailable: ['721'],
};

const summary = (year: number) => ({
  year,
  presented: 5,
  missing: 1,
  not_found: 2,
  open: 0,
  pdfs: 5,
  boxes_read: 4,
  late: 1,
  errors: 1,
  failed: ['390: la AEAT no respondió'],
  unavailable: ['721'],
  checked_at: '2026-10-20T10:00:00Z',
});

const cert = {
  fingerprint_sha256: 'ab',
  subject_dn: 'CN=EMPRESA DEMO SL',
  holder_nif: '12345678Z',
  holder_name: 'Titular',
  entity_nif: 'B12345674',
  entity_name: 'EMPRESA DEMO SL',
};

const same: AeatCheckTarget = {
  company_nif: 'B12345674',
  company_name: 'EMPRESA DEMO SL',
  cert_nif: 'B12345674',
  cert_name: 'EMPRESA DEMO SL',
  same: true,
  other_entity: false,
};

const open = async (name = 'Comprobar lo presentado en 2025') => {
  fireEvent.click(await screen.findByRole('button', { name }));
  return screen.findByRole('dialog', { name: 'Comprobar en la AEAT' });
};

describe('PresentadoAeat', () => {
  beforeEach(() => {
    api.listCertificates.mockResolvedValue([cert]);
    api.aeatCheckTarget.mockResolvedValue(same);
    api.onAeatCheckProgress.mockResolvedValue(() => {});
    api.aeatCheckSummaries.mockResolvedValue([]);
    api.aeatCheckRunning.mockResolvedValue(false);
    api.onAeatCheckDone.mockResolvedValue(() => {});
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('si no se puede leer lo guardado, lo dice (no que el año no se ha comprobado)', async () => {
    api.aeatCheckReport.mockRejectedValue(new Error('sin almacén'));
    render(<PresentadoAeat year={2025} />);
    expect((await screen.findByRole('alert')).textContent).toContain(
      'No se pudo leer lo comprobado de 2025: sin almacén',
    );
    expect(screen.queryByText(/aún no se ha comprobado/)).toBeNull();
  });

  it('al volver con una comprobación en marcha, espera a que acabe', async () => {
    api.aeatCheckRunning.mockResolvedValue(true);
    let done: () => void = () => {};
    api.onAeatCheckDone.mockImplementation(async (f: () => void) => {
      done = f;
      return () => {};
    });
    api.aeatCheckReport.mockResolvedValue({ ...report, checked_at: null, rows: [] });
    render(<PresentadoAeat year={2025} />);
    expect((await screen.findAllByText('Consultando la AEAT…')).length).toBeGreaterThan(0);
    api.aeatCheckReport.mockResolvedValue(report);
    await act(async () => done());
    expect(screen.queryAllByText('Consultando la AEAT…')).toHaveLength(0);
    expect(api.aeatCheckSummaries).toHaveBeenCalledTimes(2);
  });

  it('sin comprobar: sólo el botón', async () => {
    api.aeatCheckReport.mockResolvedValue({ ...report, checked_at: null, rows: [] });
    render(<PresentadoAeat year={2025} />);
    expect(
      await screen.findByRole('button', { name: 'Comprobar lo presentado en 2025' }),
    ).toBeTruthy();
    expect(api.aeatCheckReport).toHaveBeenCalledWith(2025);
    expect(screen.queryByRole('table')).toBeNull();
  });

  it('pide confirmación, consulta con el certificado y pinta la tabla', async () => {
    api.aeatCheckReport.mockRejectedValue(new Error('sin almacén'));
    api.aeatCheckYear.mockResolvedValue(report);
    render(<PresentadoAeat year={2025} />);
    const dialog = await open();
    expect(dialog.textContent).toContain('no se presenta nada');
    // El foco va a la confirmación (lo pone un efecto: con el equipo cargado llega algo después).
    await waitFor(() => expect(document.activeElement?.textContent).toBe('Consultar'));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Consultar' }));
    // Con un modelo sin consultar, el titular no promete que no falte nada más.
    expect(
      await screen.findByText(
        'No se pudo consultar 1 modelo. Falta 1 declaración de las que tocaban.',
        { exact: false },
      ),
    ).toBeTruthy();
    expect(api.aeatCheckYear).toHaveBeenCalledWith(2025, 'ab', false);
    const rows = screen.getAllByRole('row');
    expect(rows).toHaveLength(5);
    expect(rows[1]?.textContent).toContain('Cuadra con TaxPain');
    expect(rows[2]?.textContent).toContain('Presentada');
    expect(rows[2]?.textContent).not.toContain('Presentada fuera de plazo');
    expect(rows[2]?.textContent).toContain('complementaria fuera de plazo');
    expect(rows[2]?.textContent).toContain('la primera, el 15/07/2025');
    expect(rows[2]?.textContent).toContain('sustituye a 3030000000002');
    expect(rows[2]?.textContent).toContain('2 casillas no cuadran con TaxPain');
    expect(rows[3]?.textContent).toContain('Falta');
    expect(rows[3]?.textContent).toContain('plazo hasta el 20/10/2025');
    expect(rows[4]?.textContent).toContain('No consta en la AEAT');
    expect(rows[4]?.textContent).toContain('pagos con retención');
    expect(screen.getByText(/Sin declaraciones en la AEAT/).textContent).toContain('349');
    expect(
      screen.getByText(/La AEAT contestó «Usted no ha presentado la declaración solicitada»/),
    ).toBeTruthy();
    expect(screen.getByText(/no deja consultar 721/)).toBeTruthy();
    expect(screen.getByRole('alert').textContent).toContain('390: la AEAT no respondió');
    // Acabada la consulta, el foco vuelve al botón (puede llegar un instante después).
    await waitFor(() =>
      expect(document.activeElement?.textContent).toBe('Comprobar lo presentado en 2025'),
    );
  });

  it('cancelar (o Esc) no consulta y un error se enseña', async () => {
    api.aeatCheckReport.mockResolvedValue({ ...report, checked_at: null, rows: [] });
    api.aeatCheckYear.mockRejectedValue('La AEAT no dio las declaraciones de 2025.');
    render(<PresentadoAeat year={2025} />);
    await open();
    fireEvent.click(screen.getByRole('button', { name: 'Cancelar' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    const dialog = await open();
    fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(api.aeatCheckYear).not.toHaveBeenCalled();
    await open();
    fireEvent.click(screen.getByRole('button', { name: 'Consultar' }));
    expect((await screen.findByRole('alert')).textContent).toContain('no dio las declaraciones');
  });

  it('sin NIF de la empresa no se consulta', async () => {
    api.aeatCheckReport.mockResolvedValue({ ...report, checked_at: null, rows: [] });
    api.aeatCheckTarget.mockResolvedValue({ ...same, company_nif: null, same: false });
    render(<PresentadoAeat year={2025} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Comprobar lo presentado en 2025' }));
    expect((await screen.findByRole('alert')).textContent).toContain('Pon el NIF de la empresa');
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('el certificado personal de un apoderado, sólo con confirmación expresa', async () => {
    api.aeatCheckReport.mockResolvedValue({ ...report, checked_at: null, rows: [] });
    api.aeatCheckTarget.mockResolvedValue({
      ...same,
      cert_nif: '12345678Z',
      cert_name: 'Titular',
      same: false,
    });
    api.aeatCheckYear.mockResolvedValue(report);
    render(<PresentadoAeat year={2025} />);
    const dialog = await open();
    expect(dialog.textContent).toContain('no de la empresa (B12345674)');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Consultar con este certificado' }));
    await screen.findAllByRole('row');
    expect(api.aeatCheckYear).toHaveBeenCalledWith(2025, 'ab', true);
  });

  it('el certificado de representante de otra entidad no se usa', async () => {
    api.aeatCheckReport.mockResolvedValue({ ...report, checked_at: null, rows: [] });
    api.aeatCheckTarget.mockResolvedValue({
      ...same,
      cert_nif: 'B98765431',
      same: false,
      other_entity: true,
    });
    render(<PresentadoAeat year={2025} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Comprobar lo presentado en 2025' }));
    expect((await screen.findByRole('alert')).textContent).toContain('representante de B98765431');
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('sin perfil fiscal no dice que no falte nada', async () => {
    api.aeatCheckReport.mockResolvedValue({ ...report, without_profile: true, errors: [] });
    render(<PresentadoAeat year={2025} />);
    expect(await screen.findByText(/Sin perfil fiscal no se puede saber qué falta/)).toBeTruthy();
    expect(screen.queryByText(/No falta ninguna/)).toBeNull();
    expect(screen.getByText(/Sin declaraciones en la AEAT/).textContent).not.toContain(
      'y no tocaban',
    );
  });

  it('si se cambia de año a medias, no pinta la tabla del otro año', async () => {
    api.aeatCheckReport.mockResolvedValue({ ...report, checked_at: null, rows: [] });
    let resolve: (r: AeatCheckReport) => void = () => {};
    api.aeatCheckYear.mockReturnValue(
      new Promise<AeatCheckReport>((r) => {
        resolve = r;
      }),
    );
    const { rerender } = render(<PresentadoAeat year={2025} />);
    const dialog = await open();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Consultar' }));
    rerender(<PresentadoAeat year={2024} />);
    await act(async () => {
      resolve(report);
    });
    expect(screen.queryByRole('table')).toBeNull();
    expect(screen.getByRole('heading', { name: 'Detalle de 2024' })).toBeTruthy();
  });

  it('comprueba varios años con una sola confirmación y guarda por años', async () => {
    api.aeatCheckReport.mockResolvedValue({ ...report, checked_at: null, rows: [] });
    api.aeatCheckYears.mockResolvedValue([
      { year: 2024, summary: summary(2024), error: null },
      { year: 2025, summary: null, error: 'No se pudo conectar con la AEAT para consultar 2025.' },
    ]);
    api.pickFolder.mockResolvedValue('/tmp/j');
    api.aeatCheckSaveYears.mockResolvedValue(['/tmp/j/2024/a.pdf']);
    const thisYear = new Date().getFullYear();
    render(<PresentadoAeat year={2025} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Comprobar varios años…' }));
    fireEvent.change(screen.getByLabelText('Desde el año'), { target: { value: '2024' } });
    const dialog = await open(`Comprobar de 2024 a ${thisYear}`);
    expect(dialog.textContent).toContain(`2024 a ${thisYear}`);
    api.aeatCheckSummaries.mockResolvedValue([summary(2024)]);
    fireEvent.click(within(dialog).getByRole('button', { name: 'Consultar' }));
    const table = await screen.findByRole('table', { name: 'Resumen por años' });
    expect(api.aeatCheckYears).toHaveBeenCalledWith(2024, thisYear, 'ab', false);
    expect(table.textContent).toContain('No se pudo conectar con la AEAT para consultar 2025.');
    // Qué no se pudo consultar y por qué, no sólo cuántos.
    expect(table.textContent).toContain('No se pudo consultar el 390');
    expect(table.textContent).toContain('721: este servicio de la AEAT no lo deja consultar');
    expect(table.textContent).toContain('5 de 5');
    // La leyenda de las columnas.
    expect(screen.getByText('Qué significa cada columna')).toBeTruthy();
    fireEvent.click(
      screen.getByRole('button', {
        name: 'Guardar los justificantes de 2024 a 2025 en una carpeta…',
      }),
    );
    expect(
      await screen.findByText('1 justificante guardado en /tmp/j, cada año en su carpeta.'),
    ).toBeTruthy();
    expect(api.aeatCheckSaveYears).toHaveBeenCalledWith(2024, 2025, '/tmp/j');
  });

  it('cada año del resumen abre su detalle, con su título', async () => {
    api.aeatCheckSummaries.mockResolvedValue([summary(2024), summary(2025)]);
    api.aeatCheckReport.mockImplementation(async (y: number) => ({ ...report, year: y }));
    render(<PresentadoAeat year={2025} />);
    expect(await screen.findByRole('heading', { name: 'Detalle de 2025' })).toBeTruthy();
    fireEvent.click(await screen.findByRole('button', { name: 'Ver detalle de 2024' }));
    expect(await screen.findByRole('heading', { name: 'Detalle de 2024' })).toBeTruthy();
    expect(api.aeatCheckReport).toHaveBeenLastCalledWith(2024);
    expect(await screen.findByRole('table', { name: 'Declaraciones de 2024' })).toBeTruthy();
  });

  it('guarda los justificantes en la carpeta elegida', async () => {
    api.aeatCheckReport.mockResolvedValue(report);
    api.pickFolder.mockResolvedValue('/tmp/justificantes');
    api.aeatCheckSave.mockResolvedValue(['/tmp/justificantes/a.pdf', '/tmp/justificantes/b.pdf']);
    render(<PresentadoAeat year={2025} />);
    fireEvent.click(
      await screen.findByRole('button', {
        name: 'Guardar los justificantes de 2025 en una carpeta…',
      }),
    );
    expect(
      await screen.findByText('2 justificantes guardados en /tmp/justificantes.'),
    ).toBeTruthy();
    expect(api.aeatCheckSave).toHaveBeenCalledWith(2025, '/tmp/justificantes');
  });
});

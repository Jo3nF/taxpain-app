import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  noticesList: vi.fn(),
  noticesRefresh: vi.fn(),
  noticeAppear: vi.fn(),
  noticeDownload: vi.fn(),
  listCertificates: vi.fn(),
  noticesCourtesyDays: vi.fn(async () => [] as string[]),
  noticesSetCourtesyDays: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Notices } = await import('./Notices');

const pending = {
  id: '2319979900012',
  kind: 'N',
  state: 'P',
  subject: 'Notificación administrativa',
  available_at: '2026-09-01T09:00:00',
  environment: 'produccion',
  accessed_at: null,
  metadata: [],
  has_content: false,
  deemed_notified_on: '2026-09-11',
};

describe('Notices', () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('abrir una notificación pendiente exige confirmar que empiezan los plazos', async () => {
    api.noticesList.mockResolvedValue([pending]);
    api.listCertificates.mockResolvedValue([{ fingerprint_sha256: 'abc' }]);
    api.noticeAppear.mockResolvedValue(undefined);
    render(<Notices />);
    expect(await screen.findByText('1 pendiente')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Abrir notificación…' }));
    const open = screen.getByRole('button', { name: 'Abrir la notificación' });
    expect((open as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByLabelText('Entiendo que empiezan los plazos'));
    fireEvent.click(open);
    await vi.waitFor(() => expect(api.noticeAppear).toHaveBeenCalledWith('2319979900012', 'abc'));
  });

  it('el panel para abrirla se cancela con su botón o con Esc y devuelve el foco', async () => {
    api.noticesList.mockResolvedValue([pending]);
    api.listCertificates.mockResolvedValue([{ fingerprint_sha256: 'abc' }]);
    render(<Notices />);
    fireEvent.click(await screen.findByRole('button', { name: 'Abrir notificación…' }));
    expect(document.activeElement).toBe(screen.getByLabelText('Entiendo que empiezan los plazos'));
    fireEvent.click(screen.getByRole('button', { name: 'Cancelar' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    const trigger = screen.getByRole('button', { name: 'Abrir notificación…' });
    expect(document.activeElement).toBe(trigger);
    fireEvent.click(trigger);
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(api.noticeAppear).not.toHaveBeenCalled();
  });

  it('el certificado elegido se usa en la consulta y no se arrastra a otro montaje', async () => {
    const c = (fp: string, name: string) => ({
      fingerprint_sha256: fp,
      subject_dn: `CN=${name}`,
      holder_name: name,
      holder_nif: '12345678Z',
      entity_name: null,
      entity_nif: null,
    });
    api.noticesList.mockResolvedValue([]);
    api.listCertificates.mockResolvedValue([c('abc', 'Carmen'), c('def', 'Carmen (otro)')]);
    api.noticesRefresh.mockResolvedValue(0);
    render(<Notices />);
    fireEvent.change(await screen.findByLabelText('Certificado'), { target: { value: 'def' } });
    fireEvent.click(screen.getByRole('button', { name: 'Consultar en la AEAT' }));
    await vi.waitFor(() => expect(api.noticesRefresh).toHaveBeenCalledWith('def'));
    cleanup();
    // Otro montaje (otra empresa, otra pestaña) empieza por el primero: nada queda en el módulo.
    render(<Notices />);
    await screen.findByLabelText('Certificado');
    fireEvent.click(screen.getByRole('button', { name: 'Consultar en la AEAT' }));
    await vi.waitFor(() => expect(api.noticesRefresh).toHaveBeenLastCalledWith('abc'));
  });

  it('las comunicaciones se descargan sin confirmación y consultar avisa de lo nuevo', async () => {
    api.noticesList.mockResolvedValue([{ ...pending, kind: 'C', state: 'A' }]);
    api.listCertificates.mockResolvedValue([{ fingerprint_sha256: 'abc' }]);
    api.noticeDownload.mockResolvedValue(undefined);
    api.noticesRefresh.mockResolvedValue(2);
    render(<Notices />);
    fireEvent.click(await screen.findByRole('button', { name: 'Descargar' }));
    await vi.waitFor(() => expect(api.noticeDownload).toHaveBeenCalled());
    expect(api.noticeAppear).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Consultar en la AEAT' }));
    expect(await screen.findByText('2 nuevas.')).toBeTruthy();
  });

  it('cuenta atrás: cuántos días quedan para abrirla y cuándo se da por notificada', async () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date('2026-09-08T10:00:00Z'));
    api.noticesList.mockResolvedValue([pending]);
    render(<Notices />);
    expect(await screen.findByText('Quedan 2 días para abrirla')).toBeTruthy();
    expect(screen.getByText(/Ábrela como tarde el 10\/09\/2026: el 11\/09\/2026/)).toBeTruthy();
    cleanup();
    vi.setSystemTime(new Date('2026-09-12T10:00:00Z'));
    render(<Notices />);
    expect(await screen.findByText(/se dio por notificada el 11\/09\/2026/)).toBeTruthy();
    vi.useRealTimers();
  });

  it('apunta los días de cortesía', async () => {
    api.noticesList.mockResolvedValue([]);
    api.noticesCourtesyDays.mockResolvedValue(['2099-08-03']);
    api.noticesSetCourtesyDays.mockResolvedValue(['2099-08-03', '2099-08-04']);
    render(<Notices />);
    expect(await screen.findByText(/03\/08\/2099/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Día de cortesía'), { target: { value: '2099-08-04' } });
    fireEvent.click(screen.getByRole('button', { name: 'Apuntar' }));
    await vi.waitFor(() =>
      expect(api.noticesSetCourtesyDays).toHaveBeenCalledWith(['2099-08-03', '2099-08-04']),
    );
    expect(await screen.findByText(/04\/08\/2099/)).toBeTruthy();
  });
});

describe('importe de una notificación', () => {
  it('en céntimos, a euros; con decimales, tal cual', async () => {
    const { importeAeat } = await import('./Notices');
    expect(importeAeat('12345')).toBe('123,45 €');
    expect(importeAeat('123.45')).toBe('123.45');
  });
});

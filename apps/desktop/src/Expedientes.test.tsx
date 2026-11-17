import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ExpedienteVista } from './api';

const api = vi.hoisted(() => ({
  expedientesList: vi.fn(),
  expedientePreview: vi.fn(),
  expedienteCreate: vi.fn(),
  expedienteDelete: vi.fn(),
  expedienteEscritoSave: vi.fn(),
  companyDocsList: vi.fn(async () => []),
  certificadosAdminList: vi.fn(async () => []),
  certificadoAdminSave: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Expedientes, escritosDe, plazosDelAño } = await import('./Expedientes');

const plazo = {
  clave: 'contestar',
  titulo: 'Contestar el requerimiento',
  vence: '2026-12-09',
  computo: '10 días hábiles desde el día siguiente a la notificación',
  norma: 'Art. 29.2.g LGT',
  manual: false,
};

const vista: ExpedienteVista = {
  expediente: {
    id: 7,
    organismo: 'aeat',
    organo: '',
    referencia: '2026INS00000001',
    tipo: 'requerimiento',
    asunto: 'Requerimiento de información',
    concepto: 'IVA 2025',
    puesto_el: null,
    notificado_el: '2026-11-23',
    plazo_dias: null,
    vence_manual: null,
    importe: null,
    sancion: null,
    fin_voluntario: null,
    estado: 'abierto',
    notice_id: null,
    document_id: null,
    documentos: [],
    notas: '',
  },
  actuaciones: [],
  calculo: {
    notificado: '2026-11-23',
    presunta: false,
    plazos: [plazo],
    proximo: plazo,
    dias: 2,
    vencido: null,
    festivos_completos: true,
    recargo: [],
    reducciones: null,
    avisos: [],
  },
};

describe('Expedientes', () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('enseña la cuenta atrás y guarda un escrito sin presentar nada', async () => {
    api.expedientesList.mockResolvedValue([vista]);
    api.expedienteEscritoSave.mockResolvedValue('/Users/demo/Contestación.rtf');
    render(<Expedientes today="2026-12-07" />);
    expect(await screen.findByText('Quedan 2 días')).toBeTruthy();
    expect(screen.getByText('1 con plazo esta semana')).toBeTruthy();
    fireEvent.click(screen.getByText('Plazos, cálculos y escritos'));
    fireEvent.click(screen.getByText('Guardar RTF…'));
    expect(await screen.findByText(/nada se ha enviado/)).toBeTruthy();
    expect(api.expedienteEscritoSave).toHaveBeenCalledWith(
      7,
      'contestacion',
      { motivo: '', plazos: null, lugar: '' },
      false,
    );
  });

  it('borrar pide confirmación', async () => {
    api.expedientesList.mockResolvedValue([vista]);
    api.expedienteDelete.mockResolvedValue(null);
    render(<Expedientes today="2026-12-07" />);
    await screen.findByText('Quedan 2 días');
    fireEvent.click(screen.getByText('Plazos, cálculos y escritos'));
    fireEvent.click(screen.getByRole('button', { name: /Borrar el expediente/ }));
    expect(api.expedienteDelete).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText('Sí, borrar'));
    await vi.waitFor(() => expect(api.expedienteDelete).toHaveBeenCalledWith(7, true));
  });

  it('el alta calcula los plazos mientras se teclea y crea el expediente', async () => {
    api.expedientesList.mockResolvedValue([]);
    api.expedientePreview.mockResolvedValue(vista.calculo);
    api.expedienteCreate.mockResolvedValue(8);
    render(<Expedientes today="2026-11-16" />);
    fireEvent.click(await screen.findByText('Nuevo expediente'));
    const form = screen.getByRole('form', { name: 'Nuevo expediente' });
    fireEvent.change(within(form).getByLabelText(/Notificada el/), {
      target: { value: '2026-11-23' },
    });
    expect(await within(form).findByText('09/12/2026')).toBeTruthy();
    fireEvent.click(within(form).getByText('Crear el expediente'));
    await vi.waitFor(() => expect(api.expedienteCreate).toHaveBeenCalled());
    expect(api.expedienteCreate.mock.calls[0]?.[0].notificado_el).toBe('2026-11-23');
  });

  it('escritos según el tipo y plazos del año para el calendario', () => {
    expect(escritosDe('liquidacion')).toEqual(['recurso_reposicion', 'aplazamiento']);
    expect(escritosDe('requerimiento')).toContain('contestacion');
    const p = plazosDelAño([vista], 2026);
    expect(p).toHaveLength(1);
    expect(p[0]?.vence).toBe('2026-12-09');
    expect(plazosDelAño([vista], 2027)).toHaveLength(0);
  });
});

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CotejoCenso, GuiaCenso } from './api';

const api = vi.hoisted(() => ({
  censoCotejo: vi.fn(),
  censoGuardar: vi.fn(),
  censoGuia: vi.fn(),
  censoProgramar: vi.fn(),
  censoPresentada: vi.fn(),
  censoQuitar: vi.fn(),
  openSede: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Censo } = await import('./Censo');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function cotejo(pendiente = false): CotejoCenso {
  return {
    hoy: '2026-11-16',
    situacion: {
      origen: 'certificado',
      documento: 'Certificado de situación censal',
      fecha: '2026-10-01',
      obligaciones: ['200', '303'],
      lineas: [],
      epigrafes: ['844'],
      roi: false,
      sii: null,
      criterio_caja: null,
      domicilio_fiscal: null,
    },
    tecleado: null,
    epigrafes_actividad: [],
    filas: [
      {
        codigo: '111',
        nombre: 'Retenciones de trabajo y profesionales (111 y 190)',
        censo: false,
        perfil: true,
        contabilidad: '2 facturas de profesionales con retención (300,00 € de retención)',
        presentado: [],
      },
    ],
    diferencias: [
      {
        clave: '111-sin-alta',
        gravedad: 'alta',
        titulo: 'Retienes sin la obligación del 111',
        detalle: 'No consta en el censo.',
        que_hacer: 'Comunica el alta en el 036.',
        supuesto: 'alta-111',
      },
    ],
    comunicaciones: pendiente
      ? [
          {
            id: 1,
            supuesto: 'alta-111',
            titulo: 'Alta en las retenciones de trabajo y profesionales (111)',
            fecha_hecho: '2026-11-02',
            vence: '2026-12-02',
            nota: '',
            creada: '2026-11-16T10:00:00Z',
            hecha: null,
          },
        ]
      : [],
    supuestos: [
      {
        id: 'alta-111',
        titulo: 'Alta en las retenciones de trabajo y profesionales (111)',
        grupo: 'Retenciones',
      },
    ],
  };
}

const GUIA: GuiaCenso = {
  id: 'alta-111',
  titulo: 'Alta en las retenciones de trabajo y profesionales (111)',
  resumen: 'Quien paga facturas de profesionales con retención presenta el 111.',
  por_036: true,
  causa: 'Modificación',
  pasos: [{ apartado: 'Retenciones e ingresos a cuenta', que_hacer: 'Da de alta la obligación.' }],
  plazo: {
    tipo: 'un_mes',
    texto: 'Un mes desde el primer pago: hasta el 02/12/2026.',
    desde: '2026-11-02',
    vence: '2026-12-02',
    pide_fecha: false,
    en_calendario: null,
  },
  documentacion: [],
  norma: 'RGAT arts. 9 a 14',
  avisos: [],
  sede: 'https://sede.agenciatributaria.gob.es/Sede/procedimientoini/G322.shtml',
  verificar: 'Sin números de casilla.',
};

describe('Censo', () => {
  it('enseña el cuadro, las diferencias y abre la guía desde una diferencia', async () => {
    api.censoCotejo.mockResolvedValue(cotejo());
    api.censoGuia.mockResolvedValue(GUIA);
    api.openSede.mockResolvedValue(undefined);
    api.censoProgramar.mockResolvedValue(cotejo(true).comunicaciones[0]);
    render(<Censo />);
    expect(await screen.findByText('Retienes sin la obligación del 111')).toBeTruthy();
    expect(screen.getByText(/2 facturas de profesionales/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Ver qué marcar en el 036' }));
    expect(await screen.findByText(/Da de alta la obligación/)).toBeTruthy();
    expect(api.censoGuia).toHaveBeenCalledWith('alta-111', null);
    expect(screen.getByText(/hasta el 02\/12\/2026/)).toBeTruthy();
    // Añadir al calendario pide confirmación.
    fireEvent.click(screen.getByRole('button', { name: 'Añadir el plazo al calendario' }));
    expect(api.censoProgramar).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Añadir' }));
    await waitFor(() => expect(api.censoProgramar).toHaveBeenCalledWith('alta-111', null, ''));
    fireEvent.click(screen.getByRole('button', { name: /Abrir el 036 en la Sede/ }));
    expect(api.openSede).toHaveBeenCalledWith(GUIA.sede);
  });

  it('guarda lo tecleado del censo y quita un aviso con confirmación', async () => {
    api.censoCotejo.mockResolvedValue(cotejo(true));
    api.censoGuardar.mockResolvedValue(undefined);
    api.censoQuitar.mockResolvedValue(undefined);
    render(<Censo />);
    fireEvent.click(await screen.findByRole('button', { name: 'Teclear lo que consta' }));
    const form = screen.getByRole('form', { name: 'Lo que consta en el censo' });
    fireEvent.click(within(form).getByLabelText(/Retenciones de trabajo y profesionales/));
    fireEvent.click(within(form).getByRole('button', { name: 'Guardar' }));
    await waitFor(() => expect(api.censoGuardar).toHaveBeenCalled());
    const [tecleado, actividad] = api.censoGuardar.mock.calls[0] ?? [];
    expect(tecleado.obligaciones).toEqual(['200', '303', '111']);
    expect(tecleado.epigrafes).toEqual(['844']);
    expect(actividad).toEqual([]);
    fireEvent.click(
      screen.getByRole('button', {
        name: 'Quitar: Alta en las retenciones de trabajo y profesionales (111)',
      }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Quitar' }));
    await waitFor(() => expect(api.censoQuitar).toHaveBeenCalledWith(1, true));
  });
});

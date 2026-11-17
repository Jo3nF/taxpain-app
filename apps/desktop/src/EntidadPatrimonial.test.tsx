import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { InformePatrimonial } from './api';

const api = vi.hoisted(() => ({
  patrimonialReport: vi.fn(),
  patrimonialMark: vi.fn(),
  patrimonialSettings: vi.fn(),
  patrimonialSaleAdd: vi.fn(),
  patrimonialSaleDelete: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { EntidadPatrimonial, pct } = await import('./EntidadPatrimonial');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const trimestre = (fecha: string, computa: number, activo: number) => ({
  fecha,
  cerrado: true,
  activo,
  afecto: activo - computa,
  valores: computa,
  no_afecto: 0,
  tesoreria: 0,
  participaciones: 0,
  tesoreria_justificada: 0,
  ventas_excluidas: 0,
  tesoreria_no_justificada: 0,
  computa,
  pct: Math.floor((computa * 10000) / activo),
  computa_lip: computa,
  pct_lip: Math.floor((computa * 10000) / activo),
});

function informe(patrimonial: boolean, avisos = false): InformePatrimonial {
  const computa = patrimonial ? 6_000_000 : 3_000_000;
  const fechas = ['2026-03-31', '2026-06-30', '2026-09-30', '2026-12-31'];
  return {
    year: 2026,
    fechas,
    partidas: [
      {
        cuenta: '2110001',
        nombre: 'Local',
        clase: 'afecto',
        propuesta: 'afecto',
        marcada: false,
        motivo: '',
        desde: null,
        correctora: false,
        saldos: [6_000_000, 6_000_000, 6_000_000, 6_000_000],
      },
    ],
    trimestres: fechas.map((f) => trimestre(f, computa, 10_000_000)),
    media: {
      trimestres: 4,
      activo: 10_000_000,
      valores: computa,
      no_afecto: 0,
      participaciones: 0,
      computa,
      pct: computa / 1000,
      pct_con_participaciones: computa / 1000,
      pct_sin_tesoreria: computa / 1000,
      margen: 5_000_000 - computa,
    },
    patrimonial,
    provisional: false,
    trimestres_lip: 0,
    ajustes: {
      tesoreria_afecta: 0,
      tesoreria_motivo: '',
      beneficios_actividad: null,
      beneficios_motivo: '',
      prestamos_participativos: 0,
      dominante: false,
    },
    ventas: [{ id: 3, fecha: '2026-05-15', importe: 1_000_000, motivo: 'Furgoneta' }],
    beneficios_propuestos: 0,
    consecuencias: [
      { titulo: 'Tipo de gravamen', texto: 'Tributa al 25 %.', norma: 'Art. 29.1 LIS' },
    ],
    que_revisar: [],
    avisos: [],
    patrimonio: {
      forma: 'sl',
      fechas: [
        {
          fecha: '2026-12-31',
          cerrado: true,
          capital: 10_000_000,
          contable: 4_000_000,
          capital_ajustes: 0,
          coberturas: 0,
          prestamos_participativos: 0,
          mercantil: 4_000_000,
          pct_capital: 4000,
        },
      ],
      anterior: null,
      avisos: avisos
        ? [
            {
              codigo: 'causa_disolucion',
              gravedad: 'warning',
              titulo: 'Causa de disolución por pérdidas',
              detalle: 'Los administradores deben convocar la junta en el plazo de dos meses.',
              plazo: '2027-02-28',
              norma: 'Arts. 363.1.e, 365, 366 y 367 LSC',
            },
          ]
        : [],
      notas: [],
    },
  };
}

describe('EntidadPatrimonial', () => {
  it('formatea los porcentajes', () => {
    expect(pct(5230)).toBe('52,3 %');
    expect(pct(3000)).toBe('30 %');
  });

  it('por debajo del 50 % dice el margen y sin avisos del capital', async () => {
    api.patrimonialReport.mockResolvedValue(informe(false));
    render(<EntidadPatrimonial year={2026} />);
    expect(await screen.findByText(/No llega al 50 %/)).toBeTruthy();
    expect(screen.getByText(/computa el 30 % del activo/)).toBeTruthy();
    expect(screen.getByText(/Sin avisos/)).toBeTruthy();
    expect(screen.getByText('Qué supondría ser entidad patrimonial')).toBeTruthy();
  });

  it('por encima del 50 % y con causa de disolución avisa', async () => {
    api.patrimonialReport.mockResolvedValue(informe(true, true));
    render(<EntidadPatrimonial year={2026} />);
    expect(await screen.findByText(/Supera el 50 %/)).toBeTruthy();
    expect(screen.getByRole('alert').textContent).toContain('Causa de disolución por pérdidas');
    expect(screen.getByText('Qué supone ser entidad patrimonial')).toBeTruthy();
  });

  it('marca una partida con su motivo y guarda los ajustes', async () => {
    api.patrimonialReport.mockResolvedValue(informe(false));
    api.patrimonialMark.mockResolvedValue(undefined);
    api.patrimonialSettings.mockResolvedValue(undefined);
    render(<EntidadPatrimonial year={2026} />);
    fireEvent.change(await screen.findByLabelText('Clase de 2110001'), {
      target: { value: 'no_afecto' },
    });
    fireEvent.change(screen.getByLabelText('Motivo de 2110001'), {
      target: { value: 'Sin uso' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar la clase de 2110001' }));
    await vi.waitFor(() =>
      expect(api.patrimonialMark).toHaveBeenCalledWith(2026, '2110001', 'no_afecto', 'Sin uso'),
    );
    fireEvent.change(screen.getByLabelText('Préstamos participativos recibidos'), {
      target: { value: '20.000' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar ajustes' }));
    await vi.waitFor(() =>
      expect(api.patrimonialSettings).toHaveBeenCalledWith(
        2026,
        expect.objectContaining({ prestamos_participativos: 2_000_000 }),
      ),
    );
  });

  it('anota una venta y pide confirmación para quitarla', async () => {
    api.patrimonialReport.mockResolvedValue(informe(false));
    api.patrimonialSaleAdd.mockResolvedValue(4);
    api.patrimonialSaleDelete.mockResolvedValue(undefined);
    render(<EntidadPatrimonial year={2026} />);
    fireEvent.change(await screen.findByLabelText('Fecha de la venta'), {
      target: { value: '2026-08-01' },
    });
    fireEvent.change(screen.getByLabelText('Importe'), { target: { value: '15.000' } });
    fireEvent.change(screen.getByLabelText('Qué se vendió'), { target: { value: 'Máquina' } });
    fireEvent.click(screen.getByRole('button', { name: 'Anotar venta' }));
    await vi.waitFor(() =>
      expect(api.patrimonialSaleAdd).toHaveBeenCalledWith('2026-08-01', 1_500_000, 'Máquina'),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Quitar la venta del 15/05/2026' }));
    expect(api.patrimonialSaleDelete).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Sí, quitarla' }));
    await vi.waitFor(() => expect(api.patrimonialSaleDelete).toHaveBeenCalledWith(3, true));
  });

  it('en un autónomo no sale', async () => {
    api.patrimonialReport.mockRejectedValue(
      'el test de entidad patrimonial y los avisos del capital son de las sociedades',
    );
    const { container } = render(<EntidadPatrimonial year={2026} />);
    await vi.waitFor(() => expect(container.textContent).toBe(''));
  });
});

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  archivoConservacion: vi.fn(),
  archivoAviso: vi.fn(),
  archivoEjercicioContenido: vi.fn(),
  archivoEjercicioExportar: vi.fn(),
  exportacionCompleta: vi.fn(),
  pickZipSave: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));

const { Archivo, AvisoArchivo } = await import('./Archivo');

const C2019 = {
  ejercicio: 2019,
  cierre: '2019-12-31',
  fin_mercantil: '2025-12-31',
  fin_fiscal: '2024-07-25',
  fin_creditos: null,
  creditos_pendientes: false,
  conservar_hasta: '2025-12-31',
  destruible: true,
  motivo: 'han pasado los seis años',
  archivado: null,
};
const C2025 = {
  ...C2019,
  ejercicio: 2025,
  cierre: '2025-12-31',
  conservar_hasta: '2031-12-31',
  destruible: false,
  motivo: 'seis años del Código de Comercio',
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Archivo', () => {
  it('dice qué hay y qué falta y exporta tras avisar de que va sin cifrar', async () => {
    api.archivoConservacion.mockResolvedValue([C2019, C2025]);
    api.archivoEjercicioContenido.mockResolvedValue([
      {
        clave: 'diario',
        titulo: 'Diario, mayor y sumas y saldos',
        estado: 'si',
        detalle: '6 ficheros',
        ficheros: 6,
      },
      {
        clave: 'bancos',
        titulo: 'Extractos bancarios',
        estado: 'no',
        detalle: 'no hay nada',
        ficheros: 0,
      },
    ]);
    api.pickZipSave.mockResolvedValue('/Users/demo/Archivo 2025.zip');
    api.archivoEjercicioExportar.mockResolvedValue({
      fichero: 'Archivo 2025.zip',
      sha256: 'f'.repeat(64),
      bytes: 3_000_000,
      ficheros: 12,
      partes: [],
    });
    render(<Archivo />);
    expect(await screen.findByText(/Extractos bancarios/)).toBeTruthy();
    expect(api.archivoEjercicioContenido).toHaveBeenCalledWith(2025);
    expect(screen.getByText('Ya se puede destruir')).toBeTruthy();
    expect(screen.getByText('31/12/2031')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: /Generar el archivo de 2025/ }));
    expect(api.pickZipSave).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: /Entendido/ }));
    expect(await screen.findByText(/Huella SHA-256/)).toBeTruthy();
    expect(api.archivoEjercicioExportar).toHaveBeenCalledWith(2025, '/Users/demo/Archivo 2025.zip');
  });
});

describe('AvisoArchivo', () => {
  it('dice qué ejercicios se podrían destruir sin borrar nada', async () => {
    api.archivoAviso.mockResolvedValue({
      destruibles: [2019],
      conservar: [C2025],
      sin_archivo: [2025],
    });
    const onOpen = vi.fn();
    render(<AvisoArchivo onOpen={onOpen} />);
    expect(await screen.findByText(/Ya se podrían destruir.*2019/)).toBeTruthy();
    expect(screen.getByText(/TaxPain no borra nada/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Ir a Archivo y auditoría' }));
    expect(onOpen).toHaveBeenCalled();
  });
});

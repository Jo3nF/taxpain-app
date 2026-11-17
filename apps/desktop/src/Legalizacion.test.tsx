import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({ legalizationBuild: vi.fn(), pickFolder: vi.fn() }));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Legalizacion } = await import('./Legalizacion');

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Legalizacion', () => {
  it('prepara el ZIP con los números de libro y el registro', async () => {
    api.pickFolder.mockResolvedValue('/tmp');
    api.legalizationBuild.mockResolvedValue({
      path: '/tmp/LEGALIZACION_2026_B12345674.ZIP',
      filename: 'LEGALIZACION_2026_B12345674.ZIP',
      books: [
        { file: 'DIARIO_001.XLSX', description: 'Libro Diario 2026', number: 3, huella: 'x' },
      ],
      warnings: ['falta el domicilio de la empresa (calle, ciudad o código postal)'],
      deadline: '2027-04-30',
    });
    render(<Legalizacion year={2026} />);
    fireEvent.change(screen.getByLabelText('Nº del Diario'), { target: { value: '3' } });
    fireEvent.change(screen.getByLabelText('Código del Registro'), { target: { value: '46001' } });
    fireEvent.click(screen.getByRole('button', { name: 'Preparar legalización 2026…' }));
    expect(await screen.findByText(/Plazo: hasta el 30\/04\/2027/)).toBeTruthy();
    expect(screen.getByText(/falta el domicilio/)).toBeTruthy();
    const [req, dir] = api.legalizationBuild.mock.calls[0] as [Record<string, unknown>, string];
    expect(dir).toBe('/tmp');
    expect(req).toMatchObject({ year: 2026, diary_number: 3, registry_code: '46001' });
  });
});

import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { type RectificativaAnterior, setDemoInvoke } from './api';
import { installDemo } from './demo';
import { anteriorTexto, RectificativaPanel } from './Rectificativa';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const anterior: RectificativaAnterior = {
  fuente: 'aeat',
  justificante: '3030000000017',
  presentada: '2026-10-20',
  declaracion: 'I',
  casillas: [[71, 80_010]],
  resultado: 80_010,
};

describe('RectificativaPanel', () => {
  it('dice de dónde sale lo presentado', () => {
    expect(anteriorTexto(anterior)).toBe(
      'Presentado el 20/10/2026 · según la comprobación con la AEAT · justificante 3030000000017 · resultado 800,10 €',
    );
    expect(anteriorTexto({ ...anterior, justificante: null, presentada: null })).toContain(
      'sin número de justificante',
    );
  });

  it('en la demo recalcula con la fecha prevista: diferencia, recargo y total', async () => {
    installDemo();
    const onClose = vi.fn();
    render(
      <RectificativaPanel
        model="303"
        year={2026}
        quarter={3}
        anterior={anterior}
        labels={{ 9: 'Cuota al 21 %' }}
        onClose={onClose}
      />,
    );
    const panel = screen.getByRole('region', { name: 'Rectificar el 303 de 3T 2026' });
    expect(await within(panel).findByText(/73,50 € más a ingresar/)).toBeTruthy();
    expect(
      (within(panel).getByLabelText('Justificante de la anterior') as HTMLInputElement).value,
    ).toBe('3030000000017');
    fireEvent.change(within(panel).getByLabelText('Fecha prevista de presentación'), {
      target: { value: '2026-12-21' },
    });
    fireEvent.click(within(panel).getByRole('button', { name: 'Recalcular' }));
    // Dos meses completos desde el 20 de octubre: 3 % de 73,50 €.
    expect(await within(panel).findByText(/3 % · 2,21 €/)).toBeTruthy();
    expect(within(panel).getByText('75,71 €')).toBeTruthy();
    // La casilla que cambia, con su nombre y la diferencia.
    const fila = within(panel).getByText('Cuota al 21 %').closest('tr');
    expect(fila?.textContent).toContain('913,50 €');
    expect(fila?.textContent).toContain('987,00 €');
    expect(fila?.textContent).toContain('73,50 €');
    expect(
      within(panel).getByText('Ingresado con las autoliquidaciones anteriores del periodo'),
    ).toBeTruthy();
    fireEvent.click(within(panel).getByRole('button', { name: 'Cerrar' }));
    expect(onClose).toHaveBeenCalled();
  });

  it('«Ya devuelto» tiene que ser un importe', async () => {
    installDemo();
    render(
      <RectificativaPanel
        model="303"
        year={2026}
        quarter={3}
        anterior={anterior}
        labels={{}}
        onClose={() => {}}
      />,
    );
    await screen.findByText(/más a ingresar/);
    fireEvent.change(screen.getByLabelText('Ya devuelto por la AEAT'), {
      target: { value: 'mucho' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Recalcular' }));
    expect((await screen.findByRole('alert')).textContent).toContain('no es un importe');
  });
});

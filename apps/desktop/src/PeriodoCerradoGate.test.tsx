import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { command, setDemoInvoke } from './api';
import { PeriodoCerradoGate } from './PeriodoCerradoGate';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const AVISO = '[periodo-cerrado:3] El 3T 2026 está cerrado y vas a emitir una factura.';

describe('PeriodoCerradoGate', () => {
  it('pregunta, deja la excepción y repite la operación si se confirma', async () => {
    let intentos = 0;
    const llamadas: string[] = [];
    setDemoInvoke(async (cmd, args) => {
      llamadas.push(cmd);
      if (cmd === 'periodo_autorizar') {
        expect(args).toEqual(expect.objectContaining({ id: 3 }));
        return null;
      }
      intentos += 1;
      if (intentos === 1) throw { message: AVISO };
      return 'emitida';
    });
    render(<PeriodoCerradoGate />);
    const r = command<string>('invoice_issue', {});
    expect(await screen.findByText(/El 3T 2026 está cerrado/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Sí, seguir igualmente' }));
    await expect(r).resolves.toBe('emitida');
    expect(llamadas).toEqual(['invoice_issue', 'periodo_autorizar', 'invoice_issue']);
  });

  it('si no se confirma, no se hace nada', async () => {
    setDemoInvoke(async () => {
      throw { message: AVISO };
    });
    render(<PeriodoCerradoGate />);
    const r = command<string>('invoice_issue', {});
    fireEvent.click(await screen.findByRole('button', { name: 'No, dejarlo' }));
    await expect(r).rejects.toEqual({ message: expect.stringMatching(/^No se ha hecho nada/) });
  });
});

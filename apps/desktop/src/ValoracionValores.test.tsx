import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { setDemoInvoke } from './api';
import { ValoracionValores } from './ValoracionValores';
import type { CierreValores } from './valores';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const propuesta = (excluido: boolean, entry: number | null = null): CierreValores => ({
  year: 2026,
  fecha: '2026-12-31',
  cartera: 'coste',
  closed: false,
  valores: [
    {
      security_id: 1,
      ticker: 'ETFD',
      name: 'ETF Demo',
      kind: 'etf',
      cuenta: '5400001',
      libros: 600_600,
      valor: 480_000,
      ajuste: excluido ? 0 : -120_600,
      tratamiento: excluido ? 'excluido' : 'deterioro',
    },
  ],
  lines: excluido
    ? []
    : [
        { account: '698', name: 'Pérdidas por deterioro', debit: 120_600, credit: 0 },
        { account: '596', name: 'Deterioro de participaciones', debit: 0, credit: 120_600 },
      ],
  beneficios: 0,
  perdidas: 0,
  deterioro: excluido ? 0 : 120_600,
  reversion: 0,
  entry_id: entry,
  stale: false,
  input: { excluidos: excluido ? [1] : [] },
  avisos: [],
  notas: ['El deterioro no es deducible (art. 13.2.b LIS).'],
});

describe('ValoracionValores', () => {
  it('sin cartera no enseña nada', async () => {
    setDemoInvoke(async () => ({ ...propuesta(false), valores: [], lines: [] }));
    const { container } = render(<ValoracionValores year={2026} onChanged={() => {}} />);
    await new Promise((r) => setTimeout(r, 0));
    expect(container.textContent).toBe('');
  });

  it('propone el deterioro, deja excluir un valor y lo contabiliza tras confirmar', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    let posted = false;
    setDemoInvoke(async (cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'valores_closing') {
        const ex = (args as { input?: { excluidos: number[] } | null }).input?.excluidos ?? [];
        return propuesta(ex.includes(1), posted ? 9 : null);
      }
      if (cmd === 'valores_closing_post') {
        posted = true;
        return 9;
      }
      throw { message: `inesperado ${cmd}` };
    });
    let changed = 0;
    render(<ValoracionValores year={2026} onChanged={() => changed++} />);
    expect(await screen.findByText('Cartera de valores al cierre de 2026')).toBeTruthy();
    expect(screen.getByText('-1.206,00 €')).toBeTruthy();
    // «No es deterioro» quita el asiento.
    fireEvent.click(screen.getByLabelText('No es deterioro'));
    expect(await screen.findByText('No se deteriora')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Contabilizar la valoración' })).toBeNull();
    fireEvent.click(screen.getByLabelText('No es deterioro'));
    fireEvent.click(await screen.findByRole('button', { name: 'Contabilizar la valoración' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sí, contabilizar' }));
    expect(await screen.findByText('Contabilizada')).toBeTruthy();
    const post = calls.find((c) => c.cmd === 'valores_closing_post');
    expect(post?.args).toEqual({ year: 2026, input: { excluidos: [] }, confirmed: true });
    expect(changed).toBe(1);
  });
});

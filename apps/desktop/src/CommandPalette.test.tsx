import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setDemoInvoke } from './api';
import { CommandPalette } from './CommandPalette';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const commands = [
  { label: 'Facturas', hint: 'Emite facturas', jump: { tab: 'facturas' } },
  { label: 'Ajustes', hint: 'Certificado y copias', jump: { tab: 'ajustes' } },
];

describe('CommandPalette', () => {
  it('busca, se mueve con las flechas y abre con Intro', async () => {
    setDemoInvoke(async (cmd, args) => {
      expect(cmd).toBe('global_search');
      expect(args).toEqual({ query: 'turia' });
      return [
        { kind: 'contacto', title: 'Turia SL', subtitle: 'Cliente · B1', target: 'B1', date: null },
        {
          kind: 'asiento',
          title: 'Factura Turia',
          subtitle: 'Asiento 4/2025',
          target: '4:2025',
          date: null,
        },
      ];
    });
    const jump = vi.fn();
    const close = vi.fn();
    render(<CommandPalette open onClose={close} commands={commands} onJump={jump} />);
    const box = screen.getByRole('combobox', { name: 'Buscar' });
    fireEvent.change(box, { target: { value: 'turia' } });
    expect(await screen.findByText('Turia SL')).toBeTruthy();
    fireEvent.keyDown(box, { key: 'ArrowDown' });
    fireEvent.keyDown(box, { key: 'Enter' });
    expect(close).toHaveBeenCalled();
    expect(jump).toHaveBeenCalledWith({ tab: 'contabilidad', entry: 4, year: 2025 });
  });

  it('las órdenes se filtran sin tildes', () => {
    render(<CommandPalette open onClose={() => {}} commands={commands} onJump={() => {}} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'Buscar' }), {
      target: { value: 'a' },
    });
    expect(screen.getByRole('option', { name: /Facturas/ })).toBeTruthy();
    expect(screen.getByRole('option', { name: /Ajustes/ })).toBeTruthy();
  });
});

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setDemoInvoke } from './api';
import { ClaudeMcp } from './ClaudeMcp';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

describe('ClaudeMcp', () => {
  it('está apagado por defecto, dice qué sale sin anonimizar y guarda el nivel elegido', async () => {
    const calls: string[] = [];
    let ajustes = { activado: false, nivel: 'consultar' };
    setDemoInvoke(async (cmd, args) => {
      calls.push(`${cmd}:${JSON.stringify(args ?? {})}`);
      if (cmd === 'mcp_guardar') ajustes = args?.ajustes as typeof ajustes;
      return { ajustes, binario: null };
    });
    const { container } = render(<ClaudeMcp />);
    const text = container.textContent ?? '';
    expect(text).toContain('Va sin anonimizar');
    expect(text).toContain('clientes y proveedores con su NIF');
    expect(text).toContain('No puede emitir, presentar, firmar ni borrar nada');
    const box = screen.getByRole('switch', {
      name: /Dejar que Claude consulte esta empresa/,
    }) as HTMLInputElement;
    await vi.waitFor(() => expect(box.disabled).toBe(false));
    expect(box.checked).toBe(false);
    // Apagado, el nivel no se puede tocar.
    const borradores = screen.getByRole('radio', {
      name: /Consultar y dejar borradores/,
    }) as HTMLInputElement;
    expect(borradores.closest('fieldset')?.disabled).toBe(true);
    fireEvent.click(box);
    await vi.waitFor(() => expect(box.checked).toBe(true));
    expect(calls).toContain('mcp_guardar:{"ajustes":{"activado":true,"nivel":"consultar"}}');
    expect(screen.getByRole('status').textContent).toBe(
      'Activado: Claude puede consultar esta empresa.',
    );
    fireEvent.click(borradores);
    await vi.waitFor(() => expect(borradores.checked).toBe(true));
    expect(calls).toContain('mcp_guardar:{"ajustes":{"activado":true,"nivel":"borradores"}}');
    fireEvent.click(box);
    await vi.waitFor(() => expect(box.checked).toBe(false));
    expect(screen.getByRole('status').textContent).toBe(
      'Desactivado: Claude ya no ve esta empresa.',
    );
  });

  it('enseña la configuración con la ruta del programa si viene con la app', async () => {
    setDemoInvoke(async () => ({
      ajustes: { activado: true, nivel: 'borradores' },
      binario: '/Applications/TaxPain.app/Contents/MacOS/taxpain-mcp',
    }));
    const { container } = render(<ClaudeMcp />);
    await vi.waitFor(() =>
      expect(container.textContent).toContain(
        '"command": "/Applications/TaxPain.app/Contents/MacOS/taxpain-mcp"',
      ),
    );
    expect(container.textContent).toContain('claude mcp add taxpain --');
    expect(container.textContent).not.toContain('aún no viene con la app');
  });

  it('si no se puede guardar, lo dice y no cambia', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'mcp_guardar') throw { message: 'la base de datos está ocupada' };
      return { ajustes: { activado: false, nivel: 'consultar' }, binario: null };
    });
    render(<ClaudeMcp />);
    const box = screen.getByRole('switch') as HTMLInputElement;
    await vi.waitFor(() => expect(box.disabled).toBe(false));
    fireEvent.click(box);
    await vi.waitFor(() =>
      expect(screen.getByRole('alert').textContent).toBe('la base de datos está ocupada'),
    );
    expect(box.checked).toBe(false);
  });
});

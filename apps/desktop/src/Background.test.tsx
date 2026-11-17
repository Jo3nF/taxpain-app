import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setDemoInvoke } from './api';
import { Background } from './Background';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

describe('Background', () => {
  it('no promete mirar las notificaciones de la AEAT con la ventana cerrada', async () => {
    const calls: string[] = [];
    setDemoInvoke(async (cmd, args) => {
      calls.push(`${cmd}:${JSON.stringify(args ?? {})}`);
      if (cmd === 'autostart_get') return false;
      if (cmd === 'autostart_set') return true;
      return null;
    });
    const { container } = render(<Background />);
    const text = container.textContent ?? '';
    expect(text).toContain('plazos de los modelos');
    expect(text).toContain('carpeta vigilada');
    // Lo que no hace sola lo dice, con el plazo de los 10 días.
    expect(text).toContain('Las notificaciones de la AEAT no se miran solas');
    expect(text).toContain('10 días naturales');
    expect(text).not.toMatch(/te avisa[^.]*notificaciones de la AEAT/);
    const box = screen.getByRole('switch', {
      name: /Abrir TaxPain al iniciar sesión/,
    }) as HTMLInputElement;
    await vi.waitFor(() => expect(box.disabled).toBe(false));
    fireEvent.click(box);
    await vi.waitFor(() => expect(box.checked).toBe(true));
    expect(calls).toContain('autostart_set:{"enabled":true}');
  });
});

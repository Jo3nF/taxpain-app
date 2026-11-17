import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Menu } from './Menu';

afterEach(cleanup);

function setup() {
  const a = vi.fn();
  const b = vi.fn();
  const off = vi.fn();
  render(
    <Menu
      label="Acciones"
      entries={[
        { heading: 'Grupo' },
        { label: 'Primera', onSelect: a },
        { label: 'Apagada', onSelect: off, disabled: true },
        { separator: true },
        { label: 'Segunda', onSelect: b, hint: 'B' },
      ]}
    >
      Más
    </Menu>,
  );
  return { a, b, off, trigger: screen.getByRole('button', { name: 'Acciones' }) };
}

describe('Menu', () => {
  it('se abre, elige y devuelve el foco al botón', async () => {
    const { a, trigger } = setup();
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(trigger);
    expect(trigger.getAttribute('aria-expanded')).toBe('true');
    const first = screen.getByRole('menuitem', { name: 'Primera' });
    await vi.waitFor(() => expect(document.activeElement).toBe(first));
    fireEvent.click(first);
    expect(a).toHaveBeenCalledOnce();
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it('con el teclado salta las opciones apagadas y Escape cierra', async () => {
    const { b, off, trigger } = setup();
    fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    const menu = await screen.findByRole('menu');
    await vi.waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Primera' })),
    );
    fireEvent.keyDown(menu, { key: 'ArrowDown' });
    expect(document.activeElement?.textContent).toContain('Segunda');
    fireEvent.keyDown(menu, { key: 'ArrowDown' });
    expect(document.activeElement?.textContent).toBe('Primera');
    fireEvent.keyDown(menu, { key: 's' });
    expect(document.activeElement?.textContent).toContain('Segunda');
    fireEvent.click(screen.getByRole('menuitem', { name: 'Apagada' }));
    expect(off).not.toHaveBeenCalled();
    fireEvent.keyDown(menu, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(b).not.toHaveBeenCalled();
  });

  it('un clic fuera lo cierra sin elegir nada', () => {
    const { a, trigger } = setup();
    fireEvent.click(trigger);
    expect(screen.getByRole('menu')).toBeTruthy();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole('menu')).toBeNull();
    expect(a).not.toHaveBeenCalled();
  });
});

describe('Menu en un diálogo', () => {
  it('se pinta dentro del diálogo', () => {
    render(
      <dialog open aria-label="Diálogo">
        <Menu label="Opciones" entries={[{ label: 'Una', onSelect: () => {} }]}>
          Opciones
        </Menu>
      </dialog>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Opciones' }));
    const menu = screen.getByRole('menu');
    expect(menu.closest('dialog')).toBeTruthy();
  });
});

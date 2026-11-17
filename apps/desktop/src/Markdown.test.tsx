import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Markdown } from './Markdown';

describe('Markdown de las respuestas de la IA', () => {
  it('pinta negrita, listas y títulos sin los asteriscos', () => {
    const { container } = render(
      <Markdown
        text={
          'Significa que **la empresa pagó de más**.\n\n## Qué ha detectado\n\n- En el 200 hay **1.000,00 €** de dividendos.\n- La casilla `[00370]` está vacía.\n\n1. Primero\n2. Segundo'
        }
      />,
    );
    expect(screen.getByText('la empresa pagó de más').tagName).toBe('STRONG');
    expect(screen.getByRole('heading', { name: 'Qué ha detectado' })).toBeTruthy();
    expect(container.querySelectorAll('ul li')).toHaveLength(2);
    expect(container.querySelectorAll('ol li')).toHaveLength(2);
    expect(screen.getByText('[00370]').tagName).toBe('CODE');
    expect(container.textContent).not.toContain('**');
  });

  it('no mete HTML de la respuesta en la página', () => {
    const { container } = render(<Markdown text={'<img src=x onerror="alert(1)"> **hola**'} />);
    expect(container.querySelector('img')).toBeNull();
    expect(container.textContent).toContain('<img src=x onerror="alert(1)">');
  });

  it('respeta los saltos de línea dentro de un párrafo y la cursiva', () => {
    const { container } = render(<Markdown text={'Línea uno\nlínea *dos*'} />);
    expect(container.querySelectorAll('br')).toHaveLength(1);
    expect(screen.getByText('dos').tagName).toBe('EM');
  });
});

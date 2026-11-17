import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Hallazgo, Urgente } from './api';
import { BloqueUrgente, textoUrgente, urgentes } from './Urgentes';

afterEach(cleanup);

const U: Urgente = {
  en_juego: 100_000,
  vencio: '2026-07-20',
  porcentaje_hoy: 4,
  recargo_hoy: 4_000,
  siguiente_desde: '2026-11-20',
  siguiente_porcentaje: 5,
  siguiente_recargo: 5_000,
  nota: 'Art. 27 LGT.',
};

const h = (titulo: string, urgente?: Urgente): Hallazgo => ({
  codigo: 'cruces.fuera_de_plazo',
  area: 'plazos',
  gravedad: 'aviso',
  responsable: 'gestoria',
  ejercicio: 2026,
  titulo,
  detalle: '',
  ficheros: [],
  importe: null,
  urgente: urgente ?? null,
});

describe('Urgentes', () => {
  it('dice el recargo de hoy y cuánto sube el mes siguiente', () => {
    expect(textoUrgente(U)).toBe(
      'En juego: 1.000,00 €; venció el 20/07/2026; recargo hoy 4 %: 40,00 €; desde el 20/11/2026 sube al 5 %: 50,00 € (10,00 € más).',
    );
    expect(
      textoUrgente({
        ...U,
        en_juego: null,
        vencio: null,
        porcentaje_hoy: null,
        recargo_hoy: null,
        siguiente_porcentaje: null,
      }),
    ).toBe('Importe sin calcular.');
  });

  it('sólo coge los urgentes y abre la revisión desde el Panel', () => {
    const hs = [h('Normal'), h('Debe', U)];
    expect(urgentes(hs).map((x) => x.titulo)).toEqual(['Debe']);
    const go = vi.fn();
    render(<BloqueUrgente hallazgos={hs} id="x" compacto onOpen={{ label: 'Ver', go }} />);
    expect(screen.getByRole('region', { name: /Urgente: puede haber deuda/ })).toBeTruthy();
    expect(screen.queryByText('Normal')).toBeNull();
    expect(screen.queryByText('Art. 27 LGT.')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Ver' }));
    expect(go).toHaveBeenCalled();
  });

  it('sin urgentes no pinta nada', () => {
    const { container } = render(<BloqueUrgente hallazgos={[h('Normal')]} id="x" />);
    expect(container.innerHTML).toBe('');
  });
});

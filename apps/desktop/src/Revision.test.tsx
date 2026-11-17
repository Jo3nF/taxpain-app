import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AssistantTopic, Revision } from './api';

const api = vi.hoisted(() => ({
  folderGet: vi.fn(),
  folderReview: vi.fn(),
  folderReviewLast: vi.fn(),
  folderReviewSave: vi.fn(),
  pickFolder: vi.fn(),
  assistantConversations: vi.fn(),
  assistantStatus: vi.fn(),
  assistantTopicOpen: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { FolderReview, textoPedir, textoProximo200 } = await import('./Revision');

afterEach(() => {
  cleanup();
  for (const f of Object.values(api)) f.mockReset();
});

const REVIEW: Revision = {
  generada: '2026-10-25',
  empresa: 'Ejemplo Revisión SL',
  nif: 'B98765431',
  carpeta: '/carpeta',
  resumen: {
    ficheros: 18,
    con_detalle: 16,
    sin_leer: 1,
    no_admitidos: 1,
    por_tipo: [{ tipo: 'Facturas recibidas', n: 9 }],
    errores: 1,
    avisos: 2,
    info: 1,
    recuperable: 336_000,
  },
  hallazgos: [
    {
      codigo: 'sociedades.exencion_dividendos',
      area: 'sociedades',
      gravedad: 'error',
      responsable: 'gestoria',
      ejercicio: 2025,
      titulo: 'Dividendos de 2025 sin la exención del art. 21 LIS',
      detalle: 'Tributaron enteros.',
      ficheros: ['MODELOS/2025/200.pdf'],
      importe: 456_000,
      rectificable_hasta: '2030-07-27',
    },
    {
      codigo: 'sociedades.bases_negativas',
      area: 'sociedades',
      gravedad: 'aviso',
      responsable: 'gestoria',
      ejercicio: 2025,
      titulo: 'Bases negativas compensadas de más en 2025: 5.000,00 €',
      detalle: 'Sólo quedaban 10.000,00 €.',
      ficheros: ['MODELOS/2025/200.pdf'],
      importe: -120_000,
      rectificable_hasta: '2028-07-25',
      rectificable_aprox: true,
    },
    {
      codigo: 'facturas.nif_control',
      area: 'facturas',
      gravedad: 'aviso',
      responsable: 'empresa',
      ejercicio: null,
      titulo: '1 NIF con la letra de control mal',
      detalle: 'Pide al emisor una factura corregida.',
      ficheros: ['FACTURAS/2025/T2/GASTOS/nif-mal.pdf'],
      importe: null,
    },
    {
      codigo: 'modelos.sin_perfil',
      area: 'modelos',
      gravedad: 'info',
      responsable: 'empresa',
      ejercicio: null,
      titulo: 'Sin perfil fiscal: no se sabe qué modelos tocaban',
      detalle: 'Configura el perfil fiscal.',
      ficheros: [],
      importe: null,
    },
  ],
  tablas: [
    {
      titulo: 'Impuesto sobre Sociedades por ejercicio',
      columnas: ['Ejercicio', 'Cuota íntegra [00562]'],
      filas: [
        ['2024', '0,00 €'],
        ['2025', '15.600,00 €'],
      ],
    },
  ],
  pedir: [
    {
      a_quien: 'gestoria',
      que: 'Rectificar el Impuesto sobre Sociedades de 2025',
    },
    { a_quien: 'empresa', que: 'Una factura corregida de Imprenta Ejemplo' },
  ],
};

beforeEach(() => {
  api.folderGet.mockResolvedValue('/carpeta');
  api.folderReview.mockResolvedValue(REVIEW);
  api.folderReviewLast.mockResolvedValue(null);
  api.assistantConversations.mockResolvedValue([]);
  api.assistantStatus.mockResolvedValue({
    settings: { provider: { kind: 'Anthropic', model: 'claude-opus-5-5' }, consent: true },
    has_api_key: true,
    remote: true,
  });
  api.assistantTopicOpen.mockImplementation((topic: AssistantTopic) =>
    Promise.resolve({
      header: {
        group: 'Revisión de la carpeta · 2025',
        title: topic.kind === 'hallazgo' ? topic.title : '',
        facts: [],
        intro: 'Al revisar la carpeta de la empresa sale esto: Tributaron enteros.',
        suggestions: ['¿Qué significa y es grave?'],
        notice: null,
      },
      conversation: null,
      changed: false,
    }),
  );
});

async function revisar() {
  render(<FolderReview />);
  fireEvent.click(await screen.findByRole('button', { name: 'Revisar la carpeta' }));
  await screen.findByRole('heading', { name: /Resumen/ });
}

describe('Revisión de la carpeta', () => {
  it('revisa la carpeta recordada y pinta el resumen, los hallazgos y las tablas', async () => {
    await revisar();
    expect(api.folderReview).toHaveBeenCalledWith('/carpeta');
    const cifra = (t: string) =>
      screen.getByText(t, { selector: 'dt' }).nextElementSibling?.textContent;
    expect(cifra('Recuperable estimado')).toBe('3.360,00 €');
    expect(cifra('Errores')).toBe('1');
    expect(cifra('Avisos')).toBe('2');
    for (const h of REVIEW.hallazgos) {
      expect(screen.getByRole('heading', { name: h.titulo })).toBeTruthy();
    }
    const fichero = screen.getAllByText('1 fichero')[0];
    expect(fichero?.closest('details')?.textContent).toContain('MODELOS/2025/200.pdf');
    expect(screen.getByText('Recuperable 4.560,00 €')).toBeTruthy();
    expect(screen.getByText('A pagar 1.200,00 €')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Qué pedir' })).toBeTruthy();
    expect(screen.getByText('Rectificar el Impuesto sobre Sociedades de 2025')).toBeTruthy();
    expect(screen.getByText('Impuesto sobre Sociedades por ejercicio')).toBeTruthy();
    expect(screen.getByRole('cell', { name: '15.600,00 €' })).toBeTruthy();
  });

  it('dice hasta cuándo se puede rectificar y si la fecha es aproximada', async () => {
    await revisar();
    expect(
      screen.getByText(
        'Se puede rectificar hasta el 27/07/2030 (cuatro años desde el fin del plazo, art. 66 LGT).',
      ),
    ).toBeTruthy();
    expect(
      screen.getByText(
        'Se puede rectificar hasta el 25/07/2028 aprox. (cuatro años desde el fin del plazo, art. 66 LGT; sin los festivos de ese año, el día exacto puede ser algo después).',
      ),
    ).toBeTruthy();
  });

  it('filtra por gravedad, área y responsable', async () => {
    await revisar();
    fireEvent.change(screen.getByLabelText('Gravedad'), { target: { value: 'error' } });
    expect(screen.getByText('1 de 4')).toBeTruthy();
    expect(screen.queryByText('1 NIF con la letra de control mal')).toBeNull();
    fireEvent.change(screen.getByLabelText('Gravedad'), { target: { value: 'todas' } });
    fireEvent.change(screen.getByLabelText('Responsable'), { target: { value: 'empresa' } });
    expect(screen.getByText('2 de 4')).toBeTruthy();
    expect(screen.queryByText(/Dividendos de 2025/)).toBeNull();
    fireEvent.change(screen.getByLabelText('Área'), { target: { value: 'sociedades' } });
    expect(screen.getByText('Nada con estos filtros.')).toBeTruthy();
  });

  it('guarda el informe en el formato elegido', async () => {
    api.folderReviewSave.mockResolvedValue(true);
    await revisar();
    fireEvent.click(screen.getByRole('button', { name: 'Guardar el informe en Excel' }));
    expect(await screen.findByText('Informe guardado.')).toBeTruthy();
    expect(api.folderReviewSave).toHaveBeenCalledWith(REVIEW, 'xlsx');
  });

  it('sin carpeta, la elige y la revisa', async () => {
    api.folderGet.mockResolvedValue(null);
    api.pickFolder.mockResolvedValue('/otra');
    render(<FolderReview />);
    fireEvent.click(await screen.findByRole('button', { name: 'Elegir carpeta…' }));
    await screen.findByRole('heading', { name: /Resumen/ });
    expect(api.folderReview).toHaveBeenCalledWith('/otra');
  });

  it('enseña el error si no se puede revisar', async () => {
    api.folderReview.mockRejectedValue('la carpeta no existe');
    render(<FolderReview />);
    fireEvent.click(await screen.findByRole('button', { name: 'Revisar la carpeta' }));
    expect((await screen.findByRole('alert')).textContent).toContain('la carpeta no existe');
  });

  it('pone arriba, destacado, lo urgente que puede ser una deuda', async () => {
    api.folderReview.mockResolvedValue({
      ...REVIEW,
      hallazgos: [
        {
          codigo: 'cruces.pago_no_esta',
          area: 'bancos',
          gravedad: 'aviso',
          responsable: 'empresa',
          ejercicio: 2026,
          titulo: 'No se ve el pago del 111 del 2T 2026 en el banco',
          detalle: 'Sale a ingresar 1.000,00 € y no hay cargo.',
          ficheros: [],
          importe: null,
          urgente: {
            en_juego: 100_000,
            vencio: '2026-07-20',
            porcentaje_hoy: 5,
            recargo_hoy: 5_000,
            siguiente_desde: null,
            siguiente_porcentaje: 10,
            siguiente_recargo: 10_000,
            nota: 'Periodo ejecutivo (art. 28 LGT).',
          },
        },
        ...REVIEW.hallazgos,
      ],
    });
    await revisar();
    const bloque = screen.getByRole('region', { name: /Urgente: puede haber deuda/ });
    expect(
      within(bloque).getByText('No se ve el pago del 111 del 2T 2026 en el banco'),
    ).toBeTruthy();
    expect(bloque.textContent).toContain('recargo hoy 5 %: 50,00 €');
    expect(bloque.textContent).toContain('en juego 1.050,00 €');
    // Antes del resumen.
    const resumen = screen.getByRole('heading', { name: /Resumen/ });
    expect(bloque.compareDocumentPosition(resumen) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // Y en la lista, con su marca.
    expect(screen.getByText('Urgente: puede haber deuda', { selector: '.pill' })).toBeTruthy();
  });

  it('sin urgentes no pinta el bloque', async () => {
    await revisar();
    expect(screen.queryByRole('region', { name: /Urgente: puede haber deuda/ })).toBeNull();
  });

  it('enseña la última revisión guardada sin volver a revisar', async () => {
    api.folderReviewLast.mockResolvedValue(REVIEW);
    render(<FolderReview />);
    expect(await screen.findByText(/revisada el 25\/10\/2026/)).toBeTruthy();
    expect(api.folderReview).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Revisar la carpeta' })).toBeTruthy();
  });

  it('cada hallazgo abre su conversación con la IA', async () => {
    api.assistantConversations.mockResolvedValue([
      {
        id: 3,
        topic: {
          kind: 'hallazgo',
          origin: 'carpeta',
          code: 'facturas.nif_control',
          year: null,
          title: '1 NIF con la letra de control mal',
        },
        title: '1 NIF con la letra de control mal',
        updated_at: '2026-10-30T10:00:00Z',
        questions: 1,
        materials: 0,
      },
    ]);
    await revisar();
    // Lo único que sale es lo que se pregunte a la IA, y se dice.
    expect(screen.getByText(/ni envía nada \(salvo lo que preguntes a la IA\)/)).toBeTruthy();
    expect(
      screen.getByRole('button', {
        name: 'Seguir con la IA: 1 NIF con la letra de control mal, 1 pregunta',
      }),
    ).toBeTruthy();
    const ask = screen.getByRole('button', {
      name: 'Preguntar a la IA: Dividendos de 2025 sin la exención del art. 21 LIS',
    });
    fireEvent.click(ask);
    const dialog = await screen.findByRole('dialog', {
      name: 'Conversación con la IA: Dividendos de 2025 sin la exención del art. 21 LIS',
    });
    expect(api.assistantTopicOpen).toHaveBeenCalledWith({
      kind: 'hallazgo',
      origin: 'carpeta',
      code: 'sociedades.exencion_dividendos',
      year: 2025,
      title: 'Dividendos de 2025 sin la exención del art. 21 LIS',
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cerrar' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(document.activeElement).toBe(ask);
  });

  it('prepara la lista de lo que hay que pedir para un correo', () => {
    expect(textoPedir(REVIEW.pedir)).toBe(
      'A la gestoría:\n- Rectificar el Impuesto sobre Sociedades de 2025\n\n' +
        'En la empresa:\n- Una factura corregida de Imprenta Ejemplo',
    );
  });
  it('pone aparte lo que va en el próximo 200: las retenciones y la exención', async () => {
    api.folderReview.mockResolvedValue({
      ...REVIEW,
      resumen: {
        ...REVIEW.resumen,
        a_favor_proximo_200: 192_105,
        retenciones_proximo_200: 200_000,
        exencion_proximo_200: 150_000,
        impuesto_dividendos_proximo_200: 7_895,
      },
      hallazgos: [
        {
          codigo: 'sociedades.retencion_dividendos_grupo',
          area: 'sociedades',
          gravedad: 'error',
          responsable: 'gestoria',
          ejercicio: 2026,
          titulo:
            'Retención del 19 % sobre dividendos del grupo que no se debía practicar: 2.000,00 € adelantados en 2026',
          detalle: 'Se resta en el 200 de 2026.',
          ficheros: [],
          importe: 200_000,
        },
      ],
    });
    await revisar();
    const cifra = (t: string) =>
      screen.getByText(t, { selector: 'dt' }).nextElementSibling?.textContent;
    // No suma a lo recuperable corrigiendo modelos.
    expect(cifra('Recuperable estimado')).toBe('3.360,00 €');
    expect(screen.getByText('A favor en el próximo 200 2.000,00 €')).toBeTruthy();
    expect(
      screen.getByText(
        /En el próximo 200 se recuperan unos 1\.921,05 €: 2\.000,00 € de retenciones soportadas, que se restan de la cuota, menos 78,95 € de impuesto.*Si no se aplica la exención.*se pierden unos 1\.500,00 €/,
      ),
    ).toBeTruthy();
  });

  it('pone aparte las devoluciones pedidas, pendientes de cobrar', async () => {
    api.folderReview.mockResolvedValue({
      ...REVIEW,
      resumen: { ...REVIEW.resumen, pendiente_cobro: 395_485 },
      devoluciones_pendientes: [
        {
          modelo: '200',
          ejercicio: 2025,
          periodo: '0A',
          importe: 218_500,
          estimado: false,
          presentada: '2026-11-05',
          via: 'autoliquidación rectificativa',
          estado: 'pendiente: Hacienda tiene hasta el 05/05/2027',
        },
        {
          modelo: '200',
          ejercicio: 2023,
          periodo: '0A',
          importe: 176_985,
          estimado: true,
          presentada: '2026-11-10',
          via: 'solicitud de rectificación (registro RGE000000012026)',
          estado: 'pendiente de resolver: Hacienda tiene hasta el 10/05/2027',
        },
      ],
    });
    await revisar();
    const caja = screen.getByRole('region', {
      name: 'Devoluciones pedidas, pendientes de cobrar: 3.954,85 €',
    });
    const lineas = within(caja).getAllByRole('listitem');
    expect(lineas.map((l) => l.textContent)).toEqual([
      'El 200 de 2025: 2.185,00 €, pedida por autoliquidación rectificativa el 05/11/2026; pendiente: Hacienda tiene hasta el 05/05/2027.',
      'El 200 de 2023: 1.769,85 € (estimación), pedida por solicitud de rectificación (registro RGE000000012026) el 10/11/2026; pendiente de resolver: Hacienda tiene hasta el 10/05/2027.',
    ]);
    // Ya pedidas: no suman a lo recuperable.
    const cifra = (t: string) =>
      screen.getByText(t, { selector: 'dt' }).nextElementSibling?.textContent;
    expect(cifra('Recuperable estimado')).toBe('3.360,00 €');
  });

  it('sin devoluciones pedidas no pone el recuadro', async () => {
    api.folderReview.mockResolvedValue(REVIEW);
    await revisar();
    expect(screen.queryByText(/Devoluciones pedidas, pendientes de cobrar/)).toBeNull();
  });

  it('sin dividendos confirmados no dice nada del próximo 200', () => {
    expect(textoProximo200(REVIEW.resumen)).toBeNull();
    expect(
      textoProximo200({
        ...REVIEW.resumen,
        a_favor_proximo_200: 0,
        retenciones_proximo_200: 0,
        exencion_proximo_200: 0,
      }),
    ).toBeNull();
  });
});

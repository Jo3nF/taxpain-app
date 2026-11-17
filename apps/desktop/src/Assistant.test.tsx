import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  AssistantStatus,
  Conversation,
  ConversationSummary,
  TopicHeader,
  TopicOpened,
} from './api';

const api = vi.hoisted(() => ({
  assistantStatus: vi.fn(),
  assistantSave: vi.fn(),
  assistantSetKey: vi.fn(),
  assistantConversations: vi.fn(),
  assistantTopicOpen: vi.fn(),
  assistantConversationOpen: vi.fn(),
  assistantTopicAsk: vi.fn(),
  assistantTopicPreview: vi.fn(),
  assistantConversationDelete: vi.fn(),
  assistantConversationsDeleteAll: vi.fn(),
  assistantFactAdd: vi.fn(),
  assistantFileAdd: vi.fn(),
  pickAssistantFile: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { Assistant } = await import('./Assistant');
const { useAssistantPanel } = await import('./AssistantPanel');
const { AssistantSetup } = await import('./AssistantParts');

const configured = {
  settings: { provider: { kind: 'Anthropic', model: 'claude-opus-5-5' }, consent: true },
  has_api_key: true,
  remote: true,
};

const GENERAL: TopicHeader = {
  group: 'Asistente',
  title: 'Nueva conversación',
  facts: [],
  intro: 'Pregunta sobre tus impuestos: la IA ve un resumen de tus cifras, plazos y avisos.',
  suggestions: ['¿Cuánto IVA me toca pagar este trimestre y por qué?'],
  notice: null,
};

const general = (id: number, title: string, answer: string): Conversation => ({
  id,
  topic: { kind: 'general' },
  title,
  created_at: '2026-10-30T10:00:00Z',
  updated_at: '2026-10-30T10:01:00Z',
  messages: [
    { seq: 1, role: 'user', content: title, created_at: '2026-10-30T10:00:00Z', notes: null },
    {
      seq: 2,
      role: 'assistant',
      content: answer,
      created_at: '2026-10-30T10:01:00Z',
      notes: {
        model: 'claude-opus-5-5',
        replaced: 1,
        used: ['borrador_modelo', 'leer_articulo'],
        sources: ['BOE-A-1992-28740, artículo 99'],
        unverified: ['el IVA se deduce siempre en el trimestre siguiente'],
      },
    },
  ],
  materials: [],
  full: false,
});
const askReply = (c: Conversation): TopicOpened => ({
  header: { ...GENERAL, title: c.title },
  conversation: c,
  changed: false,
});

const CRIPTO: ConversationSummary = {
  id: 1,
  topic: { kind: 'ahorro', id: 'cripto_perdidas', year: 2026 },
  title: 'Criptomonedas: compensa ganancias con pérdidas antes del 31 de diciembre',
  updated_at: '2026-10-12T17:41:00Z',
  questions: 1,
  materials: 0,
};
const IVA: ConversationSummary = {
  id: 2,
  topic: { kind: 'general' },
  title: '¿Cuándo presento el 303?',
  updated_at: '2026-10-11T09:00:00Z',
  questions: 2,
  materials: 1,
};

beforeEach(() => {
  api.assistantStatus.mockResolvedValue(configured);
  api.assistantConversations.mockResolvedValue([]);
  api.assistantTopicOpen.mockResolvedValue({ header: GENERAL, conversation: null, changed: false });
});

afterEach(() => {
  cleanup();
  for (const f of Object.values(api)) f.mockReset();
});

describe('Assistant', () => {
  it('sin consentimiento pide configurarlo y no deja guardar sin aceptar', async () => {
    api.assistantStatus.mockResolvedValue({
      ...configured,
      settings: { ...configured.settings, consent: false },
      has_api_key: false,
    });
    render(<Assistant />);
    fireEvent.click(await screen.findByRole('button', { name: 'Guardar' }));
    expect((await screen.findByRole('alert')).textContent).toContain('acepta el envío');
    expect(api.assistantSave).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText(/Clave de API/), { target: { value: 'sk-ant-x' } });
    fireEvent.click(screen.getByRole('checkbox'));
    api.assistantSave.mockResolvedValue(undefined);
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    // La clave va con el proveedor, en la misma llamada (un solo Touch ID).
    await vi.waitFor(() => expect(api.assistantSave).toHaveBeenCalledTimes(1));
    expect(api.assistantSave.mock.calls[0]?.[0].consent).toBe(true);
    expect(api.assistantSave.mock.calls[0]?.[1]).toBe('sk-ant-x');
    expect(api.assistantSetKey).not.toHaveBeenCalled();
  });

  it('si se cancela Touch ID al guardar la clave, lo dice y no da nada por guardado', async () => {
    const status: AssistantStatus = {
      settings: { provider: { kind: 'Anthropic', model: 'claude-opus-5-5' }, consent: true },
      has_api_key: false,
      remote: true,
    };
    const onSaved = vi.fn();
    api.assistantSave.mockRejectedValue('operación no confirmada por el usuario');
    render(<AssistantSetup status={status} onSaved={onSaved} />);
    fireEvent.change(screen.getByLabelText(/Clave de API/), { target: { value: 'sk-ant-x' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    expect((await screen.findByRole('alert')).textContent).toContain('No se ha hecho nada');
    expect(api.assistantSave).toHaveBeenCalledTimes(1);
    expect(onSaved).not.toHaveBeenCalled();
  });

  it('pregunta, muestra la respuesta y enseña qué se envía', async () => {
    api.assistantTopicAsk.mockResolvedValue(
      askReply(general(9, '¿Cuánto IVA pago?', 'Te tocan 410,00 € de IVA.')),
    );
    api.assistantTopicPreview.mockResolvedValue({
      system: 'Eres el asistente… [NIF_1]',
      messages: [{ role: 'user', content: '¿Y el 130?' }],
      replaced: 1,
      omitted: 0,
    });
    render(<Assistant />);
    const box = await screen.findByLabelText('Tu pregunta');
    fireEvent.change(box, { target: { value: '¿Cuánto IVA pago?' } });
    fireEvent.click(screen.getByRole('button', { name: 'Preguntar' }));
    expect(await screen.findByText('Te tocan 410,00 € de IVA.')).toBeTruthy();
    expect(api.assistantTopicAsk).toHaveBeenCalledWith(
      { kind: 'general' },
      null,
      '¿Cuánto IVA pago?',
    );
    expect(
      screen.getByText('Ha consultado (sólo lectura): borrador de un modelo, artículo del BOE.'),
    ).toBeTruthy();
    expect(screen.getByText(/Normas leídas en el BOE: BOE-A-1992-28740, artículo 99/)).toBeTruthy();
    expect(screen.getByText(/no aparecen en lo que ha leído/)).toBeTruthy();
    fireEvent.change(box, { target: { value: '¿Y el 130?' } });
    fireEvent.click(screen.getByRole('button', { name: 'Ver qué se envía' }));
    expect(
      await screen.findByText('Esto es exactamente lo que se envía (1 dato sustituido)'),
    ).toBeTruthy();
    expect(api.assistantTopicPreview).toHaveBeenCalledWith({ kind: 'general' }, 9, '¿Y el 130?');
  });

  it('si falla, devuelve la pregunta al cuadro y avisa', async () => {
    api.assistantTopicAsk.mockRejectedValue({ message: 'falta la clave de API del proveedor' });
    render(<Assistant />);
    const box = (await screen.findByLabelText('Tu pregunta')) as HTMLTextAreaElement;
    fireEvent.change(box, { target: { value: 'Hola' } });
    fireEvent.click(screen.getByRole('button', { name: 'Preguntar' }));
    expect((await screen.findByRole('alert')).textContent).toContain('clave de API');
    expect(box.value).toBe('Hola');
  });

  it('lista las guardadas y abre una de un tema', async () => {
    api.assistantConversations.mockResolvedValue([CRIPTO, IVA]);
    api.assistantConversationOpen.mockResolvedValue({
      header: { ...GENERAL, group: 'Ahorrar impuestos · 2026', title: CRIPTO.title },
      conversation: {
        ...general(1, '¿Cuánto me ahorro si vendo las que pierden?', 'Unos 117,80 €.'),
        topic: CRIPTO.topic,
        title: CRIPTO.title,
      },
      changed: false,
    });
    render(<Assistant />);
    const list = await screen.findByRole('region', { name: 'Conversaciones guardadas' });
    expect(within(list).getByText(/Se guardan cifradas en tu Mac/)).toBeTruthy();
    expect(await within(list).findByText(/^Ahorrar impuestos 2026 · 1 pregunta · /)).toBeTruthy();
    expect(within(list).getByText(/^General · 2 preguntas · /)).toBeTruthy();
    const item = within(list).getByRole('button', { name: /^Criptomonedas: compensa/ });
    item.focus();
    fireEvent.click(item);
    expect(await screen.findByText('Unos 117,80 €.')).toBeTruthy();
    expect(api.assistantConversationOpen).toHaveBeenCalledWith(1);
    expect(item.getAttribute('aria-current')).toBe('true');
    expect(
      screen.getByRole('heading', { name: `Conversación con la IA: ${CRIPTO.title}` }),
    ).toBeTruthy();
    await waitFor(() =>
      expect(
        screen.getAllByRole('status').some((s) => s.textContent === `Abierta: «${CRIPTO.title}».`),
      ).toBe(true),
    );
    expect(document.activeElement).toBe(item);
  });

  it('cada guardada dice de qué pantalla es', async () => {
    const tema = (id: number, topic: ConversationSummary['topic'], title: string) => ({
      id,
      topic,
      title,
      updated_at: '2026-10-30T10:00:00Z',
      questions: 1,
      materials: 0,
    });
    api.assistantConversations.mockResolvedValue([
      tema(
        3,
        {
          kind: 'hallazgo',
          origin: 'aeat',
          code: 'cruces.390_303',
          year: 2024,
          title: 'El 390 de 2024 no cuadra con sus 303',
        },
        'El 390 de 2024 no cuadra con sus 303',
      ),
      tema(
        4,
        {
          kind: 'hallazgo',
          origin: 'carpeta',
          code: 'ficheros.copias',
          year: null,
          title: '1 copia de un fichero con el mismo contenido',
        },
        '1 copia de un fichero con el mismo contenido',
      ),
      tema(
        5,
        { kind: 'modelo', model: '303', year: 2026, period: 3 },
        'Borrador del modelo 303 · 3T 2026',
      ),
    ]);
    render(<Assistant />);
    const list = await screen.findByRole('region', { name: 'Conversaciones guardadas' });
    expect(await within(list).findByText(/^Qué revisar 2024 · 1 pregunta · /)).toBeTruthy();
    expect(within(list).getByText(/^Revisión de la carpeta · 1 pregunta · /)).toBeTruthy();
    expect(within(list).getByText(/^Modelos 2026 · 1 pregunta · /)).toBeTruthy();
  });

  it('borrar una y borrar todas piden confirmación', async () => {
    api.assistantConversations.mockResolvedValue([CRIPTO, IVA]);
    api.assistantConversationDelete.mockResolvedValue(undefined);
    api.assistantConversationsDeleteAll.mockResolvedValue(2);
    render(<Assistant />);
    const list = await screen.findByRole('region', { name: 'Conversaciones guardadas' });
    fireEvent.click(await within(list).findByRole('button', { name: `Borrar: ${IVA.title}` }));
    const confirm = within(list).getByRole('alertdialog');
    expect(confirm.textContent).toContain('No se puede deshacer');
    fireEvent.click(within(confirm).getByRole('button', { name: 'Borrar' }));
    await waitFor(() => expect(api.assistantConversationDelete).toHaveBeenCalledWith(2));
    await waitFor(() => expect(within(list).queryByText(IVA.title)).toBeNull());
    // Con una sola ya no sale «Borrar todas».
    expect(within(list).queryByRole('button', { name: 'Borrar todas' })).toBeNull();
    cleanup();
    api.assistantConversations.mockResolvedValue([CRIPTO, IVA]);
    render(<Assistant />);
    fireEvent.click(await screen.findByRole('button', { name: 'Borrar todas' }));
    const all = screen.getByRole('alertdialog');
    expect(all.textContent).toContain('Se borran las 2 conversaciones guardadas');
    fireEvent.click(within(all).getByRole('button', { name: 'Borrar todas' }));
    await waitFor(() => expect(api.assistantConversationsDeleteAll).toHaveBeenCalled());
    expect(await screen.findByText(/Aún no hay ninguna/)).toBeTruthy();
    await waitFor(() =>
      expect(document.activeElement).toBe(
        screen.getByRole('heading', { name: 'Conversaciones guardadas' }),
      ),
    );
  });

  it('una pregunta nueva crea una conversación general y aparece en la lista', async () => {
    const first = general(5, '¿Cuánto IVA pago?', 'Unos 410,00 €.');
    // Al llegar cada respuesta, la lista se vuelve a leer: ya está guardada.
    api.assistantConversations.mockResolvedValueOnce([]).mockResolvedValue([
      {
        id: 5,
        topic: { kind: 'general' },
        title: first.title,
        updated_at: first.updated_at,
        questions: 1,
        materials: 0,
      },
    ]);
    api.assistantTopicAsk.mockResolvedValueOnce(askReply(first)).mockResolvedValueOnce(
      askReply({
        ...first,
        messages: [
          ...first.messages,
          {
            seq: 3,
            role: 'user',
            content: '¿Y el plazo?',
            created_at: '2026-10-30T10:02:00Z',
            notes: null,
          },
          {
            seq: 4,
            role: 'assistant',
            content: 'El 20.',
            created_at: '2026-10-30T10:03:00Z',
            notes: null,
          },
        ],
      }),
    );
    render(<Assistant />);
    expect(await screen.findByText(/Aún no hay ninguna/)).toBeTruthy();
    const box = await screen.findByLabelText('Tu pregunta');
    fireEvent.change(box, { target: { value: '¿Cuánto IVA pago?' } });
    fireEvent.click(screen.getByRole('button', { name: 'Preguntar' }));
    expect(await screen.findByText('Unos 410,00 €.')).toBeTruthy();
    const list = screen.getByRole('region', { name: 'Conversaciones guardadas' });
    const item = await within(list).findByRole('button', { name: '¿Cuánto IVA pago?' });
    expect(item.getAttribute('aria-current')).toBe('true');
    fireEvent.change(box, { target: { value: '¿Y el plazo?' } });
    fireEvent.click(screen.getByRole('button', { name: 'Preguntar' }));
    expect(await screen.findByText('El 20.')).toBeTruthy();
    expect(api.assistantTopicAsk).toHaveBeenLastCalledWith({ kind: 'general' }, 5, '¿Y el plazo?');
    expect(within(list).getAllByRole('button', { name: '¿Cuánto IVA pago?' })).toHaveLength(1);
  });

  it('mientras espera la primera respuesta de una nueva, no deja añadir nada', async () => {
    let answer: (o: TopicOpened) => void = () => undefined;
    api.assistantTopicAsk.mockReturnValue(
      new Promise<TopicOpened>((r) => {
        answer = r;
      }),
    );
    render(<Assistant />);
    const box = await screen.findByLabelText('Tu pregunta');
    fireEvent.change(box, { target: { value: '¿Cuánto IVA pago?' } });
    fireEvent.click(screen.getByRole('button', { name: 'Preguntar' }));
    // Lo añadido acabaría en otra conversación (la nueva aún no tiene id).
    const dato = screen.getByRole('button', { name: 'Añadir un dato' });
    expect(dato.getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(dato);
    expect(screen.queryByLabelText('Dato para la IA')).toBeNull();
    const doc = screen.getByRole('button', { name: /^Añadir un documento…/ });
    expect(doc.getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(doc);
    expect(api.pickAssistantFile).not.toHaveBeenCalled();
    answer(askReply(general(5, '¿Cuánto IVA pago?', 'Unos 410,00 €.')));
    expect(await screen.findByText('Unos 410,00 €.')).toBeTruthy();
    // Con su id, ya se puede.
    expect(
      screen.getByRole('button', { name: 'Añadir un dato' }).getAttribute('aria-disabled'),
    ).toBeNull();
  });

  it('mientras se guarda lo primero que se añade a una nueva, no deja preguntar', async () => {
    let saved: (c: Conversation) => void = () => undefined;
    api.assistantFactAdd.mockReturnValue(
      new Promise<Conversation>((r) => {
        saved = r;
      }),
    );
    api.assistantTopicAsk.mockResolvedValue(
      askReply(general(6, '¿Cuánto IVA pago?', 'Unos 410,00 €.')),
    );
    render(<Assistant />);
    const box = await screen.findByLabelText('Tu pregunta');
    fireEvent.click(screen.getByRole('button', { name: 'Añadir un dato' }));
    fireEvent.change(screen.getByLabelText('Dato para la IA'), {
      target: { value: 'Tenemos 3 empleados.' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Añadir' }));
    fireEvent.change(box, { target: { value: '¿Cuánto IVA pago?' } });
    const preguntar = screen.getByRole('button', { name: 'Preguntar' });
    expect(preguntar.getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(preguntar);
    fireEvent.keyDown(box, { key: 'Enter', metaKey: true });
    expect(api.assistantTopicAsk).not.toHaveBeenCalled();
    saved({
      ...general(6, 'Conversación del 31/10/2026', ''),
      messages: [],
      materials: [
        {
          id: 1,
          kind: 'dato',
          content: 'Tenemos 3 empleados.',
          filename: null,
          read_by: null,
          truncated: false,
          chars: 20,
          created_at: '2026-10-31T10:00:00Z',
        },
      ],
    });
    await waitFor(() => expect(preguntar.getAttribute('aria-disabled')).toBeNull());
    fireEvent.click(preguntar);
    expect(api.assistantTopicAsk).toHaveBeenCalledWith({ kind: 'general' }, 6, '¿Cuánto IVA pago?');
    expect(await screen.findByText('Unos 410,00 €.')).toBeTruthy();
  });

  it('en la página, si tarda, no dice que se cierre un panel', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      let answer: (o: TopicOpened) => void = () => undefined;
      api.assistantTopicAsk.mockReturnValue(
        new Promise<TopicOpened>((r) => {
          answer = r;
        }),
      );
      render(<Assistant />);
      const box = await screen.findByLabelText('Tu pregunta');
      fireEvent.change(box, { target: { value: '¿Cuánto IVA pago?' } });
      fireEvent.click(screen.getByRole('button', { name: 'Preguntar' }));
      await act(async () => {
        vi.advanceTimersByTime(20_000);
      });
      // A la vista y en el estado que lee el lector de pantalla.
      expect(
        screen.getAllByText(
          'Sigue pensando… Puedes ir a otra pantalla: la respuesta se guardará en la conversación.',
        ),
      ).toHaveLength(2);
      expect(screen.queryByText(/cerrar el panel/)).toBeNull();
      answer(askReply(general(7, '¿Cuánto IVA pago?', 'Unos 410,00 €.')));
      expect(await screen.findByText('Unos 410,00 €.')).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });
  it('mientras se lee el primer documento de una nueva, no deja añadir un dato', async () => {
    let read: (c: Conversation) => void = () => undefined;
    api.pickAssistantFile.mockResolvedValue('/Users/demo/nomina.pdf');
    api.assistantFileAdd.mockReturnValue(
      new Promise<Conversation>((r) => {
        read = r;
      }),
    );
    render(<Assistant />);
    await screen.findByLabelText('Tu pregunta');
    fireEvent.click(screen.getByRole('button', { name: /^Añadir un documento…/ }));
    await waitFor(() => expect(api.assistantFileAdd).toHaveBeenCalledTimes(1));
    const dato = screen.getByRole('button', { name: 'Añadir un dato' });
    expect(dato.getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(dato);
    expect(screen.queryByLabelText('Dato para la IA')).toBeNull();
    read({ ...general(8, 'Conversación del 31/10/2026', ''), messages: [] });
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: 'Añadir un dato' }).getAttribute('aria-disabled'),
      ).toBeNull(),
    );
    expect(api.assistantFactAdd).not.toHaveBeenCalled();
  });

  it('el documento que se elige mientras se guarda el primer dato va a su conversación', async () => {
    let saved: (c: Conversation) => void = () => undefined;
    api.assistantFactAdd.mockReturnValue(
      new Promise<Conversation>((r) => {
        saved = r;
      }),
    );
    let picked: (p: string | null) => void = () => undefined;
    api.pickAssistantFile.mockReturnValue(
      new Promise<string | null>((r) => {
        picked = r;
      }),
    );
    api.assistantFileAdd.mockResolvedValue({ ...general(7, 'Conversación del 31/10/2026', '') });
    render(<Assistant />);
    await screen.findByLabelText('Tu pregunta');
    fireEvent.click(screen.getByRole('button', { name: 'Añadir un dato' }));
    fireEvent.change(screen.getByLabelText('Dato para la IA'), {
      target: { value: 'Tenemos 3 empleados.' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Añadir' }));
    // Mientras se guarda, «Añadir un documento…» no hace nada.
    const doc = screen.getByRole('button', { name: /^Añadir un documento…/ });
    expect(doc.getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(doc);
    expect(api.pickAssistantFile).not.toHaveBeenCalled();
    saved({ ...general(7, 'Conversación del 31/10/2026', ''), messages: [] });
    await waitFor(() => expect(doc.getAttribute('aria-disabled')).toBeNull());
    // Ya tiene id: el documento va a ella, también si se guarda algo mientras se elige.
    fireEvent.click(doc);
    await waitFor(() => expect(api.pickAssistantFile).toHaveBeenCalled());
    picked('/Users/demo/nomina.pdf');
    await waitFor(() =>
      expect(api.assistantFileAdd).toHaveBeenCalledWith(
        { kind: 'general' },
        7,
        '/Users/demo/nomina.pdf',
      ),
    );
  });

  it('no deja borrar en la lista la conversación que espera su respuesta', async () => {
    api.assistantConversations.mockResolvedValue([CRIPTO, IVA]);
    api.assistantConversationOpen.mockResolvedValue({
      header: { ...GENERAL, title: IVA.title },
      conversation: { ...general(2, IVA.title, 'El 20 de enero.') },
      changed: false,
    });
    let answer: (o: TopicOpened) => void = () => undefined;
    api.assistantTopicAsk.mockReturnValue(
      new Promise<TopicOpened>((r) => {
        answer = r;
      }),
    );
    render(<Assistant />);
    const list = await screen.findByRole('region', { name: 'Conversaciones guardadas' });
    fireEvent.click(await within(list).findByRole('button', { name: IVA.title }));
    const box = await screen.findByLabelText('Tu pregunta');
    await screen.findByText('El 20 de enero.');
    fireEvent.change(box, { target: { value: '¿Y el 130?' } });
    fireEvent.click(screen.getByRole('button', { name: 'Preguntar' }));
    const borrar = within(list).getByRole('button', { name: `Borrar: ${IVA.title}` });
    expect((borrar as HTMLButtonElement).disabled).toBe(true);
    expect(
      (within(list).getByRole('button', { name: 'Borrar todas' }) as HTMLButtonElement).disabled,
    ).toBe(true);
    // La otra sí se puede borrar.
    expect(
      (within(list).getByRole('button', { name: `Borrar: ${CRIPTO.title}` }) as HTMLButtonElement)
        .disabled,
    ).toBe(false);
    answer(askReply(general(2, IVA.title, 'El 20 de enero.')));
    await waitFor(() =>
      expect(borrar.isConnected && (borrar as HTMLButtonElement).disabled).toBe(false),
    );
  });

  it('la lista recoge la respuesta que llega de otra pantalla y «Borrar todas» la cuenta', async () => {
    const otra = {
      ...IVA,
      id: 3,
      topic: { kind: 'ahorro' as const, id: 'capitalizacion', year: 2026 },
      title: 'Reserva de capitalización',
      questions: 1,
    };
    // Se pregunta en otra pantalla y se cierra el panel mientras piensa.
    let answer: (o: TopicOpened) => void = () => undefined;
    api.assistantTopicAsk.mockReturnValue(
      new Promise<TopicOpened>((r) => {
        answer = r;
      }),
    );
    function Otra() {
      const ia = useAssistantPanel();
      return (
        <main>
          <button
            type="button"
            onClick={(e) =>
              ia.open(
                { topic: otra.topic, title: otra.title, conversationId: null },
                e.currentTarget,
              )
            }
          >
            Abrir
          </button>
          {ia.element}
        </main>
      );
    }
    const pantalla = render(<Otra />);
    fireEvent.click(screen.getByRole('button', { name: 'Abrir' }));
    const dialog = await screen.findByRole('dialog');
    const box = await within(dialog).findByLabelText('Tu pregunta');
    fireEvent.change(box, { target: { value: '¿Me conviene?' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Preguntar' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cerrar' }));
    pantalla.unmount();
    // En el Asistente salen las dos que había.
    api.assistantConversations.mockResolvedValue([CRIPTO, IVA]);
    render(<Assistant />);
    const list = await screen.findByRole('region', { name: 'Conversaciones guardadas' });
    await within(list).findByRole('button', { name: IVA.title });
    // Llega la respuesta: la lista la recoge.
    api.assistantConversations.mockResolvedValue([otra, CRIPTO, IVA]);
    answer({
      header: { ...GENERAL, title: otra.title },
      conversation: {
        ...general(3, '¿Me conviene?', 'Sí.'),
        topic: otra.topic,
        title: otra.title,
      },
      changed: false,
    });
    expect(await within(list).findByRole('button', { name: otra.title })).toBeTruthy();
    fireEvent.click(within(list).getByRole('button', { name: 'Borrar todas' }));
    expect(screen.getByRole('alertdialog').textContent).toContain(
      'Se borran las 3 conversaciones guardadas de esta empresa, con lo que has añadido. No se puede deshacer; las copias de seguridad que ya has hecho las conservan.',
    );
  });

  it('sin guardadas dice dónde se empieza una, también en la revisión de la carpeta', async () => {
    render(<Assistant />);
    expect(
      (await screen.findByText(/Aún no hay ninguna/)).textContent?.includes('Importar → Revisión'),
    ).toBe(true);
  });

  it('borrar una conversación de la lista dice «lo que has añadido»', async () => {
    api.assistantConversations.mockResolvedValue([CRIPTO, IVA]);
    render(<Assistant />);
    const list = await screen.findByRole('region', { name: 'Conversaciones guardadas' });
    fireEvent.click(await within(list).findByRole('button', { name: `Borrar: ${IVA.title}` }));
    expect(within(list).getByRole('alertdialog').textContent).toContain(
      'Se borran las preguntas, las respuestas y lo que has añadido. No se puede deshacer; las copias de seguridad que ya has hecho las conservan.',
    );
    // El nombre entero, en el botón (sin el espacio que deja un texto oculto al lado).
    expect(
      within(list)
        .getByRole('button', { name: `Borrar: ${CRIPTO.title}` })
        .getAttribute('aria-label'),
    ).toBe(`Borrar: ${CRIPTO.title}`);
  });

  it('«Nueva conversación» lo anuncia', async () => {
    api.assistantConversations.mockResolvedValue([CRIPTO, IVA]);
    api.assistantConversationOpen.mockResolvedValue({
      header: { ...GENERAL, title: IVA.title },
      conversation: { ...general(2, IVA.title, 'El 20 de enero.') },
      changed: false,
    });
    render(<Assistant />);
    const list = await screen.findByRole('region', { name: 'Conversaciones guardadas' });
    fireEvent.click(await within(list).findByRole('button', { name: IVA.title }));
    await screen.findByText('El 20 de enero.');
    fireEvent.click(within(list).getByRole('button', { name: 'Nueva conversación' }));
    await waitFor(() =>
      expect(
        screen.getAllByRole('status').some((s) => s.textContent === 'Nueva conversación.'),
      ).toBe(true),
    );
  });

  it('al cambiar de proveedor hay que volver a aceptar el envío', async () => {
    api.assistantSave.mockResolvedValue(undefined);
    const onSaved = vi.fn();
    render(
      <AssistantSetup
        status={{
          settings: {
            provider: {
              kind: 'OpenAiCompatible',
              base_url: 'https://api.mistral.ai/v1',
              model: 'mistral-large-latest',
            },
            consent: true,
          },
          has_api_key: true,
          remote: true,
        }}
        onSaved={onSaved}
      />,
    );
    const consent = screen.getByRole('checkbox') as HTMLInputElement;
    expect(consent.checked).toBe(true);
    fireEvent.change(screen.getByLabelText(/^Proveedor/), { target: { value: 'openai' } });
    expect(consent.checked).toBe(false);
    fireEvent.change(screen.getByLabelText(/^Modelo/), { target: { value: 'gpt-x' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    expect((await screen.findByRole('alert')).textContent).toContain('acepta el envío');
    expect(api.assistantSave).not.toHaveBeenCalled();
    // Al volver al de antes, vale lo que se aceptó para él.
    fireEvent.change(screen.getByLabelText(/^Proveedor/), { target: { value: 'mistral' } });
    expect(consent.checked).toBe(true);
  });

  it('la lista recoge la conversación que crea un documento que se termina de leer tras salir', async () => {
    const topic = { kind: 'ahorro' as const, id: 'capitalizacion', year: 2026 };
    const otra: ConversationSummary = {
      id: 3,
      topic,
      title: 'Reserva de capitalización',
      updated_at: '2026-10-30T10:05:00Z',
      questions: 0,
      materials: 1,
    };
    api.assistantTopicOpen.mockResolvedValue({
      header: { ...GENERAL, group: 'Ahorrar impuestos · 2026', title: otra.title },
      conversation: null,
      changed: false,
    });
    // Un PDF escaneado: leerlo en el Mac tarda.
    api.pickAssistantFile.mockResolvedValue('/Users/demo/escaneado.pdf');
    let leido: (c: Conversation) => void = () => undefined;
    api.assistantFileAdd.mockReturnValue(
      new Promise<Conversation>((r) => {
        leido = r;
      }),
    );
    function Otra() {
      const ia = useAssistantPanel();
      return (
        <main>
          <button
            type="button"
            onClick={(e) =>
              ia.open({ topic, title: otra.title, conversationId: null }, e.currentTarget)
            }
          >
            Abrir
          </button>
          {ia.element}
        </main>
      );
    }
    const pantalla = render(<Otra />);
    fireEvent.click(screen.getByRole('button', { name: 'Abrir' }));
    const dialog = await screen.findByRole('dialog');
    await within(dialog).findByLabelText('Tu pregunta');
    fireEvent.click(within(dialog).getByRole('button', { name: /^Añadir un documento…/ }));
    await waitFor(() => expect(api.assistantFileAdd).toHaveBeenCalled());
    // Se cierra el panel y se va al Asistente mientras se lee.
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cerrar' }));
    pantalla.unmount();
    api.assistantConversations.mockResolvedValue([CRIPTO, IVA]);
    render(<Assistant />);
    const list = await screen.findByRole('region', { name: 'Conversaciones guardadas' });
    await within(list).findByRole('button', { name: IVA.title });
    // Se termina de leer: la conversación del tema ya existe y la lista la recoge.
    api.assistantConversations.mockResolvedValue([otra, CRIPTO, IVA]);
    await act(async () => {
      leido({
        id: 3,
        topic,
        title: otra.title,
        created_at: otra.updated_at,
        updated_at: otra.updated_at,
        messages: [],
        materials: [
          {
            id: 1,
            kind: 'documento',
            content: 'CERTIFICADO DE RETENCIONES 2026 (texto leído en el Mac)',
            filename: 'escaneado.pdf',
            read_by: 'mac',
            truncated: false,
            chars: 55,
            created_at: otra.updated_at,
          },
        ],
        full: false,
      });
    });
    expect(await within(list).findByRole('button', { name: otra.title })).toBeTruthy();
    fireEvent.click(within(list).getByRole('button', { name: 'Borrar todas' }));
    expect(screen.getByRole('alertdialog').textContent).toContain(
      'Se borran las 3 conversaciones guardadas de esta empresa',
    );
  });
});

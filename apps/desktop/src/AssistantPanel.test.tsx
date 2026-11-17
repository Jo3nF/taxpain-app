import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  AssistantTopic,
  Conversation,
  ConversationMaterial,
  StoredMessage,
  TopicHeader,
  TopicOpened,
} from './api';

const api = vi.hoisted(() => ({
  assistantStatus: vi.fn(),
  assistantSave: vi.fn(),
  assistantSetKey: vi.fn(),
  assistantTopicOpen: vi.fn(),
  assistantConversationOpen: vi.fn(),
  assistantTopicAsk: vi.fn(),
  assistantTopicPreview: vi.fn(),
  assistantFactAdd: vi.fn(),
  assistantFileAdd: vi.fn(),
  assistantMaterialDelete: vi.fn(),
  assistantConversationDelete: vi.fn(),
  assistantConversations: vi.fn(),
  pickAssistantFile: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { AskAiButton, useAskAi, useAssistantPanel } = await import('./AssistantPanel');

const TOPIC: AssistantTopic = { kind: 'ahorro', id: 'capitalizacion', year: 2026 };
const HEADER: TopicHeader = {
  group: 'Ahorrar impuestos · 2026',
  title: 'Reserva de capitalización',
  facts: [
    ['Encaje', 'Te conviene'],
    ['Ahorro estimado', '2.520,00 €'],
  ],
  intro: 'Con tus datos, te conviene. Pregunta lo que quieras o añade datos que TaxPain no tenga.',
  suggestions: ['Explícamelo con mis datos', '¿Qué tengo que hacer y antes de cuándo?'],
  notice: null,
};
const CONFIGURED = {
  settings: { provider: { kind: 'Anthropic', model: 'claude-opus-5-5' }, consent: true },
  has_api_key: true,
  remote: true,
};

const q = (seq: number, content: string): StoredMessage => ({
  seq,
  role: 'user',
  content,
  created_at: '2026-10-30T10:00:00Z',
  notes: null,
});
const a = (seq: number, content: string): StoredMessage => ({
  seq,
  role: 'assistant',
  content,
  created_at: '2026-10-30T10:01:00Z',
  notes: {
    model: 'claude-opus-5-5',
    replaced: 2,
    used: ['borrador_modelo'],
    sources: ['BOE-A-2014-12328, artículo 25'],
    unverified: [],
  },
});
const conv = (
  messages: StoredMessage[],
  materials: ConversationMaterial[] = [],
  extra: Partial<Conversation> = {},
): Conversation => ({
  id: 7,
  topic: TOPIC,
  title: 'Reserva de capitalización',
  created_at: '2026-10-30T10:00:00Z',
  updated_at: '2026-10-30T10:01:00Z',
  messages,
  materials,
  full: false,
  ...extra,
});
const opened = (c: Conversation | null, changed = false): TopicOpened => ({
  header: HEADER,
  conversation: c,
  changed,
});
const DATO: ConversationMaterial = {
  id: 1,
  kind: 'dato',
  content: 'En 2025 dejamos 30.000 € del beneficio en reservas voluntarias.',
  filename: null,
  read_by: null,
  truncated: false,
  chars: 62,
  created_at: '2026-10-30T10:00:00Z',
};

const onChange = vi.fn();

function Harness({ change = onChange }: { change?: typeof onChange }) {
  const ia = useAssistantPanel(change);
  return (
    <main>
      <button
        type="button"
        onClick={(e) =>
          ia.open(
            { topic: TOPIC, title: 'Reserva de capitalización', conversationId: null },
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

async function abrir() {
  fireEvent.click(screen.getByRole('button', { name: 'Abrir' }));
  const dialog = await screen.findByRole('dialog', {
    name: /^Conversación con la IA: Reserva de capitalización/,
  });
  await within(dialog).findByText(HEADER.intro);
  return dialog;
}

/** Una pantalla con su botón «Preguntar a la IA» (como Ahorrar impuestos). */
function Page() {
  const ia = useAskAi();
  return (
    <main>
      <AskAiButton
        what="Reserva de capitalización"
        questions={ia.questions(TOPIC)}
        onOpen={(b) =>
          ia.open({ topic: TOPIC, title: 'Reserva de capitalización', conversationId: null }, b)
        }
      />
      {ia.element}
    </main>
  );
}

beforeEach(() => {
  api.assistantStatus.mockResolvedValue(CONFIGURED);
  api.assistantTopicOpen.mockResolvedValue(opened(null));
  api.assistantConversations.mockResolvedValue([]);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  for (const f of Object.values(api)) f.mockReset();
});

describe('Conversación con la IA', () => {
  it('se abre con el tema, la presentación de TaxPain y preguntas que no se envían solas', async () => {
    render(<Harness />);
    const dialog = await abrir();
    expect(dialog.getAttribute('aria-describedby')).toBeTruthy();
    expect(within(dialog).getByText(/^Hablas con una IA/)).toBeTruthy();
    expect(within(dialog).getByText('Ahorrar impuestos · 2026')).toBeTruthy();
    expect(api.assistantTopicOpen).toHaveBeenCalledWith(TOPIC);
    const box = within(dialog).getByLabelText('Tu pregunta') as HTMLTextAreaElement;
    await waitFor(() => expect(document.activeElement).toBe(box));
    const group = within(dialog).getByRole('group', { name: 'Preguntas para empezar' });
    fireEvent.click(within(group).getByRole('button', { name: 'Explícamelo con mis datos' }));
    expect(box.value).toBe('Explícamelo con mis datos');
    expect(document.activeElement).toBe(box);
    expect(api.assistantTopicAsk).not.toHaveBeenCalled();
    // Lo que sabe TaxPain, a la vista si se abre.
    expect(within(dialog).getByText('Lo que sabe TaxPain de esto')).toBeTruthy();
    expect(within(dialog).getByText('2.520,00 €')).toBeTruthy();
  });

  it('envía sólo la pregunta, enseña la respuesta con sus notas y la siguiente sigue la conversación', async () => {
    const first = conv([q(1, '¿Me conviene?'), a(2, 'Sí, unos 2.520,00 €.')]);
    const second = conv([...first.messages, q(3, '¿Y si reparto?'), a(4, 'Entonces, la mitad.')]);
    api.assistantTopicAsk
      .mockResolvedValueOnce(opened(first))
      .mockResolvedValueOnce(opened(second));
    render(<Harness />);
    const dialog = await abrir();
    const box = within(dialog).getByLabelText('Tu pregunta') as HTMLTextAreaElement;
    fireEvent.change(box, { target: { value: '  ¿Me conviene?  ' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Preguntar' }));
    expect(api.assistantTopicAsk).toHaveBeenCalledWith(TOPIC, null, '¿Me conviene?');
    expect(box.value).toBe('');
    const log = within(dialog).getByRole('log', { name: 'Conversación' });
    expect(await within(log).findByText('Sí, unos 2.520,00 €.')).toBeTruthy();
    expect(
      within(log).getByText('Ha consultado (sólo lectura): borrador de un modelo.'),
    ).toBeTruthy();
    expect(
      within(log).getByText('Antes de enviar se sustituyeron 2 datos personales por marcadores.'),
    ).toBeTruthy();
    expect(within(log).getByRole('button', { name: 'Copiar la respuesta' })).toBeTruthy();
    // Las preguntas para empezar ya no salen.
    expect(within(dialog).queryByRole('group', { name: 'Preguntas para empezar' })).toBeNull();
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ id: 7, questions: 1 }),
      expect.objectContaining({ topic: TOPIC }),
    );
    fireEvent.change(box, { target: { value: '¿Y si reparto?' } });
    fireEvent.keyDown(box, { key: 'Enter', metaKey: true });
    expect(api.assistantTopicAsk).toHaveBeenLastCalledWith(TOPIC, 7, '¿Y si reparto?');
    expect(await within(log).findByText('Entonces, la mitad.')).toBeTruthy();
    expect(document.activeElement).toBe(box);
  });

  it('si falla, la pregunta vuelve al cuadro y no se añade nada', async () => {
    api.assistantTopicAsk.mockRejectedValue({ message: 'falta la clave de API del proveedor' });
    render(<Harness />);
    const dialog = await abrir();
    const box = within(dialog).getByLabelText('Tu pregunta') as HTMLTextAreaElement;
    fireEvent.change(box, { target: { value: 'Hola' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Preguntar' }));
    expect((await within(dialog).findByRole('alert')).textContent).toContain('clave de API');
    expect(box.value).toBe('Hola');
    const log = within(dialog).getByRole('log', { name: 'Conversación' });
    expect(within(log).queryByText('Hola')).toBeNull();
    // Sin texto, «Preguntar» no hace nada.
    fireEvent.change(box, { target: { value: '   ' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Preguntar' }));
    expect(api.assistantTopicAsk).toHaveBeenCalledTimes(1);
  });

  it('sin consentimiento pide configurarlo y no deja escribir la pregunta', async () => {
    api.assistantStatus.mockResolvedValue({
      ...CONFIGURED,
      settings: { ...CONFIGURED.settings, consent: false },
    });
    render(<Harness />);
    const dialog = await abrir();
    expect(await within(dialog).findByRole('button', { name: 'Guardar' })).toBeTruthy();
    expect(within(dialog).getByText(/Sólo hace falta una vez por empresa/)).toBeTruthy();
    expect(within(dialog).getByText(/los datos del tema del que pregunte/)).toBeTruthy();
    expect(within(dialog).queryByLabelText('Tu pregunta')).toBeNull();
    await waitFor(() => expect(document.activeElement?.tagName).toBe('SELECT'));
  });

  it('añade un dato, lo enseña y lo quita; Esc cierra sólo el formulario', async () => {
    api.assistantFactAdd.mockResolvedValue(conv([], [DATO]));
    api.assistantMaterialDelete.mockResolvedValue(conv([], []));
    render(<Harness />);
    const dialog = await abrir();
    const add = within(dialog).getByRole('button', { name: 'Añadir un dato' });
    fireEvent.click(add);
    const fact = within(dialog).getByLabelText('Dato para la IA') as HTMLTextAreaElement;
    await waitFor(() => expect(document.activeElement).toBe(fact));
    // Esc cierra el formulario, no el panel, y el foco vuelve al botón.
    fireEvent.keyDown(fact, { key: 'Escape' });
    expect(within(dialog).queryByLabelText('Dato para la IA')).toBeNull();
    expect(screen.getByRole('dialog')).toBeTruthy();
    await waitFor(() => expect(document.activeElement).toBe(add));
    fireEvent.click(add);
    fireEvent.change(within(dialog).getByLabelText('Dato para la IA'), {
      target: { value: DATO.content },
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Añadir' }));
    await waitFor(() =>
      expect(api.assistantFactAdd).toHaveBeenCalledWith(TOPIC, null, DATO.content),
    );
    expect(
      await within(dialog).findByText('Dato añadido. Va con la próxima pregunta.'),
    ).toBeTruthy();
    expect(within(dialog).getByText('Lo que has añadido tú (1)')).toBeTruthy();
    fireEvent.click(
      within(dialog).getByRole('button', { name: /^Quitar: En 2025 dejamos 30\.000 €/ }),
    );
    await waitFor(() => expect(api.assistantMaterialDelete).toHaveBeenCalledWith(7, 1));
    expect(await within(dialog).findByText('Dato quitado.')).toBeTruthy();
    expect(within(dialog).getByText('Lo que has añadido tú (0)')).toBeTruthy();
  });

  it('añade un documento leído en el Mac y enseña su texto', async () => {
    api.pickAssistantFile.mockResolvedValue('/Users/demo/Documentos/certificado.pdf');
    api.assistantFileAdd.mockResolvedValue(
      conv(
        [],
        [
          {
            id: 2,
            kind: 'documento',
            content: 'CERTIFICADO DE APORTACIONES 2026 · Aportado: 1.500,00 €',
            filename: 'certificado.pdf',
            read_by: 'mac',
            truncated: true,
            chars: 15_000,
            created_at: '2026-10-30T10:00:00Z',
          },
        ],
      ),
    );
    render(<Harness />);
    const dialog = await abrir();
    expect(
      within(dialog).getByText(/Se lee en tu Mac y con cada pregunta va su texto, anonimizado/),
    ).toBeTruthy();
    fireEvent.click(within(dialog).getByRole('button', { name: /^Añadir un documento…/ }));
    expect(await within(dialog).findByText('Documento añadido: certificado.pdf.')).toBeTruthy();
    expect(api.assistantFileAdd).toHaveBeenCalledWith(
      TOPIC,
      null,
      '/Users/demo/Documentos/certificado.pdf',
    );
    expect(within(dialog).getByText(/leído en tu Mac/)).toBeTruthy();
    expect(within(dialog).getByText(/recortado: no va entero/)).toBeTruthy();
    expect(within(dialog).getByText('Ver el texto')).toBeTruthy();
    expect(within(dialog).getByText(/CERTIFICADO DE APORTACIONES 2026/)).toBeTruthy();
    expect(within(dialog).getByRole('button', { name: 'Quitar: certificado.pdf' })).toBeTruthy();
  });

  it('«Ver qué se envía» enseña lo anonimizado, lo que no va y las consultas', async () => {
    api.assistantTopicPreview.mockResolvedValue({
      system: 'Eres el asistente de TaxPain… ### Tema de esta conversación … [NIF_1]',
      messages: [{ role: 'user', content: '¿Y si lo dejo todo?' }],
      replaced: 3,
      omitted: 4,
    });
    render(<Harness />);
    const dialog = await abrir();
    fireEvent.change(within(dialog).getByLabelText('Tu pregunta'), {
      target: { value: '¿Y si lo dejo todo?' },
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Ver qué se envía' }));
    expect(api.assistantTopicPreview).toHaveBeenCalledWith(TOPIC, null, '¿Y si lo dejo todo?');
    expect(
      await within(dialog).findByText('Esto es exactamente lo que se envía (3 datos sustituidos)'),
    ).toBeTruthy();
    expect(within(dialog).getByText(/No van las 4 intervenciones más antiguas/)).toBeTruthy();
    expect(within(dialog).getByText(/la IA puede consultar, sólo leyendo/)).toBeTruthy();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Ocultar lo que se envía' }));
    expect(within(dialog).queryByText(/exactamente lo que se envía/)).toBeNull();
  });

  it('Esc cierra el panel y el foco vuelve al botón que lo abrió', async () => {
    render(<Harness />);
    const dialog = await abrir();
    fireEvent.keyDown(within(dialog).getByLabelText('Tu pregunta'), { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Abrir' }));
    // También con «Cerrar».
    const again = await abrir();
    fireEvent.click(within(again).getByRole('button', { name: 'Cerrar' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('al volver a abrirla enseña lo guardado y avisa si los datos han cambiado', async () => {
    api.assistantTopicOpen.mockResolvedValue(
      opened(conv([q(1, '¿Me conviene?'), a(2, 'Sí, unos 2.520,00 €.')], [DATO]), true),
    );
    render(<Harness />);
    const dialog = await abrir();
    expect(
      within(dialog).getByText(/Los datos de este tema han cambiado desde tu última pregunta/),
    ).toBeTruthy();
    const log = within(dialog).getByRole('log', { name: 'Conversación' });
    expect(within(log).getByText('¿Me conviene?')).toBeTruthy();
    expect(within(log).getByText('Sí, unos 2.520,00 €.')).toBeTruthy();
    expect(within(dialog).getByText('Lo que has añadido tú (1)')).toBeTruthy();
    expect(within(dialog).queryByRole('group', { name: 'Preguntas para empezar' })).toBeNull();
  });

  it('borrar pide confirmación, vuelve a la presentación y lo dice a la página', async () => {
    api.assistantTopicOpen.mockResolvedValue(
      opened(conv([q(1, '¿Me conviene?'), a(2, 'Sí, unos 2.520,00 €.')])),
    );
    api.assistantConversationDelete.mockResolvedValue(undefined);
    render(<Harness />);
    const dialog = await abrir();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Borrar la conversación' }));
    const confirm = within(dialog).getByRole('alertdialog');
    expect(confirm.textContent).toContain('No se puede deshacer');
    // Esc cierra la confirmación, no el panel.
    fireEvent.keyDown(confirm, { key: 'Escape' });
    expect(within(dialog).queryByRole('alertdialog')).toBeNull();
    expect(screen.getByRole('dialog')).toBeTruthy();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Borrar la conversación' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Borrar' }));
    await waitFor(() => expect(api.assistantConversationDelete).toHaveBeenCalledWith(7));
    expect(await within(dialog).findByText('Conversación borrada.')).toBeTruthy();
    expect(within(dialog).queryByText('Sí, unos 2.520,00 €.')).toBeNull();
    expect(within(dialog).getByRole('group', { name: 'Preguntas para empezar' })).toBeTruthy();
    expect(onChange).toHaveBeenCalledWith(null, expect.objectContaining({ topic: TOPIC }));
  });

  it('una conversación llena no deja preguntar', async () => {
    api.assistantTopicOpen.mockResolvedValue(
      opened(conv([q(1, '¿Me conviene?'), a(2, 'Sí.')], [], { full: true })),
    );
    render(<Harness />);
    const dialog = await abrir();
    expect(within(dialog).getByText(/ha llegado a 100 preguntas/)).toBeTruthy();
    expect(within(dialog).queryByLabelText('Tu pregunta')).toBeNull();
    expect(within(dialog).getByRole('button', { name: 'Borrar la conversación' })).toBeTruthy();
  });

  it('se puede cerrar mientras piensa: al volver, espera la misma respuesta', async () => {
    let answer: (o: TopicOpened) => void = () => undefined;
    api.assistantTopicAsk.mockReturnValue(
      new Promise<TopicOpened>((r) => {
        answer = r;
      }),
    );
    render(<Harness />);
    let dialog = await abrir();
    fireEvent.change(within(dialog).getByLabelText('Tu pregunta'), {
      target: { value: '¿Me conviene?' },
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Preguntar' }));
    expect(within(dialog).getByRole('status').textContent).toBe('Pensando…');
    // Mientras piensa no se puede borrar ni preguntar otra vez.
    expect(
      (within(dialog).getByRole('button', { name: 'Borrar la conversación' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cerrar' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    dialog = await abrir();
    expect(within(dialog).getByRole('status').textContent).toBe('Pensando…');
    expect(within(dialog).getByText('¿Me conviene?')).toBeTruthy();
    answer(opened(conv([q(1, '¿Me conviene?'), a(2, 'Sí, unos 2.520,00 €.')])));
    expect(await within(dialog).findByText('Sí, unos 2.520,00 €.')).toBeTruthy();
    expect(api.assistantTopicAsk).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ questions: 1 }),
      expect.objectContaining({ topic: TOPIC }),
    );
  });

  it('si falla con el panel cerrado, al volver a abrirlo la pregunta está en el cuadro con su error', async () => {
    let fail: (e: unknown) => void = () => undefined;
    api.assistantTopicAsk.mockReturnValue(
      new Promise<TopicOpened>((_, r) => {
        fail = r;
      }),
    );
    render(<Harness />);
    let dialog = await abrir();
    fireEvent.change(within(dialog).getByLabelText('Tu pregunta'), {
      target: { value: '¿Me conviene?' },
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Preguntar' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cerrar' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    fail({ message: 'el proveedor no ha respondido a tiempo' });
    await new Promise((r) => setTimeout(r, 0));
    dialog = await abrir();
    const box = within(dialog).getByLabelText('Tu pregunta') as HTMLTextAreaElement;
    expect(box.value).toBe('¿Me conviene?');
    expect(within(dialog).getByRole('alert').textContent).toContain('no ha respondido a tiempo');
    // Ya se ha devuelto: al cerrar y abrir otra vez, no vuelve a salir.
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cerrar' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    dialog = await abrir();
    expect((within(dialog).getByLabelText('Tu pregunta') as HTMLTextAreaElement).value).toBe('');
    expect(within(dialog).queryByRole('alert')).toBeNull();
  });

  it('si falla mientras escribes la siguiente, no se pierde ninguna de las dos', async () => {
    let fail: (e: unknown) => void = () => undefined;
    api.assistantTopicAsk.mockReturnValue(
      new Promise<TopicOpened>((_, r) => {
        fail = r;
      }),
    );
    render(<Harness />);
    const dialog = await abrir();
    const box = within(dialog).getByLabelText('Tu pregunta') as HTMLTextAreaElement;
    fireEvent.change(box, { target: { value: '¿Me conviene?' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Preguntar' }));
    fireEvent.change(box, { target: { value: '¿Y si reparto?' } });
    fail({ message: 'error del proveedor' });
    await waitFor(() => expect(box.value).toBe('¿Me conviene?\n\n¿Y si reparto?'));
    expect(within(dialog).getByRole('alert').textContent).toContain('error del proveedor');
  });

  it('si se vuelve a la pantalla y la respuesta llega con el panel abierto, la pantalla se entera', async () => {
    let answer: (o: TopicOpened) => void = () => undefined;
    api.assistantTopicAsk.mockReturnValue(
      new Promise<TopicOpened>((r) => {
        answer = r;
      }),
    );
    const first = render(<Harness />);
    let dialog = await abrir();
    fireEvent.change(within(dialog).getByLabelText('Tu pregunta'), {
      target: { value: '¿Me conviene?' },
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Preguntar' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cerrar' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    // Otra pantalla y vuelta: la página es otra.
    first.unmount();
    const again = vi.fn();
    render(<Harness change={again} />);
    dialog = await abrir();
    answer(opened(conv([q(1, '¿Me conviene?'), a(2, 'Sí, unos 2.520,00 €.')])));
    expect(await within(dialog).findByText('Sí, unos 2.520,00 €.')).toBeTruthy();
    expect(again).toHaveBeenCalledWith(
      expect.objectContaining({ id: 7, questions: 1 }),
      expect.objectContaining({ topic: TOPIC }),
    );
  });

  it('al llegar la respuesta, la pregunta y el día no se vuelven a pintar (ni a leer)', async () => {
    let answer: (o: TopicOpened) => void = () => undefined;
    api.assistantTopicAsk.mockReturnValue(
      new Promise<TopicOpened>((r) => {
        answer = r;
      }),
    );
    render(<Harness />);
    const dialog = await abrir();
    fireEvent.change(within(dialog).getByLabelText('Tu pregunta'), {
      target: { value: '¿Me conviene?' },
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Preguntar' }));
    const log = within(dialog).getByRole('log', { name: 'Conversación' });
    const question = within(log).getByText('¿Me conviene?').closest('li');
    const today = within(log).getByText('Hoy');
    const now = new Date().toISOString();
    answer(
      opened(
        conv([
          { ...q(1, '¿Me conviene?'), created_at: now },
          { ...a(2, 'Sí, unos 2.520,00 €.'), created_at: now },
        ]),
      ),
    );
    expect(await within(log).findByText('Sí, unos 2.520,00 €.')).toBeTruthy();
    expect(question && log.contains(question)).toBe(true);
    expect(log.contains(today)).toBe(true);
    expect(within(log).queryByText('Pensando…')).toBeNull();
  });

  it('al guardar los ajustes, el foco pasa a «Tu pregunta»', async () => {
    api.assistantStatus
      .mockResolvedValueOnce({
        ...CONFIGURED,
        settings: { ...CONFIGURED.settings, consent: false },
      })
      .mockResolvedValue(CONFIGURED);
    api.assistantSave.mockResolvedValue(undefined);
    render(<Harness />);
    const dialog = await abrir();
    fireEvent.click(within(dialog).getByRole('checkbox'));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Guardar' }));
    const box = await within(dialog).findByLabelText('Tu pregunta');
    await waitFor(() => expect(document.activeElement).toBe(box));
  });

  it('al borrar una conversación llena, el foco pasa a «Tu pregunta»', async () => {
    api.assistantTopicOpen.mockResolvedValue(
      opened(conv([q(1, '¿Me conviene?'), a(2, 'Sí.')], [], { full: true })),
    );
    api.assistantConversationDelete.mockResolvedValue(undefined);
    render(<Harness />);
    const dialog = await abrir();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Borrar la conversación' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Borrar' }));
    const box = await within(dialog).findByLabelText('Tu pregunta');
    await waitFor(() => expect(document.activeElement).toBe(box));
  });

  it('dos datos seguidos se anuncian las dos veces', async () => {
    const otro: ConversationMaterial = { ...DATO, id: 2, content: 'Tenemos 3 trabajadores.' };
    api.assistantFactAdd
      .mockResolvedValueOnce(conv([], [DATO]))
      .mockResolvedValueOnce(conv([], [DATO, otro]));
    render(<Harness />);
    const dialog = await abrir();
    const status = within(dialog).getByRole('status');
    const said: string[] = [];
    const watch = new MutationObserver(() => said.push(status.textContent ?? ''));
    watch.observe(status, { childList: true, characterData: true, subtree: true });
    const MSG = 'Dato añadido. Va con la próxima pregunta.';
    for (const [i, t] of [DATO.content, otro.content].entries()) {
      fireEvent.click(within(dialog).getByRole('button', { name: 'Añadir un dato' }));
      fireEvent.change(within(dialog).getByLabelText('Dato para la IA'), { target: { value: t } });
      fireEvent.click(within(dialog).getByRole('button', { name: 'Añadir' }));
      await waitFor(() => expect(said.filter((x) => x === MSG)).toHaveLength(i + 1));
    }
    watch.disconnect();
    expect(within(dialog).getByText('Lo que has añadido tú (2)')).toBeTruthy();
  });

  it('con un solo dato sustituido o una sola intervención fuera, lo dice en singular', async () => {
    const answer: StoredMessage = {
      ...a(2, 'Sí.'),
      notes: { model: 'claude-opus-5-5', replaced: 1, used: [], sources: [], unverified: [] },
    };
    api.assistantTopicOpen.mockResolvedValue(opened(conv([q(1, '¿Me conviene?'), answer])));
    api.assistantTopicPreview.mockResolvedValue({
      system: 'Eres el asistente de TaxPain…',
      messages: [{ role: 'user', content: '(aquí irá tu pregunta)' }],
      replaced: 1,
      omitted: 1,
    });
    render(<Harness />);
    const dialog = await abrir();
    expect(
      within(dialog).getByText('Antes de enviar se sustituyó 1 dato personal por un marcador.'),
    ).toBeTruthy();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Ver qué se envía' }));
    expect(
      await within(dialog).findByText('Esto es exactamente lo que se envía (1 dato sustituido)'),
    ).toBeTruthy();
    expect(
      within(dialog).getByText(/^No va la intervención más antigua de esta conversación/),
    ).toBeTruthy();
  });
  it('«Ver qué se envía» se cierra si cambia la pregunta o lo añadido', async () => {
    api.assistantTopicPreview.mockResolvedValue({
      system: 'Eres el asistente de TaxPain… con 0 datos añadidos',
      messages: [{ role: 'user', content: '¿Me conviene?' }],
      replaced: 0,
      omitted: 0,
    });
    api.assistantFactAdd.mockResolvedValue(conv([], [DATO]));
    api.pickAssistantFile.mockResolvedValue('/Users/demo/nomina.pdf');
    api.assistantFileAdd.mockResolvedValue(
      conv(
        [],
        [
          DATO,
          {
            ...DATO,
            id: 2,
            kind: 'documento',
            content: 'NÓMINA DE OCTUBRE · VILLALOBOS CUESTA LUCIA',
            filename: 'nomina.pdf',
            read_by: 'mac',
          },
        ],
      ),
    );
    api.assistantMaterialDelete.mockResolvedValue(conv([], [DATO]));
    render(<Harness />);
    const dialog = await abrir();
    const box = within(dialog).getByLabelText('Tu pregunta');
    fireEvent.change(box, { target: { value: '¿Me conviene?' } });
    const ver = async () => {
      fireEvent.click(within(dialog).getByRole('button', { name: 'Ver qué se envía' }));
      expect(await within(dialog).findByText(/exactamente lo que se envía/)).toBeTruthy();
    };
    const cerrado = () => {
      expect(within(dialog).queryByText(/exactamente lo que se envía/)).toBeNull();
      expect(within(dialog).getByRole('button', { name: 'Ver qué se envía' })).toBeTruthy();
    };
    // Un dato.
    await ver();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Añadir un dato' }));
    fireEvent.change(within(dialog).getByLabelText('Dato para la IA'), {
      target: { value: DATO.content },
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Añadir' }));
    expect(
      await within(dialog).findByText('Dato añadido. Va con la próxima pregunta.'),
    ).toBeTruthy();
    cerrado();
    // Un documento.
    await ver();
    fireEvent.click(within(dialog).getByRole('button', { name: /^Añadir un documento…/ }));
    expect(await within(dialog).findByText('Documento añadido: nomina.pdf.')).toBeTruthy();
    cerrado();
    // Quitar algo.
    await ver();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Quitar: nomina.pdf' }));
    expect(await within(dialog).findByText('Documento quitado: nomina.pdf.')).toBeTruthy();
    cerrado();
    // Otra pregunta.
    await ver();
    fireEvent.change(box, { target: { value: '¿Y con la nómina de Lucía?' } });
    cerrado();
    expect(api.assistantTopicPreview).toHaveBeenCalledTimes(4);
  });

  it('Esc cierra antes la capa abierta, aunque el foco esté fuera de ella', async () => {
    api.assistantTopicOpen.mockResolvedValue(opened(conv([q(1, '¿Me conviene?'), a(2, 'Sí.')])));
    render(<Harness />);
    const dialog = await abrir();
    const box = within(dialog).getByLabelText('Tu pregunta');
    const add = within(dialog).getByRole('button', { name: 'Añadir un dato' });
    fireEvent.click(add);
    fireEvent.change(within(dialog).getByLabelText('Dato para la IA'), {
      target: { value: 'Dato a medias que no quiero perder' },
    });
    box.focus();
    fireEvent.keyDown(box, { key: 'Escape' });
    expect(within(dialog).queryByLabelText('Dato para la IA')).toBeNull();
    expect(screen.getByRole('dialog')).toBeTruthy();
    await waitFor(() => expect(document.activeElement).toBe(add));
    // La confirmación de borrar, igual.
    fireEvent.click(within(dialog).getByRole('button', { name: 'Borrar la conversación' }));
    expect(within(dialog).getByRole('alertdialog')).toBeTruthy();
    box.focus();
    fireEvent.keyDown(box, { key: 'Escape' });
    expect(within(dialog).queryByRole('alertdialog')).toBeNull();
    expect(screen.getByRole('dialog')).toBeTruthy();
    await waitFor(() =>
      expect(document.activeElement).toBe(
        within(dialog).getByRole('button', { name: 'Borrar la conversación' }),
      ),
    );
    // Sin nada abierto, Esc cierra el panel.
    fireEvent.keyDown(box, { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('sin el diálogo modal del sistema (macOS anteriores a 12.3) el panel sigue siendo modal', async () => {
    render(<Harness />);
    const dialog = await abrir();
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    // Lo de detrás no se lee y el foco no se queda allí.
    const opener = screen.getByText('Abrir');
    expect(opener.closest('[aria-hidden="true"]')).toBeTruthy();
    opener.focus();
    await waitFor(() => expect(dialog.contains(document.activeElement)).toBe(true));
    // Esc cierra el panel aunque el foco esté fuera de él.
    (document.activeElement as HTMLElement | null)?.blur();
    fireEvent.keyDown(document.body, { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(opener.closest('[aria-hidden="true"]')).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it('los nombres de los botones son su texto entero, sin espacios de más', async () => {
    api.assistantTopicOpen.mockResolvedValue(
      opened(
        conv(
          [],
          [
            DATO,
            {
              ...DATO,
              id: 2,
              kind: 'documento',
              content: 'CERTIFICADO DE APORTACIONES 2026',
              filename: 'certificado.pdf',
              read_by: 'mac',
            },
          ],
        ),
      ),
    );
    render(<Harness />);
    const dialog = await abrir();
    const cerrar = within(dialog).getByRole('button', { name: 'Cerrar' });
    // El «esc» que se ve no va dentro del botón (su nombre tiene que llevar lo que se ve).
    expect(cerrar.textContent).toBe('');
    expect(within(dialog).getByText('esc')).toBeTruthy();
    const quitar = within(dialog).getAllByRole('button', { name: /^Quitar/ });
    expect(quitar.map((b) => b.getAttribute('aria-label'))).toEqual([
      'Quitar: En 2025 dejamos 30.000 € del…',
      'Quitar: certificado.pdf',
    ]);
    expect(within(dialog).getByText('Ver el texto').getAttribute('aria-label')).toBe(
      'Ver el texto: certificado.pdf',
    );
  });

  it('el atajo ⌘ + Intro se ve debajo de cada cuadro', async () => {
    render(<Harness />);
    const dialog = await abrir();
    const box = within(dialog).getByLabelText('Tu pregunta');
    expect(
      document.getElementById(box.getAttribute('aria-describedby') ?? '')?.textContent,
    ).toMatch(/⌘ \+ Intro/);
    fireEvent.click(within(dialog).getByRole('button', { name: 'Añadir un dato' }));
    const fact = within(dialog).getByLabelText('Dato para la IA');
    expect(
      document.getElementById(fact.getAttribute('aria-describedby') ?? '')?.textContent,
    ).toMatch(/⌘ \+ Intro/);
  });

  it('las preguntas para empezar no van en el registro (no se vuelven a leer)', async () => {
    api.assistantTopicAsk.mockRejectedValue({ message: 'error del proveedor' });
    render(<Harness />);
    const dialog = await abrir();
    const log = within(dialog).getByRole('log', { name: 'Conversación' });
    const group = within(dialog).getByRole('group', { name: 'Preguntas para empezar' });
    expect(log.contains(group)).toBe(false);
    fireEvent.change(within(dialog).getByLabelText('Tu pregunta'), {
      target: { value: '¿Me conviene?' },
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Preguntar' }));
    expect(await within(dialog).findByRole('alert')).toBeTruthy();
    // Vuelven, pero fuera del registro.
    const again = within(dialog).getByRole('group', { name: 'Preguntas para empezar' });
    expect(log.contains(again)).toBe(false);
  });

  it('borrar dice «lo que has añadido» y «las copias que ya has hecho»', async () => {
    api.assistantTopicOpen.mockResolvedValue(opened(conv([q(1, '¿Me conviene?'), a(2, 'Sí.')])));
    render(<Harness />);
    const dialog = await abrir();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Borrar la conversación' }));
    expect(within(dialog).getByRole('alertdialog').textContent).toContain(
      'Se borran las preguntas, las respuestas y lo que has añadido. No se puede deshacer; las copias de seguridad que ya has hecho las conservan.',
    );
  });

  it('la respuesta que llega tras salir y volver cambia el botón a «Seguir con la IA»', async () => {
    let answer: (o: TopicOpened) => void = () => undefined;
    api.assistantTopicAsk.mockReturnValue(
      new Promise<TopicOpened>((r) => {
        answer = r;
      }),
    );
    const first = render(<Page />);
    fireEvent.click(
      await screen.findByRole('button', { name: 'Preguntar a la IA: Reserva de capitalización' }),
    );
    const dialog = await screen.findByRole('dialog');
    await within(dialog).findByText(HEADER.intro);
    fireEvent.change(within(dialog).getByLabelText('Tu pregunta'), {
      target: { value: '¿Me conviene?' },
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Preguntar' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cerrar' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    // A otra pantalla y vuelta, antes de la respuesta.
    first.unmount();
    render(<Page />);
    expect(
      await screen.findByRole('button', { name: 'Preguntar a la IA: Reserva de capitalización' }),
    ).toBeTruthy();
    const c = conv([q(1, '¿Me conviene?'), a(2, 'Sí, unos 2.520,00 €.')]);
    api.assistantConversations.mockResolvedValue([
      {
        id: c.id,
        topic: c.topic,
        title: c.title,
        updated_at: c.updated_at,
        questions: 1,
        materials: 0,
      },
    ]);
    answer(opened(c));
    expect(
      await screen.findByRole('button', {
        name: 'Seguir con la IA: Reserva de capitalización, 1 pregunta',
      }),
    ).toBeTruthy();
  });

  it('«Ver qué se envía» se cierra al llegar la respuesta, si falla y al borrar la conversación', async () => {
    let answer: (o: TopicOpened) => void = () => undefined;
    let fail: (e: unknown) => void = () => undefined;
    api.assistantTopicAsk
      .mockReturnValueOnce(
        new Promise<TopicOpened>((r) => {
          answer = r;
        }),
      )
      .mockReturnValueOnce(
        new Promise<TopicOpened>((_, r) => {
          fail = r;
        }),
      );
    api.assistantTopicPreview.mockResolvedValue({
      system: 'INSTRUCCIONES-DE-ANTES',
      messages: [{ role: 'user', content: '(aquí irá tu pregunta)' }],
      replaced: 0,
      omitted: 0,
    });
    api.assistantConversationDelete.mockResolvedValue(undefined);
    render(<Harness />);
    const dialog = await abrir();
    const box = within(dialog).getByLabelText('Tu pregunta') as HTMLTextAreaElement;
    const ver = async () => {
      fireEvent.click(within(dialog).getByRole('button', { name: 'Ver qué se envía' }));
      expect(await within(dialog).findByText(/exactamente lo que se envía/)).toBeTruthy();
    };
    const cerrado = () => {
      expect(within(dialog).queryByText(/exactamente lo que se envía/)).toBeNull();
      expect(within(dialog).getByRole('button', { name: 'Ver qué se envía' })).toBeTruthy();
    };
    // Mientras piensa se abre «Ver qué se envía»; llega la respuesta: ya no es lo que se envía.
    fireEvent.change(box, { target: { value: '¿Me conviene?' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Preguntar' }));
    await ver();
    answer(opened(conv([q(1, '¿Me conviene?'), a(2, 'RESPUESTA-BORRADA')], [DATO])));
    expect(await within(dialog).findByText('RESPUESTA-BORRADA')).toBeTruthy();
    cerrado();
    // La siguiente falla y vuelve al cuadro: también se cierra.
    fireEvent.change(box, { target: { value: '¿Y si reparto?' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Preguntar' }));
    await ver();
    fail({ message: 'error del proveedor' });
    await waitFor(() => expect(box.value).toBe('¿Y si reparto?'));
    cerrado();
    // Con la conversación a la vista, se borra: lo que enseñaba ya no se envía.
    fireEvent.change(box, { target: { value: '' } });
    api.assistantTopicPreview.mockResolvedValue({
      system: 'INSTRUCCIONES · Datos que he añadido a esta conversación',
      messages: [
        { role: 'user', content: '¿Me conviene?' },
        { role: 'assistant', content: 'RESPUESTA-BORRADA' },
        { role: 'user', content: '(aquí irá tu pregunta)' },
      ],
      replaced: 0,
      omitted: 0,
    });
    await ver();
    expect(within(dialog).getByText('IA: RESPUESTA-BORRADA')).toBeTruthy();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Borrar la conversación' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Borrar' }));
    expect(await within(dialog).findByText('Conversación borrada.')).toBeTruthy();
    expect(within(dialog).queryByText(/RESPUESTA-BORRADA/)).toBeNull();
    cerrado();
  });
});

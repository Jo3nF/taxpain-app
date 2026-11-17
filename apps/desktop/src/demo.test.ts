import { afterEach, describe, expect, it } from 'vitest';
import {
  type AssistantTopic,
  assistantConversationDelete,
  assistantConversations,
  assistantFactAdd,
  assistantFileAdd,
  assistantTopicAsk,
  assistantTopicOpen,
  assistantTopicPreview,
  companyReview,
  draftTopic,
  findingTopic,
  folderReview,
  modelDraft,
  setDemoInvoke,
} from './api';
import { demoExtractCsv, installDemo } from './demo';

describe('modo demo: descarga por CSV', () => {
  it('saca los códigos con la misma regla que la app (nunca sólo cifras)', () => {
    expect(
      demoExtractCsv('ABCD1234EFGH5678\r\nabcd1234efgh5678 1234567812345678\nQWERTYUIOPASDFGH\n'),
    ).toEqual(['ABCD1234EFGH5678', 'QWERTYUIOPASDFGH']);
    // Una palabra de 16 letras dentro de una tabla no es un código.
    expect(demoExtractCsv('ADMINISTRACIONES\tZ9Y8X7W6V5U4T3S2')).toEqual(['Z9Y8X7W6V5U4T3S2']);
    expect(demoExtractCsv('qwertyuiopasdfgh')).toEqual([]);
  });
});

describe('modo demo: conversaciones con la IA', () => {
  afterEach(() => setDemoInvoke(null));

  it('trae una guardada, responde según el tema y lo añadido, y se borra', async () => {
    installDemo();
    const guardadas = await assistantConversations();
    expect(guardadas.map((c) => c.title)).toContain(
      'Criptomonedas: compensa ganancias con pérdidas antes del 31 de diciembre',
    );
    const pensiones: AssistantTopic = { kind: 'ahorro', id: 'plan_pensiones_autonomo', year: 2026 };
    const abierta = await assistantTopicOpen(pensiones);
    expect(abierta.conversation).toBeNull();
    expect(abierta.header.title).toBe('Planes de pensiones: rebaja la Renta');
    expect(abierta.header.suggestions[0]).toBe('Explícamelo con mis datos');
    const r1 = await assistantTopicAsk(pensiones, null, '¿Cuánto me ahorro?');
    const c = r1.conversation;
    expect(c?.messages[1]?.content).toMatch(/^Si aportas 5\.750,00 €/);
    expect(c?.messages[1]?.notes?.sources).toEqual(['BOE-A-2006-20764, artículo 52']);
    // Con el dato de lo ya aportado, la respuesta lo usa.
    await assistantFactAdd(
      pensiones,
      c?.id ?? null,
      'En 2026 ya aporté 1.500 € a un plan individual.',
    );
    const r2 = await assistantTopicAsk(pensiones, c?.id ?? null, '¿Y ahora?');
    expect(r2.conversation?.messages[3]?.content).toMatch(/^Con lo que has añadido/);
    expect(r2.conversation?.materials).toHaveLength(1);
    // Un documento de ejemplo leído «en el Mac».
    const d = await assistantFileAdd(
      pensiones,
      c?.id ?? null,
      'demo/certificado-aportaciones-2026.pdf',
    );
    expect(d.materials[1]?.filename).toBe('certificado-aportaciones-2026.pdf');
    expect(d.materials[1]?.read_by).toBe('mac');
    // La de criptomonedas responde con sus cifras.
    const cripto = guardadas.find((x) => x.topic.kind === 'ahorro');
    const r3 = await assistantTopicAsk(
      { kind: 'ahorro', id: 'cripto_perdidas', year: 2026 },
      cripto?.id ?? null,
      '¿Y si vendo hoy?',
    );
    expect(r3.conversation?.messages[3]?.content).toContain('117,80 €');
    await assistantConversationDelete(c?.id ?? 0);
    expect((await assistantConversations()).some((x) => x.id === c?.id)).toBe(false);
  });

  it('un hallazgo de «Qué revisar» y otro de la carpeta, con su cabecera y su respuesta', async () => {
    installDemo();
    const aeat = (await companyReview()).hallazgos.find(
      (h) => h.codigo === 'cruces.fuera_de_plazo',
    );
    if (!aeat) throw new Error('falta el hallazgo de la demo');
    const t = findingTopic(aeat, 'aeat');
    const abierta = await assistantTopicOpen(t);
    expect(abierta.header.group).toBe('Qué revisar · 2024');
    expect(abierta.header.title).toBe('El 111 del 2T 2024 se presentó fuera de plazo');
    expect(abierta.header.facts).toContainEqual(['A pagar', '91,00 €']);
    expect(abierta.header.facts).toContainEqual(['Se puede rectificar hasta', '20/04/2028']);
    expect(abierta.header.suggestions).toEqual([
      '¿Qué significa y es grave?',
      '¿Qué le pido a mi gestoría?',
    ]);
    const r = await assistantTopicAsk(t, null, '¿Es grave?');
    expect(r.conversation?.title).toBe('El 111 del 2T 2024 se presentó fuera de plazo');
    expect(r.conversation?.messages[1]?.content).toMatch(/^\(Respuesta de ejemplo de la demo\.\)/);
    expect((await assistantConversations()).some((x) => x.id === r.conversation?.id)).toBe(true);
    // De la carpeta, sin las rutas en lo que se envía.
    const carpeta = (await folderReview('/demo')).hallazgos.find(
      (h) => h.codigo === 'ficheros.copias',
    );
    if (!carpeta) throw new Error('falta el hallazgo de la demo');
    const tc = findingTopic(carpeta, 'carpeta');
    expect((await assistantTopicOpen(tc)).header.group).toBe('Revisión de la carpeta');
    const p = await assistantTopicPreview(tc, null, null);
    expect(p.system).toContain('«fichero 1»');
    expect(p.system).not.toContain('Coworking Ruzafa');
    // Uno que ya no sale no se abre.
    await expect(
      assistantTopicOpen(findingTopic({ ...aeat, titulo: 'Otro' }, 'aeat')),
    ).rejects.toMatchObject({
      message: expect.stringMatching(/^Este hallazgo ya no sale/),
    });
    await assistantConversationDelete(r.conversation?.id ?? 0);
  });

  it('el borrador del 303, con su cabecera y su respuesta', async () => {
    installDemo();
    const t = draftTopic(await modelDraft('303', 2026, 3));
    expect(t).toEqual({ kind: 'modelo', model: '303', year: 2026, period: 3 });
    const abierta = await assistantTopicOpen(t);
    expect(abierta.header.group).toBe('Modelos · 2026');
    expect(abierta.header.title).toBe('Borrador del modelo 303 · 3T 2026');
    expect(abierta.header.facts[0]).toEqual(['Resultado', '873,60 € · a ingresar']);
    expect(abierta.header.suggestions[0]).toBe('¿Por qué me sale a ingresar?');
    const r = await assistantTopicAsk(t, null, '¿Por qué me sale a ingresar?');
    expect(r.conversation?.title).toBe('Borrador del modelo 303 · 3T 2026');
    expect((await assistantTopicPreview(t, r.conversation?.id ?? null, null)).system).toContain(
      'sin el fichero ni la relación de terceros',
    );
    await assistantConversationDelete(r.conversation?.id ?? 0);
  });
});

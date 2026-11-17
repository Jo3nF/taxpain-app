import assert from 'node:assert/strict';
import { test } from 'node:test';
import { markdown, matches } from './watch.mjs';

const sumario = {
  data: {
    sumario: {
      diario: [
        {
          seccion: [
            {
              codigo: '1',
              departamento: [
                {
                  nombre: 'MINISTERIO DE HACIENDA',
                  epigrafe: [
                    {
                      item: [
                        {
                          identificador: 'BOE-A-2026-1',
                          titulo:
                            'Orden HAC/1/2026, por la que se aprueba el modelo 303 de autoliquidación del Impuesto sobre el Valor Añadido.',
                          url_html: 'https://www.boe.es/x',
                        },
                        { identificador: 'BOE-A-2026-2', titulo: 'Orden sobre pesca de bajura.' },
                        {
                          identificador: 'BOE-A-2026-5',
                          titulo:
                            'Resolución de la Agencia Estatal de Administración Tributaria por la que se publica la revocación de números de identificación fiscal.',
                        },
                      ],
                    },
                  ],
                },
                {
                  nombre: 'MINISTERIO DE TRABAJO',
                  item: {
                    identificador: 'BOE-A-2026-3',
                    titulo:
                      'Resolución por la que se publica la relación de fiestas laborales para el año 2027.',
                  },
                },
              ],
            },
            {
              codigo: '2',
              departamento: {
                nombre: 'X',
                item: { identificador: 'BOE-A-2026-4', titulo: 'Nombramiento IVA' },
              },
            },
          ],
        },
      ],
    },
  },
};

test('detecta lo fiscal en las secciones I y III', () => {
  const m = matches(sumario);
  assert.deepEqual(
    m.map((x) => [x.id, x.topics]),
    [
      ['BOE-A-2026-1', ['modelos de la AEAT', 'IVA']],
      ['BOE-A-2026-3', ['calendario']],
    ],
  );
  const md = markdown('20261021', m);
  assert.match(md, /\[BOE-A-2026-1\]\(https:\/\/www\.boe\.es\/x\)/);
  assert.equal(markdown('20261021', []), '');
});

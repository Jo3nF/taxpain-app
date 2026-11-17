import type { ReactNode } from 'react';

/**
 * Markdown sencillo de las respuestas de la IA: párrafos, títulos (#), listas con «-», «*» o
 * «1.», negrita (**), cursiva (*), código (`) y saltos de línea. Se pinta con elementos de React,
 * nunca como HTML, así que nada de lo que escriba la IA puede meter etiquetas en la página.
 */
export function Markdown({ text }: { text: string }) {
  return <div className="md-text">{bloques(text)}</div>;
}

type Bloque =
  | { k: 'p'; lineas: string[] }
  | { k: 'h'; nivel: number; texto: string }
  | { k: 'ul' | 'ol'; items: string[] }
  | { k: 'code'; texto: string };

function partir(text: string): Bloque[] {
  const out: Bloque[] = [];
  const lineas = text.replace(/\r\n?/g, '\n').split('\n');
  let i = 0;
  while (i < lineas.length) {
    const l = lineas[i] ?? '';
    if (l.trim() === '') {
      i++;
      continue;
    }
    if (l.trim().startsWith('```')) {
      const cuerpo: string[] = [];
      i++;
      while (i < lineas.length && !(lineas[i] ?? '').trim().startsWith('```')) {
        cuerpo.push(lineas[i] ?? '');
        i++;
      }
      i++;
      out.push({ k: 'code', texto: cuerpo.join('\n') });
      continue;
    }
    const h = /^(#{1,6})\s+(.*)$/.exec(l);
    if (h) {
      out.push({ k: 'h', nivel: h[1]?.length ?? 1, texto: h[2] ?? '' });
      i++;
      continue;
    }
    const ul = /^\s*[-*•]\s+/;
    const ol = /^\s*\d+[.)]\s+/;
    if (ul.test(l) || ol.test(l)) {
      const tipo = ul.test(l) ? 'ul' : 'ol';
      const re = tipo === 'ul' ? ul : ol;
      const items: string[] = [];
      while (i < lineas.length) {
        const x = lineas[i] ?? '';
        if (re.test(x)) {
          items.push(x.replace(re, ''));
        } else if (x.trim() !== '' && /^\s+/.test(x) && items.length > 0) {
          // Continuación del elemento anterior (línea sangrada).
          items[items.length - 1] += ` ${x.trim()}`;
        } else {
          break;
        }
        i++;
      }
      out.push({ k: tipo, items });
      continue;
    }
    const parrafo: string[] = [];
    while (i < lineas.length) {
      const x = lineas[i] ?? '';
      if (
        x.trim() === '' ||
        /^#{1,6}\s/.test(x) ||
        ul.test(x) ||
        ol.test(x) ||
        x.trim().startsWith('```')
      ) {
        break;
      }
      parrafo.push(x);
      i++;
    }
    out.push({ k: 'p', lineas: parrafo });
  }
  return out;
}

function bloques(text: string): ReactNode[] {
  return partir(text).map((b, n) => {
    const key = `b${n}`;
    switch (b.k) {
      case 'h':
        return (
          <h4 key={key} className="md-h">
            {enLinea(b.texto)}
          </h4>
        );
      case 'ul':
        return (
          <ul key={key}>
            {b.items.map((it, j) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: el texto de la respuesta no cambia.
              <li key={j}>{enLinea(it)}</li>
            ))}
          </ul>
        );
      case 'ol':
        return (
          <ol key={key}>
            {b.items.map((it, j) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: el texto de la respuesta no cambia.
              <li key={j}>{enLinea(it)}</li>
            ))}
          </ol>
        );
      case 'code':
        return (
          <pre key={key}>
            <code>{b.texto}</code>
          </pre>
        );
      default:
        return (
          <p key={key}>
            {b.lineas.flatMap((l, j) =>
              // biome-ignore lint/suspicious/noArrayIndexKey: las líneas de una respuesta no cambian.
              j === 0 ? enLinea(l) : [<br key={`br${j}`} />, ...enLinea(l)],
            )}
          </p>
        );
    }
  });
}

/** Negrita, cursiva y código dentro de una línea. */
export function enLinea(texto: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /(\*\*|__)(.+?)\1|`([^`]+)`|(?<![*\w])\*(?!\s)([^*\n]+?)\*(?!\w)/g;
  let ultimo = 0;
  let n = 0;
  for (let m = re.exec(texto); m !== null; m = re.exec(texto)) {
    if (m.index > ultimo) out.push(texto.slice(ultimo, m.index));
    if (m[2] !== undefined) {
      out.push(<strong key={`s${n++}`}>{enLinea(m[2])}</strong>);
    } else if (m[3] !== undefined) {
      out.push(<code key={`c${n++}`}>{m[3]}</code>);
    } else if (m[4] !== undefined) {
      out.push(<em key={`e${n++}`}>{m[4]}</em>);
    }
    ultimo = m.index + m[0].length;
  }
  if (ultimo < texto.length) out.push(texto.slice(ultimo));
  return out;
}

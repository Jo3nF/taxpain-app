// Comprueba que todas las dependencias (Rust y npm de producción) tienen una licencia
// compatible con la MIT de TaxPain. Falla si alguna no la tiene o es copyleft (GPL, LGPL,
// AGPL…) sin alternativa permisiva. Uso: node tools/licenses/check.mjs
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

const ALLOWED = new Set([
  'MIT',
  'MIT-0',
  'Apache-2.0',
  'Apache-2.0 WITH LLVM-exception',
  'BSD-2-Clause',
  'BSD-3-Clause',
  '0BSD',
  'ISC',
  'Zlib',
  'Unicode-3.0',
  'Unicode-DFS-2016',
  'Unlicense',
  'CC0-1.0',
  'BSL-1.0',
  'MPL-2.0', // copyleft por fichero: se puede usar sin modificar sus ficheros
  'CDLA-Permissive-2.0',
  'blessing', // la «bendición» de SQLite: dominio público
]);

// Excepciones por crate: el nombre y la expresión exacta. Si la licencia del crate cambia,
// vuelve a fallar y hay que mirarla otra vez.
const EXCEPCIONES = new Map([
  // Sólo trae el código fuente de SQLCipher (BSD-3-Clause) y de SQLite (dominio público) para
  // compilarlo. «WTFPL» es por libtomcrypt (dominio público o WTFPL, a elegir), que viene en
  // el paquete para wasm y TaxPain no compila: en macOS se usa CommonCrypto y en Linux, OpenSSL.
  ['sqlcipher-src', 'MIT AND BSD-3-Clause AND blessing AND WTFPL'],
]);

/** ¿Está permitida la licencia de este crate, por la lista o por su excepción? */
export function crateOk(name, license) {
  if (!license) return false;
  return permitted(license) || EXCEPCIONES.get(name) === license;
}

/** Quita los paréntesis exteriores (todos los niveles) sólo si el primero cierra en el último carácter. */
function unwrap(expr) {
  let e = expr;
  while (e.startsWith('(') && e.endsWith(')')) {
    let depth = 0;
    for (let i = 0; i < e.length; i++) {
      if (e[i] === '(') depth++;
      if (e[i] === ')') depth--;
      // «(A OR B) AND (C OR D)»: el primer grupo cierra antes del final; no son exteriores.
      if (depth === 0 && i < e.length - 1) return e;
    }
    e = e.slice(1, -1).trim();
  }
  return e;
}

/** ¿Se puede cumplir la expresión SPDX sólo con licencias permitidas? */
export function permitted(expr) {
  const e = unwrap(expr.trim());
  // OR (también «/», notación antigua de crates.io): basta una alternativa.
  const ors = splitTop(e, / OR |\//);
  if (ors.length > 1) return ors.some(permitted);
  const ands = splitTop(e, / AND /);
  if (ands.length > 1) return ands.every(permitted);
  return ALLOWED.has(e.trim());
}

/** Separa por `sep` sólo fuera de paréntesis. */
function splitTop(s, sep) {
  const out = [];
  let depth = 0;
  let cur = '';
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (c === '(') depth++;
    if (c === ')') depth--;
    if (depth === 0) {
      const m = sep.exec(s.slice(i));
      if (m && m.index === 0) {
        out.push(cur);
        cur = '';
        i += m[0].length - 1;
        continue;
      }
    }
    cur += c;
  }
  out.push(cur);
  return out.map((x) => x.trim()).filter(Boolean);
}

function main() {
  const bad = [];
  const meta = JSON.parse(
    execFileSync('cargo', ['metadata', '--format-version', '1', '--locked'], {
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    }),
  );
  for (const p of meta.packages) {
    if (p.source === null) continue; // crates del propio workspace (MIT)
    if (!crateOk(p.name, p.license))
      bad.push(`crate ${p.name} ${p.version}: ${p.license ?? 'sin licencia'}`);
  }
  const lock = JSON.parse(readFileSync(new URL('../../package-lock.json', import.meta.url)));
  for (const [path, p] of Object.entries(lock.packages)) {
    if (!path.startsWith('node_modules/') || p.dev || p.link) continue;
    if (path.startsWith('node_modules/@taxpain/')) continue; // paquetes del propio repo
    if (!p.license || !permitted(p.license))
      bad.push(`npm ${path.slice(13)} ${p.version}: ${p.license ?? 'sin licencia'}`);
  }
  if (bad.length) {
    console.error(`Dependencias con licencia no permitida:\n  ${bad.join('\n  ')}`);
    process.exit(1);
  }
  console.log(
    `Licencias: ${meta.packages.length} crates y dependencias npm de producción, todas permisivas.`,
  );
}

if (
  process.argv[1] &&
  import.meta.url.endsWith(process.argv[1].replace(/\\/g, '/').split('/').pop())
)
  main();

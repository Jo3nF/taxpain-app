// Una sola versión de TaxPain: la de `Cargo.toml` ([workspace.package]), que también ha de estar
// en `apps/desktop/src-tauri/tauri.conf.json` y `apps/desktop/package.json`; y sus notas, en
// `CHANGELOG.md` («## 0.2.0 — 2026-11-03»).
// Uso: node scripts/version.mjs                 → imprime la versión (falla si no coinciden)
//      node scripts/version.mjs set 0.2.0       → cambia las tres
//      node scripts/version.mjs notes [versión] → notas de esa versión (falla si no hay)
//      node scripts/version.mjs tag v0.2.0      → comprueba que la etiqueta es la versión y tiene notas
import { readFileSync, writeFileSync } from 'node:fs';

const root = new URL('../', import.meta.url);
const FILES = {
  cargo: 'Cargo.toml',
  tauri: 'apps/desktop/src-tauri/tauri.conf.json',
  npm: 'apps/desktop/package.json',
};
const read = (f) => readFileSync(new URL(f, root), 'utf8');
const write = (f, s) => writeFileSync(new URL(f, root), s);
const SEMVER = /^\d+\.\d+\.\d+$/;
const CARGO_RE = /(\[workspace\.package\][^[]*?\nversion\s*=\s*")([^"]+)(")/;

export function versions() {
  const cargo = read(FILES.cargo).match(CARGO_RE)?.[2];
  return {
    cargo,
    tauri: JSON.parse(read(FILES.tauri)).version,
    npm: JSON.parse(read(FILES.npm)).version,
  };
}

export function current() {
  const v = versions();
  const all = new Set(Object.values(v));
  if (all.size !== 1 || !SEMVER.test(v.cargo ?? '')) {
    throw new Error(
      `Las versiones no coinciden: ${Object.entries(v)
        .map(([k, x]) => `${FILES[k]}=${x}`)
        .join(', ')}. Usa: node scripts/version.mjs set X.Y.Z`,
    );
  }
  return v.cargo;
}

/** Notas de `version` en el texto de un CHANGELOG, o `null`. */
export function notesOf(changelog, version) {
  const lines = changelog.split('\n');
  const start = lines.findIndex((l) =>
    new RegExp(`^## ${version.replaceAll('.', '\\.')}\\b`).test(l),
  );
  if (start < 0) return null;
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((l) => l.startsWith('## '));
  const body = (end < 0 ? rest : rest.slice(0, end)).join('\n').trim();
  return body || null;
}

function setVersion(v) {
  if (!SEMVER.test(v)) throw new Error(`Versión no válida: ${v} (X.Y.Z)`);
  write(FILES.cargo, read(FILES.cargo).replace(CARGO_RE, `$1${v}$3`));
  for (const f of [FILES.tauri, FILES.npm]) {
    // Sólo el campo de primer nivel, conservando el formato.
    write(f, read(f).replace(/^(\s{2}"version":\s*")[^"]+(")/m, `$1${v}$2`));
  }
}

const isMain = import.meta.url === `file://${process.argv[1]}`;
if (isMain) {
  const [cmd, arg] = process.argv.slice(2);
  try {
    if (!cmd) {
      process.stdout.write(`${current()}\n`);
    } else if (cmd === 'set') {
      setVersion(arg ?? '');
      process.stdout.write(
        `${current()}\nRecuerda: añadir «## ${arg}» en CHANGELOG.md y compilar (actualiza Cargo.lock).\n`,
      );
    } else if (cmd === 'notes') {
      const v = arg ?? current();
      const n = notesOf(read('CHANGELOG.md'), v);
      if (!n) throw new Error(`CHANGELOG.md no tiene notas para la ${v}`);
      process.stdout.write(`${n}\n`);
    } else if (cmd === 'tag') {
      const v = current();
      if (arg !== `v${v}`) throw new Error(`La etiqueta ${arg} no es la versión v${v}`);
      if (!notesOf(read('CHANGELOG.md'), v)) {
        throw new Error(`CHANGELOG.md no tiene notas para la ${v}`);
      }
      process.stdout.write(`v${v} OK\n`);
    } else {
      throw new Error(`Orden desconocida: ${cmd}`);
    }
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}

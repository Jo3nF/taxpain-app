// Test de la versión única y de las notas de versión.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { current, notesOf, versions } from '../../scripts/version.mjs';

// Rutas desde el repositorio, no desde el directorio actual: el test vale desde cualquier carpeta.
const root = resolve(import.meta.dirname, '../..');
const script = join(root, 'scripts/version.mjs');

test('Cargo.toml, tauri.conf.json y package.json tienen la misma versión, con notas', () => {
  const v = current();
  assert.deepEqual(versions(), { cargo: v, tauri: v, npm: v });
  assert.ok(
    notesOf(readFileSync(join(root, 'CHANGELOG.md'), 'utf8'), v),
    `faltan notas de la ${v}`,
  );
  const out = execFileSync('node', [script, 'tag', `v${v}`], { encoding: 'utf8' });
  assert.equal(out.trim(), `v${v} OK`);
  assert.throws(() => execFileSync('node', [script, 'tag', 'v9.9.9'], { stdio: 'pipe' }));
});

test('notas de una versión: hasta la siguiente sección', () => {
  const c = '# Notas\n\n## 0.2.0 — 2026-11-03\n\nArreglos.\n\n## 0.1.0\n\nPrimera.\n';
  assert.equal(notesOf(c, '0.2.0'), 'Arreglos.');
  assert.equal(notesOf(c, '0.1.0'), 'Primera.');
  assert.equal(notesOf(c, '0.1.1'), null);
  assert.equal(notesOf('## 0.10.0\n\nx\n', '0.1.0'), null);
});

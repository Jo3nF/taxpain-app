import assert from 'node:assert/strict';
import { test } from 'node:test';
import { crateOk, permitted } from './check.mjs';

test('expresiones SPDX', () => {
  assert.ok(permitted('MIT'));
  assert.ok(permitted('MIT OR Apache-2.0'));
  assert.ok(permitted('MIT/Apache-2.0'));
  assert.ok(permitted('(MIT OR Apache-2.0) AND Unicode-3.0'));
  assert.ok(permitted('MIT OR Apache-2.0 OR LGPL-2.1-or-later'));
  assert.ok(permitted('Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT'));
  assert.ok(!permitted('GPL-3.0-only'));
  assert.ok(!permitted('LGPL-2.1-or-later'));
  assert.ok(!permitted('MIT AND GPL-2.0'));
  assert.ok(!permitted('AGPL-3.0 OR SSPL-1.0'));
  // Dos grupos: el recorte de paréntesis sólo vale si el primero cierra al final.
  assert.ok(!permitted('(MIT OR Apache-2.0) AND (GPL-3.0-only OR SSPL-1.0)'));
  assert.ok(permitted('(GPL-3.0-only) OR (MIT)'));
  assert.ok(permitted('((MIT OR Apache-2.0))'));
  assert.ok(!permitted('MIT AND (GPL-2.0 OR LGPL-2.1)'));
});

test('la bendición de SQLite vale; WTFPL, sólo en la excepción de sqlcipher-src', () => {
  assert.ok(permitted('blessing'));
  assert.ok(!permitted('WTFPL'));
  assert.ok(!permitted('MIT AND BSD-3-Clause AND blessing AND WTFPL'));
  assert.ok(crateOk('sqlcipher-src', 'MIT AND BSD-3-Clause AND blessing AND WTFPL'));
  // Con otro nombre o con otra licencia, la excepción no vale.
  assert.ok(!crateOk('otro-crate', 'MIT AND BSD-3-Clause AND blessing AND WTFPL'));
  assert.ok(!crateOk('sqlcipher-src', 'MIT AND GPL-3.0-only'));
  assert.ok(crateOk('cualquiera', 'MIT OR Apache-2.0'));
  assert.ok(!crateOk('sin-licencia', null));
});

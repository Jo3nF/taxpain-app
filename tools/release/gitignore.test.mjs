// El .gitignore excluye las claves que firman la app y las actualizaciones (docs/RELEASE.md).
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { test } from 'node:test';

test('.gitignore excluye las claves del actualizador y de App Store Connect', () => {
  const lines = readFileSync(resolve(import.meta.dirname, '../../.gitignore'), 'utf8')
    .split('\n')
    .map((l) => l.trim());
  for (const p of ['*.key', '*.p8', '*.minisign', '*.p12', '*.pfx', '.env']) {
    assert.ok(lines.includes(p), `falta ${p}`);
  }
});

// Los paquetes del actualizador van a la release de GitHub; en web/updates/ sólo cabe
// latest.json, que sí se puede versionar (CONSTRUCCION-WEB-PRUEBAS-01).
test('.gitignore excluye los paquetes del actualizador pero no latest.json', () => {
  const out = execFileSync('git', ['check-ignore', '--no-index', '-v', '-n', '--stdin'], {
    cwd: resolve(import.meta.dirname, '../..'),
    input: [
      'web/updates/TaxPain.app.tar.gz',
      'web/updates/TaxPain.app.tar.gz.sig',
      'web/updates/TaxPain_0.1.0_universal.dmg',
      'web/updates/latest.json',
      'apps/desktop/updater.key',
      'AuthKey_ABC.p8',
      'firma.minisign',
    ].join('\n'),
    encoding: 'utf8',
  });
  // `-n -v`: una línea por ruta; las ignoradas llevan «fichero:línea:patrón», las no ignoradas «::».
  const ignored = Object.fromEntries(
    out
      .trim()
      .split('\n')
      .map((l) => {
        const [src, , , path] = l.split(/:|\t/);
        return [path, src !== ''];
      }),
  );
  for (const p of [
    'web/updates/TaxPain.app.tar.gz',
    'web/updates/TaxPain.app.tar.gz.sig',
    'web/updates/TaxPain_0.1.0_universal.dmg',
    'apps/desktop/updater.key',
    'AuthKey_ABC.p8',
    'firma.minisign',
  ]) {
    assert.equal(ignored[p], true, `${p} debería estar ignorado`);
  }
  assert.equal(ignored['web/updates/latest.json'], false, 'latest.json debe poder versionarse');
});

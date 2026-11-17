// Genera el manifiesto estático del actualizador de Tauri (latest.json) a partir del
// `.app.tar.gz` y su `.sig` de una compilación universal de macOS.
// Uso: node scripts/updater-manifest.mjs <dir bundle/macos> [url base] > latest.json
// La URL base es de donde la app descargará el paquete. El workflow Release pasa la de los
// assets de la release de GitHub de la etiqueta (…/releases/download/vX.Y.Z); el valor por
// defecto, https://taxpain.es/updates, sólo sirve para manifiestos de prueba (docs/RELEASE.md §3).
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { current, notesOf } from './version.mjs';

const dir = process.argv[2];
const base = (process.argv[3] ?? 'https://taxpain.es/updates').replace(/\/$/, '');
if (!dir) {
  console.error('Uso: node scripts/updater-manifest.mjs <dir bundle/macos> [url base]');
  process.exit(2);
}
// Una sola fuente: falla si Cargo.toml, tauri.conf.json y package.json no coinciden o si
// CHANGELOG.md no tiene notas de esta versión (así no se anuncia algo sin explicar).
const version = current();
const notes = notesOf(readFileSync(new URL('../CHANGELOG.md', import.meta.url), 'utf8'), version);
if (!notes) {
  console.error(`CHANGELOG.md no tiene notas para la ${version}`);
  process.exit(1);
}
const archive = readdirSync(dir).find((f) => f.endsWith('.app.tar.gz'));
if (!archive) {
  console.error(`No hay ningún .app.tar.gz en ${dir}`);
  process.exit(1);
}
const signature = readFileSync(join(dir, `${archive}.sig`), 'utf8').trim();
// Universal: el mismo paquete sirve para Apple Silicon e Intel.
const entry = { signature, url: `${base}/${encodeURIComponent(archive)}` };
const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: { 'darwin-aarch64': entry, 'darwin-x86_64': entry },
};
process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);

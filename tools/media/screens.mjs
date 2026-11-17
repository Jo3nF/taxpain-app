// Capturas de la interfaz real en modo demo (datos sintéticos) para la web y el vídeo.
//
// Uso: node tools/media/screens.mjs [--no-build] [--solo=24,25…]
// (construye el frontend si hace falta; con --solo sólo escribe las capturas cuyo nombre empieza así)
// Salida: media/screens/*.png (2x). Usa el Chromium «headless shell» de Playwright ya instalado
// en la caché; no descarga navegadores.

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, resolve } from 'node:path';
import { abrir } from '../navegador.mjs';

const root = resolve(import.meta.dirname, '../..');
const dist = join(root, 'apps/desktop/dist');
const out = join(root, 'media/screens');
mkdirSync(out, { recursive: true });

const solo = process.argv
  .find((a) => a.startsWith('--solo='))
  ?.slice(7)
  .split(',');
const quiere = (name) => !solo || solo.some((p) => name.startsWith(p));

if (!process.argv.includes('--no-build')) {
  execFileSync('npx', ['vite', 'build'], { cwd: join(root, 'apps/desktop'), stdio: 'inherit' });
}
if (!existsSync(join(dist, 'index.html'))) throw new Error('falta apps/desktop/dist');

const types = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.css': 'text/css',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
};
const server = createServer((req, res) => {
  const url = new URL(req.url ?? '/', 'http://localhost');
  let path = join(dist, decodeURIComponent(url.pathname));
  if (!path.startsWith(dist) || !existsSync(path) || url.pathname === '/')
    path = join(dist, 'index.html');
  res.writeHead(200, { 'content-type': types[extname(path)] ?? 'application/octet-stream' });
  res.end(readFileSync(path));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}/?demo=1`;

// Inter (OFL) sólo para las capturas: en macOS la app usa la fuente del sistema.
const inter = readFileSync(
  join(root, 'node_modules/@fontsource-variable/inter/files/inter-latin-wght-normal.woff2'),
).toString('base64');
const fontCss = `@font-face{font-family:InterV;src:url(data:font/woff2;base64,${inter}) format('woff2');font-weight:100 900}
*{font-family:InterV,system-ui,sans-serif!important;-webkit-font-smoothing:antialiased}
main{min-height:100vh}`;

const browser = await abrir({
  headless: true,
  args: ['--lang=es-ES'],
  env: { ...process.env, LANG: 'es_ES.UTF-8', LANGUAGE: 'es_ES' },
});
const page = await browser.newPage({
  viewport: { width: 1280, height: 800 },
  deviceScaleFactor: 2,
  locale: 'es-ES',
});
page.on('pageerror', (e) => console.error('pageerror', e.message));

async function open() {
  await page.goto(base);
  await page.addStyleTag({ content: fontCss });
  await page.waitForTimeout(300);
}
async function tab(name) {
  await page.getByRole('navigation', { name: 'Secciones' }).getByRole('button', { name }).click();
  await page.waitForTimeout(300);
}
async function shot(name, fullPage = false, clip = undefined) {
  if (!quiere(name)) return;
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: join(out, `${name}.png`), fullPage, clip });
  console.log(`  ${name}.png`);
}

await open();
await page.getByText('Tu negocio en 2026').waitFor();
await shot('12-panel');
// Previsión de tesorería del panel (sólo la tarjeta).
{
  const card = page.locator('section[aria-labelledby="forecast-title"]');
  await card.scrollIntoViewIfNeeded();
  await page.waitForTimeout(300);
  if (quiere('15-prevision')) {
    await card.screenshot({ path: join(out, '15-prevision.png') });
    console.log('  15-prevision.png');
  }
  await page.evaluate(() => window.scrollTo(0, 0));
}

await tab('Presupuestos');
await page.getByText('P2026-0003').waitFor();
await shot('13-presupuestos', false, { x: 0, y: 0, width: 1280, height: 560 });

await tab('Clientes y proveedores');
await page.getByRole('button', { name: 'Atelier Lumière SAS' }).click();
await page
  .getByText(/Francia/)
  .first()
  .waitFor();
await shot('14-clientes');
await tab('Calendario');
await page.getByText('Faltan 7 días').first().waitFor();
await shot('01-calendario');

await tab('Facturas');
await page.getByText('A2026-0006 · Estudio Norte').waitFor();
await shot('02-facturas');
await page.getByRole('button', { name: 'Nueva factura' }).click();
await page.getByLabel('NIF del cliente (vacío = simplificada)').fill('B12345674');
await page.getByLabel('Nombre o razón social').last().fill('Estudio Norte Arquitectura SL');
await page.getByLabel('Concepto 1').fill('Diseño y desarrollo de la web corporativa');
await page.getByLabel('Precio 1').fill('1800');
await page.getByRole('button', { name: 'Emitir factura…' }).click();
await page.getByRole('dialog', { name: 'Confirmar emisión' }).waitFor();
await page.getByRole('dialog').scrollIntoViewIfNeeded();
await shot('03-emitir-confirmacion');

await tab('Recibidas');
await page.getByText('Coworking Ruzafa SL').first().click();
await page.getByRole('button', { name: 'Confirmar datos' }).waitFor();
await shot('04-recibidas-revision');

await tab('Contabilidad');
await page.getByText('Factura emitida A2026-0006').first().waitFor();
await shot('05-diario');
await page.getByRole('button', { name: 'Sumas y saldos' }).click();
await page.getByText('Cuadra').waitFor();
await shot('06-sumas-y-saldos');
await page.getByRole('button', { name: 'Banco' }).click();
await page.getByText(/TRANSFERENCIA ESTUDIO NORTE/).click();
await page.getByText(/Cobro de A2026-0006/).waitFor();
await shot('07-banco-conciliacion');
await page.getByRole('button', { name: 'Cuentas anuales' }).click();
await page.getByRole('heading', { name: 'Cuenta de pérdidas y ganancias' }).waitFor();
await shot('08-cuentas-anuales');
await page.getByRole('button', { name: 'Cobros y pagos' }).click();
await page.getByLabel('Mostrar').selectOption('todas');
await page.getByText('Vencida hace 16 d').waitFor();
// Recortada: la vista es corta y el resto de la ventana queda en blanco.
await shot('11-cartera', false, { x: 0, y: 0, width: 1280, height: 620 });

await tab('Modelos');
await page.getByText('Cuota al 21 %').waitFor();
await shot('10-modelo-303');

await tab('Ajustes');
await page.getByText('CARMEN ESPAÑOLA ESPAÑOLA', { exact: true }).waitFor();
await shot('09-certificado');

await tab('Evolución');
await page.waitForTimeout(600);
await shot('16-evolucion');

await tab('Ahorrar impuestos');
await page.getByRole('button', { name: /^Todas \(/ }).waitFor();
await shot('17-ahorro');

await tab('Criptomonedas');
await page.waitForTimeout(600);
await shot('18-criptomonedas');

await tab('Importar');
await page.getByRole('button', { name: 'Revisión', exact: true }).click();
await page.getByRole('button', { name: 'Revisar la carpeta', exact: true }).click();
await page.getByRole('heading', { name: 'Lo que se ha encontrado' }).waitFor();
await shot('19-revision');

await tab('Contabilidad');
await page.getByRole('button', { name: 'Banco' }).click();
{
  const s = page.locator('section[aria-labelledby="bancos-conectados"]');
  await s.scrollIntoViewIfNeeded();
  await page.waitForTimeout(300);
  if (quiere('20-banco-conexion')) {
    await s.screenshot({ path: join(out, '20-banco-conexion.png') });
    console.log('  20-banco-conexion.png');
  }
}

await tab('Inversiones en valores');
await page.getByRole('list', { name: 'Cuentas de bróker' }).waitFor();
await page
  .getByRole('button', { name: /^Sincronizar \d{4}$/ })
  .first()
  .click();
await page.getByText(/^Sincronizado \d{4}:/).waitFor();
await shot('21-valores');

await tab('Asistente');
await page.getByRole('button', { name: /^Criptomonedas: compensa/ }).click();
await page.getByRole('log', { name: 'Conversación' }).waitFor();
await page.waitForTimeout(300);
await shot('22-asistente');

await tab('Ajustes');
{
  const s = page.locator('section[aria-labelledby="claude-mcp-title"]');
  await s.scrollIntoViewIfNeeded();
  await page.waitForTimeout(300);
  if (quiere('23-claude-mcp')) {
    await s.screenshot({ path: join(out, '23-claude-mcp.png') });
    console.log('  23-claude-mcp.png');
  }
}

// ---- Capturas de los vídeos «funciones» y «pasos» ----

// Recorte de la columna principal entre dos elementos (de arriba de `desde` a abajo de `hasta`).
async function tramo(name, desde, hasta, margen = 16, pie = margen) {
  await desde.scrollIntoViewIfNeeded();
  const a = await desde.boundingBox();
  const b = await hasta.boundingBox();
  const m = await page.locator('main').boundingBox();
  const sy = await page.evaluate(() => window.scrollY);
  await shot(name, true, {
    x: m.x,
    y: a.y + sy - margen,
    width: m.width,
    height: b.y + b.height - a.y + margen + pie,
  });
}

// Alta de una empresa (el formulario se abre bajo el selector): la sociedad de la revisión de la
// demo, con su nombre y su NIF inventados.
await tab('Facturas');
await page.locator('.company-trigger').click();
await page.getByText('Añadir empresa…').click();
await page.getByRole('form', { name: 'Nueva empresa' }).waitFor();
const alta = page.getByRole('form', { name: 'Nueva empresa' });
await alta.getByLabel('Nombre o razón social').fill('Estudio Demo SL');
await alta.getByLabel('NIF (opcional)').fill('B12345674');
await shot('24-nueva-empresa');
await page.getByRole('button', { name: 'Cancelar' }).click();

// La carpeta de la empresa.
await tab('Importar');
await page.getByRole('button', { name: 'Elegir carpeta…' }).waitFor();
await page.evaluate(() => window.scrollTo(0, 0));
await shot('25-carpeta', false, { x: 0, y: 0, width: 1280, height: 640 });

// «Comprobar lo presentado»: la petición y el detalle del año. El resumen por años queda fuera.
// En el ejercicio anterior, que es el que la demo trae con sus fechas.
await tab('Modelos');
await page.getByRole('button', { name: 'Año anterior' }).first().click();
{
  const s = page.locator('section[aria-labelledby="aeat-check-h"]');
  await s.getByRole('button', { name: /^Comprobar lo presentado en \d{4}$/ }).click();
  const consultar = s.getByRole('button', { name: 'Consultar', exact: true });
  await consultar.waitFor();
  await tramo('26-presentado', s.locator('#aeat-check-h'), consultar, 28);
  await consultar.click();
  const tabla = s.getByRole('table', { name: /^Declaraciones de \d{4}$/ });
  await tabla.waitFor();
  await tramo('27-presentado-detalle', s.getByText(/^Detalle de \d{4}$/), tabla, 16, 2);
}

// «Qué revisar», con la cabecera arriba.
await tab('Evolución');
{
  const h = page.locator('#revisar-h');
  await h.waitFor();
  await h.evaluate((n) => window.scrollTo(0, n.getBoundingClientRect().top + window.scrollY - 28));
  await page.waitForTimeout(300);
  await shot('28-que-revisar');
  await page.evaluate(() => window.scrollTo(0, 0));
}

await tab('Contabilidad');
await page.getByRole('button', { name: 'Inmovilizado', exact: true }).click();
await page
  .getByText(/Plan de amortización/)
  .first()
  .waitFor();
await page.mouse.move(700, 700);
await page.waitForTimeout(400);
await shot('29-inmovilizado', false, { x: 0, y: 0, width: 1280, height: 640 });
await page.getByRole('button', { name: 'Cierre', exact: true }).click();
await page.getByText(/^Antes de cerrar \d{4}/).waitFor();
await page.mouse.move(700, 700);
await page.waitForTimeout(400);
await shot('30-cierre');

await browser.close();
server.close();
console.log(`Capturas en ${out}`);

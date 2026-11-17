// Accesibilidad de la interfaz en modo demo (datos sintéticos) con axe-core: recorre las
// secciones y las vistas de contabilidad y falla si hay infracciones graves o críticas (WCAG 2.1
// A y AA). Idea de reforma-digital (docs/research/REFORMA_DIGITAL.md).
//
// Uso: node tools/a11y/check.mjs [--no-build] [--dark]   (--dark: con el modo oscuro del sistema)
// Usa el Chromium «headless shell» de Playwright ya instalado; no descarga navegadores.

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import { extname, join, resolve } from 'node:path';
import { abrir } from '../navegador.mjs';

const root = resolve(import.meta.dirname, '../..');
const dist = join(root, 'apps/desktop/dist');
if (!process.argv.includes('--no-build')) {
  execFileSync('npx', ['vite', 'build'], { cwd: join(root, 'apps/desktop'), stdio: 'inherit' });
}
const axeSource = readFileSync(
  createRequire(import.meta.url).resolve('axe-core/axe.min.js'),
  'utf8',
);

const types = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.css': 'text/css',
  '.svg': 'image/svg+xml',
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

const browser = await abrir({ headless: true, args: ['--lang=es-ES'] });
const page = await browser.newPage({
  viewport: { width: 1280, height: 800 },
  locale: 'es-ES',
  colorScheme: process.argv.includes('--dark') ? 'dark' : 'light',
});
const problems = [];
const audited = [];
// Un error de JavaScript en la demo también es un fallo (una pantalla que se rompe al abrirla).
let vista = 'inicio';
page.on('pageerror', (e) => problems.push(`${vista}: error de JavaScript: ${e.message}`));

async function audit(label) {
  vista = label;
  audited.push(label);
  await page.waitForTimeout(250);
  if (!(await page.evaluate(() => 'axe' in window)))
    await page.addScriptTag({ content: axeSource });
  const r = await page.evaluate(async () =>
    window.axe.run(document, {
      runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] },
    }),
  );
  for (const v of r.violations) {
    if (v.impact !== 'serious' && v.impact !== 'critical') continue;
    problems.push(
      `${label}: ${v.id} (${v.impact}) — ${v.help}\n    ${v.nodes
        .slice(0, 3)
        .map((n) => n.target.join(' '))
        .join('\n    ')}`,
    );
  }
}

/** Abre la conversación con la IA del botón `boton`, con sus desplegables abiertos. */
async function abrirPanel(boton) {
  await page.getByRole('button', { name: boton }).first().click();
  const panel = page.getByRole('dialog', { name: /^Conversación con la IA: / });
  await panel.getByLabel('Tu pregunta').waitFor();
  await panel.locator('details').evaluateAll((ds) => {
    for (const d of ds) d.open = true;
  });
  return panel;
}

/** Cierra el panel con Esc y comprueba que el foco vuelve al botón que lo abrió. */
async function cerrarPanel(panel, donde) {
  await page.keyboard.press('Escape');
  await panel.waitFor({ state: 'detached' });
  const foco = (await page.evaluate(() => document.activeElement?.textContent ?? '')).trim();
  if (!/^(Preguntar a la IA|Seguir con la IA)/.test(foco))
    problems.push(`${donde}: al cerrar la conversación el foco no vuelve a su botón («${foco}»)`);
}

await page.goto(base);
const nav = page.getByRole('navigation', { name: 'Secciones' });
const sections = await nav.getByRole('button').allInnerTexts();
for (const name of sections) {
  await nav.getByRole('button', { name, exact: true }).click();
  await audit(name.trim());
  if (name.trim() === 'Importar') {
    // Descargar de la AEAT por CSV: los resultados (guardado, ya estaba, anulado con su
    // sustituto, no encontrado) y el aviso de los descargados que las reglas no entienden.
    await page
      .getByLabel('Códigos CSV')
      .fill('ABCD1234EFGH5678\nQWER5678TYUI9012\nASDF1234GHJK5678\nZXCV9012BNML3456');
    await page.getByRole('button', { name: 'Descargar', exact: true }).click();
    await page.getByText(/documentos? descargados? que las reglas/).waitFor();
    await audit('Importar → Descargar por CSV');
    await page.getByRole('button', { name: /Descargar el que lo sustituye/ }).click();
    await page.getByText(/2 documentos guardados/).waitFor();
    await audit('Importar → Descargar por CSV → sustituto');
    // Revisión de la carpeta, con las tablas y las listas de ficheros abiertas.
    await page.getByRole('button', { name: 'Revisión', exact: true }).click();
    await page.getByRole('button', { name: 'Revisar la carpeta', exact: true }).click();
    await page.getByRole('heading', { name: 'Lo que se ha encontrado' }).waitFor();
    await page.evaluate(() => {
      for (const d of document.querySelectorAll('details')) d.open = true;
    });
    await audit('Importar → Revisión');
    // La conversación con la IA de un hallazgo de la carpeta.
    const panel = await abrirPanel(/^Preguntar a la IA: 1 NIF con la letra de control mal/);
    await audit('Importar → Revisión → conversación con la IA de un hallazgo');
    await cerrarPanel(panel, 'Importar → Revisión');
  }
  if (name.trim() === 'Evolución') {
    // «Qué revisar»: la conversación con la IA de un hallazgo, con una pregunta y su respuesta.
    const panel = await abrirPanel(/^Preguntar a la IA: El 390 de 2024/);
    await audit('Evolución → Qué revisar → conversación con la IA');
    await panel
      .getByRole('group', { name: 'Preguntas para empezar' })
      .getByRole('button')
      .first()
      .click();
    await panel.getByRole('button', { name: 'Preguntar', exact: true }).click();
    await panel.getByText(/Antes de enviar se sustituyeron/).waitFor();
    await audit('Evolución → Qué revisar → conversación con la IA → respuesta');
    await cerrarPanel(panel, 'Evolución → Qué revisar');
  }
  if (name.trim() === 'Modelos') {
    // La conversación con la IA sobre el borrador que se ve.
    const panel = await abrirPanel(/^Preguntar a la IA sobre este borrador/);
    await audit('Modelos → conversación con la IA sobre el borrador');
    await cerrarPanel(panel, 'Modelos');
  }
  if (name.trim() === 'Ahorrar impuestos') {
    // Todas las recomendaciones, con «Qué es y cómo conseguirlo» abierto (requisitos y pasos).
    await page.getByRole('button', { name: /^Todas \(/ }).click();
    await page.evaluate(() => {
      for (const d of document.querySelectorAll('details')) d.open = true;
    });
    await audit('Ahorrar impuestos → Todas, abiertas');
    // La conversación con la IA de los planes de pensiones (el resumen tiene otro «Preguntar a
    // la IA» antes, y la de criptomonedas ya dice «Seguir con la IA»): pregunta, un dato, un
    // documento, «Ver qué se envía», ventana estrecha y Esc, con el foco de vuelta en su botón.
    await page.getByRole('button', { name: /^Preguntar a la IA: Planes de pensiones/ }).click();
    const panel = page.getByRole('dialog', { name: /^Conversación con la IA: / });
    await panel.getByLabel('Tu pregunta').waitFor();
    await audit('Ahorrar impuestos → conversación con la IA');
    await panel
      .getByRole('group', { name: 'Preguntas para empezar' })
      .getByRole('button')
      .first()
      .click();
    await panel.getByRole('button', { name: 'Preguntar', exact: true }).click();
    await panel.getByText(/Antes de enviar se sustituyeron/).waitFor();
    await audit('Ahorrar impuestos → conversación con la IA → respuesta');
    await panel.getByRole('button', { name: 'Añadir un dato' }).click();
    await panel
      .getByLabel('Dato para la IA')
      .fill('En 2026 ya aporté 1.500 € a un plan individual.');
    await panel.getByRole('button', { name: 'Añadir', exact: true }).click();
    await panel.getByRole('button', { name: /^Añadir un documento…/ }).click();
    await panel.getByText(/^Documento añadido/).waitFor();
    await panel.getByRole('button', { name: 'Ver qué se envía' }).click();
    await panel.getByText(/exactamente lo que se envía/).waitFor();
    await audit('Ahorrar impuestos → conversación con la IA → datos, documento y qué se envía');
    await page.setViewportSize({ width: 800, height: 800 });
    await audit('Ahorrar impuestos → conversación con la IA (ventana estrecha)');
    await page.setViewportSize({ width: 1280, height: 800 });
    await cerrarPanel(panel, 'Ahorrar impuestos');
  }
  if (name.trim() === 'Inversiones en valores') {
    // Los pasos para conectar Interactive Brokers (token y Query ID), lo que sale al sincronizar
    // y los apartados plegados (qué PDF de XTB descargar, precios, valores, asientos).
    await page.getByRole('list', { name: 'Cuentas de bróker' }).waitFor();
    await page.evaluate(() => {
      for (const d of document.querySelectorAll('details')) d.open = true;
    });
    await audit('Inversiones en valores → apartados abiertos');
    await page.getByRole('button', { name: 'Cambiar el token…' }).first().click();
    await page.getByLabel('Token del Flex Web Service').waitFor();
    await audit('Inversiones en valores → conectar Interactive Brokers');
    await page.getByRole('button', { name: 'Cancelar', exact: true }).click();
    await page
      .getByRole('button', { name: /^Sincronizar \d{4}$/ })
      .first()
      .click();
    await page.getByText(/^Sincronizado \d{4}:/).waitFor();
    await audit('Inversiones en valores → sincronizado');
    await page.getByRole('button', { name: 'Nueva operación' }).click();
    await page.getByRole('form', { name: 'Operación' }).waitFor();
    await audit('Inversiones en valores → nueva operación');
  }
  if (name.trim() === 'Asistente') {
    // Las conversaciones guardadas y una de un tema abierta en la página.
    await page.getByRole('heading', { name: 'Conversaciones guardadas' }).waitFor();
    await audit('Asistente → conversaciones guardadas');
    await page.getByRole('button', { name: /^Criptomonedas: compensa/ }).click();
    await page.getByRole('log', { name: 'Conversación' }).waitFor();
    await audit('Asistente → conversación de un tema');
    // En una ventana baja, con «Añadir un dato» y «Ver qué se envía» abiertos, el cuadro para
    // preguntar sigue dentro de la caja de la conversación (el registro es el que se encoge).
    await page.setViewportSize({ width: 1024, height: 700 });
    await page.getByRole('button', { name: 'Añadir un dato' }).click();
    await page.getByRole('button', { name: 'Ver qué se envía' }).click();
    await page.getByText(/exactamente lo que se envía/).waitFor();
    await audit('Asistente → conversación con un dato a medias y lo que se envía (ventana baja)');
    const fuera = await page.getByRole('log', { name: 'Conversación' }).evaluate((log) => {
      const caja = log.parentElement;
      const cuadro = caja?.lastElementChild;
      return caja && cuadro
        ? Math.round(cuadro.getBoundingClientRect().bottom - caja.getBoundingClientRect().bottom)
        : 0;
    });
    if (fuera > 1)
      problems.push(`Asistente: el cuadro para preguntar sale ${fuera} px por debajo de su caja`);
    await page.setViewportSize({ width: 1280, height: 800 });
  }
  if (name.trim() === 'Contabilidad') {
    const sub = page
      .locator('nav')
      .filter({ has: page.getByRole('button', { name: 'Libro diario' }) });
    const views = await sub.getByRole('button').allInnerTexts();
    for (const v of views.map((x) => x.trim()).filter(Boolean)) {
      await sub.getByRole('button', { name: v, exact: true }).click();
      await audit(`Contabilidad → ${v}`);
      if (v === 'Banco') {
        // Conexión con el banco: la configuración abierta, los bancos y el paso de pegar la vuelta.
        await page.evaluate(() => {
          for (const d of document.querySelectorAll('details')) d.open = true;
        });
        await page.getByRole('button', { name: 'Conectar un banco…' }).click();
        await page.getByRole('button', { name: 'BBVA · empresa' }).click();
        await page.getByLabel('Dirección a la que te ha llevado el banco').waitFor();
        await audit('Contabilidad → Banco → conectar un banco');
        await page.getByRole('button', { name: 'Cancelar' }).click();
      }
      if (v === 'Inmovilizado') {
        // Venta con su factura (CONTABILIDAD-08): el formulario de baja con la factura elegida y
        // su confirmación, el panel para vincular después la de una venta sin factura y el de
        // pasar a la venta una rectificativa pendiente.
        const vender = page.getByRole('button', { name: 'Dar de baja o vender', exact: true });
        await vender.first().click();
        const factura = page
          .locator('label', { hasText: 'Factura de venta (opcional)' })
          .locator('select');
        await factura.locator('option').nth(1).waitFor({ state: 'attached' });
        await factura.selectOption({ index: 1 });
        await audit('Contabilidad → Inmovilizado → vender con su factura');
        await page.getByRole('button', { name: 'Registrar la baja', exact: true }).click();
        const confirmar = page.getByRole('alertdialog');
        await confirmar.waitFor();
        await audit('Contabilidad → Inmovilizado → confirmar la venta');
        await confirmar.getByRole('button', { name: 'Cancelar', exact: true }).click();
        await page
          .getByRole('button', { name: 'Vincular la factura de venta', exact: true })
          .first()
          .click();
        // El buscador de la factura sin resultados (el aviso y el recuento para el lector).
        const buscar = page.getByLabel('Buscar la factura');
        await buscar.fill('nadie');
        await page.getByText('Ninguna factura que se pueda vincular lleva «nadie».').waitFor();
        await audit('Contabilidad → Inmovilizado → buscar la factura de venta');
        await buscar.fill('');
        const vincular = page.locator('label', { hasText: 'Factura de venta' }).locator('select');
        await vincular.locator('option').nth(1).waitFor({ state: 'attached' });
        await vincular.selectOption({ index: 1 });
        await audit('Contabilidad → Inmovilizado → vincular la factura de venta');
        await page
          .getByRole('button', { name: /^Pasar .* a la venta$/ })
          .first()
          .click();
        await page.getByLabel('Importe de este elemento (sin IVA)').waitFor();
        await audit('Contabilidad → Inmovilizado → pasar una rectificativa a la venta');
      }
      // Pestañas internas (Cobros y pagos: vencimientos, condiciones, remesas).
      for (const t of ['Condiciones de pago', 'Remesas SEPA']) {
        const b = page.getByRole('button', { name: t, exact: true });
        if (await b.isVisible()) {
          await b.click();
          await audit(`Contabilidad → ${v} → ${t}`);
        }
      }
    }
  }
}
await browser.close();
server.close();
if (problems.length) {
  console.error(
    `Accesibilidad: ${problems.length} problema(s) graves:\n  ${problems.join('\n  ')}`,
  );
  process.exit(1);
}
console.log(
  `Accesibilidad: ${audited.length} vistas sin problemas graves (axe-core, WCAG 2.1 AA):\n  ${audited.join('\n  ')}`,
);

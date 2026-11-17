// Vídeos cortos de la interfaz real en modo demo (datos sintéticos) para la web:
//   media/clips/<clip>.mp4 (1600×1000, H.264, sin audio, en bucle) y media/clips/<clip>.webp (póster)
//
// Graba la pantalla con el screencast de Chromium (fotogramas JPEG con su marca de tiempo, más
// nítidos que el vídeo de Playwright) mientras un cursor dibujado en la página mueve, pulsa y
// teclea como lo haría una persona. Luego ffmpeg monta los fotogramas respetando sus tiempos.
//
// Uso: node tools/media/clips.mjs [--no-build] [clip…]
// Requisitos: los de screens.mjs y ffmpeg.

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, resolve } from 'node:path';
import { abrir } from '../navegador.mjs';

const root = resolve(import.meta.dirname, '../..');
const dist = join(root, 'apps/desktop/dist');
const out = join(root, 'media/clips');
// Copia para el README del repositorio (la carpeta `web/` no se publica).
const repo = join(root, 'media/clips');
const tmp = join(root, 'target/clips');
mkdirSync(out, { recursive: true });
mkdirSync(repo, { recursive: true });

const args = process.argv.slice(2);
if (!args.includes('--no-build')) {
  execFileSync('npx', ['vite', 'build'], { cwd: join(root, 'apps/desktop'), stdio: 'inherit' });
}
const only = args.filter((a) => !a.startsWith('--'));
if (!existsSync(join(dist, 'index.html'))) throw new Error('falta apps/desktop/dist');

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

const inter = readFileSync(
  join(root, 'node_modules/@fontsource-variable/inter/files/inter-latin-wght-normal.woff2'),
).toString('base64');
// Fuente, cursor dibujado (el del sistema no sale en el screencast) y un círculo al pulsar.
const css = `@font-face{font-family:InterV;src:url(data:font/woff2;base64,${inter}) format('woff2');font-weight:100 900}
*{font-family:InterV,system-ui,sans-serif!important;-webkit-font-smoothing:antialiased}
main{min-height:100vh}
#fc{position:fixed;left:0;top:0;width:28px;height:28px;z-index:2147483647;pointer-events:none;
 transform:translate(640px,520px);filter:drop-shadow(0 2px 3px rgb(0 0 0/35%))}
#fc i{position:absolute;left:-14px;top:-14px;width:28px;height:28px;border-radius:50%;
 background:rgb(15 92 74/35%);transform:scale(0);opacity:0}
#fc.down i{animation:fcp .45s ease-out}
@keyframes fcp{from{transform:scale(.3);opacity:1}to{transform:scale(1.6);opacity:0}}`;
const cursor = `(() => {
  const c = document.createElement('div');
  c.id = 'fc';
  c.innerHTML = '<i></i><svg width="28" height="28" viewBox="0 0 22 22"><path d="M3 2l15 9-6.5 1.4L8.6 19z" fill="#111" stroke="#fff" stroke-width="1.5" stroke-linejoin="round"/></svg>';
  document.body.append(c);
  addEventListener('mousemove', (e) => { c.style.transform = 'translate(' + (e.clientX - 3) + 'px,' + (e.clientY - 2) + 'px)'; }, true);
  addEventListener('mousedown', () => { c.classList.remove('down'); void c.offsetWidth; c.classList.add('down'); }, true);
})()`;

const browser = await abrir({
  headless: true,
  args: ['--lang=es-ES'],
  env: { ...process.env, LANG: 'es_ES.UTF-8', LANGUAGE: 'es_ES' },
});

/** Póster en WebP: con ffmpeg si trae libwebp; si no, PNG con ffmpeg y WebP con Chromium. */
async function poster(frame, height, file) {
  const vf = `scale=2560:1600,crop=2560:${height * 2}:0:0,scale=1600:-2:flags=lanczos`;
  const ff = (...a) =>
    execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-i', frame, '-vf', vf, ...a]);
  try {
    ff('-c:v', 'libwebp', '-quality', '78', file);
    return;
  } catch {
    // Sin libwebp: lo codifica Chromium.
  }
  const png = file.replace(/\.webp$/, '.tmp.png');
  ff(png);
  const page = await browser.newPage();
  const webp = await page.evaluate(
    async (src) => {
      const img = new Image();
      img.src = src;
      await img.decode();
      const c = document.createElement('canvas');
      c.width = img.naturalWidth;
      c.height = img.naturalHeight;
      c.getContext('2d').drawImage(img, 0, 0);
      return c.toDataURL('image/webp', 0.78);
    },
    `data:image/png;base64,${readFileSync(png).toString('base64')}`,
  );
  await page.close();
  rmSync(png);
  if (!webp.startsWith('data:image/webp')) throw new Error('Chromium no ha sabido hacer el WebP');
  writeFileSync(file, Buffer.from(webp.split(',')[1], 'base64'));
}

/** Un clip: abre la demo, prepara la pantalla con `setup` (sin grabar), ejecuta `script`
 * grabando y monta el mp4. `height` recorta la ventana (en px CSS) si la pantalla es corta. */
async function clip(name, setup, script, { height = 800 } = {}) {
  if (only.length && !only.includes(name)) return;
  const page = await browser.newPage({
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 2,
    locale: 'es-ES',
  });
  page.on('pageerror', (e) => console.error('pageerror', e.message));
  await page.goto(base);
  await page.addStyleTag({ content: css });
  await page.evaluate(cursor);
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(400);
  let pos = { x: 640, y: 520 };
  await page.mouse.move(pos.x, pos.y);

  // Movimiento suave (ease-in-out) hasta el centro del elemento.
  const to = async (locator) => {
    await locator.scrollIntoViewIfNeeded();
    const b = await locator.boundingBox();
    const target = { x: b.x + Math.min(b.width / 2, 60), y: b.y + b.height / 2 };
    const n = 28;
    for (let i = 1; i <= n; i++) {
      const t = i / n;
      const e = t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2;
      await page.mouse.move(pos.x + (target.x - pos.x) * e, pos.y + (target.y - pos.y) * e);
      await page.waitForTimeout(14);
    }
    pos = target;
  };
  const ui = {
    page,
    wait: (ms) => page.waitForTimeout(ms),
    to,
    click: async (locator) => {
      await to(locator);
      await page.waitForTimeout(120);
      await page.mouse.down();
      await page.waitForTimeout(70);
      await page.mouse.up();
      await page.waitForTimeout(350);
    },
    type: async (locator, text) => {
      await ui.click(locator);
      await locator.pressSequentially(text, { delay: 55 });
      await page.waitForTimeout(250);
    },
    tab: (name) =>
      ui.click(page.getByRole('navigation', { name: 'Secciones' }).getByRole('button', { name })),
  };

  await setup(ui);
  await page.mouse.move(pos.x, pos.y);
  const dir = join(tmp, name);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const frames = [];
  const cdp = await page.context().newCDPSession(page);
  cdp.on('Page.screencastFrame', async ({ data, metadata, sessionId }) => {
    const file = join(dir, `f${String(frames.length).padStart(5, '0')}.jpg`);
    writeFileSync(file, Buffer.from(data, 'base64'));
    frames.push({ file, t: metadata.timestamp });
    await cdp.send('Page.screencastFrameAck', { sessionId }).catch(() => {});
  });
  await cdp.send('Page.startScreencast', {
    format: 'jpeg',
    quality: 92,
    maxWidth: 2560,
    maxHeight: 1600,
  });
  await page.waitForTimeout(300);
  await script(ui);
  const end = Date.now() / 1000;
  await cdp.send('Page.stopScreencast');
  await page.close();

  // Lista para el demuxer concat: cada fotograma dura hasta el siguiente.
  const list = frames
    .map((f, i) => {
      const next = i + 1 < frames.length ? frames[i + 1].t : Math.max(end, f.t + 0.5);
      return `file '${f.file}'\nduration ${Math.max(next - f.t, 0.001).toFixed(4)}`;
    })
    .join('\n');
  writeFileSync(join(dir, 'list.txt'), `${list}\nfile '${frames.at(-1).file}'\n`);
  const mp4 = join(out, `${name}.mp4`);
  execFileSync('ffmpeg', [
    '-y',
    '-loglevel',
    'error',
    '-f',
    'concat',
    '-safe',
    '0',
    '-i',
    join(dir, 'list.txt'),
    '-vf',
    `fps=30,scale=2560:1600:flags=lanczos,crop=2560:${height * 2}:0:0,scale=1600:-2:flags=lanczos,format=yuv420p`,
    '-c:v',
    'libx264',
    '-preset',
    'veryslow',
    '-crf',
    '27',
    '-tune',
    'stillimage',
    '-movflags',
    '+faststart',
    '-an',
    mp4,
  ]);
  // Póster: el primer fotograma (lo que se ve antes de reproducir o con «reducir movimiento»).
  await poster(frames[0].file, height, join(out, `${name}.webp`));
  for (const ext of ['mp4', 'webp'])
    copyFileSync(join(out, `${name}.${ext}`), join(repo, `${name}.${ext}`));
  const kb = Math.round(readFileSync(mp4).length / 1024);
  console.log(`  ${name}.mp4  ${frames.length} fotogramas, ${kb} KB`);
}

// Recorrido por la app: panel, calendario, facturas y contabilidad.
await clip(
  'recorrido',
  async (ui) => {
    await ui.page.getByText('Tu negocio en 2026').waitFor();
  },
  async (ui) => {
    await ui.wait(1200);
    await ui.tab('Calendario');
    await ui.page.getByText('Faltan 7 días').first().waitFor();
    await ui.wait(1600);
    await ui.tab('Facturas');
    await ui.wait(1500);
    await ui.tab('Contabilidad');
    await ui.wait(1500);
    await ui.click(ui.page.getByRole('button', { name: 'Cobros y pagos' }));
    await ui.wait(1600);
    await ui.tab('Modelos');
    await ui.page.getByText('Cuota al 21 %').waitFor();
    await ui.wait(1800);
    await ui.tab('Panel');
    await ui.wait(800);
  },
);

// Emitir una factura: datos del cliente, una línea y la confirmación con el desglose.
await clip(
  'factura',
  (ui) => ui.tab('Facturas'),
  async (ui) => {
    const p = ui.page;
    await ui.wait(700);
    await ui.click(p.getByRole('button', { name: 'Nueva factura' }));
    await ui.wait(400);
    await ui.type(p.getByLabel('NIF del cliente (vacío = simplificada)'), 'B12345674');
    await ui.type(p.getByLabel('Nombre o razón social').last(), 'Estudio Norte Arquitectura SL');
    await ui.type(p.getByLabel('Concepto 1'), 'Diseño y desarrollo de la web corporativa');
    await ui.type(p.getByLabel('Precio 1'), '1800');
    await ui.wait(500);
    await ui.click(p.getByRole('button', { name: 'Emitir factura…' }));
    await p.getByRole('dialog', { name: 'Confirmar emisión' }).waitFor();
    await p.getByRole('dialog').scrollIntoViewIfNeeded();
    await ui.wait(2600);
  },
);

// Factura recibida: abrir la que ha llegado y revisar lo que se ha leído del PDF.
await clip(
  'recibida',
  (ui) => ui.tab('Recibidas'),
  async (ui) => {
    const p = ui.page;
    await ui.wait(900);
    await ui.click(p.getByText('Coworking Ruzafa SL').first());
    await p.getByRole('button', { name: 'Confirmar datos' }).waitFor();
    await ui.wait(1000);
    for (const label of ['NIF proveedor', 'Base', 'IVA', 'Retención']) {
      const field = p.getByLabel(label, { exact: true }).first();
      if (await field.count()) {
        await ui.to(field);
        await ui.wait(700);
      }
    }
    await ui.to(p.getByRole('button', { name: 'Confirmar datos' }));
    await ui.wait(1800);
  },
  { height: 640 },
);

// Banco: un movimiento del extracto y la factura que le corresponde.
await clip(
  'banco',
  async (ui) => {
    await ui.tab('Contabilidad');
    await ui.click(ui.page.getByRole('button', { name: 'Banco' }));
  },
  async (ui) => {
    const p = ui.page;
    await ui.wait(1200);
    await ui.click(p.getByText(/TRANSFERENCIA ESTUDIO NORTE/));
    await p.getByText(/Cobro de A2026-0006/).waitFor();
    await ui.wait(800);
    await ui.to(p.getByText(/Cobro de A2026-0006/).first());
    await ui.wait(2400);
  },
  { height: 520 },
);

// Cobros y pagos: pendientes, vencidas y registrar un cobro.
await clip(
  'cartera',
  async (ui) => {
    await ui.tab('Contabilidad');
    await ui.click(ui.page.getByRole('button', { name: 'Cobros y pagos' }));
  },
  async (ui) => {
    const p = ui.page;
    await ui.wait(1300);
    const show = p.getByLabel('Mostrar');
    await ui.to(show);
    await show.selectOption('vencidas');
    await ui.wait(1500);
    await show.selectOption('todas');
    await ui.wait(1300);
    await ui.click(p.getByRole('button', { name: 'Registrar cobro…' }).last());
    await ui.wait(600);
    await ui.to(p.getByRole('button', { name: 'Registrar cobro', exact: true }));
    await ui.wait(2000);
  },
  { height: 620 },
);

// Ahorrar impuestos: lo que te conviene, con tus cifras, y los pasos para conseguirlo.
await clip(
  'ahorro',
  async (ui) => {
    await ui.tab('Ahorrar impuestos');
    await ui.page.getByRole('button', { name: /^Todas \(/ }).waitFor();
  },
  async (ui) => {
    const p = ui.page;
    await ui.wait(1500);
    await ui.click(p.getByText('Qué es y cómo conseguirlo').first());
    await ui.wait(1800);
    await p.mouse.wheel(0, 420);
    await ui.wait(2000);
  },
);

// Banco conectado en solo lectura: bajar los movimientos de BBVA y Revolut.
await clip(
  'bancos',
  async (ui) => {
    await ui.tab('Contabilidad');
    await ui.click(ui.page.getByRole('button', { name: 'Banco' }));
    await ui.page
      .getByRole('heading', { name: 'Conexión con el banco (solo lectura)' })
      .scrollIntoViewIfNeeded();
  },
  async (ui) => {
    const p = ui.page;
    await ui.wait(1300);
    await ui.click(p.getByRole('button', { name: 'Actualizar movimientos' }));
    await p.getByText(/3 movimientos nuevos/).waitFor();
    await ui.wait(1600);
    await ui.click(p.getByRole('button', { name: 'Conectar un banco…' }));
    await ui.wait(1800);
  },
);

// Evolución: cómo van las cifras de los últimos años y qué conviene revisar.
await clip(
  'evolucion',
  (ui) => ui.tab('Evolución'),
  async (ui) => {
    await ui.wait(1600);
    await ui.page.mouse.wheel(0, 500);
    await ui.wait(2000);
  },
);

// Criptomonedas: posiciones, ganancias por FIFO y el 721.
await clip(
  'cripto',
  (ui) => ui.tab('Criptomonedas'),
  async (ui) => {
    await ui.wait(1600);
    await ui.page.mouse.wheel(0, 500);
    await ui.wait(2000);
  },
);

// Revisión de la carpeta de la empresa: errores, documentación que falta y qué pedir.
await clip(
  'revision',
  async (ui) => {
    await ui.tab('Importar');
    await ui.click(ui.page.getByRole('button', { name: 'Revisión', exact: true }));
  },
  async (ui) => {
    const p = ui.page;
    await ui.wait(900);
    await ui.click(p.getByRole('button', { name: 'Revisar la carpeta', exact: true }));
    await p.getByRole('heading', { name: 'Lo que se ha encontrado' }).waitFor();
    await ui.wait(1500);
    await p.mouse.wheel(0, 450);
    await ui.wait(2000);
  },
);

await browser.close();
server.close();
console.log(`Vídeos en ${out}`);

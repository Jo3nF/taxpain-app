// Vídeos cortos de lanzamiento (estilo X): texto animado y capturas reales del modo demo.
//
// Uso: node tools/media/lanzamiento.mjs [lanzamiento|gestoria|seguridad|funciones|pasos|claude|cripto…]
// (sin nombres, todos)
// Necesita las capturas de `media/screens` (node tools/media/screens.mjs) y ffmpeg.
// Salida: media/lanzamiento/<vídeo>.mp4 (1920×1080, 30 fps, H.264, con música de fondo) y <vídeo>.jpg
// (póster). La música la sintetiza `musica.py` (python3 con numpy); sin ellos, el vídeo queda mudo.
//
// La escena (`lanzamiento/escena.html`) declara todas las animaciones con tiempos absolutos y las
// deja en pausa; aquí se fija cada instante y se hace una captura, así que el resultado no depende
// de la velocidad del ordenador.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join, resolve } from 'node:path';
import { abrir } from '../navegador.mjs';

const root = resolve(import.meta.dirname, '../..');
const out = join(root, 'media/lanzamiento');
mkdirSync(out, { recursive: true });
const FPS = 30;
const pedidos = process.argv.slice(2).filter((a) => !a.startsWith('--'));
// Para repasar un vídeo sin montarlo: --muestras=<carpeta> deja un fotograma cada --cada=<s>
// segundos (1 por defecto) en esa carpeta, o sólo los instantes de --en=<s,s,…>, y no escribe
// el .mp4.
const opcion = (n) =>
  process.argv
    .slice(2)
    .find((a) => a.startsWith(`--${n}=`))
    ?.split('=')[1];
const muestras = opcion('muestras');
const cada = Number(opcion('cada') ?? 1);
const en = opcion('en')?.split(',').map(Number);
// --repo=<dirección> la escribe en el cierre de los vídeos que lo llevan (p. ej. github.com/…).
const repo = opcion('repo');
// --ritmo=<n> es la velocidad a la que corre el guion de la escena: 1 es la del guion y, por
// defecto, 0.85 (un 18 % más de tiempo en cada plano, para que dé tiempo a leer). El vídeo dura
// lo que dice la escena dividido entre el ritmo.
const ritmo = Number(opcion('ritmo') ?? 0.85);
if (!(ritmo >= 0.5 && ritmo <= 1.5)) throw new Error('--ritmo tiene que estar entre 0.5 y 1.5');
// --musica=<tranquila|producto|no> elige la pista de fondo (tranquila por defecto). Con --sonorizar
// no se monta nada: sólo se cambia la música de los .mp4 que ya hay.
const musica = opcion('musica') ?? 'tranquila';
if (!['tranquila', 'producto', 'no'].includes(musica))
  throw new Error('--musica: tranquila, producto o no');
const soloSonorizar = process.argv.includes('--sonorizar');

// Pone (o cambia) la música de un .mp4 sin tocar la imagen. Si falta python3 o numpy, lo deja mudo.
function sonorizar(mp4) {
  if (musica === 'no') return;
  const dir = mkdtempSync(join(tmpdir(), 'taxpain-musica-'));
  try {
    const seg = execFileSync('ffprobe', [
      '-v',
      'error',
      '-select_streams',
      'v:0',
      '-show_entries',
      'format=duration',
      '-of',
      'csv=p=0',
      mp4,
    ])
      .toString()
      .trim();
    const wav = join(dir, 'pista.wav');
    execFileSync('python3', ['-I', join(import.meta.dirname, 'musica.py'), musica, seg, wav]);
    const con = join(dir, 'con.mp4');
    execFileSync('ffmpeg', [
      '-y',
      '-loglevel',
      'error',
      '-i',
      mp4,
      '-i',
      wav,
      '-map',
      '0:v',
      '-map',
      '1:a',
      '-c:v',
      'copy',
      '-af',
      'loudnorm=I=-19:TP=-2:LRA=9',
      '-c:a',
      'aac',
      '-b:a',
      '160k',
      '-shortest',
      '-movflags',
      '+faststart',
      con,
    ]);
    renameSync(con, mp4);
  } catch (e) {
    console.warn(`  sin música (${String(e.message).split('\n')[0]})`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
const videos = pedidos.length
  ? pedidos
  : ['lanzamiento', 'gestoria', 'seguridad', 'funciones', 'pasos', 'claude', 'cripto'];
if (soloSonorizar) {
  for (const nombre of videos) {
    const mp4 = join(out, `${nombre}.mp4`);
    if (!existsSync(mp4)) throw new Error(`No existe ${mp4}`);
    sonorizar(mp4);
    console.log(
      `  ${nombre}.mp4  música «${musica}», ${Math.round(readFileSync(mp4).length / 1024)} KB`,
    );
  }
  process.exit(0);
}

const tipos = { '.html': 'text/html; charset=utf-8', '.png': 'image/png' };
const permitido = [join(root, 'media/screens'), join(root, 'tools/media/lanzamiento')];
const server = createServer((req, res) => {
  const url = new URL(req.url ?? '/', 'http://localhost');
  const ruta =
    url.pathname === '/'
      ? '/tools/media/lanzamiento/escena.html'
      : decodeURIComponent(url.pathname);
  const fichero = join(root, ruta);
  if (!permitido.some((p) => fichero.startsWith(`${p}/`)) || !existsSync(fichero)) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { 'content-type': tipos[extname(fichero)] ?? 'application/octet-stream' });
  res.end(readFileSync(fichero));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}/`;

const inter = readFileSync(
  join(root, 'node_modules/@fontsource-variable/inter/files/inter-latin-wght-normal.woff2'),
).toString('base64');
const fuente = `@font-face{font-family:InterV;src:url(data:font/woff2;base64,${inter}) format('woff2');font-weight:100 900}`;

const browser = await abrir({
  headless: true,
  args: ['--lang=es-ES', '--force-color-profile=srgb'],
});
const page = await browser.newPage({
  viewport: { width: 1920, height: 1080 },
  deviceScaleFactor: 1,
});
page.on('pageerror', (e) => console.error('pageerror', e.message));

// Fija el instante y espera a que el navegador lo pinte: sin esperar, alguna capa animada puede
// salir en la captura con el estado del fotograma anterior.
const fijar = (t) =>
  page.evaluate(async (s) => {
    window.fijar(s);
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  }, t);

for (const nombre of videos) {
  await page.goto(`${base}?v=${nombre}${repo ? `&repo=${encodeURIComponent(repo)}` : ''}`);
  await page.addStyleTag({ content: fuente });
  await page.evaluate(() => window.LISTO);
  const total = await page.evaluate(() => window.TOTAL);
  const marcos = Math.round((total / ritmo) * FPS);
  if (muestras) {
    mkdirSync(muestras, { recursive: true });
    const instantes = en ?? Array.from({ length: Math.ceil(total / cada) }, (_, i) => i * cada);
    for (const t of instantes) {
      await fijar(t);
      const seg = t.toFixed(1).padStart(5, '0');
      await page.screenshot({ path: join(muestras, `${nombre}-${seg}.jpg`), quality: 70 });
    }
    console.log(`  ${nombre}: muestras cada ${cada} s en ${muestras}`);
    continue;
  }
  const dir = mkdtempSync(join(tmpdir(), `taxpain-${nombre}-`));
  for (let i = 0; i < marcos; i++) {
    await fijar((i / FPS) * ritmo);
    await page.screenshot({ path: join(dir, `${String(i).padStart(5, '0')}.png`) });
  }
  const mp4 = join(out, `${nombre}.mp4`);
  execFileSync('ffmpeg', [
    '-y',
    '-loglevel',
    'error',
    '-framerate',
    String(FPS),
    '-i',
    join(dir, '%05d.png'),
    '-c:v',
    'libx264',
    '-preset',
    'slow',
    '-crf',
    // Los vídeos largos, algo más comprimidos, para que no pasen de unos 15 MB.
    total > 70 ? '26' : total > 45 ? '24' : '18',
    '-pix_fmt',
    'yuv420p',
    '-movflags',
    '+faststart',
    '-an',
    mp4,
  ]);
  // Póster: el fotograma del cierre.
  execFileSync('ffmpeg', [
    '-y',
    '-loglevel',
    'error',
    '-i',
    join(dir, `${String(marcos - 1).padStart(5, '0')}.png`),
    '-vf',
    'scale=960:-2',
    '-q:v',
    '3',
    join(out, `${nombre}.jpg`),
  ]);
  rmSync(dir, { recursive: true, force: true });
  sonorizar(mp4);
  console.log(
    `  ${nombre}.mp4  ${(marcos / FPS).toFixed(1)} s, ${Math.round(readFileSync(mp4).length / 1024)} KB`,
  );
}

await browser.close();
server.close();

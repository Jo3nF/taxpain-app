// Vídeo de presentación (≈40 s, 1920×1080, H.264, sin audio) para X: media/taxpain-presentacion.mp4
// (y su copia para la web, web/video/presentacion.mp4, con el póster .webp)
//
// 1) Renderiza cada escena de templates/video.html a PNG con el Chromium de Playwright.
// 2) ffmpeg: zoom lento en cada escena y fundidos encadenados (xfade).
// Requisitos: `node tools/media/screens.mjs` antes (usa media/screens/*.png) y ffmpeg.

import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { abrir } from '../navegador.mjs';

const root = resolve(import.meta.dirname, '../..');
const frames = join(root, 'media/video');
mkdirSync(frames, { recursive: true });

// Duración de cada escena en segundos (incluye el fundido de salida).
const scenes = [
  ['s1', 3.5],
  ['s2', 4.5],
  ['s3', 5.5],
  ['s4', 5.5],
  ['s5', 5],
  ['s6', 5],
  ['s7', 5.5],
  ['s8', 5],
];
const FADE = 0.6;
const FPS = 30;

const inter = readFileSync(
  join(root, 'node_modules/@fontsource-variable/inter/files/inter-latin-wght-normal.woff2'),
).toString('base64');
const browser = await abrir({
  headless: true,
  args: ['--lang=es-ES', '--allow-file-access-from-files'],
});
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
const url = pathToFileURL(join(import.meta.dirname, 'templates/video.html')).href;
for (const [id] of scenes) {
  await page.goto(`${url}#${id}`);
  await page.reload();
  await page.addStyleTag({
    content: `@font-face{font-family:InterV;src:url(data:font/woff2;base64,${inter}) format('woff2');font-weight:100 900}`,
  });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(150);
  await page.screenshot({ path: join(frames, `${id}.png`) });
  console.log(`  escena ${id}`);
}
await browser.close();

// ffmpeg: cada escena con un zoom suave (sobre una versión 2x para evitar temblores) y fundidos.
const args = ['-y', '-loglevel', 'error'];
for (const [id, d] of scenes)
  args.push(
    '-loop',
    '1',
    '-t',
    String(d),
    '-framerate',
    String(FPS),
    '-i',
    join(frames, `${id}.png`),
  );
const filters = [];
scenes.forEach(([, d], i) => {
  const n = Math.round(d * FPS);
  filters.push(
    `[${i}:v]scale=3840:2160,zoompan=z='min(1+0.03*on/${n},1.03)':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=1:s=1920x1080:fps=${FPS},format=yuv420p,setsar=1[v${i}]`,
  );
});
let last = 'v0';
let offset = scenes[0][1] - FADE;
for (let i = 1; i < scenes.length; i++) {
  const outLabel = i === scenes.length - 1 ? 'vout' : `x${i}`;
  filters.push(
    `[${last}][v${i}]xfade=transition=fade:duration=${FADE}:offset=${offset.toFixed(2)}[${outLabel}]`,
  );
  last = outLabel;
  offset += scenes[i][1] - FADE;
}
const out = join(root, 'media/taxpain-presentacion.mp4');
args.push(
  '-filter_complex',
  filters.join(';'),
  '-map',
  '[vout]',
  '-c:v',
  'libx264',
  '-preset',
  'slow',
  '-crf',
  '20',
  '-pix_fmt',
  'yuv420p',
  '-movflags',
  '+faststart',
  '-r',
  String(FPS),
  out,
);
execFileSync('ffmpeg', args, { stdio: 'inherit' });
console.log(out);

// Copia para la web (1280×720, más ligera) y su póster: el título, hacia los 2 s.
const web = join(root, 'web/video/presentacion.mp4');
execFileSync('ffmpeg', [
  '-y',
  '-loglevel',
  'error',
  '-i',
  out,
  '-vf',
  'scale=1280:720:flags=lanczos',
  '-c:v',
  'libx264',
  '-preset',
  'slow',
  '-crf',
  '27',
  '-pix_fmt',
  'yuv420p',
  '-movflags',
  '+faststart',
  '-an',
  web,
]);
console.log(web);
const poster = join(root, 'web/video/presentacion.webp');
execFileSync('ffmpeg', [
  '-y',
  '-loglevel',
  'error',
  '-ss',
  '2',
  '-i',
  out,
  '-frames:v',
  '1',
  '-vf',
  'scale=1280:720:flags=lanczos',
  '-c:v',
  'libwebp',
  '-quality',
  '80',
  poster,
]);
console.log(poster);

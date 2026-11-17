# Material gráfico

Todo se genera con código a partir de la **interfaz real** en modo demo (datos sintéticos):

| Fichero | Cómo se genera |
|---|---|
| `screens/*.png` | `node tools/media/screens.mjs` (construye el frontend y captura con Playwright; con `--solo=24,25…` sólo escribe esas capturas) |
| `taxpain-presentacion.mp4` | `node tools/media/video.mjs` (escenas HTML → PNG → ffmpeg; 1920×1080, H.264, ≈35 s, sin audio) |
| `lanzamiento/*.mp4` y `.jpg` | `node tools/media/lanzamiento.mjs [vídeo…]` (texto animado y capturas de `screens/`; 1920×1080, 30 fps, H.264, con música de fondo sintetizada por `tools/media/musica.py`, que pide python3 con numpy). Vídeos: `lanzamiento`, `gestoria`, `seguridad`, `funciones` (tour de funciones, 10 escenas, ≈90 s), `pasos` (cómo funciona, en 9 pasos, ≈75 s), `claude` (Claude por MCP) y `cripto` (cripto e inversiones). Con `--muestras=<carpeta>` deja fotogramas sueltos para repasarlos sin montar el vídeo. Con `--repo=<dirección>` el cierre de los cuatro últimos lleva esa dirección; sin ella, sólo «GitHub · taxpain-app». Con `--ritmo=<n>` cambia la velocidad (por defecto 0.85: un 18 % más lento que el guion de la escena). Con `--musica=<tranquila|producto|no>` se elige la pista (tranquila por defecto), y con `--sonorizar` se cambia la de los `.mp4` que ya hay sin volver a montarlos. |
| `../web/img/og.png` | `node tools/media/og.mjs` |
| `../web/video/*.mp4` | `node tools/media/clips.mjs` (graba la app en modo demo con el screencast de Chromium y un cursor dibujado; 1600 px, H.264, sin audio) |

Requisitos: `npm ci`, el Chromium «headless shell» de Playwright 1.63 en la caché
(`npx playwright-core install chromium-headless-shell` si falta) y `ffmpeg`.

## Propuesta de texto para X

> Hacer los impuestos de autónomo o de una pyme en España no debería doler.
>
> TaxPain: tu gestoría en tu Mac 🇪🇸
> 📅 Calendario fiscal que no se equivoca de día (contrastado con las 99 fechas oficiales 2026)
> 🧾 Facturas con VERI*FACTU
> 📒 Contabilidad que cuadra sola
> 🔐 Tu certificado digital nunca sale de tu ordenador
>
> Código abierto. Lista de espera 👉 taxpain.es

Notas: el vídeo es silencioso (X lo reproduce sin sonido por defecto); si se añade música, que
sea con licencia que permita su uso comercial.

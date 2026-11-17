# Prueba de la app real con WebDriver (Linux)

Sirve para manejar TaxPain de verdad (la app Tauri, no el modo demo) desde un guion, como lo haría
una persona. Sólo funciona en **Linux** (WebKitGTK tiene WebDriver; el WKWebView de macOS no), así
que se usa en una sesión en la nube o en un Linux, no en el Mac.

Se usó el 2026-10-26 para revisar `feature/lotes` antes de fusionarla (ver PROGRESS.md, «Prueba
real de la app»).

## Preparar (Ubuntu/Debian, como root)
```bash
apt-get install -y libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
  librsvg2-dev libayatana-appindicator3-dev xvfb webkit2gtk-driver xdotool poppler-utils zbar-tools
cargo install --locked tauri-driver
npm ci && (cd apps/desktop && npx vite build)
cargo build -p taxpain-desktop --features tauri/custom-protocol   # binario: target/debug/taxpain-desktop
```
En Linux la clave de la base de datos va en un fichero (`dev-keys/db-master.key`) en lugar del
Llavero: sirve para probar la recuperación borrándolo.

## Usar
- `tools/e2e/lanzar.sh N [limpiar]`: pantalla virtual `:1N` y `tauri-driver` en el puerto `44N0`,
  con los datos de la app en `$E2E_DIR/areaN/home` (por defecto `/tmp/taxpain-e2e`). Varios N a la
  vez = varias pruebas en paralelo.
- `tools/e2e/wd.mjs`: cliente WebDriver mínimo (`Session.start(binario)`, `waitText`,
  `clickText`, `inputs`, `type`, `exec`, `bodyText`, `shot`, `quit`). Guiones con
  `WD_PORT=44N0 SHOTS=carpeta node guion.mjs <binario>`.
- Ventanas nativas (elegir fichero, diálogos): `DISPLAY=:1N xdotool …` y capturas con
  `import -display :1N -window root foto.png`.
- PDF: `pdftotext`; QR: `pdftoppm -png` + `zbarimg`.
- Fecha simulada para registrar en otro trimestre: `faketime` al lanzar la app.

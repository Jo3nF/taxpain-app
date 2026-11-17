#!/usr/bin/env bash
# CI local de TaxPain: formato, lint y tests de Rust y del frontend.
# Uso: ./scripts/ci.sh            (todo)
#      ./scripts/ci.sh rust|web   (sólo una parte)
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

part="${1:-all}"

# WebKitGTK descargado con scripts/webkit-sysroot.sh (sin instalar): sólo para comprobar la app.
if [[ -n "${TAXPAIN_WEBKIT_SYSROOT:-}" ]]; then
  r="$TAXPAIN_WEBKIT_SYSROOT/root"
  export PKG_CONFIG_PATH="$r/usr/lib/x86_64-linux-gnu/pkgconfig:$r/usr/share/pkgconfig:/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
fi

step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }

if [[ "$part" == all || "$part" == rust ]]; then
  step "cargo fmt --check"
  cargo fmt --all -- --check
  step "cargo clippy (default-members, -D warnings)"
  cargo clippy --all-targets -- -D warnings
  step "cargo test"
  cargo test
  # Vulnerabilidades conocidas (RustSec) en las dependencias; `--deny warnings` también para
  # crates sin mantenimiento o retirados. El CI de GitHub lo instala; en local es opcional.
  if command -v cargo-audit >/dev/null 2>&1; then
    step "cargo audit (--deny warnings)"
    cargo audit --deny warnings
  else
    echo "(omitido: cargo install cargo-audit --locked para buscar vulnerabilidades conocidas)"
  fi
  if rustup target list --installed 2>/dev/null | grep -q aarch64-apple-darwin; then
    step "cargo check taxpain-macos (aarch64-apple-darwin)"
    cargo check -p taxpain-macos --target aarch64-apple-darwin
  else
    echo "(omitido: rustup target add aarch64-apple-darwin para comprobar el crate macOS)"
  fi
  if [[ "$(uname)" == Darwin ]]; then
    # generate_context!() incrusta el frontend: hay que construirlo antes.
    [[ -d node_modules ]] || npm ci --no-audit --no-fund
    step "vite build (frontend para la app)"
    (cd apps/desktop && npx vite build)
    step "cargo build taxpain-desktop (sólo macOS)"
    cargo build -p taxpain-desktop
    step "cargo clippy taxpain-desktop (macOS)"
    cargo clippy -p taxpain-desktop --all-targets -- -D warnings
  elif pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
    # En Linux con WebKitGTK (CI de GitHub): comprueba la app Tauri sin gastar minutos de macOS.
    [[ -d node_modules ]] || npm ci --no-audit --no-fund
    step "vite build (frontend para la app)"
    (cd apps/desktop && npx vite build)
    step "cargo clippy taxpain-desktop (Linux, WebKitGTK)"
    cargo clippy -p taxpain-desktop --all-targets -- -D warnings
  else
    echo "(omitido: la app Tauri necesita macOS o WebKitGTK — libwebkit2gtk-4.1-dev)"
  fi
fi

if [[ "$part" == all || "$part" == web ]]; then
  [[ -d node_modules ]] || npm ci --no-audit --no-fund
  step "biome check"
  npx biome check .
  step "tsc"
  npm run typecheck
  step "vitest"
  npm test
  step "licencias de las dependencias"
  node --test tools/licenses/*.test.mjs tools/boe/*.test.mjs tools/release/*.test.mjs
  node tools/licenses/check.mjs
  step "diseños de registro de la AEAT (herramienta y novedades en la Sede)"
  python3 -I -m unittest discover -s tools/aeat -p 'test_*.py'
  # Necesita red: sin ella sólo avisa (código 3). Con diseños nuevos en la Sede, falla: hay que
  # bajarlos y revisarlos (docs/RELEASE.md) para que TaxPain lea las declaraciones de ese año.
  set +e
  python3 -I tools/aeat/dr_actualizar.py --comprobar
  dr=$?
  set -e
  if [[ $dr -eq 1 ]]; then
    echo "Hay diseños de registro nuevos: python3 -I tools/aeat/dr_actualizar.py (docs/RELEASE.md)"
    exit 1
  elif [[ $dr -ne 0 && $dr -ne 3 ]]; then
    exit "$dr"
  fi
  step "npm audit (dependencias de producción, nivel alto o crítico)"
  # Necesita red (consulta el registro de npm); con el nivel alto no falla por avisos menores.
  npm audit --omit=dev --audit-level=high
  step "accesibilidad (axe-core sobre el modo demo, en claro y en oscuro)"
  # Con el Chromium de Playwright o, si no está, con el Google Chrome del sistema (no se descarga nada).
  if node -e "import('./tools/navegador.mjs').then(async ({ abrir }) => { const b = await abrir({ headless: true }); await b.close(); }).then(() => process.exit(0), () => process.exit(1))" 2>/dev/null; then
    (cd apps/desktop && npx vite build)
    node tools/a11y/check.mjs --no-build
    node tools/a11y/check.mjs --no-build --dark
  else
    echo "(omitido: no hay Chromium de Playwright ni Google Chrome)"
  fi
fi

step "CI OK"

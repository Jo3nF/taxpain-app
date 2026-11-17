#!/usr/bin/env bash
# Descarga las cabeceras y .pc de WebKitGTK (y sus dependencias -dev) en una carpeta, sin
# instalar nada ni pedir sudo, para que `./scripts/ci.sh` compruebe también la app Tauri en
# Linux (sólo `cargo clippy`, no se enlaza ni se ejecuta). Uso:
#   ./scripts/webkit-sysroot.sh ~/.cache/taxpain-webkit
#   TAXPAIN_WEBKIT_SYSROOT=~/.cache/taxpain-webkit ./scripts/ci.sh
set -euo pipefail
dest="${1:?Uso: scripts/webkit-sysroot.sh <carpeta>}"
mkdir -p "$dest/debs"
cd "$dest/debs"
pkgs=$(apt-cache depends --recurse --no-recommends --no-suggests --no-conflicts --no-breaks \
  --no-replaces --no-enhances libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
  | grep -E '^[a-z0-9]' | grep -- '-dev$' | sort -u)
# shellcheck disable=SC2086
apt-get download $pkgs
for d in ./*.deb; do dpkg-deb -x "$d" "$dest/root"; done
echo "Listo: TAXPAIN_WEBKIT_SYSROOT=$dest ./scripts/ci.sh"

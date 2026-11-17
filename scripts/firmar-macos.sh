#!/usr/bin/env bash
# Firma con el certificado Developer ID un .app (cada ejecutable y el paquete, con hardened
# runtime y sello de tiempo, como `tauri build`) o un DMG. Lo usa el trabajo «firmar» de
# .github/workflows/release.yml (docs/RELEASE.md §2).
#
#   scripts/firmar-macos.sh work/TaxPain.app
#   scripts/firmar-macos.sh out/TaxPain_0.1.0_universal.dmg
#
# Variables: APPLE_CERTIFICATE (el .p12 en base64), APPLE_CERTIFICATE_PASSWORD y
# APPLE_SIGNING_IDENTITY.
#
# El certificado sólo existe mientras corre este script: se importa en un llavero temporal
# **sin poder exportar la clave privada** (`security import -x`), sólo para codesign, y el
# llavero y el .p12 se borran al salir, haya ido bien o mal (trap EXIT). Así ningún paso
# posterior del workflow (npm, brew, la CLI de Tauri) llega a tener la identidad a mano.
set -euo pipefail

target=${1:?uso: firmar-macos.sh <fichero .app o .dmg>}
target=${target%/}
[ -e "$target" ] || { echo "::error::No existe $target" >&2; exit 1; }
: "${APPLE_CERTIFICATE:?falta APPLE_CERTIFICATE}"
: "${APPLE_SIGNING_IDENTITY:?falta APPLE_SIGNING_IDENTITY}"

tmp=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/firmar.XXXXXX")
kc="$tmp/taxpain-firma.keychain-db"
# La lista de llaveros de búsqueda de antes, para dejarla como estaba.
orig=()
while IFS= read -r l; do
  l=${l#"${l%%[![:space:]]*}"}
  l=${l#\"}
  l=${l%\"}
  [ -n "$l" ] && orig+=("$l")
done < <(security list-keychains -d user)
cleanup() {
  security list-keychains -d user -s ${orig[@]+"${orig[@]}"} 2> /dev/null || true
  security delete-keychain "$kc" 2> /dev/null || true
  rm -rf "$tmp"
}
trap cleanup EXIT

kcpass=$(openssl rand -base64 24)
if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::add-mask::$kcpass"; fi
security create-keychain -p "$kcpass" "$kc"
# Se bloquea solo a los 10 minutos (y se borra al salir, que es antes).
security set-keychain-settings -lut 600 "$kc"
security unlock-keychain -p "$kcpass" "$kc"
p12="$tmp/cert.p12"
(umask 077 && printf '%s' "$APPLE_CERTIFICATE" | base64 --decode > "$p12")
# -x: la clave privada no se puede exportar del llavero; -T: sólo codesign la usa sin preguntar.
security import "$p12" -k "$kc" -P "${APPLE_CERTIFICATE_PASSWORD:-}" -x -T /usr/bin/codesign
rm -f "$p12"
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$kcpass" "$kc" > /dev/null
security list-keychains -d user -s "$kc" ${orig[@]+"${orig[@]}"}

sign() { codesign --force --timestamp --keychain "$kc" --sign "$APPLE_SIGNING_IDENTITY" "$@"; }
case "$target" in
  *.app)
    find "$target/Contents/MacOS" -type f -perm -u+x -print0 |
      while IFS= read -r -d '' f; do sign --options runtime "$f"; done
    sign --options runtime "$target"
    codesign --verify --deep --strict --verbose=2 "$target"
    ;;
  *.dmg)
    sign "$target"
    codesign --verify --verbose=2 "$target"
    ;;
  *)
    echo "::error::Sólo sé firmar un .app o un .dmg: $target" >&2
    exit 1
    ;;
esac
echo "Firmado: $(basename "$target")"

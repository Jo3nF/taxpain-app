#!/usr/bin/env bash
# Notariza con Apple un .app o un DMG ya firmados (lo usa el trabajo «firmar» de
# .github/workflows/release.yml; docs/RELEASE.md §2). No grapa: eso lo hace el workflow después.
#
#   scripts/notarizar-macos.sh work/TaxPain.app
#   scripts/notarizar-macos.sh out/TaxPain_0.1.0_universal.dmg
#
# Credenciales por variables de entorno: preferida la clave de API de App Store Connect
# (APPLE_API_KEY, APPLE_API_ISSUER y APPLE_API_PRIVATE_KEY, el texto del .p8); si no, APPLE_ID,
# APPLE_PASSWORD (contraseña de aplicación) y APPLE_TEAM_ID.
#
# - El .p8 va a un fichero temporal que se borra al salir, haya ido bien o mal (trap EXIT).
# - Si notarytool falla (credenciales, red) o Apple no devuelve «Accepted», enseña la salida y,
#   si hay identificador del envío, el registro de Apple (`notarytool log`), y sale con error.
set -euo pipefail

target=${1:?uso: notarizar-macos.sh <fichero .app o .dmg>}
[ -e "$target" ] || { echo "::error::No existe $target" >&2; exit 1; }
tmp=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/notarizar.XXXXXX")
trap 'rm -rf "$tmp"' EXIT

if [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ] && [ -n "${APPLE_API_PRIVATE_KEY:-}" ]; then
  p8="$tmp/AuthKey_${APPLE_API_KEY}.p8"
  (umask 077 && printf '%s' "$APPLE_API_PRIVATE_KEY" > "$p8")
  auth=(--key "$p8" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER")
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  auth=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID")
else
  echo "::error::Faltan las credenciales de notarización (APPLE_API_* o APPLE_ID)" >&2
  exit 1
fi

# notarytool quiere un .zip, .pkg o .dmg: el .app se comprime con ditto (conserva enlaces y
# atributos).
case "$target" in
  *.app | *.app/)
    submit="$tmp/$(basename "${target%/}").zip"
    ditto -c -k --keepParent "${target%/}" "$submit"
    ;;
  *) submit=$target ;;
esac

json="$tmp/notary.json"
rc=0
xcrun notarytool submit "$submit" "${auth[@]}" --wait --output-format json > "$json" || rc=$?
cat "$json"
echo
field() {
  node -e '
    let v;
    try { v = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"))[process.argv[2]]; } catch {}
    process.stdout.write(typeof v === "string" ? v : "");
  ' "$json" "$1"
}
status=$(field status)
id=$(field id)
if [ "$rc" -ne 0 ] || [ "$status" != Accepted ]; then
  if [ -n "$id" ]; then
    xcrun notarytool log "$id" "${auth[@]}" || true
  fi
  echo "::error::Apple no ha aceptado la notarización de $(basename "$target") (notarytool: código $rc, estado: ${status:-desconocido})"
  exit 1
fi
echo "Notarizado: $(basename "$target") (envío $id)"

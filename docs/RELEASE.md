# Publicar una versión de TaxPain (Fase 8)

Todo está preparado para que, cuando haya cuenta de Apple Developer y se decida publicar, sea
cuestión de **añadir secretos y pulsar un botón**. Nada se publica solo: el workflow deja los
ficheros como artefactos privados y, con etiqueta, en una release de GitHub **en borrador**; quien
mantiene el proyecto la publica y decide cuándo anuncia la versión a las apps instaladas (§3.4).

## 1. Qué hay

| Pieza | Dónde | Estado |
|---|---|---|
| Workflow de compilación | `.github/workflows/release.yml` | Manual (Actions → Release → *Run workflow*) o al subir una etiqueta `v*` |
| DMG universal (Apple Silicon + Intel) | artefacto `taxpain-macos-<ref>` y, con etiqueta, release de GitHub en borrador | Sin firmar hasta que haya certificado |
| Firma y notarización | secretos `APPLE_*` del environment `release` (sólo los ve el trabajo «firmar», §2.1) | Se activan solas al crear los secretos |
| Actualizaciones | `tauri-plugin-updater`, `apps/desktop/src-tauri/src/updates.rs`, Ajustes → «Buscar actualizaciones» | Inactivas hasta compilar con `TAXPAIN_UPDATER_PUBKEY` |
| Paquete del actualizador | `TaxPain.app.tar.gz` + `.sig` en la release de GitHub de la etiqueta | Firma verificada con `minisign` en el workflow |
| Manifiesto del actualizador | `scripts/updater-manifest.mjs` → `latest.json` (asset de la release; se copia a la web) | Lo genera el workflow si hay clave |

Las acciones de los workflows van fijadas por hash de commit (Dependabot propone las
actualizaciones, `.github/dependabot.yml`). El workflow tiene tres trabajos (auditoría,
CONSTRUCCION-WEB-PRUEBAS-13):

1. **«Compilar y probar (sin secretos)»**: `npm ci`, `./scripts/ci.sh` y `tauri build` sin
   firmar, con un token de sólo lectura y ningún secreto. Falla si `productor.rs` tiene los datos
   de ejemplo (`B00000000`). Sube el `.app` (en un `.tar.gz`) y el DMG sin firmar como artefacto
   `taxpain-macos-sin-firmar-<ref>` (7 días).
2. **«Firmar, notarizar y firmar el actualizador»** (environment `release`): descarga ese
   artefacto y **no compila ni ejecuta tests ni scripts de npm** (instala la CLI de Tauri con
   `--ignore-scripts`, sólo para `tauri signer sign`). Firma, notariza y grapa el `.app`, rehace
   el DMG y también lo firma, notariza y grapa; después firma el paquete del actualizador,
   comprueba la firma con la clave pública y genera `latest.json`. Cada paso recibe sólo sus
   secretos, y la firma y la notarización terminan antes de instalar la CLI de Tauri y
   `minisign` (§2, punto 3). Deja el artefacto `taxpain-macos-<ref>` (30 días).
3. **«Subir a la release de GitHub (borrador)»**: sólo con etiqueta; es el único con
   `contents: write` y no ve ningún secreto.

### 1.1 Construir el DMG en el propio Mac

La misma orden del workflow, desde `apps/desktop`, sin las rutas del Mac en el ejecutable (los
mensajes de pánico llevan la ruta de cada fuente, y la de `~/.cargo` lleva el usuario del equipo):
```sh
RUSTFLAGS="--remap-path-prefix=$HOME/.cargo=/cargo --remap-path-prefix=$(cd ../.. && pwd)=/src" \
CFLAGS="-ffile-prefix-map=$HOME/.cargo=/cargo -ffile-prefix-map=$(cd ../.. && pwd)=/src" \
  npx tauri build --target universal-apple-darwin
```
y comprueba que no queda ninguna (no tiene que salir nada):
```sh
strings -a ../../target/universal-apple-darwin/release/bundle/macos/TaxPain.app/Contents/MacOS/taxpain-desktop \
  | grep -E '/Users/|/home/'
```
(necesita `rustup target add x86_64-apple-darwin`; sin certificado el `.app` sale con firma *ad
hoc* y Gatekeeper lo rechaza: en el Mac de pruebas se abre con clic derecho → Abrir). El paso
final coloca los iconos de la ventana del DMG controlando el Finder con AppleScript; si se lanza
desde una terminal sin permiso de automatización sobre el Finder, falla con «Tiempo límite
superado (-1712)» después de haber construido el `.app`. En ese caso el DMG se termina a mano,
sin el adorno, desde `target/universal-apple-darwin/release/bundle/macos`:
```sh
../dmg/bundle_dmg.sh --skip-jenkins --volname TaxPain --icon TaxPain.app 180 170 \
  --app-drop-link 480 170 --window-size 660 400 --hide-extension TaxPain.app \
  --volicon ../dmg/icon.icns TaxPain_0.1.0_universal.dmg TaxPain.app
```
(o se concede el permiso en Ajustes del Sistema → Privacidad y seguridad → Automatización y se
repite `tauri build`). Comprobado el 26-10-2026: `.app` universal (arm64 + x86_64), DMG de 27 MB
con suma de comprobación válida.

## 2. Firma y notarización (cuando exista la cuenta de Apple Developer)

1. En developer.apple.com, crea un certificado **Developer ID Application** y expórtalo desde
   Acceso a Llaveros como `.p12` con contraseña.
2. En GitHub, en el environment `release` (*Settings → Environments → release → Environment
   secrets*, §2.1), crea los **secretos**:
   - `APPLE_CERTIFICATE`: el `.p12` en base64 (`base64 -i cert.p12 | pbcopy`).
   - `APPLE_CERTIFICATE_PASSWORD`: su contraseña.
   - `APPLE_SIGNING_IDENTITY`: p. ej. `Developer ID Application: TU EMPRESA (TEAMID)`.
   - Para notarizar, **preferible una clave de API de App Store Connect** (no caduca con la
     contraseña ni pide doble factor): en appstoreconnect.apple.com → *Usuarios y acceso →
     Integraciones → Claves de la API de App Store Connect*, crea una con rol *Developer* y
     descarga el `AuthKey_XXXXXXXXXX.p8` (sólo se puede descargar una vez). Secretos:
     `APPLE_API_KEY` (el ID de la clave, 10 caracteres), `APPLE_API_ISSUER` (el *Issuer ID*) y
     `APPLE_API_PRIVATE_KEY` (el contenido del `.p8`, texto). El workflow lo escribe en un fichero
     temporal y se lo pasa a `xcrun notarytool`.
   - Alternativa: `APPLE_ID`, `APPLE_PASSWORD` (contraseña **de aplicación** de
     appleid.apple.com) y `APPLE_TEAM_ID`.

   Créalos como secretos **del environment `release`** (§2.1), no del repositorio.
3. Lanza el workflow. El trabajo «firmar» firma cada ejecutable y el `.app` con *hardened
   runtime* y sello de tiempo (`codesign --options runtime --timestamp`, lo mismo que haría
   `tauri build`; `scripts/firmar-macos.sh`), notariza con `xcrun notarytool submit --wait`
   (`scripts/notarizar-macos.sh`) y grapa el ticket; después comprueba la firma
   (`codesign --verify --deep --strict`), el ticket (`xcrun stapler validate`) y que Gatekeeper
   la acepta (`spctl --assess`). El DMG se rehace con `hdiutil` a partir del `.app` firmado y
   grapado, y se firma, se notariza y se grapa igual (comprobado con
   `spctl --assess --type open --context context:primary-signature`): sale **sin el fondo ni la
   colocación de iconos** del DMG de `tauri build` (sólo `TaxPain.app` y el enlace a
   Aplicaciones). Sin credenciales de notarización (sólo a mano, sin etiqueta) el `.app` sale
   firmado sin notarizar y el DMG **sin firmar**, con un aviso: un DMG firmado y sin notarizar
   no es mejor para Gatekeeper.

   Cómo se protegen las claves en ese trabajo:
   - **Certificado**: sólo existe mientras corre `scripts/firmar-macos.sh` (una vez para el
     `.app` y otra para el DMG). Se importa en un llavero temporal con contraseña aleatoria,
     **sin poder exportar la clave privada** (`security import -x`) y sólo para `codesign`; el
     llavero se bloquea solo a los 10 minutos y el script lo borra al terminar, haya ido bien o
     mal. Ningún paso que instala o ejecuta código de npm o de brew llega a ver el llavero.
   - **Clave de API (`.p8`)**: `scripts/notarizar-macos.sh` la escribe en una carpeta temporal
     con permisos 600 y la borra al salir. Si `notarytool` falla (credenciales, red) o Apple no
     devuelve `Accepted`, enseña la respuesta y el registro de Apple (`notarytool log`) y para.
   - Un paso final de limpieza (`if: always()`), antes de instalar nada, borra cualquier llavero
     o `.p8` que haya quedado si un paso se ha cortado a medias.

   `TODO(VERIFY)`: en la primera versión firmada, comprobar en el registro del trabajo que
   `security import -x` + `set-key-partition-list` dejan firmar a `codesign` sin preguntar, y
   abrir el DMG descargado en un Mac limpio para ver que Gatekeeper no avisa.
4. Con una **etiqueta `v*`** (versión para distribuir) el workflow **falla** si falta el
   certificado, las credenciales de notarización o la clave del actualizador: así nunca sale una
   versión pública sin firmar. A mano (*Run workflow*) sigue generando un DMG de prueba sin firmar.
5. Apunta la **caducidad** del certificado Developer ID (5 años) y de la clave de API en el
   calendario: renovar = crear el nuevo, sustituir los secretos y lanzar una versión de prueba.

### 2.1 Environment «release» con revisor (configurar una vez en GitHub)

El trabajo «firmar» declara `environment: release`. Así los secretos sólo llegan a ese trabajo
y sólo cuando alguien lo aprueba:

1. GitHub → *Settings → Environments → New environment* → nombre **`release`** (exacto).
2. *Deployment protection rules* → marca **Required reviewers** y añádete (o a quien firme las
   versiones). Si eres el único revisor, deja desmarcado *Prevent self-review*: si no, nadie
   podría aprobar tus propias ejecuciones.
3. *Deployment branches and tags* → **Selected branches and tags** → añade la regla de etiqueta
   `v*` y la rama principal (para los *Run workflow* de prueba). Así una rama cualquiera no
   puede llegar a los secretos.
4. En ese environment, *Environment secrets* → crea los 11 secretos de §2 y §3
   (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`,
   `APPLE_PASSWORD`, `APPLE_TEAM_ID`, `APPLE_API_ISSUER`, `APPLE_API_KEY`,
   `APPLE_API_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`)
   y **borra** los que hubiera como secretos del repositorio (*Settings → Secrets and variables
   → Actions → Repository secrets*). La variable `TAXPAIN_UPDATER_PUBKEY` (no secreta) sigue
   como variable del repositorio: la usa el trabajo que compila.
5. Cada ejecución se para tras «Compilar y probar» con *Review deployments*: revisa que la
   compilación y las comprobaciones han pasado y aprueba. Sin aprobar, nada se firma.

`TODO` con la cuenta: el Keychain de protección de datos (D9) exige el *entitlement*
`keychain-access-groups` con el Team ID. Cuando esté, compila con la variable
`TAXPAIN_KEYCHAIN=data-protection` (añádela al paso «Compilar sin firmar» del workflow) y pruébalo en un Mac;
sin ella la app usa el Keychain clásico (también cifrado por el sistema), que es lo que funciona
en el DMG sin firmar.

## 3. Actualizaciones automáticas

La app sólo instala paquetes firmados con la clave del actualizador (minisign, independiente
del certificado de Apple). La clave pública va **compilada** en la app.

1. Genera el par de claves **en tu Mac** (no en el repositorio):
   `npx tauri signer generate -w ~/.tauri/taxpain-updater.key`
   Guarda la clave privada y su contraseña en tu gestor de contraseñas: si se pierde, las apps
   instaladas no aceptarán más actualizaciones.
2. En GitHub crea (§2.1):
   - secretos del environment `release` `TAURI_SIGNING_PRIVATE_KEY` (contenido del fichero) y
     `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`;
   - **variable** (no secreto) `TAXPAIN_UPDATER_PUBKEY` con la clave pública.
3. El trabajo «firmar» empaqueta el `.app` firmado en `TaxPain.app.tar.gz` (el mismo formato
   que `createUpdaterArtifacts` de Tauri), lo firma con `tauri signer sign --app-version`, genera `.sig` y
   `latest.json`, y **verifica la firma**
   del paquete con la clave pública compilada (`brew install minisign` y
   `minisign -Vm TaxPain.app.tar.gz -x <firma> -P <clave>`; Tauri guarda `.sig` y
   `TAXPAIN_UPDATER_PUBKEY` en base64 del texto minisign, y el workflow los decodifica antes). Si
   no verifica, el workflow falla: una clave pública mal pegada daría una app que nunca aceptaría
   actualizaciones y habría que reinstalarla a mano en cada Mac. Firma con
   `tauri signer sign --app-version X.Y.Z` (la de `tauri.conf.json`) y comprueba que el
   comentario de confianza de la firma lleva `version:X.Y.Z` (el comentario va firmado). La app
   lo exige (`"requireSignedVersion": true` en `plugins.updater` de `tauri.conf.json`): el
   manifiesto no va firmado, y sin esa comprobación uno falso podría anunciar una versión
   inflada con el paquete y la firma de una versión antigua. **Nunca firmes un paquete sin
   `--app-version`**: sin versión firmada, las apps lo rechazan. Probado en local con
   `@tauri-apps/cli` 2.12.2 y una clave de prueba (12/11/2026): `--app-version 0.10.0` deja
   `version:0.10.0` en el comentario de confianza. El registro del paso «Paquete del
   actualizador» anota el tamaño del `.app.tar.gz`: apúntalo aquí en el primer *Run workflow*
   (`TODO`).
4. **Dónde van los ficheros.** Los paquetes del actualizador pesan decenas de MB por versión:
   nunca entran en git ni en Cloudflare Pages (25 MiB por fichero; `.gitignore` excluye
   `web/updates/*.tar.gz*` y `*.dmg`). Con una etiqueta `v*`, el trabajo «Subir a la release de
   GitHub» deja el DMG, `TaxPain.app.tar.gz`, su `.sig` y `latest.json` en la **release de GitHub
   de esa etiqueta, en borrador** (la crea si no existe, con las notas del `CHANGELOG.md`).
   `latest.json` ya apunta a la URL pública del paquete en esa release
   (`https://github.com/<propietario>/taxpain-app/releases/download/vX.Y.Z/TaxPain.app.tar.gz`). Para
   publicar la versión:
   1. Revisa la release en GitHub (Releases → borrador `TaxPain vX.Y.Z`) y **publícala**: hasta
      entonces sus assets no se pueden descargar.
   2. Anuncia la versión a las apps instaladas. La app consulta
      `https://taxpain.es/updates/latest.json` (`MANIFEST_URL` en `updates.rs`), así que, **después**
      de publicar la release, descarga el `latest.json` de la release, cópialo a `web/updates/`
      (es el único fichero que cabe ahí; se puede versionar) y despliega la web
      (`./scripts/deploy-web.sh`). `latest.json` es lo que anuncia la versión: así nunca se anuncia
      una cuyo paquete aún no está público. Hasta entonces la app dice «Tienes la última versión».
   - Alternativa sin pasar por la web: apuntar `MANIFEST_URL` (y `plugins.updater.endpoints` en
     `tauri.conf.json`) a `https://github.com/<propietario>/taxpain-app/releases/latest/download/latest.json`.
     Entonces publicar la release ya anuncia la versión (GitHub resuelve `latest` a la última
     release publicada, no en borrador ni *pre-release*), con el repositorio público o con los
     assets accesibles. Cambiarlo exige una versión nueva de la app y actualizar la tabla de
     `docs/PRIVACIDAD_APP.md` (la petición iría a GitHub, no a taxpain.es).
5. Retirar una versión: volver a poner el `latest.json` anterior (o pasar la release a borrador si
   la app mira la release *latest*) deja de ofrecerla a quien aún no la tiene, pero **no hace
   downgrade** a quien ya la instaló (el actualizador sólo instala versiones mayores, y la
   versión anunciada ha de ser la firmada, así que tampoco vale volver a firmar la antigua con
   un número mayor sin recompilarla). Para corregir, publica una versión nueva con el arreglo.

## 4. Versión

La versión es una sola: `node scripts/version.mjs set X.Y.Z` cambia a la vez `Cargo.toml`
(`workspace.package.version`), `apps/desktop/src-tauri/tauri.conf.json` y
`apps/desktop/package.json`; el CI falla si no coinciden. Añade en `CHANGELOG.md` la sección
`## X.Y.Z — fecha` con lo que cambia para quien usa la app: son las notas que enseña el
actualizador. Luego etiqueta `vX.Y.Z`. El workflow falla si la etiqueta no es la versión o si
faltan sus notas, y comprueba que están todos los ficheros esperados (DMG y, con clave del
actualizador, `.app.tar.gz`, `.sig` y `latest.json`).

## 5. Diseños de registro de la AEAT

Antes de cada versión se comprueba que TaxPain tiene los diseños de registro (DR) que la AEAT
ha publicado: cada año salen los del ejercicio siguiente y a veces cambian a mitad de año. Sin
ellos, TaxPain no lee por posiciones los ficheros de las declaraciones de ese ejercicio
(«Comprobar lo presentado», Importar → Comparar): las lee del PDF y avisa de que hay que
actualizar TaxPain.

```sh
python3 -I tools/aeat/dr_actualizar.py --comprobar   # 0 al día, 1 novedades, 3 sin red
python3 -I tools/aeat/dr_actualizar.py               # baja las novedades a fixtures/aeat/dr/nuevos/
```

La herramienta lee las páginas «Diseños de registro» de la Sede (modelos 100-399, actuales y
de ejercicios anteriores), las compara con `fixtures/aeat/dr/sede.json` (lo ya revisado:
«incluido», «ignorado» con su motivo o «pendiente») y, sin `--comprobar`, baja lo nuevo con su
SHA-256 y genera el JSON de las hojas de cálculo con `tools/aeat/dr2json.py` (necesita openpyxl
y xlrd). No hace commit. Para cada diseño bajado:

1. Revisa el JSON (páginas y longitudes) frente al del ejercicio anterior.
2. Muévelo a `fixtures/aeat/dr/anteriores/` (o sustituye el vigente si cambia el generador) y
   añádelo a `DESIGNS` en `crates/models/src/dr_versions.rs` con sus periodos.
3. Anótalo en `fixtures/aeat/dr/SOURCES.md` y márcalo «incluido» en `sede.json`.
4. `./scripts/ci.sh` (el test `dr_versiones` lee un fichero con cada diseño).

`./scripts/ci.sh` la ejecuta con `--comprobar`: falla si hay novedades y sólo avisa sin red. El
workflow «Diseños de registro» (a mano, como el del BOE) abre una incidencia si las hay; en
local se puede programar una vez por semana.

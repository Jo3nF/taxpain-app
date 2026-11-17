# Registro de decisiones

Decisiones importantes con su motivo, para poder revisarlas. La más reciente al final.
Formato inspirado en `ref/spainassist/docs/DECISIONS.md` (MIT).

## D1 — Stack: Tauri 2 + núcleo Rust + React/TypeScript (2026-10-13)

**Contexto.** Encargo: app de escritorio macOS (Apple Silicon e Intel; Windows después),
local-first, con base de datos cifrada, Keychain, Touch ID, mTLS con certificado del usuario,
generación de ficheros oficiales y contabilidad exacta. Opciones: A) Tauri 2 + Rust,
B) Swift/SwiftUI nativo, C) Electron (lo que usa Spain Assist).

**Decisión.** Opción A: **Tauri 2** con **núcleo en Rust** (`crates/*`) y **frontend React +
TypeScript** (Vite, Tailwind) en `apps/desktop`.

**Por qué.**
- *El dominio es el producto.* Contabilidad, IVA, modelos y VERI\*FACTU son lógica pura y
  crítica: Rust da tipos fuertes (importes en céntimos `i64` con newtype, sin floats), tests
  rápidos (`cargo test`, `proptest`) y un core sin IO reutilizable en Windows, CLI o servidor de
  pruebas.
- *Seguridad de secretos.* El certificado y las claves viven en un proceso nativo (Rust), nunca
  en el WebView. `zeroize`/`secrecy` para borrar secretos de memoria. El frontend sólo recibe
  resúmenes (huella, titular, caducidad) por comandos Tauri tipados.
- *Integración macOS suficiente.* `security-framework` (Keychain, `SecAccessControl` con
  biometría, identidades), `objc2-local-authentication` (Touch ID), `rusqlite` con
  `bundled-sqlcipher` (BD cifrada), `reqwest` + `rustls` (mTLS con identidad del usuario).
- *Distribución.* Binario pequeño (~10–20 MB frente a ~150 MB de Electron), firma y
  notarización Apple soportadas por el bundler de Tauri, auto-update firmado (plugin updater).
- *Portabilidad.* Windows es la siguiente plataforma: Tauri lo soporta (WebView2); con Swift
  habría que reescribir la app entera.

**Alternativas descartadas.**
- *Swift/SwiftUI (B).* Mejor integración con el sistema y UI nativa, pero cero portabilidad a
  Windows, y el dominio contable/fiscal quedaría atado a Apple. Se mantiene como plan B sólo
  para piezas puntuales (p. ej. un helper firmado si Tauri no bastara para Touch ID).
- *Electron (C).* Spain Assist lo eligió porque todo su producto es TypeScript (agente pi,
  Playwright). Nuestro núcleo es cálculo exacto y criptografía; Electron añade ~150 MB, Node con
  acceso a secretos en el mismo runtime que dependencias npm (superficie de cadena de
  suministro) y no aporta nada al dominio. Playwright sigue disponible si hiciera falta como
  proceso aparte (ver D5).

**Costes.** Dos lenguajes (Rust + TS). El WebView de macOS es WKWebView (Safari): hay que probar
la UI en WebKit. En el entorno de desarrollo actual (Linux sin `webkit2gtk` ni sudo) **no se
puede compilar la app Tauri**; los crates del núcleo sí. Ver D4.

## D2 — Monorepo: workspace Cargo + workspace npm (2026-10-13)

```
apps/desktop           Tauri 2 (src-tauri = crate `taxpain-desktop`) + React/TS (Vite, Tailwind)
apps/landing           taxpain.es estática (se publica aparte, en Cloudflare Pages)
crates/core            dominio puro: dinero, NIF, periodos, contabilidad, IVA… (sin IO)
crates/vault           BD cifrada (SQLCipher), proveedores de clave, certificados, auditoría
crates/aeat            diseños de registro, prevalidación, presentación, VERI*FACTU, consultas
crates/invoicing       facturas, series, PDF/Facturae (Fase 3)
crates/macos           Keychain + Touch ID (sólo compila con target macOS)
docs/  fixtures/  scripts/
```

- `apps/desktop/src-tauri` es miembro del workspace pero **no** de `default-members`: `cargo test`
  en Linux prueba el núcleo; CI en macOS construye todo.
- npm workspaces (sin pnpm: menos herramientas). Biome 2 (lint + formato) para TS, `rustfmt` +
  `clippy -D warnings` para Rust. `scripts/ci.sh` ejecuta todo localmente.

## D3 — Base de datos cifrada: SQLCipher vía `rusqlite` `bundled-sqlcipher` (2026-10-13)

- Un fichero `taxpain.db` cifrado entero (páginas AES-256, HMAC-SHA512) con **clave bruta de
  256 bits** (`PRAGMA key = "x'…'"`), sin KDF de contraseña: la clave es aleatoria y vive en el
  Keychain.
- Frente a "SQLite + cifrado por columna": SQLCipher cifra también índices, metadatos y el WAL;
  no hay que decidir columna a columna qué es sensible.
- En macOS SQLCipher usa CommonCrypto; en Linux (desarrollo) usa `libcrypto` del sistema.
  Confirmado el 26-10-2026 (auditoría): `crates/vault/Cargo.toml` usa `bundled-sqlcipher` (no la
  variante `vendored-openssl`); el `build.rs` de libsqlite3-sys 0.38.2 (l. 184 y 235-247) sólo
  enlaza OpenSSL con esa feature y, cuando host y destino son Apple, compila SQLCipher con
  `-DSQLCIPHER_CRYPTO_CC`; `Cargo.lock` no contiene `openssl-sys` ni `openssl-src`. Matiz: una
  compilación cruzada desde Linux sí buscaría libcrypto; el CI de macOS compila en Apple.
  Revisado el 13-11-2026 al subir a `rusqlite` 0.42.0 (`libsqlite3-sys` 0.40.0, SQLCipher 4.20.0):
  el código fuente de SQLCipher lo trae ahora el crate `sqlcipher-src`, y el `build.rs`
  sigue igual: OpenSSL sólo si lo encuentra, y `-DSQLCIPHER_CRYPTO_CC` con el
  marco Security cuando host y destino son Apple. El libtomcrypt que acompaña a ese crate no
  se compila.
- Migraciones SQL versionadas embebidas en el binario (`PRAGMA user_version`).

## D4 — Claves y Touch ID: `KeyProvider` + `UserPresence` inyectados (2026-10-13)

Igual que Spain Assist (`@sa/vault` recibe un `KeyProvider`), el crate `vault` no depende de
macOS:

- `KeyProvider` guarda/recupera claves de 256 bits por nombre (`db-master`, `cert-seal`).
  - `KeychainKeyProvider` (crate `macos`): elemento genérico del Keychain del usuario,
    `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` (no se sincroniza con iCloud ni va a
    copias). La clave de sellado del certificado lleva además `SecAccessControl` con
    `.biometryCurrentSet` o `.userPresence` → leerla exige Touch ID (o contraseña de usuario).
  - `FileKeyProvider` (sólo desarrollo/tests, marcado **inseguro**) y `MemoryKeyProvider`.
- `UserPresence::confirm(motivo)` → Touch ID con `LAContext.evaluatePolicy` (crate `macos`);
  en tests, un doble que acepta/rechaza. Toda firma o presentación pasa por aquí.
- El crate `macos` no enlaza C: se comprueba desde Linux con
  `cargo check -p taxpain-macos --target aarch64-apple-darwin`. Las pruebas reales de Keychain
  y Touch ID requieren un Mac (`TODO(VERIFY)` en PROGRESS.md).

## D5 — Certificado: copia sellada con clave biométrica, uso sólo en el proceso nativo (2026-10-13)

- Importación: se valida el `.p12/.pfx` (contraseña, clave privada coincide con certificado,
  vigencia, emisor FNMT u otro, tipo persona física / representante, NIF del titular y, si
  procede, NIF de la entidad). La contraseña del usuario **no se guarda**.
- Se guarda la clave privada (PKCS#8) + cadena (DER) **sellada** con AES-256-GCM con la clave
  `cert-seal` (Keychain + biometría) y AAD = huella del certificado, dentro de la BD cifrada
  (doble cifrado). `certificate_ref` sólo tiene metadatos.
- Uso: `Touch ID → desellar en memoria (Zeroizing) → identidad rustls → mTLS → borrar`.
- Alternativa futura (más fuerte): importar con `SecPKCS12Import` como identidad **no
  exportable** y firmar el handshake con `SecKeyCreateSignature` mediante un `SigningKey` de
  rustls a medida. Se deja preparado el trait `ClientIdentity` para cambiarlo sin tocar el resto.
- Por qué no OpenSSL para el `.p12`: evitar empaquetar OpenSSL en macOS. Se usan crates Rust
  puros (`p12-keystore`, `x509-parser`), que leen PBES2/AES y los formatos heredados 3DES/RC2
  que aún producen algunas exportaciones. `TODO(VERIFY)` con un `.p12` FNMT real exportado de
  Safari/Firefox/Keychain (en local, nunca en el repo).

## D6 — Acceso a la Sede: ficheros DR y servicios web primero; navegador sólo para consultas (2026-10-13)

- Orden de preferencia: 1) servicio web oficial con mTLS desde Rust (`reqwest`), 2) envío de
  fichero DR por los formularios de presentación con mTLS, 3) automatización de navegador sólo
  para consultas sin servicio (Mis expedientes, deudas, notificaciones).
- Para (3) se seguirá el patrón del sidecar de Spain Assist (proxy local que presenta el
  certificado sólo a hosts exactos de una allow-list `*.agenciatributaria.gob.es`); en Rust,
  dentro del propio proceso, sin binario Go. Se diseñará en la Fase 6.
- Nunca se presenta nada real en desarrollo: el cliente AEAT tiene `Entorno::{Mock, Preproduccion,
  Produccion}` y producción exige confirmación explícita en tiempo de ejecución.

## D7 — Importes en céntimos `i64` y redondeo explícito (2026-10-13)

- `Money(i64)` en céntimos; operaciones con overflow comprobado. Porcentajes como `Rate` en
  centésimas de punto (21 % = 2100) y redondeo **half-up (alejándose de cero) al céntimo**
  por línea/base, que es el criterio habitual de la AEAT para cuotas.
  Criterio por factura o por línea (acotado el 26-10-2026): ni el ROF ni la LIVA lo fijan; el TEAC
  (RG 2233/2022, 26/11/2024) rechaza el redondeo unidad a unidad cuando distorsiona la cuota y
  sostiene que debe hacerse al final, sobre la cuota agregada, así que el modo por defecto (por
  tipo) es coherente y por línea queda como opción. `TODO(VERIFY)`: criterio aún no reiterado (no
  es doctrina del art. 239 LGT). Detalle en `docs/INVOICING_RULES.md` §11.

## D8 — Logs sin secretos por construcción (2026-10-13)

- `tracing` con tipos secretos (`secrecy::SecretBox`, newtypes con `Debug` = `[REDACTED]`).
- Test automático: se importa un certificado y se hace mTLS con nivel TRACE capturado; se
  comprueba que ni la clave (PEM/DER/hex/base64), ni la contraseña, ni la clave de BD aparecen
  en logs, en el fichero de BD en claro ni en los bytes de red capturados.

## D9 — Keychain sin ACL biométrica por ahora; Touch ID con LocalAuthentication (2026-10-13)

**Contexto.** Para que el propio Keychain exija Touch ID al leer `cert-seal` hay que usar el
*data protection keychain* con `SecAccessControl(.userPresence)`, que sólo funciona con la app
**firmada** con el entitlement `keychain-access-groups` (si no: `errSecMissingEntitlement`,
-34018). Además, sin pasar un `LAContext` ya evaluado (`kSecUseAuthenticationContext`) el
usuario vería dos diálogos (el nuestro con el motivo y el del Keychain).

**Decisión (Fase 1).**
- `KeychainKeyProvider` con dos modos: `DataProtection` (app firmada:
  `WhenUnlockedThisDeviceOnly`, no sincronizado) y `Legacy` (desarrollo sin firmar: keychain de
  sesión, ACL ligada a la firma de código del binario).
- La confirmación la hace `TouchIdPresence` (`LAPolicy.deviceOwnerAuthentication`: Touch ID o
  contraseña del usuario, para Macs Intel sin Touch ID) **antes** de leer `cert-seal`, con el
  motivo concreto ("Presentar el modelo 303 … con el certificado de …").
- Crate `taxpain-secrets` sin dependencias C (claves, `KeyProvider`, `UserPresence`) para poder
  comprobar `taxpain-macos` desde Linux con `cargo check --target aarch64-apple-darwin`.

**Pendiente (Fase 8, con firma Apple).** ACL `.userPresence` en `cert-seal` + `LAContext`
compartido para un único diálogo y aplicación por el sistema, no sólo por la app.
`TODO(VERIFY)` en un Mac real: Keychain en ambos modos, Touch ID, cancelación y tiempo de espera.

## D10 — Carpetas destino a través de las apps de sincronización; capa `taxpain-app` (2026-10-13)

- **Destino de facturas.** iCloud Drive (`~/Library/Mobile Documents/com~apple~CloudDocs`),
  Google Drive (`~/Library/CloudStorage/GoogleDrive-*/My Drive|Mi unidad`) y Dropbox
  (`~/Library/CloudStorage/Dropbox*` o `~/Dropbox`) se usan como **carpetas locales** que su app
  de escritorio sincroniza. Sin OAuth, sin secretos de cliente y sin servidor propio; la
  integración por API de Google Drive queda como opción futura si hace falta (p. ej. sin la
  app de escritorio). Los ficheros exportados están **en claro** a elección del usuario; el
  original queda siempre cifrado en la BD.
- **Casos de uso en `crates/app`.** La emisión toca BD, PDF, Facturae y VERI*FACTU en una
  transacción; vive en un crate sin UI que se prueba en Linux. Tauri sólo traduce comandos.
- **Facturas como JSON + columnas.** `invoice_issued.data` guarda la factura y sus totales
  serializados (fuente de verdad); las columnas de importes permiten consultar y sumar sin
  deserializar. La tabla `invoice_line` del modelo inicial no se crea: las líneas viven en el
  JSON y los asientos (Fase 4) usarán el desglose ya calculado.
- **Auditoría con SAVEPOINT**, para poder anotarla dentro de la transacción de cada operación.

## D11 — Bancos en solo lectura con Enable Banking, con la aplicación de cada empresa (2026-10-31)

- **Qué.** Leer cuentas, saldos y movimientos de BBVA y Revolut (PSD2, servicio de información
  de cuentas) para no depender de los extractos en fichero. Nada de pagos: el permiso que se
  pide es sólo de información de cuentas, hasta 180 días.
- **Por qué Enable Banking.** Proveedor de información de cuentas autorizado (FIN-FSA, Finlandia,
  con pasaporte para España), con BBVA y Revolut (empresa y particulares), una API documentada y
  un **modo restringido gratuito** para leer las cuentas propias. Se descartaron: GoCardless (no
  acepta altas), Tink (sin cuentas de empresa en España), Plaid (sólo planes a medida en la UE),
  Kontomatik (IP fija), Arcopay (exige un servidor para la vuelta) y los que no tienen precio
  público. finAPI («Access for own use») y Powens quedan como alternativa con contrato.
- **Sin servidor de TaxPain y sin claves compartidas.** Cada empresa registra **su propia
  aplicación** en Enable Banking y guarda en TaxPain su id y su clave privada (`.pem`), que van
  en `app_secret` (BD cifrada, fuera de las copias) y nunca vuelven a la interfaz. TaxPain firma
  cada petición en local (JWT RS256, una hora de validez) y habla directamente con
  `api.enablebanking.com`. Así no hay un intermediario de quien distribuye TaxPain que vea los datos ni una clave
  repartida en todas las instalaciones (lo que Enable Banking prohíbe).
- **Vuelta del banco sin servidor.** La dirección de vuelta que se registra puede ser
  `https://localhost/taxpain/banco` (nadie la escucha: el navegador da un error, pero la barra
  lleva el código y se pega en TaxPain, como en el ejemplo oficial de Enable Banking) o
  `http://127.0.0.1:<puerto>/…` (TaxPain escucha sólo en la propia máquina mientras se conecta,
  hasta la primera petición que trae el código o el error con el `state` de la autorización). El
  `state` al azar de cada autorización tiene que volver igual. La clave privada no va en las
  copias de seguridad (como todo `app_secret`): el usuario guarda su `.pem`.
- **Los movimientos entran como un extracto más** (`extractos::importar`): misma cuenta y
  subcuenta que sus extractos en fichero, y sin duplicados: primero por la referencia de la
  entrada que da el banco (`entry_reference`, única e inmutable en la cuenta según Enable
  Banking; el `transaction_id` no sirve), con el mismo importe, en `bank_tx.external_id`; los que
  el usuario borra no vuelven (`bank_tx_ignored`); y, contra los movimientos de fichero (sin
  identificador), por fecha, importe y saldo. Sólo los contabilizados (`BOOK`) y en la divisa de
  la cuenta. La primera descarga pide el historial más largo (`strategy=longest`); las
  siguientes, desde diez días antes de la última o del movimiento pendiente más antiguo que
  había entonces (una retención de tarjeta puede tardar en contabilizarse).
- **Límites.** TaxPain no envía las cabeceras `Psu-*` que dirían que el titular está delante
  (piden su IP pública), así que cada descarga cuenta como hecha sin él, y PSD2 las limita a
  cuatro al día por cuenta: las descargas son a petición y el saldo se pide sólo en la primera
  del día. Sin conexión, se siguen importando los extractos en fichero (Norma 43, Excel, CSV,
  PDF…).
- **Opción avanzada, no la vía principal (revisado el 13-11-2026).** Las condiciones de Enable
  Banking (9-1-2026, «Restriction of Use») limitan el modo restringido a las cuentas vinculadas
  y sólo para evaluación o uso personal de particulares; no dan licencia para uso de empresa ni
  comercial. A las empresas les dejan registrar y activar aplicaciones «como evaluación» hasta
  que avisen por escrito de que las desactivan, y prohíben ceder el acceso a terceros. El uso
  normal pide contrato, verificación de la empresa (KYB), enlaces de condiciones y privacidad,
  y un mínimo mensual (precio a petición). Por eso la pantalla y la guía presentan los extractos
  en fichero como lo habitual y la conexión como opción avanzada que puede cortarse. Vías para
  más adelante: la clave directa del propio banco donde existe (Revolut Business, Qonto: el
  cliente crea una clave de sólo lectura, sin intermediario ni servidor) y, sólo con una empresa
  y un servicio detrás, un contrato con un agregador (obliga a un servidor que ve los datos).
  `TODO(VERIFY)`: que la API de Revolut Business esté en todos sus planes; qué cuentas da la
  clave de Qonto; precio de Enable Banking con contrato.

## D12 — Conversaciones con la IA por tema, guardadas en el almacén (2026-10-31)

- **Qué.** Desde cada recomendación de «Ahorrar impuestos» (y del plan del año), cada hallazgo de
  una revisión y cada borrador de modelo se habla con la IA de ese tema; el usuario puede añadir
  datos y documentos y seguir la conversación otro día. El Asistente usa las mismas
  conversaciones (generales) y las lista todas.
- **Una conversación por tema y ejercicio** (índice único en `kind`, `topic_key`, salvo las
  generales, que pueden ser las que se quieran). Para empezar de cero un tema, se borra.
- **El tema se rehace en Rust en cada pregunta** con los datos de ahora (`ahorro::ia`), y lo
  último que se envió se guarda (`snapshot`): con él siguen funcionando las conversaciones de un
  ejercicio cerrado o de una ficha que ya no sale, y se avisa si los datos han cambiado desde la
  última pregunta. El tema va en las instrucciones (lo genera TaxPain), con reglas fijas para
  hablar de un tema.
- **Lo que añade el usuario va al principio de su última pregunta**, marcado como suyo y sin
  comprobar (menos autoridad que las instrucciones y sin repetirlo en el historial); los
  documentos, como documento externo y **sin el nombre del fichero** (suele llevar el de un
  tercero). De un documento se guarda sólo el texto leído en el Mac, su huella y su tamaño.
- **Sólo se guarda lo respondido**: la pregunta y la respuesta juntas, en una transacción, al
  llegar la respuesta; si falla, la pregunta vuelve al cuadro (también si el panel se cerró
  mientras pensaba: al volver a abrirlo) y no queda nada a medias. Si la conversación se borró
  mientras respondía, no se guarda en ninguna otra: los id no se reutilizan (`AUTOINCREMENT`) y
  se comprueba que es la misma (tema y fecha de creación).
- **La auditoría (`assistant.chat`) se escribe antes de enviar**, después de comprobar el
  consentimiento y la clave, con el tipo de tema y cuántos datos y documentos; nunca el texto.
- **Empresa fijada.** Las herramientas y la auditoría son de la empresa en la que se pregunta y
  fallan si se cambia a medias; la respuesta (ya pagada) se guarda en esa empresa aunque ya no sea
  la abierta (`AppState::company_vault`). Antes de la primera pregunta de una conversación nueva
  se comprueba que cabe, para no pagar una respuesta que no se pueda guardar.
- **Anonimización.** Los textos se reúnen con el almacén cogido y se anonimizan después, sin él;
  una segunda pasada (`scrub_seen`) tapa en todo el envío lo que se tapó en una parte (una
  respuesta guardada vuelve con los nombres reales), también en lo que devuelven las
  herramientas (la IA busca justo por ese marcador). Los terceros que nombran los avisos de un
  borrador o del resumen (`Draft::terceros`: un perceptor del 190, un viajero del 111…) se tapan
  como los datos de la empresa, como valores conocidos, y la respuesta los enseña con su nombre;
  la herramienta `borrador_modelo` los quita («un tercero»). Los hallazgos de una revisión
  apuntan (`Hallazgo::terceros`) los nombres, NIF y NIF-IVA que nombran, de los que conoce la
  revisión (facturas, libros, resúmenes anuales, Nóminas y escrituras), y van igual; una revisión
  de la carpeta guardada por una versión anterior, sin ellos, hay que repetirla antes de preguntar
  por sus hallazgos. El historial se guarda con los datos restaurados, así que lo que se tapó en
  una pregunta (lo que tapó el anonimizador como valor conocido o con el detector de nombres) se
  guarda con la respuesta y se vuelve a tapar en las siguientes, aunque ya no esté en los datos
  (un viaje borrado, el nombre de antes de la empresa). Una respuesta anterior que parezca una
  clave o un certificado (un ejemplo en PEM) no se reenvía, pero no bloquea la conversación; lo
  que escribe el usuario, sí. Un certificado o una clave se reconoce por su contenido, tenga la
  extensión que tenga: en binario (DER o PKCS#12) y en base64 sin las cabeceras PEM, en una línea
  o partido en varias (también fila a fila en una hoja de cálculo), en un documento, un dato o una
  pregunta (`pii::base64_key_or_cert` descodifica su principio y mira si es una clave o un
  certificado).
- **Límites.** 100 conversaciones por empresa (no se empieza otra; nunca se borra ninguna sin
  pedirlo; las primeras preguntas que esperan su respuesta cuentan, para que varias a la vez no
  lo pasen, salvo si su conversación ya existe porque se le ha añadido algo mientras espera), 100
  preguntas por conversación, las últimas 30 intervenciones y 30.000 caracteres del historial en
  cada pregunta, y 20 cosas añadidas (2.000 caracteres un dato, 15.000 un documento, 30.000 entre
  todas). De una hoja de cálculo, 5 hojas y 300 filas de cada una; lo que queda fuera se dice en
  el texto y el documento va marcado como «no va entero».
- **«Los datos han cambiado».** Se avisa si el tema de ahora no es el de la última pregunta; una
  recomendación de «Ahorrar impuestos» se rehace con los datos del día de su foto, porque la
  amortización que se devenga cada día cambia sus cifras sin que cambie nada.
- **Hallazgos y borradores (v1.1).** También se pregunta por cada hallazgo de «Qué revisar» y de
  la revisión de la carpeta, y por el borrador de un modelo. Un hallazgo se identifica por su
  origen («Qué revisar» se rehace con lo que tiene la AEAT; la carpeta usa su última revisión
  guardada, porque rehacerla tarda), su código, su ejercicio y su título (el código y el ejercicio
  se repiten: la clave lleva la huella del título). Nunca van las rutas de sus ficheros: cuántos
  son y, donde el texto las cita, «fichero 1», «fichero 2»…; el detalle de la carpeta, que puede
  citar lo que dicen los documentos, va como documento externo. De un borrador van el resultado,
  las casillas (`[NN] importe`), los avisos (con marcadores en los terceros que nombran),
  cuántos perceptores o terceros y si ya se presentó desde TaxPain; nunca la relación de
  terceros, el fichero, su nombre ni el CSV. Si un hallazgo deja de salir o un borrador ya no se
  calcula, se sigue con la foto de la última pregunta; sin ella (se añadió algo sin llegar a
  preguntar), de un hallazgo sólo va su código, porque su título puede nombrar a terceros y
  ficheros que ya no se pueden tapar.
- **Descartado.** Guardar en `localStorage` (sin cifrar y fuera de las copias), volver a enviar
  los ficheros enteros (sólo su texto, salvo que se pida que los lea la IA con «Full IA», que
  queda para más adelante) y que la IA marque pasos o cambie algo (el usuario marca su avance).

# Avisos de terceros

## Spain Assist

Partes de TaxPain adaptan código o diseño de Spain Assist
(https://github.com/MeLlamoPablo/SpainAssist), bajo licencia MIT:

- `crates/vault/src/certificate.rs` — `parse_holder`: interpretación del sujeto de
  certificados FNMT, adaptada de `packages/vault/src/certificate.ts` (`parseHolder`).
- Diseño del almacén con `KeyProvider` inyectado y política "pedir antes de usar"
  (`packages/vault`), y patrón de sidecar mTLS con allow-list de hosts (`packages/sidecar`).
- `crates/assistant/src/pii.rs` — sustitución de los valores conocidos del usuario con
  variantes (DNI sin letra, teléfono sin prefijo, IBAN como cuenta) y detectores con dígito de
  control (NUSS, cuenta, tarjeta), adaptados de `packages/pii/src/{known,validators,detectors}.ts`
  (versión del 2026-10-22, commit `180d6f1`); limpieza de invisibles y marcas de documento
  externo, de la nota `docs/research/prompt-injection-agents-2026.md`; prueba de canarios
  (`crates/assistant/tests/canary.rs`), de `tools/eval/src/net/canary.ts`.

```
MIT License

Copyright (c) 2026 Pablo Rodríguez Caballero

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Ficheros oficiales vendorizados (`fixtures/`)

Se incluyen, sin modificar, para validar el software contra las especificaciones oficiales:

- **Agencia Tributaria** (WSDL/XSD de VERI*FACTU, diseño `LSI.xlsx` de libros registro,
  vectores de huella y QR extraídos de sus especificaciones, diseños de registro de los
  modelos en `fixtures/aeat/dr/`), **BOE** (festivos, PGC PYMES y
  sus modelos de cuentas anuales, tablas de amortización) y **Facturae** (XSD 3.2.2 del
  Ministerio de Hacienda). Son disposiciones y documentos oficiales; véase el art. 13 del Texto
  Refundido de la Ley de Propiedad Intelectual. Cada fichero indica su URL y SHA-256 en el
  `SOURCES.md` o la cabecera correspondiente.
- **OASIS UBL 2.1** (`fixtures/oasis/ubl-2.1/`, esquemas `UBL-Invoice`, `UBL-CreditNote` y
  `common/`), © OASIS Open 2013. OASIS permite copiarlos y distribuirlos sin modificar; cada
  fichero conserva su aviso de copyright. Origen y huellas en `SOURCES.md` y `SHA256SUMS`.
- **W3C XML Signature schema** (`xmldsig-core-schema.xsd`), © World Wide Web Consortium,
  distribuido bajo la W3C Software and Document License
  (https://www.w3.org/copyright/software-license/).
- `fixtures/accounting/n43-ejemplo.txt` y `fixtures/certs/*.p12` son **sintéticos**, generados
  para los tests (NIF de prueba válidos por algoritmo; contraseña de los `.p12`: `Prueba-1234`).

## Dependencias

Las dependencias de Rust y npm tienen sus propias licencias (mayoritariamente MIT o
Apache-2.0); se listan en `Cargo.lock` y `package-lock.json`. El CI comprueba que todas tienen
una licencia permisiva (`tools/licenses/check.mjs`: falla con GPL, LGPL, AGPL u otras sin
alternativa permisiva). `TODO`: incluir el informe completo de licencias en la app.

### SQLCipher

La base de datos cifrada de cada empresa usa SQLCipher (https://www.zetetic.net/sqlcipher/),
compilado dentro de la app a través de `rusqlite` con la feature `bundled-sqlcipher`
(`libsqlite3-sys` 0.40.0, que toma el código fuente de SQLCipher 4.20.0 y SQLite 3.54.1 del crate
`sqlcipher-src`). El cifrado lo hace la biblioteca del sistema: CommonCrypto en macOS y OpenSSL
en Linux. Ese crate trae además libtomcrypt (dominio público o WTFPL) para otras plataformas;
TaxPain no lo compila. SQLite es de dominio público; SQLCipher tiene esta licencia:

```
Copyright (c) 2025, ZETETIC LLC
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:
    * Redistributions of source code must retain the above copyright
      notice, this list of conditions and the following disclaimer.
    * Redistributions in binary form must reproduce the above copyright
      notice, this list of conditions and the following disclaimer in the
      documentation and/or other materials provided with the distribution.
    * Neither the name of the ZETETIC LLC nor the
      names of its contributors may be used to endorse or promote products
      derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY ZETETIC LLC ''AS IS'' AND ANY
EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL ZETETIC LLC BE LIABLE FOR ANY
DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
(INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### Piezas con licencia MPL-2.0

Estas dependencias de Rust tienen licencia Mozilla Public License 2.0
(https://www.mozilla.org/MPL/2.0/). TaxPain las usa sin modificar; su código fuente está en
sus repositorios y en crates.io:

| Paquete | Versión | Código fuente |
|---|---|---|
| `cssparser` | 0.37.0 | https://github.com/servo/rust-cssparser |
| `cssparser-macros` | 0.7.1 | https://github.com/servo/rust-cssparser |
| `selectors` | 0.38.0 | https://github.com/servo/stylo |
| `dtoa-short` | 0.3.5 | https://github.com/upsuper/dtoa-short |
| `option-ext` | 0.2.0 | https://github.com/soc/option-ext |

## reforma-digital

Ideas de reforma-digital (https://github.com/samuelcorsan/reforma-digital,
MIT, © sus autores; revisado en el commit `fe41031`): notarización con clave de API y versiones
etiquetadas que fallan sin firma (`.github/workflows/release.yml`), prueba de privacidad de punta
a punta (`crates/app/tests/privacidad_e2e.rs`), hoja «qué sale del ordenador»
(`docs/PRIVACIDAD_APP.md`) y vigía diario del BOE (`tools/boe/watch.mjs`).

`crates/assistant/src/ner/` (detector local de nombres y direcciones) adapta el tokenizador, la
lectura por ventanas y los filtros de `packages/agent/src/rampart-edge.ts` y
`packages/agent/src/detector.ts`, y algunas de sus reglas (tratamientos, «a nombre de»,
direcciones con tipo de vía).

```
MIT License

Copyright (c) 2026 Reforma Digital contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Rampart Edge

`crates/assistant/models/rampart-edge/` (modelo `model_q4.onnx`, `vocab.txt` y `config.json`)
es Rampart Edge de National Design Studio
(https://huggingface.co/nationaldesignstudio/rampart-edge, revisión `69a99a3`), bajo
**CC0 1.0** (dominio público; texto en `LICENSE` de esa carpeta). Se incluye sin cambios.
Está entrenado a partir de `microsoft/xtremedistil-l6-h256-uncased`
(https://huggingface.co/microsoft/xtremedistil-l6-h256-uncased), © Microsoft Corporation, bajo
licencia MIT; `vocab.txt` procede de ese modelo.

## Facturae-PHP

`crates/invoicing/src/xades.rs` (firma XAdES-EPES de la Facturae con la política de firma de
Facturae v3.1) adapta la estructura de la firma de `src/Common/FacturaeSigner.php` y
`src/Common/XmlTools.php` de Facturae-PHP (https://github.com/josemmo/Facturae-PHP), bajo
licencia MIT:

```
MIT License

Copyright (c) 2018-present José M. Moreno <josemmo@pm.me> (https://github.com/josemmo)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

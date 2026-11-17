# VERI*FACTU: qué debe hacer TaxPain como SIF

> Investigado el **2026-10-13** leyendo (solo GET) documentos públicos de la AEAT y del BOE. No se ha
> llamado a ningún servicio SOAP. Todo dato sin fuente literal se marca `TODO(VERIFY)`.
> Ficheros oficiales vendorizados y sus hashes: `fixtures/aeat/verifactu/SOURCES.md`.

**Fuentes** (abreviatura → documento):
- `[RRSIF]` RD 1007/2023, texto consolidado (actualizado el 03/12/2025): https://www.boe.es/buscar/act.php?id=BOE-A-2023-24840
- `[OM]` Orden HAC/1177/2024: https://www.boe.es/buscar/act.php?id=BOE-A-2024-22138
- `[ROF]` RD 1619/2012 (Reglamento de facturación): https://www.boe.es/buscar/act.php?id=BOE-A-2012-14696
- `[XSD]` `fixtures/aeat/verifactu/SuministroInformacion.xsd`, `SuministroLR.xsd`, `RespuestaSuministro.xsd`
- `[DR]` Diseño de registro `DsRegistroVeriFactu.xlsx` v1.0 (`SF/IVA/VERI-FACTU/DsRegistroVeriFactu.xlsx`)
- `[SW]` `Veri-Factu_Descripcion_SWeb.pdf` v1.0.3 (28/07/2025)
- `[VAL]` `Validaciones_Errores_Veri-Factu.pdf` v1.2.2 (08/04/2026)
- `[HASH]` `Veri-Factu_especificaciones_huella_hash_registros.pdf` v0.1.2 · `[QR]` `DetalleEspecificacTecnCodigoQRfactura.pdf` v0.5.0
- `[FAQ]` FAQ de la Sede (actualizadas a 21/07/2026): `https://sede.agenciatributaria.gob.es/Sede/iva/sistemas-informaticos-facturacion-verifactu/preguntas-frecuentes/…`

`SF` = `https://www.agenciatributaria.es/static_files/AEAT_Desarrolladores/EEDD`.
Plazos de obligatoriedad y la posible prórroga a 2028: ver `docs/AEAT_INTEGRATION.md` §3.

## 1. Qué es un SIF VERI*FACTU

- Remite a la AEAT **todos** los registros de facturación "de forma continuada, segura, correcta, íntegra,
  automática, consecutiva, instantánea y fehaciente" ([RRSIF] art. 16.1).
- Se presume que cumple por diseño el art. 8 del RRSIF. **No** tiene que firmar los registros: basta con la huella
  ([RRSIF] art. 16.2 y 16.3; [OM] art. 3). Tampoco genera registros de evento ([FAQ] huella-hash).
- Una vez elegido, se mantiene al menos hasta el 31/12 del año del primer envío ([RRSIF] art. 16.5; [OM] art. 17.2).
  Para renunciar se informa `Cabecera/RemisionVoluntaria/FechaFinVeriFactu` antes de fin de año ([OM] art. 17.3).
  Desde el 01/01/2027 ese campo debe tener el formato `31-12-20XX` ([VAL] 3.1.1.3).
- Registros que genera: **RegistroAlta** (al expedir la factura) y **RegistroAnulacion**. Ambos se encadenan en
  una única cadena por SIF y obligado, en orden cronológico de generación ([SW] 9.1.2, 9.2.1).

## 2. RegistroAlta (`sf:RegistroFacturacionAltaType`)

Cardinalidad tomada literalmente de [XSD]. Formato y listas de [DR] hoja 2.

| Campo | Card. | Formato / valores |
|---|---|---|
| `IDVersion` | 1 | `1.0` (L15) |
| `IDFactura/IDEmisorFactura` | 1 | NIF (9). Debe coincidir con `Cabecera/ObligadoEmision/NIF` ([VAL] 3.1.3.1) |
| `IDFactura/NumSerieFactura` | 1 | Texto (1-60). Solo ASCII 32-126, sin `" ' < > =` ([VAL] 3.1.3.1) |
| `IDFactura/FechaExpedicionFactura` | 1 | `DD-MM-AAAA`. No futura y no anterior al 28/10/2024 ([VAL] 3.1.3.1) |
| `RefExterna` | 0..1 | Texto (60), libre |
| `NombreRazonEmisor` | 1 | Texto (120) |
| `Subsanacion` | 0..1 | `S`/`N` (por defecto `N`) |
| `RechazoPrevio` | 0..1 | `N`/`S`/`X` (L17) |
| `TipoFactura` | 1 | L2: `F1` factura (art. 6, 7.2 y 7.3 ROF) · `F2` simplificada y sin identificación del destinatario (art. 6.1.d) · `F3` emitida en sustitución de simplificadas · `R1` rectificativa (error fundado en derecho y art. 80 Uno, Dos y Seis LIVA) · `R2` (art. 80.3) · `R3` (art. 80.4) · `R4` (resto) · `R5` rectificativa de simplificadas |
| `TipoRectificativa` | 0..1 | `S` por sustitución, `I` por diferencias (L3). Obligatorio si y solo si `R1`-`R5` ([VAL] 3.1.3.3) |
| `FacturasRectificadas/IDFacturaRectificada` | 0..1 / 1..1000 | {IDEmisorFactura, NumSerieFactura, FechaExpedicionFactura}. Solo con `R1`-`R5`, opcional ([VAL] 3.1.3.4) |
| `FacturasSustituidas/IDFacturaSustituida` | 0..1 / 1..1000 | Ídem. Solo con `F3` |
| `ImporteRectificacion` | 0..1 | {`BaseRectificada` 1, `CuotaRectificada` 1, `CuotaRecargoRectificado` 0..1}. Obligatorio si y solo si `TipoRectificativa=S` ([VAL] 3.1.3.6) |
| `FechaOperacion` | 0..1 | `DD-MM-AAAA`, solo si es distinta de la de expedición |
| `DescripcionOperacion` | 1 | Texto (500) |
| `FacturaSimplificadaArt7273` | 0..1 | `S`/`N` |
| `FacturaSinIdentifDestinatarioArt61d` | 0..1 | `S`/`N` |
| `Macrodato` | 0..1 | `S`/`N`. Obligatorio si \|ImporteTotal\| ≥ 100.000.000,00 ([VAL] 3.1.3.10) |
| `EmitidaPorTerceroODestinatario` / `Tercero` | 0..1 / 0..1 | `D`/`T` (L6) |
| `Destinatarios/IDDestinatario` | 0..1 / 1..1000 | {`NombreRazon` 1, elección `NIF` o `IDOtro`{CodigoPais 0..1, IDType L7, ID}}. **Obligatorio** con F1, F3, R1-R4 y **prohibido** con F2 y R5 ([VAL] 3.1.3.13) |
| `Cupon` | 0..1 | `S`/`N`, solo con R1/R5 |
| `Desglose/DetalleDesglose` | 1 / **1..12** | ver tabla siguiente |
| `CuotaTotal` | 1 | Decimal: patrón `(\+\|-)?\d{1,12}(\.\d{0,2})?` |
| `ImporteTotal` | 1 | Ídem |
| `Encadenamiento` | 1 | Elección: `PrimerRegistro` = `S` **o** `RegistroAnterior`{`IDEmisorFactura`, `NumSerieFactura`, `FechaExpedicionFactura`, `Huella`} (todos 1) |
| `SistemaInformatico` | 1 | ver §4 |
| `FechaHoraHusoGenRegistro` | 1 | `xs:dateTime`, p. ej. `2024-01-01T19:20:30+01:00` (ISO 8601 con huso) |
| `NumRegistroAcuerdoFacturacion` / `IdAcuerdoSistemaInformatico` | 0..1 | Texto (15) / (16) |
| `TipoHuella` | 1 | `01` = SHA-256 (L12) |
| `Huella` | 1 | 64 caracteres hexadecimales en mayúsculas |
| `ds:Signature` | 0..1 | No se exige en remisión voluntaria VERI*FACTU ([DR] hoja 2) |

**DetalleDesglose** (`sf:DetalleType`):

| Campo | Card. | Valores |
|---|---|---|
| `Impuesto` | 0..1 | L1: `01` IVA (por defecto), `02` IPSI, `03` IGIC, `05` otros |
| `ClaveRegimen` | 0..1 | L8A (IVA): `01` general, `02` exportación, `03` REBU, `04` oro, `05` agencias de viajes, `06` grupo de entidades avanzado, `07` criterio de caja, `08` IPSI/IGIC, `09`, `10` cobros por cuenta de terceros, `11` arrendamiento de local, `14`, `15`, `17` OSS/IOSS, `18` recargo de equivalencia, `19` REAGYP, `20` simplificado. **Obligatoria si el impuesto es IVA** ([VAL] 15.6) |
| elección `CalificacionOperacion` / `OperacionExenta` | 1 | L9: `S1` sujeta y no exenta sin ISP, `S2` con inversión del sujeto pasivo, `N1` no sujeta (art. 7, 14, otros), `N2` no sujeta por localización · L10: `E1` art. 20, `E2` art. 21, `E3` art. 22, `E4` arts. 23 y 24, `E5` art. 25, `E6` otros (`E7`/`E8` solo IGIC) |
| `TipoImpositivo` | 0..1 | `\d{1,3}(\.\d{0,2})?`. Con IVA y S1: 0; 2; 4; 5; 7,5; 10; 21 (2, 5 y 7,5 solo en fechas acotadas) ([VAL] 15.1) |
| `BaseImponibleOimporteNoSujeto` | 1 | Decimal 12,2 |
| `BaseImponibleACoste` | 0..1 | Solo con clave 06, IPSI u otros |
| `CuotaRepercutida` | 0..1 | Con S1 es obligatoria, con el mismo signo que la base y igual a base × tipo / 100 **± 10,00 €**. Con S2 es 0. Prohibida con N1, N2 y exentas ([VAL] 15.4, 15.5, 15.7) |
| `TipoRecargoEquivalencia` / `CuotaRecargoEquivalencia` | 0..1 | Con 21 % → 5,2 o 1,75; con 10 % → 1,4; con 4 % → 0,5 ([VAL] 15.3) |

Coherencia ([VAL] 16 y 17, solo avisos con margen de ± 10,00 €; no se aplica con las claves 03, 05, 06, 08 y 09):
`CuotaTotal = Σ(CuotaRepercutida + CuotaRecargoEquivalencia)` e
`ImporteTotal = Σ(BaseImponibleOimporteNoSujeto + CuotaRepercutida + CuotaRecargoEquivalencia)`.
Con F2, la suma de base y cuota no puede superar 3.000,00 € (+10 €) ([VAL] 15.8).

## 3. RegistroAnulacion (`sf:RegistroFacturacionAnulacionType`)

`IDVersion` 1 · `IDFactura`{`IDEmisorFacturaAnulada`, `NumSerieFacturaAnulada`, `FechaExpedicionFacturaAnulada`} 1 ·
`RefExterna` 0..1 · `SinRegistroPrevio` 0..1 (`S`/`N`) · `RechazoPrevio` 0..1 (`S`/`N`) · `GeneradoPor` 0..1 (L16: `E`/`D`/`T`) ·
`Generador` 0..1 · `Encadenamiento` 1 · `SistemaInformatico` 1 · `FechaHoraHusoGenRegistro` 1 · `TipoHuella` 1 · `Huella` 1 ([XSD]).
La anulación es solo para facturas expedidas por error cuando no procede rectificativa (por ejemplo, si no hubo
venta real) ([SW] 9.2.1).

## 4. SistemaInformatico (`sf:SistemaInformaticoType`, todos obligatorios en alta y anulación)

`NombreRazon` (120) y elección `NIF` | `IDOtro` del **productor** (`crates/core/src/productor.rs`) · `NombreSistemaInformatico` (30) ·
`IdSistemaInformatico` (2; las dos posiciones siempre rellenas, [VAL] 3.1.5) · `Version` (50) · `NumeroInstalacion` (100,
único por instalación) · `TipoUsoPosibleSoloVerifactu` S/N · `TipoUsoPosibleMultiOT` S/N · `IndicadorMultiplesOT` S/N
(lo calcula el SIF automáticamente según los obligados que gestiona en ese momento) ([DR] hoja 5).

## 5. Huella y encadenamiento

Vectores oficiales y reglas: `fixtures/aeat/verifactu/hash-vectors.json` (3 casos, **verificados localmente**).
- Alta: `IDEmisorFactura=…&NumSerieFactura=…&FechaExpedicionFactura=…&TipoFactura=…&CuotaTotal=…&ImporteTotal=…&Huella=<anterior>&FechaHoraHusoGenRegistro=…`.
- Anulación: `IDEmisorFacturaAnulada=…&NumSerieFacturaAnulada=…&FechaExpedicionFacturaAnulada=…&Huella=<anterior>&FechaHoraHusoGenRegistro=…`.
- Los valores se recortan (*trim*), los campos vacíos quedan como `campo=`, la entrada se codifica en UTF-8 y la salida es
  SHA-256 en hexadecimal con mayúsculas (64 caracteres) ([HASH] págs. 5-9).
- Si la huella está mal, el registro queda **AceptadoConErrores**, no rechazado ([HASH] pág. 13; [VAL] 18).
- Una subsanación o anulación es un registro **nuevo** que se encadena al inmediatamente anterior; el original no se toca ([SW] 9.1.2).

## 6. Mensaje SOAP de remisión

- SOAP 1.1, *document/literal*, UTF-8, HTTPS con certificado electrónico cualificado ([SW] 4.2-4.3).
  Endpoints: `docs/AEAT_INTEGRATION.md` §3 (puertos `SistemaVerifactu*` del WSDL).
- Cuerpo `sfLR:RegFactuSistemaFacturacion` ([XSD] `SuministroLR.xsd`):
  - `Cabecera` 1: `ObligadoEmision`{`NombreRazon`, `NIF`} 1 · `Representante`{`NombreRazon`, `NIF`} 0..1 ·
    `RemisionVoluntaria`{`FechaFinVeriFactu` 0..1, `Incidencia` 0..1 S/N} 0..1 ·
    `RemisionRequerimiento`{`RefRequerimiento` 1, `FinRequerimiento` 0..1} 0..1 (este último no se usa en VERI*FACTU).
  - `RegistroFactura` **1..1000**, cada uno con `RegistroAlta` **o** `RegistroAnulacion` ([SW] pág. 10: "El número máximo de
    registros por envío es de 1.000"; [VAL] 3.1.2).
- Errores de esquema o de cabecera → `SOAPFault` y se rechaza todo el envío. Si llega `faultcode` `soapenv:Server` o no hay
  respuesta, se reenvía. Con `soapenv:Client` hay que corregir antes de reenviar ([SW] 5.1, pág. 28).

## 7. Control de flujo y reintentos

- La respuesta incluye `TiempoEsperaEnvio` (obligatorio, segundos, patrón `\d{0,4}`); el valor inicial es **60 s**.
  El siguiente envío se hace cuando pasen `t` segundos desde el anterior **o** cuando se acumulen 1.000 registros, lo que
  ocurra primero ([OM] art. 16.2; [SW] 6.4.4.1).
- Si una incidencia técnica impide enviar: se reintenta **al menos cada hora**, respetando el orden de generación, y se marca
  `Incidencia=S`. Mientras quede algo pendiente, se avisa al usuario de cuántos registros faltan ([OM] art. 16.4).
- La respuesta trae un `CSV` si se acepta al menos un registro ([OM] art. 16.5).

**Cómo lo hace TaxPain** (`crates/app/src/verifactu_cola.rs`, tabla `verifactu_cola` de la migración 0096):
- Cada respuesta guarda su `TiempoEsperaEnvio`; el siguiente lote no sale antes salvo que haya 1.000 registros
  pendientes. El botón «Enviar a la AEAT» abre una *sesión de remisión*: con un solo Touch ID manda lote tras lote
  (hasta 1.000 registros cada uno) y espera entre ellos lo que pida la AEAT si son menos de 3 minutos; si no,
  el resto queda en cola para el siguiente envío.
- Sin red, sin respuesta, HTTP de error o `SOAP Fault` `Server`: incidencia técnica. Los registros siguen pendientes,
  el reintento espera 60 s, 120 s, 240 s… hasta un máximo de una hora, y el envío siguiente lleva
  `RemisionVoluntaria/Incidencia = S` (validado contra `SuministroLR.xsd`). La incidencia se cierra con la primera
  respuesta de la AEAT, y cada registro enviado así queda marcado (`verifactu_record.incidencia`).
- `SOAP Fault` `Client` (error del mensaje): la cola queda **bloqueada** y no se reintenta sola; «Reintentar» la
  desbloquea (con confirmación y en la auditoría).
- Facturas avisa de cuántos registros quedan sin remitir y desde cuándo hay incidencia (art. 16.4).
- `TODO(VERIFY)`: el art. 16.4 no dice con exactitud qué registros llevan `Incidencia = S`; TaxPain la pone en todos
  los envíos desde el primer fallo técnico hasta la primera respuesta.
- `TODO(VERIFY)`: el envío no es desatendido: cada sesión pide Touch ID para usar el certificado. Remitir con la app
  cerrada o sin el usuario delante exigiría dejar el certificado utilizable sin confirmación, y eso es una decisión
  de seguridad pendiente (D6).

## 8. Respuesta y estados (`RespuestaSuministro.xsd`)

- `EstadoEnvio` (L18): `Correcto` (todos correctos) · `ParcialmenteCorrecto` (algún registro `Incorrecto` o
  `AceptadoConErrores`) · `Incorrecto` (todos incorrectos).
- `RespuestaLinea` 0..1000: `IDFactura`, `Operacion`{`TipoOperacion`, `Subsanacion`, `RechazoPrevio`, `SinRegistroPrevio`},
  `RefExterna`, `EstadoRegistro` (L19: `Correcto` · `AceptadoConErrores` (queda registrado) · `Incorrecto` (no queda registrado)),
  `CodigoErrorRegistro`, `DescripcionErrorRegistro` y `RegistroDuplicado`{`EstadoRegistroDuplicado`: `Correcta`,
  `AceptadaConErrores`, `Anulada`} ([SW] 6.5.2).
- Lista de códigos de error: [VAL] 4.4. `TODO(VERIFY)`: falta vendorizar la tabla de códigos en un fixture.
- TaxPain trata una línea con `RegistroDuplicado` como remitida con el estado que consta en la AEAT (`Correcta` →
  aceptado, `AceptadaConErrores` → aceptado con errores; `Anulada` → aceptada si lo enviado era la anulación y
  rechazada si era el alta). Pasa al reenviar tras un corte en el que la AEAT sí recibió el envío.
- Un registro sin línea en la respuesta sigue pendiente y se reenvía **una** vez; si tampoco entonces hay línea, sale
  de la cola como «sin respuesta» (en la base de datos sigue `pendiente`, con `sin_linea = 2`) para consultarlo en
  la Sede antes de volver a enviarlo.

## 9. Subsanación y rechazo previo ([SW] 9; [VAL] 3.1.3.2; [DR] L17)

| Caso | Subsanacion | RechazoPrevio | Condición |
|---|---|---|---|
| Alta normal | N u omitido | N u omitido | La clave no existe |
| Subsanar un alta aceptada (o aceptada con errores) sin que proceda rectificativa | `S` | N u omitido | El registro ya existe en la AEAT |
| Reenviar un alta que fue **rechazada** | `S` | `X` | La clave no existe en la AEAT ([SW] ejemplo 9.1.3.1) |
| Subsanar de nuevo tras el **rechazo de una subsanación** | `S` | `S` | La clave existe en la AEAT ([VAL] anexo 6.1, pág. 27) |
| Subsanar un registro que **nunca se envió** (p. ej., al pasar de NO VERI*FACTU a VERI*FACTU) | `S` | `X` | [VAL] anexo 6.1 y L17 |
| Anulación normal | — | N u omitido (y `SinRegistroPrevio` N) | El registro existe |
| Anulación tras rechazo de una anulación | — | `S` | — |
| Anulación de un registro que no está en la AEAT | — | — (`SinRegistroPrevio=S`) | — |

No se admiten `Subsanacion=N` con `RechazoPrevio=S` ni con `X`. Los registros `Incorrecto` y los `AceptadoConErrores` deben
subsanarse y reenviarse, salvo que proceda emitir una rectificativa o anular la factura ([SW] págs. 11-12).

## 10. Factura impresa: QR y leyenda

- Contenido obligatorio ([ROF] art. 6.5 y 7.5; [OM] art. 20): un código QR y, solo en VERI*FACTU, la frase
  **«Factura verificable en la sede electrónica de la AEAT»** o **«VERI*FACTU»**, con letra igual o mayor que la del resto de la factura.
- QR ([OM] art. 21; [QR] §2-3): ISO/IEC 18004:2015, corrección de errores **M**, tamaño **entre 30×30 y 40×40 mm**,
  margen en blanco ≥ 2 mm (se recomiendan 6 mm), encima el texto **«QR tributario:»** y debajo la frase VERI*FACTU.
  Va al principio de la factura, como primer QR, y una sola vez en la primera página: arriba y centrado (o arriba a la
  izquierda) en vertical, y a la izquierda en apaisado.
- En una factura electrónica estructurada se incluye la URL del QR como campo, sin la imagen ([OM] art. 20.2).
- Mientras TaxPain no envíe a producción, una factura de la empresa real **no** lleva QR ni leyenda (no es verificable
  en la Sede y el QR apuntaría a prewww2.aeat.es): sólo los llevan cuando se remite a producción
  (`verifactu_cola::remite_a_produccion`: interruptor de compilación `productor::PRODUCCION_HABILITADA`, datos del
  productor completos y `verifactu.production` en la empresa) —y entonces **todas** las facturas emitidas— o en la
  empresa del banco de pruebas (`verifactu_lab`). VERI*FACTU viene desactivado al dar de alta una empresa
  (FACTURACION-01).
- Tamaño y posición en el PDF (`crates/invoicing/src/pdf.rs`): «QR tributario:» encima, símbolo de 32 × 32 mm con
  3,5 mm de silencio, arriba a la izquierda de la primera página, y la leyenda debajo en 9 pt negrita (la letra del
  cuerpo de la factura es de 9 pt). Cotejado con el art. 21 de [OM] tal como se resume arriba; `TODO(VERIFY)` con el
  texto del BOE si la «zona de silencio» de 2 mm se mide desde el símbolo o desde el borde.
- La huella **no** va en el QR ([FAQ] huella-hash). URL, parámetros y codificación: `fixtures/aeat/verifactu/qr-vectors.json`.

## 11. Obligaciones del fabricante: declaración responsable

[RRSIF] art. 13 y [OM] art. 15. La declaración se hace por escrito, es visible dentro del propio SIF en cada versión y se
entrega al cliente y al comercializador. El productor conserva las de todas las versiones. Confirmado el 26-10-2026 en el
texto consolidado del RD 1007/2023, art. 13.2: debe constar «de modo visible en el propio sistema informático en cada una de
sus versiones», así que es una pantalla obligatoria de la app, no sólo un documento (ver CONSTRUCCION-WEB-PRUEBAS-08 en
`docs/AUDITORIA.md`). Empieza con el título
**«DECLARACIÓN RESPONSABLE DEL SISTEMA INFORMÁTICO DE FACTURACIÓN»** y lleva, en este orden y con el texto descriptivo
delante de cada dato:
a) nombre del SIF; b) código identificador (= `IdSistemaInformatico`); c) versión completa; d) componentes hardware y
software, con una descripción breve y sus funcionalidades; e) si solo puede funcionar como VERI*FACTU; f) si admite varios
obligados tributarios; g) tipos de firma (solo para uso no VERI*FACTU); h) nombre o razón social del productor; i) NIF del
productor; j) dirección postal completa; k) manifestación de cumplimiento del art. 29.2.j) LGT, el RRSIF, la OM y las
especificaciones de la sede; l) fecha (día, mes y año) y lugar (localidad y país) de la firma.
Anexo recomendado: otros contactos, URLs, explicación detallada del cumplimiento y otra información.
Hay ejemplos en `SEDE/.../informacion-tecnica/ejemplo-declaracion-responsable.html` (no se han usado como fuente normativa).

**En TaxPain**: `crates/app/src/declaracion_responsable.rs` la genera con los datos de `crates/core/src/productor.rs`
y del bloque `SistemaInformatico` de los registros (nombre, `IdSistemaInformatico`, versión, «sólo VERI*FACTU»,
«varios obligados»), así que no puede contradecirlos. Se ve en Ajustes › Acerca de TaxPain y se guarda en PDF
(`declaracion-responsable-TaxPain-<versión>.pdf`) para que la firme quien distribuya TaxPain. Lo que no está en el
código (la dirección postal del productor, `productor::DIRECCION`, y la fecha y el lugar de la firma) sale entre
corchetes; sin la dirección, el envío a producción sigue cerrado (`productor::faltan_datos`).
`TODO(VERIFY)`: TaxPain lleva varias empresas en una instalación y hoy declara `TipoUsoPosibleMultiOT = N`; hay que
confirmar si eso es «uso por varios obligados» (apartado f) y, si lo es, cambiarlo en los registros y en la
declaración a la vez.

## 12. Conservación

- Los SIF VERI*FACTU y sus usuarios **no están obligados** a conservar los registros de facturación ni a ofrecer su
  exportación, porque ya están en poder de la AEAT ([FAQ] conservación, accesibilidad y legibilidad).
- Las **facturas** sí se conservan durante el plazo de la LGT ([ROF] art. 19.1). Según la [FAQ], el plazo va de un mínimo
  de 4 años desde el fin del plazo de declaración hasta más de 10 años en algunos casos.
- Recomendación para TaxPain (local-first): guardar igualmente los registros, el XML enviado, la respuesta y el CSV.

## 12 ter. Modalidad «no VERI*FACTU» (sin remisión): qué falta

TaxPain se declara **sólo VERI*FACTU** (`TipoUsoPosibleSoloVerifactu = S`). Un SIF que no remite tiene que, además de
lo que ya hace TaxPain (registro de alta y anulación encadenados con huella, inalterables por triggers):
- firmar cada registro de facturación con firma electrónica XAdES ([RRSIF] art. 12; [OM] arts. 7-9), con el
  certificado del obligado o del productor;
- llevar el **registro de eventos** (`EventosSIF.xsd`): inicio y fin del uso, detección de anomalías en la integridad,
  trazabilidad y fechas de los registros y de los propios eventos, exportaciones, restauraciones, el resumen
  periódico de eventos…, también encadenado y firmado ([RRSIF] art. 9; [OM] arts. 10-12);
- conservar los registros y eventos y poder **exportarlos** ([RRSIF] art. 8.2.e-f; [OM] art. 13), y remitirlos si
  la AEAT los requiere (`RemisionRequerimiento`);
- imprimir el QR hacia `ValidarQRNoVerifactu` y **sin** la leyenda ([OM] art. 20).

Nada de eso existe todavía (la auditoría de la app es un registro encadenado, pero no es el registro de eventos del
[OM], ni va firmado). Por eso, con VERI*FACTU desactivado TaxPain no genera registros y lo avisa en Facturas con la
fecha a partir de la cual eso deja de ser conforme (1/1/2027 las sociedades, 1/7/2027 el resto, RDL 15/2025;
`verifactu_cola::aviso_sin_verifactu`). `TODO(VERIFY)`: quien lleva el SII queda fuera del reglamento y TaxPain no
lo sabe.

## 12 bis. Consulta de registros (`ConsultaFactuSistemaFacturacion`)

Mismo servicio SOAP que el envío (`fixtures/aeat/verifactu/ConsultaLR.xsd` y
`RespuestaConsultaLR.xsd`). Filtro obligatorio: `PeriodoImputacion` (ejercicio y mes, `01`…`12`,
en el espacio de nombres de `ConsultaLR`, con `sf:Ejercicio`/`sf:Periodo` dentro); opcionales
`NumSerieFactura` y `ClavePaginacion` (emisor, número y fecha del último registro recibido) si
`IndicadorPaginacion` es `S`. Cada registro trae `EstadoRegistro` (`Correcto`,
`AceptadoConErrores`, `Anulado`), el código de error si lo hay y la `Huella`. TaxPain la usa en el
banco de pruebas (`crates/app/src/verifactu_lab.rs`) para comparar estado y huella con lo que
generó; la petición se valida contra el XSD en `crates/invoicing/tests/verifactu.rs`.

## 13. Pendiente

- `TODO(VERIFY)`: falta llevar a un fixture la tabla completa de códigos de error ([VAL] 4.4).
- `TODO(VERIFY)`: el XSD admite `ClaveRegimen` `21` y `OperacionExenta` `E7`/`E8`, que no están en las listas L8A y L10 del
  [DR] v1.0. Según [VAL] son exclusivas del IGIC.
- Margen de tiempo aceptado en `FechaHoraHusoGenRegistro` (acotado el 26-10-2026 con [VAL] v1.2.2 §20, §6 y §4.3.1): la
  única validación es que sea «menor o igual que la fecha del sistema de la AEAT, admitiéndose un margen de error»; si se
  supera, aviso (no rechazo), excepcionado de la subsanación. No se exige huso horario español: el `+00:00` (UTC) que envía
  la app es válido. `TODO(VERIFY)`: el valor numérico del margen no se publica.

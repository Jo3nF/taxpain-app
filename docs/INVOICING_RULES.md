# Reglas de facturación (España, régimen común)

> Investigado el **2026-10-13** con los textos consolidados del BOE descargados ese día (solo GET). `TODO(VERIFY)` = no está literalmente en una fuente oficial. **No usar en código sin verificar.**

**Fuentes**
- `[ROF]` RD 1619/2012, Reglamento de facturación, consolidado (actualización publicada el 31/03/2026): https://www.boe.es/buscar/act.php?id=BOE-A-2012-14696
- `[LIVA]` Ley 37/1992 del IVA, consolidada (actualización publicada el 07/10/2026): https://www.boe.es/buscar/act.php?id=BOE-A-1992-28740
- `[LIRPF]` Ley 35/2006, consolidada (07/10/2026): https://www.boe.es/buscar/act.php?id=BOE-A-2006-20764
- `[RIRPF]` RD 439/2007, consolidado (07/10/2026): https://www.boe.es/buscar/act.php?id=BOE-A-2007-6820
- `[VAL]` AEAT, `Validaciones_Errores_Veri-Factu.pdf` v1.2.2 · `[DR]` `DsRegistroVeriFactu.xlsx` (ver `docs/VERIFACTU.md`)

## 1. Factura completa: contenido obligatorio ([ROF] art. 6.1)

| Letra | Dato |
|---|---|
| a) | **Número y, en su caso, serie**. La numeración dentro de cada serie es **correlativa** |
| b) | Fecha de expedición |
| c) | Nombre y apellidos o razón social completa del expedidor **y** del destinatario |
| d) | NIF del expedidor. El NIF del destinatario es obligatorio en: 1.º entregas intracomunitarias exentas (art. 25 LIVA); 2.º cuando el destinatario es el sujeto pasivo (inversión); 3.º operaciones localizadas en el territorio con un expedidor establecido en él |
| e) | Domicilio del expedidor y del destinatario (y del establecimiento, si es relevante para la tributación) |
| f) | Descripción de las operaciones con los datos para determinar la base (arts. 78 y 79 LIVA) y su importe, **incluido el precio unitario sin impuesto** y cualquier descuento no incluido en él |
| g) | Tipo o tipos impositivos |
| h) | Cuota repercutida, **consignada por separado** |
| i) | Fecha de la operación o del pago anticipado, si es distinta de la de expedición |
| j) | Si la operación está **exenta**: referencia a la Directiva 2006/112/CE o a la LIVA, o "indicación de que la operación está exenta". También si solo lo está una parte de la factura |
| k) | Medios de transporte nuevos (art. 25 LIVA): características, fecha de primera puesta en servicio y distancias u horas |
| l) | «facturación por el destinatario» |
| m) | **«inversión del sujeto pasivo»** cuando el sujeto pasivo es el destinatario |
| n) / o) / p) | «régimen especial de las agencias de viajes» / «régimen especial de los bienes usados» (o de objetos de arte, antigüedades y objetos de colección) / «régimen especial del criterio de caja» |

- Bases separadas en una misma factura ([ROF] art. 6.2) para: operaciones exentas y no exentas, operaciones con y sin
  inversión del sujeto pasivo, y operaciones con distintos tipos de IVA.
- Con un SIF (RD 1007/2023) se añaden el **QR** y, en VERI*FACTU, la frase «Factura verificable en la sede electrónica de
  la AEAT» o «VERI*FACTU» ([ROF] art. 6.5 y 7.5). Detalle en `docs/VERIFACTU.md` §10.

## 2. Numeración y series ([ROF] art. 6.1.a y 7.1.a)

- La numeración es correlativa dentro de cada serie. Se pueden usar series separadas si hay razones que lo justifiquen
  (varios establecimientos, operaciones de distinta naturaleza…).
- **Series específicas obligatorias**: 1.º las expedidas por el destinatario o por un tercero (una serie por cada uno);
  **2.º las rectificativas**; 3.º las de la disposición adicional 5.ª del RIVA; 4.º las del art. 84.Uno.2.º g) LIVA; 5.º las
  del art. 61 quinquies.2 RIVA.
- Si en un mismo año natural se expiden facturas completas (art. 6) y simplificadas (art. 7), **van en series separadas**
  ([ROF] art. 7.1.a, último párrafo). Confirmado literalmente el 26-10-2026 en el texto consolidado (arts. 6.1.a) 2.º y
  7.1.a): «será obligatoria la expedición mediante series separadas de unas y otras»). TaxPain separa las
  rectificativas y las simplificadas: cada una en su serie de los ajustes (por defecto «R» y «S»); `issue()` rechaza
  una factura en una serie de otra clase (FACTURACION-03). Las rectificativas de simplificadas (R5) son simplificadas
  (art. 7), así que no pueden compartir serie con las rectificativas de completas (art. 7.1.a, último párrafo) y van en
  su propia serie, separada también de las simplificadas ordinarias (art. 7.1.a 4.º): por defecto «RS»
  (`simplified_rectifying_series`; resuelto el 27-10-2026 con el texto consolidado).
- VERI*FACTU: `NumSerieFactura` (serie y número juntos) tiene como máximo 60 caracteres ASCII 32-126, sin `" ' < > =`. La
  clave única es NIF + NumSerieFactura + FechaExpedicionFactura ([VAL] 3.1.3.1). Ver la FAQ de desarrolladores, apartado 6,
  sobre la prohibición de numeración duplicada: `TODO(VERIFY)` (no se ha leído en detalle).

## 3. Facturas simplificadas ([ROF] art. 4 y 7)

- **Límites** (art. 4): **≤ 400 €, IVA incluido**, en cualquier operación, y siempre que se expida una rectificativa
  (art. 4.1). **≤ 3.000 €, IVA incluido**, solo en las operaciones del art. 4.2: venta al por menor, ambulancia, a domicilio,
  transporte de personas, hostelería y restauración, discotecas, cabinas telefónicas, peluquería, instalaciones deportivas,
  fotografía, aparcamiento, alquiler de películas, tintorería y peajes.
- **Prohibidas** (art. 4.4): entregas intracomunitarias (art. 25 LIVA), ventas a distancia del art. 68.Tres.a) (salvo
  OSS), operaciones de no establecidos con inversión del sujeto pasivo facturadas por el destinatario, y art. 2.3.b) b').
- Límites y prohibiciones confirmados el 26-10-2026 en el texto consolidado (actualización del 31/03/2026). El límite que
  aplica el código (`crates/core/src/invoice/model.rs`): ver FACTURACION-04 en `docs/AUDITORIA.md`.
- **Contenido** (art. 7.1): a) número y serie correlativa; b) fecha de expedición; c) fecha de la operación si es distinta;
  d) NIF y nombre o razón social del expedidor; e) tipo de bienes o servicios; f) tipo impositivo y, opcionalmente, «IVA
  incluido» (con varios tipos, la base de cada uno por separado); g) contraprestación total; h) en las rectificativas, la
  referencia a la factura rectificada; i) las menciones de las letras j) a p) del art. 6.1.
- Si el destinatario empresario lo pide, se añaden su NIF y domicilio y la cuota por separado (art. 7.2). Esto también
  se aplica a un particular que lo necesite para ejercer un derecho tributario (art. 7.3).
- VERI*FACTU: simplificada = `F2` sin `Destinatarios`; la suma de base y cuota no puede pasar de 3.000 € (+10 €)
  ([VAL] 15.8). Con art. 7.2/7.3 = `F1` con `FacturaSimplificadaArt7273=S` ([DR] L2). La factura que sustituye a
  simplificadas = `F3`, que **no es rectificativa** ([ROF] art. 15.6).

## 4. Facturas rectificativas ([ROF] art. 15)

- **Causas**: la factura original incumple los arts. 6 o 7 (15.1); cuotas mal calculadas o supuestos del art. 80 LIVA de
  modificación de la base (15.2). Una devolución de mercancías o envases a un mismo destinatario puede descontarse en la
  factura del siguiente suministro si el tipo es el mismo.
- **Plazo**: en cuanto se tenga constancia, siempre que no hayan pasado **4 años** desde el devengo o desde la circunstancia
  del art. 80 (15.3; igual en [LIVA] art. 89.Uno).
- **Contenido**: una nueva factura con los datos de la factura o facturas rectificadas (se pueden agrupar varias). En los
  descuentos por volumen basta con indicar el periodo (15.4). Cumple los arts. 6 o 7. Las letras f), g) y h) expresan la
  rectificación: **o bien el importe de la rectificación, o bien cómo quedan tras ella señalando el importe rectificado** (15.5).
  El art. 15.5 vigente **no** exige la mención «condición de documento rectificativo y descripción de la causa» (estaba en el
  RD 1496/2003), aunque la causa sí decide el código R1–R4 de VERI*FACTU (comprobado el 26-10-2026).
- **Serie propia obligatoria** para las rectificativas de completas (art. 6.1.a 2.º). Las de simplificadas (R5) van
  separadas de las completas (art. 7.1.a, último párrafo) y en serie propia (art. 7.1.a 4.º): «RS» por defecto.
- **Códigos VERI*FACTU** ([DR] L2): `R1` error fundado en derecho y art. 80 Uno, Dos y Seis LIVA (descuentos, devoluciones,
  operaciones que quedan sin efecto o cambio de precio) · `R2` art. 80.Tres (**concurso** del destinatario) · `R3` art. 80.Cuatro
  (**créditos incobrables**) · `R4` resto (`TODO(VERIFY)`: se supone que cubre los errores de los arts. 6 y 7 de la causa 15.1) · `R5` rectificativa de **simplificada**.
  `TipoRectificativa`: `S` (sustitución; exige `ImporteRectificacion`) o `I` (por diferencias). Ver `docs/VERIFACTU.md` §2.

## 5. Plazos de expedición ([ROF] art. 11 y 13)

- Regla general: **en el momento de realizar la operación**.
- Destinatario empresario o profesional: **antes del día 16 del mes siguiente** al devengo. Lo mismo en entregas
  intracomunitarias (art. 75.Uno.8.º LIVA) y en criterio de caja con destinatario empresario.
- Facturas recapitulativas (art. 13): operaciones de un mismo mes natural y un mismo destinatario. Se expiden como máximo
  el último día del mes, o antes del día 16 del mes siguiente si el destinatario es empresario.

## 6. Moneda, lengua y duplicados

- **[ROF] art. 12** (no art. 10, que trata de la factura electrónica): cualquier moneda, pero la **cuota en euros** con el tipo
  de cambio del art. 79.Once LIVA; cualquier lengua, aunque la AEAT puede pedir traducción.
- **Art. 79.Once LIVA** (texto consolidado del BOE, contrastado el 24/10/2026): «En las operaciones cuya contraprestación se
  hubiese fijado en moneda o divisa distintas de las españolas, se aplicará el tipo de cambio vendedor, fijado por el Banco de
  España, que esté vigente en el momento del devengo.» Los **cambios oficiales del Banco de España son los tipos de referencia
  del BCE**: el Banco de España publica cada día hábil en el BOE la resolución «por la que se publican los cambios del euro
  correspondientes al día …, publicados por el Banco Central Europeo, que tendrán la consideración de cambios oficiales, de
  acuerdo con lo dispuesto en el artículo 36 de la Ley 46/1998». La Directiva 2006/112/CE (art. 91.2) admite también el último
  tipo publicado por el BCE.
- **Cómo lo aplica TaxPain** (`crates/core/src/fx.rs`, `crates/app/src/invoicing.rs`):
  - Tipo como lo publica el BCE (1 € = 1,1193 USD), con 6 decimales y sin coma flotante; euros = importe ÷ tipo, al céntimo y
    la mitad alejándose de cero (como el resto de importes).
  - Se convierte la **base de cada grupo de IVA** y la cuota en euros es esa base por el tipo (art. 79.Once convierte la base;
    art. 12 pide la cuota en euros). Las líneas en euros (para el asiento y los anticipos de la 438) son la base de cada línea
    convertida, con el redondeo del grupo en la de mayor importe.
  - El tipo es el **vigente al devengo**: el de la fecha de la operación (o de expedición) o el último publicado antes (fines
    de semana y festivos de TARGET). La emisión no admite uno posterior ni de más de 7 días antes. Simplificación: se toma el
    tipo del día aunque el devengo sea antes de su publicación (16:00); el usuario puede escribir otro.
  - Todo lo que cuenta va en euros (asiento, 303/390/349/347/130, VERI*FACTU —su esquema no tiene moneda—, cartera); el PDF va
    en la moneda con la base, el IVA y el total en euros, el tipo y su fecha; Facturae con `InvoiceCurrencyCode`,
    `ExchangeRateDetails` y `EquivalentInEuros`; UBL con `TaxCurrencyCode` = EUR, `TaxExchangeRate` y la cuota en euros (BR-53).
  - Al cobrar o pagar por otros euros, la diferencia va a **768/668** (PGC, NRV 11.ª «Moneda extranjera»).
  - `TODO(VERIFY)`: qué tipo aplica a la rectificativa de una factura en moneda extranjera (el de la original o el del día; hoy
    sólo se exige que no sea posterior); sentido de `ExchangeRate` en Facturae (se usa el del BCE); la valoración al cierre de
    los saldos en moneda extranjera pendientes (NRV 11.ª 2.1.2) no se hace aún.
- Duplicados ([ROF] art. 14): solo con varios destinatarios o por pérdida del original, con la mención «duplicado».

## 7. Conservación ([ROF] art. 19)

- Se conservan **durante el plazo previsto en la LGT** las facturas recibidas, las copias de las expedidas y los justificantes.
  El ROF **no dice literalmente "4 años"**: se remite a la LGT. La FAQ VERI*FACTU de la AEAT habla de un mínimo de 4 años
  desde el fin del plazo de declaración, y más de 10 años en algunos casos. `TODO(VERIFY)`: citar el art. 66 y siguientes
  de la LGT (Ley 58/2003) y el art. 70 LGT.

## 8. Tipos de IVA y recargo de equivalencia

| Tipo | Base legal | Nota |
|---|---|---|
| **21 %** general | [LIVA] art. 90.Uno | "El tipo impositivo aplicable a cada operación será el vigente en el momento del devengo" (90.Dos) |
| **10 %** reducido | [LIVA] art. 91.Uno | — |
| **4 %** superreducido | [LIVA] art. 91.Dos | — |
| **0 %** | No vigente en 2026 | VERI*FACTU admite `TipoImpositivo=0` ([VAL] 15.1), pero el 0 % no figura como tipo general en los arts. 90-91 LIVA (fue temporal en 2023-2024). Confirmado el 26-10-2026: en 2026 rigen 21/10/4 y el DR 303 2026 v1.01 fija a «00000» los tipos de las filas [150]-[155] y [165]-[167] (no hay 0/2/5/7,5 %); las rebajas de alimentación acabaron el 31/12/2024 (RDL 4/2024). El RDL 7/2026, de 20 de marzo, bajó al 10 % (tipo ya existente) electricidad, gas, pellets, leña y carburantes desde el 22/03/2026, con fin ligado al IPC: `TODO(VERIFY)` las fechas de fin en el BOE (según resúmenes profesionales, luz y gas volvieron al 21 % el 1/06/2026, carburantes hasta el 30/06/2026, y los RDL 10/2026 y 18/2026 añadieron salvaguardas para luz y gas hasta septiembre). No hay tipos nuevos que mapear; las recibidas de energía de 2026 deben admitir el 10 %. Para exentas usar `OperacionExenta`, no tipo 0 |

Recargo de equivalencia ([LIVA] art. 161): **5,2 %** general; **1,4 %** para entregas al 10 %; **0,50 %** para entregas al
4 %; **1,75 %** para el tabaco. El vendedor expide **facturas separadas** para las entregas con recargo e indica el tipo y el
importe del recargo ([ROF] art. 16.4). VERI*FACTU: `TipoRecargoEquivalencia` y `CuotaRecargoEquivalencia` en la línea de
desglose (21 % → 5,2 o 1,75; 10 % → 1,4; 4 % → 0,5) ([VAL] 15.3). Clave de régimen `18` «Recargo de equivalencia» (L8A).
Qué clave usa el **vendedor** cuando repercute el recargo a un minorista (resuelto el 26-10-2026): la AEAT eliminó la
restricción de la clave 18 (revisión 1.0.7 de [VAL], 26/05/2025: «Eliminación en 15.6.9 ClaveRegimen 18 de la restricción de
uso de la clave de régimen 18 para informar datos sobre el recargo de equivalencia»; en 1.2.x la sección desaparece) y la
§15.3 sólo exige `Impuesto` 01 y `CalificacionOperacion` S1 con los tipos admitidos. La clave `01` con
`TipoRecargoEquivalencia`/`CuotaRecargoEquivalencia`, como hace TaxPain, es admisible.

## 9. Retención de IRPF (profesionales)

- **15 %** sobre los ingresos íntegros de actividades profesionales ([LIRPF] art. 101.5.a; [RIRPF] art. 95.1).
- **7 %** en el año de inicio de actividad y los dos siguientes, si no se ejerció ninguna actividad profesional el año
  anterior. El contribuyente debe comunicarlo al pagador, que conserva la comunicación firmada ([RIRPF] art. 95.1, 2.º y 3.er
  párrafos). También al 7 %: recaudadores municipales, mediadores de seguros con auxiliares, delegados de SELAE y ciertos
  epígrafes artísticos del IAE (art. 95.1). En Ceuta y Melilla (deducción del art. 68.4), los porcentajes se reducen un 60 % ([LIRPF] art. 101.5.a).
- El ROF **no** exige reflejar la retención en la factura, y VERI*FACTU no tiene campo de retención (`ImporteTotal` no la
  descuenta según la fórmula de [VAL] 17). `TODO(VERIFY)`: práctica recomendada por la AEAT para mostrar «Retención IRPF»
  y el líquido a percibir; FAQ de desarrolladores, apartado 20 «Concepto importe total».

## 10. Exenciones e inversión del sujeto pasivo

- Exenta: mención obligatoria de la norma (Directiva o LIVA) o la frase de que está exenta ([ROF] art. 6.1.j). VERI*FACTU:
  `OperacionExenta` `E1` art. 20, `E2` art. 21, `E3` art. 22, `E4` arts. 23-24, `E5` art. 25 y `E6` otros.
- Inversión: mención «inversión del sujeto pasivo» ([ROF] art. 6.1.m), NIF del destinatario obligatorio (6.1.d 2.º) y base
  separada (6.2.b). VERI*FACTU: `CalificacionOperacion=S2` con `TipoImpositivo=0` y `CuotaRepercutida=0` ([VAL] 15.4,
  v1.2.2: «no se admite que vaya vacío o que el campo no exista»; S2 sólo cabe con F1, F3 y R1–R4; confirmado el 26-10-2026).

## 11. Redondeo de la cuota de IVA (por línea o por factura)

- **No se ha encontrado una regla oficial literal.** Ni el ROF ni los arts. 78-91 LIVA fijan un criterio de redondeo; el ROF
  exige el tipo y la cuota "por separado" (art. 6.1.g/h) y la base separada por tipo (art. 6.2.c). Esto sugiere calcular la
  cuota **por tipo impositivo sobre la base total**, pero es una inferencia (que el TEAC respalda, ver abajo): `TODO(VERIFY)`.
- VERI*FACTU tolera **± 10,00 €** entre `CuotaRepercutida` y base × tipo / 100 por línea de desglose, y entre los totales y
  la suma de líneas. El error se marca como aviso, no como rechazo ([VAL] 15.7, 16 y 17). Las diferencias de céntimos por
  redondeo no causan rechazo.
- Resolución del **TEAC RG 2233/2022, de 26/11/2024** (leída el 26-10-2026 en la reseña y el texto de
  fiscal-impuestos.com/node/38627; precios con IVA incluido, redondeo por unidad frente al total): rechaza el redondeo
  unidad a unidad cuando distorsiona la cuota y sostiene que «el redondeo debería realizarse al final de la autoliquidación
  del IVA», sobre la cuota agregada; avala el redondeo global frente al de cada artículo. El modo por defecto de TaxPain
  (por tipo: agrupar bases y redondear la cuota del grupo) es coherente con ese criterio; por línea queda como opción.
  `TODO(VERIFY)`: es un criterio aún no reiterado (no vincula como doctrina del art. 239 LGT); falta leerlo en DYCTEAC
  (serviciostelematicosext.hacienda.gob.es) y buscar consultas vinculantes de la DGT sobre "redondeo".
- Propuesta para TaxPain (decisión de producto, no norma): agrupar las líneas por (impuesto, tipo, recargo), sumar las bases
  con precisión decimal exacta, redondear la base y la cuota de cada grupo a 2 decimales (mitad hacia arriba) y emitir un
  `DetalleDesglose` por grupo. Registrar el criterio en `docs/DECISIONS.md`.

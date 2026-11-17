# Anexo I – Contenido y estructura de los campos de la factura electrónica conforme al modelo UBL y la EN 16931

## Fuente

| | |
|---|---|
| Título | ANEXO I – CONTENIDO Y ESTRUCTURA DE LOS CAMPOS DE LA FACTURA ELECTRÓNICA CONFORME AL MODELO UBL Y LA EN 16931 |
| Norma | Orden HAC/1028/2026 (proyecto de Orden Ministerial de factura electrónica; Solución Pública de Facturación Electrónica, SPFE) |
| Emisor | Agencia Estatal de Administración Tributaria (AEAT) |
| Fichero | `Anexo-1-Proyecto-OM-FE.pdf` (682 077 bytes; generado con Microsoft Word LTSC el 2026-10-09) |
| Páginas | 34 (A4 apaisado) |

### Resumen

El anexo fija el contenido mínimo que deben tener las facturas electrónicas y copias fieles que se transmitan a la AEAT (SPFE), en los términos de los artículos 7.1, 11.2 y 11.3 del Real Decreto 238/2026, de 25 de marzo (que desarrolla la facturación electrónica obligatoria entre empresarios y profesionales y modifica el Reglamento de facturación, RD 1619/2012). Define dos mensajes: el **mensaje de alta** (documento UBL `Invoice`) y el **mensaje de anulación** (documento UBL `ApplicationResponse` con `ResponseCode` = `CANCELINVOICE`). Cada campo se identifica con su término de negocio de la EN 16931 (BT/BG) o, cuando no existe en la norma europea, con un código español (BT-ES/BG-ES), su nivel, cardinalidad, descripción, tipo semántico y, cuando procede, la ruta UBL. Al final se definen las listas de valores L1 a L10 (tipo de factura, rectificativas, claves de régimen IVA/IGIC/IPSI, categorías de impuesto, tipos de retención, situación del inmueble, cargos/minoraciones en nombre de terceros, propiedades de línea, impuestos no IVA y documentos justificativos adicionales).

### Convenciones del anexo (transcritas)

- **ID**: identificador del elemento (BT, término de negocio) o grupo (BG, grupo de términos de negocio) según la EN 16931. Los campos no definidos expresamente en la EN 16931 se identifican con los códigos **BT-ES** y **BG-ES**.
- **Nivel**: `+` primer nivel; `++` segundo nivel (forma parte de un grupo de primer nivel); `+++` tercer nivel; `++++` cuarto nivel.
- **Cardinalidad**: `1..1` obligatorio no repetitivo; `1..n` obligatorio repetitivo; `0..1` condicional no repetitivo; `0..n` condicional repetitivo.
- **Término de negocio**: nombre del elemento o grupo en el modelo.
- **Descripción**: significado semántico.
- **Tipo semántico de dato**: formato del dato.
- **Ruta UBL**: expresión que identifica un elemento o atributo dentro de la estructura XML UBL (en su última versión aprobada). Sólo aparece en los campos sin término de negocio definido en la EN 16931 y en aquellos cuyo uso implica informar con un valor determinado campos sin término de negocio definido en la EN 16931 no incluidos expresamente en el anexo I.

Notas de transcripción:
- Las rutas UBL se copian literalmente (incluido el prefijo `in:` para el elemento raíz `Invoice`, las mayúsculas/minúsculas y los espacios o barras dobles que aparecen en el original). Las rutas que en el PDF aparecen partidas en varias líneas se han unido.
- En los campos sin ruta UBL en el anexo la columna queda en blanco: se aplica el *binding* UBL estándar de la EN 16931 (CEN/TS 16931-3-2), que el anexo no reproduce.

---

## 1. FACTURA ELECTRÓNICA O COPIA FIEL – Mensaje de alta (`/Invoice/`)

| ID | Nivel | Card. | Término de negocio | Descripción | Tipo | Ruta UBL |
|---|---|---|---|---|---|---|
| BT-1 | + | 1..1 | Serie y número de la factura | Serie y número que identifican la factura emitida. | Identificador | |
| BT-ES-1 | + | 0..1 | Indicador de copia | Adoptará alguno de los valores: `True`: copia. `False` (o sin informar): original. | Indicador | `/in:Invoice/cbc:CopyIndicator` |
| BT-2 | + | 1..1 | Fecha de expedición de la factura | Fecha en la que se ha expedido la factura. | Fecha | |
| BT-166 | + | 0..1 | Hora de expedición de la factura | Hora en la que se ha expedido la factura. | Hora | |
| BT-3 | + | 1..1 | Código de tipo de factura | Las facturas comerciales se definen de acuerdo con los códigos de UNTDID 1001. Sólo podrán adoptarse los valores de la lista **L1**. Las facturas emitidas con valores **384** y **471** incorporarán el grupo de tipo de factura rectificativa (BG-24), el grupo de modalidad de factura rectificativa (BG-24) y, en su caso, hasta tres grupos de importes rectificados (BG-24). | Código (L1) | |
| BT-5 | + | 1..1 | Código de moneda de la factura | Moneda en la que se expresan todos los importes de la factura, excepto aquellos campos en los que se indica expresamente que deben expresarse en euros. Códigos de la norma ISO 4217. | Código | |
| BT-6 | + | 0..1 | Código de moneda de contabilización del IVA | Cuando la moneda de la factura no sea el euro, se consignará obligatoriamente como moneda de contabilización del IVA el euro. Valor obligatorio: `EUR`. | Código | |
| BT-167 | + | 0..1 | Tipo de cambio de contabilización del IVA | Tipo de cambio aplicado para contabilizar el IVA cuando la moneda de la factura no sea el euro. | Importe | |
| BT-7 | + | 0..1 | Fecha de realización de las operaciones | Fecha en la que se realiza la operación, siempre que sea diferente a la fecha de expedición. | Fecha | |
| BT-9 | + | 0..1 | Fecha de vencimiento del pago | Calculada según el art. 4 de la Ley 3/2004, de 29 de diciembre (morosidad en operaciones comerciales) o la normativa específica aplicable. | Fecha | |
| BT-ES-2 | + | 0..1 | Fecha efectiva de pago | Si la factura se ha pagado antes de la fecha de expedición o en dicha fecha, el emisor podrá consignar la fecha efectiva de pago. Debe ser necesariamente coincidente o posterior a la fecha de realización de las operaciones. | Fecha | `/in:Invoice/cac:PrepaidPayment/cbc:PaidDate` |
| BT-10 | + | 0..n | Referencia del comprador | Identificador asignado por el comprador para especificar rutas internas de la factura. | Texto | |
| BG-1 | + | 0..n | NOTA DE FACTURA | Notas de texto relevantes para la factura. Pueden incluirse menciones obligatorias según normativa específica, o adicionales que el emisor considere conveniente. | | |
| BT-21 | ++ | 0..1 | Código de la nota de factura | Tipología de la nota; valores de UNTDID 4451. Para menciones obligatorias según normativa específica se debe usar `REG`. | Código | |
| BT-22 | ++ | 1..1 | Nota de factura | Descripción de la nota o mención en factura. | Texto | |
| BG-2 | + | 1..1 | CONTROL DE PROCESO | Información sobre el proceso de negocio y reglas aplicables al documento Factura. | | |
| BT-23 | ++ | 1..1 | Tipo de proceso de negocio | Adoptará el valor `urn:aeat:names:specification:ubl:schema:profile:B2B` | Texto | |
| BT-24 | ++ | 1..1 | Identificador de especificación | Adoptará el valor `urn:cen.eu:en16931:2026#conformant#urn:aeat.es:spfe:1.0.0:extended` | Identificador | |
| BG-3 | + | 0..n | REFERENCIA DE UNA FACTURA ANTERIOR | Información relativa a una o varias facturas previamente emitidas relacionadas con la que se expide. | | |
| BT-25 | ++ | 1..1 | Serie y número de la factura referenciada | Serie y número de la factura referenciada o sustituida. | Referencia de documento | |
| BT-26 | ++ | 1..1 | Fecha de expedición de la factura referenciada | Fecha de expedición de la factura referenciada o sustituida. | Fecha | |
| BG-4 | + | 1..1 | DATOS DEL VENDEDOR | Información sobre el vendedor. | | |
| BT-27 | ++ | 1..1 | Nombre o razón social del vendedor | Nombre y apellidos o razón social del vendedor. | Texto | |
| BT-31 | ++ | 0..1 | Número de identificación fiscal a efectos de IVA del vendedor | NIF-IVA del vendedor, con el prefijo correspondiente a España según ISO 3166-1. Obligatorio consignar un NIF-IVA válido si BT-32 no tiene contenido. | Identificador | En este caso `/in:Invoice/cac:AccountingSupplierParty/cac:Party/cac:PartyTaxScheme/cac:TaxScheme/cbc:ID` tomará el valor `VAT` |
| BT-32 | ++ | 0..1 | Identificador fiscal del vendedor | NIF del vendedor. Obligatorio consignar un NIF válido si BT-31 no tiene contenido. | Identificador | En este caso `/in:Invoice/cac:AccountingSupplierParty/cac:Party/cac:PartyTaxScheme/cac:TaxScheme/cbc:ID` tomará el valor `LOC` y el atributo `/in:Invoice/cac:AccountingSupplierParty/cac:Party/cac:PartyTaxScheme/cbc:CompanyID@schemeID` tomará el valor `FC` |
| BT-30 | ++ | 0..1 | Identificador de inscripción legal del vendedor | NIF del vendedor. Obligatorio cuando venga informado BT-32. | Identificador | |
| BT-33 | ++ | 0..1 | Información jurídica adicional del vendedor | Información jurídica relevante adicional del vendedor. | Texto | |
| BG-5 | ++ | 1..1 | DIRECCIÓN POSTAL DEL VENDEDOR | Dirección del vendedor. | | |
| BT-35 | +++ | 1..1 | Dirección del vendedor | Incluye tipo de vía, nombre largo de la vía pública, tipo de numeración y número o punto kilométrico. | Texto | |
| BT-37 | +++ | 0..1 | Municipio del vendedor | Nombre del municipio. Obligatorio si la dirección postal del vendedor está en España. | Texto | |
| BT-38 | +++ | 0..1 | Código de dirección postal del vendedor | Código postal del domicilio. Obligatorio si la dirección está en España. | Texto | |
| BT-39 | +++ | 0..1 | Provincia del vendedor | Provincia. Obligatorio si la dirección está en España. | Texto | |
| BT-40 | +++ | 1..1 | Código de país del vendedor | Países válidos según EN ISO 3166-1. | Código | |
| BG-6 | ++ | 0..1 | CONTACTO DEL VENDEDOR | Información de contacto del vendedor. | | |
| BT-42 | +++ | 0..1 | Número de teléfono de contacto del vendedor | — | Texto | |
| BT-43 | +++ | 0..1 | Dirección de correo electrónico del vendedor | Correo electrónico de contacto del vendedor. | Texto | |
| BG-7 | + | 1..1 | DATOS DEL COMPRADOR | Información sobre el comprador. | | |
| BT-44 | ++ | 1..1 | Nombre del comprador | Nombre y apellidos o razón social del comprador. | Texto | |
| BT-47 | ++ | 0..1 | Identificador fiscal del comprador | NIF del comprador. Obligatorio consignar un NIF válido si BT-48 no tiene contenido. | Identificador | En este caso `/in:Invoice/cac:AccountingCustomerParty/cac:Party/cac:PartyTaxScheme/cac:TaxScheme/cbc:ID` tomará el valor `LOC` y el atributo `/in:Invoice/cac:AccountingCustomerParty/cac:Party/cac:PartyTaxScheme/cbc:CompanyID@schemeID` tomará el valor `FC` |
| BT-48 | ++ | 0..1 | Número de identificación fiscal a efectos de IVA del comprador | Texto literal del anexo: "Número de identificación fiscal a efectos de IVA del **vendedor**, con el prefijo correspondiente a España, según los códigos ISO 3166-1" (`TODO(VERIFY)`: errata probable, debería ser "del comprador"; y el prefijo "correspondiente a España" no encaja con compradores de otros Estados). Obligatorio consignar un NIF-IVA válido si BT-47 no tiene contenido. | Identificador | En este caso `/in:Invoice/cac:AccountingCustomerParty/cac:Party/cac:PartyTaxScheme/cac:TaxScheme/cbc:ID` tomará el valor `VAT` |
| BG-8 | ++ | 1..1 | DIRECCIÓN POSTAL DEL COMPRADOR | Dirección postal del comprador. | | |
| BT-50 | +++ | 1..1 | Dirección del comprador | Incluye tipo de vía, nombre largo de la vía pública, tipo de numeración y número o punto kilométrico. | Texto | |
| BT-52 | +++ | 0..1 | Municipio del comprador | Obligatorio si la dirección postal del comprador está en España. | Texto | |
| BT-53 | +++ | 0..1 | Código de dirección postal del comprador | Código postal. Obligatorio si la dirección está en España. | Texto | |
| BT-54 | +++ | 0..1 | Provincia del comprador | Obligatorio si la dirección está en España. | Texto | |
| BT-55 | +++ | 1..1 | Código de país del comprador | Países válidos según EN ISO 3166-1. | Código | |
| BG-9 | ++ | 0..1 | CONTACTO DEL COMPRADOR | Información de contacto relevante del comprador. | | |
| BT-57 | +++ | 0..1 | Teléfono de contacto del comprador | — | Texto | |
| BT-58 | +++ | 0..1 | Dirección de correo electrónico del comprador | — | Texto | |
| BG-ES-1 | + | 0..n | DATOS DEL TERCERO | Información relativa a un tercero. Obligatorio en el caso del tercero que expide materialmente la factura en lugar del obligado (art. 5 del Reglamento de facturación, RD 1619/2012). | | `/in:Invoice/cac:AccountingSupplierParty/cac:Party/cac:ServiceProviderParty` |
| BT-ES-3 | ++ | 1..1 | Nombre o razón social del tercero | Nombre completo o razón social del tercero. | Texto | `/in:Invoice/cac:AccountingSupplierParty/cac:Party/cac:ServiceProviderParty/cac:Party/cac:PartyLegalEntity/cbc:RegistrationName` |
| BT-ES-4 | ++ | 1..1 | NIF del tercero | NIF del tercero informado. | Identificador | `/in:Invoice/cac:AccountingSupplierParty/cac:Party/cac:ServiceProviderParty/cac:Party/cac:PartyTaxScheme/cbc:CompanyID`. En caso de NIF además: `.../cac:ServiceProviderParty/cac:Party/cac:PartyTaxScheme/cac:TaxScheme/cbc:ID` = `LOC` y el atributo `.../cac:ServiceProviderParty/cac:Party/cac:PartyTaxScheme/cbc:CompanyID@schemeID` = `FC`. En caso contrario `.../cac:PartyTaxScheme/cac:TaxScheme/cbc:ID` = `VAT` |
| BT-ES-5 | ++ | 1..1 | Tipología de tercero | Tipo de tercero. Si es un tercero que expide materialmente la factura en lugar del vendedor, se debe usar obligatoriamente `FacturaciónPorTercero`. | Código | `/in:Invoice/cac:AccountingSupplierParty/cac:Party/cac:ServiceProviderParty/cbc:ServiceTypeCode` |
| BG-14 | + | 0..1 | PERIODO DE LA FACTURA RECAPITULATIVA | Fecha de inicio y fin del periodo abarcado, en el caso de factura recapitulativa. | | |
| BT-73 | ++ | 0..1 | Fecha de inicio del período de facturación | Fecha inicial del plazo de la factura recapitulativa. | Fecha | |
| BT-74 | ++ | 0..1 | Fecha de fin del período de facturación | Fecha final del plazo de la factura recapitulativa. | Fecha | |
| BG-20 | + | 0..n | MINORACIONES A NIVEL DE DOCUMENTO | Minoraciones aplicables a la factura en su totalidad. | | Su uso implica que todos los elementos `/in:Invoice/cac:AllowanceCharge/cbc:ChargeIndicator` del grupo deben tener el valor `false` |
| BT-92 | ++ | 1..1 | Importe de la minoración a nivel de documento | Importe de las minoraciones aplicables a la factura, IVA excluido. | Importe | |
| BT-95 | ++ | 1..1 | Código de categoría de IVA de la minoración a nivel de documento | Categoría de IVA de la minoración; valores de UNTDID 5305 incluidos en **L4**. | Código (L4) | Implica que `/in:Invoice/cac:AllowanceCharge/cac:TaxCategory/cac:TaxScheme/cbc:ID` tome el valor `VAT` |
| BT-96 | ++ | 0..1 | Tipo de IVA de la minoración a nivel de documento | Tipo de IVA (porcentaje) aplicable a la minoración. | Porcentaje | |
| BT-97 | ++ | 1..1 | Razón de la minoración a nivel de documento | Razón de la minoración, como texto. | Texto | |
| BG-21 | + | 0..n | CARGOS A NIVEL DE DOCUMENTO | Cargos e impuestos distintos al IVA que se aplican a la factura en su conjunto. | | Su uso implica que todos los elementos `/in:Invoice/cac:AllowanceCharge/cbc:ChargeIndicator` del grupo deben tener el valor `true` |
| BT-99 | ++ | 1..1 | Importe del cargo a nivel de documento | Importe del cargo, sin IVA. | Importe | |
| BT-102 | ++ | 1..1 | Código de categoría de IVA del cargo a nivel de documento | Categoría de IVA del cargo; valores de UNTDID 5305 incluidos en **L4**. | Código (L4) | Implica que `/in:Invoice/cac:AllowanceCharge/cac:TaxCategory/cac:TaxScheme/cbc:ID` tome el valor `VAT` |
| BT-103 | ++ | 0..1 | Tipo de IVA del cargo a nivel de documento | Tipo de IVA (porcentaje) aplicable al cargo. | Porcentaje | |
| BT-104 | ++ | 1..1 | Razón de cargo a nivel de documento | Razón del cargo, como texto. | Texto | |
| BG-ES-2 | ++ | 0..n | DETALLE DEL IMPUESTO O TASA A NIVEL DE DOCUMENTO | Tasas o impuestos distintos al IVA que aplican a la factura en su conjunto. | | (sin ruta propia) |
| BT-ES-6 | +++ | 0..1 | Base del impuesto o tasa | Base imponible del impuesto o tasa. | Importe | `/in:Invoice/cac:Invoice/cac:AllowanceCharge/cac:TaxTotal/cac:TaxSubTotal/cbc:TaxableAmount` (`TODO(VERIFY)`: el segmento `cac:Invoice` duplicado y `TaxSubTotal` con T mayúscula figuran así en el original; en UBL 2.x el elemento es `cac:TaxSubtotal`) |
| BT-ES-7 | +++ | 0..1 | Tipo del impuesto o tasa | Porcentaje aplicable a la base. | Porcentaje | `/in:Invoice/cac:Invoice/cac:AllowanceCharge/cac:TaxTotal/cac:TaxSubTotal/cac:TaxCategory/cbc:Percent` |
| BT-ES-8 | +++ | 1..1 | Cuota del impuesto o tasa | Resultado de aplicar a la base el tipo. El mismo importe debe consignarse en dos rutas distintas. | Importe | `/in:Invoice/cac:Invoice/cac:AllowanceCharge/cac:TaxTotal//cbc:TaxAmount` (literal, con doble barra) **y también en** `/in:Invoice/cac:Invoice/cac:AllowanceCharge/cac:TaxTotal/cac:TaxSubTotal/cbc:TaxAmount` |
| BT-ES-9 | +++ | 1..1 | Código del tipo de impuesto incluido como cargo a nivel de documento | Tipo de impuesto (no IVA) incluido como cargo a nivel de documento, expresado como código. | Código (L9) | `/in:Invoice/cac:Invoice/cac:AllowanceCharge/ cac:TaxTotal/cac:TaxSubTotal/cac:TaxCategory/cac:TaxScheme/cbc:TaxTypeCode` |
| BG-22 | + | 1..1 | TOTALES A NIVEL DE DOCUMENTO | Totales monetarios de la factura. | | |
| BT-106 | ++ | 1..1 | Suma del importe neto de las líneas de factura | Suma de todos los importes netos de las líneas. | Importe | |
| BT-107 | ++ | 0..1 | Suma de minoraciones a nivel de documento | Suma de todas las minoraciones a nivel de documento. | Importe | |
| BT-108 | ++ | 0..1 | Suma de cargos a nivel de documento | Suma de todos los cargos a nivel de documento. | Importe | |
| BT-109 | ++ | 1..1 | Importe total de la factura sin IVA | Importe total antes de aplicar el IVA. | Importe | |
| BT-110 | ++ | 1..1 | Cuota total del IVA de la factura | Cuota total del IVA. | Importe | |
| BT-111 | ++ | 0..1 | Cuota total del IVA de la factura en la moneda contable | Si la moneda no es el euro, la cuota total del IVA debe expresarse adicionalmente en euros. Si en la misma factura se repercute recargo de equivalencia, se adicionarán las cuotas del recargo en euros. | Importe | |
| BT-112 | ++ | 1..1 | Importe total de la factura con IVA | Importe total después de aplicar el IVA. | Importe | |
| BT-113 | ++ | 0..1 | Importe pagado | Suma de importes pagados por adelantado. No debe utilizarse para pagos anticipados que hayan supuesto el devengo del IVA del art. 75.Dos de la Ley 37/1992. | Importe | |
| BT-114 | ++ | 0..1 | Importe de redondeo | Importe a añadir o restar al total para redondear el importe a pagar. | Importe | |
| BT-115 | ++ | 1..1 | Total a pagar | Total con IVA menos importes pagados por adelantado, junto con los importes de los conceptos BT-179 y los redondeos BT-114. Es cero si la factura está pagada en su totalidad. Puede ser negativo: en ese caso el vendedor debe dicho importe al comprador. | Importe | |
| BG-34 | ++ | 0..n | CARGOS EN NOMBRE DE UN TERCERO | Importes a favor de un tercero distinto del vendedor, a pagar por el comprador. | | |
| BT-179 | +++ | 1..1 | Importe del cargo cobrado en nombre de un tercero | Suplidos y/o cantidades repercutidas o exigidas en nombre de un tercero, sin reflejo en la base imponible. | Importe | |
| BT-180 | +++ | 1..1 | Descripción del cargo en nombre de un tercero | Motivo del cargo. | Texto | |
| BT-ES-10 | +++ | 1..1 | Identificación del cargo en nombre de un tercero | Código obligatorio: `SUPL`. | Código (L7) | `/in:Invoice/cac:CollectionInvoiceLine/cbc:ID` |
| BG-34 | ++ | 0..n | IMPUESTOS Y TASAS | Cargos en nombre de un tercero correspondientes a impuestos y tasas; incluye la información obligatoria sobre su cálculo. | | |
| BT-179 | +++ | 1..1 | Suma de las cuotas de los impuestos o tasas | Suma de las cuotas de los impuestos y tasas incluidos como cargos en nombre de un tercero. | Importe | |
| BT-180 | +++ | 1..1 | Denominación del impuesto o tasa | Denominación del impuesto o tasa. | Texto | |
| BT-ES-11 | +++ | 1..1 | Identificación del impuesto o tasa | Código obligatorio: `IEDMT` para el Impuesto Especial sobre Determinados Medios de Transporte; en el resto de casos `OTROS`. | Código (L7) | `/in:Invoice/cac:CollectionInvoiceLine/cbc:ID` |
| BG-ES-3 | +++ | 0..n | DESGLOSE IMPUESTOS Y TASAS | Detalle del cálculo de los impuestos y tasas incluidos como cargos en nombre de un tercero. | | Su uso implica que `/in:Invoice/cac:CollectionInvoiceLine/cac:TaxTotal/cac:TaxSubtotal/cac:TaxCategory/cac:TaxScheme/cbc:ID` tomará el valor `VAT` y `/in:Invoice/cac:CollectionInvoiceLine/cac:TaxTotal/cac:TaxSubtotal/cac:TaxCategory/cbc:ID` adoptará el valor `S` |
| BT-ES-12 | ++++ | 0..1 | Base del impuesto o tasa | Base de cálculo del impuesto o tasa incluido como cargo en nombre de un tercero. | Importe | `/in:Invoice/cac:CollectionInvoiceLine/cac:TaxTotal/cac:TaxSubtotal/cbc:TaxableAmount` |
| BT-ES-13 | ++++ | 0..1 | Tipo del impuesto o tasa | Tipo aplicable. | Porcentaje | `/in:Invoice/cac:CollectionInvoiceLine/cac:TaxTotal/cac:TaxSubtotal/cac:TaxCategory/cbc:Percent` |
| BT-ES-14 | ++++ | 1..1 | Cuota total del impuesto o tasa | Cuota resultante de aplicar a la base el tipo. | Importe | `/in:Invoice/cac:CollectionInvoiceLine/cac:TaxTotal/cac:TaxSubtotal/cbc:TaxAmount` |
| BG-34 | ++ | 0..n | MINORACIONES EN NOMBRE DE UN TERCERO | Minoraciones, no parte de la factura, que deben minorar el total a pagar (p. ej. devolución de un suplido). | | |
| BT-179 | +++ | 1..1 | Importe de la minoración en nombre de un tercero | Minoraciones de cantidades exigidas en nombre de un tercero, sin reflejo en la base imponible. | Importe | |
| BT-180 | +++ | 1..1 | Descripción de la minoración en nombre de un tercero | Motivo de la minoración. | Texto | |
| BT-ES-15 | +++ | 1..1 | Identificación de la minoración en nombre de un tercero | Código obligatorio: `MINO`. | Código (L7) | `/in:Invoice/cac:CollectionInvoiceLine/cbc:ID` |
| BG-34 | ++ | 0..1 | RETENCIONES | Retenciones practicadas en la factura. | | |
| BT-179 | +++ | 1..1 | Suma de retenciones | Sumatorio de todas las retenciones practicadas en factura. | Importe | |
| BT-180 | +++ | 1..1 | Descripción de la retención | Motivo de la retención. | Texto | |
| BT-ES-16 | +++ | 1..1 | Identificación de retención | Código obligatorio: `RETE`. | Código (L7) | `/in:Invoice/cac:CollectionInvoiceLine/cbc:ID` |
| BT-ES-17 | +++ | 1..1 | Suma de retenciones | Sumatorio de todas las retenciones practicadas en factura. | Importe | `/in:Invoice/cac:WithholdingTaxTotal/ cbc:TaxAmount` |
| BG-ES-4 | +++ | 0..n | RETENCIÓN APLICADA | Información de una retención practicada en la factura. | | Su uso implica que `/in:Invoice/cac:WithholdingTaxTotal/cac:TaxSubtotal/cac:TaxCategory/cbc:ID` tome el valor `S` y `/in:Invoice/cac:WithholdingTaxTotal/cac:TaxSubtotal/cac:TaxCategory/cac:TaxScheme/cbc:ID` adopte el valor `WTH` |
| BT-ES-18 | ++++ | 1..1 | Base imponible retención | Base sobre la que se aplica el porcentaje de retención. | Importe | `/in:Invoice/cac:WithholdingTaxTotal/cac:TaxSubtotal/cbc:TaxableAmount` |
| BT-ES-19 | ++++ | 1..1 | Total retención | Resultado de aplicar a la base el porcentaje de retención. | Importe | `/in:Invoice/cac:WithholdingTaxTotal/cac:TaxSubtotal/cbc:TaxAmount` |
| BT-ES-20 | ++++ | 1..1 | Porcentaje | Porcentaje de retención aplicable. | Porcentaje | `/in:Invoice/cac:WithholdingTaxTotal/cac:TaxSubtotal/cac:TaxCategory/cbc:Percent` |
| BT-ES-21 | ++++ | 1..1 | Concepto | Información de la retención practicada; claves de la tabla **L5**. | Código (L5) | `/in:Invoice/cac:WithholdingTaxTotal/cac:TaxSubtotal/cac:TaxCategory/cac:TaxScheme/cbc:TaxTypeCode` |
| BG-25 | + | 1..n | LÍNEA DE FACTURA | Información a nivel de líneas individuales. | | |
| BT-126 | ++ | 1..1 | Identificador de línea de factura | Identificador único de la línea. | Identificador | |
| BT-129 | ++ | 1..1 | Cantidad facturada | Cantidad de artículos (bienes o servicios) cargados en la línea. | Cantidad | |
| BT-130 | ++ | 1..1 | Código de la unidad de medida de la cantidad facturada | Debe elegirse de la Recomendación UN/ECE nº 20 ("Códigos de Unidades de Medida utilizados en el comercio internacional") y la nº 21 ("Tipos de Pasajeros, tipos de carga, embalajes y materiales de embalaje…"). | Código | |
| BT-131 | ++ | 1..1 | Importe neto a nivel de línea de factura | Importe total de la línea. En el régimen especial de grupo de entidades, nivel avanzado, se informará el importe total de la contraprestación. | Importe | |
| BG-27 | ++ | 0..n | MINORACIONES DE LÍNEA DE FACTURA | Minoraciones aplicables a una línea. | | Su uso implica que todos los elementos `/in:Invoice/cac:InvoiceLine/cac:AllowanceCharge/cbc:ChargeIndicator` del grupo deben tener el valor `false` |
| BT-136 | +++ | 1..1 | Importe de la minoración de línea de factura | Importe de la minoración, sin IVA. | Importe | |
| BT-139 | +++ | 1..1 | Razón de la minoración de línea de factura | Razón, como texto. | Texto | |
| BG-28 | ++ | 0..n | CARGOS DE LÍNEA DE FACTURA | Cargos, tasas e impuestos distintos del IVA aplicables a una línea. | | Su uso implica que todos los elementos `/in:Invoice/cac:InvoiceLine/cac:AllowanceCharge/cbc:ChargeIndicator` del grupo deben tener el valor `true` |
| BT-141 | +++ | 1..1 | Importe del cargo de línea de factura | Importe del cargo, sin IVA. | Importe | |
| BT-142 | +++ | 0..1 | Base del importe de cargo de línea de factura | Base que, con el porcentaje (BT-143), permite calcular el cargo o impuesto de la línea. | Importe | |
| BT-143 | +++ | 0..1 | Tipo aplicable a la base del cargo de línea de factura | Porcentaje que, con la base, permite calcular el cargo, tasa o impuesto de la línea. | Porcentaje | |
| BT-ES-22 | +++ | 1..1 | Código del tipo de impuesto incluido como cargo de línea de factura | Tipo de impuesto (no IVA) o tasa incluido como cargo, como código. | Código (L9) | `/in:Invoice/cac:InvoiceLine/cac:AllowanceCharge/cac:TaxCategory/cac:TaxScheme/cbc:TaxTypeCode` |
| BT-144 | +++ | 1..1 | Razón del cargo de línea de factura | Razón, como texto. | Texto | |
| BG-29 | ++ | 1..1 | DETALLES DEL PRECIO | Precio aplicado a los bienes y servicios de la línea. | | |
| BT-146 | +++ | 1..1 | Precio neto del artículo | Precio, excluido el IVA, una vez minorado por el descuento sobre el precio del artículo. | Importe de precio unitario | |
| BT-149 | ++ (sic) | 0..1 | Cantidad del precio base por artículo | Número de unidades a las que se aplica el precio. | Cantidad | |
| BG-30 | ++ | 1..1 | INFORMACIÓN DEL IVA A NIVEL DE LÍNEA | IVA aplicable a los bienes y servicios de la línea. | | Su uso implica que `/in:Invoice/cac:InvoiceLine/cac:Item/cac:ClassifiedTaxCategory/cac:TaxScheme/cbc:ID` tomará el valor `VAT` |
| BT-151 | +++ | 1..1 | Código de categoría de IVA del artículo facturado | Valores de UNTDID 5305 incluidos en **L4**. | Código (L4) | |
| BT-152 | +++ | 0..1 | Tipo impositivo de IVA del artículo facturado | Porcentaje aplicable al artículo. | Porcentaje | |
| BT-195 | +++ | 0..1 | Código de razón de exención | Lista CEF-VATEX (Mecanismo "Conectar Europa"). Solo podrán adoptar valores **VATEX-EU** o **VATEX-ES**. | Código | |
| BG-31 | ++ | 1..1 | INFORMACIÓN DE ARTÍCULO | Bienes y servicios facturados. | | |
| BT-153 | +++ | 1..1 | Nombre de artículo | Nombre del artículo. | Texto | |
| BT-154 | +++ | 0..1 | Descripción de artículo | Descripción más detallada que el nombre. | Texto | |
| BG-32 | +++ | 0..n | INFORMACIÓN ADICIONAL A NIVEL DE LÍNEA | Información adicional de inclusión obligatoria según normativa específica, o que el emisor considere conveniente. | | |
| BT-160 | +++ (sic) | 1..1 | Código de la propiedad - Información adicional a nivel de línea | Valor fijo `INFO`. | Código (L8) | |
| BT-161 | +++ (sic) | 1..1 | Información adicional a nivel de línea | Descripción de la información adicional. | Texto | |
| BG-32 | +++ | 0..1 | REFERENCIA CATASTRAL | Inmueble sobre el que recae la operación facturada. | | |
| BT-160 | ++++ | 1..1 | Código de la propiedad - Referencia catastral | Valor fijo `RCAT`. | Código (L8) | |
| BT-161 | +++ (sic) | 1..1 | Referencia catastral | Referencia catastral del inmueble. | Texto | |
| BT-ES-23 | +++ (sic) | 1..1 | Situación del inmueble | Situación del inmueble. | Código (L6) | `/in:invoice/cac:InvoiceLine/cac:Item/cac:AdditionalItemProperty/cbc:ValueQualifier` |
| BG-32 | +++ | 1..2 | CLAVE DE RÉGIMEN | Régimen aplicable a la operación facturada. | | |
| BT-160 | ++++ | 1..1 | Código de la propiedad - Régimen | Valor fijo `REGI`. | Código (L8) | |
| BT-161 | ++++ | 1..1 | Código de clave de régimen | Clave de régimen de la operación según **L3A**, **L3B** y **L3C**. | Código (L3A/L3B/L3C) | |
| BT-ES-24 | ++++ | 1..1 | Tipo de impuesto nacional | Identificador de la sujeción de la operación al IVA, IGIC y al IPSI. | Código-Calificador (L3A/L3B/L3C) | `/in:invoice/cac:InvoiceLine/cac:Item/cac:AdditionalItemProperty/cbc:ValueQualifier` |
| BG-23 | + | 1..n | DESGLOSE DEL IVA | Desglose del IVA por categorías, tipos y razones de exención. También el desglose del recargo de equivalencia, en su caso. | | Su uso implica que `/in:Invoice/cac:TaxTotal/cac:TaxSubtotal/cac:TaxCategory/cac:TaxScheme/cbc:ID` tomará el valor `VAT` |
| BT-116 | ++ | 1..1 | Base imponible por categoría de IVA | Suma de todas las bases imponibles con un mismo código de categoría de IVA. En el régimen especial de grupo de entidades, nivel avanzado, se informará la base imponible a coste. | Importe | |
| BT-117 | ++ | 1..1 | Cuota tributaria por categoría de IVA | Cuota de IVA por categoría. En el caso del recargo de equivalencia, se informará la cuota de dicho recargo. | Importe | |
| BT-118 | ++ | 1..1 | Código de cuota tributaria por categoría de IVA | Valores de UNTDID 5305 incluidos en **L4**. | Código (L4) | |
| BT-119 | ++ | 1..1 | Tipo de IVA por categoría | Tipo de IVA (porcentaje) de la categoría. En el caso del recargo de equivalencia, se informará el tipo del recargo aplicable. | Porcentaje | |
| BT-120 | ++ | 0..1 | Razón de exención | Enunciado que explica por qué un importe está exento de IVA o por qué no se aplica el IVA. | Texto | |
| BT-121 | ++ | 0..1 | Código de razón de exención | Lista CEF-VATEX. Solo podrán adoptar valores **VATEX-EU** o **VATEX-ES**. | Código | |
| BG-33 | + | 0..n | Condiciones de pago | Condiciones de pago a aplicar a la cantidad a pagar. | | |
| BT-20 | ++ | 0..1 | Descripción de las condiciones de pago | — | Texto | |
| BG-16 | + | 0..1 | INSTRUCCIONES DE PAGO | Información acerca del pago. | | |
| BT-81 | ++ | 1..1 | Código de tipo de medios de pago | Medio por el que se liquida o se ha liquidado el pago. Obligatorio usar códigos de UNTDID 4461. | Código | |
| BG-17 | ++ | 0..n | TRANSFERENCIA BANCARIA | Pagos por transferencia. | | |
| BT-84 | +++ | 1..1 | Identificador de la cuenta de pago | Identificador único de la cuenta a la que debería efectuarse el pago. Con carácter general, un IBAN. | Identificador | |
| BG-24 | + | 0..1 | QR FISCAL | Información obligatoria relativa al código QR del art. 6.5 del RD 1619/2012 o normativa foral aplicable. | | |
| BT-122 | ++ | 1..1 | Referencia de documento justificativo - ID | Valor obligatorio: `QR`. | Código (L10) | |
| BT-123 | ++ | 0..1 | Descripción de documento justificativo - Tipo de QR | Valor obligatorio: `VERIFACTU` o `TICKETBAI`, según normativa aplicable. | Texto | |
| BT-ES-25 | ++ | 1..1 | Localización de documento externo - Descripción | URL correspondiente al QR VERI\*FACTU o TICKETBAI, según normativa aplicable. | Texto | `/in:Invoice/cac:AdditionalDocumentReference/cbc:UUID` |
| BG-24 | + | 0..n | AUTORIZACIÓN DE FACTURACIÓN | Autorización concedida al obligado a expedir factura (RD 1619/2012 o RD 1624/1992), por el Departamento de Gestión Tributaria de la AEAT, la Administración Tributaria Canaria o la Diputación Foral correspondiente. | | |
| BT-122 | ++ | 1..1 | Referencia de documento justificativo - ID | Valor obligatorio: `AUT`. | Código (L10) | |
| BT-123 | ++ | 0..1 | Descripción de documento justificativo - Descripción autorización | Texto descriptivo de la autorización. | Texto | |
| BT-ES-26 | ++ | 1..1 | Referencia de documento justificativo - Nº referencia autorización | Número de referencia de la autorización. | Texto | `/in:Invoice/cac:AdditionalDocumentReference/cbc:UUID` |
| BG-24 | + | 0..1 | TIPO DE FACTURA RECTIFICATIVA | Tipo de factura rectificativa expedida. | | |
| BT-122 | ++ | 1..1 | Referencia de documento justificativo - ID | Valor fijo: `RECT:TIPO`. | Código (L10) | |
| BT-123 | ++ | 0..1 | Descripción del tipo de factura rectificativa | Se especificará el tipo de factura rectificativa. | Texto | |
| BT-ES-27 | ++ | 1..1 | Código de tipo de factura rectificativa | Las rectificativas (BT-3 = 384 y 471) adoptarán los valores de **L2.A**. | Código (L2.A) | `/in:Invoice/cac:AdditionalDocumentReference/cbc:DocumentTypeCode` |
| BG-24 | + | 0..1 | MODALIDAD DE FACTURA RECTIFICATIVA | Modalidad de rectificación utilizada. | | |
| BT-122 | ++ | 1..1 | Referencia de documento justificativo - ID | Valor fijo: `RECT:MODALIDAD`. | Código (L10) | |
| BT-123 | ++ | 0..1 | Descripción de la modalidad de factura rectificativa | Descripción de la modalidad empleada. | Texto | |
| BT-ES-28 | ++ | 1..1 | Código de modalidad de factura rectificativa | Valores de **L2.B**. | Código (L2.B) | `/in:Invoice/cac:AdditionalDocumentReference/cbc:DocumentTypeCode` |
| BG-24 | + | 0..5 | IMPORTES RECTIFICADOS | Importes rectificados respecto de la factura original, cuando se rectifica por sustitución y se opta por emitir una única factura con los importes correctos, incluyendo aquí los importes rectificados. (`TODO(VERIFY)`: la cardinalidad aquí es 0..5 pero BT-3 habla de "hasta tres grupos" de importes rectificados.) | | |
| BT-122 | ++ | 1..1 | Referencia de documento justificativo - ID | Valor fijo: `RECT:IMPORTES`. | Código (L10) | |
| BT-123 | ++ | 1..1 | Importe rectificado | Importe de la factura rectificada; según el código informado se refiere a la base imponible, a la cuota de IVA o a la cuota del recargo de equivalencia. | Texto | |
| BT-ES-29 | ++ | 1..1 | Código de los importes rectificados | Valores de **L2.C**. | Código (L2.C) | `/in:Invoice/cac:AdditionalDocumentReference/cbc:DocumentTypeCode` |

Notas sobre el mensaje de alta:
- `(sic)`: el nivel se transcribe tal como figura en el original aunque no sea coherente con el del grupo padre (p. ej. BT-149 aparece como `++` dentro de BG-29; en BG-32 "Información adicional" BT-160/BT-161 figuran como `+++`; en BG-32 "Referencia catastral" BT-161 y BT-ES-23 figuran como `+++` mientras BT-160 es `++++`). `TODO(VERIFY)` con el XSD/validaciones.
- Los grupos BG-34 (cargos, impuestos y tasas, minoraciones y retenciones) y BG-24 (QR, autorización, rectificativas) se repiten con el mismo ID y se distinguen por el código de BT-ES-10/11/15/16 (`cac:CollectionInvoiceLine/cbc:ID`, lista L7) y por BT-122 (lista L10), respectivamente. El anexo no da la ruta UBL de BT-179, BT-180, BT-122 ni BT-123 (`TODO(VERIFY)`: se presume `cac:CollectionInvoiceLine` para BG-34 y `cac:AdditionalDocumentReference/cbc:ID` y `cbc:DocumentDescription` para BG-24 según el binding de la EN 16931 vigente; no figura en el anexo).
- Los términos BT-166, BT-167, BT-179, BT-180 y BT-195 no existen en la EN 16931-1:2017; el anexo los presenta como BT de la EN 16931 (BT-24 hace referencia a `en16931:2026`). `TODO(VERIFY)` contra la revisión 2026 de la norma y su binding UBL.

---

## 2. FACTURA ELECTRÓNICA O COPIA FIEL – Mensaje de anulación

Documento UBL `ApplicationResponse` con `ApplicationResponse/cac:DocumentResponse/cac:Response/cbc:ResponseCode` = **`CANCELINVOICE`**.

La tabla del original no tiene columna ID (no hay BT/BG asignados).

| ID | Nivel | Card. | Término de negocio | Descripción | Tipo | Ruta UBL |
|---|---|---|---|---|---|---|
| — | + | 1..1 | Fecha de la anulación | Fecha en la que se produce la anulación de la factura. | Fecha | `ApplicationResponse/cac:DocumentResponse/cac:Response/cbc:EffectiveDate` |
| — | + | 1..n | FACTURA ANULADA | Datos de la factura que se anula. | | `ApplicationResponse/cac:DocumentResponse/cac:Response/cac:DocumentReference` |
| — | ++ | 1..1 | Número de factura (ID) | Serie y número de la factura anulada. | Texto | `ApplicationResponse/cac:DocumentResponse/cac:Response/cac:DocumentReference/cbc:ID` |
| — | ++ | 1..1 | Fecha de expedición de la factura | Fecha de expedición de la factura anulada. | Fecha | `ApplicationResponse/cac:DocumentResponse/cac:Response/cac:DocumentReference/cbc:IssueDate` |
| — | ++ | 1..1 | DATOS DEL VENDEDOR | Información sobre el vendedor identificado en la factura. | | `ApplicationResponse/cac:DocumentResponse/cac:Response/cac:DocumentReference/cac:IssuerParty` |
| — | +++ | 1..1 | NIF del vendedor | NIF del vendedor en la factura anulada. | Identificador | `ApplicationResponse/cac:DocumentResponse/cac:Response/cac:DocumentReference/cac:IssuerParty/cac:PartyTaxScheme/cbc:CompanyID` |
| — | +++ | 1..1 | Nombre o razón social del vendedor | Nombre y apellidos o razón social del vendedor. | Texto | `ApplicationResponse/cac:DocumentResponse/cac:Response/cac:DocumentReference/cac:IssuerParty/cac:PartyTaxScheme/cac:PartyLegalEntity/cbc:RegistrationName` |

Notas: `TODO(VERIFY)` la última ruta: en UBL 2.x `cac:PartyLegalEntity` es hijo de `cac:Party`, no de `cac:PartyTaxScheme` (que tiene su propio `cbc:RegistrationName`); se transcribe literal. El anexo no menciona ningún otro tipo de mensaje (p. ej. estado de pago o rechazo): sólo alta y anulación.

---

## 3. Listas de valores

Texto introductorio del anexo: "Se definen a continuación los valores permitidos en los campos con tipo semántico de dato de tipo código en los que se ha indicado expresamente que se definirán los valores en una lista (L). En los restantes campos de tipo código, en la columna descripción, se indica la forma de definición de los valores admitidos." (ISO 4217, ISO 3166-1, UNTDID 4451, UNTDID 4461, UN/ECE Rec. 20/21, CEF-VATEX).

### L1 – Tipo de factura (BT-3)

| Valor | Descripción |
|---|---|
| 380 | Factura (art. 6, 7.2 y 7.3 del RD 1619/2012) |
| 384 | Factura rectificativa |
| 388 | Factura emitida en sustitución de facturas simplificadas |
| 389 | Factura emitida por el destinatario |
| 471 | Factura rectificativa emitida por el destinatario |

### L2.A – Tipo de factura rectificativa (BT-ES-27)

| Valor | Descripción |
|---|---|
| R1 | Factura Rectificativa (Error fundado en derecho y Art. 80 Uno Dos y Seis LIVA) |
| R2 | Factura Rectificativa (Art. 80.3) |
| R3 | Factura Rectificativa (Art. 80.4) |
| R4 | Factura Rectificativa (Resto) |

### L2.B – Modalidad de factura rectificativa (BT-ES-28)

| Valor | Descripción |
|---|---|
| I | Rectificativa por diferencias |
| S | Rectificativa por sustitución |

### L2.C – Importes rectificados (BT-ES-29)

| Valor | Descripción |
|---|---|
| Base | Base imponible de la factura rectificada. |
| Cuota | Cuota repercutida de la factura rectificada. |
| CuotaRecargo | Cuota recargo de equivalencia de la factura rectificada. |

### L3A – Clave de régimen-IVA (BT-161 / BT-ES-24) — CALIFICADOR: `IVA`

Descripción de la clave de régimen para desgloses donde el impuesto de aplicación es el IVA.

| Valor | Descripción |
|---|---|
| 01 | Operación en régimen general y no incluidas en otras claves |
| 02 | Exportación |
| 03 | Operaciones a las que se aplique el régimen especial de bienes usados, objetos de arte, antigüedades y objetos de colección |
| 04 | Régimen especial del oro de inversión |
| 05 | Régimen especial de las agencias de viajes |
| 06 | Régimen especial grupo de entidades en IVA (Nivel Avanzado) |
| 07 | Régimen especial del criterio de caja |
| 08 | Operaciones sujetas al IPSI / IGIC (Impuesto sobre la Producción, los Servicios y la Importación / Impuesto General Indirecto Canario) |
| 09 | Facturación de las prestaciones de servicios de agencias de viaje que actúan como mediadoras en nombre y por cuenta ajena (D.A.4ª RD1619/2012) |
| 11 | Operaciones de arrendamiento de local de negocio |
| 14 | Factura con IVA pendiente de devengo en certificaciones de obra cuyo destinatario sea una Administración Pública |
| 15 | Factura con IVA pendiente de devengo en operaciones de tracto sucesivo |

(No hay claves 10, 12, 13 ni posteriores a 15 en L3A.)

### L3B – Clave de régimen-IGIC — CALIFICADOR: `IGIC`

| Valor | Descripción |
|---|---|
| 01 | Operación en régimen general y no incluidas en otras claves |
| 02 | Exportación |
| 03 | Operaciones a las que se aplique el régimen especial de bienes usados, objetos de arte, antigüedades y objetos de colección |
| 04 | Régimen especial del oro de inversión |
| 05 | Régimen especial de las agencias de viajes |
| 06 | Régimen especial grupo de entidades en IGIC (Nivel Avanzado) |
| 07 | Régimen especial del criterio de caja |
| 08 | Operaciones sujetas al IPSI / IVA (Impuesto sobre la Producción, los Servicios y la Importación / Impuesto sobre el Valor Añadido) |
| 09 | Facturación de las prestaciones de servicios de agencias de viaje que actúan como mediadoras en nombre y por cuenta ajena (D.A.4ª RD1619/2012) |
| 11 | Operaciones de arrendamiento de local de negocio |
| 14 | Factura con IGIC pendiente de devengo en certificaciones de obra cuyo destinatario sea una Administración Pública |
| 15 | Factura con IGIC pendiente de devengo en operaciones de tracto sucesivo |
| 17 | Régimen especial de comerciante minorista |
| 18 | Régimen especial del pequeño empresario o profesional |
| 19 | Operaciones interiores exentas por aplicación artículo 25 Ley 19/1994 |

### L3C – Clave de régimen-IPSI — CALIFICADOR: `IPSI`

| Valor | Descripción |
|---|---|
| 01 | Operación en régimen general y no incluidas en otras claves |
| 08 | Operaciones sujetas al IGIC / IVA (Impuesto sobre la Producción, los Servicios y la Importación / Impuesto sobre el Valor Añadido) — sic |
| 11 | Operaciones de arrendamiento de local de negocio |
| 19 | Operaciones interiores exentas |
| 20 | Régimen simplificado |

### L4 – Código de tipo de operación (BT-95, BT-102, BT-118, BT-151; UNTDID 5305)

| Valor | Descripción |
|---|---|
| S | Operaciones en régimen general |
| Z | Bienes con tipo impositivo 0% |
| E | Exento de IVA |
| AE | Inversión del sujeto pasivo |
| O | Operación no sujeta a IVA |
| G | Exportación. |
| L | Operación sujeta al IGIC |
| M | Operación sujeta al IPSI |
| N | Regímenes nacionales de IVA (régimen especial de grupo de entidades, nivel avanzado, y régimen especial del recargo de equivalencia) |
| D | Régimen especial de agencias de viajes |
| F | Régimen especial de bienes usados, objetos de arte antigüedades, y objetos de colección |

(L4 **no** incluye el código UNTDID 5305 `K` – entrega intracomunitaria exenta – ni `B`.)

### L5 – Tipo de retención (BT-ES-21)

| Valor | Descripción |
|---|---|
| 01 | Artículo 75.1.c) Inciso primero RIRPF – Rendimiento de actividades profesionales |
| 02 | Artículo 75.1.c) Inciso segundo RIRPF – Rendimiento de actividades agrícolas y ganaderas |
| 03 | Artículo 75.1.c) Inciso tercero RIRPF – Rendimiento de actividades forestales |
| 04 | Artículo 75.1.c) Inciso cuarto RIRPF – Rendimientos de actividades empresariales del artículo 95.6.2º RIRPF |
| 05 | Artículo 75.1. d) Inciso segundo RIRPF – Ganancias patrimoniales de los aprovechamientos forestales de los vecinos en montes públicos |
| 06 | Artículo 75.2.a) RIRPF – Rendimientos de arrendamientos o subarrendamientos de locales urbanos |
| 07 | Artículo 75.2.b) RIRPF - Rendimientos procedentes de la propiedad intelectual, industrial, de la prestación de asistencia técnica, del arrendamiento de bienes muebles, negocios o minas, del subarrendamiento sobre los bienes anteriores y los procedentes de la cesión del derecho a la explotación del derecho de imagen |
| 08 | Artículo 60.1.c) RIS - Las contraprestaciones obtenidas como consecuencia de la atribución de cargos de administrador o consejero en otras sociedades |
| 09 | Artículo 60.1.d) RIS - Las rentas procedentes de la cesión del derecho a la explotación de la imagen o del consentimiento o autorización para su utilización, aun cuando constituyan ingresos derivados de explotaciones económicas |
| 10 | Artículo 60.1.e) RIS - Las rentas procedentes del arrendamiento o subarrendamiento de inmuebles urbanos, aun cuando constituyan ingresos derivados de explotaciones económicas |
| 11 | Otros |

### L6 – Situación del inmueble (BT-ES-23)

| Valor | Descripción |
|---|---|
| 01 | Inmueble con referencia catastral situado en cualquier punto del territorio español, excepto País Vasco y Navarra |
| 02 | Inmueble situado en la Comunidad Autónoma del País Vasco o en la Comunidad Foral de Navarra |
| 03 | Inmueble en cualquiera de las situaciones anteriores pero sin referencia catastral |
| 04 | Inmueble situado en el extranjero |

### L7 – Tipo de cargo o minoración (BT-ES-10, BT-ES-11, BT-ES-15, BT-ES-16) — CALIFICADOR: `urn:aeat:names:specification:schema:add:`

| Valor | Descripción |
|---|---|
| SUPL | Cargo en nombre de un tercero |
| MINO | Minoración en nombre de un tercero |
| RETE | Retención |
| IEDMT | Impuesto especial sobre determinados medios de transporte |
| OTROS | Otros supuestos de tasas o impuestos no incluidos en el resto de claves |

### L8 – Tipo de propiedad (BT-160) — CALIFICADOR: `urn:aeat:names:specification:schema:lin:`

| Valor | Descripción |
|---|---|
| RCAT | Referencia catastral |
| REGI | Clave de régimen |
| INFO | Regulación aplicable a la línea de la factura |

### L9 – Impuestos y cargos a nivel de línea o documento (BT-ES-9, BT-ES-22)

| Valor | Descripción |
|---|---|
| IEH | Impuesto especial sobre los hidrocarburos |
| IECE | Impuesto especial sobre la cerveza |
| IEPI | Impuesto especial sobre productos intermedios |
| IEA | Impuesto especial sobre el alcohol y bebidas derivadas |
| IET | Impuesto especial sobre las labores del tabaco |
| IELC | Impuesto sobre los líquidos para cigarrillos electrónicos y otros productos relacionados con el tabaco |
| IEE | Impuesto especial sobre la electricidad |
| IEC | Impuesto especial sobre el carbón |
| IEP | Impuesto especial sobre los envases de plástico no reutilizables (artículo 82.9.a) Ley 7/2022, de 8 de abril) |
| IEDE | Impuesto sobre el depósito de residuos en vertederos, la incineración y la coincineración de residuos |
| IMPTAS | Otros impuestos y tasas |
| RESTO | Otros cargos a nivel de línea o documento |

### L10 – Documentos justificativos adicionales (BT-122) — CALIFICADOR: `urn:aeat:names:specification:schema:`

| Valor | Descripción |
|---|---|
| QR | QR fiscal |
| AUT | Autorización de facturación |
| RECT:TIPO | Tipo de factura rectificativa |
| RECT:MODALIDAD | Modalidad de factura rectificativa |
| RECT:IMPORTES | Importes rectificados |

`TODO(VERIFY)`: el anexo indica un "CALIFICADOR" para L3A/L3B/L3C (`IVA`/`IGIC`/`IPSI`), L7, L8 y L10, pero sólo dice dónde va en el caso de BT-ES-24 (`cbc:ValueQualifier`, tipo "Código-Calificador"). No especifica en qué atributo (p. ej. `listID`/`schemeID`) se consignan los calificadores URN de L7, L8 y L10. Comprobar en el XSD/guía técnica.

---

## 4. Diferencias respecto a EN 16931 / campos BT-ES

### Identificadores de perfil y especificación

| Campo | Valor obligatorio |
|---|---|
| BT-23 Tipo de proceso de negocio (UBL `cbc:ProfileID`) | `urn:aeat:names:specification:ubl:schema:profile:B2B` |
| BT-24 Identificador de especificación (UBL `cbc:CustomizationID`) | `urn:cen.eu:en16931:2026#conformant#urn:aeat.es:spfe:1.0.0:extended` |

(La correspondencia BT-23 → `cbc:ProfileID` y BT-24 → `cbc:CustomizationID` es la del binding UBL estándar; el anexo no la repite.) El sufijo `#conformant#…:extended` declara una extensión conforme de la EN 16931.

### Campos y grupos BT-ES / BG-ES

| ID | Término | Ruta UBL / valor |
|---|---|---|
| BT-ES-1 | Indicador de copia | `/in:Invoice/cbc:CopyIndicator` (`true` copia / `false` o ausente original) |
| BT-ES-2 | Fecha efectiva de pago | `/in:Invoice/cac:PrepaidPayment/cbc:PaidDate` |
| BG-ES-1 | Datos del tercero (facturación por tercero) | `/in:Invoice/cac:AccountingSupplierParty/cac:Party/cac:ServiceProviderParty` |
| BT-ES-3 | Nombre del tercero | `…/cac:ServiceProviderParty/cac:Party/cac:PartyLegalEntity/cbc:RegistrationName` |
| BT-ES-4 | NIF del tercero | `…/cac:ServiceProviderParty/cac:Party/cac:PartyTaxScheme/cbc:CompanyID` (NIF: `TaxScheme/cbc:ID`=`LOC`, `CompanyID@schemeID`=`FC`; si no: `TaxScheme/cbc:ID`=`VAT`) |
| BT-ES-5 | Tipología de tercero | `…/cac:ServiceProviderParty/cbc:ServiceTypeCode` = `FacturaciónPorTercero` |
| BG-ES-2, BT-ES-6..9 | Impuesto o tasa no IVA a nivel de documento | `/in:Invoice/cac:Invoice/cac:AllowanceCharge/cac:TaxTotal/…` (literal; `TODO(VERIFY)` el `cac:Invoice` duplicado); código L9 en `…/cac:TaxSubTotal/cac:TaxCategory/cac:TaxScheme/cbc:TaxTypeCode`; la cuota en `TaxTotal/cbc:TaxAmount` y en `TaxSubTotal/cbc:TaxAmount` |
| BT-ES-10/11/15/16 | Identificación de cargo/impuesto/minoración/retención en nombre de tercero | `/in:Invoice/cac:CollectionInvoiceLine/cbc:ID` = `SUPL` / `IEDMT` u `OTROS` / `MINO` / `RETE` (L7) |
| BG-ES-3, BT-ES-12..14 | Desglose de impuestos y tasas cobrados por cuenta de tercero | `/in:Invoice/cac:CollectionInvoiceLine/cac:TaxTotal/cac:TaxSubtotal/…` con `TaxCategory/cbc:ID`=`S` y `TaxScheme/cbc:ID`=`VAT` |
| BT-ES-17 | Suma de retenciones | `/in:Invoice/cac:WithholdingTaxTotal/cbc:TaxAmount` |
| BG-ES-4, BT-ES-18..21 | Retención aplicada | `/in:Invoice/cac:WithholdingTaxTotal/cac:TaxSubtotal/…` con `TaxCategory/cbc:ID`=`S`, `TaxScheme/cbc:ID`=`WTH`; base `cbc:TaxableAmount`, cuota `cbc:TaxAmount`, porcentaje `TaxCategory/cbc:Percent`, concepto L5 en `TaxCategory/cac:TaxScheme/cbc:TaxTypeCode` |
| BT-ES-22 | Impuesto no IVA como cargo de línea | `/in:Invoice/cac:InvoiceLine/cac:AllowanceCharge/cac:TaxCategory/cac:TaxScheme/cbc:TaxTypeCode` (L9) |
| BT-ES-23 | Situación del inmueble | `/in:invoice/cac:InvoiceLine/cac:Item/cac:AdditionalItemProperty/cbc:ValueQualifier` (L6) |
| BT-ES-24 | Tipo de impuesto nacional (IVA/IGIC/IPSI) | `/in:invoice/cac:InvoiceLine/cac:Item/cac:AdditionalItemProperty/cbc:ValueQualifier` (calificador de L3A/L3B/L3C) |
| BT-ES-25 | URL del QR VERI\*FACTU / TicketBAI | `/in:Invoice/cac:AdditionalDocumentReference/cbc:UUID` |
| BT-ES-26 | Nº referencia de autorización de facturación | `/in:Invoice/cac:AdditionalDocumentReference/cbc:UUID` |
| BT-ES-27 | Tipo de rectificativa (R1–R4) | `/in:Invoice/cac:AdditionalDocumentReference/cbc:DocumentTypeCode` (L2.A) |
| BT-ES-28 | Modalidad de rectificativa (I/S) | `/in:Invoice/cac:AdditionalDocumentReference/cbc:DocumentTypeCode` (L2.B) |
| BT-ES-29 | Código del importe rectificado (Base/Cuota/CuotaRecargo) | `/in:Invoice/cac:AdditionalDocumentReference/cbc:DocumentTypeCode` (L2.C) |

### Otros usos específicos de términos EN 16931

- **Identificación fiscal** (BT-31/BT-32 vendedor, BT-47/BT-48 comprador): NIF español → `cac:PartyTaxScheme/cac:TaxScheme/cbc:ID` = `LOC` y `cbc:CompanyID@schemeID` = `FC`; NIF-IVA → `cac:TaxScheme/cbc:ID` = `VAT`. Uno de los dos es obligatorio. BT-30 (inscripción legal) = NIF del vendedor, obligatorio si se informa BT-32.
- **Direcciones**: municipio, código postal y provincia obligatorios cuando la dirección está en España.
- **BT-3** restringido a L1 (380, 384, 388, 389, 471).
- **BT-21** = `REG` para menciones obligatorias por normativa.
- **Categorías de IVA (L4)**: subconjunto/adaptación de UNTDID 5305 sin `K` ni `B`, con `N` para recargo de equivalencia y REGE avanzado, `D` agencias de viajes, `F` REBU.
- **BT-121 / BT-195**: sólo códigos VATEX-EU o VATEX-ES (el anexo no enumera los códigos VATEX-ES; `TODO(VERIFY)` en la lista CEF-VATEX).
- **Recargo de equivalencia** dentro de BG-23 (BT-117 = cuota del recargo; BT-119 = tipo del recargo) y sumado en euros en BT-111.
- **Clave de régimen** (BG-32 con BT-160=`REGI`, 1..2 por línea) y **referencia catastral** (BT-160=`RCAT`).
- **BG-34** reutilizado para suplidos (`SUPL`), impuestos/tasas por cuenta de tercero (`IEDMT`/`OTROS`), minoraciones (`MINO`) y retenciones (`RETE`).
- **BG-24** reutilizado (vía BT-122, lista L10) para QR fiscal, autorización de facturación y datos de rectificativa.
- **BT-113** no debe usarse para anticipos que hayan devengado IVA (art. 75.Dos LIVA).
- **BT-131/BT-116** en REGE nivel avanzado: contraprestación total / base a coste.
- **Mensaje de anulación**: `ApplicationResponse` con `ResponseCode` = `CANCELINVOICE`.

---

## 5. Reglas para TaxPain

Lo que un emisor de facturas ordinarias (autónomo o SL) debe consignar según el anexo. Cuando algo es inferencia y no texto literal, se indica.

- **Cabecera**: `cbc:ProfileID` (BT-23) = `urn:aeat:names:specification:ubl:schema:profile:B2B`; `cbc:CustomizationID` (BT-24) = `urn:cen.eu:en16931:2026#conformant#urn:aeat.es:spfe:1.0.0:extended`. BT-1 serie+número, BT-2 fecha, BT-166 hora opcional, BT-3 = `380` (ordinaria) o `384` (rectificativa), BT-5 moneda (si no es EUR: BT-6 = `EUR` y BT-111 en euros). `cbc:CopyIndicator` (BT-ES-1) sólo si es copia (`true`). BT-7 sólo si la fecha de operación difiere de la de expedición.
- **Identificadores fiscales**: emisor español con NIF → `cac:AccountingSupplierParty/cac:Party/cac:PartyTaxScheme/cbc:CompanyID` con `@schemeID="FC"` y `cac:TaxScheme/cbc:ID` = `LOC`; además BT-30 (inscripción legal) = NIF (obligatorio si se usa BT-32). Alternativamente NIF-IVA con prefijo `ES` y `cac:TaxScheme/cbc:ID` = `VAT` (BT-31). Igual para el comprador (BT-47 `LOC`/`FC`; BT-48 `VAT`); para un cliente de otro Estado miembro se usará BT-48 con su NIF-IVA (inferencia; la descripción literal de BT-48 tiene erratas). Direcciones: vía + número (BT-35/BT-50), municipio, CP y provincia obligatorios si están en España, y país ISO 3166-1.
- **Línea**: BT-126, BT-129, BT-130 (UN/ECE Rec. 20/21), BT-131, BT-146, BT-153; BG-30 con `cac:ClassifiedTaxCategory/cac:TaxScheme/cbc:ID` = `VAT`, BT-151 categoría L4 y BT-152 tipo. **Clave de régimen obligatoria en cada línea** (BG-32 1..2): `cac:AdditionalItemProperty` con BT-160 = `REGI`, BT-161 = clave L3A (normalmente `01`; `07` criterio de caja; `11` arrendamiento de local; `02` exportación) y BT-ES-24 `cbc:ValueQualifier` = `IVA`. (La ubicación exacta de BT-160/BT-161 dentro de `AdditionalItemProperty` – presumiblemente `cbc:Name`/`cbc:Value` – no figura en el anexo: `TODO(VERIFY)`.)
- **IVA 21/10/4 %**: categoría `S` en BT-151 y en BG-23 (`cac:TaxTotal/cac:TaxSubtotal`, `TaxScheme/cbc:ID`=`VAT`), un desglose por tipo con BT-116 base, BT-117 cuota, BT-118 `S`, BT-119 21/10/4.
- **Exentas**: BT-151/BT-118 = `E`, BT-119 (obligatorio, 1..1) = 0 (inferencia), BT-120 texto y/o BT-121 (y BT-195 en línea) con código VATEX-EU o VATEX-ES. El anexo no enumera los códigos VATEX-ES.
- **Entregas intracomunitarias exentas**: L4 **no tiene** `K`; el anexo no dice qué categoría usar. Lo razonable sería `E` + VATEX-EU-IC, pero queda `TODO(VERIFY)`. El comprador se identifica por BT-48 (`VAT`).
- **Inversión del sujeto pasivo**: BT-151/BT-118 = `AE` (cuota 0; razón con BT-120/BT-121 VATEX, p. ej. VATEX-EU-AE, como inferencia). **Exportación**: `G` + clave régimen `02`. **No sujeta**: `O`.
- **Recargo de equivalencia**: se informa en BG-23 como desglose propio: BT-117 = cuota del recargo, BT-119 = tipo del recargo (5,2/1,4/0,5…). El código L4 que encaja es `N` ("régimen especial del recargo de equivalencia"), pero el anexo no lo asocia explícitamente al desglose del recargo: `TODO(VERIFY)`. Si la moneda no es el euro, las cuotas del recargo se suman en BT-111. En rectificativas, `CuotaRecargo` (L2.C).
- **Retención IRPF** (se informa en dos sitios):
  1. BG-34 "RETENCIONES" (0..1): `cac:CollectionInvoiceLine` con `cbc:ID` = `RETE` (BT-ES-16), BT-179 = suma de retenciones, BT-180 = descripción.
  2. `cac:WithholdingTaxTotal/cbc:TaxAmount` (BT-ES-17) = suma, y por cada retención `cac:WithholdingTaxTotal/cac:TaxSubtotal` con `cbc:TaxableAmount` (base), `cbc:TaxAmount` (cuota), `cac:TaxCategory/cbc:ID` = `S`, `cac:TaxCategory/cbc:Percent` (p. ej. 15 / 7 / 19), `cac:TaxCategory/cac:TaxScheme/cbc:ID` = `WTH` y `cac:TaxCategory/cac:TaxScheme/cbc:TaxTypeCode` = clave L5 (`01` profesionales, `04` actividades empresariales art. 95.6.2º, `06` arrendamiento de locales, `07` propiedad intelectual…, `11` otros).
  BT-115 (total a pagar) incorpora los importes BT-179. El signo con que la retención se refleja en BT-179/BT-115 no se precisa: `TODO(VERIFY)`.
- **Suplidos**: BG-34 con `cac:CollectionInvoiceLine/cbc:ID` = `SUPL`, fuera de la base imponible.
- **Rectificativas por diferencias**: BT-3 = `384`; BG-3 con BT-25/BT-26 (serie-número y fecha de la factura rectificada); tres `cac:AdditionalDocumentReference`:
  - `cbc:ID` = `RECT:TIPO` + `cbc:DocumentTypeCode` = `R1`…`R4` (BT-ES-27);
  - `cbc:ID` = `RECT:MODALIDAD` + `cbc:DocumentTypeCode` = `I` (BT-ES-28);
  - el grupo IMPORTES RECTIFICADOS (`RECT:IMPORTES`, BT-ES-29 = `Base`/`Cuota`/`CuotaRecargo`, BT-123 = importe) se describe sólo para la rectificación **por sustitución** (`S`) con factura única de importes correctos, así que no aplica a la de diferencias.
- **QR fiscal**: `cac:AdditionalDocumentReference` con `cbc:ID` = `QR` (BT-122), BT-123 = `VERIFACTU` (o `TICKETBAI`) y la URL del QR en `cbc:UUID` (BT-ES-25). BG-24 cardinalidad 0..1.
- **Notas**: menciones legales obligatorias (p. ej. "Inversión del sujeto pasivo", exención) como BG-1 con BT-21 = `REG` (inferencia sobre qué menciones; la regla `REG` es literal).
- **Pago**: BT-9 vencimiento (Ley 3/2004) recomendable; BT-ES-2 `cac:PrepaidPayment/cbc:PaidDate` si ya está pagada; BG-16 BT-81 (UNTDID 4461) + BT-84 IBAN si hay transferencia.
- **Anulación**: `ApplicationResponse` con `cac:Response/cbc:ResponseCode` = `CANCELINVOICE`, `cbc:EffectiveDate`, y por factura `cac:DocumentReference` (`cbc:ID`, `cbc:IssueDate`, `cac:IssuerParty` con NIF y razón social).
- **Facturación por tercero o destinatario**: BG-ES-1 con `cbc:ServiceTypeCode` = `FacturaciónPorTercero`; autofactura BT-3 = `389` (rectificativa `471`).

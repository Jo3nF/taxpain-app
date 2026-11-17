# Reglas contables (PGC PYMES) y libros registro AEAT

> Investigado el **2026-10-13** (solo GET). `TODO(VERIFY)` = no está literalmente en una fuente oficial. **No usar en código sin verificar.**

**Fuentes**
- `[PGCP]` RD 1515/2007, PGC PYMES, consolidado (última actualización 30/01/2021): https://www.boe.es/buscar/act.php?id=BOE-A-2007-19966 — Cuarta parte (cuadro de cuentas) y Quinta parte (definiciones y relaciones contables). Datos: `fixtures/accounting/pgc-pymes-cuentas.json`, `fixtures/accounting/cuentas-anuales-pymes.json`.
- `[LIS]` Ley 27/2014, art. 12 · `[TAS]` Orden 27/03/1998 (tabla simplificada IRPF) → `fixtures/accounting/amortizacion.json`.
- `[LLR]` AEAT, «Formatos de los Libros Registro de IVA e IRPF…», Especificaciones 2025-2026, **V.18.02.2026** (página «actualizado 01-01-2026»): https://www.agenciatributaria.es/AEAT.desarrolladores/Desarrolladores/_menu_/Documentacion/IVA/Modelo_303/Formatos_Libros_Registro_IVA_para_Requerimientos/Formatos_Libros_Registro_IVA_para_Requerimientos.html — PDF `…/static_files/Sede/Tema/IVA/Fact_registro/Libros_registro/Formato_Electronico_Comun_Libros_Registro_IVA_IRPF.pdf` y diseño `https://www.agenciatributaria.es/static_files/AEAT/LSI.xlsx`.

Los **movimientos** («se cargará / se abonará con …») vienen de la Quinta parte de `[PGCP]`. Los **asientos tipo** que siguen son la aplicación de esos movimientos; el PGC no los publica como "asientos modelo" (los ejemplos de importes son nuestros). Grupos 8 y 9 **no existen** en el PGC PYMES.

## 1. Factura emitida (venta / prestación de servicios)

| Debe | Haber |
|---|---|
| 430 Clientes — total a cobrar (base + IVA − retención) | 700 Ventas de mercaderías / 705 Prestaciones de servicios — base |
| 473 Hacienda Pública, retenciones y pagos a cuenta — retención IRPF que nos practica el cliente | 477 Hacienda Pública, IVA repercutido — cuota |

- 473: «Se cargará por el importe de la retención o pago a cuenta» `[PGCP]`. En SL se compensa luego contra 6300 (IS); en autónomo es pago a cuenta del IRPF del titular. TODO(VERIFY): el PGC no define el tratamiento en un empresario individual (el IRPF no es gasto de la actividad; práctica habitual: saldar 473 contra la cuenta del titular, p. ej. 550 Titular de la explotación).
- Rectificativa por diferencias: mismo asiento con importes negativos o 708 Devoluciones de ventas y operaciones similares (Debe) — 472/477 «se cargará o se abonará … en los casos de alteraciones de precios posteriores» `[PGCP]`.
- Ejemplo: base 1.000, IVA 21 % 210, retención 15 % 150 → 430 = 1.060; 473 = 150 | 705 = 1.000; 477 = 210.

## 2. Factura recibida (compra / gasto)

| Debe | Haber |
|---|---|
| 600 Compras de mercaderías / 602 Compras de otros aprovisionamientos / 62x (621 Arrendamientos y cánones, 623 Servicios de profesionales independientes, 628 Suministros, 629 Otros servicios…) — base | 400 Proveedores (mercaderías, grupo 3) / 410 Acreedores por prestaciones de servicios — total a pagar |
| 472 Hacienda Pública, IVA soportado — cuota deducible | 4751 Hacienda Pública, acreedora por retenciones practicadas — retención que practicamos (profesional: modelo 111; alquiler: modelo 115) |

- 472 «Se cargará por el importe del IVA deducible cuando se devengue el impuesto, con abono a cuentas de acreedores o proveedores» `[PGCP]`. IVA no deducible = mayor gasto (TODO(VERIFY): no hay cita literal en la definición de 472; criterio general de valoración).
- 4751 «Se abonará al devengo del tributo, cuando la empresa sea sustituto del contribuyente o retenedor, con cargo a cuentas de los grupos 4, 5 ó 6» `[PGCP]`.
- Inmovilizado (p. ej. 217 Equipos para procesos de información) → contrapartida 523 Proveedores de inmovilizado a corto plazo (no 400/410).

## 3. Cobros y pagos

| Operación | Debe | Haber |
|---|---|---|
| Cobro de cliente | 572 Bancos e instituciones de crédito c/c vista, euros | 430 Clientes |
| Pago a proveedor / acreedor | 400 Proveedores / 410 Acreedores por prestaciones de servicios | 572 |
| Comisión bancaria (N43 concepto 17) | 626 Servicios bancarios y similares | 572 |

Cuentas de tesorería según el cuadro de cuentas y las definiciones del PGC (RD 1514/2007, cuarta y quinta parte; comprobado el 26-10-2026 en el texto consolidado del BOE, <https://www.boe.es/buscar/pdf/2007/BOE-A-2007-19884-consolidado.pdf>). Las definiciones y relaciones contables no son de aplicación obligatoria, así que una gestoría puede usar otra subcuenta sin que sea un error:

- **572** «Bancos e instituciones de crédito c/c vista, euros» y **573** «… moneda extranjera» (574/575 para cuentas de ahorro): subcuentas 572x para euros y 573x para otra divisa. El contravalor y la valoración al cierre (NRV 11.ª) son otra cuestión, pendiente como función.
- **5201** «Deudas a corto plazo por crédito dispuesto» para las tarjetas de crédito: «se abonará por las cantidades dispuestas, con cargo, generalmente, a cuentas del subgrupo 57; se cargará por la cancelación… con abono a cuentas del subgrupo 57», que es el ciclo compra con tarjeta → liquidación mensual contra la 572. La 5208 es «Deudas por efectos descontados» (no aplica) y la 555 «Partidas pendientes de aplicación» es una cuenta puente, no de deuda.
- **Monederos**: PayPal (Europe) S.à r.l. et Cie, S.C.A. es una institución de crédito supervisada por la CSSF, y la definición de 572/573 abarca «Bancos e Instituciones de Crédito… y entidades análogas si se trata de saldos situados en el extranjero»: 572 (EUR) o 573 (otra divisa). Stripe (Stripe Technology Europe Ltd) es una entidad de dinero electrónico (Banco Central de Irlanda, C187865), no una institución de crédito: su saldo pendiente de liquidar encaja mejor en una 55x (551/555) o en un 4xx de la pasarela. TODO(VERIFY): sólo la cuenta de Stripe.

## 4. Liquidación trimestral de IVA (modelo 303)

Al terminar el periodo:

| Debe | Haber |
|---|---|
| 477 Hacienda Pública, IVA repercutido — saldo del periodo | 472 Hacienda Pública, IVA soportado — saldo del periodo |
| (si 472 > 477) 4700 Hacienda Pública, deudora por IVA — diferencia | (si 477 > 472) 4750 Hacienda Pública, acreedora por IVA — diferencia |

- 4750 «Se abonará al terminar cada período de liquidación por el importe del mencionado exceso, con cargo a la cuenta 477. Se cargará … cuando se efectúe su pago, con abono a cuentas del subgrupo 57.» `[PGCP]`
- 4700 «Se cargará al terminar cada período de liquidación … con abono a la cuenta 472. Se abonará: b1) En caso de compensación en declaración-liquidación posterior, con cargo a la cuenta 477. b2) En los casos de devolución … con cargo a cuentas del subgrupo 57.» `[PGCP]`
- Pago: 4750 / 572. Devolución cobrada: 572 / 4700. Compensación en un periodo posterior: 477 / 4700.
- En TaxPain la liquidación sale del 303 del periodo (`settle_vat_quarter`): 4700 al haber por la
  compensación aplicada ([78]) y 4750 al haber sólo por el resultado a ingresar ([71], el mismo
  importe que paga `tax_payments::pay`), o 4700 al debe por el nuevo exceso. Si el 303 no coincide
  con 477 − 472 (regularizaciones [43]/[44], casillas tocadas a mano) la diferencia va a 634/639
  (ajustes en la imposición indirecta). El cierre guiado detecta los saldos simultáneos de la 4700
  y la 4750 que dejaban las liquidaciones antiguas y propone el asiento 4750 / 4700, con confirmación.
- Leasing: cada cuota es una factura recibida del arrendador (capital a la 524, intereses a la 662,
  IVA a la 472, contra el arrendador), que entra en el 303, el libro registro y la cartera, y se paga
  conciliando el cargo del banco. Las cuotas del sistema antiguo (asiento directo contra la 572, sin
  factura) se regularizan desde Leasing con confirmación: se registra la factura del arrendador
  (su IVA va al 303 del periodo en que se registra, art. 99.Tres LIVA), se da por pagada el día de
  la cuota desde la misma cuenta y se borra el asiento antiguo; no si su ejercicio está cerrado,
  el 303 de su trimestre presentado o su IVA ya liquidado en el diario. La factura real del
  arrendador de una cuota ya registrada (misma fecha ±10 días e importe ±1 €) se rechaza en
  Recibidas y no se importa del libro del gestor. Al revés (la factura real ya estaba en Recibidas
  al pulsar «Registrar cuotas»: mismo número, o importe ±1 € con fecha ±5 días), la cuota se
  enlaza con ella en vez de registrar otra: su gasto del grupo 6 pasa a capital (524) e intereses
  (662) si el ejercicio está abierto, y vence el día de la cuota si aún no está pagada. Las cuotas
  registradas antes de octubre de 2026 vencían a +30 días: «Corregir vencimientos» pone cada una
  (sólo sin pagar ni conciliar) en su fecha de cuota. Un contrato dado de alta por error se borra
  con confirmación (contrato, cuotas previstas, alta, paso a la 174 e inmovilizado) sólo si no
  tiene cuotas contabilizadas, factura del arrendador en Recibidas ni amortización, y si sus
  asientos son de ejercicios abiertos.

## 5. Pago de retenciones (modelos 111 y 115)

| Debe | Haber |
|---|---|
| 4751 Hacienda Pública, acreedora por retenciones practicadas | 572 |

Recomendado: subcuentas de 4751 por modelo (p. ej. 4751.111 / 4751.115). El PGC solo exige el nivel de 4 dígitos; el desglose es nuestro (TODO(VERIFY) numeración interna).

## 6. Nómina básica

Devengo:

| Debe | Haber |
|---|---|
| 640 Sueldos y salarios — bruto | 4751 — retención IRPF del trabajador |
| 642 Seguridad Social a cargo de la empresa — cuota patronal | 476 Organismos de la Seguridad Social, acreedores — cuota obrera + patronal |
| | 465 Remuneraciones pendientes de pago — líquido |

Pagos: 465 / 572 (líquido; N43 concepto 15) · 476 / 572 (seguros sociales) · 4751 / 572 (modelo 111).
- Préstamos (NRV 9.ª): alta contra la tesorería con lo que vence en doce meses en la cuenta de
  corto plazo; cada cuota lleva el capital a la de corto plazo, los intereses a la 762/662 (menos
  lo ya devengado, que sale de la 547/534x o 527/528/514x) y la retención del 19 % a la 473 (nos
  retienen) o a la 4751 (retenemos: 123 y 193, clave B naturaleza 03); en carencia total los
  intereses se capitalizan contra la cuenta de largo plazo. Devengo a fecha proporcional a los
  días del periodo. Al cierre, la cuenta de largo plazo se deja con el capital que vence a más
  de doce meses según el cuadro (por saldos y agrupando préstamos con las mismas cuentas). Tipo
  nominal: sin coste amortizado con comisiones (`TODO(VERIFY)`).
- 465 «Se abonará por las remuneraciones devengadas y no pagadas, con cargo a las cuentas 640 y 641. Se cargará cuando se paguen … con abono a cuentas del subgrupo 57.» `[PGCP]`
- Cuota de autónomo (RETA) del titular: TODO(VERIFY) — el PGC no tiene una cuenta específica; práctica habitual 642 o 649 Otros gastos sociales.

## 7. Amortización (cierre del ejercicio o periodo)

| Debe | Haber |
|---|---|
| 680 Amortización del inmovilizado intangible | 280 Amortización acumulada del inmovilizado intangible (2800–2806) |
| 681 Amortización del inmovilizado material | 281 Amortización acumulada del inmovilizado material (2811–2819, p. ej. 2817 … de equipos para procesos de información) |
| 682 Amortización de las inversiones inmobiliarias | 282 Amortización acumulada de las inversiones inmobiliarias |

Cuota fiscal máxima: tabla `[LIS]` art. 12.1.a (SL y ED normal) o tabla simplificada `[TAS]` (IRPF ED simplificada); ver `amortizacion.json`.

### Baja y venta (`dispose_asset_with_invoice`, `crates/app/src/venta_inmovilizado.rs`)

Ejemplo: máquina (213) de 12.000 € con 2.400 amortizados al empezar el año, vendida el 30/6 por 10.000 € + IVA.

| Asiento | Debe | Haber |
|---|---|---|
| Amortización hasta la baja (1.200 × 181/365) | 681 595,07 | 2813 595,07 |
| Baja | 2813 2.995,07 · 543 10.000 | 213 12.000 · 771 995,07 |
| Factura de venta (la de siempre) | 4300001 12.100 | 705 10.000 · 477 2.100 |
| Factura vinculada a la venta (fecha de la factura) | 705 10.000 | 543 10.000 |

- Resultado por grupo del elemento: intangible 670/770, material 671/771, inversiones inmobiliarias 672/772 (cuentas 670–672 y 770–772 `[PGCP]`; NRV 2.ª: la diferencia entre el importe neto recibido y el valor contable va a resultados).
- La baja deja el precio en la 543 (créditos por enajenación de inmovilizado). Vincular la factura de venta (al dar de baja o después) pasa su base de la cuenta de ventas a la 543, con un asiento con la fecha de la factura (en su ejercicio): el ingreso cuenta una vez y el cobro sigue en la 430 de la factura. Si la base no es el precio de la baja, la diferencia va al resultado de la venta, y lo mismo las rectificativas de la factura. TODO(VERIFY): el PGC lleva a la 543 el crédito por la venta; dejarlo en la 430 de la factura es una simplificación (si no se cobra al cierre, sale en «Clientes» y no en «Inversiones financieras a corto plazo»).
- Cada cambio del precio va primero contra el resultado que la venta ya tiene en ese ejercicio: una rebaja quita beneficio antes de dar pérdida y una subida quita pérdida antes de dar beneficio. Así una venta acaba con beneficio (77x) o con pérdida (67x), no con los dos (la 671 es la pérdida de la baja y la 771 su beneficio). Como cada asiento cuenta con los anteriores, las facturas se desvinculan en orden inverso.
- La factura de venta sólo se vincula si la 543 aún tiene el precio de la baja. Si un cobro conciliado contra la 543 o un asiento manual ya la ha bajado, vincularla la dejaría en negativo y el cobro contaría dos veces (en la 543 y en la 430). La 543 es una para todas las ventas: lo que aún tiene cada venta sin factura sale de su saldo, repartido de la venta más reciente a la más antigua.
- Una factura de un ejercicio anterior al de la baja (facturada antes de la entrega) también se vincula. Su asiento va en ese ejercicio: sus ventas quedan sin ella y la 543 queda allí con saldo acreedor hasta la baja, como lo facturado por adelantado.
- Con la amortización del año ya registrada hasta el cierre, la baja rehace ese asiento hasta el día de la baja (equivale a 28x a 68x por lo que pasaba de ese día) y quita la de ejercicios posteriores. Los asientos que se borran solos (esa amortización, o el que pasa una factura a la venta al desvincularla) no se llevan sus documentos ni sus observaciones: pasan al asiento de la baja o al de la factura.

## 8. Impuesto sobre Sociedades (solo SL)

6300 Impuesto corriente (Debe) / 473 (Haber, retenciones y pagos fraccionados hasta la cuota líquida) / 4752 Hacienda Pública, acreedora por impuesto sobre sociedades (Haber, cuota a ingresar) o 4709 Hacienda Pública, deudora por devolución de impuestos (Debe, si 473 > cuota). Fuente: definiciones de 473, 4709 y 4752 `[PGCP]`.

## 9. Regularización (determinación del resultado)

- Cuentas de los grupos 6 y 7 con saldo **deudor**: 129 Resultado del ejercicio (Debe) / cuentas 6xx (Haber).
- Cuentas con saldo **acreedor**: cuentas 7xx (Debe) / 129 (Haber).
- 129 «Se abonará … con cargo a las cuentas de los grupos 6 y 7 que presenten al final del ejercicio saldo acreedor. Se cargará … con abono a las cuentas de los grupos 6 y 7 que presenten al final del ejercicio saldo deudor.» `[PGCP]`; grupo 6: «En general todas las cuentas del grupo 6 se abonan, al cierre del ejercicio, con cargo a la cuenta 129».
- Excepciones: algunas cuentas de los grupos 6 y 7 no van contra 129 (p. ej. 6300 y 6301 tienen movimientos propios). TODO(VERIFY) lista completa de excepciones en la Quinta parte.

## 10. Cierre y apertura

- **Cierre:** asiento único que salda todas las cuentas patrimoniales (grupos 1–5, incluida 129): cuentas con saldo deudor al Haber y con saldo acreedor al Debe. **Apertura** (1 de enero): el inverso exacto del cierre.
- Aplicación del resultado (tras la junta, SL): 129 / 112 Reserva legal, 113 Reservas voluntarias, 120 Remanente, 526 Dividendo activo a pagar…; pérdidas: 121 Resultados negativos de ejercicios anteriores / 129 («Por el traspaso del resultado negativo, con cargo a la cuenta 121» `[PGCP]`).
- TODO(VERIFY): el PGC PYMES **no regula literalmente** los asientos de cierre y apertura; son práctica contable (Código de Comercio art. 28 exige el Libro Diario y el de Inventarios y Cuentas anuales — no comprobado en esta sesión).

## 11. Formato unificado de los libros registro IVA/IRPF (AEAT)

`[LLR]` define el formato para requerimientos y para importar en Renta Web (modelo 100), modelo 130 y Pre303. No es una Orden ministerial: es una especificación AEAT. No se ha localizado una «Orden HAP/2250/2015» que lo regule (TODO(VERIFY) si hay base normativa adicional).

- **Fichero:** **XLSX** («El tipo de fichero deberá ser XLSX»). Las FAQ Pre303 mencionan también CSV con cabeceras del diseño (TODO(VERIFY): el PDF V.18.02.2026 solo cita XLSX).
- **Nombre:** Ejercicio + NIF + tipo de libro + Nombre o razón social. Ejemplo del diseño: `202699999999R@español español, juan`, donde `@` es el tipo:
  - fichero completo: `C` (todos IVA: pestañas EXPEDIDAS, RECIBIDAS, BIENES-INVERSIÓN), `D` (todos IRPF: INGRESOS, GASTOS, BIENES-INVERSIÓN), `T` (unificado: EXPEDIDAS_INGRESOS, RECIBIDAS_GASTOS, BIENES-INVERSIÓN);
  - por libro: `U` (unificado expedidas/ingresos), `E` (facturas emitidas IVA), `I` (ventas e ingresos IRPF); `V` (unificado recibidas/gastos), `R` (recibidas IVA), `G` (compras y gastos IRPF).
- **Formatos de dato:** `Decimal(12,2)` con coma decimal, sin separador de miles, signo `-` a la izquierda (`66325,12`, `-502,36`); tipos `0,00` para distinguir recargo 0 %; fechas `dd/mm/yyyy`; `Alfanumérico (n)` = longitud máxima. Un movimiento por línea; una factura con varios tipos/conceptos/cobros → una línea por cada uno.
- Los libros se presentan **sin fraccionar por trimestre** (todo el ejercicio).

**Columnas de EXPEDIDAS_INGRESOS** (diseño LSI.xlsx, hoja 1; `[IVA]`/`[IRPF]` = solo ese impuesto):
Autoliquidación: Ejercicio `Decimal(4,0)`, Periodo `A(2)` · Actividad: Código `A(1)`, Tipo `A(2)`, Grupo o Epígrafe del IAE `A(4)` · Tipo de Factura `A(2)` · Concepto de Ingreso `A(3)` [IRPF] · Ingreso Computable `D(12,2)` [IRPF] · Fecha Expedición · Fecha Operación · Identificación de la Factura: Serie `A(20)`, Número `A(20)`, Número-Final `A(20)` · NIF Destinatario: Tipo `A(2)`, Código País `A(2)`, Identificación `A(20)` · Nombre Destinatario `A(40)` · Clave de Operación `A(2)` [IVA] · Calificación de la Operación `A(2)` [IVA] · Operación Exenta `A(2)` [IVA] · Total Factura · Base Imponible · Tipo de IVA `D(4,2)` · Cuota IVA Repercutida · Tipo de Recargo Eq. `D(4,2)` · Cuota Recargo Eq. · Cobro (criterio de caja IVA y/o art. 7.2.1º RIRPF): Fecha, Importe, Medio Utilizado `A(2)`, Identificación Medio Utilizado `A(34)` · Tipo Retención del IRPF `D(4,2)` [IRPF] · Importe Retenido del IRPF [IRPF] · Registro Acuerdo Facturación `A(15)` [IVA] · Inmueble: Situación `A(1)`, Referencia Catastral `A(20)` · Referencia Externa `A(40)`. (36 columnas, A–AJ.)

**Columnas de RECIBIDAS_GASTOS** (hoja 2):
Autoliquidación: Ejercicio, Periodo · Actividad: Código, Tipo, Grupo o Epígrafe del IAE · Tipo de Factura · Concepto de Gasto `A(3)` [IRPF] · Gasto Deducible [IRPF] · Fecha Expedición · Fecha Operación · Identificación Factura del Expedidor: (Serie-Número) `A(40)`, Número-Final `A(20)` · Fecha Recepción [IVA] · Número Recepción `A(20)` · Número Recepción Final `A(20)` · NIF Expedidor: Tipo, Código País, Identificación · Nombre Expedidor `A(40)` · Clave de Operación [IVA] · Bien de Inversión `A(1)` [IVA] · Inversión del Sujeto Pasivo `A(1)` [IVA] · Deducible en Periodo Posterior `A(1)` [IVA] · Periodo Deducción: Ejercicio, Periodo [IVA] · Total Factura · Base Imponible · Tipo de IVA · Cuota IVA Soportado · Cuota Deducible · Tipo de Recargo Eq. · Cuota Recargo Eq. · Pago: Fecha, Importe, Medio Utilizado, Identificación Medio Utilizado · Tipo Retención del IRPF [IRPF] · Importe Retenido del IRPF [IRPF] · Registro Acuerdo Facturación [IVA] · Inmueble: Situación, Referencia Catastral · Referencia Externa. (42 columnas, A–AP.)

**BIENES-INVERSIÓN** (hoja 3): Autoliquidación, Actividad, Tipo de Bien, Descripción del Bien (Identificador `A(40)`, Literal `A(160)`), Fecha Inicio Utilización, Valor Adquisición, Valor Amortizable, Método de Amortización (01 lineal según tablas, 02 porcentaje constante, …), amortización Acumulada al Inicio / Cuota Resultante / Acumulada al final / Pendiente…, datos IVA de regularización y Baja (Causa 01 Amortización total, 02 Venta o transmisión, 03 Deterioro o destrucción, 04 Retirada…). TODO(VERIFY): transcribir todas las columnas de la hoja 3 desde LSI.xlsx.

TODO(VERIFY): los catálogos de valores (Tipo de Factura, Concepto de Ingreso/Gasto, Clave de Operación, Calificación, Exenta, Medio de pago) están en las notas numeradas de cada hoja de LSI.xlsx (filas «VALIDACIONES») y no se han transcrito aquí.

## 12. Ejercicios del gestor (histórico importado asiento a asiento)

Una empresa que llevaba la contabilidad en una gestoría (a3, Sage, ContaSOL…) carga en TaxPain un
ejercicio ya cerrado entero (`crates/app/src/ejercicio_gestor.rs`): el diario completo del gestor
(hoja de cálculo o CSV, una hoja por mes o todo junto; el «Diario.» de a3 con cuatro filas de
cabecera, `Fecha | Asiento | Apunte | Concepto | Documento | Cuenta | Descripción de la cuenta |
Importe debe | Importe haber`, fecha y número sólo en la primera línea de cada asiento y una fila
«Total en <mes>» al final), el diario resumido de a3 (`Fecha | Importe Debe | Cuenta Debe |
Descripción Cuenta | Cuenta Haber | Importe Haber`: una línea por cuenta y mes, que se agrupa en un
asiento por fecha), el SUENLACE o un CSV con esas columnas.

- **Un asiento por operación y todos cuadrados** (art. 28 CCom; invariante del diario): se agrupa
  por número de asiento (o por fecha y bloque si no lo hay) y si uno no cuadra se dice la fila y
  no se importa nada. Las fechas tienen que caer dentro del ejercicio.
- **Apertura, regularización y cierre**: se reconocen por el documento o el concepto («Apertura»;
  «Regularización» con la 129; «Cierre» con sólo cuentas de balance en la última fecha) o, la
  apertura, por ser el primer asiento del 1 de enero sólo con grupos 1–5. Entran con sus
  orígenes de siempre (`Apertura`, `Regularizacion`, `Cierre`); el resto, con `Gestor(lote)`.
  Si falta el cierre, el ejercicio queda abierto y lo cierra TaxPain (`cierre.rs`: la
  regularización que falte, el cierre y la apertura del siguiente, sección 10). Si viene, el
  ejercicio queda cerrado y se crea la apertura del siguiente (el inverso exacto del cierre)
  salvo que ya la tenga.
- **Un ejercicio vacío**: sólo se carga si no tiene asientos en TaxPain (si los tiene, se explica
  cuáles son y cómo quitarlos). Las cuentas que faltan se crean con el código y el nombre del
  gestor sin tocar; las subcuentas sin nombre toman el de su cuenta del PGC.
- **Nada automático**: los asientos del gestor son histórico. No entran en la cartera ni en los
  303/130 (que salen de las facturas de TaxPain); lo que lee el diario por cuentas (200, balance,
  mayor, nóminas del 111 de un diario importado) los ve como a los importados. No se editan uno a
  uno: el lote se deshace entero (`gestor_import`) mientras el ejercicio no se haya cerrado con
  TaxPain.
- **Cotejo**: por cuenta con el balance de sumas y saldos del gestor (a3 «Sumas y saldos.»:
  `Cuenta | Descripción | Saldo inicial | Debe | Haber | Debe acum. | Haber acum. | Saldo deudor |
  Saldo acreedor`), sin el cierre y con o sin la regularización según lo que lleve el balance; y
  con los modelos presentados guardados: 303 de cada trimestre (477 devengada frente a la [27],
  472 deducible frente a la [45], sin contar el asiento de liquidación), 130 del autónomo si hay
  alguno guardado (ingresos del grupo 7 y gastos del grupo 6 acumulados hasta el fin del
  trimestre frente a [01] y [02], y su diferencia frente a [03]; art. 110 RIRPF) y 200 (resultado
  de los grupos 6 y 7 frente a la [500]). TODO(VERIFY): las diferencias del 303 pueden ser IVA no
  deducible, prorrata o facturas de otro periodo; las del 130, el 5 % de difícil justificación de
  la simplificada (dentro de la [02]), la amortización contabilizada al cierre o gastos de otro
  trimestre; las del 200, ajustes extracontables: se muestran en llano para preguntar al gestor,
  no se corrigen.
- **Inventario de activos** (a3 «Inventario de Activos»: `Código de Activo | Descripción |
  Ampliación Activo | Valor Adquisición | Valor contable | Fecha Ini Amort.`, con filas «Total
  Cta. Inmov.» que llevan la cuenta de las filas de encima): cada bien pasa al asistente de
  saldos iniciales con `amortizado antes de TaxPain` = adquisición − valor contable, su fecha de
  inicio de amortización y su cuenta; el coeficiente lo propone la cuenta (tabla del art. 12.1.a
  LIS: 217 → 25 %, 216 → 10 %, 218 → 16 %, 213 → 12 %, 214 → 25 %, 212/215 → 10 %, 211 → 2 %,
  206 → 33 %) y lo elige el usuario. El 2 % de la 211 es el de «Edificios comerciales,
  administrativos, de servicios y viviendas» (100 años); los «Edificios industriales» van al 3 %
  (68 años): tabla del art. 12.1.a LIS leída en el BOE el 27-10-2026; el usuario lo cambia si el
  edificio es industrial.

## 13. Criptomonedas (`crates/app/src/cripto`, `taxpain_core::cripto`)

Investigado el 2026-10-26 en el BOE (arts. 35 y 37 LIRPF, art. 13.2 LIS, art. 42 quater RGAT) y
en fuentes secundarias; el 27-10-2026 se cotejó con el índice de consultas del ICAC, la Orden
HAC/277/2026 (modelo 100 de Renta 2025), la ayuda de Renta Web 2025 y las consultas de la DGT en
petete (ver PROGRESS, «Dudas cerradas el 2026-10-27»).

- **Qué son**: ICAC, BOICAC 120 (diciembre de 2019), **consulta 4** «Emisión de criptomoneda.
  NRV 10.ª»: **existencias** (grupo 3) si se tienen para venderlas en el curso ordinario de la
  actividad; si no, **inmovilizado intangible** (grupo 20); nunca tesorería ni activo financiero.
- **Valoración**: coste de adquisición = euros pagados + comisión (NRV 10.ª.1 o 5.ª PGC). Salidas
  por **precio medio ponderado** o **FIFO** (NRV 10.ª.1.3: «precio medio o coste medio ponderado…
  El método FIFO es aceptable»). La app guarda el método elegido en `cripto_settings` y enseña
  siempre los dos; contabiliza y declara con el elegido (persona física: FIFO siempre).
- **Permuta cripto-cripto**: transmisión del entregado por su valor de mercado y alta del recibido
  por ese valor (NRV 2.ª.1.3 PGC; art. 37.1.h LIRPF, literal comprobado: «el mayor de los dos
  siguientes: el valor de mercado del bien o derecho entregado; el valor de mercado del bien o
  derecho que se recibe a cambio»; DGT V0999-18). Traspaso entre plataformas propias: nada.
- **Comisiones**: en la compra, mayor coste; en la venta, menor valor de transmisión (art. 35 LIRPF:
  «gastos y tributos inherentes»); las de traspasos y las pagadas en cripto, gasto (626).
- **Recompensas** (staking, airdrop): ingreso por el valor de mercado (769) y ese valor es el coste.
  En IRPF (consultas leídas en petete el 27-10-2026): el staking es rendimiento del capital
  mobiliario por cesión a terceros de capitales propios, en especie, valorado «por su valor de
  mercado en euros el día de su percepción», base del ahorro y sin ingreso a cuenta porque no hay
  obligado a retener (V1766-22, de 26-7-2022); los airdrops son ganancia patrimonial sin
  transmisión del art. 37.1.l LIRPF, por el valor de mercado en euros al recibirlos, en la **base
  general** (V1948-21, de 21-6-2021). V2520-22 (7-12-2022) es la de las permutas cripto-cripto y
  el FIFO, no la de los airdrops.
- **Deterioro al cierre** (sólo si el precio de cierre deja el valor por debajo del coste):
  existencias 693 / 390 (deducible: no está en el art. 13.2 LIS), intangible 690 / 290 (**no
  deducible**: art. 13.2.a LIS, literal comprobado: «No serán deducibles: a) Las pérdidas por
  deterioro del inmovilizado material, inversiones inmobiliarias e inmovilizado intangible»). Se
  revierte antes el del ejercicio anterior (793/790). Cuentas configurables; subcuenta por moneda
  (3090001 BTC…), con lo que cada moneda queda «registrada de forma individualizada».
  El PGC no tiene cuenta para criptomonedas y la 209 ya es «Anticipos para inmovilizaciones
  intangibles»: por defecto el intangible va a la **207** (libre en el subgrupo 20; configurable).
  La app no amortiza el intangible aunque el art. 39.4 CCom y la NRV 5.ª obligan (diez años si la
  vida útil no se puede estimar; fiscalmente deducible hasta 1/20 anual, art. 12.2 LIS):
  pendiente de decisión.

| Operación | Debe | Haber |
|---|---|---|
| Compra (existencias o intangible) | 309x / 207x (euros + comisión) | 572x (o la cuenta de euros de la plataforma) |
| Venta (existencias) | 572x (euros − comisión), 626 (comisión), 610 (coste) | 700 (euros), 309x (coste) |
| Venta (intangible) | 572x (euros − comisión), 626 (comisión), [671 pérdida] | 207x (coste), [771 beneficio] |
| Permuta | 309y/207y (valor de mercado), 610 (coste) [o 671/771 neto], 626 (comisión) | 700 (valor) [existencias], 309x/207x (coste), 572x (comisión) |
| Pago de un gasto en cripto | 555x puente (valor − comisión), 626, 610 | 700, 309x — la 555 se salda contra el proveedor |
| Cobro de una factura en cripto | 309x (valor + comisión) | 555x puente — se salda contra el cliente |
| Comisión pagada en cripto | 626 (valor), 610 (coste) | 700 (valor), 309x (coste) |
| Recompensa | 309x (valor de mercado) | 769 |
| Pérdida o robo | 610 (existencias) / 671 (intangible) | 309x / 207x (coste) |
| Traspaso con comisión en euros | 626 | 572x |
| Deterioro al cierre | 390/290 (reversión del anterior), 693/690 (nuevo) | 793/790, 390/290 |

- **IRPF (persona física)**: ganancias y pérdidas patrimoniales de la base del ahorro (arts. 33, 46
  y 49 LIRPF), no rendimiento de la actividad salvo trading profesional (TODO(VERIFY): criterio
  para considerarlo actividad). Identificación de lo transmitido: art. 37.2 LIRPF («aquéllos que
  adquirió en primer lugar»; literal para valores homogéneos; aplicado por analogía a las monedas
  virtuales por la DGT: V0999-18, V0975-22 y V2520-22, leída: «las criptomonedas que se transmiten
  son las adquiridas en primer lugar». En contra, la STSJ del País Vasco 37/2025, de 9-1-2025
  (rec. 75/2024, ECLI:ES:TSJPV:2025:41), con la norma foral de Bizkaia: no afecta al territorio
  común). Casillas del modelo 100 (anexo I de la Orden HAC/277/2026, Renta 2025): apartado
  «transmisión o permuta de monedas virtuales por particulares», [1800] a [1814] ([1804] valor de
  transmisión, [1806] adquisición, [1808] pérdida imputable, [1812] ganancia imputable, [1813] y
  [1814] sumas). TODO(VERIFY): las letras de la clave de la [1803] (F/N/B) son del desplegable de
  Renta Web y no constan en la orden. El FIFO de la Renta se calcula **global por activo**, sin
  distinguir plataforma: la ayuda de Renta Web 2025 (7.6.6.2) lo dice expresamente («aunque las
  operaciones se hayan realizado en distintos exchanges»).
- **Modelo 721** (Orden HFP/886/2023, BOE 29-7-2023; art. 42 quater RGAT, literal comprobado):
  saldos a 31/12 en plataformas del extranjero valorados con «la cotización a 31 de diciembre que
  ofrezcan las principales plataformas de negociación»; no se declara si «no superen,
  conjuntamente, los 50.000 euros»; plazo 1 de enero a 31 de marzo; años sucesivos sólo con
  «incremento superior a 20.000 euros». **Exentas** (42 quater.5.b y c) las personas jurídicas y
  las físicas con contabilidad por el Código de Comercio que las tengan «registradas en su
  contabilidad de forma individualizada e identificadas por su denominación, valor y entidad de
  custodia y país». «Del extranjero» = custodio que no presenta el 172/173 en España (42 quater.2);
  TODO(VERIFY): qué exchanges con filial española lo presentan: no hay lista oficial (buscado el
  27-10-2026) y los registros del Banco de España y de la CNMV no dicen qué entidad custodia cada
  cuenta, así que la app marca Kraken, Binance, Coinbase… como del extranjero por defecto y el
  usuario lo corrige mirando con qué entidad tiene el contrato. Los **172/173** (Orden
  HFP/887/2023) los presentan las plataformas, no el usuario.

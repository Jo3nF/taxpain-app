# Reglas del calendario fiscal (AEAT, territorio común)

Consultado el 2026-10-13, solo con lecturas GET públicas. Datos de prueba: `fixtures/calendar/aeat-2026.json` (99 vencimientos oficiales de 2026) y `fixtures/calendar/festivos-nacionales.json` (BOE 2026 y 2027).
Fuentes base: Calendario del contribuyente 2026 (`SEDE/Sede/ayuda/calendario-contribuyente/calendario-contribuyente-2026/...`), la tabla de la AEAT «Plazos de presentación de autoliquidaciones con domiciliación bancaria» (en adelante, **tabla de domiciliación**) y los textos consolidados del BOE.
Lo marcado `TODO(VERIFY)` no se ha leído literalmente en una fuente oficial.

## 1. Plazo general de presentación por modelo

| Modelo | Regla general (sin desplazamiento) | Norma |
|---|---|---|
| 303 trimestral | 1–20 de abril, julio y octubre; **4T: 1–30 de enero** | RIVA (RD 1624/1992) art. 71.4, párrafos 1.º y 2.º. [BOE-A-1992-28925](https://www.boe.es/buscar/act.php?id=BOE-A-1992-28925) |
| 303 mensual | «treinta primeros días naturales del mes siguiente»; **enero: hasta el último día de febrero**; **diciembre: 1–30 de enero** | RIVA art. 71.4, párrafo 3.º (sujetos del art. 62.6, es decir, SII) y párrafo 2.º. El período es mensual en los casos del art. 71.3 (más de 6.010.121,04 € de volumen, REDEME, grupo de entidades…) |
| 130 / 131 | 1T–3T: «entre el día 1 y el 20» de abril, julio y octubre; **4T: 1–30 de enero** | RIRPF (RD 439/2007) art. 111.1. [BOE-A-2007-6820](https://www.boe.es/buscar/act.php?id=BOE-A-2007-6820) |
| 111 / 115 / 123 trimestral | «primeros veinte días naturales» de abril, julio, octubre y **enero (4T: hasta el 20 de enero)** | RIRPF art. 108.1, párrafo 1.º; RIS (RD 634/2015) art. 68 ([BOE-A-2015-7771](https://www.boe.es/buscar/act.php?id=BOE-A-2015-7771)) |
| 111 / 115 / 123 mensual | 1–20 del mes siguiente (incluido diciembre: hasta el **20 de enero**) | RIRPF art. 108.1, párrafo 2.º: grandes empresas (art. 71.3, 1.º y 2.º RIVA) y AAPP con presupuesto de más de 6 M€ (este último caso solo para algunas rentas) |
| 349 | Mensual: 1–20 del mes siguiente; **julio: agosto y hasta el 20 de septiembre** (el calendario agrupa «Julio y agosto»). Trimestral si en el trimestre y en los 4 anteriores no se superan los 50.000 €: 1–20. Si se superan al cierre de un mes del trimestre, se declara por los meses transcurridos en los 20 días siguientes. **Último período del año: 30 primeros días de enero** | RIVA art. 81.2 y 81.3, texto consolidado leído el 26-10-2026. La periodicidad anual quedó suprimida (apartados 4 y 5 derogados por el RDL 3/2020). El motor ya es mensual/trimestral; la app no calcula el cambio de periodicidad a mitad de trimestre |
| 390 | 30 primeros días naturales de enero | Orden EHA/3111/2009 art. 8 ([BOE-A-2009-18472](https://www.boe.es/buscar/act.php?id=BOE-A-2009-18472)); RIVA art. 71.7. **Exonerados** (art. 1.3 de la misma orden, consolidado a 26-01-2026; confirmado el 26-10-2026): quienes presentan 303 trimestral sólo en territorio común y tributan en régimen simplificado y/o arrendamiento de inmuebles urbanos (aunque tengan otras actividades sin autoliquidación periódica) y quienes llevan los libros por la Sede (SII, art. 62.6 RIVA); la exoneración no vale si no hay que presentar el último periodo, y los exonerados rellenan el apartado específico del 303 del último periodo. Una SL o un autónomo en régimen general sin SII **no** está exonerado. Ver CALENDARIO-AEAT-03 y CALENDARIO-AEAT-04 en `docs/AUDITORIA.md` |
| 190 / 180 / 193 | 1–31 de enero (presentación electrónica) | RIRPF art. 108.2. Orden propia de cada modelo: `TODO(VERIFY)` (180: Orden de 20-11-2000, modificada por HAP/1732/2014; 193: Orden EHA/3377/2011) |
| 184 | 1–31 de enero | Orden HAP/2250/2015, en la redacción de la Orden HFP/1106/2017 ([BOE-A-2017-13279](https://www.boe.es/diario_boe/txt.php?id=BOE-A-2017-13279)) |
| 347 | «durante el mes de febrero» | Orden EHA/3012/2008 art. 10 ([BOE-A-2008-16973](https://www.boe.es/buscar/act.php?id=BOE-A-2008-16973)), en la redacción de la Orden HAC/1148/2018 |
| 200 | «25 días naturales siguientes a los 6 meses posteriores a la conclusión del período impositivo». Si el ejercicio coincide con el año natural: 1–25 de julio (ejercicio 2025: hasta el 27-07-2026 por el sábado, confirmado en el calendario 2026). En ejercicios partidos la AEAT cuenta los seis meses **de fecha a fecha** y luego los 25 días (cierre 30/06 → seis meses hasta el 30/12 → del 31/12 al 24/01) | LIS (Ley 27/2014) art. 124.1. [BOE-A-2014-12328](https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328); folleto de actividades económicas de la AEAT, 4.9.1 «Plazo de presentación» (leído el 26-10-2026). El motor usa fin de mes y da el 25/01: ver CALENDARIO-AEAT-01 en `docs/AUDITORIA.md` |
| 202 | «primeros 20 días naturales» de **abril, octubre y diciembre** (períodos 1P, 2P y 3P, no trimestres naturales), sobre el período impositivo en curso el día 1 de esos meses | LIS art. 40.1 |
| 100 (Renta) | **Se fija cada año por orden.** Renta 2025: del 8 de abril al 30 de junio de 2026 | Orden HAC/277/2026 art. 7.1 ([BOE-A-2026-7041](https://www.boe.es/buscar/act.php?id=BOE-A-2026-7041)) |

Diferencias que hay que tener en cuenta:

- **El 4T no cierra igual en todos los modelos.** 303, 130, 131 y 349 terminan el 30 de enero; 111, 115 y 123 terminan el **20 de enero**.
- **El 303 mensual vence el día 30**, no el 20. La excepción es enero, que vence el último día de febrero.
- **El 349 mensual vence el día 20**, salvo el de diciembre (30 de enero) y el de julio (20 de septiembre).
- El 202 no se rige por trimestres: «1P» corresponde a abril, «2P» a octubre y «3P» a diciembre.
- Modelos 200 y 202 con ejercicio no coincidente con el año natural: hay que calcularlos con la regla del art. 124.1 LIS, contando los seis meses de fecha a fecha (ver la fila del 200). Las fechas del fixture son solo para ejercicios que coinciden con el año natural. `TODO(VERIFY)`: qué año se consigna como «Ejercicio» en el 202 de ejercicios partidos (CALENDARIO-AEAT-08 en `docs/AUDITORIA.md`).
- Renta (100): el **inicio** de la campaña no sigue ninguna regla. Hay que tomarlo de la orden anual o del calendario. En 2026 empezó el 8 de abril; la atención telefónica, el 6 de mayo, y la presencial, el 1 de junio.

## 2. Vencimiento en día inhábil

- Si el último día del plazo es inhábil, el plazo se prorroga al primer día hábil siguiente (**Ley 39/2015 art. 30.5**, [BOE-A-2015-10565](https://www.boe.es/buscar/act.php?id=BOE-A-2015-10565)). Esta ley se aplica a los tributos de forma supletoria, por su **DA 1.ª.2.a** y por el **art. 7.2 de la LGT** (Ley 58/2003). La LGT no tiene una regla propia de cómputo; el «art. 48» que se mencionaba regula el domicilio fiscal, así que no es la norma aplicable.
- **Los sábados son inhábiles.** Así lo dicen la Ley 39/2015 art. 30.2 y el calendario AGE 2026 ([BOE-A-2025-23702](https://www.boe.es/buscar/doc.php?id=BOE-A-2025-23702)): «En todo el territorio nacional: los sábados, los domingos y los días declarados como fiestas de ámbito nacional no sustituibles, o sobre las que la totalidad de las CCAA no han ejercido la facultad de sustitución». Los datos de 2026 lo confirman: 31-01 (sábado) pasa a 02-02, 28-02 (sábado) a 02-03, 30-05 (sábado) a 01-06, 20-06 (sábado) a 22-06 y 25-07 (sábado) a 27-07.
- **Los festivos autonómicos y locales sí cuentan, también en la presentación telemática.** La AEAT lo dice literalmente en la página «Vencimientos en días inhábiles, sábados y festivos» del calendario 2026: «Si el vencimiento coincide con una festividad local o autonómica, el plazo finaliza el primer día hábil siguiente al señalado en este calendario, excepto para el modelo 369». Base legal: Ley 39/2015 art. 30.6, según el cual un día inhábil en el municipio o CA del interesado es inhábil «en todo caso». El calendario de la AEAT solo aplica el desplazamiento por sábados, domingos y festivos nacionales. **El motor debe aplicar después, por contribuyente, sus festivos de la CA y locales** (confirmado el 26-10-2026: el motor ya lo hace; sólo queda borrar el `TODO` de `business.rs`). Las fiestas locales no están en el fixture (las publican los boletines autonómicos o provinciales, art. 46 RD 2001/1983): `TODO(VERIFY)` fuente de datos.
- Festivos nacionales en el fixture: los marcados `*` o `**` para las 19 columnas del anexo del BOE. Los nacionales que caen en domingo no aparecen en el anexo (2026: 1-11 y 6-12; 2027: 15-08).
- **Notificaciones electrónicas** (no son plazos de presentación, pero se preguntó si agosto es inhábil): el plazo para acceder es de **diez días naturales** desde la puesta a disposición (Ley 39/2015 art. 43.2) y agosto no es inhábil a estos efectos, porque el art. 30.7 sólo rige el calendario de días inhábiles de los plazos administrativos. Lo único que suspende la puesta a disposición son los «días de cortesía»: hasta 30 días al año que elige el obligado (RD 1363/2010, DA 3.ª, [BOE-A-2010-17575](https://www.boe.es/buscar/act.php?id=BOE-A-2010-17575)). Confirmado el 26-10-2026; los textos de la app son correctos.

## 3. Domiciliación bancaria

- Norma: **Orden EHA/1658/2009 art. 3.2 y anexo II** ([BOE-A-2009-10326](https://www.boe.es/buscar/act.php?id=BOE-A-2009-10326)). Plazos generales:
  - 111, 115 y 123 (trimestrales y mensuales) y 202: del 1 al **15**.
  - 130, 131 y 303 trimestral: del 1 al **15**, y del 1 al **25** de enero para el 4T.
  - 303 mensual: del 1 al **25**; enero, hasta el **23 de febrero** (24 en año bisiesto).
  - 200: del 1 al **20 de julio**.
  - Renta: según la orden anual. En 2026, del 8 de abril al **25 de junio** (Orden HAC/277/2026 art. 12.3). Si solo se domicilia el segundo plazo, hasta el 30 de junio.
- Si el fin del plazo de presentación es inhábil, el plazo de domiciliación «se ampliará el mismo número de días» (art. 3.2.b). Además, entre el fin de la domiciliación y el fin del plazo de presentación tiene que haber «un mínimo de tres días hábiles o cinco naturales». A estos efectos son inhábiles los sábados, los domingos, los festivos nacionales y los festivos autonómicos o locales **del municipio del Departamento de Informática de la AEAT**: Madrid, `TODO(VERIFY)` (sin fuente oficial a 26-10-2026). El texto vigente del art. 3.2 (consolidado a 23/06/2026) confirma todo lo anterior; la regla de «vencimiento − 3 días hábiles» de la §5 es una inferencia nuestra coherente con ese texto, no un criterio publicado.
- **Recomendación:** tomar la fecha de domiciliación de la tabla oficial de cada año, porque la AEAT no aplica un algoritmo uniforme (ver Discrepancias).

## 4. Alcance y advertencias

- Solo territorio común. Los modelos forales (País Vasco y Navarra) tienen otros plazos y no están cubiertos.
- La página «Vencimientos en días inhábiles» advierte que el calendario 2026 se elaboró «en diciembre de 2025, con proyectos normativos en tramitación».
- Los .ics oficiales (Google Calendar) tienen las mismas fechas que el HTML, pero con erratas en el texto. Por ejemplo, el evento del 30-10-2026 dice «Septiembre 2025. Autoliquidación: 303» y el del 31-01-2026 dice «Cuarto trimestre 2024: …369». Hay que usar el HTML como fuente de la verdad.
- Calendario del contribuyente 2027: no publicado a 26-10-2026 (`TODO(VERIFY)`; se espera en diciembre). Las fiestas laborales de 2027 sí se publicaron: Resolución de 8 de octubre de 2026, BOE-A-2026-21254 (BOE de 13-10-2026). Nacionales: 1 y 6 de enero, 26 de marzo, 1 de mayo, 12 de octubre, 1 de noviembre, 6, 8 y 25 de diciembre; coinciden con `fixtures/calendar/festivos-nacionales.json` (el 15 de agosto cae en domingo y no figura).

## 5. Discrepancias

Método: se recalculó cada vencimiento con la regla general de la §1 y se movió al siguiente día hábil saltando sábados, domingos y festivos nacionales del BOE.

**Presentación (`end`): sin discrepancias.** Las 99 fechas oficiales coinciden con la regla.

**Domiciliación (`end_domiciliacion`): 5 fechas distintas (12 entradas) no coinciden.** La regla simple es: día general (+ los días que se amplíe la presentación) y, si cae en inhábil, el siguiente día hábil.

| Vencimientos | Día general | Regla simple | Oficial AEAT |
|---|---|---|---|
| 303 4T-2025, 303 12-2025, 130/131 4T-2025 (presentación hasta 30-01) | 25-01 (domingo) | 26-01 | **27-01** |
| 111/115/123 mensual 01-2026 (presentación hasta 20-02) | 15-02 (domingo) | 16-02 | **17-02** |
| 111/115/123 mensual 10-2026 (presentación hasta 20-11) | 15-11 (domingo) | 16-11 | **17-11** |
| 303 mensual 09-2026 (presentación hasta 30-10) | 25-10 (domingo) | 26-10 | **27-10** |
| 303 mensual 11-2026 (presentación hasta 30-12) | 25-12 (Navidad) | 28-12 | **24-12**: se *adelanta* |

En cambio, estos casos sí cumplen la regla simple: 303 de 03 y 06 (27-04 y 27-07), 111/115/123 de 02 (16-03) y de 07 (17-08).

Estas fechas encajan con otra regla: **cuando el día general de domiciliación es inhábil, la fecha oficial es la del vencimiento de presentación menos 3 días hábiles**. Con ella salen todas las fechas de la tabla y también 27-04, 27-07 y 17-08.

**Excepción:** 111/115/123 de 02-2026 (presentación hasta el viernes 20-03; el día 15 fue domingo). La AEAT da el **16-03**, pero con la regla de los 3 días hábiles saldría el 17-03. `TODO(VERIFY)`: falta el criterio exacto de la AEAT, ya que la orden no lo detalla (revisado el 26-10-2026 en el texto consolidado del art. 3.2: sigue sin detallarlo).

**Vencimientos que coinciden con festivos autonómicos** (por la página de la AEAT, se desplazan en esa CA):

- 02-03-2026 (303 de enero y 347): festivo en Illes Balears, que pasa al 03-03.
- 20-03-2026 (111/115/123 mensuales y 349 de febrero): festivo en Melilla (Eid Fitr), que pasa al 23-03.
- 27-05-2026 (domiciliación del 303 de abril): festivo en Ceuta y en Melilla.

En la página de cada día, la AEAT no muestra estas variantes.

**Pendiente (`TODO(VERIFY)`):**

- Orden propia de los modelos 180, 190 y 193.
- Municipio del Departamento de Informática de la AEAT (y el criterio exacto de la domiciliación de la §5).
- Fuente de datos de festivos locales.
- Calendario del contribuyente 2027 (no publicado a 26-10-2026).
- Año que se consigna como «Ejercicio» en el 202 de ejercicios partidos (CALENDARIO-AEAT-08).

Resueltas el 26-10-2026 por la auditoría (detalle en `PROGRESS.md`, «Dudas»): exoneraciones del 390, festivos autonómicos y locales, cómputo del plazo del 200 en ejercicios partidos, periodicidad del 349, plazo del 200 de 2025 y notificaciones electrónicas.

## 6. Otras obligaciones (según el perfil)

Añadidas el 26-10-2026 (lote «calendario y alertas»; sin acceso a la Sede ni al BOE desde el contenedor: lo no leído literalmente va con `TODO(VERIFY)` en `rules.rs`). Sólo aparecen si el perfil las marca.

| Obligación | Plazo | Norma | Traslado |
|---|---|---|---|
| 720 (cuentas, valores e inmuebles en el extranjero > 50.000 €) | 1 de enero – 31 de marzo del año siguiente | Orden HAP/72/2013 art. 7; RGAT arts. 42 bis, 42 ter y 54 bis | Hábil siguiente |
| 721 (monedas virtuales en el extranjero > 50.000 €), desde el ejercicio 2023 | 1 de enero – 31 de marzo | Orden HFP/886/2023 **art. 4** (leído el 27-10-2026); RGAT art. 42 quater | Hábil siguiente |
| 232 (operaciones vinculadas y paraísos fiscales) | «Mes siguiente a los diez meses posteriores a la conclusión del período impositivo»: noviembre con ejercicio natural; con cierre el 30/06, mayo | Orden HFP/816/2017 art. 4; RIS art. 13.4. `TODO(VERIFY)`: meses enteros en ejercicios partidos | Hábil siguiente |
| Renta, 2.º plazo (40 %) | Hasta el 5 de noviembre (Renta 2025: 5-11-2026); estimada sin orden anual | RIRPF art. 62.2; Orden anual (HAC/277/2026) | Hábil siguiente (2022: 7-11) |
| 840 (alta en el IAE al perder la exención) | Diciembre del año anterior al primero en que se paga | RD 243/1995 **art. 5.3, párrafo segundo** («durante el mes de diciembre inmediato anterior al año en el que el sujeto pasivo resulte obligado a contribuir», leído el 27-10-2026); TRLRHL art. 82.1.c | Hábil siguiente |
| 848 (cifra de negocios, entidades del art. 35.4 LGT que pagan IAE y no la declararon en el 184) | 1 de enero – 14 de febrero (cifra del penúltimo año) | Orden HAC/85/2003 apartado tercero («entre el día 1 de enero y el día 14 de febrero, ambos incluidos»); exonerados en el apartado primero: exentos del IAE y quienes ya la hicieron constar en el 200 o en el 184 (leído el 27-10-2026) | Hábil siguiente |
| 369 OSS (Unión y exterior) | Mes siguiente al trimestre (1T: abril; 4T: enero) | LIVA arts. 163 unvicies y ss.; Directiva 2006/112/CE arts. 364 y 369 septies; Orden HAC/610/2021 | **No se traslada** (calendario AEAT 2026: «excepto para el modelo 369») |
| 369 IOSS (importación) | Mes siguiente al mes | Las mismas | **No se traslada** |
| 036/037 de noviembre: REDEME (alta o baja) y **renuncia** al SII voluntario | 1–30 de noviembre, con efectos el año siguiente | RIVA arts. 30.4, 62.6 (párrafo segundo) y **68 bis** (leído el 27-10-2026): la **opción** por el SII «podrá ejercitarse a lo largo de todo el ejercicio», con efectos en el primer periodo de liquidación siguiente; sólo la renuncia va en noviembre | Hábil siguiente |
| 036/037 de diciembre: renuncia a módulos o a la ED simplificada, al régimen simplificado del IVA, criterio de caja | 1–31 de diciembre, con efectos el año siguiente | RIRPF arts. 28.1 y 33.1.a; RIVA arts. 33.2 y 61 septies | Hábil siguiente |
| Prorrata especial | En el 303 del último periodo del año (30 de enero) | RIVA art. 28.1.1.º **letra a)** (redacción del RD 828/2013, leída el 27-10-2026); con inicio de actividad, letra b); mínimo tres años y revocación también en la última del año | Hábil siguiente |
| Cuota de autónomos (RETA) | Dentro del mismo mes; cargo el último día hábil | RD 1415/2004 art. 56.1.b 1.º (leído el 27-10-2026); Guía práctica de trabajo autónomo de la TGSS: «La cuota se cargará en tu cuenta bancaria el último día hábil de cada mes» | **Se adelanta** al último hábil |
| Cambio de base del RETA | Seis plazos: ene-feb, mar-abr, may-jun, jul-ago, sep-oct y nov-dic; vale desde el día 1 del mes siguiente | Art. 308.1.a TRLGSS; art. 45.1 RD 2064/1995 (redacción del RDL 13/2022; el art. 43 bis está derogado desde 2023) | No se mueve (solicitud telemática). En una sociedad, con `cotiza_reta` (administrador autónomo societario) salen también la cuota y estos plazos |
| Seguros sociales (RLC/RNT) | Mes siguiente al de devengo | RD 1415/2004 art. 56.1 | **Se adelanta** al último hábil (la domiciliación de la TGSS se carga el último hábil, como la cuota de autónomos) |

Las opciones censales sólo salen si el perfil pide «Avísame de los plazos para cambiar de régimen». La cuota de autónomos avisa una sola vez (dos días antes): se carga sola.

## 7. Avisos de umbral (`crates/app/src/umbrales.rs`)

Con los datos de TaxPain y sólo cuando el perfil o los ajustes de Modelos no lo reflejan ya (se ven en Calendario, «Avisos según tus datos»):

- **347**: algún tercero pasa de 3.005,06 € (IVA incluido) en el año en curso o, en enero y febrero, en el anterior. No con SII (RGAT art. 32.1.e).
- **349**: hay entregas o servicios a la UE y el perfil no tiene el 349, o más de 50.000 € en el trimestre o en alguno de los cuatro anteriores con el 349 trimestral (RIVA art. 81.2).
- **Gran empresa y SII**: más de 6.010.121,04 € de bases emitidas el año anterior (aproximación del volumen de operaciones del art. 121 LIVA) sin «gran empresa» o sin IVA mensual con SII; aviso previo si el año en curso ya lo supera.
- **720**: cuentas con IBAN de otro país con saldo contable conjunto de más de 50.000 € a 31/12 (no mira el saldo medio del 4T ni valores o inmuebles).
- **232**: facturas emitidas o recibidas con socios del 25 % o más o con administradores por más de 250.000 € en el año (no ve préstamos, alquileres sin factura ni las operaciones específicas de 100.000 €).
- **Tipo del IS** (microempresa < 1 M€; reducida dimensión < 10 M€ de cifra de negocios del último ejercicio cerrado) frente al régimen elegido en Modelos; **202** del art. 40.3 LIS (6 M€) frente a la casilla de Modelos; **IAE** (1 M€ → se paga dos años después; 840 en diciembre del año anterior). La cifra de negocios es la de `payroll::director_turnover` (sólo con el ejercicio completo en la contabilidad).

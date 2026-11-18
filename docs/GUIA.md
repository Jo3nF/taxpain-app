# Guía de uso de TaxPain

TaxPain es una app para Mac que lleva los impuestos, las facturas y la contabilidad de un
autónomo o de una sociedad española sin que tus datos ni tu certificado digital salgan de tu
ordenador. Esta guía recorre cada pantalla en el orden de la barra lateral.

> **TaxPain está en desarrollo (alfa) y no es asesoramiento fiscal.** Calcula, prepara y
> explica; quien presenta es
> responsable de lo que presenta. Revisa siempre los borradores y, ante la duda, consulta con un
> asesor. En las compilaciones de desarrollo nada va a producción: los envíos van al entorno de
> pruebas de la AEAT. **VERI\*FACTU está en pruebas** (sólo envía al entorno de pruebas de la
> AEAT y viene desactivado); pronto estará listo.

**Lo más importante**

1. **Revisar a tu gestoría**: «Comprobar lo presentado en la AEAT» baja con tu certificado lo que
   se ha presentado ([Modelos](#modelos)), y la [Revisión de la carpeta](#importar) y [«Qué
   revisar»](#evolución) lo cruzan con tus facturas, extractos y escrituras: qué está mal, de
   quién es el error, cuánto se podría recuperar y hasta cuándo rectificar.
2. **Claude por MCP**: pregunta a Claude Desktop o Claude Code por tu empresa
   ([Claude (MCP)](#claude-mcp)).
3. **Tu negocio al día**: el [Panel](#panel) con la tesorería y su previsión, y la
   [Evolución](#evolución) año a año.
4. **Criptomonedas e inversiones**: [Criptomonedas](#criptomonedas) e [Inversiones en
   valores](#inversiones-en-valores).
5. **Modelos**: calculados casilla a casilla, con el fichero oficial, la prevalidación y la
   presentación directa en el entorno de pruebas de la AEAT ([Modelos](#modelos)).

**Índice**

- [Antes de empezar](#antes-de-empezar)
- [Primeros pasos](#primeros-pasos)
- [General](#general): Panel · Evolución · Ahorrar impuestos · Calendario · Importar
- [Facturación](#facturación): Facturas · Presupuestos · Recibidas · Clientes y proveedores
- [Contabilidad](#contabilidad): Contabilidad · Modelos · Criptomonedas · Inversiones en valores ·
  Notificaciones
- [Ayuda](#ayuda): Asistente · Ajustes · Claude (MCP)
- [Varias empresas, copias y avisos](#varias-empresas-copias-y-avisos)
- [Qué sale de tu Mac](#qué-sale-de-tu-mac)
- [Preguntas frecuentes](#preguntas-frecuentes)

---

## Antes de empezar

- **Dónde están tus datos.** Cada empresa tiene su propia base de datos cifrada (SQLCipher) en tu
  Mac, con la clave en el Llavero. Los documentos y el certificado también se guardan cifrados.
  No hay servidor de TaxPain.
- **Qué pide confirmación.** Todo lo que tiene efectos (emitir o anular una factura, presentar,
  abrir una notificación de la AEAT, cerrar un ejercicio, borrar) se confirma en la propia
  pantalla. Además piden **Touch ID** (o la contraseña de tu Mac):
  - todo lo que usa tu certificado;
  - emitir una factura (también desde un presupuesto o una factura periódica), anularla y
    subsanar su registro VERI\*FACTU;
  - cerrar y reabrir un ejercicio, y contabilizar o deshacer el pago de dividendos;
  - borrar una empresa y cambiar la contraseña de las copias;
  - borrar lo que no se puede recuperar: asientos, nóminas, viajes, contratos de leasing,
    centros de coste, presupuestos en borrador, contactos, facturas periódicas, documentos de la
    empresa, modelos presentados guardados, socios, partícipes del 184, plataformas,
    operaciones, precios y el punto de partida de cripto, y cuentas, operaciones y precios de
    valores;
  - guardar o quitar la clave de API del asistente (si guardas la clave al cambiar de
    proveedor, se guardan juntos: si cancelas Touch ID, no cambia ninguno), activar «Full IA»
    (los documentos van al asistente sin anonimizar) y vigilar una carpeta nueva.

  El aviso de Touch ID dice qué se va a hacer («Borrar el presupuesto P-2026-0003»). En algunos
  casos en que no se puede borrar (un presupuesto ya enviado, una plataforma o una cuenta con
  operaciones, un leasing con cuotas contabilizadas, un pago de dividendos de un ejercicio
  cerrado), te lo dice sin pedir Touch ID; en otros, como un asiento de un ejercicio cerrado,
  lo dice después de Touch ID, y no se borra nada. No lo
  piden, porque tienen vuelta atrás o no tienen valor fiscal: descartar una factura recibida
  (se recupera), tirar el borrador de la apertura, borrar plantillas de asiento, reglas del
  banco o conversaciones con el asistente, dejar de vigilar una carpeta y desactivar «Full IA».
- **Auditoría.** Los actos relevantes quedan en un registro encadenado con huellas SHA-256 que
  detecta si alguien cambia o borra entradas, o si se vuelve a una copia antigua. Si al abrir una
  empresa sus datos son más antiguos que la última vez, o falta la marca que lo comprueba, sale un
  aviso en rojo: revisa las facturas y presentaciones recientes y pulsa **«Entendido»** (queda
  anotado).
- **Modo demo.** Para ver la app con datos inventados, sin certificado ni datos tuyos: con el
  código descargado (ver [Compilar desde el código](../README.md#compilar-desde-el-código)),
  ejecuta `npm run dev` en `apps/desktop` y abre `http://127.0.0.1:5173/?demo=1` en el navegador.
- **Atajos.** `⌘K` abre la paleta para buscar facturas, clientes, cuentas, asientos y documentos o
  ir a cualquier pantalla. En el libro diario: `⌥N` nuevo asiento, `⌘↵` registrarlo, `Esc`
  cerrarlo. En el asistente y en las conversaciones con la IA, `⌘+Intro` pregunta (en «Añadir un
  dato», añade) y `Esc` cierra lo último que se ha abierto (la confirmación de borrar, «Añadir un
  dato») y, si no hay nada, el panel.

## Primeros pasos

1. **Protege tus datos.** La primera vez, TaxPain pide la contraseña de las copias de seguridad
   (al menos 10 caracteres) y crea un **código de recuperación**: guárdalo, sin él nadie puede
   abrir tus copias si pierdes el Mac.
2. **Importa tu certificado** en Ajustes → Certificado → «Importar certificado (.p12 / .pfx)». Se
   comprueba (titular, NIF, caducidad, emisor) y se guarda sellado en el Mac.
3. **Configura tus facturas** (Facturas → «Configura tus facturas» o Ajustes → Facturación):
   datos del emisor, series, IVA y retención por defecto, pie de factura y en qué carpeta se
   guarda una copia de cada factura.
4. **Rellena el perfil fiscal** (Calendario → «Editar perfil»). Empieza con la plantilla
   «Autónomo (estimación directa)» o «Sociedad limitada» y marca lo que te toca: así salen tus
   plazos, los avisos y las estimaciones. Si todavía no tienes perfil y TaxPain ya tiene
   declaraciones de la empresa (las de «Comprobar lo presentado» o los modelos de la gestoría),
   **«Rellenar con lo presentado»** lo deduce y dice de dónde sale cada cosa: revísalo y guárdalo.
5. **Trae lo que ya tienes.** Importar → «Elegir carpeta…» con la carpeta de la empresa
   (facturas, libros, extractos, modelos, escrituras…). Si empiezas a mitad de año o vienes de
   una gestoría: Contabilidad → «Saldos iniciales».
6. **Comprueba lo presentado** (Modelos → «Comprobar lo presentado en la AEAT»): baja lo que tú o
   tu gestoría presentasteis y alimenta Evolución y el panel.
7. **Opcional:** copias automáticas, «Abrir TaxPain al iniciar sesión» para los avisos, las
   [carpetas vigiladas](#recibidas), el asistente de IA, la conexión con el banco y con tu bróker
   y el acceso de [Claude (MCP)](#claude-mcp).

---

## General

### Panel

«Tu negocio en {año}»: ingresos, gastos, resultado, tesorería y lo pendiente de cobro y de pago,
con la gráfica de ingresos y gastos por mes.

- **Previsión de tesorería** a 13 semanas o 12 meses: saldo de hoy, mínimo previsto y «Dinero que
  no es tuyo» (IVA y retenciones que tendrás que ingresar). Cuenta cobros y pagos pendientes,
  modelos, nóminas, leasing, las cuotas de los préstamos aún sin registrar (cobros de los
  concedidos, pagos de los recibidos, netos de la retención) y asientos periódicos; «Cómo se
  calcula» lo explica.
- **Impuestos del trimestre** (estimación) y los **próximos plazos**.
- **Pendiente**, con un enlace a cada sitio: notificaciones de la AEAT, recibidas por revisar,
  banco sin conciliar, facturas vencidas, presupuestos aceptados por facturar, facturas
  periódicas por emitir, registros VERI\*FACTU sin enviar, copias de seguridad y certificado que
  caduca.
- Con dos o más empresas: **«Todas las empresas»** (lo vencido sin presentar y lo de los próximos
  45 días de cada una) y las **cuentas del grupo** consolidadas.
- En **diciembre y enero**, **«Cambio de año: qué revisar para {año}»**: lo que cambia cada año y
  si TaxPain ya lo tiene. Cada punto dice «Cargado», «Se usa el del año anterior» (por ejemplo,
  el interés de demora sin ley de presupuestos, la tabla del RETA sin orden de cotización o un
  diseño de registro que la AEAT aún no ha publicado), «Pendiente de publicar» (festivos locales
  o de tu comunidad: TaxPain no se los inventa) o «Revísalo tú» (tu perfil fiscal con las cifras
  del año que acaba). Lo ya cargado va plegado al final.

Si TaxPain todavía no tiene la contabilidad del año, el panel lo dice y enseña las cifras de lo
presentado a la AEAT.

### Evolución

- **«Evolución año a año»**: las cifras clave de cada ejercicio (ventas, gastos, resultado,
  impuestos…), con el modelo y la casilla de donde salen, la variación con el año anterior y la
  tendencia.
- **«Qué revisar»**: cruces entre lo presentado (390 frente a los 303, resúmenes anuales frente a
  los trimestres, 349 frente al 303, 200 frente a los 202, Impuesto sobre Sociedades…), con
  errores, avisos, quién tiene que corregirlo, **lo que se podría recuperar** y hasta cuándo se
  puede rectificar (cuatro años desde el fin del plazo; en el Impuesto sobre Sociedades, con el
  calendario de festivos, y «aprox.» si TaxPain no tiene los de ese año). Incluye qué pedir y las
  tablas de detalle.
- **Año en curso:** el 200 de un año no se presenta hasta julio del siguiente. Mientras falta, la
  cifra de negocios sale como avance con las bases de las ventas de los 303 ya presentados,
  marcada **«a medias»** y sin compararla con el año entero anterior.
- «Preguntar a la IA», en cada aviso, abre una conversación sobre ese hallazgo (ver
  [Asistente](#asistente)).
- «Guardar el informe en PDF…» o en Excel.

Necesita lo que baja «Comprobar lo presentado en la AEAT» (Modelos).

### Ahorrar impuestos

Lo que puedes aprovechar según la situación de tu empresa, con **cuánto te ahorrarías y los pasos
para conseguirlo**: reservas de capitalización y de nivelación, financiación de producciones de
cine y series, amortización acelerada y compras de poco importe, planes de pensiones, empresas
emergentes, donativos, IVA, compensar pérdidas de criptomonedas antes del 31 de diciembre,
recuperar lo pagado de más… (unas treinta, revisadas con la ley del año).

- Arriba, el ahorro estimado del año y la situación con la que se calcula (régimen, cifra de
  negocios, base o rendimiento, tipo).
- Cada tarjeta dice si **te conviene**, **puede interesarte**, **ya lo aprovechas** o **no te
  aplica**, el motivo con tus cifras, el ahorro y la fecha límite. «Qué es y cómo conseguirlo»
  tiene los requisitos, los pasos (con casillas para ir marcándolos), los riesgos, el cálculo,
  la norma (enlace al BOE o a la Sede) y un botón a la pantalla de TaxPain que ayuda.
- «Lo estoy haciendo», «Hecho» y «No me interesa» ordenan las tarjetas en «En marcha», «Hechas o
  aprovechadas» y «Descartadas».
- «Preguntar a la IA», en cada tarjeta y en el resumen del año, abre una conversación sobre esa
  recomendación o sobre tu plan (ver [Asistente](#asistente)).

Son orientaciones: antes de firmar, invertir o presentar, confírmalo con tu asesor.

### Calendario

Tus plazos del año, por meses y con cuenta atrás: 303, 130/131, 111, 115, 123, 349, 390, 190,
180, 193, 347, 184, 200, 202, Renta (y su segundo plazo), 720, 721, 232, depósito y legalización
de libros, cuotas de autónomos y seguros sociales… Cada plazo se traslada si cae en festivo
**nacional, autonómico o local** y dice hasta cuándo se puede domiciliar.

- **«Avisos según tus datos»**: cuando superas un umbral (347, 349 mensual, gran empresa y SII,
  720, 721, 232, tipo del Impuesto sobre Sociedades, 202, IAE), con «Qué hacer».
- «Exportar a Calendario (.ics)» para tenerlos en el calendario del Mac o del móvil.
- «Editar perfil»: el perfil fiscal (ver [Primeros pasos](#primeros-pasos)).

**El año siguiente.** Con las flechas del año puedes ver ya los plazos de 2027: salen de la norma
de cada modelo, trasladados por los festivos nacionales y autonómicos de 2027 (BOE de octubre de
2026). Mientras la AEAT no publique su calendario del contribuyente de 2027 (en diciembre), la
pantalla avisa de que el último día para domiciliar y la campaña de Renta son estimados. Si no
tienes festivos locales del año en el perfil, también lo dice: añádelos cuando salgan en el
boletín de tu comunidad o provincia.

Los borradores de 2027 se hacen igual. Si la AEAT todavía no ha publicado el diseño de registro
de 2027 de un modelo, el fichero sale con el último vigente y el borrador lo avisa («Diseño de
registro del 303 de 2027 no publicado todavía: se usa el de DR303e26v101…»); las casillas del
borrador sí valen.

#### Censo (modelo 036)

Debajo del calendario, **Censo** compara lo que consta en el censo de la AEAT con tu perfil
fiscal y con lo que dice la contabilidad de este año y el anterior, y te ayuda a preparar el 036.
TaxPain no lo presenta: el 036 se rellena en la Sede.

- **Lo que consta**: sale del último *certificado de situación censal* guardado en Importar
  (obligaciones periódicas, epígrafes del IAE y si está el ROI). Si no lo tienes o es una foto,
  pulsa «Teclear lo que consta» y marca lo que ves en «Mis datos censales» de la Sede. Ahí mismo
  puedes escribir los epígrafes de lo que la empresa hace de verdad, para cotejarlos.
- **El cuadro**: cada obligación (IVA, 111, 115, 123, 349, 130 o 200) en el censo, en el perfil,
  en la contabilidad (nóminas y facturas de profesionales con retención, alquiler con retención,
  dividendos con retención, facturas intracomunitarias) y en lo presentado.
- **Diferencias**, de más a menos urgente: «retienes a profesionales y no consta el 111»,
  «facturas intracomunitarias sin alta en el ROI», «consta el 115 y no pagas alquiler», epígrafes
  que no corresponden o un alquiler (cuenta 752) sin epígrafe 861, domicilio fiscal distinto del
  de las facturas, criterio de caja que no coincide, el 202 por la base elegido sin comunicar…
  Cada una dice qué hacer y, si va por el 036, «Ver qué marcar en el 036».
- **Quiero comunicar…**: eliges el cambio (alta o baja en el ROI, alta o baja del 111, 115 o 123,
  nuevo epígrafe o baja, domicilio fiscal o social, administrador, 202 por la base, criterio de
  caja, prorrata especial, REDEME) y la fecha del hecho. TaxPain dice la causa de presentación,
  los apartados del 036 que tocar, el plazo (un mes desde el hecho, de fecha a fecha y trasladado
  si cae en festivo; antes de la primera operación en el ROI; febrero para el 202, noviembre para
  el REDEME, diciembre para el criterio de caja; la prorrata especial va en el último 303), la
  documentación y la norma. «Abrir el 036 en la Sede» y «Añadir el plazo al calendario» (pide
  confirmación). Lo apuntado sale en el calendario como «Modelo 036» hasta que pulsas «Ya lo he
  presentado».

Los apartados se nombran por su título: los números de casilla todavía no se han cotejado con el
diseño del 036 (Orden HAC/1526/2024), así que busca el apartado por su nombre en el formulario.

#### Cerrar el trimestre

Arriba del calendario, **«Cerrar el trimestre»** repasa lo que conviene tener hecho antes de
presentar los modelos de un trimestre. Abre el que toca cerrar (en enero, abril, julio y
octubre, el anterior) y cada botón del trimestre dice cómo está: «listo para presentar»,
«presentado» o cuántas cosas faltan. El Panel lo recuerda en «Pendiente».

- **Antes de los modelos**: recibidas por revisar, facturas emitidas vencidas sin cobrar, banco
  sin conciliar (o sin extracto importado), nóminas y cuotas del RETA de los meses terminados,
  facturas periódicas por emitir, cuotas de los préstamos sin registrar y registros VERI*FACTU
  sin enviar. Las amortizaciones se hacen una vez al año: sólo cuentan en el trimestre que cierra
  el ejercicio. Cada paso lleva a donde se arregla. Los que TaxPain no puede comprobar del todo
  (p. ej. las nóminas que lleva una gestoría laboral) se marcan a mano como «hecho» o «no aplica».
- **Modelos del trimestre**: los que te tocan según el perfil fiscal (303, 130, 202, 111, 115,
  123, 349), con lo que sale, el plazo y la **comprobación con el diario**: el IVA devengado
  [27] con la 477 y el deducible [45] con la 472, las retenciones con la 4751 y las entregas
  intracomunitarias del 349 con la [59] del 303 (se admite 1 € de diferencia por redondeos).
  Abajo, lo que hay que ingresar y hasta cuándo se puede domiciliar.
- **Después**: la liquidación del IVA (Contabilidad → IVA y libros).

«Listo para presentar» quiere decir que no falta nada de lo de arriba y que todo cuadra, no que
los modelos estén bien: revísalos en Modelos. Nada se presenta ni se contabiliza desde aquí.

#### Otras obligaciones (no fiscales)

Debajo del calendario, una lista de control de lo que no es de la AEAT y que, sin gestoría, nadie
te recuerda: depósito de cuentas, legalización de libros, titularidad real, protección de datos
(registro de actividades, información, encargados), aviso legal y cookies de la web (LSSI),
prevención de riesgos, registro de jornada e igualdad (si tienes trabajadores), seguros, licencia
de actividad y cambios en el Registro Mercantil. Cada una dice qué es, a quién suele aplicar, el
plazo y la norma, con el enlace oficial cuando lo hay. Marca tú si te **aplica**, si **no
aplica** o si ya está **hecha**, y apunta una fecha y una nota si quieres. TaxPain no te asesora:
ante la duda, consúltalo.

### Importar

Cuatro pestañas para traer lo que ya existe y revisarlo.

**Carpeta de la empresa.** «Elegir carpeta…» lee la carpeta y sus subcarpetas **sin cambiar
nada** y agrupa lo que encuentra: documentos de la empresa, libros de facturas, contabilidad
(diarios de a3, Excel o CSV), modelos presentados (fichero de la AEAT o PDF), extractos del
banco, criptomonedas, inversiones en valores (informes de XTB y Flex de Interactive Brokers),
facturas sueltas (PDF o Facturae) y lo que no reconoce. Eliges qué se importa («Importar N
ficheros»); no se duplica lo que ya estaba y los asientos entran en lotes que se pueden deshacer.

- **«Buscar»**: con miles de ficheros, filtra por nombre, carpeta y lo leído de cada uno (banco,
  proveedor…), sin tener en cuenta tildes ni mayúsculas. «Marcar los N que se ven» y «Desmarcarlos»
  marcan o desmarcan de golpe lo filtrado.
- Se leen hasta 50.000 ficheros. Si la carpeta tiene más (o demasiados niveles de subcarpetas),
  TaxPain lo dice y la revisión avisa de que no se ha leído entera.
- **Avisos de las carpetas vigiladas.** Los documentos nuevos de las [carpetas
  vigiladas](#recibidas) que no son facturas recibidas (extractos, modelos, libros…) no entran
  solos: salen aquí como «Hay N documentos nuevos en las carpetas vigiladas», agrupados por
  carpeta. **«Revisar esta carpeta»** la lee para que elijas qué entra y **«Quitar aviso»** lo
  descarta. Aparte, en rojo, los ficheros que no se han podido abrir (dañados, protegidos o de más
  de 25 MB), con el motivo: ábrelos tú para ver qué son.

- Los libros van antes que las facturas sueltas: el PDF de una factura que está en el libro se
  le adjunta, también si lo ha leído la IA, en vez de quedar pendiente en Recibidas.
- Un diario (leído por las reglas o con IA) de un ejercicio que ya tiene sus asientos en TaxPain
  (de otro lote o cargado con «Un diario entero») llega sin marcar y dice por qué.
- Las líneas del libro de emitidas que son la **autorrepercusión de compras** (inversión del
  sujeto pasivo o adquisiciones intracomunitarias: el total sin la cuota, o la misma factura que
  en el libro de recibidas) no entran como ventas. Si lo son todas, o si la lectura con IA dice
  que no es un libro de ventas, el libro llega sin marcar.

- Lo que las reglas no entienden se puede completar con el asistente (texto anonimizado) o, si
  activas **«Full IA»** (con su aviso), enviando **el fichero completo, sin anonimizar**, al
  proveedor de IA que elijas. Cada lectura se guarda y no se vuelve a pagar.
- Con Claude Code, el comando `/leer-carpeta <ruta>` lee la carpeta con tu suscripción y deja
  `taxpain-lecturas.json`, que TaxPain usa al volver a leer (etiqueta «IA»). Las lecturas de
  copias del mismo fichero en otras carpetas se juntan en una, sin perder datos.
- Los **justificantes de presentación de la AEAT** («Información de la presentación de la
  declaración») se reconocen solos, de cualquier modelo: ejercicio, periodo, fecha de
  presentación, si es complementaria o sustitutiva y sus casillas, si cuadran entre sí. Sin las
  casillas cuentan igual como presentados en la revisión, pero no entran en sus cruces. Sólo se
  guardan para comparar los que TaxPain compara, y los de otra empresa (una participada) no
  cuentan como de ésta.
- Los **documentos de la Sede de la AEAT** (certificados, requerimientos, notificaciones,
  recibos de presentación, resoluciones, liquidaciones, cartas de pago) se reconocen por su
  código seguro de verificación y su título, cada uno con su tipo, y nunca son facturas. Un
  informe (como la revisión que exporta TaxPain), un correo o un escrito a la Administración no
  son documentos oficiales: de ellos no salen escrituras, depósitos de cuentas ni
  legalizaciones. Los de la agencia tributaria de una comunidad, una hacienda foral o un
  ayuntamiento no son de la AEAT. Sin número y sin importe, un PDF no es una factura, salvo que
  diga que lo es y se sepa quién la emite (queda para leerla con IA). La complementaria de una
  informativa (347, 349, 190…) no deja fuera la original: sólo trae lo que faltaba.
- **Descargar de la AEAT por CSV**: pega hasta 50 códigos seguros de verificación y TaxPain baja
  los PDF a la subcarpeta «Documentos AEAT» (con tu certificado y Touch ID por el servicio
  oficial, o por la página pública de la Sede). Sólo descarga: no abre notificaciones ni cambia
  nada en la AEAT.

**Documentos y datos.** Los datos de la empresa que salen de sus documentos (razón social, NIF,
domicilio, capital, CNAE, IAE, notario…), con un «Aplicar» por dato, y los documentos guardados,
cifrados en el Mac.

**Comparar con la gestoría.** Los modelos que presentó la gestoría (del fichero, leídos del PDF o
tecleados) frente a lo que calcula TaxPain, casilla a casilla, con la diferencia: 303 (también de
un mes), 390, 111, 115, 123, 130, 202, 349, 200 y las informativas 347, 190, 180 y 193. Cada
casilla dice qué es y, si no cuadra, de dónde suele venir la diferencia («TaxPain calcula 21,00 €
más: IVA soportado deducible: compara las facturas recibidas…»). Es orientativo: puede ser un
error de la gestoría o que a TaxPain le falten datos de ese periodo.

- De las informativas con su **fichero** (el `.txt` de la AEAT), la comparación va además
  perceptor a perceptor (190, 180, 193) o tercero a tercero (347), por NIF y clave: quién falta en
  un lado, quién tiene otro importe y quién otra retención. Del PDF sólo salen los totales.
- Al importar la carpeta, los ficheros del 347, el 190, el 180 y el 193 se guardan para comparar
  (salvo las complementarias, que sólo traen lo que faltaba).
- La revisión de la carpeta y Evolución → Qué revisar cuentan cada modelo que no coincide como un
  hallazgo («El 303 de 3T 2026 presentado no coincide con lo que calcula TaxPain»), con las
  casillas y su explicación, y la tabla «Lo presentado frente a TaxPain». Sólo si TaxPain tiene
  datos de ese periodo.
- No se comparan el 216, el 296, el 720 ni el 721: TaxPain no los calcula.

**Urgente: puede haber deuda.** Lo que puede suponer dinero pendiente con Hacienda sale el
primero, en un bloque rojo arriba de la revisión (y en el Panel, con la última revisión guardada, y
en Evolución → Qué revisar): un modelo que sale a ingresar sin su cargo en el banco (o con un
cargo menor), uno pagado tarde, uno presentado fuera de plazo con importe, autoliquidaciones que
faltan, retenciones practicadas y no ingresadas, IVA compensado de más, un 200 que descuenta más
pago fraccionado del ingresado y un certificado de estar al corriente negativo. Cada uno dice lo
que hay en juego, el recargo de hoy y cuándo sube: sin presentar, el del art. 27 LGT (1 % más
otro 1 % por mes completo; pasados doce meses, 15 % e intereses), que sube cada mes; presentado y
sin pagar, el del periodo ejecutivo (5, 10 o 20 %, art. 28 LGT). TaxPain no sabe si la deuda ya
está en periodo ejecutivo ni si Hacienda la ha notificado: compruébalo en la Sede. Se ordenan por
importe y luego por antigüedad, y el informe (PDF, Excel y Markdown) los pone en una sección
propia antes del resumen.

**Revisión.** «Revisar la carpeta» busca facturas repetidas o mal hechas, saltos de numeración,
facturas del libro sin PDF (y al revés), modelos que faltan, el 303 frente a los libros registro del
trimestre (y a las cuentas 472 y 477 del diario, si lo has importado; sin libro, frente a las
facturas), el 111 frente a las retenciones del libro de recibidas (sin libro, frente a las
facturas) y, con el diario importado, lo que se abona a la 4751 en cada trimestre frente al 111
(retenciones practicadas y no ingresadas: lo que un 111 no recoge puede ir en uno de los dos
siguientes, si se pagó más tarde, y si aún no se puede ver es una nota; si faltan 111, el año
frente al 190; las de alquileres, las del capital y las de no residentes, que van al 115, al 123
y al 216, se cuentan aparte), el Impuesto sobre Sociedades, los bancos, plazos y Registro
Mercantil, lo de la AEAT y socios y administradores. En Sociedades mira si el acta que aprueba las
cuentas, otra decisión posterior o el 200 del año siguiente dotan la reserva de nivelación, avisa
de los dividendos cobrados en el banco (para el próximo 200 o, si ya tenía que estar presentado,
para comprobarlo) y de si el administrador cobra como un empleado (clave A). También compara las
participaciones del grupo del balance con el cuadro de participaciones del 200, cruza cada dividendo
cobrado de una participada con sus documentos (acta de reparto, 123 o 193, certificado; se
reconocen por el concepto del banco y por el nombre de su carpeta o del documento) y, si hay que
rectificar el 200 de varios años, dice en qué orden (del más antiguo al más reciente, con los de en
medio que cambian con ellos). Con los dividendos confirmados, si la empresa tenía al menos el 5 %
de la participada y le retuvieron el 19 %, es un error de la gestoría (esa retención no se debía
practicar) y se resta en el 200 del año; el resumen dice lo que se recupera en el próximo 200 (las
retenciones menos el impuesto que corresponde a esos dividendos con la exención, una estimación).
Si los 123 de la empresa declaran retenciones de dividendos que ha pagado (desde 2024), avisa
cuando en la carpeta no está el acta que acuerda el reparto. Con varias declaraciones del mismo
periodo usa la vigente y, en los
resúmenes anuales (190, 180, 193, 347 y 349), le suma sus complementarias (una complementaria sin
la anterior no entra en los cruces). En el IVA avisa también del IVA de comidas, bebidas, regalos u
ocio que deduce el libro (art. 96 LIVA; si sólo lo dice el nombre del proveedor, o la empresa vende
con IVA reducido, como un bar, es una nota), de las facturas apuntadas dos veces, del 349 que tenía
que ser mensual (mientras no haya prescrito) y de las emitidas sin IVA a la UE sin la mención
«inversión del sujeto pasivo». Lo que se deduce de más no sale «a pagar» si los 303 salen a
compensar y lo pendiente lo cubre. Si el libro no trae la fecha de registro, cada factura puede
contarse en el trimestre de la carpeta de su PDF. Las líneas del libro de emitidas que son la
autorrepercusión de compras no se cuentan como ventas. En los bancos, un extracto sin número de
cuenta va con la única cuenta del mismo banco, divisa y titular con fechas seguidas y saldos que
encadenan (si puede ser de varias, se dice y no se revisa; si no es de ninguna, se revisa aparte,
como otra cuenta), y en los meses sin extracto da el saldo de antes y de después: si apenas cambia,
es una nota (probablemente sólo comisiones; con el saldo a cero, no). Dice de quién es cada cosa
(empresa, gestoría o documentación) y prepara **«Qué pedir»** (a la gestoría, al Registro, en la
Sede o en la empresa). Informe en PDF, Excel o Markdown. No cambia nada ni envía nada (salvo lo que
preguntes a la IA con «Preguntar a la IA», en cada hallazgo).

- **Modelos que faltan**: en los ejercicios comprobados con «Comprobar lo presentado», según lo que
  tiene la AEAT; en los demás, según el perfil fiscal. Sin perfil guardado, se deduce de lo
  presentado (el NIF, los modelos de los tres últimos ejercicios con declaraciones y el certificado
  de situación censal), con una nota, y un modelo sólo se pide en los años en que se ha presentado
  cerca. También por el perfil, en un ejercicio
  comprobado, los modelos de los que la AEAT no contestó (la consulta falló o no se hizo) y lo que
  vencía después de la comprobación. Lo que está en la carpeta pero no se ha podido leer es una nota
  (no se pide); un cálculo o un borrador de la gestoría no cuenta como presentado. El 115 y el 180
  sólo se piden con indicios de un alquiler (contrato, facturas con la retención del alquiler, algún
  115 o 180, o la obligación en el certificado de situación censal); el 202, sólo si la cuota del
  200 que le sirve de base sale positiva; y el 303 no se pide antes del alta en el IVA ni después de
  la baja que dé el certificado de situación censal. Si no se sabe, se dice sin pedirlo.
- **AEAT**: los certificados (estar al corriente, situación censal, IAE, residencia fiscal) con
  su fecha, lo que dicen y hasta cuándo valen; los requerimientos, con su plazo (aproximado si no
  está la notificación), sus contestaciones (una solicitud de ampliación del plazo no lo es) y lo
  que pueden suponer para la prescripción de los años que piden, si prescribirían antes por su
  plazo ordinario; y las devoluciones pedidas, frente a los abonos del banco y al plazo de seis
  meses (después, la AEAT debe intereses). Las copias que guarda la Sede en cada expediente y las
  de los justificantes se dicen, sin avisar.
- **Plazos**: una complementaria o sustitutiva se juzga con la primera declaración del periodo; si
  llegó a tiempo, no es una presentación fuera de plazo, y la complementaria que ingresa algo
  tarde lleva el recargo sólo sobre lo que ingresa.

### Dejar la gestoría

Si vienes de una gestoría, esta sección es el plan para empezar a llevar tú la empresa, en orden
y con lo que vas marcando guardado (el Panel recuerda los pasos que faltan mientras no acabes):

1. Datos de la empresa y perfil fiscal. 2. Certificado electrónico. 3. **Pedir a la gestoría lo
que es tuyo**: un correo listo para copiar (libros y diario, libros de IVA, modelos y
justificantes, cuentas anuales y libros legalizados, actas, contratos, notificaciones y las
autorizaciones que usan en tu nombre); TaxPain no lo envía: lo copias y lo mandas tú. 4. Importar
su carpeta. 5. Importar el ejercicio del gestor. 6. Fecha de corte y saldos iniciales (apunta la
fecha). 7. Revisar lo que hicieron. 8. Un trimestre en paralelo con «Comparar con la gestoría».
9. **Revocar el apoderamiento** en la Sede de la AEAT (enlace a «Apoderamientos»; se hace allí
con tu certificado) y, si presentaban como colaborador social, comunicarles por escrito que el
documento de representación queda sin efecto; en la Seguridad Social, quitar al autorizado RED.
10. Avisos de notificaciones (AEAT y DEHú) y cuenta de domiciliación. 11. Quedarte con el
calendario.

Cada paso se marca «hecho» o «no aplica»; si TaxPain ve que algo ya está (p. ej. hay diarios del
gestor importados), lo dice, pero no lo marca solo.

---

## Facturación

> La versión que se descarga de la web **no emite facturas**: no verás «Nueva factura»,
> «Rectificar» ni «Anular», y un aviso lo explica. Sí puedes ver las facturas que importes y
> preparar presupuestos. Para emitir hace falta compilar TaxPain desde el código, con tus datos
> de productor (README → Instalación → Compilarla tú). Lo que sigue describe esa versión.

### Facturas

La primera vez, «Configura tus facturas». Después:

- **«Nueva factura»**: país y NIF del cliente (vacío = simplificada, hasta 400 €, o hasta
  3.000 € si en la configuración de facturas marcas comercio minorista, hostelería u otra
  actividad del art. 4.2 del RD 1619/2012; la completa y la rectificativa piden tu domicilio y el
  del cliente; NIF-IVA para la UE, con
  «Comprobar el NIF-IVA en VIES»), fecha de la operación, idioma (español o inglés) y moneda
  (en otra moneda, con el «Tipo del BCE»). Cada línea con su IVA (21, 10, 4 %, exenta, servicio
  o bienes a la UE, exportación, inversión del sujeto pasivo), retención y anticipos.
- «Ver borrador (PDF)» y **«Emitir factura…»**: se confirma con el desglose y recibe su número
  definitivo; desde ahí no se edita.
- En la lista: «PDF»; «Guardar» como PDF, **Facturae** (XML), **Facturae firmada** (`.xsig`, para
  FACe, con tu certificado) o **UBL**; «Correo» abre un mensaje en Mail con el PDF adjunto (lo
  envías tú); «Duplicar» o «Repetir» cada mes, trimestre o año; **«Rectificar»**; **«Anular»** (el asiento
  va en la fecha de la factura; si su 303 ya está presentado, su IVA liquidado o el ejercicio
  cerrado, toca rectificativa).
- **Rectificar**: eliges el **motivo** (R1 error o devoluciones, descuentos posteriores,
  operación anulada o cambio de precio, art. 80.Uno, Dos y Seis LIVA; R2 concurso del cliente;
  R4 resto de causas; la de una simplificada es siempre R5 y va sin los datos del cliente) y el
  **tipo**:
  - **Por diferencias**: sólo lo que cambia, en negativo para reducir (un abono).
  - **Por sustitución**: los importes correctos completos; TaxPain parte de las líneas de la
    factura que sustituyes. En VERI\*FACTU lleva el importe de lo sustituido, y en el 303, el
    349, el 347, el 130, el libro registro, Cobros y pagos y la contabilidad sólo cuenta la
    diferencia. Una factura se sustituye una vez (si hay que volver a corregirla, rectifica la
    rectificativa).
  - «Agrupar otras facturas del mismo cliente» rectifica varias de una vez (un descuento por
    volumen, por ejemplo).
  Las rectificativas van en su serie propia. La rectificativa por **crédito incobrable (R3)** no
  se hace aquí: en Cobros y pagos → «Créditos incobrables».
- **Control de numeración** (debajo de la lista): por serie y año, cuántas hay, de qué número a
  cuál, y si falta algún número, alguna tiene fecha anterior a la anterior o está en una serie que
  no es la suya.
- **Facturas periódicas**: plantillas que se emiten a mano, cada una con su confirmación.
- **VERI\*FACTU** (si lo activas en Ajustes): cada factura genera su registro encadenado con
  huella y QR; «Enviar a la AEAT (pruebas)» los manda con tu certificado y «Subsanar…» corrige
  los rechazados. **Por ahora sólo al entorno de pruebas**: lo que acepta sale como «Aceptado en
  pruebas» y no cuenta como remitido; cuando haya envío real se volverá a mandar en orden.
  - **La cola de envío**: con un solo Touch ID, «Enviar a la AEAT» manda todo lo pendiente en
    lotes de hasta 1.000 registros y espera entre lote y lote lo que pida la AEAT (60 s al
    principio). Si pide más de 3 minutos, el resto se queda en cola y Facturas te dice a qué
    hora puedes volver a enviar.
  - **Si no hay conexión** o la AEAT no contesta, no se pierde nada: los registros siguen
    pendientes, el aviso de Facturas dice cuántos faltan y el siguiente envío va marcado como
    «incidencia», como pide la norma. Si la AEAT rechaza el envío entero por un error del
    mensaje, la cola se para hasta que pulses «Reintentar…».
  - **Cada factura dice su estado**: pendiente de remitir, aceptado en pruebas, remitida,
    aceptada con errores, rechazada (pasa el ratón por encima para ver el motivo) o «sin
    respuesta de la AEAT» (no contestó por ella dos veces: consúltala en la Sede antes de
    reenviarla). Si la AEAT ya tenía un registro (lo reenviaste tras un corte), cuenta como
    remitido.
  - **El QR y la leyenda «Factura verificable en la sede electrónica de la AEAT»** irán en todas
    tus facturas en cuanto TaxPain envíe de verdad a la AEAT; mientras sólo envíe a pruebas, tus
    facturas reales salen sin ellos (sólo los lleva la empresa de pruebas).
  - **Con VERI\*FACTU desactivado** no se genera registro y Facturas te avisa: TaxPain sólo
    funciona en modo VERI\*FACTU, y desde el 1 de enero de 2027 (sociedades) o el 1 de julio de
    2027 (autónomos) hay que facturar con un programa que lo genere, salvo que lleves el SII.

### Presupuestos

Presupuestos (con fecha de validez) y facturas proforma: PDF, guardar, enviar por Mail, cambiar
el estado (borrador, enviado, aceptado, rechazado) y **«Facturar»**, que abre la factura rellena
para revisarla y emitirla. No son facturas: no cuentan en la contabilidad ni en los modelos.

### Recibidas

Las facturas de tus proveedores.

- Entran con «Importar PDF / Facturae», arrastrándolas a la ventana (también fotos, que el Mac lee
  con su reconocimiento de texto) o **vigilando carpetas**.
- **Carpetas vigiladas.** «Vigilar una carpeta» (y después «Vigilar otra carpeta») añade una
  carpeta de esta empresa, por ejemplo la de Drive donde otra herramienta deja las facturas del
  correo. Cada empresa tiene las suyas y se miran **con todas sus subcarpetas** cada dos minutos,
  sólo los ficheros nuevos o cambiados (uno que se está copiando espera a la pasada siguiente).
  - Las **facturas recibidas** nuevas entran solas aquí, pendientes de revisar y sin duplicados.
    No entran solas las que ya están en un libro registro ni las de un periodo ya declarado.
  - Los **extractos, modelos, libros y demás documentos** no se importan solos, porque cambian la
    contabilidad: salen como aviso en [Importar](#importar) para que los revises.
  - Un fichero que no se puede leer (dañado, protegido o de más de 25 MB) sale como aviso en
    Importar y no se vuelve a intentar hasta que cambie. Uno que hace fallar la lectura dos veces
    se aparta y se avisa.
  - «Quitar», junto a cada carpeta, deja de vigilarla.
- **«Pendientes de revisar»**: TaxPain rellena lo que ha leído y marca lo dudoso. Tú eliges el
  tipo de gasto (general, profesionales con retención, alquiler), cómo va el IVA (interior,
  proveedor de la UE, inversión del sujeto pasivo), qué parte es deducible, si es un bien de
  inversión… y pulsas **«Confirmar datos»**. Antes de grabar, TaxPain enseña el resumen
  (proveedor y NIF, número, fecha, base, IVA con su porcentaje deducible, retención, total y
  **el trimestre del 303 al que va su IVA**, que es el del día en que la confirmas) y lo que
  conviene saber: por ejemplo, que la factura es de un trimestre anterior. Se graba con «Sí,
  confirmar»; «Volver a los datos» no graba nada. **Nada cuenta hasta que lo confirmas.**
- «Rellenar con el asistente» (anonimizado), «Ver documento» y «Descartar» (se recupera desde
  «Descartadas»).
- **Corregir una revisada.** Si confirmaste un dato mal (la base, el IVA, el deducible, la
  fecha, el régimen…), «Corregir…» en «Revisadas» abre el formulario con lo que confirmaste.
  «Revisar la corrección» enseña el antes y el después y los avisos; «Sí, corregir» guarda los
  datos nuevos y **rehace su asiento en su sitio**: el mismo asiento, con sus documentos y
  observaciones; las líneas que siguen conservan el reparto por centros de coste y, si no cambia
  su importe, el punteo del mayor. Si la nueva fecha es de otro ejercicio, el asiento pasa a él.
  La factura sigue en el 303 del trimestre en que la registraste, y la corrección queda en la
  auditoría con el antes y el después.
  - Es para lo que **tecleaste** mal. Si la que está mal es la factura del proveedor, pídele una
    factura rectificativa y regístrala como otra recibida.
  - **Avisos.** Si el 303 de su trimestre ya está presentado (desde TaxPain, importado en «Modelos
    presentados» o según la comprobación de lo presentado en la AEAT), TaxPain lo avisa y no
    presenta nada: si la corrección baja el IVA deducible, toca una autoliquidación
    rectificativa de ese 303; si lo sube, puedes deducir la diferencia en un 303 posterior,
    dentro de los cuatro años (arts. 99 y 114 de la Ley del IVA). También avisa si ya presentaste
    el 111 o el 115 (cambia la retención), el 349 o el 347, y si la liquidación del IVA del
    trimestre ya está en el diario (rehazla después en Modelos).
  - **No se puede** si el ejercicio está cerrado (reábrelo en Cierre), si es un bien de inversión
    (está en Inmovilizado), si es la cuota de un leasing (se cambia desde Leasing) o, si ya está
    pagada, cambiar lo que rompería el pago: el total, el proveedor, la moneda o el criterio de
    caja (deshaz antes el pago). El desglose, la fecha o el deducible de una pagada sí se corrigen.
- **«Deshacer la revisión»** devuelve una revisada a «Pendientes de revisar» y borra su asiento,
  si no tiene pagos ni está conciliada con el banco (y su ejercicio está abierto). Se queda con
  los datos que confirmaste para revisarla otra vez; entonces su IVA irá al trimestre en que la
  vuelvas a confirmar. Sirve, por ejemplo, para pasar a bien de inversión una que entró como
  gasto.

### Clientes y proveedores

Fichas con lo facturado o comprado en el año, lo pendiente y lo vencido, un resumen por años (con
la pista del 347) y el historial de facturas. «Comprobar en VIES» para las empresas de la UE.
Borrar una ficha no toca sus facturas.

---

## Contabilidad

### Contabilidad

Partida doble con el plan general (PGC o PGC PYMES). Arriba se elige el ejercicio y «Exportar a
Excel…» saca diario, mayor, sumas y saldos, balance, pérdidas y ganancias y cartera.

- **Libro diario**: los asientos que salen solos de facturas, cobros y pagos, y los tuyos
  («Nuevo asiento», con plantillas, cuadre automático y repetición mensual, trimestral o anual).
  Buscar, modificar o borrar (queda en la auditoría), adjuntar documentos, copiar asientos a otra
  empresa y «Factura recibida sin documento».
- **Plan de cuentas**: dar de alta o renombrar cuentas, las cuentas que se usan al contabilizar
  facturas y copiar el plan de otra empresa.
- **Cobros y pagos**: vencimientos con su antigüedad, «Registrar cobro/pago…», «Fraccionar…» y
  «Recordatorio…» por Mail; condiciones de pago de cada cliente o proveedor; **remesas SEPA** de
  recibos (pain.008 o Cuaderno 19.14) y de transferencias (pain.001 o Cuaderno 34.14), que se
  suben al banco a mano, con sus devoluciones; el cuadre con la contabilidad y el periodo medio
  de pago a proveedores; y los **créditos incobrables**:
  - Salen las emitidas vencidas sin cobrar. «Seguir como incobrable…» abre su calendario: el
    plazo de **seis meses** (si el año anterior no pasaste de 6.010.121,04 € de volumen de
    operaciones) o **un año** desde el devengo (con varios vencimientos, desde el último
    impagado), y desde ahí **seis meses** para emitir la rectificativa. Dile si el crédito tiene
    garantía real, aval o seguro de crédito, o si el cliente está en concurso (entonces es una R2
    desde Facturas); si es vinculado, TaxPain lo detecta.
  - «Anotar la reclamación…»: hace falta reclamar el cobro por **burofax** (con certificación de
    texto y acuse de recibo), **requerimiento notarial** o **demanda** antes de rectificar.
  - Con todo en verde, «Emitir la rectificativa R3…» la emite por la parte no cobrada, en la serie
    de rectificativas y con su registro VERI\*FACTU. Sólo reduce el IVA: el asiento es 477 a
    cliente por la cuota y el cliente sigue debiendo la base, que se **deteriora** en los ajustes
    de cierre (ya sin el IVA).
  - «Datos para comunicarla a la AEAT» reúne lo que pide la sede y lo que hay que adjuntar; tienes
    **un mes** desde la R3. TaxPain no la envía: al hacerlo, «Anotar la comunicación…».
  - Si después desistes de la reclamación o llegas a un acuerdo de cobro, anótalo: hay que
    rectificar al alza en el mes siguiente.
- **Sumas y saldos** y **Libro mayor** (con punteo y traspaso de saldos).
- **Banco**: ver [Banco](#banco).
- **Inmovilizado**: altas con su método y coeficiente, plan de amortización, bajas y ventas, y
  «Registrar amortizaciones de {año}». «Dar de alta el inventario del gestor en Inmovilizado…»
  da de alta los bienes del inventario de activos con lo ya amortizado hasta el final del periodo
  o del ejercicio del listado, sin ningún asiento (sus saldos ya vienen del diario del gestor o de
  la apertura): con el coeficiente que da lo ya amortizado (el plan del gestor) y, si el diario
  del gestor ya está en TaxPain, con sus subcuentas de amortización. Lo que ya está no se repite
  (dos bienes iguales del listado son dos) y los terrenos no entran. No es el «Importar el
  inventario del gestor…» de Saldos iniciales, que los mete en la apertura que contabiliza el
  asistente. Al «Dar de baja o vender», elige la «Factura de venta
  (opcional)» y su base deja de contar como ventas: el ingreso queda una vez, como beneficio o
  pérdida. Si aún no la has emitido, véndelo sin factura (el precio queda en la 543) y después
  «Vincular la factura de venta». Una factura vinculada se puede «Desvincular»; las rectificativas
  pendientes se pasan con «Pasar R… a la venta». Si la amortización del año ya estaba registrada,
  se rehace hasta el día de la baja. **«Deshacer la baja»** la quita: se borra su asiento y la
  amortización vuelve a como estaba (la del año, hasta el cierre; la de los años siguientes que
  la baja quitó, otra vez registrada). Si tiene la factura de venta vinculada, desvincúlala antes;
  si cobraste la venta contra la 543, deshaz también esa conciliación. El 390 resta esas ventas del volumen de operaciones ([106]
  inmuebles y [107] bienes de inversión desde 3.005,06 € de coste). **Leasing**: contratos con su cuadro, las cuotas como
  facturas del arrendador y la reclasificación a largo plazo.
- **Corregir el inmovilizado.** «Editar» cambia el nombre, la fecha de puesta en funcionamiento,
  el coeficiente (se ve la vida útil) y el valor residual: las amortizaciones ya registradas no
  cambian y las de los años sin registrar se calculan con los datos nuevos (un cambio de
  estimación vale hacia delante). La fecha no se cambia si ya hay amortizaciones registradas:
  deshazlas antes. «Borrar el elemento» quita un alta hecha por error (sin asientos).
  «Registrar amortizaciones de {año}» enseña antes cada cuota y el total; **«Deshacer la
  amortización de {año}»** las borra si el ejercicio está abierto (no la de un elemento dado de
  baja, que es parte de la baja, ni la que no es la última del elemento: se deshace de la última
  hacia atrás).
- **Incentivos fiscales de la amortización** («Incentivo fiscal» en cada elemento). La
  contabilidad sigue con su coeficiente; lo que cambia es la amortización fiscal, y la diferencia
  va sola al modelo 200 (diferencias de amortización, aumento o disminución) y se revierte en
  los años siguientes y, entera, en el de la baja o la venta. Debajo de «Registrar
  amortizaciones» salen las diferencias del año por elemento.
  - *Escaso valor* (art. 12.3.e LIS): elementos nuevos del inmovilizado material de hasta 300 €
    cada uno, con un límite de 25.000 € al año: se deducen enteros el año del alta. TaxPain no
    deja pasar de los 300 € ni de los 25.000 €.
  - *Libertad de amortización con creación de empleo* (art. 102 LIS, empresa de reducida
    dimensión): pon el incremento de la plantilla media (con dos decimales). La inversión máxima
    es 120.000 € por cada persona de incremento. Ojo: el aumento tiene que darse en los 24 meses
    siguientes y mantenerse otros 24; si no, hay que ingresar lo deducido de más con intereses.
    TaxPain no lo vigila.
  - *Amortización acelerada* (art. 103 LIS, empresa de reducida dimensión): pon el coeficiente
    máximo de tablas del elemento; el fiscal llega como mucho al doble (si lo dejas vacío, el
    doble).
  - *Otra libertad de amortización*: sin topes que comprobar.

  Ser empresa de reducida dimensión (cifra de negocios del año anterior por debajo de 10 M€) y
  que el elemento sea nuevo no se puede comprobar del todo: TaxPain avisa con las ventas del
  diario del año anterior.
- **Préstamos**: los préstamos que **concede** la sociedad (a un socio, a una participada o a un
  tercero) y los que **recibe** (de un banco, de un socio o de otro), con su cuadro. En «Nuevo
  préstamo» eliges el sentido y la contraparte (un socio de la ficha, un contacto, una participada
  o un texto), si es un banco o una persona física y su **vínculo** (grupo, asociada u otra parte
  vinculada: un socio con el 25 % o más, un administrador…); el importe, la fecha, el interés
  nominal anual, el sistema (**francés** de cuota constante, **lineal**, **intereses periódicos y
  el capital al final** o **todo al vencimiento**), la periodicidad, el plazo y la carencia (de
  capital, pagando sólo intereses, o total, con los intereses sumados al capital). TaxPain propone
  las subcuentas (concedido: 2520001/5420001, o 2423001/5323001 si es del grupo; recibido de un
  banco: 1700001/5200001; de un tercero: 1710001/5210001; de una vinculada: 1633001/5133001…;
  intereses a la 762 o la 662 y los devengados a la 547, 5343, 527, 528, 5143…) y la retención:
  la sociedad **retiene el 19 %** de los intereses que paga a un prestamista que no es un banco
  (va a la 4751, al **123** del trimestre en «Resto de rentas» y al **193** con la clave B), y una
  sociedad que te paga intereses te retiene el 19 % (473, pago a cuenta del Impuesto sobre
  Sociedades). Para una cuenta corriente con el socio, pon la 551 en las dos cuentas de capital.
  Todo se puede cambiar antes de contabilizar.
  - **Alta**: «Contabilizar la entrega (o recepción) del principal» contra un movimiento del banco
    que cuadre (queda conciliado) o contra una cuenta; lo que vence en doce meses va a la cuenta
    de corto plazo. Si el préstamo ya estaba en el diario (saldos iniciales o el del gestor),
    «Ya está en el diario», y en «Cuotas ya contabilizadas fuera de TaxPain» marca las cuotas
    pasadas sin hacer asiento.
  - **Registrar cuota**: la siguiente del cuadro, con el capital, los intereses, la retención y el
    neto contra el movimiento del banco (o una cuenta). Se puede deshacer la última.
  - **Devengar intereses a fecha** (en cada préstamo o «Devengar los intereses de todos» al cierre):
    los intereses de la cuota en curso desde el último vencimiento, en proporción a los días;
    al registrar la cuota, lo devengado se salda.
  - **Reclasificar al cierre de {año}**: cada cuenta de largo plazo queda con el capital que vence
    a más de un año según el cuadro (el resto pasa al corto plazo). Se puede repetir.
  - **Avisos**: préstamo sin interés o con un tipo muy bajo con una vinculada (art. 18 LIS, valor
    de mercado), consecuencias en el IRPF del socio, retención que falta o que sobra y el modelo
    600 de un préstamo de un particular. El tipo de mercado lo decides tú: TaxPain sólo avisa.
  - **Contrato de préstamo (RTF)…**: un borrador con los datos de la ficha y el cuadro, para
    revisar en Pages o Word, completar lo que va entre corchetes y firmar.
  Un préstamo con una vinculada (marcado así o con un socio, participada o vinculada por su NIF)
  va solo al registro de operaciones vinculadas y al 232, a la memoria y, si es con el socio
  único, a los contratos por transcribir; sus cuotas pendientes salen en la previsión de
  tesorería.
- **Cuentas anuales**: balance y pérdidas y ganancias (modelos PYMES, abreviado o normal,
  comparados con el año anterior), borrador de la memoria (RTF o PDF), **ratios financieros** y el
  **depósito de cuentas** (un ZIP para el programa D2 de los Registradores; TaxPain no presenta
  nada).
  - **Qué modelo te toca**: arriba sale el activo, la cifra de negocios y la plantilla media de
    este año y del anterior y cuántas de las tres condiciones cumples. Con dos de tres en los dos
    años (activo hasta 4 millones, cifra de negocios hasta 8 millones, hasta 50 personas) puedes
    usar el de PYMES o el abreviado; si no, el normal (arts. 257 y 258 de la Ley de Sociedades de
    Capital). Si miras otro modelo, «Usar el modelo … para la sociedad» lo guarda (pide
    confirmación) y lo usan la memoria, el ECPN, el 200 y el depósito.
  - **Memoria**: sale con las notas del modelo elegido (la abreviada tiene su nota de hechos
    posteriores; la normal, sus 26 notas) rellenas con la contabilidad: movimientos del
    inmovilizado, activos y pasivos financieros por categoría y los vencimientos de los préstamos
    de Contabilidad → Préstamos, fondos propios con sus movimientos y las participaciones del libro
    registro de socios, conciliación con el 200, ingresos y gastos, plantilla media (fijos y no
    fijos), periodo medio de pago. En «Lo que no sale de la contabilidad» escribes los hechos
    posteriores, los honorarios del auditor (0 si no se audita), la fecha de formulación, los
    administradores que firman (sale la hoja de firmas) y una **nota fija** que se repite cada año
    (por ejemplo, la del canje de valores del art. 86 LIS). Lo demás queda entre corchetes.
  - **Estado de cambios en el patrimonio neto (ECPN)**: debajo del balance, con los ingresos y
    gastos reconocidos y el estado total (saldo inicial, resultado, dividendos, ampliaciones,
    distribución del resultado y saldo final, por capital, reservas, resultados…). Obligatorio en
    el abreviado y en el normal; voluntario en el de PYMES. Se guarda en PDF o RTF.
  - En el depósito, sin memoria adjunta puedes marcar «Incluir el borrador de memoria de
    TaxPain»: va al ZIP con el modelo del depósito (complétala antes si lleva corchetes).
- **Analítica y presupuesto**: centros de coste, repartos, la cuenta de resultados por centro y
  el presupuesto del año con sus desviaciones (sin tocar el diario).
- **Nóminas**: fichas de los empleados y las nóminas **con los importes del recibo de tu gestoría
  laboral** (TaxPain no las calcula): asiento, 111 y 190. Gastos de viaje (dietas y kilometraje)
  con lo exento.
- **IVA y libros**: liquidación del IVA de cada trimestre, **libros registro** de IVA e IRPF en el
  formato de la AEAT, exportación a **a3** (`SUENLACE.DAT`), importación de asientos (a3, Excel o
  CSV) y **legalización de libros** (ZIP para los Registradores). Debajo, el **IVA de holding y
  cartera**:
  - **Diagnóstico**: con las participadas de Vinculadas, los dividendos (760), los ingresos
    financieros (761, 762, 769), los préstamos concedidos a participadas y lo facturado a ellas,
    TaxPain dice si parece una holding pura (la mera tenencia no deduce), mixta o una actividad
    con cartera, y enseña tres porcentajes para comparar: la prorrata con las facturas, la misma
    con los intereses en el denominador (si prestar es habitual; si no, no cuentan, art.
    104.Tres.4.º LIVA) y la proporción de operaciones sujetas sobre el total de ingresos con los
    dividendos (art. 93.Cinco). Orienta, no califica.
  - **Afectación de gastos comunes**: una regla por proveedor (NIF) o por cuenta de gasto (la de
    más importe del asiento de la recibida) con el porcentaje deducible, la fecha desde la que se
    aplica y el **criterio** escrito. El 303 (y el 390) la aplican por la fecha de registro; si la
    factura ya tenía un porcentaje menor, se queda el menor. La regla del proveedor gana a la de
    la cuenta. Debajo sale lo aplicado en el año y el IVA que deja de deducirse. Cambiar o borrar
    una regla queda en la auditoría y no toca los 303 ya presentados.
  - **Sectores diferenciados**: anota cada actividad con su CNAE, su porcentaje de deducción y su
    volumen del año anterior. Si una es de otro grupo de la CNAE y su porcentaje difiere en más de
    50 puntos del de la principal (la de más volumen), forma otro sector (art. 9.1.º.c LIVA); la
    que no pasa del 15 % de la principal puede ser accesoria. El 303 avisa mientras no separes la
    deducción; al separarla (pide confirmación), las recibidas con una regla de un sector deducen
    con el porcentaje de ese sector y sin la prorrata general (art. 101).
  - **Libro de bienes de inversión**: los bienes de inversión de las recibidas (y los elementos
    del inmovilizado a los que anotes el IVA soportado) durante su periodo de regularización,
    con la prorrata de cada año. Si la definitiva difiere en más de 10 puntos de la del año de
    compra, la quinta parte (la décima en inmuebles) va a [43] del último 303 del año. Si vendes
    el bien antes de que acabe el periodo, se regulariza de una vez por los años que quedan en el
    303 del periodo de la venta (art. 110): como deducible al 100 % si la venta lleva IVA (sin
    pasar del IVA repercutido) o al 0 % si es exenta o no sujeta, aunque no tengas prorrata. Cómo
    tributó la venta sale de su factura vinculada; si no la hay, anótalo («Anotar datos de IVA»).
    Los bienes de menos de 3.005,06 € de base no son de inversión a efectos del IVA.
- **Ventanilla única (OSS, modelos 035 y 369)**, también en Impuestos, para quien vende a
  **particulares de otros países de la UE** servicios electrónicos, de telecomunicaciones o de
  radiodifusión (cursos en línea, suscripciones, descargas…) o bienes que envía (ventas a
  distancia). Hasta que esas ventas pasan de **10.000 € al año** sumando todos los países (sin
  IVA), llevan IVA español; desde la venta con la que se superan (o desde el 1 de enero si se
  superaron el año anterior, o si optas por ello), llevan el **IVA del país del cliente**, que se
  declara cada trimestre en el **modelo 369** y no en el 303 (art. 73 de la Ley del IVA).
  - **Umbral**: lo vendido el año anterior y el actual, con avisos al acercarte (80 %) y al
    superarlo. Para sumar bien, clasifica cada factura a un particular de la UE: «servicio
    electrónico o de telecomunicaciones», «venta a distancia de bienes» u «otra operación» (una
    consultoría a un particular lleva IVA español y no cuenta). Las ventas de fuera de TaxPain
    (una tienda en línea) se apuntan en los ajustes. El Panel avisa también.
  - **Alta (035)**: cuando toca, TaxPain avisa de que hay que darse de alta en el régimen de la
    Unión con el modelo 035 en la Sede; apunta su fecha de efectos en los ajustes. Marca además
    la ventanilla única en el perfil fiscal para que el calendario avise del 369.
  - **Tipos por país**: TaxPain trae una tabla con el tipo general de cada Estado miembro, con su
    fecha y su fuente (Comisión Europea). Los tipos cambian: **confirma el de cada país** (aquí o
    en la propia factura) antes de emitir la primera factura con su IVA; sin confirmar, no se
    emite. Si la operación va a un tipo reducido en ese país, pon el tipo en la factura.
  - **La factura**: con un cliente de la UE marcado como particular, marca «Lleva el IVA del país
    del cliente», elige la operación y revisa el tipo. Las líneas van sin IVA español (no sujetas
    por las reglas de localización) y el total suma el IVA del país (p. ej. «IVA FR 20 %»). En el
    303 la base va a la casilla informativa [123] y en el 390 a la [126]; en contabilidad, el IVA
    extranjero va a una subcuenta propia de la 477 por país (4779006 Alemania, 4779011 Francia…),
    que la liquidación del 303 no salda. Estas facturas no generan Facturae ni UBL. Para
    corregirlas, rectificativa por diferencias.
  - **Libro y 369**: las facturas del trimestre con IVA de otro país; el borrador del 369 por
    país, clase de operación y tipo, con las rectificativas de trimestres anteriores como
    correcciones de aquel periodo, el plazo (último día del mes siguiente, sin trasladarlo si es
    festivo) y el asiento para cuando lo pagues (4779 de cada país contra el banco, a mano).
    «Guardar la hoja del 369 (XLSX)» lo deja en una carpeta para pasarlo al formulario de la Sede:
    TaxPain no tiene el diseño de registro del 369 ni lo presenta.
- **Cierre**: lo que hay que hacer antes y después del cierre, los **ajustes de cierre** (ver
  [Ajustes de cierre](#ajustes-de-cierre)), el Impuesto sobre Sociedades, las
  existencias, «Cerrar el ejercicio» (y reabrirlo; no cierra si quedan saldos en los grupos 8 o 9:
  pásalos a la 130 con un asiento), la **aplicación del resultado** con los
  mínimos legales y el texto para el acta, y **socios y dividendos** (con su retención, 123 y
  193).
- **Saldos en moneda extranjera al cierre** (en Cierre, sólo si tienes cuentas del banco en
  divisa o facturas en divisa sin cobrar o pagar al cierre). El Plan General Contable (norma de
  valoración 11.ª) pide valorar al **tipo de cierre** el dinero en divisa, lo que te deben los
  clientes y lo que debes a los proveedores en divisa. La tabla enseña cada partida: lo que es en
  divisa (el saldo del banco lo saca del último extracto; si no llega al cierre, escríbelo), los
  euros contabilizados, lo que vale al tipo de cierre y la diferencia. El tipo es el del BCE del
  último día del ejercicio: pulsa «Tipo del BCE del 31/12/…» o escríbelo y «Recalcular con lo
  tecleado». «Contabilizar la valoración» (con confirmación) graba el asiento del 31 de diciembre
  (las diferencias a favor a la 768 y las en contra a la 668) y **su reversión el 1 de enero**:
  así, cuando el año siguiente cobres o pagues esas facturas, la diferencia de cambio sale desde
  los euros de la factura y, sumando la reversión, el año siguiente sólo recoge lo que cambió el
  tipo desde el cierre. «Deshacer» quita los dos asientos mientras los dos ejercicios estén
  abiertos. Los anticipos (438, 407) no se valoran: no son dinero que se vaya a cobrar o pagar en
  divisa. En «Antes de cerrar» sale «Saldos en moneda extranjera valorados».
- **Cartera de valores al cierre** (en Cierre, sólo sociedades con valores en Inversiones). Según
  cómo clasificaste la cartera en Inversiones → «Cómo se calcula»:
  - **Mantenidos para negociar**: cada valor se lleva a su cotización del último día del
    ejercicio y la diferencia con lo que vale en libros (el valor del cierre anterior más lo
    comprado después) va a resultados: subidas a la 7630 y bajadas a la 6630. El año siguiente
    parte de ese valor. Tributa con el resultado, salvo las bajadas de participaciones de al menos
    el 5 % (art. 15.l LIS): el 200 lo propone sin incluir para que lo confirmes.
  - **A coste**: sólo hay asiento si un valor cotiza por debajo de lo que costó (deterioro, a la
    698 y la 596 a corto plazo, o a la 6963 y la 296 a largo). Si crees que la bajada es pasajera,
    marca «No es deterioro» en ese valor. Cada cierre revierte antes el deterioro de cierres
    anteriores (798 o 7963). El deterioro no es deducible: el 200 lo suma en la [00325] y su
    reversión la resta en la [00326].
  Los bonos y, en una cartera a coste, las opciones no se valoran aquí (se dice por qué). Si
  falta algún precio de cierre, ponlo antes en Inversiones. «Contabilizar la valoración» (con
  confirmación) graba el asiento del último día del ejercicio; «Deshacer» lo quita mientras el
  ejercicio esté abierto. En «Antes de cerrar» sale «Cartera de valores valorada al cierre».
- **Libros societarios** (sociedades): libro de actas, libro registro de socios y libro de
  contratos con el socio único. Ver [Libros societarios](#libros-societarios).
- **Entidad patrimonial y patrimonio neto {año}** (en Cierre, sólo para sociedades): el test del
  art. 5.2 de la Ley del Impuesto sobre Sociedades. Una sociedad es **patrimonial** si más de la
  mitad de su activo son valores o bienes no afectos a la actividad, con la **media de los
  balances trimestrales** (TaxPain toma el saldo de la contabilidad al último día de cada
  trimestre). Si lo es, no tiene el tipo reducido de las de nueva creación ni el de las de menos
  de 1 millón de cifra de negocios, ni los incentivos de reducida dimensión (con la reserva de
  nivelación), y hay limitaciones con las bases negativas y en la venta de sus participaciones.
  - La tabla trimestral dice, a cada fecha, el activo, lo afecto, los valores y lo no afecto que
    computan, lo que no computa (participaciones, tesorería justificada, dinero de ventas) y el
    porcentaje; abajo, la media y lo que separa del 50 %.
  - **Partidas del activo**: cada subcuenta de los grupos 2 y 5 (y cada cuenta de los grupos 3 y
    4) con la clase que propone TaxPain por su cuenta del PGC: inversiones inmobiliarias (22) no
    afectas, participaciones y fondos (24x, 25x, 53x, 54x) como valores, préstamos a socios (551)
    y créditos que no son de la actividad como no afectos, bancos (57) como tesorería y el resto
    afecto. Cámbiala si no es la que corresponde, **con su motivo** (un inmueble sin uso, un
    terreno, una obra de arte; una participación de al menos el 5 % que tienes desde hace un año
    y diriges con medios propios, que no computa). La marca vale desde ese año para los
    siguientes. Las amortizaciones y deterioros van con la clase de su elemento.
  - **Tesorería**: la ley no la menciona; se cuenta como no afecta salvo lo que pongas en
    «Tesorería necesaria para la actividad», con cómo lo justificas.
  - **Ventas de elementos afectos**: el dinero o el crédito de la venta de un elemento afecto (o
    de una participación que no computa) de este ejercicio o de los dos anteriores no computa
    desde la fecha de la venta. Anótala con «Anotar venta».
  - **Beneficios no distribuidos de la actividad** y los trimestres por encima del 50 %: sólo
    para el cálculo orientativo de la exención del socio en el Impuesto sobre el Patrimonio.
  - Si la sociedad es dominante de un grupo, márcalo: el cálculo debería hacerse con los
    balances consolidados.
  - **Patrimonio neto frente al capital**: a cada trimestre, el patrimonio neto a efectos
    mercantiles (el contable, más el capital no exigido, más los **préstamos participativos**,
    que indicas tú). Avisa si queda **por debajo de la mitad del capital** (causa de disolución,
    art. 363.1.e LSC: los administradores tienen dos meses para convocar la junta y, si no actúan,
    responden de las deudas posteriores, arts. 365 a 367 LSC), por debajo de dos tercios (en la
    SA, reducción obligatoria si pasa un ejercicio sin recuperarse, art. 327 LSC) o del capital
    (no se pueden repartir dividendos, art. 273.2 LSC).
  - «Ahorrar impuestos» usa este cálculo para avisar de que las ventajas de las empresas
    pequeñas pueden no aplicarse. Es un cálculo para revisarlo con tu asesor, no un veredicto.
- **Saldos iniciales**: un asistente de siete pasos para empezar a mitad de camino (balance del
  gestor, clientes y proveedores pendientes, inmovilizado, Hacienda y bancos) o «Importar un
  ejercicio entero del gestor» asiento a asiento, cotejado con su balance y sus modelos. Si el
  diario trae el cierre y el del ejercicio siguiente (en TaxPain o junto al fichero) trae su
  propia apertura, «Crear la apertura de {año} con los saldos del cierre» llega sin marcar, para
  que los saldos iniciales no vayan dos veces; si no la trae, llega marcada y ese diario se carga
  después sobre ella.

#### Ajustes de cierre

En Contabilidad → **Cierre**, debajo de «Antes de cerrar», están los **pasos del cierre** en su
orden; cada uno es un enlace (a la tarjeta de abajo o a la vista donde se hace: Inmovilizado para
las amortizaciones y el leasing, Préstamos para los intereses devengados y la reclasificación al
corto plazo, IVA y libros para la última liquidación). Cada ajuste se enseña antes («Ver el
asiento»), se contabiliza al confirmar con la fecha de cierre y se puede **deshacer** mientras el
ejercicio esté abierto.

- **Comprobaciones de saldos**: proveedores o acreedores con saldo deudor, clientes con saldo
  acreedor, bancos o caja en negativo, la 555 (partidas pendientes) con saldo, la 551 con socios o
  administradores y el IVA (472/477) sin liquidar. Cada aviso lleva a donde se arregla (el mayor de
  la cuenta, el banco, Préstamos o IVA y libros). Nada se corrige solo.
- **Periodificaciones**: las facturas del ejercicio de alquileres, seguros, publicidad y otros
  servicios (o todas, con la casilla). Indica el periodo del servicio (se propone un año desde la
  fecha de la factura) y TaxPain calcula por días lo que es del año siguiente: de una recibida, a
  la 480 (gastos anticipados); de una emitida, a la 485 (ingresos anticipados). El 1 de enero se
  revierte solo. Si el servicio dura más de un año, el cierre siguiente lo vuelve a proponer.
- **Facturas pendientes de recibir o de emitir**: un gasto de este año cuya factura llega después
  (4009) o un trabajo hecho y aún sin facturar (4309), por la base. Las facturas de enero y febrero
  salen como propuesta («Usar…»). Se revierte solo el 1 de enero: cuando llegue o emitas la
  factura, regístrala como siempre.
- **Deterioro de clientes**: los cobros que al cierre llevan más de seis meses vencidos. Eliges la
  circunstancia (seis meses, concurso, alzamiento de bienes, reclamación judicial) y si el crédito
  está excluido (deudor público, vinculado, con aval o seguro de crédito, renovado): TaxPain dice
  si es **deducible** (art. 13.1 LIS). El asiento es 694 a 490. Los de años anteriores salen en
  «De ejercicios anteriores» para **revertirlos** (490 a 794) cuando se cobran; si el cliente no
  va a pagar nunca, la pérdida (650) se contabiliza a mano.
- **Provisiones**: un riesgo o litigio con su importe estimado, la cuenta de la provisión (142,
  141, 145, 529x o 4999) y la de gasto (678, 631…). Por defecto **no deducible** (art. 14 LIS):
  márcala si lo es. Se revierte por exceso (a la 795) en un cierre posterior.
- **Existencias**: la tarjeta de siempre, más abajo.
- Lo **no deducible** (deterioros y provisiones) sale solo en el **200** (Modelos → 200,
  correcciones al resultado) con origen «Ajustes de cierre» y su casilla: el deterioro de créditos
  en [00321] (aumento) y su reversión en [00322] (disminución); las provisiones en [00337] y su
  exceso o aplicación en [00338]. Se puede cambiar o descartar como las demás. No lo repitas a
  mano. Si ya contabilizaste el IS, el cierre te avisa de que hay que rehacerlo.

#### Acuerdos de dividendos

En Contabilidad → **Cierre** → «Socios y dividendos», con los socios dados de alta, **«Acordar un
dividendo»** prepara el acuerdo antes del pago. Elige la clase:

- **A cuenta** del resultado del ejercicio en curso: la fecha del acuerdo, el importe, la fecha
  del **estado contable de liquidez** (la del acuerdo o anterior, del mismo ejercicio) y el tipo
  del Impuesto sobre Sociedades para estimarlo (25 % si no cambias nada). TaxPain saca el estado
  de la contabilidad: resultado desde el último cierre, menos pérdidas anteriores, la reserva
  legal que toca dotar y el impuesto estimado, menos lo ya repartido a cuenta: es lo **máximo**
  que se puede repartir (art. 277 LSC). Al contabilizar, el asiento es **557 a 526**.
- **Con cargo a reservas**: de las voluntarias (113) o del remanente (120). Asiento: la reserva a
  la 526.
- **Con cargo al resultado** del ejercicio que estás cerrando: se contabiliza en la aplicación del
  resultado (arriba); aquí registras el acuerdo de la junta y se crea su acta de aprobación de
  cuentas.

«Comprobar» enseña un **semáforo**: reserva legal dotada (art. 274 LSC), patrimonio neto no
inferior al capital tras el reparto (art. 273.2), gastos de investigación y desarrollo cubiertos
por reservas disponibles (art. 273.3), reservas de libre disposición suficientes, el límite y la
liquidez del dividendo a cuenta (art. 277), la ficha de socios y, si con lo marcado en Entidad
patrimonial más de la mitad del activo son valores o no está afecto, un aviso de lo que cambia. En
**rojo**, TaxPain no deja seguir y explica por qué; en **ámbar**, revisa antes. Con todo bien,
«Contabilizar el acuerdo y crear el acta» (con confirmación y Touch ID) graba el asiento y deja el
**acta en borrador** en Libros societarios → Libro de actas, con su plantilla (dividendo a cuenta
con el estado como anexo, dividendo con cargo a reservas o aprobación de cuentas), marcada como
«Acuerdo de dividendos». El órgano lo puedes elegir (el dividendo a cuenta lo pueden acordar los
administradores); si no, socio único o junta universal.

Después, «Repartir y pagar dividendos» liga cada pago a su acuerdo (el que elijas o el más antiguo
pendiente) y **avisa** si el acta aún no está aprobada o si el pago no tiene acuerdo registrado.
La retención va al **123** del trimestre de la fecha de pago y al 193 de enero, como siempre. Al
aprobar las cuentas, la aplicación del resultado **salda la 557** con el dividendo del ejercicio
(el dividendo ha de ser al menos lo repartido a cuenta). Un acuerdo se deshace mientras no tenga
pagos ni, el de a cuenta, el resultado aplicado; su acta se borra si sigue en borrador sin número.

#### Datos para la Renta y el Patrimonio del socio

Debajo de **Socios y dividendos** (Contabilidad → Cierre) está **Datos para la Renta y el
Patrimonio del socio**: eliges a un socio persona física o a un administrador y TaxPain junta lo
que la sociedad sabe de él en el año, para que lo que declare cuadre con lo que la sociedad ha
declarado en el 190, el 193 y el 123. **No calcula su Renta ni su Patrimonio ni le aconseja.**

- **Lo que le ha pagado la sociedad**, cada concepto con su importe, su retención, de dónde sale
  y la casilla de Renta WEB en la que suele ir: las nóminas (clave E o A del 190), la cuota del
  RETA que paga la sociedad (retribución en especie, con su ingreso a cuenta) y las cuotas que
  puede restar, los dividendos cobrados con su retención, los intereses de lo que él ha prestado a
  la sociedad y las facturas que le ha emitido (un alquiler, unos servicios). Si el préstamo del
  socio a la sociedad no tiene interés, sale un **interés presunto** estimado con el interés legal
  del dinero; si es la sociedad la que le presta por debajo de ese interés, la diferencia estimada
  como retribución en especie. Lo estimado se marca. Las casillas cambian cada año: cotéjalas con
  el borrador de la AEAT.
- **Impuesto sobre el Patrimonio**: el valor de sus participaciones por el valor teórico del
  último balance aprobado (el patrimonio neto del ejercicio anterior por su participación) y un
  **semáforo de la exención de empresa familiar** con sus tres requisitos: que la sociedad no sea
  patrimonial (sale del cálculo de entidad patrimonial del año, con los beneficios de actividad que
  hayas indicado), que tenga al menos el 5 % (o el 20 % con su familia) y que cobre por dirigirla
  más de la mitad de sus rendimientos del trabajo y de actividades (teclea lo que gana fuera de la
  sociedad para comprobarlo). Si no se descarta, da la parte exenta orientativa con el activo
  afecto medio.

**Guardar en PDF** deja el informe para dárselo al socio o a quien le haga la Renta.

#### Libros societarios

En Contabilidad → **Libros societarios**, tres pestañas. Son documentos privados de la sociedad:
las escrituras y las inscripciones en el Registro Mercantil siguen pasando por la notaría.

- **Libro de actas.** «Nueva acta» → elige la plantilla y la fecha → «Preparar el borrador».
  Plantillas: aprobación de las cuentas con la aplicación del resultado y la gestión (la del
  ejercicio que elijas, que ha de estar cerrado: lleva el reparto contabilizado en Cierre o, si aún
  no, el que propone TaxPain); dividendo con cargo a reservas; **dividendo a cuenta**, con el
  **estado contable de liquidez** del art. 277 LSC sacado de la contabilidad a la fecha que digas
  (resultado desde el último cierre, pérdidas anteriores, reserva legal, impuesto estimado y
  tesorería; avisa si el dividendo pasa del máximo o de la tesorería); nombramiento y cese de
  administrador; retribución máxima del administrador (art. 217 LSC, avisa si las nóminas del
  año ya la superan); cambio de domicilio; y en blanco. Si la sociedad tiene un solo socio con el
  100 %, el acta es una **decisión del socio único**; si no, una junta universal. Revisa el texto
  (lo que va entre corchetes lo tienes que completar) y «Guardar borrador». Al **aprobar** un acta
  recibe el número siguiente del ejercicio y ya no se cambia; «Reabrir» la vuelve a borrador
  conservando el número (si ya legalizaste el libro, mejor un acta nueva que rectifique). De cada
  acta aprobada, «Certificación…» saca la certificación de los acuerdos (PDF o RTF) para el
  depósito de cuentas o el banco. «Libro de actas {año} en PDF…» guarda el libro con folios.
- **Registro de socios.** Anota los movimientos de las participaciones por orden de fechas:
  constitución, ampliación, reducción, transmisión, constitución y cancelación de cargas, con la
  numeración (del … al …), el nominal, el título (escritura, notario y protocolo) y, del
  adquirente, el domicilio y la nacionalidad. TaxPain no deja anotar lo que no cuadra (transmitir
  participaciones que no son del transmitente, repetir números) y enseña la **situación actual**:
  quién tiene qué numeración y qué porcentaje. Si no coincide con la ficha de «Socios y dividendos»
  o con la cuenta 100, lo dice; «Pasar los porcentajes a la ficha de socios» actualiza la ficha
  (los socios nuevos hay que darlos de alta allí a mano).
- **Contratos con el socio único.** Los contratos entre la socia o el socio único y la sociedad
  (art. 16 LSC: si no se transcriben y no se citan en la memoria, en un concurso no valen frente a
  los acreedores). TaxPain propone los que ya conoce del ejercicio (sus nóminas como
  administrador o trabajador, los dividendos que ha cobrado, los préstamos que le ha dado o que
  ha recibido de él en Contabilidad → Préstamos, con su principal y condiciones): «Revisar y
  transcribir». Los del
  ejercicio salen en la nota 12 del borrador de la memoria.

En **IVA y libros → legalización**, marca «Libro de actas», «Libro registro de socios» o «Libro de
contratos del socio único» con su número de libro y van al ZIP de los Registradores
(`ACTASCON_nnn.pdf`, `SOCIOS_nnn.pdf`, `SOCUNICO_nnn.pdf`) con su entrada en NOMBRES.TXT y
DATOS.TXT. El libro de actas lleva sólo las actas aprobadas del ejercicio. El de socios sólo hay
que legalizarlo el primer año y los ejercicios con cambios de titularidad o cargas.
#### Autónomo societario y recibo del administrador

En **Contabilidad → Nóminas**, si la ficha de un administrador o socio con control dice
«Autónomos (RETA)», sale la sección **Autónomo societario**:

- **Ficha del año**: la base por la que cotiza, el rendimiento neto mensual que comunicó a la
  Seguridad Social, los meses de alta, si tiene el control de la sociedad (sus dividendos cuentan),
  lo que gana fuera y sus gastos, y **quién paga la cuota**. Si la paga la sociedad, TaxPain la
  trata como retribución en especie del administrador y la asienta en la 642 (la puedes cambiar
  por la 649 o la 640) contra el banco; si la paga él y la sociedad se la devuelve, ese reembolso
  va en el bruto de su nómina. El año siguiente propone la ficha del anterior.
- **Rendimiento computable**: sus nóminas, las cuotas que paga la sociedad, los dividendos que
  cobra como socio (al 100 % por defecto) y lo de fuera, menos los gastos y un 3 % de gastos
  genéricos. El 3 % y la base mínima de los societarios sólo valen con **90 días de alta como
  societario en el año**: pon la **fecha de alta como societario** si empezó en el año (vacía si
  viene de antes); con menos de 90 días TaxPain aplica el 7 % y la base mínima del tramo, y lo
  avisa. Con eso dice el **tramo** que le toca, la base más baja que no tendría
  regularización y **cuánto reclamará o devolverá la Seguridad Social** al regularizar el año.
- **Cuotas**: registra cada cargo (con confirmación; desmarca el asiento si el cargo del banco ya
  lo contabiliza una regla). También el ingreso a cuenta de la especie (19 o 35 %).
  La tabla de tramos es la oficial de cada año (2025 y 2026, leídas en el BOE). En 2026 la base
  máxima es de 5.101,20 € y la **base mínima del autónomo societario** ya no son 1.000 €, sino la
  mínima del grupo 7 del Régimen General (1.424,40 €). Los dividendos cuentan enteros si el socio
  tiene al menos el 33 % del capital, o el 25 % siendo administrador; si no, pon el 0 %. Para un
  año sin orden de cotización publicada (hoy, 2027) TaxPain usa la del año anterior y lo avisa.
- **Cambios de base**: hasta seis al año, del 1 de enero al 28 de febrero (vale desde el 1 de
  marzo), marzo-abril, mayo-junio, julio-agosto, septiembre-octubre y noviembre-diciembre. Si en
  el perfil de la sociedad marcas «El administrador o un socio con control cotiza en el RETA», el
  calendario avisa de la cuota y del último día de cada plazo (también a los autónomos que
  marcan la cuota de autónomos).
- **Soporte societario**: si los estatutos dicen que el cargo es retribuido y el acuerdo de la
  junta con el importe máximo anual (art. 217 LSC). Avisa si falta o si lo pagado lo supera.

En la lista de nóminas, las de un administrador tienen **Recibo (PDF)…**: el recibo de su
retribución (dineraria, en especie, retención y líquido), con el aviso si no consta el acuerdo de
la junta.

Ojo: las tablas de tramos y bases de 2025 y 2026 **no están cotejadas con el BOE** (la pantalla
dice qué falta) y la de 2026 es la de 2025 prorrogada; la regularización es una estimación. La
especie y su ingreso a cuenta todavía no pasan solos al 111 ni al 190: añádelos al revisar los
borradores.

#### Calcular la nómina de un trabajador, seguros sociales y finiquito

En **Contabilidad → Nóminas**, debajo de la lista, **Calcular la nómina** es para trabajadores por
cuenta ajena del Régimen General (no para administradores):

- **Ficha**: grupo de cotización (1 a 11), contrato indefinido o temporal (cambia el desempleo),
  jornada, salario base, complementos (salariales, no salariales que cotizan o exentos, como las
  dietas dentro del límite), número e importe de las pagas extra (prorrateadas o no), la tarifa de
  accidentes de trabajo de tu actividad (IT e IMS, la de tu CNAE en la tabla de la Ley 42/2006) y
  el **tipo de retención del IRPF**. TaxPain no calcula la retención: el botón «Programa de
  cálculo de retenciones» abre el de la AEAT; pon allí los datos del trabajador y copia el tipo.
  También el número de afiliación, el código de cuenta de cotización, la categoría y la
  antigüedad, que salen en el recibo. «Guardar ficha» la deja para los meses siguientes.
- **Mes**: días cotizados, horas extraordinarias, pagas que se cobran ese mes, anticipos y otras
  deducciones. **Calcular nómina** enseña los devengos, la base de contingencias comunes (lo del
  mes más la prorrata de las pagas, entre la mínima de su grupo y la máxima), la de profesionales,
  cada cuota del trabajador y de la empresa (contingencias comunes, MEI, desempleo, formación
  profesional, FOGASA, AT y EP, horas extra y, si cobra por encima de la base máxima, la
  cotización de solidaridad), la retención, el líquido y el coste para la empresa.
- **Registrar nómina** (pide confirmación) hace el recibo con su asiento (640 y 642 contra 4751,
  476 y 465), que va al 111 y al 190 como los demás. En la lista sale **Recibo de salarios
  (PDF)…** con el modelo oficial de recibo (Orden de 27 de diciembre de 1994).

Las bases y los tipos son los de la orden de cotización de cada año (2025 y 2026). Lo que no se ha
cotejado con el BOE lo dice el propio cálculo («Sin cotejar con el BOE: …»): entre otras cosas, las
bases mínimas de los grupos 1 a 3 de 2026. Para un año sin orden (2027) no calcula. No calcula
bonificaciones, incapacidad temporal, especie ni convenios: el salario lo pones tú.

**Seguros sociales del mes** suma las cuotas de las nóminas del mes por concepto (y las de los
recibos tecleados, sin desglose): es lo que la TGSS cargará por la liquidación (RLC). Avisa del
plazo (el último día del mes siguiente) y enseña el asiento del devengo que ya hicieron las
nóminas. Cuando el banco cargue el RLC, **Registrar el pago** (con confirmación) hace el asiento
476 contra el banco. TaxPain no se conecta al Sistema RED ni a SILTRA: compara el total con el RLC,
que es el que vale.

**Finiquito** calcula los días del último mes, las vacaciones devengadas y no disfrutadas, la parte
de la paga extra pendiente y, si es el fin de un contrato temporal, la indemnización de doce días
por año. Lo que cotiza va en el último recibo; la indemnización no cotiza. Revisa siempre tu
convenio.

#### Subvenciones

En **Contabilidad → Subvenciones** se anotan las subvenciones, donaciones y legados recibidos:
quién la concede, el importe, la finalidad, las condiciones, si es de minimis y de qué tipo es:

- **De capital** (financia un bien: elige el bien del inmovilizado) o **donación de capital**: van
  al patrimonio neto (130 o 131) y pasan a resultados (746) **al ritmo de la amortización del
  bien**; si el bien se da de baja, pasa todo lo que quede.
- **Para gastos de ejercicios futuros** (132): pasan a la 747 a medida que se gastan (tecleas lo
  de cada año).
- **A la explotación**: van directamente a la 740.

Si al concederla **quedan condiciones por cumplir**, es reintegrable: es una deuda (172, o 522 la
de explotación) hasta que marcas **Pasar a no reintegrable** con la fecha en que se cumplen. Con
**efecto impositivo** (marcado por defecto, al 25 %), la parte del impuesto va a la **479**
(pasivo por impuesto diferido) y la 13x queda neta; al imputar, se revierte a la vez. Fiscalmente
la subvención tributa cuando se imputa a resultados: no hay ajuste en el 200.

Cada asiento (concesión, cobro, paso a no reintegrable, imputación del año) se **ve antes de
grabarlo** y se confirma; **Deshacer el último asiento** lo quita del diario mientras el
ejercicio esté abierto. Los asientos salen en el **estado de cambios en el patrimonio neto**
(ingresos imputados al patrimonio y transferencias a resultados, con su efecto impositivo) y la
**nota de subvenciones de la memoria** sale rellena con el registro. Arriba, el acumulado de las
ayudas de minimis de los tres últimos años frente al tope de 300.000 €.

El reintegro de una subvención (devolverla) todavía se asienta a mano.

#### Banco

**Extractos en fichero.** «Importar extractos del banco…» acepta **cualquier banco y formato**:
Norma 43, camt, OFX, QIF, MT940, Excel, CSV o PDF. Cada extracto se comprueba con sus saldos (uno
que no cuadra no entra sin confirmarlo) y no se repiten los movimientos que ya estaban, aunque
los extractos se solapen o vengan en formatos distintos. Las cuentas en otra divisa, las tarjetas
de crédito y los monederos (PayPal…) van cada uno a su subcuenta.

**Extractos importados.** «Ver los extractos importados…» enseña, por cuenta, cada extracto
que has importado: el fichero, el periodo, cuántos movimientos traía (y cuántos entraron nuevos
o están conciliados) y sus saldos inicial y final. Debajo avisa de los **huecos**: si un extracto
acaba con un saldo y el siguiente empieza con otro, faltan movimientos entre medias («Faltan
movimientos del … al …», con la diferencia); si no hay saldos y pasan más de siete días sin
extracto, «Posible hueco». Importa el extracto que falta y el aviso desaparece.

- **Deshacer** un extracto importado por error (de otra cuenta, repetido con otro nombre…) borra
  los movimientos que **sólo trae ese extracto**: los que también trae otro extracto se quedan.
  Si alguno está conciliado, TaxPain lo dice y, al confirmarlo, deshace antes esas
  conciliaciones (con su asiento), igual que en «Movimientos conciliados»: no se puede si es de
  un ejercicio cerrado o de un modelo ya presentado. El fichero se puede volver a importar.
- Los extractos importados antes de esta versión salen agrupados por su fichero, con el periodo
  y los saldos de sus movimientos.

**Conciliar.** Pulsa un movimiento y TaxPain propone a qué corresponde (el cobro o pago de una
factura, una remesa, una devolución) o lo llevas a una cuenta; de lo que contabilizas aprende
**reglas** para la próxima vez. «Ver los movimientos conciliados…» deja deshacerlo.

**Conexión con el banco (solo lectura, opción avanzada): BBVA y Revolut.** No hace falta para
llevar el banco: lo habitual es importar los extractos en fichero (arriba), que vale para
cualquier banco y no pide ningún alta. La conexión es para quien quiera que los movimientos
lleguen solos y no le importe montarla: hay que registrar una aplicación propia en Enable
Banking y vincular cada cuenta. TaxPain lee las cuentas, los saldos y
los movimientos de BBVA y Revolut (empresa o particulares) a través de
[Enable Banking](https://enablebanking.com), un proveedor de información de cuentas autorizado
por la normativa europea de pagos (PSD2). Es **sólo lectura**: el permiso que se pide no deja
mover dinero. Los movimientos entran como un extracto más: en la misma cuenta y subcuenta que sus
extractos en fichero y sin repetir ninguno.

*Configurarla (una vez por empresa).* No hay ningún servidor de TaxPain en medio: cada empresa
usa su propia aplicación de Enable Banking.

1. Entra en el [panel de Enable Banking](https://enablebanking.com/sign-in/) con tu email (la
   cuenta es gratuita) y registra una aplicación de **producción** («Production»).
2. Como dirección de vuelta («redirect URL») pon `https://localhost/taxpain/banco`. Si te deja,
   añade también `http://127.0.0.1:47219/taxpain/banco`: con ella TaxPain recoge solo la vuelta
   del banco.
3. Al registrarla, el navegador descarga su **clave privada**, un fichero `.pem` cuyo nombre es el
   id de la aplicación.
4. Activa la aplicación **vinculando tus cuentas** de BBVA o Revolut, una a una: sólo se leen
   las cuentas vinculadas (las demás vuelven vacías). Es el **modo restringido**, sin coste.
   Las [condiciones de Enable Banking](https://enablebanking.com/terms/) (9/1/2026) lo reservan
   a pruebas y al uso personal de particulares; a las empresas se lo permiten como prueba y
   pueden retirarlo avisando por escrito. Para un uso estable de empresa hace falta un contrato
   con Enable Banking (con verificación de la empresa y un mínimo mensual; el precio lo dan a
   petición). Si la conexión se corta, sigues con los extractos en fichero y no pierdes nada de
   lo importado.
5. En TaxPain, abre «Configurar la conexión», pulsa **«Elegir el fichero .pem…»** (el id sale del
   nombre del fichero; si no, escríbelo antes), marca **«Acepto…»** y **«Guardar»**. La clave
   queda cifrada en tu Mac y no vuelve a verse. Guarda el `.pem` en un sitio seguro (no en
   Descargas): no va en las copias de seguridad y te hará falta para restaurar en otro Mac.

*Conectar un banco.* Pulsa **«Conectar un banco…»** y elige, por ejemplo, «BBVA · empresa». Se
abre la página del banco en el navegador: autoriza con tu app o tu firma, como siempre.

- Con `https://localhost/…` como dirección de vuelta, al terminar el navegador dice que no puede
  abrir la página: es normal. **Copia la dirección completa** de la barra (lleva el código de la
  autorización), pégala en TaxPain y pulsa **«Terminar la conexión»**.
- Con `http://127.0.0.1:47219/…`, el navegador dice «Listo» y TaxPain sigue solo.

TaxPain guarda la conexión con sus cuentas y baja los movimientos: la primera vez, todo lo que dé
el banco; después, desde unos días antes de la última descarga.

*Después.*

- **«Actualizar movimientos»** baja lo nuevo de todas las cuentas activas («Actualizar BBVA», de
  un banco), con los avisos de lo que conviene revisar. Cada descarga cuenta como una consulta
  hecha sin ti delante y los bancos las limitan a cuatro al día por cuenta: con una o dos al día
  basta.
- Cada cuenta se puede **activar o desactivar**. Las tarjetas salen desactivadas: actívalas sólo
  si son de crédito y quieres conciliar sus compras (las de débito ya salen en la cuenta).
- Se ve el **saldo que da el banco**, la subcuenta contable y hasta qué día se ha bajado.
- El permiso dura hasta **180 días**. Quince días antes, TaxPain avisa: renuévalo conectando otra
  vez el mismo banco (se conserva todo lo bajado).
- **«Quitar la conexión»** retira el permiso en Enable Banking (si no puede, lo dice: entonces
  retíralo desde tu banco); los movimientos ya importados se quedan.
- Si borras un movimiento que llegó por la conexión (un duplicado, por ejemplo), no vuelve en
  las descargas siguientes.

*Lo habitual, sin conexión: descargar los extractos e importarlos.* Descarga el extracto de la
banca online y pulsa «Importar extractos del banco…»: vale para cualquier banco y no se repite
nada de lo que ya llegó por la conexión.

- **BBVA** (web o BBVA net cash): en los movimientos de la cuenta, descárgalos en **Norma 43**
  (Cuaderno 43) si te lo ofrece, que trae los saldos y los códigos de cada movimiento; si no, en
  Excel. El PDF del extracto también sirve.
- **Revolut** (Business o particulares): en la cuenta, «Extracto» («Statement»), en Excel o CSV
  (o PDF), uno por divisa si tienes varias.
- Los menús de los bancos cambian a veces: busca «Descargar» o «Exportar» en los movimientos.

**Cuentas en otra divisa (dólares, libras…).** Una cuenta en divisa (la de dólares de Revolut o
Wise, PayPal en libras…) lleva sus movimientos en esa moneda y la contabilidad en euros: cada
movimiento se contabiliza por su **contravalor en euros del día**.

- Debajo de cada movimiento en divisa sale su contravalor («= 833,33 € (1 € = 1,2 USD, tipo del
  BCE)») o el aviso «Sin contravalor en euros». Pulsa **«Tipo del BCE»** para descargar el tipo
  de referencia del BCE de ese día (es la única vez que se consulta al BCE; después queda
  guardado), o escribe los **euros que da el propio extracto** y «Guardar euros». Lo tecleado
  manda sobre el BCE; «Quitar lo tecleado» lo deshace.
- Si el extracto ya trae los euros, se recogen **solos al importarlo**: el CSV de **Wise**
  («Exchange To Amount» cuando se cambió a euros, o el importe entre el «Exchange Rate» cuando se
  cambió desde euros), el de **Revolut Business** («Orig amount» cuando «Orig currency» es EUR) y
  cualquier extracto con una columna «Contravalor». Quedan como tecleados y se pueden cambiar en
  cada movimiento; el resumen de la importación dice cuántos.
- Con el contravalor, el movimiento **se concilia como uno en euros**: se proponen las facturas
  en la misma divisa por su importe en ella (1.000 USD cobran la factura de 1.000 USD). Al cobrar
  o pagar, el cliente o el proveedor se saldan por los euros con que se contabilizó la factura y
  la diferencia con el contravalor va a **768** (diferencias positivas de cambio) o **668**
  (negativas). También se puede pagar desde la cuenta en dólares una factura en euros: la
  diferencia también es de cambio.
- **Traspasos** entre la cuenta en euros y la de divisa: lleva el cargo de la cuenta en euros a la
  subcuenta de la cuenta en divisa (por ejemplo 5730001). El abono en dólares de unos días
  después queda conciliado con el mismo asiento y toma como contravalor **los euros reales** del
  traspaso (no el tipo del BCE). Vale igual empezando por el lado en dólares.
- «**Saldos de las cuentas**» (sale si tienes alguna en divisa) dice el saldo de cada cuenta en
  su moneda según el extracto, el de la contabilidad en euros y lo que valdría al último tipo del
  BCE descargado, y avisa de los movimientos que aún no tienen contravalor. La previsión de
  tesorería cuenta las cuentas en divisa por sus euros contables y lo dice en sus notas.
- Al cierre, los saldos en divisa se valoran al tipo del 31 de diciembre (ver «Saldos en moneda
  extranjera al cierre» en Cierre).

### Modelos

Los modelos calculados desde tu contabilidad, casilla a casilla, con sus avisos y el detalle por
cliente o proveedor: **303, 130, 111, 115, 123, 216, 349, 390, 202, 200**, **347, 190, 180,
193, 296 y 184**. Eliges el ejercicio y el periodo.

- **«Guardar fichero para la Sede…»**: el fichero oficial (diseño de registro) para presentarlo en
  la Sede con «Presentación mediante fichero»; el del 200, para Sociedades WEB.
- **«Prevalidar con la AEAT…»**: la AEAT comprueba el fichero sin presentarlo (y puede devolver el
  borrador oficial en PDF). Con tu certificado y Touch ID.
- **«Presentar en el entorno de pruebas…»**: presentación de prueba, sin efectos, con tu
  conformidad expresa y la huella del fichero. La presentación real llegará con la versión
  firmada.
- «Corregir una casilla a mano» cuando hace falta, y «Volver al automático».
- **«Preguntar a la IA sobre este borrador»**, junto al resultado: una conversación sobre ese
  modelo y periodo (ver [Asistente](#asistente)).
- **Pago**: apunta en la contabilidad el pago del modelo (eligiendo el cargo del banco). TaxPain no
  paga nada.
- **«Datos de los modelos»**: domiciliación, compensaciones, prorrata, criterio de caja, tipos y
  datos del Impuesto sobre Sociedades, y lo de cada ejercicio.
- **Meses (gran empresa, REDEME, SII).** Si el perfil fiscal tiene el IVA mensual o marcas el
  REDEME en «Datos de los modelos», el 303 se prepara por meses («Periodo»: enero…diciembre, con
  el fichero del mes «01»…«12»); con «Gran empresa», el 111, el 115 y el 123; con el 349 mensual,
  el 349. La compensación se encadena mes a mes; en el REDEME puedes pedir la devolución cada
  mes (art. 116 LIVA) y en los demás, en diciembre. Los trimestres siguen ahí como comprobación,
  con un aviso. El pago y el asiento de liquidación del IVA de un mes aún se hacen a mano.
- **SII.** Debajo del 303, TaxPain te dice si tienes que llevar los libros por el Suministro
  Inmediato de Información (IVA mensual, gran empresa, REDEME, o porque las facturas del año
  pasado superan 6.010.121,04 €) y, si lo llevas, hasta cuándo puedes enviar cada factura del
  último mes: cuatro días sin sábados, domingos ni festivos nacionales desde que se expide o se
  registra, y siempre antes del día 16 del mes siguiente (art. 69 bis RIVA). Avisa también si el
  año en curso ya supera el umbral. **TaxPain no genera aún los mensajes del SII ni envía nada.**
- 347: «Generar las cartas (PDF)…» para tus clientes y proveedores. 184: socios y datos de la
  entidad.
- **190, 180 y 193: «Certificados para los perceptores…»**. Quien retiene tiene que dar a cada
  perceptor un certificado de lo que le pagó y le retuvo, antes de que empiece la campaña de la
  Renta (art. 108.3 del Reglamento del IRPF). En el borrador de cada modelo eliges a quién
  (trabajadores, administradores y profesionales en el 190; arrendadores en el 180; socios que
  cobran dividendos en el 193, también la sociedad matriz), el lugar y la fecha de expedición y
  quién firma (propone el administrador de Nóminas). Cada certificado lleva tus datos y los del
  perceptor, el ejercicio y las cifras que el borrador declara de él: en el 190, por clave y
  subclave, las percepciones dinerarias y en especie, los ingresos a cuenta, los gastos
  deducibles (cotizaciones) y las retenciones; en el 180, las rentas, el tipo y la retención, y
  el inmueble (escríbelo: TaxPain aún no guarda la dirección ni la referencia catastral); en el
  193, cada dividendo con su fecha, el rendimiento íntegro, la base, el tipo y la retención (o
  «no sometido a retención» si es una sociedad con la exención). «Guardar los certificados…»
  deja un PDF por perceptor, uno con todos o los dos en la carpeta que elijas; «Preparar correo»
  abre en tu programa de correo un mensaje sin enviar con el certificado adjunto (al correo de
  su ficha de contacto, si lo tiene). TaxPain no registra retribuciones en especie ni
  reducciones: salen a cero. Queda anotado en la auditoría. Firma el PDF antes de entregarlo.
- **202: por la cuota o por la base.** Al elegir el 202, arriba del borrador sale «Modalidad del
  202» con:
  - **La modalidad del ejercicio.** Por defecto, la de la **cuota** (18 % de la cuota del último
    Impuesto sobre Sociedades, art. 40.2 LIS). Puedes elegir la de la **base** (art. 40.3 LIS):
    el 17 % (5/7 del tipo, redondeado hacia abajo; 16 % con el 23 % de 2026, 13 % y 15 % en las
    microempresas) de la base imponible de los 3, 9 u 11 primeros meses, menos las retenciones y
    lo ya pagado en el año. Se guarda por ejercicio y el borrador, el fichero y el calendario la
    usan. **Elegirla en TaxPain no la comunica a Hacienda**: hay que optar en el censo (modelo
    036) en febrero (en los dos primeros meses del ejercicio si no empieza el 1 de enero), y vale
    para ese ejercicio y los siguientes hasta que renuncies, también en ese plazo.
  - **Obligatoria con más de 6 millones.** Si la cifra de negocios del ejercicio anterior pasa
    de 6 millones (o lo marcas en «Datos de los modelos»), va por la base sin opción, y TaxPain te
    lo dice. Con 10 millones o más, el porcentaje es el 24 % y hay un pago mínimo del 23 % del
    resultado contable del periodo.
  - **El simulador**: los tres pagos del año por cada modalidad, con lo que hay (la contabilidad
    de los periodos ya acabados; para los que faltan, el presupuesto o los mismos meses del año
    anterior), el total y cuál sale más barato. Es tesorería: lo pagado de más o de menos se
    regulariza en el 200. Úsalo en enero o febrero para decidir.
  - En la modalidad de la base, la base sale del resultado contable del periodo con la
    corrección del Impuesto sobre Sociedades contabilizado y las bases negativas pendientes (con
    su límite). Lo que TaxPain no sabe (el 95 % exento de los dividendos de participadas, la
    amortización fiscal, bonificaciones…) lo pones en «Corregir una casilla a mano»: [07] y [08]
    (correcciones), [14] (bases negativas), [27] (bonificaciones), [28] (retenciones), [30]
    (pagos anteriores) o [44] (reserva de capitalización). Si hay dividendos, el borrador te
    avisa.
- **200: el Impuesto sobre Sociedades completo.** Debajo del borrador sale la liquidación por
  bloques. Cada cifra lleva su casilla, de dónde sale y la norma:
  1. **Resultado contable** y la corrección por el propio impuesto contabilizado (630, 633, 638).
  2. **Correcciones al resultado.** TaxPain propone las que ve en la contabilidad y en tus
     fichas: multas y sanciones (subcuentas que lo dicen en el nombre; el resto de la 678 se
     propone sin incluir), donativos y liberalidades, atenciones a clientes por encima del 1 % de
     la cifra de negocios, deterioros del inmovilizado, de participaciones y de valores de deuda
     (art. 13 LIS), provisiones no deducibles (sin incluir, para que confirmes), los deterioros
     de clientes y las provisiones no deducibles de los **ajustes de cierre** (ya incluidos, con
     origen «Ajustes de cierre», en [00321]/[00322] y [00337]/[00338]; sus asientos no vuelven a
     salir en las propuestas de la 678 o la 695, para no contarlos dos veces), diferencias de
     amortización de las fichas de inmovilizado, la limitación de gastos financieros (sólo si los
     netos pasan de un millón) y la **exención del 95 % de los dividendos de participadas** (art.
     21 LIS) desde su ficha. Cada una se puede **cambiar**, **descartar** o **volver a la
     propuesta**, y puedes **añadir a mano** cualquier otra del catálogo (operaciones vinculadas,
     libertad de amortización, plusvalías de participadas…). Lo que tecleaste antes en «Datos de
     los modelos» sigue saliendo.
  3. **Base imponible previa** ([00550]).
  4. **Bases negativas por año de origen.** Añade lo pendiente de cada año al empezar el
     ejercicio; se compensan de la más antigua a la más reciente con el límite del art. 26 (el
     70 % de la base o un millón; 50 % o 25 % para las grandes; sin límite en los tres primeros
     años con beneficio de una empresa nueva si lo marcas). Van en la página 15 del fichero.
  5. **Base imponible**, con la reserva de capitalización y la de nivelación.
  6. **Reservas.** La de capitalización (art. 25): el incremento de fondos propios sale del
     balance (desplegable con las cuentas que computan y las que no) y se puede corregir; el 15 %
     (10 % antes de 2025) hasta el 20 % o 25 % de la base. La de nivelación (art. 105): la
     minoración que pidas hasta el 10 % de la base, y las de hace cinco años se suman solas.
     TaxPain mira si la **aplicación del resultado** o un **acta** del año siguiente dota la
     reserva y te avisa si falta: sin dotarla se pierde la reducción.
  7. **Cuota**, con el tipo que propone TaxPain por la cifra de negocios del año anterior (Ley
     7/2024) y, si la empresa es **entidad patrimonial** según su test, el tipo general sin
     ventajas de empresa pequeña (con aviso). La deducción por doble imposición internacional sale
     de Valores y de los dividendos de participadas extranjeras sin exención; las bonificaciones
     y las demás deducciones (I+D+i, donaciones…) las tecleas, y su desglose se completa en
     Sociedades WEB.
  8. **Retenciones** desglosadas (facturas, capital mobiliario…) y los tres 202.
  9. **Resultado** a ingresar o a devolver.
  - **Participadas**: para cada una (se dan de alta en Operaciones vinculadas), el valor nominal
    y el fiscal, cómo se adquirió, sus fondos propios del año y los **dividendos cobrados** con
    la comprobación del art. 21 (5 % y un año; las extranjeras, que tributen al 10 %), lo exento,
    la retención que procede y avisos de retención indebida, posible deterioro o reparto por
    encima de sus reservas. Van en la página 02 del fichero.
  - **«Pasar a <año siguiente>»** guarda lo que queda pendiente (bases negativas, reservas,
    deducción por doble imposición) como punto de partida del ejercicio siguiente. Hazlo cuando
    el 200 esté presentado. Todo queda en la auditoría.
  - TaxPain no presenta el 200: importa el fichero en Sociedades WEB, completa lo que diga (el
    detalle por años de las reservas va en la página 20 bis) y revísalo antes de presentar.
- Autónomos: las casillas de la actividad para la Renta (modelo 100, incluidas las
  criptomonedas) y el **cotejo con los datos fiscales** de la AEAT.
- **«Comprobar lo presentado en la AEAT»**: baja lo presentado (también por tu gestoría) de uno o
  varios años, con justificantes y casillas, y dice lo que se presentó fuera de plazo o falta.
  Lo que vencía después de la última comprobación sale como «No consta», con una nota para
  volver a comprobar. Sólo lee.
- **«Preparar rectificativa…»** (303, 111, 115, 123, 130, 202 y 200). Si el periodo elegido ya
  está presentado (desde TaxPain, importado o tecleado en «Modelos presentados», o según la
  comprobación con la AEAT), encima del borrador sale cuándo se presentó, su justificante y su
  resultado, y el botón. TaxPain recalcula el periodo con tu contabilidad de hoy y enseña, casilla
  a casilla, lo presentado, lo que sale ahora y la diferencia, y el resultado:
  - **Si sale a ingresar más**: lo de más, y, si lo presentas después del plazo y sin que te lo
    pida Hacienda, el **recargo** del art. 27 LGT (1 % más un 1 % por cada mes completo de
    retraso; pasados doce meses, el 15 % y los intereses de demora), calculado con la **fecha
    prevista de presentación** que pongas. El recargo no va en el fichero: lo liquida después la
    AEAT (con un 25 % menos si lo pagas entero en su plazo).
  - **Si sale a devolver**: lo ingresado de más que se pide devolver y una estimación de los
    intereses de demora que te abonará la AEAT.
  - **Cauce**: en el **303 y el 200** es la *autoliquidación rectificativa* (art. 120.3 LGT y RD
    117/2024): sirve para pagar más y para que te devuelvan. En el **111, 115, 123, 130 y 202**
    los diseños de registro aún traen la *complementaria*, que sólo sirve para ingresar más; si
    sale menos, TaxPain te dice que pidas la rectificación en la Sede (no genera fichero).
  - Escribe el **número de justificante** de la anterior si TaxPain no lo tiene (13 cifras, en el
    justificante de presentación). En el 303 y el 200 puedes indicar lo que la AEAT ya te
    devolvió y pedir la baja de la domiciliación de la anterior.
  - «Guardar el fichero de la rectificativa…» (o de la complementaria) guarda el fichero con las
    marcas del diseño de registro (rectificativa o complementaria, justificante anterior, motivo,
    lo ingresado con la anterior en su casilla) y lo deja anotado en la auditoría con su huella.
    **No se presenta nada**: súbelo tú en la Sede. Revisa los avisos: algunos casos (el tipo de
    declaración de un 303 que queda a devolver, la casilla de la rectificación del 200) están
    pendientes de cotejar con la AEAT.
  - Desde **Recibidas**: al corregir una factura de un trimestre con el 303 (o el 111/115) ya
    presentado, sale «Preparar la rectificativa del 303 del 3T de 2026», que abre aquí ese
    periodo con la rectificativa preparada.

#### Modelos 720 y 721 (bienes y criptomonedas en el extranjero)

En **Modelos**, elige «720 · Bienes en el extranjero» o «721 · Monedas virtuales en el
extranjero» y el ejercicio. TaxPain prepara un **borrador** con un registro por bien a 31 de
diciembre:

- **720, cuentas**: las cuentas bancarias con IBAN de otro país (o las que marques «Cuenta en una
  entidad del extranjero»), con su saldo a 31/12 y el **saldo medio del último trimestre** (la
  media de los saldos de cada día del 1 de octubre al 31 de diciembre, de la contabilidad).
- **720, valores**: las posiciones de las cuentas de bróker marcadas como del extranjero
  (Inversiones), con su valor a 31/12; clave V para acciones y bonos e I para fondos y ETF. Los
  inmuebles y los seguros no están: si los tienes, añádelos tú en el formulario.
- **721**: las monedas de las plataformas del extranjero (Criptomonedas), en unidades y en euros
  con los precios de cierre.

Para cada bloque te dice si **hay que presentar**: la primera vez, si pasa de 50.000 € (en las
cuentas basta con que lo pase el saldo a 31/12 o el saldo medio); los años siguientes, sólo si
sube **más de 20.000 €** respecto del último declarado o si has dejado de tener algún bien que
declaraste (se declara con origen C). Para eso necesita lo declarado: pulsa **«Ya lo he
presentado»** cuando lo presentes (pide confirmación y queda en la auditoría) o, si lo presentaste
antes de usar TaxPain, tecléalo en **«Lo declarado otros años»** (valor del bloque, saldo medio y
los IBAN, ISIN o monedas). Si la AEAT dice que presentaste uno y faltan los importes, lo avisa.

En **«Datos de los bienes»** completa lo que la contabilidad no sabe: tu condición (titular,
autorizado…), el porcentaje, la fecha de apertura (si no, se usa el primer movimiento), la fecha
de cancelación, y la entidad o el custodio con su NIF, BIC, domicilio y país. La columna «Falta»
de cada registro dice qué queda.

**No se genera el fichero**: TaxPain no tiene el diseño de registro del 720 (Orden HAP/72/2013)
ni del 721 (Orden HFP/886/2023), y la pantalla lo dice. «Guardar hoja de registros (Excel)…» te
da un resumen y una fila por registro para pasarlos al formulario de la Sede. Una **sociedad** no
presenta los bienes que tiene contabilizados de forma individualizada: el borrador lo indica y
lista lo que falta para cumplirlo (subcuenta propia, entidad, NIF y país), con una nota para los
años en que llevaba la contabilidad la gestoría.

#### Inmuebles arrendados (115 y 180)

En **Modelos → 180** (y en el 115) está **«Inmuebles arrendados»**: el inmueble de cada
arrendador con su **situación** (1 con referencia catastral en territorio común, 2 País Vasco, 3
Navarra, 4 sin referencia), la **referencia catastral** (20 caracteres), la vía, el número, el
municipio, el código de provincia y el código postal. Los arrendadores del 180 que no tienen
inmueble salen con «Añadir su inmueble».

- Con el inmueble de **todos** los arrendadores guardado, el 180 genera el **fichero** para la
  Sede (antes había que presentarlo en el formulario). Si falta alguno, el borrador lo dice.
- El **certificado de retenciones** del 180 lleva la dirección y la referencia catastral
  guardadas; lo que escribas al generar sigue mandando.
- **Sin retención**: si pagas un alquiler marcado como tal sin retención y pasa de 900 € al año,
  el 115 avisa salvo que apuntes hasta cuándo vale la **acreditación del arrendador** para no
  retener (art. 75.3.g del Reglamento del IRPF).
- Un arrendador, un inmueble: si alquilas dos inmuebles al mismo arrendador, reparte las rentas
  en el formulario.

#### Rentas pagadas a no residentes (216 y 296)

En **Modelos → 216** (trimestral) y **296** (resumen anual) están los pagos a quien no reside en
España y no tiene aquí establecimiento permanente:

- **Dividendos a socios no residentes**: salen solos de Socios → Pagar dividendos, con la
  retención del 19 % que se practicó.
- **Intereses, cánones (licencias, software con cesión de derechos), servicios sujetos y otras
  rentas**: apúntalos con **«Apuntar una renta»**, o desde la lista de **facturas de proveedores
  de otro país** («Apuntar como renta»). La mayoría de esas facturas no llevan retención (compras
  de bienes, servicios de una empresa de un país con convenio): apunta sólo las sujetas.
- **«Calcular el tipo»** propone el tipo antes de apuntar: el general (24 %, o 19 % si reside en
  la UE o el EEE; 19 % en dividendos e intereses), el del **convenio** si hay un **certificado de
  residencia** vigente (de la tabla de TaxPain, que sólo tiene unos pocos países y conviene
  comprobar, o el que teclees), o la **exención** (intereses a residentes en la UE, dividendos a
  la matriz de la UE, cánones a una sociedad asociada de la UE) si la pides y se cumplen el país
  y el certificado. Apuntar pide confirmación y no hace asientos: la retención se contabiliza con
  el pago.
- **Certificados de residencia fiscal**: perceptor, país, fecha de expedición y validez (un año si
  no la pones). La pantalla marca los que caducan pronto y los caducados; una renta posterior a la
  caducidad va al tipo general.
- El **216** del trimestre sale con el número de rentas, la base y las retenciones (con y sin
  retención) y el **296** con un registro por perceptor, naturaleza y tipo, cuadrado con los 216.
  **No se genera el fichero** (TaxPain no tiene su diseño de registro): teclea las casillas en el
  formulario de la Sede. Del 296 salen los **certificados de retenciones** para cada perceptor.
- Marca en el perfil fiscal «Pago cánones, intereses o dividendos a no residentes» para que el
  calendario te avise del 216 y del 296.

### Criptomonedas

- **Operaciones** de tus plataformas y monederos: compras, ventas, permutas, traspasos,
  comisiones, recompensas, pérdidas, pagos y cobros. A mano o con «Importar fichero…»
  (Trade List de CoinTracking, ledgers de Kraken o una hoja de operaciones o de saldos por fecha,
  también dentro de un zip). Los de la carpeta de la empresa llegan aquí desde Importar.
- **Posiciones** y **resultados realizados** por precio medio ponderado y por FIFO (el de la Renta
  para las personas físicas), con los precios de cierre. Eliges el método en «Método»; si eres
  persona física, la Renta y los asientos usan FIFO aunque elijas precio medio.
- **Punto de partida.** Si faltan registros de años antiguos (plataformas que cerraron, DeFi), el
  cálculo puede empezar en lo que calculó tu gestoría: una fecha de corte (el último día que no
  cuenta, normalmente el cierre del último ejercicio declarado) y, por moneda, la cantidad y el
  coste pendiente. Desde el día siguiente se sigue con tu método; las operaciones de antes quedan
  como historial (se ven, pero no cuentan ni se contabilizan).
  - **«Usar los cálculos de la gestoría»** propone el punto de partida con lo que hay en TaxPain
    (el diario de la gestoría, la última hoja de saldos importada y el 200). Compruébalo y guarda.
    **«Ponerlo a mano»** lo deja escribir; **«Añadir el polvo»** suma las monedas de menos de 1 €.
  - La «Cuenta del diario» es donde la gestoría tiene el coste: las ventas posteriores se
    contabilizan contra ella (vacía, una subcuenta por moneda).
  - Si faltan operaciones (se vende o se traspasa más de lo que había) y no hay punto de partida,
    la pantalla lo avisa. «Cambiar» y «Quitar el punto de partida» lo corrigen o lo quitan.
- Asientos de las operaciones y deterioro al cierre.
- **Modelo 721**: avisa si superas los 50.000 € en el extranjero.
- Exportar a Excel o PDF.

### Operaciones vinculadas (modelo 232)

En **Contabilidad → Vinculadas** está todo lo que la sociedad ha hecho en el ejercicio con sus
personas y entidades vinculadas (art. 18 de la Ley del Impuesto sobre Sociedades), si eso obliga
a presentar el **modelo 232** y la **documentación** que hay que tener por si la pide Hacienda.
Nada se presenta desde TaxPain.

- **Quién es vinculado.** Sin teclear nada: los socios con el 25 % o más (de Socios y
  dividendos), los administradores (de Nóminas) y las participadas con el 25 % o más. Con
  «Añadir vinculada» o, en la ficha de un cliente o proveedor, «Marcar como vinculada», añades
  los demás: cónyuge o familiares hasta el tercer grado de un socio o administrador, entidades
  del grupo, entidades con los mismos socios… Marca «Del mismo grupo de consolidación fiscal» si
  lo es: sus operaciones no van al 232.
- **Participadas.** NIF, razón social, porcentaje, fecha de adquisición, coste y subcuenta de
  cada sociedad en la que tienes participación.
- **Operaciones del ejercicio.** Salen solas de las facturas emitidas y recibidas, las nóminas
  (la retribución del administrador por el cargo se ve, pero queda fuera del 232), los dividendos
  (sólo para la memoria) y las **subcuentas** que digas de quién son (la 551 de la socia, el
  préstamo a la participada, sus intereses…: elige la persona en «Subcuentas de préstamos,
  cuentas corrientes e intereses»). Los préstamos de Contabilidad → Préstamos con una vinculada
  entran solos: el principal entregado o recibido en el año (clave 5) y los intereses del año
  (clave 9); si además asignas sus subcuentas a esa persona, no se cuentan dos veces. Lo que TaxPain no ve (venta de un local, de participaciones,
  una cesión de marca) se añade en «Operaciones que TaxPain no ve», marcando si es una
  operación específica.
- **¿Toca el 232?** Arriba lo dice, con el motivo y el mes en que se presenta (con ejercicio
  natural, noviembre del año siguiente). Hay que presentarlo con más de 250.000 € con la misma
  persona o entidad, con operaciones específicas de un mismo tipo por más de 100.000 €, o con
  operaciones del mismo tipo y método que pasen del 50 % de la cifra de negocios. También avisa
  en el Panel.
- **Valoración y documentación.** En cada operación, «Valoración» guarda el método (precio libre
  comparable, coste incrementado…), la descripción y los comparables. Con eso y los dos textos
  generales, «Documentación (RTF)…» o «(PDF)…» prepara la documentación con contenido
  simplificado (sociedades con menos de 45 millones de cifra de negocios) y «Borrador del 232
  (Excel)…», una hoja con un registro por persona, tipo de operación, ingreso o pago y método,
  para teclearlo en el formulario de la Sede.
- La **memoria** (Cuentas anuales) rellena la nota de partes vinculadas con la retribución del
  administrador y estas operaciones por categoría, y cita cada préstamo con una vinculada (saldo
  vivo al cierre, tipo e intereses del año).

### Inversiones en valores

Acciones, ETF, fondos y opciones en tus brókeres: lo que tienes y lo que vale, lo que has ganado o
perdido al vender, los dividendos y sus retenciones en origen, el modelo 720 y los asientos. Arriba
se elige el ejercicio.

- **Cuentas y conectores.** «Añadir cuenta de Interactive Brokers», «Añadir cuenta de XTB» o
  «Añadir otra cuenta» (con las operaciones a mano). Marca «En el extranjero (cuenta para el
  720)» si la cuenta está fuera de España. Al importar un fichero, la cuenta se crea sola.
- **Interactive Brokers, por su API (Flex Web Service).** «Conectar por API…» explica cómo crear
  la Flex Query y el token en el Client Portal. Pega el token y el Query ID y pulsa **«Guardar y
  conectar»**: el token se guarda cifrado en tu Mac y no vuelve a verse. Después, **«Sincronizar
  {año}»** pide el extracto del año. «Desconectar» borra el token (lo importado se queda). También
  puedes importar el Flex en fichero (XML o CSV) sin conectar nada.
- **XTB, por su informe.** XTB no tiene API: exporta el historial de la cuenta en xStation 5, en
  **Excel** (un fichero por cuenta, una por divisa: importa todos) o en **PDF**, y pulsa
  «Importar informe de XTB (PDF o Excel)…». «Qué informe descargar en xStation 5» lo explica.
  Las compras de antes del periodo que no estén ya importadas se toman de las posiciones abiertas,
  con aviso.
- Antes de importar, «Lo leído» enseña qué ha salido de cada fichero, cuántas operaciones son
  nuevas y cuántas ya estaban (no se duplican). Confirma con **«Importar»**.
- **Pasar a euros.** Las operaciones en otra divisa sin su importe en euros se pasan con **«Pasar a
  euros con el BCE»**, que descarga los tipos de referencia de esas fechas (sólo se envían la
  divisa y las fechas).
- **Posiciones** a la fecha de cierre, con su coste, el precio de cierre y la plusvalía latente.
  Los precios de cierre de IBKR llegan al sincronizar y los de XTB con el informe de ese día; si
  falta alguno, escríbelo en «Precios de cierre».
- **Resultado del año**: ganancias y pérdidas en ventas, derivados (opciones), dividendos,
  retenciones e intereses. En una sociedad, con el **coste medio ponderado** de cada valor; en una
  persona física, por **FIFO**. «Ver cada venta» da el detalle.
- **Opciones**: se tratan como derivados. La prima pagada es una compra y el vencimiento o el
  cierre, una venta. Van en su propia línea y no cuentan para el 720.
- **Dividendos y retenciones en origen**, por país: lo retenido, lo que permite el convenio, lo
  deducible y el exceso que podrías reclamar en el país de origen.
- **Modelo 720**: el total en el extranjero frente al umbral, con un aviso si lo superas.
- **Asientos** (sólo sociedades): los de las compras, las ventas con su resultado (766/666 a
  coste, 7630/6630 en negociación), los dividendos íntegros, las retenciones (la española a la
  473, que el 200 resta; la de fuera, gasto por impuesto, 6300, que da la deducción por doble
  imposición), los intereses y las comisiones. **«Contabilizar»** (con confirmación) los graba en
  el diario como un lote: «Rehacer» los borra y los vuelve a generar con los datos de ahora y
  «Deshacer» los quita. Mientras una operación tenga asiento no se puede cambiar ni borrar. El
  ajuste del cierre se graba en Cierre («Cartera de valores al cierre»). En «Cómo se calcula»
  eliges la cartera (a coste o mantenidos para negociar) y el plazo (corto o largo); con asientos
  grabados no se puede cambiar: deshazlos antes.
- «Nueva operación» para añadir una a mano, y «Exportar a Excel…» o «Exportar a PDF…».

Sólo se conecta a internet para pedir tus extractos a Interactive Brokers cuando pulsas
«Sincronizar» y para los tipos del BCE cuando lo confirmas.

### Notificaciones

- «Consultar en la AEAT» trae la lista de notificaciones y comunicaciones de tu buzón (con tu
  certificado y Touch ID), con la cuenta atrás de los diez días para abrirlas.
- **«Abrir notificación…» es la comparecencia real: tiene efectos legales** (empiezan los plazos).
  TaxPain te lo pide marcar antes. Después: descargar, guardar el PDF y el justificante.
- Los «Días de cortesía» que marcaste en la Sede, como recordatorio.
- TaxPain **nunca consulta ni abre notificaciones solo**.
- Enlaces a la Sede: datos censales, deudas y certificado de estar al corriente.

#### Expedientes y plazos

Debajo del buzón, **Expedientes y plazos** lleva lo que te ha notificado Hacienda (u otro
organismo) para no perder ningún plazo:

- **Crear un expediente**: con «Crear expediente…» en una notificación del buzón, «desde
  Documentos AEAT» (un requerimiento o una liquidación que hayas importado) o tecleado a mano.
  Sirve también para lo que llega por la **DEHú, la Seguridad Social, la comunidad autónoma o el
  ayuntamiento**: elige el organismo. Tipos: requerimiento, propuesta de liquidación, trámite de
  audiencia, propuesta de sanción, liquidación, acuerdo sancionador, providencia de apremio y
  diligencia de embargo.
- **Plazos**: con la fecha de notificación (o la de puesta a disposición, si aún no la has
  abierto: se da por notificada a los diez días naturales) TaxPain cuenta cada plazo como dice la
  ley: los de días son **hábiles** (sin sábados, domingos ni los festivos nacionales, de tu
  comunidad y los locales de tu perfil) y los de meses van **de fecha a fecha**, y si el último día
  es inhábil pasa al siguiente hábil. Contestar un requerimiento (10 días si el documento no dice
  otro), pedir la ampliación (como tarde tres días antes), alegaciones (10, o 15 a una propuesta
  de sanción), recurso de reposición o reclamación económico-administrativa (un mes), y el pago en
  periodo voluntario o de la providencia de apremio. Si el documento da otro plazo, tecléalo.
- **Cuenta atrás y avisos**: el expediente enseña los días que quedan; el Panel avisa de los
  plazos de los próximos 15 días, el Calendario los lista y el Mac avisa a 7, 3 y 1 días y el
  último día. Anota lo que hagas (contestación, recurso, pago con su número de registro) y el
  expediente pasa a contestado, recurrido o cerrado y deja de avisar.
- **Lo que costaría pagar**: con el importe, el recargo del periodo ejecutivo (5 % antes de la
  providencia de apremio, 10 % en su plazo y 20 % con intereses de demora después) y, en una
  sanción, lo que quedaría con la reducción por conformidad (30 %), por pronto pago (40 %) o con
  las dos.
- **Escritos**: «Preparar un escrito» guarda en RTF (para Pages o Word) o PDF un borrador de
  contestación con la relación de documentos que aportas, alegaciones, recurso de reposición,
  solicitud de aplazamiento o fraccionamiento (con su motivo y los plazos) o de ampliación del
  plazo, con los datos del expediente y de la empresa. Lo que tienes que escribir va entre
  corchetes. **TaxPain no presenta ni envía nada**: se presenta en la sede del organismo. Son
  modelos generales: en una inspección o una sanción, consulta a un asesor.
- **Certificados y apoderamientos que caducan**: anota (o lee de Documentos AEAT) el certificado
  de estar al corriente, el de residencia fiscal, el de la Seguridad Social o un apoderamiento con
  su fecha de caducidad; el Panel y el Mac avisan 15 días antes (30 los apoderamientos). El
  certificado electrónico ya avisa por su lado.
- TaxPain no vigila la DEHú ni otras sedes por su cuenta (no tienen un servicio para
  aplicaciones): mira esos buzones y anota aquí lo que encuentres.

---

### Archivo y auditoría

- **Archivo del ejercicio.** Elige el ejercicio y mira la lista de lo que hay y lo que falta
  (diario y mayor, libros de IVA, facturas con PDF, recibidas con su original, modelos con
  justificante, cuentas anuales, actas, movimientos del banco). «Generar el archivo…» guarda un
  ZIP con todo en formatos abiertos (CSV, PDF, XLSX, Facturae), un índice (`00_INDICE.txt` e
  `indice.csv`) con la huella SHA-256 de cada fichero, y al lado un `.sha256` con la del ZIP
  entero. Queda en la auditoría con su huella. El ZIP **no va cifrado**: se avisa y se pide
  Touch ID; guárdalo en un disco cifrado. Los extractos originales del banco no están en TaxPain:
  consérvalos aparte.
- **Hasta cuándo conservar.** Para cada ejercicio, la fecha hasta la que hay que guardarlo: seis
  años desde el cierre (art. 30 del Código de Comercio), cuatro años de prescripción fiscal desde
  el fin del plazo del Impuesto sobre Sociedades (o de la Renta), y diez años o más si generó
  bases negativas o deducciones que siguen pendientes. Un requerimiento o una comprobación
  alargan el plazo: tenlo en cuenta. El Panel avisa de qué ejercicios ya se podrían destruir y
  cuáles no tienen archivo. **TaxPain no borra nada.**
- **Exportación completa.** Todos los datos de la empresa en un ZIP: cada tabla en CSV y JSON y
  los documentos tal cual, con un `LEEME.txt`. Sirve para un asesor, un auditor o para seguir sin
  TaxPain. No lleva las claves de los certificados ni de los servicios conectados. Pide Touch ID.
- **Periodos cerrados.** Cierra el trimestre cuando hayas presentado su 303 (TaxPain te sugiere
  los que ya lo tienen) y el ejercicio cuando esté cerrado. Si después vas a emitir una factura,
  registrar una recibida o crear, cambiar o borrar un asiento con fecha de un periodo cerrado,
  sale un aviso: se puede seguir (lo declarado se corrige con una rectificativa o una
  complementaria), pero hay que confirmarlo, y queda en la auditoría. «Reabrir» pide Touch ID.
- **Registro de auditoría.** Todo lo que se ha hecho con efecto, de lo más reciente a lo más
  antiguo, con filtros por fechas, tipo de acción, quién y texto. Pulsa una entrada para ver el
  detalle y su huella. «Verificar la cadena de huellas» comprueba que ninguna entrada se ha
  cambiado ni borrado.

## Ayuda

### Asistente

Un asistente de IA opcional que explica y comprueba con tus datos: Claude, OpenAI o Mistral (con
tu propia clave, guardada cifrada) o un **modelo local** (Ollama, LM Studio), con el que nada sale
de tu Mac.

- Con un proveedor externo, **tus datos salen anonimizados**: nombre, NIF, domicilio, cuentas,
  correos, teléfonos, los terceros que nombran los avisos y los hallazgos de TaxPain y los nombres
  que detecta se sustituyen por marcadores; lo que se ha tapado en una pregunta se sigue tapando en
  las siguientes de la conversación (aunque el dato ya no esté en TaxPain, como el nombre de antes de
  la empresa). «Ver qué se envía» enseña el texto exacto (se cierra si cambias la pregunta o lo
  añadido, al llegar la respuesta y al borrar la conversación: vuelve a abrirlo). El certificado,
  las contraseñas y las claves no se envían nunca.
- Lo que aceptas vale para el proveedor que has elegido: si cambias a otro, hay que volver a
  aceptarlo.
- Consulta (sólo leyendo) tus plazos, el borrador de un modelo, tus facturas, el saldo de una
  cuenta y el **BOE**; marca las citas que no ha podido comprobar.
- **Preguntar a la IA sobre un tema.** En cada tarjeta de «Ahorrar impuestos» y en el resumen del
  año, en cada hallazgo de «Qué revisar» (Evolución) y de la revisión de la carpeta (Importar), y
  en el borrador de un modelo, «Preguntar a la IA» abre un panel con lo que TaxPain sabe de ese
  tema y preguntas para empezar. No se envía nada hasta que preguntas; la IA recibe los datos del
  tema recalculados en cada pregunta (los de la carpeta, de su última revisión). De un hallazgo no
  van las rutas de sus ficheros y los terceros que nombra van con marcadores (una revisión de la
  carpeta guardada con una versión anterior hay que repetirla antes de preguntar; si deja de salir
  antes de la primera pregunta, va su código y no su título); de un borrador, ni la relación de
  clientes y proveedores ni el fichero, y los que nombran sus avisos van con marcadores (en la
  respuesta, con su nombre). Si cierras el panel mientras piensa y la pregunta falla, al volver a
  abrirlo la tienes en el cuadro con el error; si la respuesta llega con el panel cerrado, el botón
  pasa a «Seguir con la IA». Si no se puede conectar con el proveedor o no responde en tres minutos,
  lo dice y la pregunta vuelve al cuadro (con un modelo local, que lo arranques). Las respuestas se
  ven con su formato (negritas, listas y títulos).
- **Tus datos y documentos.** «Añadir un dato» y «Añadir un documento…» (PDF, foto, Excel, CSV o
  texto: se lee en tu Mac y va su texto, anonimizado; el fichero no sale). Van con cada pregunta
  hasta que los quitas; TaxPain no los comprueba. De una hoja de cálculo se leen 5 hojas y 300
  filas de cada una; si queda algo fuera (o pasa de 15.000 caracteres), sale «recortado: no va
  entero», el texto dice qué falta y la IA lo sabe. Un certificado o una clave no se añade nunca,
  aunque tenga otra extensión o vaya en base64 (tampoco escrito como un dato o en la pregunta).
- **Se guardan** cifradas con la empresa y en sus copias. El botón dice «Seguir con la IA»; en
  Asistente → «Conversaciones guardadas» están todas, cada una con su tema (General, Ahorrar
  impuestos, Tu plan de ahorro, Qué revisar, Revisión de la carpeta o Modelos, con el año), cuántas
  preguntas lleva y cuándo se usó. «Nueva conversación» empieza una general. Se borran una a una o
  con «Borrar todas» (la que espera su respuesta, no);
  la lista recoge las respuestas que llegan mientras la miras y lo que se termina de añadir con el
  panel ya cerrado. Se envían las últimas 30 intervenciones y siempre lo que has añadido.
- No es asesoramiento fiscal y no presenta nada.

### Ajustes

- **Certificado**: importar, ver sus datos y eliminarlo.
- **Facturación**: emisor, series, idioma, IVA y retención por defecto, actividad, pie, qué
  ficheros se generan (PDF, Facturae, UBL), la carpeta de copia (iCloud Drive, Google Drive,
  Dropbox u otra) y **VERI\*FACTU** (en pruebas y desactivado por defecto; pronto estará
  listo).
- **Copias de seguridad**: ver [abajo](#varias-empresas-copias-y-avisos).
- **Avisos de plazos**: «Abrir TaxPain al iniciar sesión, sin ventana», para que los avisos
  lleguen aunque no la abras.
- **Claude (MCP)**: el acceso de Claude Desktop o Claude Code a esta empresa (ver
  [abajo](#claude-mcp)).
- **Pruebas VERI\*FACTU**: seis pasos en una empresa de pruebas aparte para enviar, anular y
  consultar registros en el entorno de pruebas de la AEAT y guardar un informe.
- **Actualizaciones**: buscar e instalar (sólo si la compilación lleva la clave pública con la
  que se firman).
- **Acerca de TaxPain**: la versión y la **declaración responsable** del programa de facturación
  (quién lo hace y que cumple el reglamento VERI\*FACTU), que la ley obliga a enseñar dentro del
  propio programa. «Guardar en PDF…» la guarda para firmarla; lo que aún falta (la dirección del
  productor, la fecha y el lugar de la firma) sale entre corchetes.

### Claude (MCP)

Si usas **Claude Desktop** o **Claude Code**, puedes dejar que Claude consulte TaxPain desde tu Mac
mientras hablas con él: tus plazos, el panel, facturas, cobros pendientes, el banco, el mayor, los
modelos, las criptomonedas y los presupuestos. Lo hace un programa aparte, `taxpain-mcp`, que
arranca tu cliente de Claude; no abre ningún puerto.

- **Viene apagado.** Se activa empresa por empresa en Ajustes → **Claude (MCP)** → «Dejar que
  Claude consulte esta empresa». Claude no ve las empresas que no lo tienen activado.
- **Dos niveles** («Hasta dónde llega»):
  - **«Sólo consultar»**.
  - **«Consultar y dejar borradores»**: además puede dejar un presupuesto o una proforma en
    borrador y añadir una factura a Recibidas, pendiente de revisar. Nada se emite ni se
    contabiliza sin ti.
- **No puede** emitir, presentar, firmar, contabilizar ni borrar nada, ni usar tu certificado.
- **Va sin anonimizar.** A diferencia del asistente de TaxPain, Claude ve los nombres, NIF, IBAN y
  conceptos tal como están, y lo que lee pasa por tu cliente de Claude hacia el proveedor del
  modelo (Anthropic, según las condiciones de tu cuenta).
- Cada consulta queda en la auditoría de la empresa (qué herramienta, no lo que devuelve).
- Los textos de facturas, movimientos y contactos los escriben terceros: revisa siempre lo que
  Claude te proponga hacer con ellos.
- Para cortarlo, apaga el interruptor: vale desde la siguiente consulta.

**Todavía no viene dentro de la app**: hay que compilar `taxpain-mcp` desde el código y añadirlo a
la configuración de Claude. «Cómo conectarlo», en la misma pantalla, da la configuración con la ruta
del programa; los pasos completos están en [`CLAUDE_CODE.md`](CLAUDE_CODE.md#servidor-mcp-que-claude-consulte-taxpain).

---

## Varias empresas, copias y avisos

- **Varias empresas.** El selector de la barra lateral cambia de empresa; cada una tiene sus
  datos aparte. «Añadir empresa…» crea otra; «Modificar o borrar empresas…» las cambia o las
  manda a la Papelera (confirmando con su nombre o NIF).
- **Copias de seguridad cifradas** con tu contraseña: «Hacer copia ahora» (se comprueba al
  guardarla) y automáticas cada día, semana o mes, guardando las últimas que elijas en la carpeta
  que quieras. «Cambiar la contraseña» pide Touch ID.
- **Restaurar una copia** («Elegir copia…», su contraseña y «Restaurar como empresa nueva») la abre
  como empresa nueva; la que tenías abierta no cambia. Después:
  - la empresa restaurada te pide **una contraseña de copias nueva** (no hereda la de la copia);
  - vuelve a importar el certificado, que no va en las copias;
  - las rutas del otro Mac se vacían: las carpetas vigiladas, la carpeta de la empresa, la carpeta
    donde se guarda una copia de cada factura y la de las copias automáticas. Elígelas otra vez;
  - antes de emitir, un aviso enseña la última factura de cada serie: comprueba que no emitiste
    ninguna después de la copia en el otro Mac y pulsa «Son las últimas: ya puedo emitir».
- **Una empresa que no abre** (datos dañados, falta la clave en el Llavero, la actualización de
  los datos ha fallado, son de una versión más nueva): sale en el selector como «no abre» y las
  demás siguen funcionando. Al elegirla, una pantalla explica qué ha pasado y ofrece **abrir la
  última copia** (se restaura como empresa nueva; la dañada se queda como está) o **exportar lo
  que se pueda** (sus ficheros tal cual y, si el Llavero tiene la clave, sus tablas en CSV). Si no
  abre ninguna empresa, la misma pantalla sale al arrancar. TaxPain no cambia ni borra nada de la
  empresa que no abre.
- **Comprobación de integridad.** Al abrir una empresa, TaxPain comprueba que su base de datos
  está sana (cifrado de cada página y estructura). Si encuentra daños, avisa arriba: haz una
  exportación completa y restaura la última copia.
- **Avisos del sistema** (notificaciones del Mac): los plazos a 15, 7, 2 y 0 días y las
  notificaciones de la AEAT ya consultadas que siguen sin abrir. Se revisan cada media hora, de
  todas las empresas. Cerrar la ventana deja TaxPain en el Dock; `⌘Q` la cierra.

## Qué sale de tu Mac

Nada sale sin que lo pidas. Cuando lo pides, sólo a estos sitios (detalle en
[`PRIVACIDAD_APP.md`](PRIVACIDAD_APP.md)):

| A dónde | Qué | Cuándo |
|---|---|---|
| AEAT | Modelos, registros VERI\*FACTU, consultas y notificaciones, con tu certificado | Cuando lo pides y confirmas con Touch ID |
| Proveedor de IA que elijas | Tu pregunta, los datos del tema y lo que añades (también el texto de tus documentos), con un resumen, anonimizados (o, con «Full IA», los ficheros completos) | Si activas el asistente y preguntas |
| BOE | Palabras de búsqueda de una norma | Cuando el asistente consulta la ley |
| Comisión Europea (VIES) | El NIF-IVA de un cliente de la UE | «Comprobar en VIES» |
| AEAT, página pública de la Sede | El código CSV del documento que bajas | «Descargar de la AEAT por CSV» sin certificado |
| Banco Central Europeo | El código de una moneda y unas fechas | «Tipo del BCE» y «Pasar a euros con el BCE» |
| Enable Banking y tu banco | Las peticiones de cuentas, saldos y movimientos | Si conectas el banco y actualizas |
| Interactive Brokers | Tu token del Flex Web Service, el Query ID y las fechas | Si lo conectas y pulsas «Sincronizar» |
| Claude, por el servidor MCP | Lo que Claude pida para responderte, **sin anonimizar** | Sólo en las empresas en que lo activas, cuando preguntas a Claude |
| Servidor de actualizaciones | La petición de la versión disponible | Al buscar actualizaciones |

## Preguntas frecuentes

**¿Puedo presentar los modelos de verdad?** Todavía no desde TaxPain: genera el fichero oficial
para presentarlo en la Sede (o en Sociedades WEB, el 200), lo prevalida con la AEAT y presenta en
su entorno de pruebas. La presentación real llegará con la versión firmada.

**¿Sustituye a mi gestoría?** Hace el trabajo de cálculo, registro y control, y te dice qué
revisar; para decisiones con consecuencias (reservas, inversiones, operaciones societarias),
consulta con un asesor.

**¿Qué pasa si pierdo el Mac?** Tus datos están cifrados. Con una copia de seguridad y tu
contraseña (o el código de recuperación) los recuperas en otro Mac.

**¿Puedo preguntarle a Claude por mis datos?** Sí, con el asistente de TaxPain (anonimizado) o, si
usas Claude Desktop o Claude Code, con el [servidor MCP](#claude-mcp), que va sin anonimizar, se
activa empresa por empresa y todavía hay que compilarlo aparte.

**¿Y si una empresa no es de BBVA ni de Revolut?** Importa sus extractos en fichero (Norma 43,
Excel, CSV, PDF…): sirven los de cualquier banco.

**¿Funciona en Windows o Linux?** La app es para Mac. El núcleo (cálculos, contabilidad,
modelos) compila y pasa sus pruebas también en Linux.

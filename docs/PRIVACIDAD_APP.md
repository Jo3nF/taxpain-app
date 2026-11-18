# Qué sale de tu ordenador y a quién (app TaxPain)

Borrador para la información de protección de datos de la app y para el registro de
actividades de tratamiento (RGPD art. 30). Se ha hecho a partir del código (hosts y
comandos que existen hoy). **Debe revisarlo un abogado antes de publicarlo**
en la web o en la app. La idea viene de `docs/proteccion-de-datos.md` y
`apps/desktop/security-protocol.md` de reforma-digital (MIT; ver
`docs/research/REFORMA_DIGITAL.md`).

## 1. Principio

TaxPain es local: los datos fiscales y contables, los documentos y el certificado se guardan
cifrados en el Mac (SQLCipher con la clave en el Llavero). Quien distribuye TaxPain **no recibe**
esos datos: no hay servidor de TaxPain que los guarde ni telemetría.

## 2. Qué sale, cuándo y a quién

| Destino | Qué se envía | Cuándo | Base / control |
|---|---|---|---|
| AEAT (`www1`/`www2.agenciatributaria.gob.es`, `sede.agenciatributaria.gob.es`; pruebas en `prewww1`/`prewww2.aeat.es`) | Modelos (fichero BOE), registros VERI\*FACTU, consultas de declaraciones y notificaciones, identificándote con tu certificado (mTLS) | Sólo cuando tú lo pides y confirmas (Touch ID en cada uso del certificado) | Obligación legal del contribuyente; la AEAT es responsable de lo que recibe. TaxPain no presenta nada sin tu confirmación |
| AEAT, cotejo público de documentos (`www2.agenciatributaria.gob.es`, **sin certificado**) | El código seguro de verificación (CSV) del documento que quieres bajar y, para leer las casillas de la declaración, también el NIF del declarante. Quien tiene el CSV puede ver el documento: TaxPain lo guarda sólo en el almacén cifrado y no lo escribe en el registro | Sólo cuando pulsas «Descargar» en Importar → «Descargar de la AEAT por CSV» sin certificado (con certificado va por el servicio de la fila de arriba) | Consulta de tus propios documentos en la página pública de cotejo de la Sede; lo descargado queda en la auditoría |
| Proveedor de IA que elijas (Anthropic, OpenAI o Mistral, y ningún otro; o un modelo local en tu Mac, que no sale del equipo) | Tu pregunta y un resumen de tu situación (importes, plazos, avisos), o el texto de una factura recibida si pides que la lea, o el de un documento de la empresa (escrituras, tarjeta NIF…) si pulsas «Completar con el asistente» en él (va anonimizado como el resto y los datos vuelven como marcadores que se restauran en tu Mac); en una conversación sobre un tema, los datos de ese tema que calcula TaxPain (p. ej. una recomendación de «Ahorrar impuestos» con su motivo, ahorro y pasos; un hallazgo de una revisión, sin las rutas ni los nombres de sus ficheros (si deja de salir antes de la primera pregunta, sólo su código, no su título); o el borrador de un modelo con su resultado, casillas, avisos y cuántos perceptores o terceros lleva, nunca la relación de clientes y proveedores, el fichero del modelo ni el CSV de lo presentado; los terceros que nombra un aviso o un hallazgo van con marcadores), los datos que escribes y el texto de los documentos que añades, leído en tu Mac (no el fichero ni su nombre), y las últimas intervenciones de la conversación. **Antes se sustituyen** tu nombre, NIF, domicilio, cuentas, NIF de terceros, los nombres, NIF y NIF-IVA de los terceros que nombran los avisos y los hallazgos de TaxPain (un perceptor del 190, un viajero del 111, un cliente de la UE que falta en el 349…), emails, teléfonos, NUSS y tarjetas por marcadores, y también los nombres de persona y las direcciones que encuentra un detector que corre en tu Mac (no los encuentra todos; lo que tapa en una parte se tapa en todo el envío, también en lo que devuelven las consultas, y lo que se ha tapado en una pregunta se tapa en las siguientes de la conversación aunque ya no esté en los datos); nunca el certificado, contraseñas ni claves (un certificado o una clave no se añade a una conversación aunque tenga otra extensión o vaya en base64, ni como documento ni escrito) | Sólo si activas el asistente, aceptas qué se envía (otra vez si cambias de proveedor) y preguntas | Tu consentimiento; el proveedor actúa según sus condiciones (contratadas por ti con tu clave). Ver aviso del art. 50.1 del Reglamento de IA en la pestaña |
| El mismo proveedor de IA, con «Full IA» activada (Importar → carpeta de la empresa) | **Ficheros completos y sin anonimizar**: cada PDF o imagen de la carpeta que las reglas no entienden, tal cual (con los nombres, NIF e importes que lleve), acompañado del nombre y el NIF de tu empresa y del nombre del fichero (que suele llevar el del cliente o el proveedor). Las hojas de cálculo van como texto, anonimizado como el resto del asistente. Nunca el certificado, contraseñas ni claves | Sólo si activas «Full IA» aceptando el aviso que lo explica, y al pulsar «Leer con IA»; cada fichero una vez (la lectura se guarda) | Tu consentimiento expreso; cada envío queda en la auditoría (`assistant.file`) con la huella SHA-256 del fichero, para saber qué documentos salieron y cuándo. El proveedor actúa según sus condiciones (contratadas por ti con tu clave) |
| BOE (`www.boe.es`, API de datos abiertos) | Palabras de búsqueda de una norma (p. ej. «Impuesto sobre Sociedades») y el identificador y artículo que se lee | Sólo cuando el asistente consulta la legislación al responder | Datos públicos; TaxPain rechaza antes de enviarla cualquier búsqueda con marcadores o datos personales, y también con cinco o más cifras seguidas que no sean el identificador de una norma («BOE-A-2014-12328») |
| Comisión Europea, VIES (`ec.europa.eu`) | El NIF-IVA del cliente de la UE y el NIF de tu empresa (para que VIES dé número de consulta) | Sólo cuando pulsas «Comprobar el NIF-IVA en VIES» | Comprobación previa a facturar sin IVA español; el resultado queda en la ficha del cliente y en la auditoría |
| Banco Central Europeo (`data-api.ecb.europa.eu`) | El código de una moneda (p. ej. USD) y unas fechas; nada tuyo ni de tus clientes | Sólo cuando pulsas «Tipo del BCE» en una factura en moneda extranjera o «Pasar a euros con el BCE» en Inversiones (operaciones de valores en otra divisa) | Datos públicos (tipos de cambio de referencia); queda en la auditoría |
| GitHub (el `latest.json` de la última release del repositorio público de TaxPain y, si hay versión nueva y la aceptas, su paquete) | Petición de la versión disponible y, al actualizar, descarga del paquete (IP y versión de la app, como cualquier descarga; GitHub, Inc. es quien lo recibe) | Al comprobar actualizaciones | Interés legítimo (seguridad de la app). Los paquetes sólo se instalan si están firmados y tú lo confirmas |
| Enable Banking (`api.enablebanking.com`; Enable Banking Oy, proveedor de información de cuentas autorizado en Finlandia) y, a través de él, tu banco (BBVA, Revolut) | Al conectar: el banco, el tipo de cuenta (empresa o particular), la dirección de vuelta y un valor al azar. Después, las peticiones de cuentas, saldos y movimientos, firmadas con la clave de **tu** aplicación de Enable Banking. Enable Banking y el banco ven los datos de las cuentas que autorizas (pasan por ellos); TaxPain no envía nada de tu contabilidad | Sólo si configuras la conexión, la aceptas y autorizas el acceso en el banco; la primera descarga, al terminar de conectar, y las siguientes, cuando pulsas «Actualizar movimientos» | Tu consentimiento (PSD2, servicio de información de cuentas, sólo lectura, hasta 180 días; lo retiras en el banco o con «Quitar la conexión»). Queda en la auditoría (`banca.*`, `bank.import`) |
| Interactive Brokers (`ndcdyn.interactivebrokers.com`, Flex Web Service) | El token del Flex Web Service y el número (Query ID) de tu Flex Query, que has creado tú en el Client Portal, y las fechas del periodo; se recibe el extracto de tu cuenta de valores. El token se guarda en el almacén cifrado de la empresa y no vuelve a la interfaz ni sale en el registro | Sólo si guardas el token en Inversiones, y cada vez que pulsas «Sincronizar» | Tu consentimiento (el token es tuyo y sólo deja leer informes; lo revocas en el Client Portal y lo borras de TaxPain en Inversiones). Queda en la auditoría (`valores.ibkr.token`, `valores.ibkr.sincronizar`) |
| Claude por el servidor MCP (`taxpain-mcp`, un programa aparte que arranca **tu** cliente de Claude: Claude Desktop o Claude Code) y, a través de él, el proveedor del modelo que uses con ese cliente (Anthropic, con tu cuenta) | **Los datos que pida Claude para responderte, sin anonimizar**: facturas emitidas y recibidas, clientes y proveedores con su nombre y NIF, movimientos del banco con IBAN y concepto, el libro mayor y las sumas y saldos, los borradores de los modelos con su detalle de terceros, los modelos presentados, presupuestos y criptomonedas. Aquí **no** se sustituye nada por marcadores (a diferencia del asistente de la app). Nunca el certificado, contraseñas ni claves, ni los ficheros de los documentos | Sólo en las empresas en las que lo activas (Ajustes → Claude (MCP); apagado por defecto y empresa por empresa) y sólo cuando preguntas a Claude y él llama a una herramienta. TaxPain no envía nada por su cuenta | Tu consentimiento, que retiras apagando el interruptor (vale desde la siguiente consulta) o quitando el servidor de la configuración de Claude. No usa el certificado y no puede emitir, presentar, firmar ni borrar nada; con el nivel «con borradores» puede dejar un presupuesto en borrador o una factura recibida pendiente de revisar. Cada consulta queda en la auditoría (`mcp.call`: herramienta y empresa, no el contenido) y activarlo o cambiar de nivel, también (`mcp.ajustes`). El proveedor actúa según las condiciones de tu cuenta de Claude |
| Ninguno más | — | — | Bancos y brókeres (salvo las conexiones de arriba), Registro Mercantil, a3, Sociedades WEB, FACe…: TaxPain **genera ficheros** que subes tú |

**Lo que se guarda de las conversaciones.** Se guardan en el almacén cifrado de la empresa y van
en sus copias, con lo que se ha sustituido por marcadores en cada pregunta (para volver a taparlo
en las siguientes; no sale del Mac); se borran en Asistente (una o todas; las copias ya hechas las
conservan). En la
auditoría sólo queda `assistant.chat` (modelo, remoto, datos sustituidos, tipo de tema y cuántos
datos y documentos), nunca el contenido.

**El servidor MCP no anonimiza.** El asistente de la app sustituye nombres, NIF, IBAN y demás por
marcadores antes de enviar nada, y nunca envía la relación de clientes y proveedores. El servidor
MCP es otra cosa: Claude consulta los datos tal como están en TaxPain, con nombres y NIF, y lo que
lee pasa por el cliente de Claude que tengas configurado y por su proveedor. Por eso viene
apagado, se activa empresa por empresa y la sección de Ajustes lo dice antes de activarlo. Los
textos que devuelve (el nombre de un proveedor, el concepto de un movimiento, el nombre de un
fichero) los han escrito terceros: se les quitan los caracteres invisibles y van con el aviso de
que son datos y no instrucciones, pero revisa siempre lo que Claude te proponga hacer con ellos.
Cómo instalarlo y desactivarlo: `docs/CLAUDE_CODE.md` («Servidor MCP»).

## 3. Registro de actividades de tratamiento

Los datos de la app los trata **el propio usuario** en su equipo: quien distribuye TaxPain no
accede a ellos. Si publicas una web o un servicio de actualizaciones, apunta aquí lo que se trata
en ellos.

## 4. Antes de cambiar algo que toque datos (lista de comprobación)

- [ ] ¿Sale algo nuevo del Mac? Añádelo a la tabla del §2 y a la interfaz donde se pida.
- [ ] ¿Pasa por el asistente? Usa `prepare_with` (o `prepare_conversation`, en una conversación)
      con `known_values` y, si es texto ajeno, `external_document`; que la prueba `crates/app/tests/privacidad_e2e.rs` lo cubra.
- [ ] ¿Es una herramienta nueva del servidor MCP? Sólo en empresas con el acceso activado
      (`mcp_ajustes`), anotada en la auditoría (`mcp.call`), con el aviso de texto de terceros si lo
      devuelve, y aquí en la fila del servidor MCP: lo que sale por él va sin anonimizar.
- [ ] ¿Usa el certificado? Sólo con Touch ID en el momento (`prepare_identity` → `confirm`),
      nunca hacia un modelo, y quedando en la auditoría.
- [ ] ¿Tiene efecto legal (presentar, enviar, pagar)? Confirmación explícita de la persona con
      los datos exactos que se envían; nunca automático.
- [ ] ¿Guarda algo nuevo? En el almacén cifrado, nunca en `localStorage` ni en ficheros sueltos
      sin cifrar; sin datos personales en los registros (`tracing`).
- [ ] ¿Dependencia nueva? Licencia permisiva (lo comprueba el CI) y sin telemetría.

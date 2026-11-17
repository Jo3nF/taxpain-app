# Notas de versión de TaxPain

Una sección por versión, con el número exacto que tiene la app (`node scripts/version.mjs`). El
actualizador enseña estas notas y el workflow de versiones falla si falta la de la etiqueta.
Escribe para quien usa la app, no para quien la programa.

## 0.1.0 — sin publicar

Primera versión de prueba para Mac: fiscalidad de autónomos y sociedades (calendario, modelos y
presentación con certificado, facturas con Facturae firmada y VERI\*FACTU en pruebas), contabilidad (diario,
mayor, cierre, cuentas anuales, ejercicios no naturales, depósito y legalización), banco y
remesas SEPA, y asistente con datos anonimizados.

Además, en esta versión: contraseña de copias desde el alta y recuperación de los datos en otro
Mac con ella; VERI\*FACTU desactivado por defecto y sin QR en facturas reales hasta que haya envío
a Hacienda; serie S para facturas simplificadas; página 04 del 303 para quien no presenta el 390;
estimación directa simplificada en el 130 y pantalla de Renta del autónomo; liquidación del IVA
desde el 303; leasing con su factura del arrendador; deshacer una conciliación del banco;
asistente de saldos iniciales; más plazos y avisos en el calendario; una revisión más completa
del trabajo de la gestoría; y conversaciones con la IA sobre cada recomendación de «Ahorrar
impuestos», cada hallazgo de «Qué revisar» y cada borrador de modelo, con tus datos y documentos,
que se guardan para seguirlas otro día.

También: inversiones en valores (Interactive Brokers y XTB) y criptomonedas con punto de
partida; carpetas vigiladas, varias por empresa y con subcarpetas; servidor MCP para Claude Code
y Claude Desktop, **apagado hasta que lo actives** en Ajustes → Claude (MCP) para cada empresa;
conversaciones del asistente por tema; perfil fiscal rellenado con lo presentado; y avance de la
cifra de negocios en Evolución.

Correcciones de cálculo: reserva de capitalización al 10 % antes de 2025; bases negativas al 50 %
o 25 % con más de 20 M€ de cifra de negocios (DA 15.ª LIS); prorrata sin las ventas de
inmovilizado; marca de volumen del 303 del 4T; factura simplificada hasta 400 € (3.000 € en las
actividades del art. 4.2); domicilio obligatorio en la completa; anulación en el periodo de la
factura; sin cierre con saldo en los grupos 8 y 9; lo aceptado en pruebas de VERI\*FACTU no cuenta
como remitido; importes con separador de miles; retrocesiones de camt; Kraken en hora de Madrid;
«Ingreso» de CoinTracking; cripto por ejercicio no natural.

Criptomonedas de CoinTracking y Kraken: la comisión cobrada en la misma cripto ahora se descuenta
de la cantidad (antes el saldo quedaba de más). **Si ya las habías importado**, vuelve a importar
el mismo fichero: la vista previa dice qué operaciones cambian. Las que no están contabilizadas se
corrigen al importar: cambian las cantidades, y el valor en euros que hubieras puesto a mano (en
una permuta, p. ej.) se conserva. Las de Kraken de cerca de medianoche también se reconocen, aunque
se guardaran con el día de UTC. Las contabilizadas, o con un ejercicio cerrado de por medio, no se tocan: el
aviso te dice cuáles hay que borrar y volver a importar. Si una comisión no se puede descontar
(igual o mayor que lo comprado), la operación queda como en el fichero y sale un aviso para
revisarla.

Banco: un cobro o pago por menos de lo pendiente de una factura deja pendiente el resto, y uno por
más salda la factura por lo pendiente y lleva lo de más a la 438 (anticipos de clientes) o a la
407 (anticipos a proveedores), con el concepto del asiento que lo dice.

Seguridad: emitir, anular, reenviar a VERI\*FACTU, cerrar o reabrir un ejercicio, pagar
dividendos o deshacer su pago, borrar una empresa y cambiar la contraseña de las copias piden
Touch ID (o tu contraseña). También borrar lo que no se puede recuperar (asientos, nóminas,
viajes, leasing, centros de coste, presupuestos, contactos, facturas periódicas, documentos,
modelos presentados, socios, partícipes, y lo de cripto y valores), guardar o quitar la clave
del asistente, activar «Full IA» y vigilar una carpeta nueva: el aviso dice qué se borra o
cambia y lo comprueba Rust, no la pantalla. El actualizador no instala una versión que se
anuncie como igual o anterior a la instalada, y exige que la versión anunciada sea la que va
firmada en el paquete: así tampoco cuela una versión antigua disfrazada de nueva. Al restaurar
una copia de seguridad se pide una contraseña nueva para las copias y hay que volver a elegir
las carpetas vigiladas, la carpeta de la empresa y dónde guardar las facturas. La base de datos
de cada empresa se actualiza sola la primera vez que la abres con esta versión.

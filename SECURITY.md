# Seguridad

TaxPain guarda en tu Mac el certificado digital, la contabilidad y las facturas de tu empresa, y
habla con la Agencia Tributaria en tu nombre. Un fallo de seguridad puede tener consecuencias
legales y económicas para quien lo usa, así que te agradecemos que avises con discreción.

## Cómo avisar

**No abras una incidencia pública.** Usa **Security → Report a vulnerability** en este
repositorio (aviso privado de GitHub). Cuenta:

- qué has encontrado y qué permite hacer a un atacante;
- la versión o el commit, y los pasos para reproducirlo;
- si ya lo has contado a alguien más.

Contestaremos en cuanto podamos (es un proyecto pequeño: normalmente en unos días). Te diremos
si lo confirmamos, cómo lo arreglamos y cuándo sale el arreglo, y, si quieres, te citaremos en
las notas de la versión. No hay programa de recompensas.

## Qué versiones

TaxPain está en **beta**. Sólo se arregla la última versión publicada y la rama principal.

## Qué nos interesa especialmente

- Cualquier forma de sacar del Mac el certificado, su contraseña o las claves del almacén, o de
  usarlo sin que el usuario lo confirme con Touch ID o su contraseña.
- Emitir, anular o presentar algo ante la AEAT sin confirmación.
- Que un fichero, una factura recibida, un extracto, una copia de seguridad o la respuesta de un
  servidor consiga ejecutar código, escribir fuera de donde debe o tumbar la app.
- Que lleguen datos sin anonimizar al asistente de IA, o que el servidor MCP haga más de lo que
  su nivel permite.
- Conexiones a hosts que no están en la lista permitida.

Cómo está pensado y qué sale del ordenador: [`docs/PRIVACIDAD_APP.md`](docs/PRIVACIDAD_APP.md) y
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Qué no hace falta avisar en privado

Fallos sin impacto de seguridad (cálculos, textos, interfaz): mejor en una incidencia normal.
Recuerda no adjuntar nunca datos reales (certificados, NIF, facturas, extractos).

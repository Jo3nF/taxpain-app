# Modelo 193 (ejercicio 2025): notas para el generador

Fuente: `DR_Modelo_193_2025.pdf` (sha256 25ab1907…62489, 33 págs.). Las páginas citadas son las
del PDF. Layout en `193.json` (`193-1` declarante, `193-2` perceptor, 500 posiciones cada uno),
transcrito a mano; un test comprueba que los campos son contiguos y suman 500.
El registro de «Relación de gastos» del art. 26.1.a) LIRPF (también tipo 2) no se transcribe:
sólo lo presentan las entidades que cobran gastos de administración y depósito de valores.

## Formato (págs. 9 y 33)
- Numéricos a la derecha con ceros; alfanuméricos a la izquierda con blancos, en mayúsculas, sin
  caracteres especiales ni vocales acentuadas. «Todos los importes serán positivos»: no hay
  campos de signo (a diferencia del 190 y el 180).
- NIF de declarante, perceptor y representante ajustados a la derecha con ceros (págs. 2, 10).
- Importes en dos subcampos: parte entera y 2 decimales.

## Registro tipo 1 (págs. 2-9)
- 58 tipo de soporte («T»); 59-67 teléfono y 68-107 persona de contacto.
- 108-120 número identificativo (13 cifras, empieza por 193); 121 «C» / 122 «S» y 123-135 la
  declaración anterior.
- 136-144 número de registros tipo 2.
- 145-159 base (suma de 152-164 del tipo 2); 160-174 retenciones (suma de 169-181);
  175-189 retenciones «ingresadas» (claves C, o A/B/D con pago 1 o 3; a ceros con «S»).
- 190-219 blancos; 220-234 gastos (relación de gastos; a ceros con «S»).
- 235 naturaleza del declarante: «S» si no está en las categorías a) entidad que paga por
  cuenta ajena o es depositaria, b) emisor con pagos por cuenta ajena o valores depositados,
  c) regularización de rentas anteriores a 1999, d) perceptor de gastos del art. 26.1.a). Con
  «S», el 208 de todos los tipo 2 va también a «S» y no hay registros de relación de gastos.
- 236-487 blancos; 488-500 sello electrónico (blancos).

## Registro tipo 2, perceptor (págs. 9-33)
- 18-26 NIF del perceptor; 27-35 representante (menores); 36-75 nombre.
- 76 perceptor mediador (sólo A/B/D; blanco con «S»); 77-78 provincia (01-53).
- 79 clave código y 80-91 código emisor: sólo A/B/D, a ceros/blancos con «S».
- 92 clave de percepción: A participación en fondos propios (dividendos), B cesión de
  capitales, C otros, D cesión de capitales a entidades vinculadas por personas físicas.
- 93-94 naturaleza. Para la clave A: 01 primas de asistencia a juntas, 02 dividendos y
  participaciones en beneficios (salvo 07/08), 03-05 otras rentas por la condición de socio,
  06 rendimientos exentos, 07 IIC, 08 dividendos no sometidos a retención ni ingreso a cuenta.
- 95 pago, 96 tipo código, 97-116 cuenta de valores: sólo A/B/D y entidades financieras; a
  ceros/blancos con «S».
- 117 pendiente («X»); 118-121 ejercicio de devengo (sólo rentas de años anteriores pagadas
  ahora); 122 tipo de percepción (1 dineraria, 2 especie).
- 123-135 importe de las percepciones; 136-138 blancos; 139-151 reducciones (art. 26.2
  LIRPF); 152-164 base de retención; 165-168 % (19,00 en general; el último si hubo varios);
  169-181 retenciones; 182-192 penalizaciones (B/D); 193-204 ISIN; 205-207 blancos.
- 208 naturaleza del declarante (como el 235 del tipo 1).
- 209-248 préstamo de valores (fechas, compensaciones, garantías): sólo con tipo código «P».
- 249-313 retenciones por administración (Estado, Navarra, Álava, Gipuzkoa, Bizkaia): sólo si
  se ingresan en proporción al volumen de operaciones (concierto/convenio); si no, ceros.
- 314 Ceuta/Melilla (1) / La Palma (2) / 0.
- 315-321 número de orden: secuencial por registro.
- 322-330 NIF del pagador anterior (pago 2-5).
- 331-338 fecha de devengo (sólo clave A): DD, MM, AAAA, «en particular, la fecha de pago
  establecida en el acuerdo de distribución de dividendos».
- 339 clave de mercado (A/B/D): A mercado secundario oficial español, B de la UE, C otros
  oficiales extranjeros, D otros.
- 340-500 blancos.

## Dudas
- TODO(VERIFY): el número de orden (315-321) es alfanumérico; se ponen 7 cifras con ceros a la
  izquierda («0000001»).
- TODO(VERIFY): que una SL que paga directamente a sus socios declare la naturaleza «S».
- TODO(VERIFY): clave de mercado «D» con naturaleza del declarante «S» (la lista de campos a
  vaciar con «S» no la incluye).
- TODO(VERIFY): separador de registros (CRLF, como en el resto de informativas).

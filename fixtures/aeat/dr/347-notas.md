# Modelo 347 — notas del diseño de registro

Fuente: `347.pdf` (Ejercicio 2025 y siguientes, Orden HAC/1431/2025). Spec: `347.json`.
Páginas = numeración física del PDF (26 págs.; tipo 1 págs. 1-6, declarado 6-20, inmueble 20-25).

## Estructura del fichero (pág. 1)
- Registros de 500 posiciones. Un único registro tipo 1 (declarante) y tantos tipo 2 como
  declarados e inmuebles; los de declarado (pos. 76 = `D`) y los de inmueble (pos. 76 = `I`)
  son registros distintos. Orden: por tipo de registro.
- Alfanuméricos/alfabéticos: a la izquierda, relleno de blancos, MAYÚSCULAS, sin caracteres
  especiales ni vocales acentuadas. Codificación ISO-8859-1 (Ñ = 0xD1, Ç = 0xC7).
- Numéricos: a la derecha, relleno de ceros, sin signo, sin empaquetar.
- Campo sin contenido: numéricos a ceros, alfanuméricos a blancos.
- NIF (declarante, declarado, representante, arrendatario): ajustado a la DERECHA con ceros a la
  izquierda, último carácter = control (excepción a la regla general de alfanuméricos).
- Tipo 1 pos. 488-500: sello electrónico, solo AEAT; en otro caso blancos.

## Importes y negativos
- Euros, sin coma decimal: 13 dígitos de parte entera + 2 de decimales.
- Campos con signo (1 posición `An` delante): `N` si el importe es < 0, blanco en otro caso; la
  magnitud va siempre en positivo. Las notas al pie de cada registro dicen "Todos los importes
  serán positivos" (se entiende la magnitud; el signo va en su campo).
- Sin signo: importe percibido en metálico (2D pos. 101-115).
- 2I pos. 99-114 está rotulado "Numérico" pero incluye la posición de signo 99 (alfabética).
  En el JSON: 99 `An` signo, 100-112 entera, 113-114 decimal.

## Registro tipo 1 — totales del declarante (págs. 4-5)
- 136-144: nº de registros de declarado. 145-160: suma de "importe anual de las operaciones"
  (2D pos. 83-98) restando los que lleven `N`. 161-169: nº de registros de inmueble.
  170-185: suma de 2I pos. 99-114 con el mismo criterio de signo.
- 121 `C` complementaria (solo para añadir registros omitidos; modificar datos se hace en la
  Sede) / 122 `S` sustitutiva; 123-135 nº de la declaración anterior (si no, ceros).
- 108-120: nº identificativo, empieza por `347`. 58: `C` soporte / `T` telemática.

## Claves de operación (2D pos. 82, pág. 9)
- A adquisiciones > 3.005,06 €; B entregas/prestaciones > 3.005,06 €; C cobros por cuenta de
  terceros > 300,51 €; D adquisiciones al margen de actividad empresarial > 3.005,06 € por
  entidades públicas, partidos, sindicatos, asociaciones empresariales, comunidades de
  propietarios (Ley 49/1960) y entidades sociales art. 20.Tres LIVA; E subvenciones de AA.PP.
  (cualquier importe, solo la Administración que paga); F ventas de agencia de viajes;
  G compras de agencia de viajes (transporte aéreo de viajeros, DA 4ª RD 1619/2012).
- El umbral 3.005,06 € aparece SOLO en el texto de las claves A, B y D. El PDF no explica
  cómo se computa (por año natural y por declarado, IVA incluido, etc.). TODO(VERIFY) en RD
  1065/2007 arts. 31-35.
- Exclusiones (operaciones ya en el 349, sujetas a retención, importaciones/exportaciones…):
  el PDF NO las enumera. Solo remite al art. 34.1 RD 1065/2007 para las operaciones que se
  declaran "separadamente en registros diferentes". TODO(VERIFY) art. 33 RD 1065/2007.

## Registro de declarado (2D)
- Provincia (77-78, `Num`): domicilio fiscal; `99` si no residente sin EP. País (79-80, `An`):
  solo no residentes sin EP, código Orden EHA/3496/2011 anexo II (págs. 8-9).
- NIF declarado (18-26) solo NIF español; excluyente con NIF operador comunitario (264-265 país
  UE + 266-280 número, ajustado a la izquierda) (págs. 7, 17-18).
- 83-98: importe anual (pág. 9-10). Trimestrales (págs. 11-17): para cada trimestre, primero el
  importe de operaciones y luego el de transmisiones de inmuebles sujetas a IVA, 16 posiciones
  cada uno (signo+13+2): 1T 136/152, 2T 168/184, 3T 200/216, 4T 232/248.
  Sin contenido si declara una comunidad de propietarios (Ley 49/1960) o si son operaciones en
  criterio de caja (tanto el sujeto pasivo como el destinatario).
  TODO(VERIFY): el PDF no dice explícitamente que la suma 1T-4T deba cuadrar con 83-98.
- 116-131: importe anual por transmisiones de inmuebles sujetas a IVA (IVA incluido).
- 101-115 metálico: cobros en efectivo > 6.000 € de cada declarado; 132-135 = ejercicio en que
  se declararon las operaciones que originan el cobro (pág. 10-11).
- Marcas `X` (cada una obliga a declarar esas operaciones separadamente, otro registro):
  99 operación de seguro (solo aseguradoras); 100 arrendamiento local de negocio;
  281 criterio de caja (compatible con seguro y arrendamiento); 282 inversión del sujeto pasivo
  (solo destinatario, art. 84.Uno.2º LIVA); 283 depósito distinto del aduanero (págs. 10, 18-19).
- Criterio de caja: 83-98 lleva lo devengado por regla general (art. 75 LIVA) y 284-299 lo
  devengado por art. 163 terdecies, ambos en cómputo anual (págs. 18-20).
- 300-305: nº convocatoria BDNS, solo con clave `E` (pág. 20).

## Registro de inmueble (2I, págs. 20-25)
- Lo cumplimenta el ARRENDADOR por cada local arrendado, cualquiera que sea el importe, aunque
  ya lo haya incluido en clave B (pág. 10). Importe = total anual del arrendamiento.
- 115 situación: 1 con ref. catastral (salvo País Vasco/Navarra), 2 País Vasco/Navarra,
  3 sin ref. catastral, 4 extranjero. 116-140 referencia catastral.
- 141-333 dirección (subcampos en el JSON). El PDF rotula todo el bloque como alfanumérico;
  código municipio (INE), provincia y CP van como `An` aunque sean dígitos. TODO(VERIFY).

## Otras dudas
- 2D pos. 77-80 rotulado "Numérico" pero 79-80 es alfabético: se separa en 77-78 `Num` y
  79-80 `An`.
- El PDF no menciona en el texto extraído la orden HAC/1431/2025 (está en el título del
  fichero/portada según el encargo). TODO(VERIFY) si cambia algo respecto a 2024 (p. ej. BDNS).

# Modelo 184: notas del diseño de registro

Fuente: `DR_Modelo_184_2025.pdf` (diseños lógicos, ejercicio 2025, Orden HAP/2250/2015
actualizada por Orden HAC/1430/2025; 49 págs.). Spec en `184.json`. Páginas = numeración física
del PDF (la que figura al pie). Registros de 500 posiciones:

| Hoja | Registro | Págs. |
|---|---|---|
| `184-1` | tipo 1, declarante (la entidad) | 3-8 |
| `184-2E` | tipo 2, rentas de la entidad (pos. 76 = `E`) | 8-36 |
| `184-2S` | tipo 2, socio, heredero, comunero o partícipe (pos. 76 = `S`) | 36-49 |

## Estructura y formato (págs. 2, 7-8, 36, 48-49)
- Un único registro tipo 1 y tantos tipo 2 como claves/subclaves declaradas por la entidad y
  por cada miembro. Orden: por tipo de registro (pág. 2). El PDF no fija el orden entre E y S.
- "Se consignará un registro por cada clave o subclave ... para los que se haya consignado un
  importe y país" (págs. 8 y 36): un registro E por clave/subclave (y por país si es extranjero;
  en clave C, presumiblemente uno por inmueble, ver Dudas); un registro S por miembro y
  clave/subclave.
- Alfanuméricos/alfabéticos: a la izquierda, blancos a la derecha, MAYÚSCULAS, sin caracteres
  especiales ni vocales acentuadas. ISO-8859-1 (Ñ = 0xD1, Ç = 0xC7) (pág. 2).
- Numéricos: a la derecha, ceros a la izquierda, sin signo y sin empaquetar. Sin contenido:
  numéricos a ceros, alfanuméricos a blancos (pág. 2).
- NIF (declarante, representante, miembro, representante fiscal): ajustados a la DERECHA, carácter
  de control al final, ceros a la izquierda (págs. 3, 7, 37).
- Nombres de personas físicas: primer apellido, espacio, segundo apellido, espacio, nombre.
- Importes en euros sin coma decimal: parte entera + 2 decimales. "Todos los importes serán
  positivos": cuando el campo admite negativo, hay una posición de SIGNO delante con "N" (si no,
  espacio). Campos con signo: 2E 151 (ingresos/resultado), 177 (renta atribuible), 196 (renta
  con derecho a reducción); 2S 96 (importe), 216 (rendimiento minorado EO agrario). Los demás
  importes no tienen signo (gastos, retenciones, bases de deducción, ganancias y pérdidas: el
  signo de F/G lo da la subclave).
- Los campos "Blancos" y el sello electrónico se transcriben como `An`. Los alfabéticos del PDF
  se transcriben como `An` (igual que en 180/347).

## Registro tipo 1 (declarante, págs. 3-8)
- 9-17 NIF de la entidad; 18-57 razón social completa, sin anagrama ni nombre comercial.
- 58 tipo de soporte: "T" (la "C" está derogada tácitamente por Orden HFP/231/2018, pág. 3).
- 59-67 teléfono, 68-107 persona de contacto (pág. 4).
- 108-120 número identificativo, 13 dígitos que empiezan por "184" (pág. 4).
- 121 "C" complementaria (sólo añade registros tipo 2 omitidos; las modificaciones se hacen en la
  Sede) / 122 "S" sustitutiva; 123-135 nº de la declaración anterior, si no, ceros (pág. 4).
- 136-144 nº total de registros de socios/comuneros (tipo 2 con hoja "S") (pág. 5).
- 145 tipo de entidad constituida en España: 1 sociedad civil, 2 comunidad de bienes, 3 herencia
  yacente, 4 comunidad de propietarios, 5 otros (pág. 5).
- 146 actividad principal: 1 empresarial, 2 profesional, 3 tenencia y administración de
  inmuebles, 4 de valores o activos financieros, 5 otras (pág. 5).
- 147-155 sólo entidades constituidas en el extranjero: 147 tipo (1-4), 148 objeto (A
  empresarial, B profesional), 149-150 clave país, 151-155 % de renta atribuible a miembros
  residentes (3 enteros + 2 decimales) (págs. 5-6). Para una CB/SC española: ceros/blancos.
- 156 "X" si todos los miembros son contribuyentes del IS o IRNR con EP (pág. 6).
- 157-171 importe neto de la cifra de negocios, sin signo: 157-169 entera, 170-171 decimal
  (págs. 6-7).
- 172-180 NIF del representante, 181-220 su nombre o razón social (pág. 7).
- 221-229 nº total de registros de entidad (tipo 2 con hoja "E") (pág. 7).
- 230-487 blancos; 488-500 sello electrónico (blancos salvo programas AEAT) (pág. 7).

## Registro tipo 2 de entidad (hoja E, págs. 8-36)
Comunes: 1 = "2", 2-4 = "184", 5-8 ejercicio, 9-17 NIF declarante, 18-26 NIF entidad (= NIF
declarante), 27-35 blancos, 36-75 razón social del tipo 1, 76 = "E" (pág. 8).

### Claves (pos. 77) y subclaves (pos. 78-79) (págs. 9-12)
- **A** capital mobiliario: 01 España, 02 extranjero, 03 reducciones aplicables.
- **B** identificación del cesionario de capitales propios (sólo si algún miembro es
  contribuyente del IS o IRNR con EP). Sin subclave en el PDF.
- **C** capital inmobiliario: 01 España, 02 extranjero, 03 reducciones aplicables.
- **D** actividades económicas: 01 España, 02 extranjero, 03 importe con derecho a reducción.
- **E** rentas contabilizadas de IIC (miembros IS/IRNR con EP).
- **F** ganancias/pérdidas NO derivadas de transmisiones: 01 ganancias España, 02 pérdidas
  España, 03 ganancias extranjero, 04 pérdidas extranjero.
- **G** ganancias/pérdidas derivadas de transmisiones: 01/02 ganancias/pérdidas sin reducción
  España, 03/04 ídem extranjero, 05/06 ganancias con reducción de elementos no afectos (España/
  extranjero), 07/08 ganancias con reducción de elementos afectos (España/extranjero).
- **H** entidades que determinan la renta según el IS (todos los miembros IS/IRNR con EP).
- **I** deducciones IRPF (base): 01 patrimonio histórico, 02 donativos, 03 Ceuta y Melilla,
  04 deducciones en actividades económicas, 05 doble imposición internacional (impuesto
  satisfecho en el extranjero), 06 inversión en empresas de nueva creación.
- **J** deducciones IS: 01 doble imposición internacional, 02 con límite de cuota, 03 donativos,
  04 otras.
- **K** retenciones e ingresos a cuenta soportados por la entidad: 01 capital mobiliario,
  02 arrendamiento de inmuebles urbanos (sea capital inmobiliario o actividad económica),
  03 actividades económicas (excepto arrendamientos urbanos), 04 ganancias patrimoniales (IIC y
  premios), 05 otros conceptos (págs. 12, 26-27).
- **L** exceso de rentas negativas en países sin convenio; subclave = letra de la fuente
  ("A", "C", "D", "E", "F", "G") en la pos. 78 (ver Dudas).

### Campos por clave (lo relevante para una CB/SC pequeña)
- 80-81 clave país: sólo A/02, B, C/02, D/02, F/03-04, G/03-04-06-08 y L; un registro por país;
  en los demás casos blancos (págs. 12-13).
- 82 régimen de determinación (sólo clave D): 1 ED normal, 2 ED simplificada, 3 EO (sólo si
  todos los miembros son personas físicas) (págs. 13-14).
- 83 tipo de actividad (clave D): 1 empresarial mercantil, 2 agrícola/ganadera, 3 otras
  empresariales no mercantiles, 4 profesional artística o deportiva, 5 restantes profesionales.
  84-87 grupo o epígrafe IAE (clave D) (pág. 14).
- 151-164 ingresos íntegros (signo 151, 152-162 entera, 163-164 decimal): A/01-02, B, C/01-02,
  D/01 (no en EO), D/02; clave H = resultado contable (págs. 16-17).
- 165-176 gastos fiscalmente deducibles sin signo: A/01-02, C/01-02, D/01 (no en EO), D/02;
  clave L = exceso de rentas negativas (págs. 17-19).
- 177-190 renta atribuible / rendimiento neto (signo 177): A/01-02, C/01-02, D/01-02 (ingresos −
  gastos; en EO el rendimiento neto; con régimen 2 es el "rendimiento neto atribuible previo",
  antes de provisiones y gastos de difícil justificación), E, H (págs. 19-20).
- 191-195 % de reducción (A/03 art. 26.2 LIRPF; C/03 art. 23 LIRPF) y 196-207 renta atribuible
  con derecho a reducción (A/03, C/03, D/03 art. 32 LIRPF) (págs. 20-22).
- 208-220 ganancias o pérdidas sin signo (claves F y G) (págs. 22-24).
- 221-232 base de deducción (claves I y J) (págs. 24-26).
- 233-244 retenciones e ingresos a cuenta (clave K) (págs. 26-27).
- 245 situación del inmueble (clave C): 1 con ref. catastral salvo País Vasco y Navarra,
  2 País Vasco, 3 Navarra, 4 sin referencia catastral, 5 extranjero; 246-265 referencia
  catastral (clave C) (págs. 27-28).
- 266-395 detalle de gastos de actividades económicas (D/01-02, no EO), cada uno entera +
  2 decimales: personal 266-277, consumos 278-289, tributos 290-301, arrendamientos y cánones
  302-313, reparaciones 314-325, servicios profesionales 326-337, suministros 338-349,
  financieros 350-361, amortizaciones 362-373, provisiones 374-385 (con régimen 2 NO se incluyen
  provisiones ni gastos de difícil justificación), otros 386-395 (págs. 28-32).
- 396 criterio de cobros y pagos: "X" si se aplica (art. 7.2 RIRPF / 11.4 LIS); si no, blanco
  (págs. 32-33).
- 397-497 detalle de gastos de capital inmobiliario (C/01-02): intereses 397-407, conservación
  y reparación 408-418, intereses/reparaciones pendientes de los 4 ejercicios anteriores
  419-429, tributos y recargos 430-439, dudoso cobro 440-450, servicios de terceros 451-460,
  seguros 461-470, amortización del inmueble 471-480, amortización de muebles 481-488, otros
  489-497 (págs. 33-36).
- 498-500 días de arrendamiento o cesión (C/01-02) (pág. 36).

## Registro tipo 2 de socio, heredero, comunero o partícipe (hoja S, págs. 36-49)
- 18-26 NIF del miembro (persona física o jurídica o entidad en atribución). 27-35 NIF del
  representante (menor de 14 años o representante fiscal de no residente; si no, blancos).
  36-75 apellidos y nombre o razón social (págs. 37-38). 76 = "S".
- 77-78 código provincia del domicilio (tabla 01-52 en pág. 38; 99 no residentes sin EP).
  79-80 clave país (sólo no residentes sin EP; Orden EHA/3496/2011 anexo II). 81 tipo de
  partícipe: 1 residente, 2 no residente sin EP, 3 no residente con EP (pág. 39).
- 82 "X" si sigue siendo miembro a 31 de diciembre; 83-85 número de días miembro (pág. 39).
- 86-92 % de participación: 86-88 entero, 89-92 decimal con CUATRO decimales (pág. 39).
- 93 clave: A, C, D, E, F, G, I, J, K (sin B, H ni L) (págs. 39-40).
- 94-95 subclave (págs. 40-41): A 01 (art. 25.1-3 LIRPF) / 02 (art. 25.4 LIRPF); D 01 ED
  normal, 02 ED simplificada, 03 EO no agraria, 04 EO agraria; F y G 01 ganancias, 02 pérdidas;
  I 01-06 y J 01-04 como en la entidad; K 01-05 como en la entidad. C y E sin subclaves en el
  PDF (ver Dudas).
- 96-108 importe atribuido: signo 96, 97-106 entera, 107-108 decimal. Renta atribuible (A, C,
  D, E), ganancias/pérdidas netas (F, G), base o importe de deducción (I, J), retenciones (K).
  Con régimen 2 (ED simplificada) en la entidad, el importe D es el rendimiento neto atribuible
  previo menos las provisiones y gastos de difícil justificación de 160-171 (págs. 41-44).
- 109-119 reducción (A/02 art. 24.2 [sic], C arts. 23.2-23.3, D art. 32 LIRPF), sólo para
  miembros contribuyentes del IRPF (págs. 44-45).
- 120-159 domicilio fiscal del miembro, texto libre (pág. 45).
- 160-171 provisiones y gastos de difícil justificación (clave D con régimen 2 en la entidad):
  5 % del rendimiento neto de la entidad (sin este gasto) × % de participación, máximo 2.000 €
  por socio (pág. 45).
- 172 naturaleza del inmueble (clave C): 1 urbano, 2 rústico; 173 situación (1-5, como 2E 245);
  174-193 referencia catastral (si situación 1, 2 o 3); 194 clave de declarado: N nudo
  propietario, T titular, U usufructuario, O otro derecho real; 195-199 % de titularidad del
  inmueble (3 + 2 decimales); 200-202 días arrendado o cedido (págs. 45-47).
- 203-215 rendimiento neto previo EO no agraria (D/03), sin signo; 216-229 rendimiento neto
  minorado EO agraria (D/04), signo 216 (págs. 47-48). 230-500 blancos.

## Caso típico: comunidad de bienes / sociedad civil en ED o con alquileres
- Tipo 1: 145 = 2 (CB) o 1 (SC); 146 = 1/2 (actividad) o 3 (alquileres); 157-171 cifra de
  negocios; 136-144 y 221-229 contadores S y E.
- Actividad en ED: un 2E clave D/01 con 82 (1 o 2), 83, 84-87, 151-164 ingresos, 165-176
  gastos, 177-190 rendimiento neto, detalle 266-395 y 396. Retenciones soportadas: 2E K/03.
- Alquiler (capital inmobiliario): un 2E clave C/01 con 151-176, 177-190, 245-265, 397-497 y
  498-500; si hay reducción por vivienda, 2E C/03 con 191-207. Retenciones de inquilinos
  empresarios: 2E K/02.
- Por cada comunero y cada clave/subclave con importe: un 2S con 77-92, 93-95, 96-108 y, si
  procede, 109-119, 120-159, 160-171 (D con régimen 2) y 172-202 (C).

## Dudas
- TODO(VERIFY): 2E 196-207 (renta con derecho a reducción): el PDF (pág. 22) dice "Campo
  numérico de 13 posiciones" para 197-207, pero 197-207 son 11 (197-205 entera + 206-207
  decimal). Se transcribe 196 signo + 197-205 + 206-207, que encaja con el rango 196-207.
- TODO(VERIFY): 2E 396 (criterio de cobros y pagos) está rotulado "Numérico" pero su contenido
  es "X" o blanco: se transcribe `An`.
- TODO(VERIFY): 2E 151-164 y 177-190 están rotulados "Alfanumérico" (incluyen el signo); 2S
  216-229 "Numérico" pero 216 es un signo "N"/espacio. Se transcribe signo `An` + partes `Num`.
- TODO(VERIFY): 2S 160-171 (provisiones y gastos de difícil justificación) no se subdivide en el
  PDF; por analogía serían 160-169 entera + 170-171 decimal. Se deja como un único campo `Num`
  de 12 posiciones.
- TODO(VERIFY): 2S 94-95 (subclave) es "Numérico". Para claves sin subclave (C, E) se asume
  "00"; en 2E 78-79 es alfanumérico y para claves sin subclave (B, E, H) se asume blancos. El PDF
  no lo dice.
- TODO(VERIFY): clave L de 2E: la subclave es una letra ("A", "C", ...) en un campo de 2
  posiciones (78-79); se asume letra en 78 y blanco en 79.
- TODO(VERIFY): 2E 84-87 grupo o epígrafe IAE es numérico de 4 posiciones; epígrafes con
  decimal (p. ej. 831.2) no caben tal cual. Probablemente sin punto y truncado (8312); no lo
  aclara el PDF.
- TODO(VERIFY): 2E 147 dice "dígito ... que corresponda a la actividad principal" pero la lista
  es de tipos de entidad (1-4): se transcribe como tipo de entidad extranjera.
- TODO(VERIFY): clave C con varios inmuebles: los campos de inmueble (245-265, 498-500 en 2E;
  172-202 en 2S) son por inmueble, lo que sugiere un registro C por inmueble, pero el PDF sólo
  dice "un registro por cada clave o subclave". Igual para el número de registros S por
  inmueble.
- TODO(VERIFY): 2S 109-119 reducción para clave A cita "subclave 02" y el "artículo 24.2" LIRPF
  (en la entidad la reducción de capital mobiliario es la del art. 26.2): posible errata.
- TODO(VERIFY): 2E 165-176 y 2E 208-220 sin signo: las pérdidas F/G van por subclave; en
  2S F/G 02 (pérdidas) no se aclara si el importe de 96-108 lleva "N" o va en positivo.
- TODO(VERIFY): separador entre registros y presentación por fichero: el PDF no los fija (como
  en 180/190/347).
- TODO(VERIFY): el PDF no marca campos obligatorios uno a uno; "Todos los campos tendrán
  contenido, a no ser que se especifique lo contrario" (pág. 2), y la mayoría dependen de la
  clave.

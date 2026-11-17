# Modelo 190 (ejercicio 2025): notas para el generador

Fuente: `DISENOS_LOGICOS_190_2025.pdf` (sha256 a7d1092f…0134d). Las páginas citadas son las del PDF.
El layout de posiciones está en `190.json` (`190-1` declarante, `190-2` perceptor, 500 posiciones cada uno).

## Estructura y formato (pág. 2, 6, 53)
- Un registro tipo 1 (declarante) y, a continuación, N registros tipo 2 (perceptores). Se ordenan ascendentemente por tipo de registro.
- Cada registro tiene 500 posiciones. TODO(VERIFY): el PDF no dice qué separador de línea usar (CRLF o LF); lo habitual en la Sede es CRLF.
- Los campos alfanuméricos y alfabéticos van alineados a la izquierda y rellenos de blancos, en mayúsculas, sin caracteres especiales ni vocales acentuadas.
- Los campos numéricos van alineados a la derecha, rellenos de ceros, sin signo y sin empaquetar.
- Codificación ISO-8859-1: «Ñ» = 0xD1 y «Ç» = 0xC7.
- Un campo sin contenido se rellena con ceros si es numérico y con blancos si es alfanumérico.
- Los NIF (declarante 9-17, perceptor 18-26, representante 27-35, cónyuge 158-166) van ajustados a la derecha, con el carácter de control en la última posición y ceros a la izquierda (pág. 3, 7, 8, 34).
- Nombres de persona física: primer apellido, espacio, segundo apellido, espacio, nombre. Sin nombre comercial (pág. 3, 8).
- Registro 1: la posición 58 «Tipo de soporte» vale siempre `T`. Las posiciones 121 y 122 llevan `C` (complementaria) o `S` (sustitutiva); si se usa una de ellas, 123-135 es el número de la declaración anterior, y si no, ceros (pág. 4). 488-500 es el sello electrónico, que va a blancos (pág. 6).

## Importes y signos (pág. 5, 28-31)
- Todos los importes están en euros y sin coma decimal, como dos subcampos: parte entera (11 dígitos en el tipo 2, 13 en el total del tipo 1) y parte decimal (2 dígitos).
- No existen importes negativos en los campos numéricos. Un reintegro de cantidades cobradas indebidamente en ejercicios anteriores se indica con `N` en el campo de signo y el importe en positivo. En cualquier otro caso el signo es un blanco.
- Signos del tipo 2: 81 (dineraria), 108 (especie), 255 (dineraria por IL) y 282 (especie por IL).
- En un reintegro: las retenciones o ingresos a cuenta van a 0, «Ejercicio devengo» lleva el año del devengo original y se usan la misma clave y subclave que en su día (pág. 31).
- Retenciones, ingresos a cuenta, reducciones, gastos, etc. no tienen signo («Todos los importes serán positivos», pág. 53).

## Totales del registro 1 (pág. 4-6)
- 136-144 Número total de percepciones = número de registros tipo 2, sea cual sea su clave.
- 145 signo, más 146-158 (parte entera) y 159-160 (parte decimal): Importe total de percepciones. Es la suma de 82-94, 109-121, 256-268 y 283-295 de todos los registros tipo 2, es decir, dinerarias más valoración en especie, con y sin IL. Los registros con signo `N` se restan. Si el resultado es negativo, la posición 145 lleva `N`.
- 161-173 (parte entera) y 174-175 (parte decimal): Total de retenciones e ingresos a cuenta. Es la suma de 95-107, 122-134, 269-281 y 296-308, sin signo.
- TODO(VERIFY): los ingresos a cuenta repercutidos (135-147 y 309-321) no entran en el total de retenciones según el texto.

## Campos clave del registro 2
- 76-77 Provincia (pág. 8-9): código INE de 2 dígitos del domicilio del perceptor. 53 = Isla de La Palma y 38 = S.C. Tenerife salvo La Palma. 98 = residente en el extranjero (art. 8.2 y 10 LIRPF). Los no residentes que no tributan por IRPF no van en el 190 (van en el 296).
- 78 Clave (alfabética) y 79-80 Subclave. La subclave solo se rellena en las claves B, C, E, F, G, H, I, K y L; en las demás va a ceros (pág. 16). TODO(VERIFY): el texto dice «no se cumplimentará», y entendemos que, al ser un campo numérico, eso significa `00`.
- Hace falta un registro por cada combinación de clave/subclave, ejercicio de devengo y tipo de contrato del mismo perceptor (pág. 16, 31, 35).
- 81-107 Percepciones dinerarias no IL: 81 signo, 82-94 íntegra y 95-107 retenciones (pág. 28-29).
- 108-147 Percepciones en especie no IL: 108 signo, 109-121 valoración, 122-134 ingresos a cuenta efectuados y 135-147 ingresos a cuenta repercutidos (pág. 29-30).
- 148-151 Ejercicio devengo (pág. 30-32): se rellena solo en atrasos de ejercicios anteriores o en reintegros. En cualquier otro caso va a `0000`.
- 152 Ceuta o Melilla: 1 = Ceuta/Melilla, 2 = La Palma y 0 = en otro caso (pág. 32).

## Claves (pág. 10-16) y subclaves (pág. 16-27)
- **A** Empleados por cuenta ajena en general (procedimiento general del art. 82 RIRPF). Incluye la IL pagada directamente por la empresa. No lleva subclave.
- **B** Pensionistas y haberes pasivos. Subclaves 01 (pensiones de la SS y Clases Pasivas, e IL pagada por la SS), 02, 03, 04 y 99.
- **C** Desempleo. Subclaves 01 (general), 02 (ERE), 03 (ERTE) y 04 (cese de actividad de autónomos).
- **D** Desempleo en pago único: solo reintegros anteriores a 2013. Sin subclave.
- **E** Consejeros y administradores. Subclaves 01-04.
- **F** Cursos, conferencias y obras literarias/artísticas/científicas como rendimiento del trabajo. Subclaves 01-07.
- **G** Actividades profesionales (pág. 13, 20-21):
  - 01: tipo general del art. 95.1 RIRPF.
  - 02: tipo específico para recaudadores municipales, mediadores de seguros con auxiliares externos y delegados de Loterías y Apuestas del Estado (LAE).
  - 03: tipo reducido por inicio de actividad (año de inicio y los 2 siguientes).
  - 04: tipo específico de la letra d) del art. 95.1 RIRPF.
  - 05 y 06: propiedad intelectual con tipo general (05) o reducido (06) del art. 101.9 LIRPF.
  - 07: anticipos de derechos de autor.
  - 08: cesión de derechos de imagen.
- **H** Actividades agrícolas, ganaderas y forestales, y estimación objetiva. Subclaves 01-04.
- **I** Rendimientos del art. 75.2.b RIRPF (propiedad intelectual o industrial, asistencia técnica, arrendamiento de muebles, imagen). Subclaves 01-03.
- **J** Imputación de rentas por cesión de derechos de imagen (art. 92.8 LIRPF). Sin subclave.
- **K** Premios y aprovechamientos forestales. Subclaves 01-05.
- **L** Rentas exentas y dietas exceptuadas. Subclaves 01-31 y 99 (por ejemplo, 01 dietas, 05 indemnización por despido, 29 IMV).

## Clave A frente a clave G (pág. 32-52)
- Común a las dos: 18-26 NIF, 36-75 nombre, 76-77 provincia, 78 clave, 81-107 dinerarias y retenciones, 108-147 si hay especie, 148-151 y 152.
- **A** usa todos los «datos adicionales» (153-254, rellenos con ceros/blancos cuando no aplican):
  - 153-156 año de nacimiento, obligatorio.
  - 157 situación familiar 1/2/3, obligatoria.
  - 158-166 NIF del cónyuge, solo si la situación es 2.
  - 167 discapacidad 0-3.
  - 168 contrato 1-4, obligatorio (pág. 34-35).
  - 170 movilidad geográfica.
  - 171-222 reducciones, gastos deducibles, pensiones compensatorias y anualidades.
  - 223-253 descendientes, ascendientes y cómputo de hijos.
  - 254 préstamo de vivienda.
  - Solo en A (y B.01): 255-281 IL dineraria. Solo en A: 282-321 IL en especie, 388 y 389.
- **G** (subclaves 01-06 y 08) solo admite, de los datos adicionales, 171-183 «Reducciones aplicables» (pág. 36). El resto de 153-394 va a ceros/blancos. Los gastos deducibles (184-196) no se aplican a G.
- TODO(VERIFY): el PDF no marca explícitamente qué datos adicionales de A son obligatorios y cuáles opcionales. Las validaciones de la Sede pueden exigir el año de nacimiento, la situación familiar y el contrato.

## Otros campos condicionales
- 169 y 322: solo L.29 (IMV).
- 323-387: solo clave E, para el desglose Estado/Navarra/Álava/Gipuzkoa/Bizkaia (pág. 47-50). Si
  todo se ingresó en la Hacienda Estatal, «únicamente se cumplimentará» 323-335 con la suma de
  las retenciones (95-107) y los ingresos a cuenta (122-134): TaxPain lo rellena en la clave E.

## Clave E (pág. 11-12, 17-18 y 41)
- Retribuciones «satisfechas a personas físicas por su condición de administradores y miembros
  de los Consejos de administración…». Subclave obligatoria: 01 asimilado al régimen general
  (salvo 02); 02 asimilado y cifra de negocios del último periodo impositivo finalizado antes del
  pago inferior a 100.000 €; 03 autónomos (RETA) y esa cifra inferior a 100.000 €; 04 el resto.
- Gastos deducibles (184-196): en E.01 y E.02, las cotizaciones a la Seguridad Social deducidas
  por el pagador (pág. 41). En E.03/E.04 no se rellenan.
- Los datos adicionales de la clave A (153-170: año de nacimiento, situación familiar,
  contrato…) no se rellenan en la clave E.

## Clave L, subclave 01 (pág. 15 y 23)
- «Dietas y asignaciones para gastos de viaje exceptuadas de gravamen conforme a lo previsto en
  el artículo 9 del Reglamento». Sin retención. Lo que pase de los límites va en la clave que
  corresponda (A, E…).
- 390-394: solo B.01, con un indicador 0/1 por tipo de prestación (pág. 52-53).
- 226-487 (tipo 1) y 395-500 (tipo 2) van a blancos.

# Modelo 180: notas del diseño de registro

Fuente: `DR_Mod_180_2023.pdf` (ejercicio 2023, 17 págs.). Spec en `180.json`. Registros de 500
posiciones: tipo 1 (declarante, págs. 2-7) y tipo 2 (perceptor = arrendador, págs. 8-17).

## Formato general (págs. 6-7 y 17)
- Numéricos: ajustados a la derecha, ceros a la izquierda; sin contenido, a ceros.
- Alfanuméricos/alfabéticos: ajustados a la izquierda, blancos a la derecha, en MAYÚSCULAS, sin
  caracteres especiales ni vocales acentuadas; sin contenido, a blancos.
- NIF (declarante, perceptor, representante): ajustados a la derecha, carácter de control al
  final, ceros a la izquierda (págs. 2, 9).
- Importes en euros, sin coma decimal: parte entera + 2 dígitos de parte decimal. "Todos los
  importes serán positivos": el negativo va en un campo SIGNO aparte con "N" (si no, espacio).
- Nombres de personas físicas: primer apellido, espacio, segundo apellido, espacio, nombre.

## Registro tipo 1 (declarante)
- 58 tipo de soporte: "T" telemática / "C" cinta (pág. 3).
- 59-67 teléfono, 68-107 persona de contacto (pág. 3).
- 108-120 número identificativo: 13 dígitos que empiezan por "180" (pág. 3).
- 121 "C" complementaria / 122 "S" sustitutiva; 123-135 nº de la declaración anterior (si no,
  ceros) (págs. 3-4). Una complementaria solo añade registros tipo 2 omitidos.
- 136-144 número total de perceptores = número de registros tipo 2 (pág. 4).
- 145-160 total base: 145 signo ("N" si la suma es < 0), 146-158 entera, 159-160 decimal.
  La suma resta los registros tipo 2 con signo "N" (págs. 5-6).
- 161-175 total retenciones, sin signo: 161-173 entera, 174-175 decimal (pág. 6).
- 176-237 blancos; 238-500 sello electrónico (blancos salvo programas AEAT) (pág. 6).

## Registro tipo 2 (perceptor), campos obligatorios
- 5-8 ejercicio y 9-17 NIF declarante: copia del tipo 1 (pág. 8).
- 18-26 NIF del arrendador (solo NIF asignados en España) (págs. 8-9).
- 27-35 NIF representante legal: solo si el perceptor es menor de 14 años; si no, blancos (pág. 9).
- 36-75 apellidos y nombre / razón social, sin anagramas (pág. 9).
- 76-77 código provincia del domicilio del perceptor: 01-52 + 53 Isla de La Palma (S.C.
  Tenerife 38 excluye La Palma) (pág. 10).
- 78 modalidad: "1" dineraria, "2" en especie (pág. 10).
- 79-92 base: 79 signo ("N" = reintegro de cantidades indebidas de ejercicios anteriores),
  80-90 entera, 91-92 decimal. IRPF: rendimientos íntegros sin IVA; IS/IRNR: contraprestación
  íntegra sin IVA (pág. 11).
- 93-96 % retención: 93-94 entero, 95-96 decimal; si hubo varios, el último (pág. 12).
- 97-109 retenciones e ingresos a cuenta, 13 posiciones, sin signo (pág. 12).
- 110-113 ejercicio devengo: solo atrasos de ejercicios anteriores o reintegros (año de devengo
  original); si no, ceros. Un registro por ejercicio de devengo (págs. 12-13).
- 114 situación del inmueble: 1 con ref. catastral en España salvo País Vasco y Navarra;
  2 País Vasco; 3 Navarra; 4 sin referencia catastral (págs. 13-14).
- 115-134 referencia catastral (20 caracteres) (pág. 14).
- 135-327 dirección del inmueble: tipo de vía INE (135-139), nombre vía (140-189), tipo
  numeración NUM/KM./S/N (190-192), número (193-197), calificador BIS/DUP/MOD/ANT (198-200),
  bloque, portal, escalera, planta, puerta (3 c/u, 201-215), complemento (216-255), localidad
  (256-285), municipio (286-315), código municipio INE (316-320), código provincia (321-322),
  código postal (323-327) (págs. 14-17). 328-500 blancos.

## Dudas
- TODO(VERIFY): pág. 5 da la parte entera de la base total del tipo 1 como 146-159 y la decimal
  como 159-160 (solapan). Se ha transcrito 146-158 (13) + 159-160 (2), coherente con el resto.
- TODO(VERIFY): págs. 5-6 dicen que el total del tipo 1 suma "posiciones 80 a 92" del tipo 2;
  el signo está en 79 (coherente).
- TODO(VERIFY): 97-109 (retenciones del tipo 2) no se subdivide en el PDF; se asume 97-107
  entera + 108-109 decimal (como el total del tipo 1). Se deja como un único campo.
- TODO(VERIFY): el PDF no indica qué subcampos de la dirección son obligatorios; con situación
  4 la referencia catastral irá en blanco. Los subcampos de dirección (incluidos códigos de
  municipio, provincia y postal) son "Alfanumérico" según el campo padre; se transcriben "An".
- TODO(VERIFY): el diseño es del ejercicio 2023; confirmar que no ha cambiado para 2024/2025.

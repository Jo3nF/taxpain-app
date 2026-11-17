# Presentación directa y consulta de declaraciones (AEAT)

Descargados el 2026-10-14 de `https://www.agenciatributaria.es/static_files/AEAT_Desarrolladores/EEDD/General/`
(el servidor usa una cadena con firma SHA-1 que OpenSSL rechaza por defecto: se descargó con
`--ciphers 'DEFAULT:@SECLEVEL=0'`, verificando la cadena igualmente).

| Fichero | SHA-256 |
|---|---|
| `EspecificacionesServiciosDeclaracionesAEAT_ES.pdf` (v30) | `06dc03439e97532dfb1e6ccb69d80711356b066e2029432651f6c89965fdd29a` |
| `consultas.xsd` | `54220f18daa13363cb98558d15441ae6752c6eb60799e812893a482a433f41d2` |

`consulta-ejemplo.xml` es **sintético**, construido según el XSD (un test lo valida).

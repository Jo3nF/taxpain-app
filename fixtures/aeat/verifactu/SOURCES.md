# Fuentes de los ficheros VERI*FACTU vendorizados

Descarga: **2026-10-13**, solo GET anónimo (sin certificado, sin POST, sin SOAP).
Los ficheros se copian **byte a byte** tal como se descargaron (algunos tienen fin de línea CRLF:
no normalizar). Si cambia un SHA-256 al volver a descargar, la AEAT ha publicado una versión nueva.

Base AEAT: `https://prewww2.aeat.es/static_files/common/internet/dep/aplicaciones/es/aeat/tikeV1.0/cont/ws/`

| Fichero | URL | SHA-256 |
|---|---|---|
| `SistemaFacturacion.wsdl` | base + `SistemaFacturacion.wsdl` | `05919120708ff7650612fa6683c9336eaf919335d9a4db10e86759190af48602` |
| `SuministroLR.xsd` | base + `SuministroLR.xsd` | `cbdac8d427cc5ab5d77ca48974cab0f35d6bb819c4c66db361681e3710aeba36` |
| `SuministroInformacion.xsd` | base + `SuministroInformacion.xsd` | `ee4c1655175644de44c4c25055ffeb8e5f4bb4bc3834ce8254d4222ef18c8aa1` |
| `RespuestaSuministro.xsd` | base + `RespuestaSuministro.xsd` | `82acf80f785643caac13087aae66808ed721a13f08ca5218cf8ae81b695549ef` |
| `ConsultaLR.xsd` | base + `ConsultaLR.xsd` | `bf2cdb8fc4b95b291757a72b76d8fffca06a6d30d9329122ca2fd6b2d5f8f1b1` |
| `RespuestaConsultaLR.xsd` | base + `RespuestaConsultaLR.xsd` | `de35063acb8d9ba0d6ae51acc6b595de9c2b12333250e95e13108ef5f2670d45` |
| `EventosSIF.xsd` | base + `EventosSIF.xsd` | `cc7347c6a9a57a0c8edbc6b9ddcce55176452d0db0e68369477e207e9fbdd7e7` |
| `RespuestaValRegistNoVeriFactu.xsd` | base + `RespuestaValRegistNoVeriFactu.xsd` | `8f47af4f3c49d29b6a62aed261c09f171e855ad6d6bb72ef3fc0b147dc9572f0` |
| `xmldsig-core-schema.xsd` (W3C) | `http://www.w3.org/TR/xmldsig-core/xmldsig-core-schema.xsd` | `d102ad3df7664c307e0c2c776ba4a90513b1969974d8a940bae1a77f9f21e15d` |

Notas:
- `SuministroInformacion.xsd` y `EventosSIF.xsd` importan `http://www.w3.org/2000/09/xmldsig#` con
  `schemaLocation="http://www.w3.org/TR/xmldsig-core/xmldsig-core-schema.xsd"` (URL remota). El WSDL solo declara
  el prefijo `xmlns:ns` de ese namespace, sin importarlo. Para validar sin red
  hay que redirigir esa URL a la copia local (catálogo XML o reescritura en el validador). El fichero W3C
  contiene un `DOCTYPE` con DTD interna: el validador debe aceptarla sin descargar `XMLSchema.dtd`.
- El resto de imports son relativos (`schemaLocation="SuministroLR.xsd"`, etc.) y resuelven dentro de este directorio.
- Los *namespaces* declarados apuntan a `https://www2.agenciatributaria.gob.es/static_files/common/internet/dep/aplicaciones/es/aeat/tike/cont/ws/<fichero>`.
  Se comprobó que en esa ruta (`tike/`, no `tikeV1.0/`) se sirven copias **idénticas** (mismo SHA-256) de
  `SistemaFacturacion.wsdl`, `SuministroLR.xsd` y `SuministroInformacion.xsd`.
- Todas las XSD declaran `version="1.0"` (coincide con `IDVersion` = `1.0`, lista L15).

## Documentos consultados (no vendorizados; solo para referencia y verificación de cambios)

Base: `https://www.agenciatributaria.es/static_files/AEAT_Desarrolladores/EEDD/IVA/VERI-FACTU/`.
Desde Linux, `curl` exige `--ciphers 'DEFAULT@SECLEVEL=0'` contra `www.agenciatributaria.es` (CA firmada con
un algoritmo antiguo; la cadena sí se valida).

| Documento | Versión / fecha | SHA-256 |
|---|---|---|
| `Veri-Factu_especificaciones_huella_hash_registros.pdf` | v0.1.2, 27/08/2024 | `f4334c254bb875b417247b54315199f89d75a8c4814dfd1e86efec562653d7de` |
| `DetalleEspecificacTecnCodigoQRfactura.pdf` | v0.5.0, 10/12/2025 | `f86b3c260d8a4963dbc18c5007732b53199156c5d1db63242e68db71501b49eb` |
| `Veri-Factu_Descripcion_SWeb.pdf` | v1.0.3, 28/07/2025 | `b3570f6a308ce98a5f52001a0dc427310ad6cf7bccd60a9ee98720a59e553c02` |
| `Validaciones_Errores_Veri-Factu.pdf` | v1.2.2, 08/04/2026 | `426eb926fc098a36a163f66ca5f40d9e0847ca23300bbe5008979832d3513440` |
| `DsRegistroVeriFactu.xlsx` (diseño de registro) | v1.0 | `40ce191aa1def6e44a5f1e86d7ece727258745b34e3fe4d6abe1468252dac2ca` |
| `FAQs-Desarrolladores.pdf` | v1.3 | `73906dc8afbbb9da35f6cb489980352b42aed66d48828fd62a00168883c09d5e` |

Derivados en este directorio: `hash-vectors.json` (ejemplos de huella) y `qr-vectors.json` (ejemplos de URL del QR).

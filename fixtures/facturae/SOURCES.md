# Esquemas Facturae (vendorizados, sin modificar)

| Fichero | URL | Descargado | SHA-256 |
|---|---|---|---|
| `Facturaev3_2_2.xsd` | https://www.facturae.gob.es/content/dam/facturae/formato/versiones/Facturaev3_2_2.xml | 2026-10-13 | `b4bbcd587f5fb0a8a906336cca09b0a40d06ffaa78c6a62f6e438c4e6ea86e07` |
| `xmldsig-core-schema.xsd` | https://www.w3.org/TR/2002/REC-xmldsig-core-20020212/xmldsig-core-schema.xsd | 2026-10-13 | `35cf8197da812c85e40d57891b35c94187569ed474a2dac813ce5090dafcd35c` |

El XSD de Facturae importa xmldsig desde `http://www.w3.org/TR/xmldsig-core/xmldsig-core-schema.xsd`;
los tests validan sin red apuntando ese import a la copia local.

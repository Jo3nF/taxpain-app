# Servicio web de comunicaciones y notificaciones de la AEAT (GNNO-JDIT)

Descargado el 2026-10-14 de
`https://www2.agenciatributaria.gob.es/static_files/common/dep/aduanas/es/aeat/gnno/jdit/ws/`
(enlazado desde la página de la DEHú en la Sede). SHA-256 en `SHA256SUMS`.

- `Documentación_WS_Envíos_AEAT.pdf`: especificación v1.7 (30/06/2025).
- WSDL y XSD oficiales, sin modificar.
- `respuesta-consulta-ejemplo.xml` y `respuesta-acceso-ejemplo.xml` son **sintéticos** (el
  primero basado en el ejemplo de la especificación; el segundo construido según el XSD).

Discrepancia conocida (`TODO(VERIFY)`): `TiposGN.xsd` exige que `Version` tenga longitud 5,
pero la especificación y sus ejemplos usan «1.1»/«1.3». Se envía lo que dice la especificación.

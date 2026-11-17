# Integración con la AEAT: endpoints y diseños de registro

> Fase 0. Investigación hecha el **2026-10-13** leyendo (solo GET) páginas públicas de
> `sede.agenciatributaria.gob.es`, `www.agenciatributaria.es` (portal de desarrolladores), `boe.es`,
> `hacienda.gob.es` y `facturae.gob.es`. No se ha iniciado sesión, no se ha usado ningún certificado
> y no se ha enviado ninguna petición POST ni SOAP. Los endpoints `www1`/`prewww1`/`www10` se citan tal
> como aparecen en la documentación oficial, pero **no se han invocado**.

**Leyenda**
- ✅ **verificado 2026-10-13**: la URL o el dato aparece literalmente en la página o documento oficial enlazado en la columna Notas/Fuente.
- `TODO(VERIFY)`: no se ha podido confirmar en una fuente oficial. Se indica qué falta. **No usar en código sin verificar.**
- Abreviaturas: `SEDE` = `https://sede.agenciatributaria.gob.es`, `DEV` = `https://www.agenciatributaria.es/AEAT.desarrolladores/Desarrolladores/_menu_/Documentacion`, `SF` = `https://www.agenciatributaria.es/static_files/AEAT_Desarrolladores/EEDD`.

Nota técnica: desde este entorno Linux, `curl` contra `www.agenciatributaria.es` falla con
"CA signature digest algorithm too weak" (cadena TLS con un certificado antiguo). En macOS hay que comprobar
que el cliente TLS del core (reqwest/rustls o native-tls) acepta esa cadena. `TODO(VERIFY)`.

---

## 1. Diseños de registro (DR) de modelos

Índice: ✅ `SEDE/Sede/ayuda/disenos-registro.html` → `SEDE/Sede/ayuda/disenos-registro/modelos.html` →
páginas por rango: `modelos-100-199.html`, `modelos-200-299.html`, `modelos-300-399.html`, `resto-modelos.html`
(todas bajo `SEDE/Sede/ayuda/disenos-registro/`; también hay `ejercicios-anteriores-*.html`).
Los ficheros cuelgan de `SEDE/static_files/Sede/Disenyo_registro/...` (columna Recurso, ruta relativa a `SEDE`).

| Modelo | Recurso (fichero DR) | Estado | Ejercicio / versión según el índice |
|---|---|---|---|
| **303** | `/static_files/Sede/Disenyo_registro/DR_300_399/archivos_26/DR303e26v101.xlsx` | ✅ (modelos-300-399) | "Ejercicio 2026 y siguientes (actualizado 28/01/26)" |
| **130** | `/static_files/Sede/Disenyo_registro/DR_100_199/archivos/DR130e15v12.xls` | ✅ (modelos-100-199) | "Orden HAP/258/2015 (Ejercicios 2019 y siguientes, actualizado marzo 2019)" |
| **111** | `/static_files/Sede/Disenyo_registro/DR_100_199/archivos/dr111e16v18.xls` | ✅ (modelos-100-199) | "Orden EHA/3127/2009 (Ejercicios 2019 y siguientes, actualizado mayo 2021)" |
| 115 | `/static_files/Sede/Disenyo_registro/DR_100_199/archivos/DR115e15v13.xls` | ✅ | "Orden EHA/3435/2007 (Ejercicios 2019 y siguientes, actualizado febrero 2019)" |
| 349 | `/static_files/Sede/Disenyo_registro/DR_300_399/archivos_20/DR_Anexo_349.pdf` | ✅ | "Orden HAC/174/2020 (Ejercicio 2020 y siguientes)", formato PDF |
| 390 | `/static_files/Sede/Disenyo_registro/DR_300_399/archivos_26/dr390e2026.xlsx` | ✅ | "Ejercicio 2026 y siguientes" (v1.00; Orden HAC/27/2026, BOE 26/01/2026; comprobado el 31-10-2026, página actualizada el 26/10/2026). El de 2025 (`archivos_25/dr390e2025.xlsx`, "actualizado 05/12/2025") pasa a ejercicios anteriores y TaxPain lo sigue usando para generar 2025 y los anteriores. La AEAT revisó en diciembre los de 2024 y 2025: volver a mirar en diciembre de 2026 y en enero de 2027 (`tools/aeat/dr_actualizar.py --comprobar`), `TODO(VERIFY)` |
| 190 | `/static_files/Sede/Disenyo_registro/DR_100_199/archivos_25/DISENOS_LOGICOS_190_2025.pdf` | ✅ | "Orden EHA/3127/2009 (actualizada por Orden HAC/1431/2025, de 3 de diciembre)" |
| 180 | `/static_files/Sede/Disenyo_registro/DR_100_199/archivos_23/DR_Mod_180_2023.pdf` | ✅ | "Orden HAP/1732/2014 (actualizado por Orden HFP/1284/2023)" |
| 347 | `/static_files/Sede/Disenyo_registro/DR_300_399/archivos/347.pdf` | ✅ | "Ejercicio 2025 y siguientes. Modificados por Orden HAC/1431/2025" |
| 202 | `/static_files/Sede/Disenyo_registro/DR_200_299/archivos_25/DR202e25.xlsx` | ✅ | "Ejercicio 2025 y siguientes (actualizado 14-07-26)". Anexo: `.../DR_200_299/A202e25.xlsx` |
| 200 | `/static_files/Sede/Disenyo_registro/DR_200_299/archivos_25/DR200e25.xls` | ✅ | "Ejercicio 2025 (actualizado 19-06-2026)", 11,2 MB. Anexos (FJD, RIC, etc.) en la misma página |
| 131 | `/static_files/Sede/Disenyo_registro/DR_100_199/archivos_26/DR131_2026.xlsx` | ✅ | "Ejercicios 2026 (actualizado 28/09/26)" |
| 123 | `/static_files/Sede/Disenyo_registro/DR_100_199/archivos_24/DR123e24.xls` | ✅ | "Orden EHA/3435/2007 (Ejercicio 2024 y siguientes)" |
| 193 | `/static_files/Sede/Disenyo_registro/DR_100_199/DR_Modelo_193_2025.pdf` | ✅ | "Orden EHA/3377/2011 (actualizado por Orden HAC/1430/2025)" |
| 184 | `/static_files/Sede/Disenyo_registro/DR_100_199/DR_Modelo_184_2025.pdf` | ✅ | "Orden HAP/2250/2015 (actualizado por Orden HAC/1430/2025)" |

Notas:
- Los DR de las **autoliquidaciones** (303, 130, 111, 115, 123, 131, 390, 200, 202) son hojas Excel con posiciones y longitudes. Los de las **informativas** (190, 180, 347, 349, 193, 184) son PDF con "diseños lógicos" (registro de declarante + registros de detalle). No se han descargado ni parseado aquí: las posiciones de campo **no** se transcriben en este documento. Siguiente paso: descargar cada fichero a `fixtures/aeat/dr/` con su SHA-256 y fecha, y generar los tests a partir de ellos. `TODO(VERIFY)` por campo.
- Que el 130 y el 111 tengan DR "2019 y siguientes" sugiere que el formato sigue vigente en 2026. Hay que confirmarlo comparando con el formulario de 2026: `TODO(VERIFY)`.
- El portal de desarrolladores también tiene secciones por impuesto (Retenciones 2025/2026, IVA 303–390, Pagos a cuenta, IS, Informativas) en `DEV/Documentacion.html`. No se han revisado una a una: `TODO(VERIFY)`.
- Vigencia de los DR de las informativas para el ejercicio 2025 (presentación en 2026), confirmada el 26-10-2026: los PDF de `fixtures/aeat/dr/` ya son las versiones de la Orden HAC/1430/2025 (182/184/193/195/199/282/289/345; [BOE-A-2025-25389](https://www.boe.es/diario_boe/txt.php?id=BOE-A-2025-25389)) y de la Orden HAC/1431/2025 (190/270/347; [BOE-A-2025-25390](https://www.boe.es/diario_boe/txt.php?id=BOE-A-2025-25390)), BOE de 12-12-2025, aplicables por primera vez al ejercicio 2025: el 190 las cita en portada, el 347 trae el campo BDNS (300-305), el 184 el «número total de registros de entidad» (221-229) y el 193 el código ISIN (193-204). El 180 y el 115 no fueron modificados por esas órdenes.

## 2. Presentación directa, prevalidación y consulta (servicios comunes de declaraciones)

Fuente principal: ✅ **"Especificaciones de los servicios de Presentación Directa, Consulta de Declaraciones Presentadas y Validación de Impresión"**, versión 30, en
`SF/General/EspecificacionesServiciosDeclaracionesAEAT_ES.pdf`. Está enlazado desde `DEV/Documentacion.html` → General
(`DEV/General/General.html`).

| Recurso | URL | Estado | Notas |
|---|---|---|---|
| Presentación directa (producción) | `https://www1.agenciatributaria.gob.es/wlpl/PFTW-PICW/PresBasicaDos` | ✅ (doc v30 §1.1) | HTTP **POST JSON** (`Content-Type: application/json;charset=UTF-8`). Autenticación **mTLS con certificado** ("Firma No Criptográfica"): el fichero no se firma criptográficamente y, si se firma, se rechaza |
| Presentación directa (pruebas) | `https://prewww1.aeat.es/wlpl/PFTW-PICW/PresBasicaDos` | ✅ (§1.1, §1.4) | "Entorno de Pruebas para Externos": días laborables de 8:00 a 15:00. Su CA de servidor es distinta de la de producción. El justificante lleva marca de agua. Logs borrados a diario |
| Campos de la petición | `MODELO, EJERCICIO, PERIODO, NRC, IDI, F01, FIR="FirmaBasica", FIRNIF, FIRNOMBRE` | ✅ (§1.2) | `F01` = fichero DR completo **sin CRLF ni tabuladores**. `FIRNIF` debe coincidir con el NIF del certificado. `NRC` solo si hay ingreso (tipo I) |
| Respuesta | JSON `respuesta.correcta{CodigoSeguroVerificacion, Fecha, Hora, Expediente, Justificante, urlPdf, avisos, advertencias, ...}` o `respuesta.errores[]` | ✅ (§1.3) | El justificante legal es el **PDF** de `urlPdf` (ejemplo en `www2.agenciatributaria.gob.es/wlpl/...`). "No es posible anular declaraciones ya grabadas" |
| Modelos admitidos en la presentación | 036, 100, **111, 115**, 117, **123**, …, **130, 131**, …, **200, 202**, …, **303**, …, **390**, … | ✅ (§4) | **Las informativas NO están admitidas** (190, 180, 347, 349, 193, 184). Tampoco el 006 ni el 568. El 123 y el 131 sí están admitidos (también en la validación, §5; comprobado el 26-10-2026): ver CALENDARIO-AEAT-05 en `docs/AUDITORIA.md` |
| Prevalidación **ServValiDos** | `https://prewww2.aeat.es/wlpl/PFTW-PICW/ServValiDos` | ✅ (§3, 4.2) | POST JSON `{MODELO, EJERCICIO, PERIODO, IDI, F01, [SINVL]}`. Devuelve `RESPUESTA.AVISOS` y `PDF` (base64) o `respuesta.errores`. Se presta desde el entorno de pruebas; ServVali dice "no requiere autenticación", y para ServValiDos no se indica: `TODO(VERIFY)` (revisado otra vez el 26-10-2026 en la v30 §4.1-4.2: sigue sin decirlo, y la tabla dice `FIC` donde el ejemplo dice `F01`; la app envía `F01` con certificado, que vale en ambos casos) |
| ServVali (antiguo) | `https://prewww2.aeat.es/wlpl/PFTW-PICW/ServVali` | ✅ obsoleto | Form-urlencoded. Sin soporte para ejercicios **2025 y posteriores**, así que no se usa |
| Modelos admitidos en la validación | 100, **111, 115**, 117, **123**, …, **130, 131**, …, **200, 202**, …, **303**, …, **390**, … | ✅ (§5) | Las informativas se validan con "TGVI", no con ServValiDos |
| Consulta de declaraciones presentadas (servicio) | `https://www1.agenciatributaria.gob.es/wlpl/SCEJ-MANT/ConsultaExt` | ✅ (§2) | POST form-urlencoded `NIF, ANR, MOD, EJF, PER, FED, FEH, HOD, HOH` (`MOD` y `EJF` obligatorios). Requiere certificado. Responde XML, como máximo 100 declaraciones. **Sin declaraciones** de ese modelo y año responde `<error><descripcionError>Usted no ha presentado la declaración solicitada</descripcionError></error>` (comprobado el 27/10/2026 con una consulta real: modelo 130, ejercicio 2025). Lista también las informativas (comprobado el 28/10/2026 con 190, 193, 347 y 349 reales; sin ver aún 180, 182, 184, 232, 270, 296, 720 y 721) |
| XSD de esa consulta | `SF/General/consultas.xsd` | ✅ (§2 y menú DEV) | — |
| Juego de caracteres del fichero DR | ISO-8859-1, con una lista cerrada de caracteres | ✅ (§6) | La petición JSON va en UTF-8. Hay que mapear sin desplazar posiciones |
| Pago de autoliquidaciones (REST, NRC) | `SF/Pago_impuestos/PagoImpuestosServiciosREST.pdf` (v1.3), `SF/Pago_impuestos/PagoAutoliquidacionesIBAN.yaml` (OpenAPI) | ✅ enlazados en menú DEV | No leídos. Endpoints `TODO(VERIFY)` |
| Presentación de **informativas** (TGVI / presentación por fichero) | — | `TODO(VERIFY)` | Falta localizar la especificación TGVI ("Presentación de declaraciones informativas mediante fichero") y si existe un servicio directo |
| Documento "Nueva Firma No Criptográfica" | — | `TODO(VERIFY)` | El doc v30 lo cita como publicado en los portales de Entidades Colaboradoras/Desarrolladoras. URL no localizada |
| Presentación vía formulario web (303/130/111) | 303: `https://www1.agenciatributaria.gob.es/wlpl/TOAE-JDIT/VentanaCensalIva?fOrigen=pre303` (Pre303); 130: `https://www1.agenciatributaria.gob.es/wlpl/PAMW-M130/E2026/CONT/index.zul`; 111: `https://www1.agenciatributaria.gob.es/wlpl/OV17-M111/index.zul` | ✅ (fichas `SEDE/Sede/procedimientoini/G414.shtml`, `G601.shtml`, `GH01.shtml`) | Plan B para el MVP (presentación asistida). Simulador 303 sin certificado: `https://www2.agenciatributaria.gob.es/wlpl/A303-FWME/E2026/OPEN/index.zul` |

## 3. VERI*FACTU / Sistemas Informáticos de Facturación (SIF)

| Recurso | URL | Estado | Notas |
|---|---|---|---|
| Portada SIF/VERI*FACTU | `SEDE/Sede/iva/sistemas-informaticos-facturacion-verifactu.html` | ✅ | FAQ "actualizadas a 21 de julio de 2026" |
| Índice de información técnica | `SEDE/Sede/iva/sistemas-informaticos-facturacion-verifactu/informacion-tecnica.html` | ✅ | Enlaza al portal de pruebas `https://preportal.aeat.es` y a la FAQ para desarrolladores `https://www.agenciatributaria.es/static_files/AEAT_Desarrolladores/EEDD/IVA/VERI-FACTU/FAQs-Desarrolladores.pdf` (actualizada 04-12-2025) |
| Página de WSDL | `DEV/Sistemas_Informaticos_de_Facturacion_y_Sistemas_VERI_FACTU/WSDL_de_los_servicios_web/WSDL_de_los_servicios_web.html` | ✅ | — |
| WSDL | `https://prewww2.aeat.es/static_files/common/internet/dep/aplicaciones/es/aeat/tikeV1.0/cont/ws/SistemaFacturacion.wsdl` | ✅ descargado | Servicios `sfVerifactu` y `sfRequerimiento` |
| Endpoint VERI*FACTU, producción | `https://www1.agenciatributaria.gob.es/wlpl/TIKE-CONT/ws/SistemaFacturacion/VerifactuSOAP` | ✅ (WSDL, puerto `SistemaVerifactu`) | SOAP con mTLS (certificado de persona física o de representante) |
| Endpoint VERI*FACTU, producción con certificado de sello | `https://www10.agenciatributaria.gob.es/wlpl/TIKE-CONT/ws/SistemaFacturacion/VerifactuSOAP` | ✅ (puerto `SistemaVerifactuSello`) | — |
| Endpoint VERI*FACTU, pruebas | `https://prewww1.aeat.es/...VerifactuSOAP` · sello: `https://prewww10.aeat.es/...VerifactuSOAP` | ✅ (puertos `*Pruebas`) | El sufijo de la ruta es igual al de producción |
| No VERI*FACTU (por requerimiento) | `.../SistemaFacturacion/RequerimientoSOAP` en www1 / www10 / prewww1 / prewww10 | ✅ (WSDL) | Fuera del MVP |
| XSD | `https://prewww2.aeat.es/static_files/common/internet/dep/aplicaciones/es/aeat/tikeV1.0/cont/ws/` + `SuministroLR.xsd`, `SuministroInformacion.xsd`, `RespuestaSuministro.xsd`, `ConsultaLR.xsd`, `RespuestaConsultaLR.xsd`, `EventosSIF.xsd`, `RespuestaValRegistNoVeriFactu.xsd` | ✅ (página "Esquemas" de DEV) | Vendorizar los ficheros con su hash. Hay copias en `www2.agenciatributaria.gob.es` según terceros: `TODO(VERIFY)` |
| Descripción de los servicios web | `SF/IVA/VERI-FACTU/Veri-Factu_Descripcion_SWeb.pdf` (v1.0.3) | ✅ enlazado | No leído: límites por envío y control de flujo (tiempo de espera entre envíos) `TODO(VERIFY)` |
| Validaciones y errores | `DEV/Sistemas_Informaticos_de_Facturacion_y_Sistemas_VERI_FACTU/Validaciones_y_errores/Validaciones_y_errores.html` | ✅ enlazado | No leído |
| Diseño de registro de facturación | `DEV/Sistemas_Informaticos_de_Facturacion_y_Sistemas_VERI_FACTU/Documento_Diseno_de_registro_de_facturacion/Documento_Diseno_de_registro_de_facturacion.html` | ✅ enlazado | No leído |
| Especificación de la huella (hash) | `https://www.agenciatributaria.es/static_files/AEAT_Desarrolladores/EEDD/IVA/VERI-FACTU/Veri-Factu_especificaciones_huella_hash_registros.pdf` | ✅ leído (v0.1.2, 27/08/2024) | **SHA-256**, entrada UTF-8, salida **hexadecimal en mayúsculas de 64 caracteres**. Alta: `IDEmisorFactura, NumSerieFactura, FechaExpedicionFactura, TipoFactura, CuotaTotal, ImporteTotal, Huella(anterior), FechaHoraHusoGenRegistro` en formato `campo=valor&...`. Anulación: `IDEmisorFacturaAnulada, NumSerieFacturaAnulada, FechaExpedicionFacturaAnulada, Huella, FechaHoraHusoGenRegistro`. El documento trae ejemplos que deben usarse como tests |
| Especificación del QR y del servicio de cotejo | `https://www.agenciatributaria.es/static_files/AEAT_Desarrolladores/EEDD/IVA/VERI-FACTU/DetalleEspecificacTecnCodigoQRfactura.pdf` | ✅ leído (v0.5.0, 10/12/2025) | ISO/IEC 18004:2015, corrección de errores **nivel M**, tamaño entre 30×30 y 40×40 mm. Producción: `https://www2.agenciatributaria.gob.es/wlpl/TIKE-CONT/ValidarQR?nif=…&numserie=…&fecha=DD-MM-AAAA&importe=NNN.DD`. Pruebas: `https://prewww2.aeat.es/wlpl/TIKE-CONT/ValidarQR?...`. Para sistemas no verificables, la ruta es `ValidarQRNoVerifactu`. Codificación URL de `numserie`: `TODO(VERIFY)` en el propio documento |
| Firma electrónica de los registros (solo no VERI*FACTU) | `SEDE/.../informacion-tecnica/especificaciones-tecnicas-firma-electronica-registros-evento.html` | ✅ página existe | No leída |
| Ejemplos de declaración responsable | `SEDE/.../informacion-tecnica/ejemplo-declaracion-responsable.html` | ✅ página existe | Necesario para quien distribuya TaxPain |

**Estado normativo (verificado 2026-10-13)**
- ✅ RD 1007/2023 (BOE-A-2023-24840): `https://www.boe.es/buscar/act.php?id=BOE-A-2023-24840`. Lo modifica el RD 254/2025 (BOE de 02-04-2025, según la portada SIF).
- ✅ Orden HAC/1177/2024, de 17 de octubre (BOE-A-2024-22138): `https://www.boe.es/buscar/act.php?id=BOE-A-2024-22138`.
- ✅ RDL 15/2025, de 2 de diciembre (BOE-A-2025-24446, BOE nº 290 de 03-12-2025): `https://www.boe.es/buscar/doc.php?id=BOE-A-2025-24446`. Convalidado (BOE-A-2025-25695). Plazos: **contribuyentes del IS, antes del 1/1/2027; resto, antes del 1/7/2027**. Lo confirma la nota de la AEAT `SEDE/Sede/iva/sistemas-informaticos-facturacion-verifactu/nota-informativa-ampliacion-plazo-adaptacion-facturacion.html`, actualizada el 13/10/2026.
- ✅ **Nota del Ministerio de Hacienda del 05/10/2026** (`https://www.hacienda.gob.es/sgt/gabsehacienda/nota-informativa-verifactu.pdf`, enlazada desde la portada SIF): es una "previsión" de aplazar las obligaciones pendientes del RD 1007/2023 **hasta octubre de 2028**, alineadas con la factura electrónica de quienes facturan ≤ 8 M€. **No está en el BOE** a 2026-10-26 (reverificado: el texto consolidado del RD 1007/2023, última actualización del 03/12/2025, mantiene en su DF 4.ª «antes del 1 de enero de 2027» y «antes del 1 de julio de 2027», y la página de la AEAT actualizada el 13/10/2026 repite esas fechas sin mencionar 2028). Hasta que se publique, los plazos legales siguen siendo 1/1/2027 y 1/7/2027. `TODO(VERIFY)`: vigilar el BOE (`tools/boe/watch.mjs` ya detecta «sistemas informáticos de facturación»).

## 4. SII (opcional, avanzado)

| Recurso | URL | Estado | Notas |
|---|---|---|---|
| Información técnica SII | `SEDE/Sede/iva/suministro-inmediato-informacion/informacion-tecnica.html` (+ `/wsdl-servicios-web.html`, `/esquemas.html`) | ✅ | Ficha del procedimiento: `SEDE/Sede/procedimientoini/G417.shtml` |
| WSDL (v1.1) | `SEDE/static_files/Sede/Procedimiento_ayuda/G417/FicherosSuministros/V_1_1/WSDL/` + `SuministroFactEmitidas.wsdl`, `SuministroFactRecibidas.wsdl`, `SuministroBienesInversion.wsdl`, `SuministroOpIntracomunitarias.wsdl`, `SuministroCobrosEmitidas.wsdl`, `SuministroPagosRecibidas.wsdl`, `SuministroOpTrascendTribu.wsdl`, `SuministroInmueblesAdicionales.wsdl`, `SuministroVentaBienesConsigna.wsdl`, `ConsultaLLAA.wsdl` | ✅ | — |
| Endpoints (facturas emitidas) | Producción: `https://www1.agenciatributaria.gob.es/wlpl/SSII-FACT/ws/fe/SiiFactFEV1SOAP`. Con sello: `https://www10...`. Pruebas: `https://prewww1.aeat.es/...`. Pruebas con sello: `https://prewww10.aeat.es/...` | ✅ (WSDL descargado) | No se han extraído los endpoints de los demás WSDL: `TODO(VERIFY)` |

## 5. Consultas en la Sede (web, sin API pública salvo lo indicado)

| Recurso | URL | Estado | Notas |
|---|---|---|---|
| Mi área personal | `SEDE/Sede/mi-area-personal.html` | ✅ | — |
| Mis datos censales | `https://www1.agenciatributaria.gob.es/wlpl/BUGC-JDIT/MdcAcceso` | ✅ (enlace en Mi área personal) | Web. Sin API pública documentada |
| Mis expedientes | `https://www1.agenciatributaria.gob.es/wlpl/TEWV-CORE/ResumenVlt` | ✅ | Web |
| Consulta de declaraciones presentadas | API: `SCEJ-MANT/ConsultaExt` (§2). Ayuda: `SEDE/Sede/ayuda/consultas-informaticas/otros-servicios-ayuda-tecnica/consulta-declaraciones-presentadas.html` | ✅ | **Es la única consulta con servicio documentado.** URL de la página web de consulta: `TODO(VERIFY)` |
| Deudas: consulta | `https://www1.agenciatributaria.gob.es/wlpl/SRVO-JDIT/ConsultaDdas?faccion=CONS_DDAS` | ✅ (`SEDE/Sede/deudas-apremios-embargos-subastas/pagar-aplazar-consultar.html`) | Web. Sin API pública documentada |
| Certificados tributarios (estar al corriente, G304) | Ficha: `SEDE/Sede/procedimientoini/G304.shtml`. Solicitud: `https://www1.agenciatributaria.gob.es/wlpl/EMCE-JDIT/ECOTInternetCiudadanosServlet` | ✅ | Web. Índice general: `SEDE/Sede/certificaciones.html` |
| Notificaciones AEAT (ZN01) | `SEDE/Sede/procedimientoini/ZN01.shtml`. Resumen: `https://www1.agenciatributaria.gob.es/wlpl/GNNO-JDIT/ResumenInteresados`. Consulta: `https://www1.agenciatributaria.gob.es/wlpl/GNNO-JDIT/SvInteresadosQuery?VEZ=BUSCAR1` | ✅ | Web |
| DEHú | `https://dehu.redsara.es/` | ✅ (enlazado desde ZN01) | Por documentar: si DEHú ofrece API para destinatarios (p. ej. "LEMA") — `TODO(VERIFY)` en administracionelectronica.gob.es |
| Apoderamientos | Selector: `SEDE/static_files/common/html/selector_acceso/SelectorAccesos.html?ref=%2Fwlpl%2FITTI-APOD%2FSvApodPoderQuery&aut=CPE` | ✅ | Para el roadmap de gestorías |

## 5 bis. Cotejo de documentos por CSV (comprobado el 27/10/2026)

| Recurso | URL | Estado | Notas |
|---|---|---|---|
| Ficha del trámite | `SEDE/Sede/procedimientoini/ZZ05.shtml` | ✅ | Tres gestiones: cotejo sin certificado (`www2…/KATA-APLI/cotejo/forms/CotejoSv`), cotejo con certificado y puesta de manifiesto de expedientes (`www1…/KATA-APLI/cotejo/forms/CotejoIdSv`) y descarga de ficheros por CSV |
| **Servicio web oficial `CotejoInternetV1`** | Índice «E2 - Servicio web de cotejo de documentos electrónicos» en `https://www2.agenciatributaria.gob.es/static_files/common/internet/dep/aduanas/ws.html`; WSDL, XSD y especificación v1.00 (11/01/2022) en `…/static_files/common/internet/dep/aduanas/es/aeat/kata/apli/ws/` (`CotejoInternetV1.wsdl`, `cotejo_request_int_V1.xsd`, `cotejo_response_int_V1.xsd`, `CotejoInternetV1.pdf`). Página informativa: `SEDE/Sede/todas-gestiones/otros-servicios/cotejo-documentos/cotejo-documentos-electronicos_/nuevo-servicio-web-cotejo-documentos-csv.html` (disponible desde 27/02/2023) | ✅ | SOAP 1.1 documento literal. **Con certificado personal**: producción `https://www1.agenciatributaria.gob.es/wlpl/KATA-APLI/CotejoInternetV1SOAP` (sello: `www10`), pruebas `https://prewww1.aeat.es/…` (sello: `prewww10`). Entrada `peticionDocumento{CSV (16), ENI S/N}`; salida `cotejoResponse{documentoENI?, documento{huella, algoritmo, binario base64}?, mensajeSalida{codigo, descripcion, csvSustituto?}}`. Códigos: 1 correcto, 2 sin CSV, 3 no encontrado, 4 anulado (con `csvSustituto`), 5 no cotejable, 6-8 errores, 100 reintentar. Límite de accesos fallidos por día. `TODO(VERIFY)`: que acepte el certificado de representante (no probado) |
| Página pública: PDF por CSV | `https://www2.agenciatributaria.gob.es/wlpl/KATA-APLI/cotejo/CotejoDocSv?CSV=<csv>` | ✅ probado | `application/pdf` sin sesión, certificado ni captcha. Alternativa si el servicio web falla |
| Visor de la declaración (XML) | `https://www2.agenciatributaria.gob.es/wlpl/ADHT-DECL/DeclaVisor?fDetalle=1&fCsv=<csv>&fNif=<nif>` (botón «Declaración electrónica: Ver XML» de la página de cotejo) | ✅ probado, sin documentar | `text/xml` sin sesión. Raíz `DocumentoElectronicoV2` (`…/ADUA/internet/es/aeat/dit/DocumentoElectronicoV2.xsd`) con `DatosDocumento` (NIF titular, presentador, fecha) y `Declaracion csv=…` cuyo CDATA es la petición de presentación en JSON: el campo `fichero` es la declaración en el formato de los diseños de registro. `TODO(VERIFY)`: forma en las informativas; no es un servicio documentado |

Lo usa «Comprobar lo presentado en el año» (`taxpain-aeat::cotejo`, `taxpain-app::presentado_aeat`):
el CSV da acceso al documento, así que sólo se guarda en el almacén cifrado.

Lo usa también «Descargar de la AEAT por CSV» (Importar → Carpeta de la empresa;
`taxpain-app::descarga_csv`) para **cualquier documento** con CSV (requerimientos,
notificaciones ya abiertas en la Sede, justificantes…): servicio web con el certificado si se
elige y, si no da el documento, la página pública; el PDF va a la subcarpeta «Documentos AEAT»
de la carpeta de la empresa. Sólo cotejo: no llama a notificaciones ni a expedientes. Como los
CSV los pega el usuario, una respuesta de la Sede sin PDF (`PublicClient::pdf_answer` →
`Ok(None)`) es de ese código y no cuenta como fallo de la Sede (se para tras 10 seguidos), y en
el servicio web los «no encontrado» y «anulado» no lo dejan de usar. Lo que ya está en la
subcarpeta no se vuelve a pedir.

Visto con 163 declaraciones reales (28/10/2026): el visor responde `text/html` con «Error
interno» para las declaraciones antiguas (2017); para 2018 en adelante, el XML con el fichero,
también de las informativas (190, 193, 347, 349: registros de 500 posiciones) y del 200. Esa
respuesta HTML de «Error interno» es definitiva para el documento (no se vuelve a pedir ni
corta las demás); cualquier otra respuesta que no sea la declaración (mantenimiento, límite de
accesos) cuenta como fallo y se reintenta (`TODO(VERIFY)`: el visor no está documentado). Con
varios años, se recorren del más reciente al más antiguo, con un tope de peticiones a la Sede
por año. El
fichero hay que leerlo con el **diseño de registro de su ejercicio y periodo**
(`taxpain_models::dr_versions`): con el vigente, un 303 de 2025 sale con las casillas cambiadas.
ConsultaExt contesta «El modelo y el ejercicio seleccionados no están disponibles para esta
consulta» para el 721: no es un error ni quiere decir que no se presentara. El servicio web
`CotejoInternetV1` con certificado de representante respondió sin PDF a todas las peticiones
(`TODO(VERIFY)`: el código que da; ahora se registra). La página pública se
pide con una pausa de 300 ms entre peticiones y un tope de 600 por comprobación; las respuestas
definitivas del servicio web (códigos 3, 4 y 5) se guardan y no se vuelven a pedir.

## 6. Calendario del contribuyente

| Recurso | URL | Estado | Notas |
|---|---|---|---|
| Portada | `SEDE/Sede/calendario-contribuyente.html` | ✅ | Enlaza solo el **2026** |
| Calendario 2026 | `SEDE/Sede/ayuda/calendario-contribuyente/calendario-contribuyente-2026.html` (+ `/calendario-anual.html`) | ✅ | HTML. PDF descargable no localizado: `TODO(VERIFY)` |
| iCalendar | `SEDE/Sede/ayuda/calendario-contribuyente/icalendar/instrucciones-integrar-calendario.html` | ✅ | Son **calendarios públicos de Google** (`.../public/basic.ics`), uno por impuesto. Por ejemplo, IVA: `https://www.google.com/calendar/ical/517mcuhcis0lldnp9b7c0nk2q8%40group.calendar.google.com/public/basic.ics`, y Renta: `.../invitado2aeat%40gmail.com/public/basic.ics`. Hay calendarios para Sociedades, Renta y Sociedades, Informativas, CCT y otros: copiar la tabla completa de la página al implementar |
| Calendario 2027 | — | `TODO(VERIFY)` | No publicado a 2026-10-26 (la portada sólo enlaza el de 2026 y «años anteriores»). Se espera hacia diciembre: volver a comprobar |

## 7. Factura electrónica B2B y Facturae

| Recurso | URL | Estado | Notas |
|---|---|---|---|
| RD 238/2026, de 25 de marzo (facturación electrónica obligatoria B2B) | `https://www.boe.es/buscar/act.php?id=BOE-A-2026-7295` | ✅ | Sintaxis admitidas (art. 7.1): UBL, CII, EDIFACT y Facturae, sobre EN 16931. La solución pública exige UBL (art. 11.2). Aplicación: **12 meses** (>8 M€) y **24 meses** (resto) desde la entrada en vigor de la orden ministerial |
| Orden HAC/1028/2026, de 2 de octubre (solución pública de facturación electrónica) | `https://www.boe.es/diario_boe/txt.php?id=BOE-A-2026-20587` (BOE 05-10-2026) | ✅ | Entra en vigor al día siguiente de su publicación (6/10/2026) y desde esa fecha computan los plazos del RD 238/2026 (DF 3.ª). **Confirmado por la AEAT** el 26-10-2026 en «Calendario de implantación» (`SEDE/Sede/iva/factura-electronica/calendario-implantacion.html`): emitir y recibir factura electrónica desde el **6/10/2027** quienes superaron 8 M€ de volumen de operaciones (art. 121 LIVA) el año anterior (los primeros 12 meses acompañando un PDF legible) y desde el **6/10/2028** el resto; comunicación de estados de la factura y de los pagos: 6/10/2027 (> 8 M€), 6/10/2028 (demás personas jurídicas) y 6/10/2029 (personas físicas y entidades en atribución de rentas); plataformas privadas: 6/10/2027. Falta añadir las fechas de los estados (2028/2029) a la web y a la documentación de UBL |
| SPFE en el portal de desarrolladores | WSDL+XSD: `SF/Facturacion-electronica/wsdl_xsd.zip`. Anexo I UBL: `SF/Facturacion-electronica/Anexo-1-Proyecto-OM-FE.pdf`. Descripción: `DEV/Solucion_Publica_de_Facturacion_Electronica__SPFE_/...` | ✅ enlazados en el menú DEV | No leídos. Endpoints `TODO(VERIFY)` |
| Facturae 3.2.x | Página: `https://www.facturae.gob.es/formato/Paginas/version-3-2.aspx`. Esquemas: `https://www.facturae.gob.es/content/dam/facturae/formato/versiones/Facturaev3_2_2.xml` (también `Facturaev3_2_1.xml` y `Facturaev3_2.xml`) | ✅ (enlaces en la página) | El XSD se publica con extensión `.xml`. Firma XAdES según la política de Facturae: `TODO(VERIFY)` |

## 8. Certificados y hosts

| Recurso | URL | Estado | Notas |
|---|---|---|---|
| Ayuda de certificados electrónicos | `SEDE/Sede/ayuda/firma-digital-sistema-clave-pin-tecnica/certificados-electronicos.html` | ✅ | Enlaza "Información sobre certificados de persona física de la FNMT (AC FNMT Usuarios)" y "Información sobre nuevos certificados de representante (FNMT)" bajo `SEDE/Sede/ayuda/consultas-informaticas/firma-digital-sistema-clave-pin-tecnica/`. Requisitos concretos no leídos: `TODO(VERIFY)` |
| Presentación directa | Certificado electrónico reconocido. `FIRNIF` = NIF del titular | ✅ (doc v30) | En una SL presenta el **representante**, con su NIF de persona física (presentador) y el NIF de la sociedad en el DR (declarante). Confirmar que el certificado de representante de persona jurídica cumple: `TODO(VERIFY)` |
| Certificado de sello | Endpoints `www10` y `prewww10` (VERI*FACTU y SII) | ✅ (WSDL) | Útil si en el futuro hay envíos desatendidos |

## Flujo propuesto (modelos 303/130/111)

1. **Calcular** el modelo desde la contabilidad y mostrar el borrador por casillas (core puro, sin IO).
2. **Generar el fichero DR** del ejercicio y periodo con el DR vendorizado (§1), en ISO-8859-1 y con la lista cerrada de caracteres (doc v30 §6).
3. **Prevalidar** con `ServValiDos` (POST JSON a `prewww2.aeat.es`). Mostrar AVISOS/errores y el PDF (base64) del borrador oficial. Si en producción hace falta mTLS: `TODO(VERIFY)`.
4. **Confirmación explícita del usuario**: pantalla con los datos (exigida por la doc v30, que dice que "el propio envío supone la aceptación de los datos") y Touch ID.
5. **Presentar** con mTLS a `PresBasicaDos`: `FIR=FirmaBasica`, `FIRNIF`/`FIRNOMBRE` sacados del certificado y `NRC` si hay ingreso (obtenido antes con el servicio de pago REST o por el banco). En desarrollo, **solo** `prewww1.aeat.es`, con un guard que impida usar el host de producción en builds de desarrollo.
6. **Guardar** CSV, Expediente, Justificante y el PDF descargado de `urlPdf` (mTLS si hace falta: `TODO(VERIFY)`), con su hash en `audit_log`.
7. **Reconciliar** con `SCEJ-MANT/ConsultaExt`, que devuelve un XML validable con `consultas.xsd`.
8. Si es una informativa (190/180/347/349/193/184): de momento, generar el fichero y que el usuario lo presente a mano. TGVI `TODO(VERIFY)`.

## Hosts que exigen certificado (allow-list para el sidecar)

Hosts vistos en enlaces oficiales a 2026-10-13. "Cert" = la documentación indica autenticación con certificado.

| Host | Uso visto | ¿Cert cliente? |
|---|---|---|
| `www1.agenciatributaria.gob.es` | PresBasicaDos, ConsultaExt, VERI*FACTU, SII, datos censales, expedientes, deudas, notificaciones, G304 | **Sí** en servicios (doc v30, WSDL). En la web puede pedir también Cl@ve |
| `www10.agenciatributaria.gob.es` | VERI*FACTU y SII con **certificado de sello** | Sí (sello) |
| `www12.agenciatributaria.gob.es` | Formulario del 130 ("predeclaración") | `TODO(VERIFY)` (posiblemente Cl@ve) |
| `www2.agenciatributaria.gob.es` | Cotejo QR, simulador 303 OPEN, `urlPdf`, pago con tarjeta | **No** para QR y OPEN. Para `urlPdf`: `TODO(VERIFY)` |
| `prewww1.aeat.es` | Pruebas: PresBasicaDos, VERI*FACTU, SII | Sí, **con otra CA de servidor** (doc v30 §1.4) |
| `prewww10.aeat.es` | Pruebas con sello | Sí (sello) |
| `prewww2.aeat.es` | ServValiDos, WSDL/XSD, cotejo QR de pruebas | No para descargar estáticos. ServValiDos `TODO(VERIFY)` |
| `preportal.aeat.es` | Portal de Pruebas externas | `TODO(VERIFY)` |
| `sede.agenciatributaria.gob.es` | Páginas informativas y DR | No |

Allow-list inicial para el sidecar mTLS (coincidencia exacta de host): `www1.agenciatributaria.gob.es`,
`www10.agenciatributaria.gob.es` (solo con sello), `prewww1.aeat.es`, `prewww10.aeat.es`. Hay que
**fijar (pin) las CA**: la de producción y la de pruebas son distintas. `www12` solo cuando se verifique.

### Fuera de la AEAT: VIES (Comisión Europea)

`ec.europa.eu` (`/taxation_customs/vies/rest-api/check-vat-number`), sin certificado de cliente
y sólo cuando el usuario pulsa «Comprobar en VIES» (formulario de factura con cliente de la UE,
o su ficha). Se envían el NIF-IVA del cliente y el NIF de la empresa (para que VIES dé número
de consulta, la prueba para no repercutir IVA). Mismas reglas que el cliente de la AEAT: sólo
`https`, host exacto, sin redirecciones ni proxies, servidor verificado con el almacén del
sistema (`crates/aeat/src/vies.rs`). Queda en la auditoría (`vies.check`) y en la ficha del
cliente. `TODO(VERIFY)`: campos de la respuesta con una consulta real.

### Fuera de la AEAT: tipos de cambio del BCE

`data-api.ecb.europa.eu` (`/service/data/EXR/D.<moneda>.EUR.SP00.A`, CSV), sin certificado de
cliente y sólo cuando el usuario pulsa «Tipo del BCE» (factura, presupuesto, plantilla periódica
o factura recibida en moneda extranjera). Se envían el código de la moneda y las fechas (los diez
días anteriores a la de la operación, para tomar el último tipo publicado: art. 79.Once LIVA; los
cambios oficiales del Banco de España son los de referencia del BCE). Mismas reglas que VIES:
sólo `https`, host exacto, sin redirecciones ni proxies, servidor verificado con el almacén del
sistema, 20 s de tiempo máximo; en el registro, sólo el host y el estado
(`crates/aeat/src/bce.rs`). Queda en la auditoría (`fx.bce`) y en una caché de la bóveda (los
tipos de días pasados, que ya no cambian). Comprobado con la API el 24/10/2026.

### Fuera de la AEAT: bancos (Enable Banking, solo lectura)

`api.enablebanking.com` (`/aspsps`, `/auth`, `/sessions`, `/accounts/{uid}/balances` y
`/accounts/{uid}/transactions`), sin certificado de cliente: cada petición lleva un JWT RS256
firmado en local con la clave de la aplicación de Enable Banking de la empresa (cabecera `kid` =
id de la aplicación; `iss` «enablebanking.com», `aud` «api.enablebanking.com», una hora de
validez). Sólo cuando el usuario conecta un banco o pulsa «Actualizar movimientos». Mismas reglas
que VIES: sólo `https`, host exacto, sin redirecciones ni proxies, servidor verificado con el
almacén del sistema; en el registro, sólo el host, el tipo de petición y el estado (y, si algo
falla, el mensaje de error de Enable Banking); los errores no llevan la URL (lleva el id de la
cuenta) (`crates/banca`). La vuelta del banco se pega a mano
o la recoge TaxPain en `127.0.0.1` (sólo conexiones de la propia máquina). Queda en la auditoría
(`banca.*`). Ver D11. `TODO(VERIFY)`: nombres exactos de BBVA y Revolut en la lista de bancos y
los campos de sus movimientos (que den `entry_reference`), con una conexión real.

## Dudas / TODO(VERIFY)

1. Si **ServValiDos** requiere certificado o tiene un equivalente en producción (`www1`/`www2`). El documento solo da la URL de `prewww2`. Además, la tabla de campos dice `FIC` y el ejemplo `F01`: hay que confirmar el nombre en pruebas. Revisado el 26-10-2026 con la v30 (§4.1, §4.2): sólo del ServVali antiguo dice «Este servicio no requiere autenticación»; para ServValiDos sigue sin indicarlo. La app envía `F01` con certificado, que funciona en ambos casos. Sigue abierto.
2. Documento "**Nueva Firma No Criptográfica**" (especificación completa y calendario): URL no localizada.
3. **Informativas** (190, 180, 347, 349, 193, 184): especificación de TGVI y si hay presentación directa por servicio. El doc v30 las excluye.
4. Posiciones y longitudes de campo de cada DR: descargar los ficheros, vendorizarlos con hash y generar tests. Ninguna posición se ha transcrito aquí.
5. Vigencia del DR del **130** (2019+) y del **111** (2019+) para 2026. **DR 390 de 2026** publicado ("Ejercicio 2026 y siguientes", v1.00; comprobado el 31-10-2026) y ya usado para generar el 390 de 2026; sus instrucciones aún no (la página del procedimiento G412 sólo tiene las de 2025): confirmar el CNAE de 4 cifras de la página 7 y volver a mirar el diseño en diciembre.
6. **Aplazamiento de VERI*FACTU a octubre de 2028**: solo hay nota de prensa (05/10/2026), pendiente de una norma con rango de ley en el BOE. Reverificado el 26-10-2026: sigue sin norma; vigentes el 1-1-2027 (IS) y el 1-7-2027 (resto) del RDL 15/2025. Vigilar (`.github/workflows/boe.yml`).
7. ~~Fechas exactas de la factura electrónica B2B (6/10/2027 y 6/10/2028): cálculo propio.~~ Resuelto el 26-10-2026: confirmadas por la AEAT en su «Calendario de implantación» (ver §7, con las fechas de los estados de la factura: 2027, 2028 y 2029).
8. Endpoints SPFE (factura electrónica pública) y del servicio REST de pago (NRC): documentos enlazados pero no leídos.
9. Si `urlPdf` (www2) exige mTLS para descargar el justificante.
10. Hosts `www12` y `preportal.aeat.es`: tipo de autenticación.
11. Requisitos del certificado de **representante de persona jurídica** (FNMT) en PresBasicaDos/VERI*FACTU. Relación presentador/declarante cuando presenta un tercero (apoderamiento).
12. API de DEHú para destinatarios; calendario del contribuyente **2027** (no publicado a 26-10-2026); PDF del calendario 2026.
13. Cadena TLS de `www.agenciatributaria.es` rechazada por OpenSSL moderno ("CA signature digest algorithm too weak"): comprobar el comportamiento en macOS y en rustls.
14. VERI*FACTU: límites por envío y control de flujo (doc `Veri-Factu_Descripcion_SWeb.pdf` v1.0.3, no leído). Codificación de `numserie` en la URL del QR.

## 9. Censo, deudas, notificaciones y certificados (investigación Fase 6, 2026-10-14)

Fuentes leídas con GET normales, sin enviar nada a la AEAT. El portal PAe/CTT
(administracionelectronica.gob.es) bloqueó las descargas con un WAF, así que el PDF de DEHú LEMA
**no se leyó**.

| Recurso | URL oficial | Estado | Requisitos | Notas |
|---|---|---|---|---|
| Consulta de datos censales (036/037, IAE, obligaciones, domicilio) | Índice EEDD `DEV/Censos/Censos.html`; Sede `/Sede/ayuda/consultas-informaticas/otros-servicios-ayuda-tecnica/datos-censales.html` | ❓ sin servicio web | — | Sólo diseños de registro y tablas. Las consultas («Mis datos censales») son **sólo interfaz web** |
| Presentación 030/036 | Especificación v30 §4 | ✅ | Certificado propio | 005, 030 y 036 admitidos en PresBasicaDos (no en la validación) |
| Deudas / estado de cuenta | Sede `/Sede/deudas-apremios-embargos-subastas/pagar-aplazar-consultar.html` → `https://www1.agenciatributaria.gob.es/wlpl/SRVO-JDIT/ConsultaDdas?faccion=CONS_DDAS` | ❓ sin servicio web | Interfaz web (certificado, DNIe, Cl@ve) | Sólo Sede y app móvil |
| Aplazamientos (RB01) | `/Sede/procedimientos/RB01.shtml` | ❓ sin servicio web | Interfaz web | — |
| Pago con cargo en cuenta (REST) | `SF/Pago_impuestos/PagoImpuestosServiciosREST.pdf` v1.3 + `PagoAutoliquidacionesIBAN.yaml` | ✅ existe, no leído | `TODO(VERIFY)`: si exige ser colaborador | — |
| **Servicio web de comunicaciones y notificaciones** | Especificación v1.7 (30/06/2025) `https://www2.agenciatributaria.gob.es/static_files/common/dep/aduanas/es/aeat/gnno/jdit/ws/Documentaci%C3%B3n%20WS%20Env%C3%ADos%20AEAT.pdf`; WSDL `…/gnno/jdit/ws/{ConsultaEnvios,AccesoEnvio,ConsultaAutorizados}.wsdl` (enlazado desde la página de la DEHú en la Sede) | ✅ | **Certificado propio** por mTLS, sin firma del contenido, «no se requiere ningún tipo de registro previo»; envíos de prueba pidiéndolos a wsnotificaciones@correo.aeat.es | SOAP. Producción `https://www1.agenciatributaria.gob.es/wlpl/GNNO-JDIT/sede/descargas/{ConsultaV1SOAP,AccesoV1SOAP,AutorizadosV1SOAP}`; pruebas `https://prewww1.aeat.es/…`. Consulta por NIF, tipo C/N/T, estado A/P/T y 3 meses como máximo. Acceso con **C = comparecer** (pendientes) o **D = descargar**. **Comparecer abre plazos legales**: confirmación explícita |
| DEHú LEMA (grandes destinatarios) | Especificación en el CTT (no leída) | ❓ `TODO(VERIFY)` | Alta como gran destinatario con declaración responsable y alta en el Centro de Servicios | Parece pensado para personas jurídicas; duplica el servicio propio de la AEAT |
| DEH (060) | `/Sede/informacion-importante.html` | ✅ | — | La AEAT la ha desconectado |
| Certificado de estar al corriente (G304/G331) | `/Sede/procedimientoini/G304.shtml` | ❓ sin servicio web | Interfaz web | Cotejo por CSV también en la web |
| Datos fiscales de Renta | `fixtures/aeat/renta/` (instrucciones v1.2 y diseño v1.3, descargados) | ✅ implementado | Certificado del contribuyente (persona física); domicilio ratificado (`TOAG-JDIT/IsRatificadoExJson`) | `DFPA-D182/SvDesDF25Pei?nif=…&pdp=N`; URL anual `TODO(VERIFY)` cada campaña. TaxPain coteja RTC/PF/CT/RET con sus datos. El 100 se presenta por PresBasicaDos |

**Conclusión.** Se puede hacer ya en nombre propio: notificaciones (servicio SOAP de la AEAT),
datos fiscales de Renta y presentación del 030/036. Requieren alta o convenio (no ahora): DEHú
LEMA y gestiones como colaborador social. Sólo interfaz web (la app abrirá la página de la
Sede): datos censales, deudas, aplazamientos y certificado de estar al corriente.

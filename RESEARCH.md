# TaxPain — Research (13/10/2026)

## 1. Referencia: Spain Assist (Pablo Caballero, @pabcrab)
- Tuit (12/10/2026): https://x.com/pabcrab/status/2109696757378777186 — "Un agente de IA open source que automatiza todos tus trámites con la administración". ~83k impresiones, 969 likes, 879 guardados.
- Web: https://spainassist.es (Framer). Repo: https://github.com/MeLlamoPablo/SpainAssist (MIT; app Android EUPL-1.2). Creado 2/10/2026, v0.1.1 (12/10/2026) con DMG mac arm64/x64, .deb Linux.
- Copia local de referencia: `ref/spainassist/` (leer SPEC.md, docs/ARCHITECTURE.md, docs/DECISIONS.md D1–D22).

### Cómo funciona
- **Electron 44 + React** (apps/desktop), monorepo pnpm/TypeScript, Biome, Vitest, Playwright e2e, electron-builder + auto-update por GitHub Releases.
- **Agente**: harness "pi" (SDK), un orquestador + un agente por trámite; proveedores: Sign in with ChatGPT, vouchers (LiteLLM/OpenRouter), BYOK. Investigación con Exa/Parallel y BOE.
- **Navegador**: Playwright/Chromium con un contexto por trámite; el agente navega las sedes (no usa APIs oficiales; automatiza la web).
- **Certificado**: `.p12` en un **vault cifrado** (clave envuelta con Electron safeStorage → Keychain en macOS; Touch ID para "pedir antes de usar"). Login con certificado cliente vía **sidecar TLS en Go** (`packages/sidecar`): proxy local que presenta el certificado sólo a orígenes exactos; el navegador nunca tiene la clave.
- **AutoFirma**: no lo instala; inyecta un **shim de AutoScript** en cada frame y firma en Node (`packages/signer`: CAdES, XAdES, PAdES, trifásica, batch).
- **DNIe**: app Android lee el DNIe por NFC y firma; relay cifrado extremo a extremo.
- **Cl@ve**: permanente (usuario/contraseña) y móvil (QR).
- **PII**: detección/scrub de datos españoles antes de compartir aprendizajes anónimos (feedback-server, Postgres FTS).
- Backends Hono+Postgres en un VM Hetzner (Coolify/Terraform). Ninguno ve conversaciones.
- Es **generalista de trámites** (no contabilidad ni calendario fiscal). Nuestro hueco: **gestoría automática para autónomos y pymes**.

### Qué podemos reutilizar (MIT, con atribución)
Ideas y posiblemente código de: sidecar TLS con .p12, shim de AutoFirma/signer, vault con Keychain, PII scrubber. Mantener aviso de copyright MIT si se copia código.

## 2. Dominios
- `taxpain.es` → DoH NS: NXDOMAIN (SOA ns1.nic.es). **Parece libre.**
- `taxepain.es` → NXDOMAIN. **Parece libre.**
- Confirmar en el registrador antes de comprar (algunos .es están reservados/prohibidos). No se ha comprado nada.

## 3. Calendario fiscal AEAT (régimen general; plazos que el motor debe calcular, con traslado a día hábil siguiente si cae en sábado/domingo/festivo nacional)
| Modelo | Quién | Periodicidad | Plazo |
|---|---|---|---|
| 303 IVA | autónomos y sociedades | trimestral (mensual si REDEME/gran empresa) | 1–20 abr/jul/oct; 4T 1–30 ene |
| 130 IRPF pago fraccionado | autónomos estimación directa (salvo ≥70% con retención) | trimestral | 1–20 abr/jul/oct; 4T 1–30 ene |
| 131 | módulos | trimestral | ídem |
| 111 retenciones trabajo/profesionales | quien retiene | trimestral (mensual grandes) | ídem 303 |
| 115 retenciones alquiler | arrendatarios de local | trimestral | ídem |
| 123 | rendimientos capital mobiliario | trimestral | ídem |
| 349 operaciones intracomunitarias | si hay ops UE | mensual/trimestral | 1–20 mes siguiente; 4T hasta 30 ene |
| 390 resumen IVA | obligados 303 (salvo exonerados SII) | anual | 1–30 ene |
| 190 / 180 / 193 resúmenes retenciones | quien presentó 111/115/123 | anual | 1–31 ene |
| 347 operaciones >3.005,06 € | no SII | anual | febrero |
| 184 entidades en atribución | CB/SCP | anual | enero |
| 200 Impuesto Sociedades | sociedades | anual | 25 días naturales tras 6 meses del cierre (25 jul si cierre 31/12) |
| 202 pagos fraccionados IS | sociedades con cuota positiva | abr, oct, dic | 1–20 |
| 100 Renta | autónomo persona física | anual | abril–30 jun |
| 036/037 censo | alta/modificación | evento | 1 mes desde el hecho |
| Depósito de cuentas | sociedades | anual | 1 mes tras aprobación (≤30 jun) → Registro Mercantil (formato D2/Legalia) |
| Libros (Legalización) | sociedades | anual | 4 meses tras cierre, Registro Mercantil |
Fuente oficial: calendario del contribuyente AEAT (sede.agenciatributaria.gob.es → Calendario), y BOE de cada Orden de modelo. El calendario oficial es iCal/descargable; usarlo para validar.

## 4. Opciones técnicas de integración con la AEAT
1. **Presentación de modelos con certificado**: la Sede permite presentar la mayoría de autoliquidaciones mediante **formulario web** y mediante **"Presentación mediante fichero"** con el **diseño de registro (formato BOE / "DR")** que publica la AEAT para cada modelo (p.ej. DR303e2026.xlsx). Flujo: generar fichero DR → validar con el servicio de **pre-validación** → presentar firmando con certificado → obtener **CSV y justificante PDF**. También existe la presentación automatizada vía **servicios web (SOAP/REST con autenticación mTLS por certificado)** usada por el software de colaboradores sociales ("presentación por lotes / servicio de presentación de declaraciones"). Verificar en "Información técnica → Servicios web" y "Diseños de registro" de sede.agenciatributaria.gob.es.
2. **Pre303 / datos fiscales**: para autónomos la AEAT ofrece en Sede el 303 prellenado (sólo SII/Libros IVA llevados en sede). Para Renta, servicio de datos fiscales/borrador.
3. **Consulta de lo presentado**: Sede → "Mis expedientes", "Consulta de declaraciones presentadas", "Datos censales" (036), "Deudas y pagos", "Certificados tributarios". No hay API pública documentada para todo: hacerlo **por automatización de navegador con mTLS** (como Spain Assist) con capturas como evidencia.
4. **Notificaciones**: DEHú (dehu.redsara.es) y Notificaciones AEAT en Sede; consultar con certificado.
5. **SII**: servicios web SOAP con mTLS; obligatorio sólo para REDEME, >6 M€, grupos IVA, voluntario para el resto. Útil como opción avanzada.
6. **VERI*FACTU** (RD 1007/2023, Orden HAC/1177/2024): servicio web SOAP con mTLS para remitir registros de facturación con hash encadenado, QR en factura, leyenda "VERI*FACTU". Fechas legales vigentes: 1/1/2027 (IS) y 1/7/2027 (resto) por RDL 15/2025; **Hacienda anunció el 5/10/2026 aplazar a octubre de 2028** (pendiente BOE). Desde 29/7/2025 el software comercializado debe cumplir → como fabricante necesitamos **declaración responsable** del software. Diseñar desde el día 1 compatible (hash encadenado, registros de alta/anulación, eventos, QR).
7. **Factura electrónica B2B** (Ley Crea y Crece; Orden HAC/1028/2026, BOE 5/10/2026): obligatoria desde 6/10/2027 (>8 M€) y 6/10/2028 (resto). Formatos UBL/Facturae/CII, Facturae 3.2.x para FACe (sector público).
8. **TicketBAI** (País Vasco) y **Batuz/SII Navarra**: fuera del MVP, marcar por territorio foral.
9. **AutoFirma/@firma**: para trámites web que piden firma (AutoScript). Reutilizar enfoque de shim o lanzar AutoFirma instalado (`afirma://` protocolo). Firmas XAdES/CAdES/PAdES con certificado del Keychain.
10. **Certificado en macOS**: importar .p12 en el **Keychain** (SecPKCS12Import) o guardar el .p12 cifrado con clave en Keychain + Touch ID (LocalAuthentication). Para mTLS desde Swift: URLSession delegate `didReceive challenge` con `URLCredential(identity:certificates:persistence:)`; desde Rust/Tauri: `reqwest` con `native-tls`/`Identity::from_pkcs12_der` o `security-framework`. Certificados FNMT persona física, representante de persona jurídica (necesario para SL), sello. Nunca enviar la clave fuera del dispositivo ni al LLM.
11. **Contabilidad**: PGC 2007 / PGC PYMES (RD 1515/2007), cuentas 1–7, libro diario, mayor, balance de sumas y saldos, balance, PyG, ECPN; libros registro IVA (expedidas, recibidas, bienes inversión) y para autónomos libros de ingresos/gastos (formato AEAT CSV/XLSX unificado). Normas de redondeo y doble partida.
12. **Bancos**: Norma 43 (AEB) / CAMT.053 para conciliación; PSD2 vía agregador (GoCardless Bank Account Data, etc.) como fase posterior.
13. **Invoices entrada**: OCR/LLM de PDFs/emails, carpetas (Google Drive API, iCloud/carpeta local, Dropbox), Gmail.

## 5. Límites legales y riesgos
- **Presentar por uno mismo**: con el certificado propio (autónomo) o de representante de persona jurídica (SL), la app actúa como herramienta del contribuyente → no hace falta ser colaborador social.
- **Presentar por terceros** (servicio gestoría): requiere **apoderamiento** (Registro de apoderamientos AEAT) o ser **colaborador social** (convenio con AEAT, art. 92 LGT, RD 1065/2007 arts. 79–81). Si TaxPain hiciera presentaciones en servidores propios con certificados de clientes sería otro régimen → evitarlo: **todo local**.
- **Responsabilidad**: los errores en declaraciones generan recargos/sanciones (LGT arts. 27, 191 y ss.). La app debe: mostrar borrador y pedir **confirmación explícita** antes de firmar/presentar; guardar justificantes; disclaimer; no es asesoramiento fiscal; plan "revisión por asesor colegiado" como upsell.
- **RGPD/LOPDGDD**: datos locales; si hay backend, DPA, EIPD. LLM: no enviar certificados ni contraseñas; minimizar PII; ofrecer modelo local o proveedor UE.
- **Software de facturación**: RD 1007/2023 obliga al fabricante (declaración responsable, sanción hasta 150.000 €/ejercicio por software no conforme, art. 201 bis LGT).
- **Automatización web de la Sede**: puede cambiar sin aviso, CAPTCHAs; preferir ficheros DR/servicios web oficiales y usar navegador sólo para consultas.
- **Marca**: "TaxPain" → comprobar OEPM/EUIPO antes de lanzar.

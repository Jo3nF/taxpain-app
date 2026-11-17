# TaxPain — Arquitectura

> Documento vivo. Motivos de cada elección en `docs/DECISIONS.md`; tablas en
> `docs/DATA_MODEL.md`; integración con la AEAT en `docs/AEAT_INTEGRATION.md`.

## 1. Visión general

```
┌──────────────────────────── App de escritorio (apps/desktop, Tauri 2) ────────────────────────────┐
│                                                                                                   │
│  WebView (React/TS)  ◄── comandos Tauri tipados (invoke) + eventos ──►  Proceso nativo (Rust)     │
│  • calendario, facturas, contabilidad,                       ┌──────────────────────────────────┐ │
│    modelos, panel AEAT, asistente                            │ AppState                          │ │
│  • NUNCA recibe claves, contraseñas                          │ • vault  (BD SQLCipher, certs,    │ │
│    ni el .p12: sólo resúmenes                                │   auditoría)                      │ │
│                                                              │ • core   (dominio puro)           │ │
│                                                              │ • aeat   (DR, prevalidación,      │─┼──► Sede AEAT (mTLS,
│                                                              │   presentación, VERI*FACTU)       │ │    sólo hosts exactos)
│                                                              │ • invoicing (PDF, Facturae)       │ │
│                                                              │ • macos  (Keychain, Touch ID,     │ │
│                                                              │   notificaciones)                 │ │
│                                                              │ • asistente IA (BYOK / local)     │─┼──► proveedor LLM (sin
│                                                              └──────────────────────────────────┘ │    secretos, PII mínima)
└───────────────────────────────────────────────────────────────────────────────────────────────────┘
        Keychain (login) ◄── claves `db-master` y `cert-seal` (esta última con biometría)
        ~/Library/Application Support/es.taxpain.desktop/taxpain.db  (SQLCipher)
```

No hay backend propio en el MVP. La landing (`apps/landing`) es estática.

## 2. Crates y responsabilidades

| Crate | Responsabilidad | IO |
|---|---|---|
| `taxpain-core` | Dinero (`Money` céntimos), tipos de IVA/IRPF, NIF/NIE/CIF, periodos fiscales, (Fase 2) calendario, (Fase 4) contabilidad, (Fase 5) cálculo de modelos | Ninguna |
| `taxpain-vault` | BD cifrada y migraciones, `KeyProvider`, `UserPresence`, importación y sellado de certificados, identidad mTLS, registro de auditoría encadenado | Disco |
| `taxpain-aeat` | Diseños de registro (DR), prevalidación, presentación, VERI\*FACTU, consultas; cliente HTTP mTLS con allow-list de hosts | Red |
| `taxpain-invoicing` | Facturas, series, rectificativas, PDF, Facturae/UBL (Fase 3) | Disco |
| `taxpain-macos` | `KeychainKeyProvider`, `TouchIdPresence`, (Fase 2) notificaciones | macOS |
| `taxpain-desktop` | App Tauri: comandos, estado, ventana | Todo |

Reglas: `core` no depende de nada con IO. Los crates no importan Tauri. El código específico
de macOS vive en `crates/macos` y se inyecta desde la app.

## 3. Flujos clave

### 3.1 Primer arranque y desbloqueo
1. La app pide al `KeyProvider` la clave `db-master` (la crea si no existe: 32 bytes de
   `getrandom`, guardada en el Keychain como `WhenUnlockedThisDeviceOnly`).
2. Abre `taxpain.db` con `PRAGMA key` y aplica migraciones.
3. Opcional (ajuste "Bloquear con Touch ID"): `UserPresence::confirm` antes de abrir.

### 3.2 Importar certificado (.p12/.pfx)
1. El usuario elige el fichero; el WebView envía **la ruta** y la contraseña al proceso nativo
   (la contraseña viaja una vez por IPC local y se borra con `zeroize` tras usarla).
2. `vault::certificate::import` valida: contraseña y MAC, clave privada presente y coincidente,
   vigencia, tipo (FNMT persona física / representante de persona jurídica / otro), NIF del
   titular (y de la entidad representada).
3. Se sella la clave privada + cadena con `cert-seal` (AES-256-GCM, AAD = huella) y se guarda en
   la BD junto a `certificate_ref` (metadatos). Se anota en `audit_log`.
4. El WebView recibe `CertificateSummary` (huella, titular, NIF, caducidad, tipo).

### 3.3 Usar el certificado (mTLS)
`UserPresence::confirm("Presentar modelo 303 2T 2026 con el certificado de …")` → leer
`cert-seal` del Keychain (Touch ID) → desellar en memoria → `reqwest` con identidad rustls
**sólo** hacia hosts permitidos → borrar material → auditoría (host, operación, resultado; nunca
contenido secreto).

### 3.4 Actos con efectos legales (humano en el bucle)
Presentar, firmar, enviar a VERI\*FACTU y emitir factura definitiva siguen siempre:
`borrador → validación local → resumen comprensible → confirmación explícita + Touch ID →
acción → justificante (CSV/PDF) → auditoría`. El asistente IA puede preparar y explicar, nunca
confirmar.

## 4. Modelo de seguridad

- **Datos en reposo:** todo en `taxpain.db` (SQLCipher). Documentos adjuntos: cifrados en BD o
  en ficheros AES-256-GCM con subclave (Fase 3). Claves sólo en el Keychain.
- **Certificado:** nunca sale del dispositivo; nunca llega al WebView, a logs, al LLM ni a
  ningún servidor salvo como identidad TLS frente a hosts AEAT de la allow-list. Test
  `no_secret_leaks` lo comprueba (logs, BD en claro, bytes de red).
- **Red:** sólo HTTPS con verificación de servidor; el cliente AEAT rechaza hosts fuera de la
  allow-list exacta. CSP estricta en el WebView; sin `eval`, sin recursos remotos.
- **Auditoría:** `audit_log` append-only con hash SHA-256 encadenado (`hash = SHA256(prev_hash ‖
  registro canónico)`); verificación en arranque.
- **IA:** el contexto que se envía se construye con lista blanca de campos; nunca certificado,
  contraseñas, PIN ni claves. Proveedor configurable (BYOK) u opción local.
- **Huecos conocidos:** sin protección contra rollback (restaurar una BD antigua + Keychain);
  secretos en memoria mientras se usan.

## 5. Entornos y pruebas

- `cargo test` (núcleo, Linux/macOS), Vitest (frontend), Playwright e2e (Fase 3+).
- Servidor mTLS local con CA propia en tests (`vault/tests`, Fase 1) y mock de la Sede (Fase 6).
- Ninguna prueba automática contacta servidores reales de la AEAT ni APIs de pago.

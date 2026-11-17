# TaxPain — Modelo de datos

Una base de datos cifrada **por empresa** (`taxpain.db`, SQLCipher, ver D3): la principal en
`data/` y cada empresa añadida en `companies/<id>/` (ver `crates/app/src/companies.rs`). Por eso
ninguna tabla lleva `company_id`: lo que hay en un almacén es de una sola empresa y nada se mezcla
entre ellas. El esquema lo crean las migraciones de `crates/vault/migrations/` (`0001`…`0068`),
que son la fuente de verdad; estas tablas se regeneraron a partir de ellas el 2026-10-26.

Convenciones:

- Claves primarias `INTEGER PRIMARY KEY` (rowid) salvo indicación; identificadores externos
  estables en columnas `uuid TEXT UNIQUE` cuando haga falta exportarlos.
- **Importes en céntimos `INTEGER`** (nunca `REAL`). Tipos impositivos en centésimas de punto
  (`2100` = 21 %; sufijo `_bp`).
- Fechas `TEXT` ISO-8601 (`YYYY-MM-DD`; instantes en UTC `YYYY-MM-DDTHH:MM:SSZ`).
- Enumeraciones como `TEXT` con `CHECK (… IN (…))`; tablas `STRICT`.
- Lo que no cabe en columnas (una factura entera, una lectura del asistente, los datos de un
  socio el día del pago) va en una columna `TEXT` con JSON (`data`, `boxes`, `facts`…).
- Columna 🔐 = dato secreto: nunca sale del proceso nativo ni aparece en logs.
- 👤 = la tabla contiene datos personales (RGPD): a tener en cuenta en el registro de
  actividades de tratamiento (`docs/PRIVACIDAD_APP.md`) y en las copias.

La columna «Migración» dice dónde se creó la tabla y, entre paréntesis, las que le añadieron
columnas.

## Identidad, configuración y auditoría

| Tabla | Campos principales | Migración |
|---|---|---|
| `settings` | `key TEXT PK`, `value TEXT` (JSON). Aquí viven los ajustes de facturación, el perfil fiscal del calendario, el asistente, los datos de la empresa… | 0001 |
| `company` 👤 | `nif`, `name`, `legal_form` (`autonomo`/`sl`/`slu`/`sa`/`cb`/`scp`/`cooperativa`/`asociacion`/`otra`), `vat_regime` (`general`/`simplificado`/`recargo_equivalencia`/`rebu`/`agricultura`/`exento`), `iae_epigraphs` (JSON), `cnae`, `territory` (`comun`/`alava`/`bizkaia`/`gipuzkoa`/`navarra`/`canarias`/`ceuta_melilla`), `fiscal_year_start` (`MM-DD`), `redeme`, `sii`, `cash_criterion`, `irpf_method` | 0001 |
| `certificate_ref` 👤 | `fingerprint_sha256 UNIQUE`, `subject_dn`, `issuer_dn`, `serial`, `valid_from`, `valid_to`, `kind` (`persona_fisica`/`representante_pj`/`otro`), `issuer_kind`, `holder_name`, `holder_nif`, `entity_nif`, `entity_name`, `key_algorithm`, `created_at`, `last_used_at` | 0001 |
| `certificate_secret` | `certificate_id PK/FK`, 🔐 `sealed BLOB` (AES-256-GCM con `cert-seal`, AAD = huella; contenido: PKCS#8 + cadena DER) | 0001 |
| `audit_log` | `seq PK`, `at`, `actor` (`user`/`system`/`assistant`), `action`, `subject`, `details` (JSON sin secretos), `prev_hash`, `hash` (SHA-256 hex). Sólo se añade; la cadena se verifica desde `seq = 1` | 0001 |
| `reminder_sent` | `reminder_key PK`, `sent_at`: avisos de plazos ya mostrados | 0002 |
| `app_secret` | `name PK`, 🔐 `value` (clave de API del asistente, sellada), `updated_at` | 0010 |
| `backup_key` | `kek`, `salt`, `log_n`, `r`, `p` (scrypt), `created_at`: clave de las copias automáticas, sellada | 0039 |
| `fiscal_year` | `year PK`, `closed`, `closed_at`, `start_date`, `end_date` (ejercicio no natural) | 0005 (0021) |

## Terceros y facturación

| Tabla | Campos principales | Migración |
|---|---|---|
| `contact` 👤 | `kind` (`cliente`/`proveedor`/`ambos`), `tax_id`, `country` (ISO 3166-1), `name`, `data` (JSON: dirección, email, tipo de identificación AEAT…), `created_at` | 0003 |
| `invoice_series` | `code UNIQUE`, `data` (JSON: prefijo, formato, reinicio anual, rectificativa), `created_at`. La serie `HIST` es la de las facturas importadas de un libro | 0003 |
| `invoice_counter` | `series_id`, `year`, `last_seq`: numeración correlativa por serie y año | 0003 |
| `document` | `uuid UNIQUE`, `kind` (`pdf`/`facturae`/`recibida`/`asiento`/`libro_recibidas`…), `filename`, `mime`, `sha256`, `size`, 🔐 `content BLOB` (el fichero, dentro de la BD cifrada), `created_at` | 0003 |
| `invoice_issued` 👤 | `series_id`, `year`, `seq`, `number UNIQUE`, `issue_date`, `kind` (F1…R5), `contact_id`, `recipient_name`, `recipient_tax_id`, `base`, `vat`, `surcharge`, `withholding`, `total`, `payable`, `status` (`emitida`/`anulada`), `rectifies_id`, `data` (JSON: factura y totales, fuente de verdad; con `fx` en moneda extranjera), `pdf_document_id`, `facturae_document_id`, `ubl_document_id`, `exported_path`, `origin`, `book_import_id` (si vino de un libro o de una lectura) | 0003 (0013, 0036, 0040) |
| `verifactu_record` | `invoice_id`, `record_type` (`alta`/`anulacion`), `emisor_nif`, `num_serie`, `fecha`, `huella`, `prev_huella` (cadena SHA-256), `generated_at`, `xml`, `qr_url`, `send_status` (`pendiente`/`correcto`/`aceptado_con_errores`/`incorrecto`), `error_code`, `error_text`, `csv`, `sent_at`, `subsanacion`, `rechazo_previo`, `resubmits`, `superseded_by` | 0003 (0011) |
| `invoice_received` 👤 | `document_id`, `file_sha256`, `filename`, `source` (`manual`/`carpeta`/…), `status` (`pendiente`/`revisada`/`descartada`), `extraction` (JSON OCR/LLM con sus avisos), `supplier_nif`, `supplier_name`, `number`, `issue_date`, `base`, `vat`, `withholding`, `total`, `deductible_pct`, `capital_good`, `nature`, `vat_regime` (`interior`/`intracomunitaria`/`isp`/…), `supplier_country`, `self_assessed_rate`, `intra_goods`, `cash_basis`, `import_dua`, `prorata_use`, `property`, `posted`, `book_import_id`, `fx` (JSON: moneda, tipo e importes en la moneda), `reviewed_at`, `created_at`. Índice único (`supplier_nif`, `number`) entre las revisadas | 0004 (0008, 0012, 0022, 0030, 0032, 0036, 0040, 0043) |
| `quote` 👤 | `kind` (presupuesto/proforma), `number`, `year`, `seq`, `issue_date`, `valid_until`, `status`, `recipient_name`, `total`, `draft` (JSON), `language`, `invoice_id` (si se convirtió), `created_at`, `updated_at` | 0041 |
| `book_import` | `kind` (`emitidas`/`recibidas`), `source` (`libro`/`lectura`), `filename`, `sha256`, `invoices`, `created_at`: lote de un libro del gestor o de lecturas, que se puede deshacer | 0040 |
| `ai_reading` | `file_sha256 PK`, `reading` (JSON: la lectura del asistente o de Claude Code), `full`, `created_at` | 0038 |

## Cobros, pagos, banco y Hacienda

| Tabla | Campos principales | Migración |
|---|---|---|
| `invoice_installment` | `kind` (`emitida`/`recibida`), `invoice_id`, `seq`, `due_date`, `amount`: vencimientos de una factura | 0017 |
| `settlement` | `kind`, `invoice_id`, `seq` (vencimiento), `date`, `account` (tesorería), `entry_id`, `created_at`, `method` (forma de ese cobro o pago; «efectivo» es el metálico del 347, NULL si no se dijo): cobro o pago registrado | 0015 (rehecha en 0017, 0053) |
| `party_terms` | `account PK` (cuenta del tercero), `days`, `pay_day`, `method`, `treasury`, `iban`, `mandate_ref`, `mandate_date`, `key_190` (clave del 190 del proveedor: G01…G04, H01…H04; NULL = G.01), `updated_at`: condiciones de cobro o pago y mandato SEPA | 0017 (0018, 0054) |
| `inmueble_arrendado` | `nif PK` (arrendador), `situacion` (1-4), `ref_catastral`, `via`, `numero`, `municipio`, `provincia`, `codigo_postal`, `exonerado_hasta` (acreditación para no retener, art. 75.3.g RIRPF), `updated_at`: inmueble del 180 | 0092 |
| `certificado_residencia` | `id`, `nif`, `nombre`, `pais`, `expedido`, `valido_hasta` (un año), `convenio`, `documento`, `notas`, `created_at`: certificado de residencia fiscal de un no residente | 0092 |
| `renta_no_residente` | `id`, `fecha` (devengo), `nif`, `nombre`, `pais`, `naturaleza` (`interes`/`canon`/`servicio`/`otros`), `bruto`, `tipo_bp`, `retencion`, `regimen` (`general`/`convenio`/`exenta`), `exencion`, `concepto`, `recibida_id`, `created_at`: rentas del 216/296 (los dividendos salen de `dividend_line`) | 0092 |
| `party_counterpart` | `account PK`, `counterpart`, `updated_at`: contrapartida habitual de un tercero | 0026 |
| `remittance` | `kind` (cobros/pagos), `reference`, `bank_iban`, `bridge`, `charge_date`, `total`, `file` (SEPA XML), `bank_tx_id`, `created_at` | 0018 |
| `remittance_item` | `remittance_id`, `kind`, `invoice_id`, `seq`, `amount`, `returned_on` | 0018 |
| `bank_account` | `iban`, `bank_code`, `number` (clave: entidad+oficina+cuenta como en su Norma 43, IBAN, `tarjeta:<entidad>:<últimos>`…), `account` (572x euros, 573x otra divisa, 5201x tarjeta de crédito, vacía en tarjeta de débito), `name`, `currency` (ISO 4217), `kind` (`cuenta`/`tarjeta_credito`/`tarjeta_debito`/`monedero`), `bank`, `holder`; `UNIQUE(number, currency)` | 0005 (0044) |
| `bank_tx` | `bank_account`, `op_date`, `value_date`, `amount` (+ ingreso, − cargo, en la divisa de la cuenta), `concept_code`, `description`, `import_hash UNIQUE`, `balance` (saldo tras el movimiento, si el extracto lo da: evita duplicados entre extractos que se solapan), `source` (fichero de origen), `entry_id`, `matched_kind` (`emitida`/`recibida`/`cuenta`), `matched_id`, `matched_seq`, `created_at` | 0005 (0017, 0044) |
| `bank_rule` | `pattern`, `sign`, `account`, `hits`, `updated_at`: reglas de conciliación aprendidas | 0024 |
| `tax_payment` | `model`, `year`, `period`, `date`, `account` (4750/4751/473/4752), `treasury`, `amount`, `entry_id`, `bank_tx`, `created_at`; único por modelo y periodo | 0023 |

## Contabilidad

| Tabla | Campos principales | Migración |
|---|---|---|
| `account` | `code PK` (PGC, p. ej. `4720001`), `name`, `created_at` | 0005 |
| `third_party_account` | `tax_id`, `role`, `account`, `name`: subcuenta 430x/400x de cada tercero | 0005 |
| `journal_entry` | `fiscal_year`, `number` (correlativo por ejercicio), `date`, `concept`, `origin_kind` (`FacturaEmitida`/`FacturaRecibida`/`Banco`/`Amortizacion`/`LiquidacionIva`/`Manual`/`Importado`/…), `origin_id`, `notes`, `created_at`. Un documento se contabiliza una vez por ejercicio (índice único por origen) | 0005 (0027) |
| `journal_line` | `entry_id`, `account`, `debit`, `credit` (uno de los dos a 0), `description`. Invariante: Σdebe = Σhaber por asiento | 0005 |
| `line_tick` | `line_id PK`, `ticked_at`: punteo de apuntes | 0016 |
| `entry_document` | `entry_id`, `document_id`: justificantes adjuntos a un asiento (se van con él) | 0027 |
| `entry_template` | `name`, `concept`, `lines` (JSON), `every_months`, `next_date`, `counter`, `created_at`: asientos periódicos | 0014 |
| `import_batch` | `source` (`suenlace`/`tabla`), `filename`, `sha256`, `entries`, `created_at`: lote de asientos importados (`origin_kind = Importado`), que se puede deshacer | 0019 |
| `fixed_asset` | `name`, `account`, `depreciation_acc`, `expense_acc`, `acquired_on`, `cost`, `residual`, `rate_bp`, `method`, `tax_rate_bp`, `disposed_on`, `sale_price`, `disposal_entry`, `prior_depreciation` y `prior_until` (amortizado antes de TaxPain y hasta qué día: la amortización sigue desde el día siguiente), `opening_id`, `created_at` | 0005 (0028, 0033, 0049) |
| `opening_balance` | `fiscal_year`, `date`, `entry_id`, `data` (JSON: el borrador del asistente de saldos iniciales, para deshacerlo), `created_at` | 0049 |
| `opening_item` 👤 | `opening_id`, `kind` (`emitida`/`recibida`), `account`, `tax_id`, `name`, `number`, `issue_date`, `due_date`, `amount`: cobros y pagos pendientes de antes de TaxPain; en la cartera (`invoice_installment`, `settlement`, `bank_tx.matched_id`, `remittance_item`) van con el id en negativo | 0049 |
| `leasing` / `leasing_installment` | contrato (`data` JSON, `asset_id`, `start_entry`) y sus cuotas (`n`, `entry_id`) | 0031 |
| `prestamo` / `prestamo_cuota` / `prestamo_devengo` | préstamo concedido o recibido (contraparte, vínculo, condiciones, subcuentas, retención, estado y asiento del alta), cuotas registradas o ya contabilizadas fuera (importes y `entry_id`, NULL si fuera) e intereses devengados a una fecha | 0067 |
| `cost_center` / `cost_rule` / `line_cost` / `budget` | centros de coste (`code`, `name`, `parent`), reparto por prefijo de cuenta (`pct_bp`), imputación por apunte y presupuesto por año, cuenta, centro y mes | 0020 (0034) |
| `cost_redistribution` | `source`, `target`, `pct_bp`: reparto de un centro a otros | 0034 |
| `model_adjustment` | `model`, `year`, `period`, `box`, `amount`, `updated_at`: casillas de un modelo fijadas a mano | 0029 |
| `gestor_import` | lote de un ejercicio entero importado del gestor: `fiscal_year`, `source` (`diario`/`resumido`/`suenlace`), `filename`, `sha256 UNIQUE` (no se importa dos veces), `entries`, `lines`, `accounts_created`, ids de la apertura, la regularización, el cierre y la apertura del siguiente, `closed_year` (el lote dejó el ejercicio cerrado) | 0050 |
| `crypto_asset` | `ticker UNIQUE`, `name`, `account` (subcuenta 309x/209x asignada al contabilizar) | 0051 |
| `crypto_platform` | `name UNIQUE`, `kind` (`exchange`/`monedero`), `country`, `abroad` (custodio que no presenta el 172/173: cuenta para el 721), `account` (contrapartida de los euros en esa plataforma) | 0051 |
| `crypto_op` | `date`, `time`, `kind` (`compra`/`venta`/`permuta`/`traspaso`/`comision`/`recompensa`/`perdida`/`pago`/`cobro`), `platform_id`, `to_platform_id`, `ticker`, `qty` (texto decimal, hasta 18 decimales: nunca `REAL`), `ticker2`/`qty2` (recibido en una permuta), `eur` y `fee_eur` (céntimos), `ref_price` (texto), `note`, `source` (`manual`/`hoja`/`cointracking`/`kraken`), `fingerprint UNIQUE` (deduplica importaciones), `entry_id` | 0051 |
| `crypto_price` | `date`, `ticker`, `price` (texto decimal, 8 decimales), `source`; PK (`date`, `ticker`): precios de cierre tecleados o importados de una hoja de saldos | 0051 |
| `security_account` | cuenta de bróker: `broker` (`xtb`/`ibkr`/`otro`), `name UNIQUE`, `number`, `currency`, `country` y `abroad` (depositario en el extranjero: cuenta para el 720), `account` (subcuenta del efectivo), `flex_query` (Query ID de IBKR; el token va en `app_secret`), `last_sync`, `last_sync_note` | 0063 |
| `security` | valor (grupo homogéneo): `isin` (único si lo hay; junta el mismo valor en dos brókeres), `ticker`, `name`, `currency`, `kind` (`accion`/`etf`/`fondo`/`bono`/`cfd`/`otro`), `country`, `account` | 0063 |
| `security_op` | `account_id`, `to_account_id`, `security_id`, `date`, `time`, `kind` (`compra`/`venta`/`dividendo`/`retencion`/`interes`/`comision`/`impuesto`/`traspaso`/`split`/`deposito`/`retirada`), `qty` y `price` (texto decimal), `currency`, `amount` y `fee` (céntimos en la divisa), `fx` (euros por unidad, texto), `eur` y `fee_eur`, `country` (de la retención), `note`, `source` (`manual`/`ibkr`/`xtb`/`xtb-posiciones`), `fingerprint UNIQUE` (identificador del bróker), `entry_id` | 0063 |
| `security_price` / `security_fx` | precios de cierre por (`date`, `security_id`) en la divisa del valor y cambios del bróker por (`date`, `currency`) en euros por unidad | 0063 |
| `security_import` | cada importación o sincronización: `account_id`, `kind` (`flex`/`api`/`xtb-pdf`), `file_sha256`, periodo de las operaciones en efectivo, `ops_new`, `ops_dup` | 0063 |
| `foreign_asset_info` | datos de un bien en el extranjero para el 720/721 que la contabilidad no tiene: `source` (`banco`/`valores`/`cripto`) y `ref_id` (UNIQUE), `condition` (condición del declarante), `share_bp`, `opened_on`, `closed_on`, `entity_name`, `entity_tax_id`, `bic`, `address`, `country`, `abroad` (fuerza que una cuenta bancaria sea o no del extranjero) | 0087 |
| `foreign_asset_filing` | lo declarado en un 720/721: `model`, `year`, `block` (`cuentas`/`valores`/`monedas`) UNIQUE, `value` y `avg_balance` (céntimos), `assets` (JSON con IBAN, ISIN o moneda@plataforma), `origin` (`borrador`/`tecleado`): base de la regla de los 20.000 € | 0087 |

## Personas (nóminas, socios, viajes)

| Tabla | Campos principales | Migración |
|---|---|---|
| `employee` 👤 | `nif`, `name`, `province`, `birth_year`, `family`, `spouse_nif`, `disability`, `contract`, `mobility`, `children_*`, `active`, `administrator`, `ss_regime`, `updated_at`: datos para la retención y el 190 | 0025 (0042) |
| `payslip` 👤 | `employee_id`, `date`, `gross`, `irpf`, `ss_employee`, `ss_company`, `entry_id`, `director_subkey`, `created_at` | 0025 (0042) |
| `partner` 👤 | `nif`, `name`, `province`, `address`, `pct`, `joined_on`, `left_on`, `created_at`: socios de una entidad en atribución de rentas (184) | 0035 |
| `shareholder` 👤 | `nif`, `name`, `address`, `province`, `country`, `pct`, `legal_person`, `resident`, `exempt`, `active`, `created_at`: socios de la SL (dividendos, 123/193) | 0042 |
| `dividend_payment` / `dividend_line` 👤 | pago (`date`, `gross`, `withheld`, `treasury`, `entry_id`, `allocation_year`: ejercicio de la aplicación del resultado de la que sale, NULL si no se sabe) y, por socio, los datos del día del pago (`nif`, `name`, `pct`, `gross`, `rate_bp`, `withheld`…) | 0042 (0052) |
| `share_movement` 👤 | libro registro de socios: `date`, `kind` (`constitucion`/`ampliacion`/`reduccion`/`transmision`/`carga`/`cancelacion_carga`), `from_nif`/`from_name` (transmitente o titular), `to_nif`/`to_name`/`to_address`/`to_nationality` (adquirente, suscriptor o beneficiario de la carga), `number_from`…`number_to`, `unit_nominal`, `price`, `charge`, `title`, `notary`, `protocol`, `title_date`, `notes`. La situación sale de reproducirlos en orden | 0068 |
| `sole_partner_contract` 👤 | libro de contratos del socio único (art. 16 LSC): `date`, `object`, `amount`, `conditions`, `document`, `source UNIQUE` (propuesta de la que sale: `nominas:<año>:<nif>`, `dividendo:<id>`) | 0068 |
| `minute` 👤 | libro de actas: `number` (correlativo del ejercicio, al aprobar), `fiscal_year`, `date`, `place`, `kind` (`junta_ordinaria`/`junta_extraordinaria`/`junta_universal`/`socio_unico`/`consejo`/`administrador`), `template`, `attendees`, `chair`, `secretary`, `agreements`, `status` (`borrador`/`aprobada`), `approved_at`; `UNIQUE(fiscal_year, number)` | 0068 |
| `travel_expense` 👤 | `date`, `nif`, `name`, `employee_id`, `origin`, `destination`, `reason`, `labor`, `abroad`, `overnight`, `days`, `km`, `km_amount`, `meals`, `lodging`, `tolls_parking`, `transport`, `exempt`, `taxable`, `counter_account`, `entry_id`, `created_at` | 0042 |

## AEAT y documentos de la empresa

| Tabla | Campos principales | Migración |
|---|---|---|
| `tax_return` | `model`, `year`, `period`, `environment` (pruebas/producción), `declaration` (JSON), `result`, `boxes` (JSON), `file_sha256`, `file` (fichero BOE), `csv`, `receipt_number`, `receipt`, `presented_at`: lo presentado desde TaxPain | 0006 |
| `tax_return_receipt` | `tax_return_id`, `document_id`, `downloaded_at`: justificante PDF descargado | 0007 |
| `aeat_notice` 👤 | `kind`, `state`, `role`, `subject`, `available_at`, `holder_nif`, `environment`, `seen_at`, `accessed_at`, `metadata` (JSON), `document_id`, `certification_id`: notificaciones de la AEAT y la DEHú | 0009 |
| `filed_model` | `model`, `year`, `period`, `filename`, `file_sha256`, `source` (`fichero`/`pdf`/`manual`), `boxes` (JSON), `document_id`, `imported_at`: modelos del gestor importados para compararlos | 0036 |
| `company_document` 👤 | `kind` (escritura, tarjeta NIF, 036, ROI, cuentas depositadas…), `title`, `filename`, `file_sha256`, `document_id`, `facts` (JSON: NIF, domicilio, capital, administrador…), `doc_date`, `imported_at` | 0037 |

## Asistente

Las conversaciones con la IA (generales o sobre un tema de TaxPain) y lo que el usuario les añade.
Lo que se envía se anonimiza de nuevo en cada pregunta; aquí está en claro (dentro del almacén
cifrado), como se ve en el Mac.

| Tabla | Campos principales | Migración |
|---|---|---|
| `assistant_conversation` 👤 | `kind` (`general`, `ahorro`, `ahorro_plan`, `hallazgo`, `modelo`; lo valida la app), `topic_key` (el tema con su ejercicio: `capitalizacion@2026`, `2026`, `aeat/cruces.390_303/2024/<huella del título>`, `303/2026/3`; vacía en las generales; única por `kind` salvo en las generales), `topic` (JSON), `title`, `snapshot` (JSON: lo que se envió del tema en la última pregunta, su cabecera, la fecha y los terceros que nombra, que se tapan al reenviarlo), `created_at`, `updated_at`. El `id` no se reutiliza (`AUTOINCREMENT`): una respuesta que llega tarde nunca acaba en otra conversación | 0061 |
| `assistant_message` 👤 | `conversation_id`, `seq`, `role` (`user`/`assistant`), `content` (con los datos restaurados), `notes` (JSON de las respuestas: modelo, datos sustituidos, herramientas, normas leídas, citas sin comprobar), `created_at`. Se borran con su conversación | 0061 |
| `assistant_material` 👤 | `conversation_id`, `kind` (`dato`/`documento`), `content` (el dato o el texto del documento, como mucho 15.000 caracteres), `filename`, `file_sha256`, `file_size`, `read_by` (`mac`/`ia`), `truncated`, `created_at`: lo que añade el usuario (nunca el fichero; el nombre no se envía). Se borra con su conversación | 0061 |
| `patrimonial_marca` | `year`, `cuenta` (subcuenta de los grupos 2 y 5 o cuenta de tres cifras de los 3 y 4), `clase` (`afecto`, `no_afecto`, `valores`, `participacion`, `tesoreria`), `motivo`, `updated_at`: clase de una partida del activo para el test de entidad patrimonial (art. 5.2 LIS) desde ese ejercicio; vale para los siguientes mientras no haya otra | 0073 |
| `patrimonial_ejercicio` | `year`, `tesoreria_afecta` y su motivo, `beneficios_actividad` (NULL: sin confirmar) y su motivo, `prestamos_participativos` (patrimonio neto mercantil), `dominante` | 0073 |
| `patrimonial_venta` | `fecha`, `importe`, `motivo`, `created_at`: venta de un elemento afecto o de una participación que no computa; su dinero no computa ese ejercicio ni los dos siguientes | 0073 |

## Lo que el encargo nombraba y vive en otro sitio

- `fiscal_profile`, `deadline`, `notification`: el perfil fiscal se guarda en `settings` y los
  plazos se calculan cada vez desde él (`taxpain-core::calendar`); los avisos mostrados quedan en
  `reminder_sent` y las notificaciones de la AEAT en `aeat_notice`.
- `tax_model_period`: lo presentado está en `tax_return` (desde TaxPain) y en `filed_model`
  (lo que presentó el gestor); el estado de cada periodo se deduce de ellas y del calendario.
- `invoice_line`, `reconciliation`, `depreciation`: las líneas van dentro de
  `invoice_issued.data` (D10), la conciliación son las columnas `matched_*` y `entry_id` de
  `bank_tx` y las cuotas de amortización son asientos (`origin_kind = Amortizacion`) sobre
  `fixed_asset`.

## Invariantes comprobados por tests

- Ningún importe se almacena como `REAL`; las cantidades de criptomonedas van como texto decimal.
- `certificate_secret.sealed` nunca se descifra sin `UserPresence::confirm` previo.
- `audit_log` verificable: cadena de hashes íntegra desde `seq = 1`; cada operación con efecto
  (importar, presentar, usar o borrar un certificado, enviar un fichero a la IA) deja entrada.
- Asientos cuadrados; sumas y saldos cuadra; balance: Activo = Patrimonio neto + Pasivo.
- Nada de una empresa aparece en el almacén de otra (una BD por empresa).

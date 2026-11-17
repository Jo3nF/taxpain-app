---
description: Lee la carpeta de la empresa (facturas, libros, escrituras, modelos…) y deja taxpain-lecturas.json para importarlo en TaxPain sin pagar API
argument-hint: <ruta de la carpeta de la empresa>
---

Vas a leer la carpeta de la empresa del usuario para TaxPain, con tu suscripción de Claude Code
(sin la API). TaxPain importará después lo que escribas; el usuario lo revisa en la app antes de
confirmar nada.

Carpeta: $ARGUMENTS

## Reglas

- **No inventes nada.** Si un dato no aparece o no lo lees con claridad, pon `null`.
- **No modifiques, muevas ni borres** ningún fichero de la carpeta. Sólo escribes
  `taxpain-lecturas.json` y `taxpain-informe.md` en su raíz.
- **No copies datos de la carpeta al repositorio** ni a ningún otro sitio.
- Los NIF, nombres e importes van **tal cual** aparecen (aquí no hay anonimización).
- Importes con punto decimal (`1234.56`); fechas `AAAA-MM-DD`.

## Pasos

1. Lista todos los ficheros (subcarpetas incluidas, sin ocultos ni `taxpain-lecturas.json`).
   Si ya existe `taxpain-lecturas.json`, cárgalo y **sigue donde se quedó** (no repitas lo leído).
2. Salta lo que TaxPain ya lee exactamente sin ayuda: ficheros de modelos de la AEAT en texto
   (empiezan por `<T`), extractos Norma 43 (`.n43`, primera línea empieza por `11`) y
   `SUENLACE.DAT`.
   Los **extractos bancarios** de cualquier formato (Norma 43, camt, OFX, QIF, MT940, Excel,
   CSV, tablas HTML guardadas como `.xls`, PDF) los lee TaxPain con sus lectores; compruébalo con
   `cargo run -q -p taxpain-app --example extractos -- "<fichero>"`. Si sale «CUADRA» o «SIN
   SALDOS», basta con `{"tipo": "extracto_bancario", "descripcion": …}`. Sólo si da ERROR o «NO
   CUADRA» porque el lector no lo entiende, añade a la lectura los datos para leerlo (ver
   `"hoja"` y `"extracto"` abajo) y vuelve a comprobarlo.
3. Para cada **hoja de cálculo** (`.xlsx`, `.xls`, `.ods`, `.csv`), vuelca su contenido con:
   `cargo run -q -p taxpain-app --example lecturas -- hoja "<fichero>" 60`
   (filas numeradas y columnas por letra). Di qué es y, si es un diario o un libro de facturas,
   la fila de títulos, la primera fila de datos y qué letra es cada campo.
4. Para cada **PDF o imagen** (`.pdf`, `.jpg`, `.png`, `.heic`), ábrelo con la herramienta de
   lectura y extrae sus datos según su tipo.
5. Ve añadiendo cada lectura a `taxpain-lecturas.json` (guárdalo cada 10 ficheros, para poder
   seguir si se corta) con este formato:

```json
{
  "version": 1,
  "generado_por": "Claude Code",
  "lecturas": [
    { "fichero": "ruta/relativa/a/la/carpeta.pdf", "lectura": { "tipo": "…", "…": "…" } }
  ]
}
```

   Cada `lectura` es un objeto con estas claves (omite las que no apliquen):
   - `"tipo"`: uno de diario, libro_emitidas, libro_recibidas, sumas_saldos, extracto_bancario,
     factura_recibida, factura_emitida, modelo_aeat, escritura_constitucion, escritura_poderes,
     escritura_modificacion, estatutos, tarjeta_nif, censal_036, alta_roi, certificado_aeat,
     certificado_seguridad_social, alta_seguridad_social, registro_mercantil, cuentas_anuales,
     iae, acta_junta, contrato_alquiler, seguro, licencia, nomina, otro.
     **factura_emitida** si el emisor es la propia empresa (su NIF); **factura_recibida** si la
     empresa es el destinatario.
     Casos que se confunden: la **certificación del depósito de las cuentas anuales** que expide
     el Registro Mercantil es **cuentas_anuales** (con `"registro": {"tramite":
     "deposito_cuentas", …}`), no registro_mercantil, que es para las notas simples y las
     certificaciones de la hoja (cargos, vigencia). Una escritura de modificación (ampliación de
     capital o de objeto, elevación a público de acuerdos) cita siempre la de constitución: es
     **escritura_modificacion**, aunque vaya detrás del «justificante de expedición de copia»
     del notario. El justificante de presentación de un 036 o 037 que llega por las
     notificaciones de la Sede («Comunicación - Documento.pdf») es **censal_036**: su fecha y su
     causa (casilla 1xx) dicen desde cuándo cambian las obligaciones. Si el nombre del fichero o
     el título del documento dicen claramente lo que es, eso manda.
   - Factura sin fecha (un recibo de una web extranjera, por ejemplo): deja `"fecha": null`
     (TaxPain la sitúa por la carpeta del año y del trimestre en que está) y pon las pistas en la
     descripción.
   - `"descripcion"`: una frase con lo que es (periodo, emisor…).
   - `"hoja"` (diario, libros de facturas o extractos que TaxPain no reconoce):
     `{"fila_titulos": n (0 si no hay), "primera_fila": n, "columnas": {campo: letra}}`. Campos
     del diario: fecha, asiento, documento, concepto, cuenta (o `cuenta_debe` y `cuenta_haber` si
     la cuenta va en dos columnas), nombre_cuenta, debe, haber. De los libros: fecha,
     fecha_registro, serie, numero, nif, nombre, pais, base, tipo_iva, cuota_iva, recargo,
     retencion, total. De los extractos: fecha, fecha_valor, concepto (y concepto_2,
     beneficiario u observaciones), importe (con signo) o cargo y abono, saldo, divisa. Si el
     libro tiene varias hojas con los mismos títulos (una por mes, o las de adquisiciones, sujeto
     pasivo y no deducibles), las columnas valen para todas: TaxPain las lee todas. Las fechas
     sin año (dd/mm) toman el del periodo de la cabecera.
   - `"extracto"` (sólo extractos en PDF o en foto que TaxPain no reconoce):
     `{"cuenta": {"iban", "banco", "titular", "divisa", "clase" (cuenta, tarjeta_credito,
     tarjeta_debito o monedero), "desde", "hasta", "saldo_inicial", "saldo_final"},
     "movimientos": [[fecha, fecha_valor, concepto, importe, saldo], …]}` con **todos** los
     movimientos en el orden del documento, el importe con signo (negativo si sale dinero) y el
     saldo después de cada movimiento (`null` si no lo pone). Varias cuentas o divisas en el
     mismo fichero: `"extractos": [ {"cuenta": …, "movimientos": …}, … ]`. TaxPain lo comprueba
     con los saldos antes de importarlo.
   - `"factura"` (facturas): `{"nif_emisor", "nombre_emisor", "pais_emisor" (código ISO
     3166-1 de dos letras, como `US`; sólo si el emisor no es de España), "nif_destinatario",
     "nombre_destinatario", "numero", "fecha", "base", "iva", "retencion" (positiva), "total",
     "moneda" (código ISO 4217, como `USD`; sólo si no es EUR), "tipos": [{"base", "tipo",
     "cuota"}]}`. En `"nombre_destinatario"` va a quien está hecha la factura aunque no tenga NIF
     (su nombre o, si sólo sale eso, su correo): con él la revisión ve las facturas a nombre de
     otro o del administrador.
   - `"modelo"` (modelos de la AEAT en PDF): `{"modelo": "303", "ejercicio": 2025,
     "periodo": "1T" (o 0A, 1P, 01…), "casillas": {"01": "1234.56", …}, "presentado":
     "AAAA-MM-DD"}` con todas las casillas que tengan importe y la fecha de presentación del
     justificante (si aparece). En los resúmenes anuales (190, 180, 193, 347, 349) añade
     `"registros": [{"nif", "nombre", "clave", "importe", "retencion"}]` con **una línea por
     perceptor o tercero** (clave de percepción del 190 como `G` o `E`, clave `A`/`B` del 347,
     `E`/`A`/`S`/`I`/`T` del 349): con ellas la revisión cruza el modelo NIF a NIF con las
     facturas. Si es una declaración complementaria o sustitutiva, añade `"tipo":
     "complementaria"` o `"sustitutiva"`: en los resúmenes anuales la complementaria sólo trae
     lo que faltaba y se suma a la anterior.
   - `"registro"` (justificantes del Registro Mercantil y cuentas anuales depositadas):
     `{"tramite": "deposito_cuentas" o "legalizacion_libros", "ejercicio", "presentado"
     (fecha de presentación en el Registro), "resultado" (resultado del ejercicio de la cuenta de
     pérdidas y ganancias), "cifra_negocios"}`.
   - `"acta"` (actas de la junta o decisiones del socio único que aprueban las cuentas o mueven
     reservas): `{"aprueba_cuentas" (true si aprueba las cuentas y la aplicación del resultado;
     false si es otra decisión, como un dividendo con cargo a reservas o una dotación posterior),
     "ejercicio" (el de las cuentas que se aprueban o al que se refiere), "fecha", "resultado" (el
     que se reparte; negativo si son pérdidas; `null` si no se reparte un resultado),
     "reserva_legal", "reservas_voluntarias", "reserva_nivelacion", "reserva_capitalizacion",
     "dividendos", "compensacion_perdidas", "remanente"}` con lo que va a cada partida del
     ejercicio del acta (no la columna del año anterior; `null` si no va nada; negativo si sale de
     ella): con ellas la revisión comprueba que se dotan las reservas que pide el Impuesto sobre
     Sociedades y desde cuándo corre el plazo del depósito de las cuentas.
   - `"nomina"` (nóminas): `{"nif_trabajador", "nombre_trabajador", "periodo" (AAAA-MM),
     "bruto" (total devengado), "retencion" (la del IRPF, positiva), "tipo_retencion" (el
     porcentaje del IRPF)}`: con ellas la revisión ve si el administrador cobra como empleado.
   - `"empresa"` (documentos de la empresa): `{"nif", "name", "street", "post_code", "town",
     "constitution_date", "capital", "cnae", "iae", "vat_number", "administrator", "notary",
     "protocol", "registry", "date"}`.

6. Comprueba el fichero y corrige lo que salga mal hasta que no haya problemas:
   `cargo run -q -p taxpain-app --example lecturas -- comprobar "<carpeta>"`
7. Escribe `taxpain-informe.md` en la carpeta. Empieza por la revisión de TaxPain, que hace sola
   las comprobaciones con tus lecturas (copias, facturas repetidas, totales que no cuadran, NIF,
   numeración de las emitidas, libro frente a PDF, modelos que faltan, el 303 frente a las
   facturas, el Impuesto sobre Sociedades…):
   `cargo run -q -p taxpain-app --example revision -- "<carpeta>" --nif <NIF> --nombre "<nombre>"`
   (con `--sl` si es una sociedad limitada, para saber qué modelos tocaban). Debajo, añade lo
   que hayas visto al leer y la revisión no ve: los ficheros que no has podido leer y por qué,
   y lo raro de un documento concreto.
8. Termina con un resumen corto y dile al usuario que abra TaxPain → Importar → Carpeta de la
   empresa → «Volver a leer»: las lecturas salen con la etiqueta «IA», sin coste de API, y él
   decide qué importar. En la pestaña «Revisión» tiene la misma revisión, con filtros y para
   guardarla en PDF o Excel.

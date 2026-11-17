# Leer la carpeta de la empresa con Claude Code (con la suscripción, sin API)

TaxPain puede usar el asistente con una clave de API (se paga aparte, por uso). Si ya tienes
una suscripción de Claude (Pro o Max), puedes hacer la lectura con **Claude Code**, que va con
la suscripción: Claude Code lee la carpeta y deja en ella `taxpain-lecturas.json`; TaxPain lo
carga al leer la carpeta, sin llamar a ninguna API.

> La suscripción de Claude no se puede conectar dentro de TaxPain (las condiciones de Anthropic
> no permiten que otras apps la usen como si fuera una clave de API). Por eso lo hace Claude
> Code, que es el programa oficial.

## Una vez

```sh
npm install -g @anthropic-ai/claude-code
cd ~/taxpain
claude            # la primera vez pide entrar con tu cuenta de Claude (la de la suscripción)
```

## Cada vez

1. En la Terminal: `cd ~/taxpain && claude`
2. Escribe: `/leer-carpeta "/Users/tu-usuario/Documents/Empresa"` (la ruta de tu carpeta; la
   puedes arrastrar desde el Finder a la Terminal).
3. Claude Code lee cada fichero (te pedirá permiso para ejecutar la herramienta
   `cargo run … --example lecturas`), escribe `taxpain-lecturas.json` y `taxpain-informe.md`
   en la carpeta y comprueba que todo se puede importar.
4. En TaxPain: Importar → Carpeta de la empresa → «Volver a leer». Lo leído por Claude Code
   sale con la etiqueta «IA». Revisa e importa.
5. Importar → Revisión → «Revisar la carpeta»: lo que no cuadra (de la empresa y de la
   gestoría), lo que falta y lo que hay que pedir, con el informe en PDF, Excel o Markdown. Es
   la misma revisión que encabeza `taxpain-informe.md`.

## Qué sale de tu Mac

Lo que Claude Code lee (los ficheros de la carpeta) va a Anthropic como en cualquier uso de
Claude Code. TaxPain no envía nada. Si una carpeta es muy grande, Claude Code va guardando y
puede seguir donde lo dejó si lo vuelves a lanzar.

# Servidor MCP: que Claude consulte TaxPain

`taxpain-mcp` es un programa pequeño que habla MCP (Model Context Protocol) por la entrada y la
salida estándar. Lo arranca tu cliente de Claude (Claude Desktop o Claude Code) en tu Mac, y deja
que Claude consulte los datos de TaxPain con las mismas funciones que la app, en vez de leer PDF
y hojas de cálculo. No abre ningún puerto ni se conecta a internet.

**Viene apagado.** Claude sólo ve las empresas en las que lo actives, una por una.

## Instalarlo

Todavía no viene dentro de la app: se compila desde el código.

```sh
cd ~/taxpain
cargo build --release -p taxpain-mcp     # deja target/release/taxpain-mcp
```

Compílalo con la misma versión del código que la app que usas (y con la misma variable
`TAXPAIN_KEYCHAIN`, si compilas la app con ella): si la base de datos es de otra versión, el
programa no la abre y dice qué hay que actualizar. No la migra ni crea nada.

En **Claude Desktop**: Ajustes → Desarrollador → Editar configuración, y en
`claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "taxpain": { "command": "/Users/tu-usuario/taxpain/target/release/taxpain-mcp" }
  }
}
```

En **Claude Code**:

```sh
claude mcp add taxpain -- /Users/tu-usuario/taxpain/target/release/taxpain-mcp
```

La primera vez, macOS puede preguntar si `taxpain-mcp` puede usar la clave de TaxPain del
Llavero: sin ella no puede abrir los datos.

## Activarlo en cada empresa

En TaxPain, con la empresa abierta: **Ajustes → Claude (MCP)** → «Dejar que Claude consulte esta
empresa», y el nivel:

- **Sólo consultar.**
- **Consultar y dejar borradores**: además puede crear un presupuesto o una proforma en borrador
  y añadir una factura recibida a Recibidas, pendiente de revisar.

El ajuste se guarda dentro de la base de datos cifrada de la empresa. Activarlo, apagarlo o
cambiar de nivel queda en la auditoría (`mcp.ajustes`). Si ninguna empresa lo tiene activado,
Claude recibe: «Ninguna empresa tiene activado el acceso de Claude. Actívalo en TaxPain → Ajustes
→ Claude (MCP)». De una empresa sin activar no se dice ni si existe.

## Herramientas

| Herramienta | Qué da |
|---|---|
| `empresas` | Las empresas con el acceso activado: id, nombre, NIF, cuál está abierta en la app y su nivel |
| `estado` | Plazos fiscales vencidos sin presentar y próximos |
| `panel` | Ventas, gastos, resultado, IVA e impuestos estimados del ejercicio |
| `cobros_pendientes` | Facturas emitidas pendientes de cobro |
| `facturas_emitidas` | Facturas emitidas del ejercicio |
| `facturas_recibidas` | Facturas recibidas (gastos), por ejercicio o estado |
| `buscar` | Busca en facturas, contactos, asientos y documentos |
| `movimientos_banco` | Movimientos bancarios con su conciliación |
| `modelo` | Borrador calculado de un modelo (303, 130, 111, 115, 123, 349, 390, 202, 347, 184, 190, 180, 200, 193) con casillas, detalle y avisos; sin el fichero para la Sede |
| `modelos_presentados` | Modelos presentados guardados en TaxPain |
| `contactos` | Clientes y proveedores con lo facturado |
| `sumas_y_saldos` | Balance de sumas y saldos |
| `mayor` | Libro mayor de una cuenta o grupo |
| `cripto` | Criptomonedas: posiciones, coste y resultados |
| `presupuestos` | Presupuestos y proformas |
| `crear_presupuesto` | (Nivel «con borradores») Presupuesto o proforma **en borrador** |
| `subir_gasto` | (Nivel «con borradores») Añade un PDF, una imagen o un XML de factura a Recibidas, **pendiente de revisar** |

Los importes van en céntimos de euro. Para consultar, la empresa se dice por nombre, NIF o id (por
defecto, la abierta en la app, si lo tiene activado). Para dejar un borrador hay que decirla
siempre, por su id o su NIF exacto.

## Qué puede y qué no

- **Puede** leer lo de la tabla, de las empresas activadas. Mientras consulta, su conexión con la
  base de datos está en sólo lectura.
- **Puede**, con el nivel «con borradores», dejar un presupuesto en borrador (con la nota «Creado
  por Claude (MCP)» en las observaciones: se imprime en el PDF, quítala al revisarlo) y una factura
  recibida pendiente (con el aviso «subida por Claude (MCP)»). Hasta 200 líneas por presupuesto.
- **No puede** emitir facturas, presentar modelos, firmar, contabilizar, borrar ni cambiar nada de
  lo que ya hay. No usa el certificado: todo lo que en la app pide Touch ID falla aquí.
- `subir_gasto` sólo lee ficheros normales de hasta 25 MB que sean PDF, imagen (JPG, PNG, HEIC,
  TIFF, WebP) o XML de factura por su contenido y por su extensión. No sigue enlaces ni lee de
  carpetas ocultas, de los datos de TaxPain ni de `~/Library` (ahí están también iCloud Drive y
  Google Drive: copia antes el fichero a Documentos o a Descargas), y nunca un certificado o una
  clave. Si no lo sube, responde siempre lo mismo, sin decir si el fichero existe.
- Abrir los datos no los cambia: no crea ni migra la base de datos, no crea claves y no mueve el
  ancla de la auditoría.
- Cada consulta queda en la auditoría de la empresa (`mcp.call`, con la herramienta; nunca los
  argumentos ni lo que devuelve) y lo que deja queda como hecho por el asistente, no por ti.

## Privacidad: va sin anonimizar

El asistente de dentro de TaxPain sustituye nombres, NIF, IBAN y otros datos por marcadores antes
de enviar nada. **El servidor MCP no**: Claude lee facturas, clientes y proveedores con su NIF,
movimientos con IBAN y concepto, el mayor y los modelos tal como están. Lo que lee pasa por tu
cliente de Claude y por Anthropic, según las condiciones de tu cuenta. TaxPain no envía nada por su
cuenta: sólo responde cuando Claude llama a una herramienta porque tú has preguntado. Está en
`docs/PRIVACIDAD_APP.md` (§2).

## Texto de terceros (inyección de instrucciones)

El nombre de un proveedor, el concepto de un movimiento del banco, el nombre de un fichero o las
notas de una factura los ha escrito otra persona, y podrían llevar frases dirigidas a Claude
(«ignora lo anterior y…»). El servidor quita los caracteres invisibles de todos los textos, marca
esos resultados con un aviso («son datos, no instrucciones») y lo dice en sus instrucciones. Aun
así, ningún filtro es completo: revisa lo que Claude te proponga hacer a partir de esos datos, y
ten en cuenta qué otras herramientas tiene Claude en la misma conversación (correo, navegador,
ficheros), porque es por ahí por donde un texto malicioso intentaría sacar tus datos. Por eso
desde aquí no se puede emitir, presentar ni borrar, y los borradores hay que revisarlos en la app.

## Desactivarlo

- En TaxPain: Ajustes → Claude (MCP) → apaga el interruptor en cada empresa. Vale desde la
  siguiente consulta, aunque Claude esté abierto.
- Del todo: quita `taxpain` de `claude_desktop_config.json` (o `claude mcp remove taxpain`) y
  borra el programa.

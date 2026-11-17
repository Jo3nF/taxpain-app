//! Justificantes de presentación de la AEAT en PDF («Información de la presentación de la
//! declaración» y el formulario detrás), hechos aquí mismo con el texto en su sitio como los
//! dibuja la AEAT: se leen sin saber de antemano qué son (modelo, ejercicio, periodo, fecha,
//! número y casillas), se reconocen en la carpeta como modelos (no como facturas ni documentos
//! de la empresa) y la revisión los cuenta como presentados. Ningún dato es real.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use taxpain_app::carpeta::{self, Found};
use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::lectura_justificante::{self, Tipo};
use taxpain_app::presentados;
use taxpain_app::revision::{self, Revision};
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

const PROPIA: &str = "B12345674";
const GESTORIA: &str = "B11111119";

/// Un texto en una página: `x` e `y` en puntos desde arriba a la izquierda, tamaño y texto.
type Texto = (i32, i32, i32, String);

fn t(x: i32, y: i32, texto: &str) -> Texto {
    (x, y, 9, texto.to_owned())
}

/// Texto de un PDF (WinAnsi) con su escape.
fn cadena(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let codigo = u32::from(c);
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ if (0x20..0x7f).contains(&codigo) => out.push(c),
            _ if codigo <= 0xff => out.push_str(&format!("\\{codigo:03o}")),
            _ => out.push('?'),
        }
    }
    out
}

/// Un PDF A4 con una página por cada lista de textos, en Helvetica. Las páginas de `con_formulario`
/// dibujan antes un formulario (XObject) con su propia fuente /F1, que sólo tiene ancho para la
/// «A»: así los dibuja la AEAT en algunos años, y la página se leía con la fuente del
/// formulario (las cifras, sin ancho, se apilaban).
fn pdf(paginas: &[Vec<Texto>], con_formulario: &[usize]) -> Vec<u8> {
    let n = paginas.len();
    // 1 catálogo, 2 páginas, 3 Helvetica, 4 fuente del formulario, 5 formulario; luego cada
    // página y su contenido.
    let formulario = "BT /F1 9 Tf 40 820 Td (A) Tj ET";
    let mut objetos: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {n} >>",
            (0..n)
                .map(|i| format!("{} 0 R", 6 + 2 * i))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /DEMO+Formulario /FirstChar 65 /LastChar 65 \
         /Widths [667] /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 595 842] /Resources << /Font << /F1 4 0 \
             R >> >> /Length {} >>\nstream\n{formulario}\nendstream",
            formulario.len() + 1
        ),
    ];
    for (i, textos) in paginas.iter().enumerate() {
        let mut contenido = String::new();
        if con_formulario.contains(&i) {
            contenido.push_str("q /Fm0 Do Q\n");
        }
        for (x, y, tam, texto) in textos {
            contenido.push_str(&format!(
                "BT /F1 {tam} Tf 1 0 0 1 {x} {} Tm ({}) Tj ET\n",
                842 - y,
                cadena(texto)
            ));
        }
        objetos.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 \
             0 R >> /XObject << /Fm0 5 0 R >> >> /Contents {} 0 R >>",
            7 + 2 * i
        ));
        objetos.push(format!(
            "<< /Length {} >>\nstream\n{contenido}endstream",
            contenido.len()
        ));
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut posiciones = Vec::new();
    for (i, o) in objetos.iter().enumerate() {
        posiciones.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objetos.len() + 1).bytes());
    for p in posiciones {
        out.extend(format!("{p:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objetos.len() + 1
        )
        .bytes(),
    );
    out
}

/// Lo que cambia de un justificante a otro.
struct Datos<'a> {
    modelo: &'a str,
    ejercicio: i32,
    periodo: &'a str,
    numero: &'a str,
    /// «dd-mm-aaaa».
    presentado: &'a str,
    nif: &'a str,
    /// Líneas del formulario, debajo de la cabecera de la primera página.
    formulario: Vec<Texto>,
    /// Más páginas del formulario.
    mas: Vec<Vec<Texto>>,
}

/// La portada y el formulario de un justificante.
fn justificante(d: &Datos<'_>, con_formulario: &[usize]) -> Vec<u8> {
    let expediente = format!("{}{}34500001A", d.ejercicio, d.modelo);
    let portada = vec![
        t(160, 70, "INFORMACIÓN DE LA PRESENTACIÓN DE LA DECLARACIÓN"),
        t(300, 110, &format!("Modelo {}", d.modelo)),
        t(42, 140, "Registro"),
        t(
            62,
            158,
            &format!("Presentación realizada el: {} a las 10:15:30", d.presentado),
        ),
        t(
            62,
            178,
            &format!("Expediente/Referencia (nº registro asignado): {expediente}"),
        ),
        t(62, 198, "Código Seguro de Verificación: ABCD1234EFGH5678"),
        t(62, 218, &format!("Número de justificante: {}", d.numero)),
        t(62, 238, "Vía de entrada: Presentación por Internet"),
        t(42, 270, "Presentador"),
        t(62, 288, &format!("NIF Presentador: {GESTORIA}")),
        t(
            62,
            308,
            "Apellidos y Nombre / Razón social: ASESORES DEMO SL",
        ),
        t(62, 328, "En calidad de: Colaborador"),
        t(
            160,
            822,
            "La autenticidad de este documento puede ser comprobada mediante el Código Seguro",
        ),
        t(
            170,
            832,
            "de Verificación ABCD1234EFGH5678 en https://sede.agenciatributaria.gob.es",
        ),
    ];
    let mut primera = vec![
        t(40, 50, "Agencia Tributaria"),
        t(520, 60, d.modelo),
        t(340, 100, "Ejercicio"),
        t(400, 100, &d.ejercicio.to_string()),
        t(460, 100, "Período"),
        t(510, 100, d.periodo),
        t(370, 150, &format!("Número justificante: {}", d.numero)),
        t(40, 140, "NIF"),
        t(40, 160, d.nif),
        t(140, 160, "EMPRESA DEMO SL"),
    ];
    primera.extend(d.formulario.iter().cloned());
    let mut paginas = vec![portada, primera];
    paginas.extend(d.mas.iter().cloned());
    pdf(&paginas, con_formulario)
}

/// Una línea del formulario con su etiqueta y pares «casilla importe».
fn fila(y: i32, etiqueta: &str, pares: &[(i32, &str)]) -> Vec<Texto> {
    let mut v = vec![t(40, y, etiqueta)];
    v.extend(pares.iter().map(|(x, s)| t(*x, y, s)));
    v
}

/// El formulario de un 303: 1.000 € de base al 21 %, 31,20 € de adquisiciones intracomunitarias,
/// 300 € de IVA soportado; sale a ingresar 147 €.
fn formulario_303() -> (Vec<Texto>, Vec<Vec<Texto>>) {
    let mut f = Vec::new();
    f.extend(fila(
        300,
        "Régimen general",
        &[(296, "07"), (330, "1.000,00"), (461, "09"), (520, "210,00")],
    ));
    f.extend(fila(
        320,
        "Adquisiciones intracomunitarias",
        &[(296, "10"), (340, "31,20"), (461, "11"), (530, "6,55")],
    ));
    f.extend(fila(
        360,
        "Total cuota devengada",
        &[(461, "27"), (520, "216,55")],
    ));
    f.extend(fila(
        400,
        "Por cuotas soportadas en operaciones interiores corrientes",
        &[(296, "28"), (340, "300,00"), (461, "29"), (530, "63,00")],
    ));
    f.extend(fila(
        420,
        "En adquisiciones intracomunitarias",
        &[(296, "36"), (340, "31,20"), (461, "37"), (530, "6,55")],
    ));
    f.extend(fila(460, "Total a deducir", &[(461, "45"), (530, "69,55")]));
    f.extend(fila(
        480,
        "Resultado régimen general",
        &[(461, "46"), (520, "147,00")],
    ));
    let mas = vec![vec![
        t(40, 50, "Agencia Tributaria"),
        t(40, 100, "Resultado"),
        t(461, 100, "69"),
        t(520, 100, "147,00"),
        t(40, 120, "Resultado de la autoliquidación"),
        t(461, 120, "71"),
        t(520, 120, "147,00"),
    ]];
    (f, mas)
}

fn datos_303<'a>(periodo: &'a str, numero: &'a str, presentado: &'a str) -> Datos<'a> {
    let (formulario, mas) = formulario_303();
    Datos {
        modelo: "303",
        ejercicio: 2025,
        periodo,
        numero,
        presentado,
        nif: PROPIA,
        formulario,
        mas,
    }
}

fn eur(c: i64) -> Money {
    Money::from_cents(c)
}

#[test]
fn un_303_se_lee_sin_saber_que_es() {
    let pdf = justificante(&datos_303("4T", "3030000000017", "28-01-2026"), &[]);
    let r = presentados::read(&pdf).unwrap();
    assert_eq!(r.source, "justificante");
    // Un 4T presentado en enero es del ejercicio anterior: lo dice el formulario.
    assert_eq!(
        (r.model.as_str(), r.year, r.period.as_str()),
        ("303", 2025, "4T")
    );
    assert_eq!(r.presented_at, Some(date!(2026 - 01 - 28)));
    assert_eq!(r.receipt.as_deref(), Some("3030000000017"));
    assert_eq!(r.kind.as_deref(), Some("normal"));
    assert_eq!(r.nif.as_deref(), Some(PROPIA));
    assert!(r.boxes_read);
    assert_eq!(r.boxes.get(&7), Some(&eur(100_000)));
    assert_eq!(r.boxes.get(&46), Some(&eur(14_700)));
    assert_eq!(r.boxes.get(&71), Some(&eur(14_700)));
    let j = lectura_justificante::leer_justificante(&pdf).unwrap();
    assert_eq!(j.csv.as_deref(), Some("ABCD1234EFGH5678"));
    assert_eq!(j.presentador.as_deref(), Some(GESTORIA));
    assert_eq!(j.en_calidad_de.as_deref(), Some("Colaborador"));
    assert_eq!(j.expediente.as_deref(), Some("202530334500001A"));
}

/// El formulario dibujado dentro de la página (XObject) con una fuente del mismo nombre: las
/// casillas y los importes se leen igual.
#[test]
fn con_un_formulario_de_fuente_del_mismo_nombre() {
    let pdf = justificante(&datos_303("1T", "3030000000025", "20-04-2025"), &[1, 2]);
    let r = presentados::read(&pdf).unwrap();
    assert_eq!(r.source, "justificante");
    assert_eq!((r.year, r.period.as_str()), (2025, "1T"));
    assert!(r.boxes_read, "{:?}", r.boxes);
    assert_eq!(r.boxes.get(&46), Some(&eur(14_700)));
    assert_eq!(r.boxes.get(&9), Some(&eur(21_000)));
    assert_eq!(r.boxes.get(&45), Some(&eur(6_955)));
}

/// Si las casillas no cuadran entre sí (aquí falta la [45]), el justificante se reconoce igual
/// pero sus casillas no se usan: no son cero.
#[test]
fn casillas_que_no_cuadran_quedan_sin_leer() {
    let mut d = datos_303("2T", "3030000000033", "18-07-2025");
    d.formulario
        .retain(|(_, _, _, s)| s != "45" && s != "69,55");
    let pdf = justificante(&d, &[]);
    let r = presentados::read(&pdf).unwrap();
    assert_eq!(
        (r.model.as_str(), r.year, r.period.as_str()),
        ("303", 2025, "2T")
    );
    assert!(!r.boxes_read);
    assert!(r.boxes.is_empty());
}

#[test]
fn complementaria_con_el_justificante_anterior() {
    let mut d = datos_303("3T", "3030000000041", "05-11-2025");
    d.mas[0].extend([
        t(300, 300, "X"),
        t(320, 300, "Autoliquidación complementaria"),
        t(300, 320, "Nº de justificante:"),
        t(400, 320, "3030000000058"),
    ]);
    let r = presentados::read(&justificante(&d, &[])).unwrap();
    assert_eq!(r.kind.as_deref(), Some("complementaria"));
    assert_eq!(r.previous_receipt.as_deref(), Some("3030000000058"));
    let j = lectura_justificante::leer_justificante(&justificante(&d, &[])).unwrap();
    assert_eq!(j.tipo, Tipo::Complementaria);
}

/// Los resúmenes anuales que no compara TaxPain también se reconocen (el 190 no está en
/// `presentados::MODELS`), con sus totales.
#[test]
fn un_190_con_sus_totales() {
    let d = Datos {
        modelo: "190",
        ejercicio: 2025,
        periodo: "",
        numero: "1900000000011",
        presentado: "21-01-2026",
        nif: PROPIA,
        formulario: vec![
            t(40, 300, "Número total de perceptores ......"),
            t(400, 300, "01"),
            t(480, 300, "3"),
            t(40, 320, "Importe total de las percepciones ......"),
            t(400, 320, "02"),
            t(480, 320, "4.210,50"),
            t(40, 340, "Importe total de las retenciones ......"),
            t(400, 340, "03"),
            t(480, 340, "631,58"),
        ],
        mas: Vec::new(),
    };
    let r = presentados::read(&justificante(&d, &[])).unwrap();
    assert_eq!(
        (r.model.as_str(), r.year, r.period.as_str()),
        ("190", 2025, "0A")
    );
    assert!(r.boxes_read);
    assert_eq!(r.boxes.get(&2), Some(&eur(421_050)));
    assert_eq!(r.boxes.get(&3), Some(&eur(63_158)));
}

fn vault() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let info = CompanyInfo {
        name: "Empresa Demo SL".into(),
        nif: Some(PROPIA.into()),
    };
    companies::set_info(&v, &info, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

fn escribir(root: &Path, ruta: &str, bytes: &[u8]) {
    let p = root.join(ruta);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

/// Un extracto MT940 con cargos de Hacienda (fecha AAMMDD e importe).
fn mt940(movs: &[(&str, &str)]) -> Vec<u8> {
    let mut s = String::from(
        "{1:F01DEMOESMMAXXX0000000000}{4:\n:20:DEMO\n:25:ES9121000418450200051332\n:28C:1/1\n:60F:C250101EUR10000,00\n",
    );
    for (f, imp) in movs {
        s.push_str(&format!(
            ":61:{f}D{imp}NTRFNONREF\n:86:AEAT IMPUESTOS DOMICILIADOS\n"
        ));
    }
    s.push_str(":62F:C251231EUR9000,00\n-}");
    s.into_bytes()
}

/// Una carpeta con los justificantes de la AEAT y sin ninguna lectura de IA.
fn carpeta_de_justificantes(root: &Path) {
    let r303 =
        |p: &'static str, n: &'static str, f: &'static str| justificante(&datos_303(p, n, f), &[]);
    escribir(
        root,
        "MODELOS/2025/303_1T.pdf",
        &r303("1T", "3030000000066", "20-04-2025"),
    );
    // El del 2T no cuadra: se reconoce, pero sin casillas.
    let mut d = datos_303("2T", "3030000000074", "18-07-2025");
    d.formulario
        .retain(|(_, _, _, s)| s != "45" && s != "69,55");
    escribir(root, "MODELOS/2025/303_2T.pdf", &justificante(&d, &[]));
    // Un 349 del 2T con 1.000 €: con el 303 del 2T sin casillas, no se cruzan.
    let d = Datos {
        modelo: "349",
        ejercicio: 2025,
        periodo: "2T",
        numero: "3490000000013",
        presentado: "18-07-2025",
        nif: PROPIA,
        formulario: vec![
            t(
                40,
                300,
                "Número total de operadores intracomunitarios ......",
            ),
            t(400, 300, "01"),
            t(480, 300, "1"),
            t(
                40,
                320,
                "Importe de las operaciones intracomunitarias ......",
            ),
            t(400, 320, "02"),
            t(480, 320, "1.000,00"),
        ],
        mas: Vec::new(),
    };
    escribir(root, "MODELOS/2025/349_2T.pdf", &justificante(&d, &[]));
    // Un 111 del 1T y el 202 del 1P.
    let d = Datos {
        modelo: "111",
        ejercicio: 2025,
        periodo: "1T",
        numero: "1110000000021",
        presentado: "20-04-2025",
        nif: PROPIA,
        formulario: vec![
            t(40, 300, "Rendimientos de actividades económicas"),
            t(300, 300, "08"),
            t(340, 300, "1.000,00"),
            t(461, 300, "09"),
            t(520, 300, "150,00"),
            t(40, 340, "Suma de retenciones"),
            t(461, 340, "28"),
            t(520, 340, "150,00"),
            t(40, 360, "Resultado a ingresar"),
            t(461, 360, "30"),
            t(520, 360, "150,00"),
        ],
        mas: Vec::new(),
    };
    escribir(root, "MODELOS/2025/111_1T.pdf", &justificante(&d, &[]));
    let d = Datos {
        modelo: "202",
        ejercicio: 2025,
        periodo: "1P",
        numero: "2020000000037",
        presentado: "20-04-2025",
        nif: PROPIA,
        formulario: vec![
            t(40, 300, "A ingresar"),
            t(461, 300, "03"),
            t(520, 300, "500,00"),
        ],
        mas: Vec::new(),
    };
    escribir(root, "MODELOS/2025/202_1P.pdf", &justificante(&d, &[]));
    // El justificante de una participada (otro NIF): no es de la empresa.
    let mut d = datos_303("1T", "3030000000082", "20-04-2025");
    d.nif = "B22222228";
    escribir(root, "PARTICIPADAS/303_1T.pdf", &justificante(&d, &[]));
    escribir(
        root,
        "BANCO/2025.sta",
        &mt940(&[
            ("250422", "147,00"),
            ("250422", "150,00"),
            ("250422", "500,00"),
        ]),
    );
}

#[test]
fn en_la_carpeta_son_modelos_y_no_facturas() {
    let v = vault();
    let dir = tempfile::tempdir().unwrap();
    carpeta_de_justificantes(dir.path());
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    let found = |ruta: &str| {
        &inv.items
            .iter()
            .find(|i| i.path == ruta)
            .unwrap_or_else(|| panic!("falta {ruta}"))
            .found
    };
    assert!(matches!(
        found("MODELOS/2025/303_1T.pdf"),
        Found::Modelo { model, source, boxes_read: true, .. } if model == "303" && source == "justificante"
    ));
    assert!(matches!(
        found("MODELOS/2025/303_2T.pdf"),
        Found::Modelo {
            boxes_read: false,
            ..
        }
    ));
    assert!(matches!(
        found("MODELOS/2025/349_2T.pdf"),
        Found::Modelo { model, .. } if model == "349"
    ));
    assert!(matches!(
        found("PARTICIPADAS/303_1T.pdf"),
        Found::Otro { .. }
    ));
    // Al importar, sólo se guarda lo que se puede comparar y se ha leído.
    let paths: Vec<String> = inv
        .items
        .iter()
        .filter(|i| i.path.starts_with("MODELOS"))
        .map(|i| i.path.clone())
        .collect();
    carpeta::apply(&v, dir.path(), &paths, datetime!(2026-01-02 9:00 UTC)).unwrap();
    let guardados = presentados::list(&v, Some(2025)).unwrap();
    let claves: Vec<(String, String)> = guardados
        .iter()
        .map(|f| (f.model.clone(), f.period.clone()))
        .collect();
    assert!(claves.contains(&("303".into(), "1T".into())), "{claves:?}");
    assert!(!claves.contains(&("303".into(), "2T".into())), "{claves:?}");
    // El 349 del PDF, con su importe total en la [02] (la que se compara).
    let m349 = guardados.iter().find(|f| f.model == "349").unwrap();
    assert_eq!(m349.boxes.get(&2), Some(&eur(100_000)));
    assert!(guardados.iter().all(|f| f.source == "pdf"));
}

fn revisar(root: &Path) -> Revision {
    let v = vault();
    v.set_setting("taxpayer_profile", &TaxpayerProfile::sociedad_limitada())
        .unwrap();
    revision::revisar(&v, root, date!(2026 - 10 - 31)).unwrap()
}

#[test]
fn la_revision_los_cuenta_como_presentados() {
    let dir = tempfile::tempdir().unwrap();
    carpeta_de_justificantes(dir.path());
    let r = revisar(dir.path());
    let titulos: Vec<String> = r
        .hallazgos
        .iter()
        .map(|h| format!("{}: {}", h.codigo, h.titulo))
        .collect();
    // No faltan los que están (aunque el del 2T no tenga las casillas leídas).
    let faltan = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "modelos.faltan" && h.ejercicio == Some(2025))
        .unwrap();
    assert!(
        faltan.detalle.contains("303 (3T, 4T)"),
        "{}",
        faltan.detalle
    );
    assert!(
        faltan.detalle.contains("111 (2T, 3T, 4T)"),
        "{}",
        faltan.detalle
    );
    // El 2P y el 3P del 202 dependen de la cuota del 200 de 2024, que no está: no se piden; se
    // dice que no se sabe si tocaban.
    assert!(!faltan.detalle.contains("202 ("), "{}", faltan.detalle);
    let dudas = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "modelos.no_se_sabe" && h.ejercicio == Some(2025))
        .unwrap_or_else(|| panic!("{titulos:?}"));
    assert!(
        dudas.detalle.contains("202 del 2P") && dudas.detalle.contains("202 del 3P"),
        "{}",
        dudas.detalle
    );
    // Ni facturas ni documentos de la empresa.
    for c in &r.resumen.por_tipo {
        assert!(
            !c.tipo.starts_with("Facturas") && c.tipo != "Documentos de la empresa",
            "{:?}",
            r.resumen.por_tipo
        );
    }
    // El 349 no se cruza con un 303 cuyas casillas no se han leído (no son cero).
    assert!(
        !titulos.iter().any(|t| t.starts_with("modelos.349_303")),
        "{titulos:?}"
    );
    // Los pagos casan con el banco: ninguno sin modelo ni sin pagar.
    assert!(
        !titulos
            .iter()
            .any(|t| t.starts_with("cruces.cargos_sin_modelo") || t.starts_with("cruces.pago")),
        "{titulos:?}"
    );
    let pagos = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Pagos de los modelos en el banco")
        .unwrap();
    assert_eq!(pagos.filas.len(), 3, "{:?}", pagos.filas);
    assert!(
        pagos.filas.iter().all(|f| f[5] == "pagado"),
        "{:?}",
        pagos.filas
    );
    // El de la participada no cuenta.
    assert!(
        !titulos.iter().any(|t| t.contains("B22222228")),
        "{titulos:?}"
    );
}

/// «Comprobar lo presentado» con justificantes cuyo fichero no da el visor de la AEAT.
mod comprobar_lo_presentado {
    use super::*;
    use taxpain_app::invoicing;
    use taxpain_app::presentado_aeat::{self, Answer, Download, Presented, Visor};
    use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
    use taxpain_core::invoice::{Party, PartyId, RoundingMode};
    use taxpain_core::money::Rate;

    /// La empresa con su facturación configurada (para los borradores de TaxPain).
    fn con_facturacion() -> Vault {
        let v = vault();
        let s = InvoicingSettings {
            issuer: Party {
                id: PartyId::Nif(PROPIA.into()),
                name: "EMPRESA DEMO SL".into(),
                address: addr(),
                person: None,
            },
            default_series: "A".into(),
            rectifying_series: "R".into(),
            simplified_series: "S".into(),
            simplified_rectifying_series: "RS".into(),
            language: Language::Es,
            default_vat: Rate::percent(21),
            default_withholding: None,
            outputs: Outputs {
                pdf: false,
                facturae: false,
                ubl: false,
            },
            destination: None,
            rounding: RoundingMode::PerRate,
            verifactu: VerifactuSettings {
                enabled: false,
                installation_id: "x".into(),
                production: false,
            },
            footer: None,
            activity: None,
            simplified_up_to_3000: false,
        };
        invoicing::save_settings(&v, &s, datetime!(2026-01-01 9:00 UTC)).unwrap();
        v
    }

    fn presentada(model: &str, period: &str, csv: &str, receipt: &str) -> Presented {
        Presented {
            model: model.into(),
            year: 2025,
            period: period.into(),
            csv: csv.into(),
            file_number: String::new(),
            receipt: receipt.into(),
            previous_receipt: None,
            presented_at: "2025-07-18T10:15:30".into(),
        }
    }

    fn registrar(v: &Vault, p: Presented) {
        let a = Answer {
            models: vec![p.model.clone()],
            list: vec![p],
            ..Answer::default()
        };
        presentado_aeat::record(v, PROPIA, 2025, &a, datetime!(2026-01-02 9:00 UTC)).unwrap();
    }

    /// Un 349 leído del PDF del justificante se guarda con su importe total en la [02] y se
    /// compara con lo que calcula TaxPain (antes se guardaba sin casillas).
    #[test]
    fn el_349_del_pdf_guarda_su_importe() {
        let v = con_facturacion();
        registrar(
            &v,
            presentada("349", "2T", "AAAAAAAAAAAAAAA1", "3490000000013"),
        );
        let d = Datos {
            modelo: "349",
            ejercicio: 2025,
            periodo: "2T",
            numero: "3490000000013",
            presentado: "18-07-2025",
            nif: PROPIA,
            formulario: vec![
                t(
                    40,
                    300,
                    "Número total de operadores intracomunitarios ......",
                ),
                t(400, 300, "01"),
                t(480, 300, "1"),
                t(
                    40,
                    320,
                    "Importe de las operaciones intracomunitarias ......",
                ),
                t(400, 320, "02"),
                t(480, 320, "1.000,00"),
            ],
            mas: Vec::new(),
        };
        let pdf = justificante(&d, &[]);
        presentado_aeat::attach(
            &v,
            "AAAAAAAAAAAAAAA1",
            Download::Pdf(&pdf),
            Visor::Final("no_disponible"),
            datetime!(2026-01-02 9:00 UTC),
        )
        .unwrap();
        let guardado = presentados::list(&v, Some(2025)).unwrap();
        assert_eq!(guardado.len(), 1);
        assert_eq!(guardado[0].boxes.get(&2), Some(&eur(100_000)));
        let c = presentados::compare(&v, guardado[0].id).unwrap();
        assert_eq!(c.rows.len(), 1, "{:?}", c.rows);
        assert_eq!(c.rows[0].theirs, Some(eur(100_000)));
        assert_eq!(c.mismatches, 1);
    }

    /// Las casillas sacadas del PDF son aproximadas: si el visor no contestó, se le vuelve a
    /// pedir el fichero en la próxima comprobación.
    #[test]
    fn se_vuelve_a_pedir_el_fichero_si_las_casillas_son_del_pdf() {
        let v = vault();
        registrar(
            &v,
            presentada("303", "2T", "AAAAAAAAAAAAAAA2", "3030000000074"),
        );
        let pdf = justificante(&datos_303("2T", "3030000000074", "18-07-2025"), &[]);
        presentado_aeat::attach(
            &v,
            "AAAAAAAAAAAAAAA2",
            Download::Pdf(&pdf),
            Visor::Unknown,
            datetime!(2026-01-02 9:00 UTC),
        )
        .unwrap();
        let p = presentado_aeat::pending(&v, 2025).unwrap();
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].has_pdf && !p[0].visor_done);
        // Si el visor dice que no tiene el fichero, ya no se le vuelve a pedir.
        presentado_aeat::attach(
            &v,
            "AAAAAAAAAAAAAAA2",
            Download::Stored,
            Visor::Final("sin_fichero"),
            datetime!(2026-01-03 9:00 UTC),
        )
        .unwrap();
        assert!(presentado_aeat::pending(&v, 2025).unwrap().is_empty());
    }
}

/// Un resumen informativo (347 o 349) con su número de registros y su importe total.
fn informativa<'a>(
    modelo: &'a str,
    periodo: &'a str,
    numero: &'a str,
    presentado: &'a str,
    importe: &str,
) -> Datos<'a> {
    Datos {
        modelo,
        ejercicio: if modelo == "347" { 2024 } else { 2025 },
        periodo,
        numero,
        presentado,
        nif: PROPIA,
        formulario: vec![
            t(40, 300, "Número total de registros ......"),
            t(400, 300, "01"),
            t(480, 300, "1"),
            t(40, 320, "Importe total de las operaciones ......"),
            t(400, 320, "02"),
            t(480, 320, importe),
        ],
        mas: Vec::new(),
    }
}

/// Marca un justificante como complementario o sustitutivo de otro.
fn de_otra(d: &mut Datos<'_>, tipo: &str, anterior: &str) {
    d.formulario.extend([
        t(300, 360, "X"),
        t(320, 360, tipo),
        t(300, 380, "Nº de justificante:"),
        t(400, 380, anterior),
    ]);
}

#[test]
fn la_complementaria_de_una_informativa_no_deja_fuera_la_original() {
    let v = vault();
    let dir = tempfile::tempdir().unwrap();
    let d = informativa("349", "2T", "3490000000013", "18-07-2025", "1.000,00");
    escribir(dir.path(), "MODELOS/349_2T.pdf", &justificante(&d, &[]));
    let mut c = informativa("349", "2T", "3490000000021", "10-09-2025", "500,00");
    de_otra(&mut c, "Declaración complementaria", "3490000000013");
    let pdf_c = justificante(&c, &[]);
    let r = presentados::read(&pdf_c).unwrap();
    assert_eq!(r.kind.as_deref(), Some("complementaria"));
    assert_eq!(r.previous_receipt.as_deref(), Some("3490000000013"));
    escribir(dir.path(), "MODELOS/349_2T_complementaria.pdf", &pdf_c);
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    let paths: Vec<String> = inv.items.iter().map(|i| i.path.clone()).collect();
    let out = carpeta::apply(&v, dir.path(), &paths, datetime!(2026-01-02 9:00 UTC)).unwrap();
    // Se guarda la original: la complementaria sólo trae lo que faltaba.
    let guardados = presentados::list(&v, Some(2025)).unwrap();
    assert_eq!(guardados.len(), 1, "{out:?}");
    assert_eq!(guardados[0].boxes.get(&2), Some(&eur(100_000)));
    let o = out
        .iter()
        .find(|o| o.path.ends_with("complementaria.pdf"))
        .unwrap();
    assert!(o.message.contains("complementaria"), "{o:?}");
}

/// Escribe las lecturas de IA de la carpeta.
fn lecturas(root: &Path, l: serde_json::Value) {
    std::fs::write(
        root.join("taxpain-lecturas.json"),
        serde_json::to_vec(
            &serde_json::json!({"version": 1, "generado_por": "Claude Code", "lecturas": l}),
        )
        .unwrap(),
    )
    .unwrap();
}

fn fuera_de_plazo<'a>(r: &'a Revision, que: &str) -> Vec<&'a revision::Hallazgo> {
    r.hallazgos
        .iter()
        .filter(|h| h.codigo == "cruces.fuera_de_plazo" && h.titulo.contains(que))
        .collect()
}

#[test]
fn una_complementaria_sin_casillas_no_toma_las_de_la_original() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // La original, leída con IA de otra copia: sin número de justificante ni fecha.
    escribir(
        root,
        "MODELOS/copias/303 1T 2025.pdf",
        b"%PDF-1.4 copia de la original",
    );
    lecturas(
        root,
        serde_json::json!([{"fichero": "MODELOS/copias/303 1T 2025.pdf", "lectura": {
            "tipo": "modelo_aeat", "descripcion": "Modelo 303 1T 2025",
            "modelo": {"modelo": "303", "ejercicio": 2025, "periodo": "1T",
                       "casillas": {"71": "147.00"}}}}]),
    );
    // La complementaria, presentada en septiembre, con las casillas que no cuadran.
    let mut d = datos_303("1T", "3030000000090", "10-09-2025");
    d.formulario
        .retain(|(_, _, _, s)| s != "45" && s != "69,55");
    d.mas[0].extend([
        t(300, 300, "X"),
        t(320, 300, "Autoliquidación complementaria"),
        t(300, 320, "Nº de justificante:"),
        t(400, 320, "3030000000058"),
    ]);
    escribir(root, "MODELOS/2025/303_1T.pdf", &justificante(&d, &[]));
    let r = revisar(root);
    let h = fuera_de_plazo(&r, "303 del 1T 2025");
    assert_eq!(h.len(), 1, "{:?}", r.hallazgos);
    assert!(
        h[0].titulo.starts_with("La complementaria"),
        "{}",
        h[0].titulo
    );
    assert!(
        h[0].detalle.contains("No se han podido leer sus casillas"),
        "{}",
        h[0].detalle
    );
    assert!(!h[0].detalle.contains("147,00"), "{}", h[0].detalle);
}

#[test]
fn la_sustitutiva_posterior_al_plazo_no_es_una_presentacion_tardia() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let d = informativa("347", "", "3470000000014", "20-02-2025", "36.000,00");
    escribir(root, "MODELOS/2024/347.pdf", &justificante(&d, &[]));
    let mut s = informativa("347", "", "3470000000022", "10-06-2025", "37.000,00");
    de_otra(&mut s, "Declaración sustitutiva", "3470000000014");
    escribir(
        root,
        "MODELOS/2024/347_sustitutiva.pdf",
        &justificante(&s, &[]),
    );
    let r = revisar(root);
    // La original llegó a tiempo (el plazo acababa el 28 de febrero).
    assert!(
        fuera_de_plazo(&r, "347").is_empty(),
        "{:?}",
        fuera_de_plazo(&r, "347")
    );

    // Si la original también es tarde, sí, con la fecha de la original.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let d = informativa("347", "", "3470000000014", "20-03-2025", "36.000,00");
    escribir(root, "MODELOS/2024/347.pdf", &justificante(&d, &[]));
    escribir(
        root,
        "MODELOS/2024/347_sustitutiva.pdf",
        &justificante(&s, &[]),
    );
    let r = revisar(root);
    let h = fuera_de_plazo(&r, "347 de 2024");
    assert_eq!(h.len(), 1, "{:?}", r.hallazgos);
    assert!(
        h[0].detalle.contains("Se presentó el 20/03/2025"),
        "{}",
        h[0].detalle
    );
}

/// Una reimpresión con el sello de la gestoría encima del formulario: el NIF del declarante es
/// el de la identificación, no el del sello.
#[test]
fn el_nif_del_declarante_es_el_de_la_identificacion() {
    let mut d = datos_303("2T", "3030000000107", "18-07-2025");
    d.formulario.insert(
        0,
        t(
            40,
            30,
            &format!("ASESORES DEMO SL · NIF {GESTORIA} · Copia para el cliente"),
        ),
    );
    let pdf = justificante(&d, &[]);
    let j = lectura_justificante::leer_justificante(&pdf).unwrap();
    assert_eq!(j.nif.as_deref(), Some(PROPIA));
    let v = vault();
    let dir = tempfile::tempdir().unwrap();
    escribir(dir.path(), "MODELOS/2025/303_2T.pdf", &pdf);
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    assert!(
        matches!(&inv.items[0].found, Found::Modelo { model, .. } if model == "303"),
        "{:?}",
        inv.items[0].found
    );
}

/// Un justificante y su copia en «otras copias» no se cuentan dos veces: sólo se dice.
#[test]
fn las_copias_de_un_justificante_solo_se_dicen() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let j = justificante(&datos_303("1T", "3030000000115", "20-04-2025"), &[]);
    escribir(root, "MODELOS/2025/303_1T.pdf", &j);
    escribir(root, "MODELOS/2025/otras copias/303_1T.pdf", &j);
    let factura = pdf(
        &[vec![t(
            40,
            100,
            "Factura F-1 de Proveedor Demo SL. Total 121,00 EUR.",
        )]],
        &[],
    );
    escribir(root, "FACTURAS/f1.pdf", &factura);
    escribir(root, "FACTURAS/copia de f1.pdf", &factura);
    let r = revisar(root);
    let h = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "ficheros.copias_justificantes")
        .unwrap_or_else(|| panic!("{:?}", r.hallazgos));
    assert_eq!(h.gravedad, revision::Gravedad::Info);
    assert!(h.ficheros.iter().any(|f| f.contains("otras copias")));
    let aviso = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "ficheros.copias")
        .unwrap_or_else(|| panic!("{:?}", r.hallazgos));
    assert!(
        aviso.ficheros.iter().all(|f| f.starts_with("FACTURAS/")),
        "{:?}",
        aviso.ficheros
    );
}

/// El justificante de un 111 del 1T con 150 € de retenciones (o 200 €, para una copia que se lee
/// con otras casillas).
fn datos_111<'a>(numero: &'a str, retencion: &'a str) -> Datos<'a> {
    Datos {
        modelo: "111",
        ejercicio: 2025,
        periodo: "1T",
        numero,
        presentado: "20-04-2025",
        nif: PROPIA,
        formulario: vec![
            t(40, 300, "Rendimientos de actividades económicas"),
            t(300, 300, "08"),
            t(340, 300, "1.000,00"),
            t(461, 300, "09"),
            t(520, 300, retencion),
            t(40, 340, "Suma de retenciones"),
            t(461, 340, "28"),
            t(520, 340, retencion),
            t(40, 360, "Resultado a ingresar"),
            t(461, 360, "30"),
            t(520, 360, retencion),
        ],
        mas: Vec::new(),
    }
}

#[test]
fn la_copia_del_mismo_justificante_no_se_guarda_otra_vez() {
    let v = vault();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let now = datetime!(2026-01-02 9:00 UTC);
    let oficial = "MODELOS/2025/111_1T_justificante_1110000000021.pdf";
    escribir(
        root,
        oficial,
        &justificante(&datos_111("1110000000021", "150,00"), &[]),
    );
    // La misma declaración con otro contenido (otra descarga, con el formulario dibujado).
    let copia = "MODELOS/2025/otras copias/111 1T copia.pdf";
    escribir(
        root,
        copia,
        &justificante(&datos_111("1110000000021", "150,00"), &[1]),
    );
    let inv = carpeta::scan(&v, root).unwrap();
    let paths: Vec<String> = inv.items.iter().map(|i| i.path.clone()).collect();
    let out = carpeta::apply(&v, root, &paths, now).unwrap();
    let msg = |p: &str| {
        out.iter()
            .find(|o| o.path == p)
            .unwrap_or_else(|| panic!("falta {p}: {out:?}"))
            .message
            .clone()
    };
    assert!(msg(oficial).contains("guardado para comparar"), "{out:?}");
    assert!(msg(copia).contains("ya estaba guardado"), "{out:?}");
    assert_eq!(presentados::list(&v, Some(2025)).unwrap().len(), 1);

    // Una copia que se lee con otras casillas: se queda el justificante oficial, con aviso.
    let rara = "MODELOS/2025/otras copias/111 1T otra.pdf";
    escribir(
        root,
        rara,
        &justificante(&datos_111("1110000000021", "200,00"), &[]),
    );
    let out = carpeta::apply(&v, root, &[rara.into()], now).unwrap();
    assert!(
        out[0].message.contains("ya estaba guardado") && out[0].message.contains("distintas"),
        "{out:?}"
    );
    let guardados = presentados::list(&v, Some(2025)).unwrap();
    assert_eq!(guardados.len(), 1);
    assert_eq!(guardados[0].boxes.get(&30), Some(&eur(15_000)));

    // Al revés: la copia rara primero y el oficial después. Gana el oficial, con aviso.
    let v = vault();
    carpeta::apply(&v, root, &[rara.into()], now).unwrap();
    let out = carpeta::apply(&v, root, &[oficial.into()], now).unwrap();
    assert!(out[0].message.contains("distintas"), "{out:?}");
    let guardados = presentados::list(&v, Some(2025)).unwrap();
    assert_eq!(guardados.len(), 1, "{guardados:?}");
    assert_eq!(guardados[0].boxes.get(&30), Some(&eur(15_000)));
    assert_eq!(
        guardados[0].filename,
        "111_1T_justificante_1110000000021.pdf"
    );
}

/// El justificante de un 200 de 2024: un balance que cuadra ([00180] = [00252]), veinte casillas
/// más y la liquidación (cuota 500 €, un pago fraccionado de 200 €, a ingresar 300 €).
fn justificante_200(numero: &str) -> Vec<u8> {
    let mut formulario = vec![t(40, 280, "Página 1")];
    let mut y = 300;
    let mut casilla = |c: &str, importe: &str| {
        formulario.extend(fila(y, "Casilla", &[(300, c), (400, importe)]));
        y += 16;
    };
    casilla("00180", "10.000,00");
    casilla("00252", "10.000,00");
    for n in 0..20 {
        casilla(&format!("{:05}", 101 + n), &format!("{},00", 100 + n));
    }
    casilla("00599", "500,00");
    casilla("00601", "200,00");
    casilla("00611", "300,00");
    casilla("01586", "300,00");
    let d = Datos {
        modelo: "200",
        ejercicio: 2024,
        periodo: "0A",
        numero,
        presentado: "20-07-2025",
        nif: PROPIA,
        formulario,
        mas: Vec::new(),
    };
    justificante(&d, &[])
}

#[test]
fn el_200_se_guarda_en_modelos_presentados_como_los_demas() {
    let v = vault();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let ruta = "MODELOS/2024/200_justificante_2000000000016.pdf";
    escribir(root, ruta, &justificante_200("2000000000016"));
    let copia = "MODELOS/2024/otras copias/200 2024.pdf";
    let mut otra = justificante_200("2000000000016");
    otra.extend_from_slice(b"\n% copia\n");
    escribir(root, copia, &otra);
    let inv = carpeta::scan(&v, root).unwrap();
    let i = inv.items.iter().find(|i| i.path == ruta).unwrap();
    assert!(
        matches!(&i.found, Found::Modelo { model, boxes_read: true, .. } if model == "200"),
        "{i:?}"
    );
    assert!(i.importable, "{i:?}");
    let paths: Vec<String> = inv.items.iter().map(|i| i.path.clone()).collect();
    let out = carpeta::apply(&v, root, &paths, datetime!(2026-01-02 9:00 UTC)).unwrap();
    let msg = |p: &str| &out.iter().find(|o| o.path == p).unwrap().message;
    assert!(
        msg(ruta).starts_with("modelo 200 2024 0A guardado para comparar"),
        "{out:?}"
    );
    assert!(msg(copia).contains("ya estaba guardado"), "{out:?}");
    let guardados = presentados::list(&v, Some(2024)).unwrap();
    assert_eq!(guardados.len(), 1, "{guardados:?}");
    assert_eq!(guardados[0].model, "200");
    assert_eq!(guardados[0].boxes.get(&1586), Some(&eur(30_000)));
}

/// Domicilio inventado: la factura completa lo exige (art. 6.1.e RD 1619/2012).
fn addr() -> Option<taxpain_core::invoice::Address> {
    Some(taxpain_core::invoice::Address {
        street: "Calle Inventada 1".into(),
        post_code: "46001".into(),
        town: "València".into(),
        province: "Valencia".into(),
        country: "ES".into(),
    })
}

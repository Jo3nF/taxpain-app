//! Diseño de registro de cada modelo según el ejercicio y el periodo de la declaración.
//!
//! La AEAT cambia los diseños de registro (DR) de un año a otro, y a veces dentro del año: el
//! 303 de 2025 leído con el diseño de 2026 tiene páginas de la misma longitud pero con los campos
//! en otro orden, y salen casillas cambiadas. Para leer un fichero de un ejercicio anterior hay
//! que usar el diseño de ese ejercicio y periodo. Los diseños anteriores están en
//! `fixtures/aeat/dr/anteriores/` (originales de la Sede con su SHA-256 en `SOURCES.md` y el JSON
//! generado con `tools/aeat/dr2json.py`); los vigentes, en `fixtures/aeat/dr/`.
//!
//! Correspondencia según las páginas «Diseños de registro» de la Sede (ejercicios anteriores de
//! los modelos 100-199, 200-299 y 300-399), comprobada con declaraciones reales: un 303 leído con el diseño de su periodo cuadra en [27],
//! [45] y [46].
//!
//! Un ejercicio sin diseño conocido (anterior al primero que tiene TaxPain o posterior al último
//! publicado, como el 200 de 2026) no se lee por posiciones: [`design`] da `None` y quien lee usa
//! el PDF y avisa de que hay que actualizar TaxPain (`tools/aeat/dr_actualizar.py`).
//!
//! Los generadores escriben con el diseño vigente ([`generator_design`]), salvo el 390, que
//! escribe 2025 y los anteriores con el de 2025 y 2026 y siguientes con el de 2026: el fichero de
//! un periodo con otro diseño no sirve para presentar ([`generates_with_right_design`]).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::dr::Spec;

/// Un diseño de registro y los periodos en que vale (`from` y `to` incluidos; `to = None`: sin
/// fin conocido, el vigente).
#[derive(Debug)]
pub struct Design {
    pub model: &'static str,
    /// Nombre del fichero oficial sin extensión («DR303e25»).
    pub name: &'static str,
    /// (ejercicio, periodo): el periodo es el trimestre (1-4) o el pago fraccionado (1-3); los
    /// meses cuentan por su trimestre y las anuales, como el 4.
    pub from: (i32, u8),
    pub to: Option<(i32, u8)>,
    json: &'static str,
}

macro_rules! anterior {
    ($f:literal) => {
        include_str!(concat!(
            "../../../fixtures/aeat/dr/anteriores/",
            $f,
            ".json"
        ))
    };
}

const fn d(
    model: &'static str,
    name: &'static str,
    from: (i32, u8),
    to: Option<(i32, u8)>,
    json: &'static str,
) -> Design {
    Design {
        model,
        name,
        from,
        to,
        json,
    }
}

/// Todos los diseños, por modelo y en orden.
pub const DESIGNS: &[Design] = &[
    // 303: ejercicios anteriores (DR_300_399, «ejercicios anteriores») y el vigente.
    d(
        "303",
        "DR303e17v20_04",
        (2017, 1),
        Some((2017, 4)),
        anterior!("DR303e17v20_04"),
    ),
    d(
        "303",
        "DR303e18v10_10",
        (2018, 1),
        Some((2018, 3)),
        anterior!("DR303e18v10_10"),
    ),
    d(
        "303",
        "DR303e18v10_40",
        (2018, 4),
        Some((2018, 4)),
        anterior!("DR303e18v10_40"),
    ),
    d(
        "303",
        "DR303e20v10_20",
        (2019, 1),
        Some((2020, 4)),
        anterior!("DR303e20v10_20"),
    ),
    d(
        "303",
        "DR303e21v103",
        (2021, 1),
        Some((2021, 2)),
        anterior!("DR303e21v103"),
    ),
    d(
        "303",
        "DR303e21v200",
        (2021, 3),
        Some((2021, 4)),
        anterior!("DR303e21v200"),
    ),
    d(
        "303",
        "DR303e22",
        (2022, 1),
        Some((2022, 4)),
        anterior!("DR303e22"),
    ),
    d(
        "303",
        "DR303e23",
        (2023, 1),
        Some((2023, 4)),
        anterior!("DR303e23"),
    ),
    d(
        "303",
        "DR303e24",
        (2024, 1),
        Some((2024, 2)),
        anterior!("DR303e24"),
    ),
    d(
        "303",
        "DR303e24v200",
        (2024, 3),
        Some((2024, 4)),
        anterior!("DR303e24v200"),
    ),
    d(
        "303",
        "DR303e25",
        (2025, 1),
        Some((2025, 4)),
        anterior!("DR303e25"),
    ),
    d("303", "DR303e26v101", (2026, 1), None, crate::m303::DR_JSON),
    // 390: uno por ejercicio.
    d(
        "390",
        "dr390e2017v3",
        (2017, 4),
        Some((2017, 4)),
        anterior!("dr390e2017v3"),
    ),
    d(
        "390",
        "dr390e2018v101",
        (2018, 4),
        Some((2018, 4)),
        anterior!("dr390e2018v101"),
    ),
    d(
        "390",
        "dr390e2019v101",
        (2019, 4),
        Some((2020, 4)),
        anterior!("dr390e2019v101"),
    ),
    d(
        "390",
        "dr390e2021",
        (2021, 4),
        Some((2021, 4)),
        anterior!("dr390e2021"),
    ),
    d(
        "390",
        "dr390e2022",
        (2022, 4),
        Some((2022, 4)),
        anterior!("dr390e2022"),
    ),
    d(
        "390",
        "dr390e2023v100",
        (2023, 4),
        Some((2023, 4)),
        anterior!("dr390e2023v100"),
    ),
    d(
        "390",
        "dr390e2024",
        (2024, 4),
        Some((2024, 4)),
        anterior!("dr390e2024"),
    ),
    // El generador del 390 escribe con el de 2025 hasta 2025 y con el de 2026 desde 2026
    // (`m390::dr_json`).
    d(
        "390",
        "dr390e2025",
        (2025, 4),
        Some((2025, 4)),
        crate::m390::DR_JSON_2025,
    ),
    d("390", "dr390e2026", (2026, 4), None, crate::m390::DR_JSON),
    // 123: hasta 2023, página 01 de 500 posiciones.
    d(
        "123",
        "DR123e15v12",
        (2015, 1),
        Some((2018, 4)),
        anterior!("DR123e15v12"),
    ),
    d(
        "123",
        "DR123e15v13",
        (2019, 1),
        Some((2023, 4)),
        anterior!("DR123e15v13"),
    ),
    d("123", "DR123e24", (2024, 1), None, crate::m123::DR123_JSON),
    // 202: el 1P de 2018 aún con el diseño de 2017.
    d(
        "202",
        "DR202e17v47",
        (2017, 1),
        Some((2018, 1)),
        anterior!("DR202e17v47"),
    ),
    d(
        "202",
        "DR202e17v48",
        (2018, 2),
        Some((2018, 3)),
        anterior!("DR202e17v48"),
    ),
    d(
        "202",
        "DR202v52",
        (2019, 1),
        Some((2022, 3)),
        anterior!("DR202v52"),
    ),
    d(
        "202",
        "DR202e23",
        (2023, 1),
        Some((2024, 3)),
        anterior!("DR202e23"),
    ),
    d("202", "DR202e25", (2025, 1), None, crate::m202::DR_JSON),
    // 200: un diseño por ejercicio (DR_200_299, «ejercicios anteriores»). Los originales pesan
    // unos 10 MB cada uno y no se guardan: el JSON es compacto (marcas de página, campos de
    // importe con casilla y el último campo de cada página), con el SHA-256 del original.
    d(
        "200",
        "DR200-2017",
        (2017, 4),
        Some((2017, 4)),
        anterior!("DR200-2017-1_00"),
    ),
    d(
        "200",
        "DR200-2018",
        (2018, 4),
        Some((2018, 4)),
        anterior!("DR200-2018"),
    ),
    d(
        "200",
        "DR200-2019",
        (2019, 4),
        Some((2019, 4)),
        anterior!("DR200-2019"),
    ),
    d(
        "200",
        "DR200-2020",
        (2020, 4),
        Some((2020, 4)),
        anterior!("DR200-2020"),
    ),
    d(
        "200",
        "DR200-2021",
        (2021, 4),
        Some((2021, 4)),
        anterior!("DR200-2021"),
    ),
    d(
        "200",
        "DR200-2022",
        (2022, 4),
        Some((2022, 4)),
        anterior!("DR200-2022"),
    ),
    d(
        "200",
        "DR200-2023",
        (2023, 4),
        Some((2023, 4)),
        anterior!("DR200-2023"),
    ),
    d(
        "200",
        "DR200-2024",
        (2024, 4),
        Some((2024, 4)),
        anterior!("DR200-2024"),
    ),
    // El de 2025 es con el que TaxPain escribe el 200; el de 2026 aún no está publicado.
    d(
        "200",
        "DR200e25",
        (2025, 4),
        Some((2025, 4)),
        crate::m200::DR_JSON,
    ),
    // 111: el diseño no cambia desde 2016 (dr111e16v17 hasta 2018 y dr111e16v18 después, con
    // las mismas posiciones).
    d(
        "111",
        "dr111e16v18",
        (2016, 1),
        None,
        crate::withholding::DR111_JSON,
    ),
    // 115 y 130: el vigente dice «2019 y siguientes»; los anteriores no los tiene TaxPain.
    d(
        "115",
        "DR115e15v13",
        (2019, 1),
        None,
        crate::withholding::DR115_JSON,
    ),
    d("130", "DR130e15v12", (2019, 1), None, crate::m130::DR_JSON),
];

/// Diseño con que TaxPain genera cada modelo y último ejercicio para el que se ha comprobado en
/// la Sede (índice «Diseños de registro», `fixtures/aeat/dr/SOURCES.md`, descargado en octubre
/// de 2026). Un ejercicio posterior no falla: se genera con ese mismo diseño (el último vigente)
/// y el borrador lo avisa ([`aviso_diseno_no_publicado`]) hasta que se compruebe el del año.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Generador {
    pub model: &'static str,
    /// Nombre del diseño con que se escribe el fichero de los ejercicios posteriores.
    pub name: &'static str,
    /// Último ejercicio para el que la Sede da ese diseño por vigente.
    pub comprobado_hasta: i32,
    /// La Sede lo publica como «… y siguientes» (sin fecha de fin): lo normal es que valga
    /// también para el año siguiente, pero la AEAT puede cambiarlo con la orden anual.
    pub y_siguientes: bool,
}

const fn g(model: &'static str, name: &'static str, hasta: i32, sig: bool) -> Generador {
    Generador {
        model,
        name,
        comprobado_hasta: hasta,
        y_siguientes: sig,
    }
}

/// Todos los modelos que TaxPain escribe como fichero. Autoliquidaciones periódicas: los
/// periodos de 2026 ya se presentan con ellos. Informativas anuales y 200: el diseño publicado es
/// el del ejercicio 2025 (presentado en 2026); el de 2026 lo publica la AEAT con la orden que
/// aprueba los modelos (normalmente en noviembre o diciembre).
pub const GENERADORES: &[Generador] = &[
    g("303", "DR303e26v101", 2026, true),
    g("390", "dr390e2026", 2026, true),
    g("130", "DR130e15v12", 2026, true),
    g("111", "dr111e16v18", 2026, true),
    g("115", "DR115e15v13", 2026, true),
    g("123", "DR123e24", 2026, true),
    g("202", "DR202e25", 2026, true),
    g("349", "DR_Anexo_349", 2026, true),
    g("200", "DR200e25", 2025, false),
    g("347", "DR347 de 2025", 2025, true),
    g("190", "DISENOS_LOGICOS_190_2025", 2025, false),
    g("180", "DR_Mod_180_2023", 2025, false),
    g("193", "DR_Modelo_193_2025", 2025, false),
    g("184", "DR_Modelo_184_2025", 2025, false),
];

/// El generador de `model`, si TaxPain escribe ese modelo como fichero.
pub fn generador(model: &str) -> Option<&'static Generador> {
    GENERADORES.iter().find(|g| g.model == model)
}

/// Aviso cuando el ejercicio es posterior al último diseño comprobado: el fichero se genera
/// igual, con el último vigente, y se dice cuál. `None` si el diseño del ejercicio está
/// comprobado o el modelo no se genera como fichero.
pub fn aviso_diseno_no_publicado(model: &str, year: i32) -> Option<String> {
    let g = generador(model)?;
    if year <= g.comprobado_hasta {
        return None;
    }
    // El nombre real con que se escribe (el 390 elige por ejercicio).
    let usado = generator_design(model, year, "0A").map_or(g.name, |d| d.name);
    Some(if g.y_siguientes {
        format!(
            "Diseño de registro del {model} de {year} no publicado todavía: se usa el de {usado} \
             (la Sede lo da por vigente «y siguientes»; comprobado hasta {}). Si la AEAT publica \
             otro para {year}, actualiza TaxPain antes de presentar el fichero; las casillas del \
             borrador sí valen.",
            g.comprobado_hasta
        )
    } else {
        format!(
            "Diseño de registro del {model} de {year} no publicado todavía: se usa el de {usado} \
             (el del ejercicio {}). Comprueba en la Sede que admite el fichero o actualiza \
             TaxPain antes de presentarlo; las casillas del borrador sí valen.",
            g.comprobado_hasta
        )
    })
}

/// El periodo como número para comparar dentro del ejercicio: trimestre «1T»…«4T», mes «01»…«12»
/// (por su trimestre), pago fraccionado «1P»…«3P» y anual «0A» (como el 4). `None` si no es un
/// periodo de la AEAT.
pub fn period_index(period: &str) -> Option<u8> {
    let p = period.trim().to_ascii_uppercase();
    let b = p.as_bytes();
    match (b.first(), b.get(1), b.len()) {
        (Some(d @ b'1'..=b'4'), Some(b'T'), 2) => Some(d - b'0'),
        (Some(d @ b'1'..=b'3'), Some(b'P'), 2) => Some(d - b'0'),
        (Some(b'0'), Some(b'A'), 2) => Some(4),
        _ => {
            let m: u8 = p.parse().ok()?;
            (1..=12).contains(&m).then(|| m.div_ceil(3))
        }
    }
}

/// El diseño con que se lee una declaración de `model`, `year` y `period`. `None`: TaxPain no
/// tiene el diseño de ese ejercicio o periodo.
pub fn design(model: &str, year: i32, period: &str) -> Option<&'static Design> {
    let at = (year, period_index(period)?);
    DESIGNS
        .iter()
        .filter(|d| d.model == model)
        .find(|d| d.from <= at && d.to.is_none_or(|to| at <= to))
}

impl Design {
    /// El diseño ya interpretado (se interpreta la primera vez que se pide).
    pub fn spec(&self) -> &'static Spec {
        static CACHE: OnceLock<Mutex<HashMap<&'static str, &'static Spec>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut c = cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        c.entry(self.name)
            .or_insert_with(|| Box::leak(Box::new(Spec::parse(self.json))))
    }
}

/// El diseño con que TaxPain genera el fichero de ese modelo y ejercicio: el vigente, salvo el
/// 390, que hasta 2025 sigue con el de 2025 (`m390::dr_json`).
fn generator_json(model: &str, year: i32) -> Option<&'static str> {
    Some(match model {
        "303" => crate::m303::DR_JSON,
        "390" => crate::m390::dr_json(year),
        "123" => crate::m123::DR123_JSON,
        "202" => crate::m202::DR_JSON,
        "200" => crate::m200::DR_JSON,
        "111" => crate::withholding::DR111_JSON,
        "115" => crate::withholding::DR115_JSON,
        "130" => crate::m130::DR_JSON,
        _ => return None,
    })
}

/// El diseño (de [`DESIGNS`]) con que TaxPain genera el fichero de ese ejercicio y periodo, sea
/// o no el que le corresponde. `None` si TaxPain no genera ese modelo por posiciones.
pub fn generator_design(model: &str, year: i32, _period: &str) -> Option<&'static Design> {
    let json = generator_json(model, year)?;
    // Por el contenido y no por la dirección: dos usos de la misma constante no tienen por qué
    // compartirla.
    DESIGNS.iter().find(|d| d.model == model && d.json == json)
}

/// El diseño de ese ejercicio y periodo, ya interpretado.
pub fn spec_for(model: &str, year: i32, period: &str) -> Option<&'static Spec> {
    design(model, year, period).map(Design::spec)
}

/// ¿Genera TaxPain el fichero de ese ejercicio y periodo con el diseño que le corresponde? Si no,
/// el fichero no sirve para presentar.
pub fn generates_with_right_design(model: &str, year: i32, period: &str) -> bool {
    design(model, year, period)
        .zip(generator_design(model, year, period))
        .is_some_and(|(d, g)| d.name == g.name)
}

/// Fichero de una autoliquidación con sólo sus casillas de importe, escrito con el diseño de
/// registro de su ejercicio y periodo: como el que presenta una gestoría con su programa. Sirve
/// para las pruebas y para comparar; no para presentar (no lleva la identificación). `None` si
/// TaxPain no tiene ese diseño.
pub fn file_with_boxes(
    model: &str,
    year: i32,
    period: &str,
    boxes: &[(u16, taxpain_core::Money)],
) -> Option<String> {
    use crate::dr::PageWriter;
    let spec = spec_for(model, year, period)?;
    let mut out = format!("<T{model}0{year}{period}0000>");
    for page in &spec.pages {
        let marker: String = page
            .fields
            .iter()
            .take_while(|f| f.constant.is_some())
            .filter_map(|f| f.constant.clone())
            .collect();
        if marker.len() < 8 || !marker.starts_with("<T") {
            continue;
        }
        let mut w = PageWriter::new(page);
        let mut any = false;
        for f in page
            .fields
            .iter()
            .filter(|f| f.is_amount() && f.constant.is_none())
        {
            // En el 111 el diseño no trae la casilla: el campo `11 + n` es la [n].
            let b = f
                .box_
                .or_else(|| (model == "111" && (12..=41).contains(&f.n)).then(|| f.n - 11));
            if let Some((_, m)) = b.and_then(|b| boxes.iter().find(|(x, _)| *x == b))
                && w.amount(f.n, *m).is_ok()
            {
                any = true;
            }
        }
        if any || page.sheet.ends_with("01") {
            let mut page_text = w.finish().ok()?;
            // La marca de cierre bien escrita aunque el diseño oficial la tenga mal (página 7
            // del 390 de 2017 a 2023: «</T3900700>»), como la escribe la AEAT.
            let close = format!("</{}", &marker[1..]);
            if let Some(last) = page.fields.last()
                && !page_text.ends_with(&close)
                && close.chars().count() <= last.len
            {
                let keep = page_text.chars().count() - last.len;
                page_text = page_text.chars().take(keep).collect::<String>()
                    + &format!("{close:<width$}", width = last.len);
            }
            out.push_str(&page_text);
        }
    }
    out.push_str(&format!("</T{model}0{year}{period}0000>"));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periodos() {
        assert_eq!(period_index("1T"), Some(1));
        assert_eq!(period_index("4t"), Some(4));
        assert_eq!(period_index("2P"), Some(2));
        assert_eq!(period_index("0A"), Some(4));
        assert_eq!(period_index("07"), Some(3));
        assert_eq!(period_index("12"), Some(4));
        assert_eq!(period_index("13"), None);
        assert_eq!(period_index("5T"), None);
        assert_eq!(period_index(""), None);
    }

    #[test]
    fn cada_periodo_con_su_diseno() {
        let name = |m: &str, y: i32, p: &str| design(m, y, p).map(|d| d.name);
        assert_eq!(name("303", 2025, "3T"), Some("DR303e25"));
        assert_eq!(name("303", 2026, "1T"), Some("DR303e26v101"));
        assert_eq!(name("303", 2027, "1T"), Some("DR303e26v101"));
        assert_eq!(name("303", 2024, "2T"), Some("DR303e24"));
        assert_eq!(name("303", 2024, "3T"), Some("DR303e24v200"));
        assert_eq!(name("303", 2018, "4T"), Some("DR303e18v10_40"));
        assert_eq!(name("303", 2018, "11"), Some("DR303e18v10_40"));
        assert_eq!(name("303", 2020, "2T"), Some("DR303e20v10_20"));
        assert_eq!(name("303", 2016, "4T"), None);
        assert_eq!(name("390", 2020, "0A"), Some("dr390e2019v101"));
        assert_eq!(name("390", 2025, "0A"), Some("dr390e2025"));
        assert_eq!(name("390", 2026, "0A"), Some("dr390e2026"));
        assert_eq!(name("202", 2018, "1P"), Some("DR202e17v47"));
        assert_eq!(name("202", 2018, "2P"), Some("DR202e17v48"));
        assert_eq!(name("202", 2024, "3P"), Some("DR202e23"));
        assert_eq!(name("202", 2026, "2P"), Some("DR202e25"));
        assert_eq!(name("123", 2023, "4T"), Some("DR123e15v13"));
        assert_eq!(name("123", 2024, "1T"), Some("DR123e24"));
        assert_eq!(name("111", 2017, "1T"), Some("dr111e16v18"));
        assert_eq!(name("115", 2018, "1T"), None);
        assert_eq!(name("349", 2025, "1T"), None, "informativa: otro lector");
    }

    #[test]
    fn todos_los_disenos_se_interpretan_y_no_se_solapan() {
        for d in DESIGNS {
            let s = d.spec();
            assert!(!s.pages.is_empty(), "{}", d.name);
            assert!(d.to.is_none_or(|to| d.from <= to), "{}", d.name);
        }
        for m in ["303", "390", "123", "202"] {
            let mine: Vec<&Design> = DESIGNS.iter().filter(|d| d.model == m).collect();
            for w in mine.windows(2) {
                let to = w[0].to.unwrap_or((i32::MAX, 4));
                assert!(to < w[1].from, "{} y {} se solapan", w[0].name, w[1].name);
            }
        }
    }

    #[test]
    fn solo_el_vigente_genera() {
        assert!(generates_with_right_design("303", 2026, "3T"));
        assert!(!generates_with_right_design("303", 2025, "3T"));
        assert!(generates_with_right_design("111", 2019, "2T"));
        // El 390, con el de 2025 hasta 2025 y con el de 2026 desde 2026.
        let g = |m: &str, y: i32| generator_design(m, y, "0A").map(|d| d.name);
        assert_eq!(g("390", 2017), Some("dr390e2025"));
        assert_eq!(g("390", 2024), Some("dr390e2025"));
        assert_eq!(g("390", 2025), Some("dr390e2025"));
        assert_eq!(g("390", 2026), Some("dr390e2026"));
        assert_eq!(g("390", 2027), Some("dr390e2026"));
        assert!(!generates_with_right_design("390", 2024, "0A"));
        assert!(generates_with_right_design("390", 2025, "0A"));
        assert!(generates_with_right_design("390", 2026, "0A"));
        assert!(generates_with_right_design("390", 2027, "0A"));
        // El 200 de 2025 con el suyo; el de 2026 aún no está publicado.
        assert_eq!(g("200", 2026), Some("DR200e25"));
        assert!(generates_with_right_design("200", 2025, "0A"));
        assert!(!generates_with_right_design("200", 2026, "0A"));
        // Las informativas no se generan con estos diseños.
        assert_eq!(g("347", 2025), None);
    }

    /// 2027 no falla en ningún modelo: se genera con el último diseño vigente y se avisa de cuál.
    #[test]
    fn cada_modelo_en_2027_usa_el_ultimo_vigente_con_aviso() {
        for gen_ in GENERADORES {
            let m = gen_.model;
            assert_eq!(
                aviso_diseno_no_publicado(m, gen_.comprobado_hasta),
                None,
                "{m}"
            );
            let w = aviso_diseno_no_publicado(m, 2027).unwrap_or_else(|| panic!("{m}"));
            assert!(
                w.starts_with(&format!("Diseño de registro del {m} de 2027 no publicado")),
                "{w}"
            );
            // El diseño que se nombra es el que escribe el fichero.
            let usado = generator_design(m, 2027, "0A").map_or(gen_.name, |d| d.name);
            assert!(w.contains(&format!("se usa el de {usado} ")), "{w}");
            if let Some(d) = generator_design(m, 2027, "0A") {
                assert_eq!(
                    d.name, gen_.name,
                    "{m}: la tabla nombra el diseño del generador"
                );
            }
        }
        // Todos los modelos con fichero por posiciones están en la tabla.
        for d in DESIGNS {
            assert!(generador(d.model).is_some(), "{} sin generador", d.model);
        }
        assert_eq!(aviso_diseno_no_publicado("100", 2027), None, "sin fichero");
        // El 200 y las informativas de 2026 tampoco están publicados (se presentan en 2027).
        assert!(aviso_diseno_no_publicado("200", 2026).is_some());
        assert!(aviso_diseno_no_publicado("190", 2026).is_some());
        assert!(aviso_diseno_no_publicado("303", 2026).is_none());
    }
}

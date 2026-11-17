//! Ficheros de criptomonedas de la carpeta de la empresa: sueltos (una hoja de cálculo o un CSV
//! que entiende [`super::importar::leer`]) o dentro de un zip, como los que se preparan con los
//! historiales de los exchanges. Del zip se abren las hojas de cálculo, los
//! CSV y los zip de dentro (dos niveles); lo que no se puede leer se dice con su motivo.

use std::io::Read;

use sha2::{Digest, Sha256};
use taxpain_core::cripto::{Hueco, Metodo, calcular};
use taxpain_vault::Vault;
use time::{Date, OffsetDateTime};

use super::importar::{self, Lectura};
use crate::error::Result;
// Como mucho tantas entradas en un zip (como en las hojas de cálculo): más ni se recorren.
use crate::import::limites::MAX_ENTRADAS;
use crate::import::norm;

/// Zip dentro de zip: como mucho tantos niveles.
const MAX_NIVEL: u8 = 2;
/// Ficheros que se miran dentro de un zip.
const MAX_FICHEROS: usize = 500;
/// Tamaño máximo de un fichero de dentro, descomprimido.
const MAX_TAM: u64 = 30 * 1024 * 1024;
/// Como mucho tanto descomprimido entre todos los ficheros del zip (y de los zip de dentro): un
/// zip pequeño puede declarar gigas (cientos de ficheros de 30 MB, y zip dentro de zip).
const MAX_TOTAL: u64 = 256 * 1024 * 1024;
const HOJAS: [&str; 6] = ["xlsx", "xlsm", "xls", "ods", "csv", "txt"];

/// Un fichero leído: su ruta (dentro del zip, si lo es) y lo que trae.
#[derive(Clone, Debug)]
pub struct Leido {
    pub ruta: String,
    pub lectura: Lectura,
}

/// Lo que hay de cripto en un fichero de la carpeta.
#[derive(Clone, Debug, Default)]
pub struct Paquete {
    pub leidos: Vec<Leido>,
    /// Ficheros de dentro de un zip que no se leen, con el motivo.
    pub no_leidos: Vec<(String, String)>,
    /// Ficheros de dentro de un zip (sin contar los zip de dentro).
    pub ficheros: usize,
    /// Sin punto de partida: ventas o traspasos de los ficheros con más unidades de las que
    /// había en ellos (faltan operaciones de antes o de otros sitios).
    pub huecos: Vec<Hueco>,
    /// Huellas de lo ya leído: el mismo fichero suelto en el zip y dentro de otro zip de dentro
    /// se lee una vez.
    vistos: Vec<[u8; 32]>,
}

impl Paquete {
    pub fn ops(&self) -> usize {
        self.leidos.iter().map(|l| l.lectura.ops.len()).sum()
    }

    pub fn precios(&self) -> usize {
        self.leidos.iter().map(|l| l.lectura.prices.len()).sum()
    }

    pub fn duplicadas(&self) -> usize {
        self.leidos.iter().map(|l| l.lectura.duplicates).sum()
    }

    /// Primera y última fecha de las operaciones y los precios.
    pub fn fechas(&self) -> (Option<Date>, Option<Date>) {
        let fechas = || {
            self.leidos.iter().flat_map(|l| {
                l.lectura
                    .ops
                    .iter()
                    .map(|o| o.op.date)
                    .chain(l.lectura.prices.iter().map(|p| p.date))
            })
        };
        (fechas().min(), fechas().max())
    }

    /// ¿Es de un formato exacto (CoinTracking, Kraken), que no puede ser otra cosa?
    pub fn exacto(&self) -> bool {
        self.leidos.iter().any(|l| exacta(&l.lectura))
    }

    /// «Kraken (CRYPTO/KRAKEN/ledgers.csv): 266 operaciones, 2019-01-01 a 2023-04-13 · …».
    pub fn resumen(&self) -> String {
        self.leidos
            .iter()
            .map(|l| {
                let x = &l.lectura;
                let mut partes = Vec::new();
                if !x.ops.is_empty() {
                    partes.push(format!("{} operaciones", x.ops.len()));
                }
                if !x.prices.is_empty() {
                    partes.push(format!("{} precios de cierre", x.prices.len()));
                }
                if x.duplicates > 0 {
                    partes.push(format!("{} ya importadas", x.duplicates));
                }
                let fechas: Vec<Date> = x
                    .ops
                    .iter()
                    .map(|o| o.op.date)
                    .chain(x.prices.iter().map(|p| p.date))
                    .collect();
                if let (Some(a), Some(b)) = (fechas.iter().min(), fechas.iter().max()) {
                    partes.push(format!("del {a} al {b}"));
                }
                format!("{} ({}): {}", tipo(&x.kind), l.ruta, partes.join(", "))
            })
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// Avisos para enseñar: operaciones incompletas, los de cada lectura (los primeros) y los
    /// ficheros no leídos.
    pub fn avisos(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.extend(super::inicio::texto_huecos(&self.huecos));
        for l in &self.leidos {
            let n = l.lectura.warnings.len();
            out.extend(
                l.lectura
                    .warnings
                    .iter()
                    .take(3)
                    .map(|w| format!("{}: {w}", l.ruta)),
            );
            if n > 3 {
                out.push(format!("{}: y {} avisos más", l.ruta, n - 3));
            }
        }
        out.extend(
            self.no_leidos
                .iter()
                .map(|(r, m)| format!("no se ha podido leer {r}: {m}")),
        );
        out
    }
}

fn exacta(l: &Lectura) -> bool {
    matches!(l.kind.as_str(), "cointracking" | "kraken")
}

/// ¿Es de criptomonedas? Los formatos exactos (CoinTracking, Kraken), sí; una hoja genérica de
/// fecha, tipo y cantidad o de valores y cantidades por fecha también puede ser un inventario o
/// un listado del banco: sólo si trae alguna criptomoneda conocida.
fn es_cripto(l: &Lectura) -> bool {
    const CONOCIDAS: [&str; 24] = [
        "BTC", "ETH", "SOL", "ADA", "XRP", "DOGE", "USDT", "USDC", "BNB", "LTC", "DOT", "AVAX",
        "MATIC", "LINK", "TRX", "XLM", "ZEC", "BCH", "ALGO", "ATOM", "XMR", "ETC", "SHIB", "DAI",
    ];
    exacta(l)
        || l.ops
            .iter()
            .map(|o| o.op.ticker.as_str())
            .chain(l.prices.iter().map(|p| p.ticker.as_str()))
            .any(|t| CONOCIDAS.contains(&t))
}

/// Nombre de lo leído para el usuario.
pub fn tipo(kind: &str) -> &'static str {
    match kind {
        "cointracking" => "Trade List de CoinTracking",
        "kraken" => "ledgers de Kraken",
        "hoja" => "hoja de operaciones",
        "saldos" => "saldos por fecha",
        _ => "criptomonedas",
    }
}

fn es_zip(name: &str, bytes: &[u8]) -> bool {
    name.to_lowercase().ends_with(".zip") && bytes.starts_with(b"PK")
}

/// Lo que hay de cripto en un fichero (una hoja de cálculo, un CSV o un zip). `None` si no es un
/// zip y no se lee como cripto, o si es un zip sin nada de cripto que se pueda leer (entonces el
/// motivo va en `Err`).
pub fn leer(v: &Vault, name: &str, bytes: &[u8]) -> std::result::Result<Paquete, String> {
    let mut p = Paquete::default();
    let sin_inicio = || super::inicio::inicio(v).ok().flatten().is_none();
    if es_zip(name, bytes) {
        abrir_zip(v, "", bytes, 1, &mut p, &mut Presupuesto::default());
        if p.leidos.is_empty() {
            return Err(motivo_zip(&p));
        }
        if sin_inicio() {
            p.huecos = huecos(&p.leidos);
        }
        return Ok(p);
    }
    match importar::leer(v, name, bytes) {
        Ok(l) if !es_cripto(&l) => Err("no trae ninguna criptomoneda conocida".into()),
        Ok(l) if !l.ops.is_empty() || !l.prices.is_empty() => {
            p.leidos.push(Leido {
                ruta: name.to_owned(),
                lectura: l,
            });
            if p.ops() > 0 && sin_inicio() {
                p.huecos = huecos(&p.leidos);
            }
            Ok(p)
        }
        Ok(_) => Err("no trae operaciones ni precios".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// Ventas o traspasos de los ficheros leídos con más unidades de las que había en ellos (con
/// las plataformas por nombre).
fn huecos(leidos: &[Leido]) -> Vec<Hueco> {
    let mut nombres: Vec<String> = Vec::new();
    let mut id = |n: &str| -> i64 {
        let n = n.trim().to_lowercase();
        let i = nombres.iter().position(|x| *x == n).unwrap_or_else(|| {
            nombres.push(n);
            nombres.len() - 1
        });
        i64::try_from(i).unwrap_or(i64::MAX - 1) + 1
    };
    let mut ops = Vec::new();
    for (n, d) in leidos.iter().flat_map(|l| l.lectura.ops.iter()).enumerate() {
        let mut o = d.op.clone();
        o.id = i64::try_from(n).unwrap_or(i64::MAX);
        o.platform_id = id(&d.platform);
        o.to_platform_id = d.to_platform.as_deref().map(&mut id);
        ops.push(super::a_operacion(&o));
    }
    calcular(&ops, Metodo::Pmp, None, true).huecos
}

/// Motivo de un zip sin nada de cripto que se pueda leer.
fn motivo_zip(p: &Paquete) -> String {
    if p.ficheros == 0 {
        return "zip vacío o que no se puede abrir".into();
    }
    let conocidos: Vec<String> = p
        .no_leidos
        .iter()
        .filter(|(_, m)| !m.starts_with("no es") && !m.starts_with("dentro de un zip"))
        .map(|(r, m)| format!("{r}: {m}"))
        .collect();
    let ficheros = if p.ficheros == 1 {
        "1 fichero".to_owned()
    } else {
        format!("{} ficheros", p.ficheros)
    };
    if conocidos.is_empty() {
        format!(
            "zip con {ficheros}: no se ha podido leer ninguno como historial de criptomonedas \
             (de un zip sólo se abren los de cripto)"
        )
    } else {
        format!(
            "zip con {ficheros}: no se ha podido leer ninguno: {}",
            conocidos.join("; ")
        )
    }
}

/// Lo que queda por descomprimir de todo el paquete (todos los niveles).
#[derive(Default)]
struct Presupuesto {
    leido: u64,
    agotado: bool,
}

const AGOTADO: &str = "el zip descomprime demasiado: no se lee el resto";

fn abrir_zip(
    v: &Vault,
    prefijo: &str,
    bytes: &[u8],
    nivel: u8,
    p: &mut Paquete,
    presupuesto: &mut Presupuesto,
) {
    let este = || {
        if prefijo.is_empty() {
            "el zip".to_owned()
        } else {
            prefijo.trim_end_matches('/').to_owned()
        }
    };
    let Ok(mut z) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        p.no_leidos.push((este(), "no se puede abrir".into()));
        return;
    };
    if z.len() > MAX_ENTRADAS {
        p.no_leidos
            .push((este(), "tiene demasiados ficheros dentro: no se lee".into()));
        return;
    }
    for i in 0..z.len().min(MAX_FICHEROS) {
        if presupuesto.agotado {
            return;
        }
        let Ok(mut f) = z.by_index(i) else {
            continue;
        };
        let Ok(nombre) = f.name().map(std::borrow::Cow::into_owned) else {
            continue;
        };
        let base = nombre.rsplit('/').next().unwrap_or(&nombre).to_owned();
        if f.is_dir()
            || nombre.starts_with("__MACOSX")
            || base.starts_with('.')
            || base.is_empty()
            || f.size() == 0
        {
            continue;
        }
        let ruta = format!("{prefijo}{nombre}");
        let ext = base.rsplit('.').next().unwrap_or("").to_lowercase();
        if ext == "zip" {
            if nivel >= MAX_NIVEL {
                p.no_leidos
                    .push((ruta, "zip dentro de otro zip: demasiados niveles".into()));
                continue;
            }
        } else {
            p.ficheros += 1;
            if !HOJAS.contains(&ext.as_str()) {
                p.no_leidos.push((
                    ruta,
                    "dentro de un zip sólo se leen las hojas de cálculo y los CSV de \
                     criptomonedas"
                        .into(),
                ));
                continue;
            }
        }
        if f.size() > MAX_TAM {
            p.no_leidos.push((ruta, "demasiado grande".into()));
            continue;
        }
        let queda = MAX_TOTAL.saturating_sub(presupuesto.leido);
        if f.size() > queda {
            presupuesto.agotado = true;
            p.no_leidos.push((ruta, AGOTADO.into()));
            return;
        }
        let mut datos = Vec::new();
        // El lector de zip hace cumplir el tamaño declarado; el tope es por si acaso.
        if f.by_ref()
            .take(MAX_TAM.min(queda))
            .read_to_end(&mut datos)
            .is_err()
        {
            p.no_leidos.push((ruta, "no se puede descomprimir".into()));
            continue;
        }
        drop(f);
        presupuesto.leido = presupuesto.leido.saturating_add(datos.len() as u64);
        if ext == "zip" {
            abrir_zip(v, &format!("{ruta}/"), &datos, nivel + 1, p, presupuesto);
            continue;
        }
        // El mismo fichero dos veces en el zip (suelto y dentro de otro zip): se lee una vez.
        let huella: [u8; 32] = Sha256::digest(&datos).into();
        if p.vistos.contains(&huella) {
            p.no_leidos
                .push((ruta, "es el mismo fichero que otro del zip".into()));
            continue;
        }
        p.vistos.push(huella);
        match importar::leer(v, &base, &datos) {
            Ok(l) if !es_cripto(&l) => p.no_leidos.push((
                ruta,
                "no es un historial de criptomonedas que TaxPain sepa leer".into(),
            )),
            Ok(l) if !l.ops.is_empty() || !l.prices.is_empty() => {
                p.leidos.push(Leido { ruta, lectura: l });
            }
            Ok(_) => p
                .no_leidos
                .push((ruta, "no trae operaciones ni precios".into())),
            Err(_) => {
                let m = conocido(&base, &datos).unwrap_or_else(|| {
                    "no es un historial de criptomonedas que TaxPain sepa leer".into()
                });
                p.no_leidos.push((ruta, m));
            }
        }
    }
    if z.len() > MAX_FICHEROS {
        p.no_leidos.push((
            prefijo.trim_end_matches('/').to_owned(),
            format!("sólo se miran los {MAX_FICHEROS} primeros ficheros del zip"),
        ));
    }
}

/// Motivo de un historial de exchange que se reconoce pero que no se importa.
fn conocido(name: &str, bytes: &[u8]) -> Option<String> {
    let hojas = crate::import::sheets(name, bytes).ok()?;
    let cab: Vec<String> = hojas
        .iter()
        .flat_map(|h| h.filas.iter().take(5))
        .flat_map(|f| f.iter().map(|c| norm(c)))
        .collect();
    let tiene = |k: &str| {
        let k = norm(k);
        cab.iter().any(|c| c.starts_with(&k))
    };
    Some(
        if tiene("contracts") && tiene("exec price") {
            "contratos de derivados de Bybit (futuros inversos): su resultado es de derivados, no \
             una compraventa de cripto; no se importa"
        } else if tiene("realized p&l") || (tiene("coin") && tiene("wallet balance")) {
            "historial de saldos de Bybit: movimientos sin valor en euros y resultados de \
             derivados; no se importa"
        } else if tiene("spot pairs") || (tiene("pair") && tiene("side") && tiene("executed")) {
            "operaciones de cripto por cripto (Bybit o Binance) sin valor en euros: aún no se \
             leen; impórtalas desde CoinTracking («Trade List») si las necesitas"
        } else if tiene("coin") && tiene("wallet address") {
            "retiradas de Bybit: sin los depósitos y las operaciones de la cuenta no cuadran; \
             no se importan"
        } else if tiene("ordertxid") || (tiene("pair") && tiene("vol")) {
            "«trades» de Kraken: TaxPain lee el «ledgers» de Kraken, que trae lo mismo y más"
        } else {
            return None;
        }
        .into(),
    )
}

/// Guarda todo lo leído: (operaciones nuevas, duplicadas, precios).
pub fn aplicar(v: &Vault, p: &Paquete, now: OffsetDateTime) -> Result<(usize, usize, usize)> {
    let mut total = (0, 0, 0);
    for l in &p.leidos {
        let (n, d, pr) = importar::aplicar(v, &l.lectura, now)?;
        total.0 += n;
        total.1 += d;
        total.2 += pr;
    }
    Ok(total)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::io::{Cursor, Write};
    use std::sync::Arc;

    use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider};

    use super::*;

    fn vault() -> Vault {
        Vault::open_in_memory(
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
        )
        .unwrap()
    }

    #[test]
    fn un_zip_pequeno_que_declara_mucho_se_deja_de_leer() {
        // Un zip de dentro de casi 30 MB (ceros: no se puede abrir), comprimido una vez y
        // copiado 12 veces: unos 350 MB descomprimidos en un zip de menos de 1 MB.
        let mut uno = zip::ZipWriter::new(Cursor::new(Vec::new()));
        uno.start_file("a.zip", zip::write::SimpleFileOptions::default())
            .unwrap();
        uno.write_all(&vec![0; 29 * 1024 * 1024]).unwrap();
        let uno = uno.finish().unwrap().into_inner();
        let mut origen = zip::ZipArchive::new(Cursor::new(uno)).unwrap();
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..12 {
            z.raw_copy_file_rename(origen.by_index(0).unwrap(), format!("{i}.zip"))
                .unwrap();
        }
        let bytes = z.finish().unwrap().into_inner();
        assert!(bytes.len() < 1024 * 1024, "{}", bytes.len());
        let mut p = Paquete::default();
        let mut presupuesto = Presupuesto::default();
        abrir_zip(&vault(), "", &bytes, 1, &mut p, &mut presupuesto);
        assert!(presupuesto.agotado);
        assert!(presupuesto.leido <= MAX_TOTAL);
        assert!(
            p.no_leidos.iter().any(|(_, m)| m == AGOTADO),
            "{:?}",
            p.no_leidos
        );
        // Se para al agotarse: no llega a abrir los 12.
        let abiertos = p
            .no_leidos
            .iter()
            .filter(|(_, m)| m == "no se puede abrir")
            .count();
        assert!(abiertos < 12, "{abiertos}");
    }

    #[test]
    fn un_zip_con_demasiadas_entradas_no_se_recorre() {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for i in 0..=MAX_ENTRADAS {
            z.start_file(format!("{i}.txt"), o).unwrap();
            z.write_all(b"x").unwrap();
        }
        let bytes = z.finish().unwrap().into_inner();
        let mut p = Paquete::default();
        abrir_zip(&vault(), "", &bytes, 1, &mut p, &mut Presupuesto::default());
        assert_eq!(p.ficheros, 0);
        assert!(
            p.no_leidos
                .iter()
                .any(|(_, m)| m.contains("demasiados ficheros")),
            "{:?}",
            p.no_leidos
        );
    }
}

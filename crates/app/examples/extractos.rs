//! Prueba los lectores de extractos con ficheros reales, sin tocar ningún dato: dice qué ha
//! leído de cada fichero, si cuadra con sus saldos y qué meses de cada cuenta quedan sin
//! extracto.
//!
//! ```text
//! cargo run -q -p taxpain-app --example extractos -- <fichero|carpeta>... [--movs] [--todos]
//! cargo run -q -p taxpain-app --example extractos -- --lista <lista.txt> --base <carpeta>
//! ```
//! `--movs` enseña los movimientos; `--todos`, también los ficheros de una carpeta que no son
//! extractos; `--lista`, sólo los ficheros de la lista (una ruta por línea, relativa a `--base`;
//! lo que vaya detrás de «  ||  » se ignora). `--importar` los importa además en un almacén en
//! memoria (de usar y tirar) y comprueba cuentas, duplicados y que el saldo encadene.

#![allow(clippy::print_stdout, clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use taxpain_app::extractos::{self, Cuadre};

const EXTENSIONES: [&str; 20] = [
    "pdf", "xls", "xlsx", "xlsm", "ods", "csv", "txt", "html", "htm", "ofx", "qfx", "qif", "sta",
    "mt940", "940", "n43", "aeb", "q43", "c43", "xml",
];

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let movs = quitar(&mut args, "--movs");
    let importar = quitar(&mut args, "--importar");
    let todos = quitar(&mut args, "--todos");
    let lista = valor(&mut args, "--lista");
    let base = valor(&mut args, "--base").map(PathBuf::from);
    let mut ficheros: Vec<PathBuf> = Vec::new();
    if let Some(l) = lista {
        let base = base.unwrap_or_else(|| PathBuf::from("."));
        for linea in std::fs::read_to_string(&l).unwrap().lines() {
            let ruta = linea.split("  ||  ").next().unwrap_or("").trim();
            if !ruta.is_empty() {
                ficheros.push(base.join(ruta));
            }
        }
    }
    let mut desde_carpeta = BTreeSet::new();
    for a in &args {
        let p = PathBuf::from(a);
        if p.is_dir() {
            recorrer(&p, &mut ficheros);
            desde_carpeta.insert(p);
        } else {
            ficheros.push(p);
        }
    }
    if ficheros.is_empty() {
        eprintln!(
            "uso: extractos <fichero|carpeta>... [--movs] [--todos] | --lista <l> --base <c>"
        );
        std::process::exit(2);
    }
    let mut cuenta = Totales::default();
    let mut cobertura: BTreeMap<String, Vec<(time::Date, time::Date, String)>> = BTreeMap::new();
    for f in &ficheros {
        let nombre = f.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = match std::fs::read(f) {
            Ok(b) => b,
            Err(e) => {
                println!("{}: no se puede leer: {e}", f.display());
                cuenta.errores += 1;
                continue;
            }
        };
        match extractos::leer(&nombre, &bytes) {
            Ok(leidos) => {
                for l in leidos {
                    let e = &l.extracto;
                    let ultimos: String = e
                        .cuenta
                        .chars()
                        .rev()
                        .take(4)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    let estado = match &l.cuadre {
                        Cuadre::Cuadra => {
                            cuenta.cuadran += 1;
                            "CUADRA".to_owned()
                        }
                        Cuadre::SinSaldos => {
                            cuenta.sin_saldos += 1;
                            "SIN SALDOS".to_owned()
                        }
                        Cuadre::NoCuadra {
                            diferencia,
                            fila,
                            detalle,
                        } => {
                            cuenta.no_cuadran += 1;
                            format!("NO CUADRA ({diferencia}, fila {fila:?}: {detalle})")
                        }
                    };
                    println!(
                        "{}: {} · {} · {:?} …{} · {} · {}–{} · {} movs · saldo {} → {} · {}{}",
                        f.display(),
                        e.formato,
                        if e.banco.is_empty() { "?" } else { &e.banco },
                        e.clase,
                        ultimos,
                        e.divisa,
                        e.desde.map(|d| d.to_string()).unwrap_or_default(),
                        e.hasta.map(|d| d.to_string()).unwrap_or_default(),
                        e.movimientos.len(),
                        e.saldo_inicial
                            .map(|m| en_divisa(m, &e.divisa))
                            .unwrap_or("?".into()),
                        e.saldo_final
                            .map(|m| en_divisa(m, &e.divisa))
                            .unwrap_or("?".into()),
                        estado,
                        if e.avisos.is_empty() {
                            String::new()
                        } else {
                            format!(" · avisos: {}", e.avisos.join("; "))
                        }
                    );
                    if movs {
                        for m in &e.movimientos {
                            println!(
                                "    {} {} {:>12} {:>12}  {}",
                                m.fecha,
                                m.fecha_valor.map(|d| d.to_string()).unwrap_or_default(),
                                m.importe.to_string(),
                                m.saldo.map(|s| s.to_string()).unwrap_or_default(),
                                m.concepto
                            );
                        }
                    }
                    if let (Some(d), Some(h)) = (e.desde, e.hasta) {
                        cobertura
                            .entry(format!("{} {} …{ultimos}", e.banco, e.divisa))
                            .or_default()
                            .push((d, h, nombre.clone()));
                    }
                    cuenta.extractos += 1;
                }
            }
            Err(err) => {
                let de_carpeta = desde_carpeta.iter().any(|c| f.starts_with(c));
                if todos || !de_carpeta {
                    println!("{}: ERROR {err}", f.display());
                }
                cuenta.errores += 1;
            }
        }
    }
    if importar {
        importar_todo(&ficheros);
    }
    println!(
        "\nExtractos: {} (cuadran {}, sin saldos {}, no cuadran {}). Ficheros sin leer: {}.",
        cuenta.extractos, cuenta.cuadran, cuenta.sin_saldos, cuenta.no_cuadran, cuenta.errores
    );
    for (cuenta, tramos) in &cobertura {
        let meses = meses_cubiertos(tramos);
        let (Some(primero), Some(ultimo)) = (meses.first(), meses.last()) else {
            continue;
        };
        let mut huecos = Vec::new();
        let mut m = *primero;
        while m <= *ultimo {
            if !meses.contains(&m) {
                huecos.push(format!("{}-{:02}", m.0, m.1));
            }
            m = if m.1 == 12 {
                (m.0 + 1, 1)
            } else {
                (m.0, m.1 + 1)
            };
        }
        println!(
            "{cuenta}: {}-{:02} a {}-{:02}, {} extractos; meses sin extracto: {}",
            primero.0,
            primero.1,
            ultimo.0,
            ultimo.1,
            tramos.len(),
            if huecos.is_empty() {
                "ninguno".to_owned()
            } else {
                huecos.join(", ")
            }
        );
    }
}

#[derive(Default)]
struct Totales {
    extractos: usize,
    cuadran: usize,
    sin_saldos: usize,
    no_cuadran: usize,
    errores: usize,
}

fn meses_cubiertos(tramos: &[(time::Date, time::Date, String)]) -> Vec<(i32, u8)> {
    let mut meses = BTreeSet::new();
    for (d, h, _) in tramos {
        let mut m = (d.year(), u8::from(d.month()));
        let fin = (h.year(), u8::from(h.month()));
        while m <= fin {
            meses.insert(m);
            m = if m.1 == 12 {
                (m.0 + 1, 1)
            } else {
                (m.0, m.1 + 1)
            };
        }
    }
    meses.into_iter().collect()
}

fn recorrer(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entradas: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entradas.sort();
    for p in entradas {
        if p.is_dir() {
            recorrer(&p, out);
        } else if p
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .is_some_and(|e| EXTENSIONES.contains(&e.as_str()))
        {
            out.push(p);
        }
    }
}

fn quitar(args: &mut Vec<String>, flag: &str) -> bool {
    let n = args.len();
    args.retain(|a| a != flag);
    args.len() != n
}

fn valor(args: &mut Vec<String>, flag: &str) -> Option<String> {
    let i = args.iter().position(|a| a == flag)?;
    let v = args.get(i + 1).cloned();
    args.drain(i..(i + 2).min(args.len()));
    v
}

/// Importa los ficheros en un almacén en memoria y repasa lo que ha quedado: cuentas,
/// movimientos y si el saldo de cada movimiento sigue al anterior (si no, falta o sobra algo).
fn importar_todo(ficheros: &[PathBuf]) {
    use std::sync::Arc;
    use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};

    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = time::OffsetDateTime::now_utc();
    println!("\n== Importación en un almacén en memoria");
    let (mut nuevos, mut ya) = (0, 0);
    for f in ficheros {
        let nombre = f.file_name().unwrap().to_string_lossy().into_owned();
        let Ok(bytes) = std::fs::read(f) else {
            continue;
        };
        match extractos::importar::importar_fichero(&v, &nombre, &bytes, None, false, now) {
            Ok(r) => {
                nuevos += r.movimientos_nuevos;
                ya += r.ya_estaban;
                println!("{nombre}: {}", r.frase());
            }
            Err(e) => println!("{nombre}: ERROR {e}"),
        }
    }
    println!("Movimientos nuevos: {nuevos}; repetidos que no se han vuelto a meter: {ya}.");
    let conn = v.connection();
    let mut cuentas = conn
        .prepare("SELECT id, name, number, currency, kind, account FROM bank_account ORDER BY id")
        .unwrap();
    let cuentas: Vec<(i64, String, String, String, String, String)> = cuentas
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap()
        .flatten()
        .collect();
    for (id, nombre, numero, divisa, clase, subcuenta) in cuentas {
        let mut q = conn
            .prepare(
                "SELECT op_date, amount, balance FROM bank_tx WHERE bank_account = ?1
                 ORDER BY op_date, id",
            )
            .unwrap();
        let filas: Vec<(String, i64, Option<i64>)> = q
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .flatten()
            .collect();
        // El saldo encadena si cada saldo es el anterior más el importe (dentro de cada día el
        // orden puede variar: se mira al final de cada día).
        let mut roturas = 0;
        let mut previo: Option<i64> = None;
        let mut i = 0;
        while i < filas.len() {
            let dia = &filas[i].0;
            let fin = filas[i..]
                .iter()
                .position(|f| &f.0 != dia)
                .map_or(filas.len(), |p| i + p);
            let suma: i64 = filas[i..fin].iter().map(|f| f.1).sum();
            let saldos: Vec<i64> = filas[i..fin].iter().filter_map(|f| f.2).collect();
            if let Some(p) = previo
                && !saldos.is_empty()
                && !saldos.contains(&(p + suma))
            {
                roturas += 1;
            }
            previo = match (previo, saldos.is_empty()) {
                (Some(p), _) if saldos.contains(&(p + suma)) => Some(p + suma),
                (_, false) => saldos.last().copied(),
                (Some(p), true) => Some(p + suma),
                (None, true) => None,
            };
            i = fin;
        }
        let ultimos: String = numero
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        println!(
            "{nombre} (…{ultimos} {divisa}, {clase}, {subcuenta}): {} movimientos, {} a {}, \
             saldo final {}; días en que el saldo no encadena: {roturas}",
            filas.len(),
            filas.first().map(|f| f.0.as_str()).unwrap_or(""),
            filas.last().map(|f| f.0.as_str()).unwrap_or(""),
            previo
                .map(|c| format!("{},{:02}", c / 100, (c % 100).abs()))
                .unwrap_or_default(),
        );
    }
}

/// Un importe con su divisa («1.234,56 USD»).
fn en_divisa(m: taxpain_core::money::Money, divisa: &str) -> String {
    format!("{} {divisa}", m.to_string().trim_end_matches(" €"))
}

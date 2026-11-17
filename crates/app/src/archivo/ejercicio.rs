//! «Archivo del ejercicio {año}» (G38): un ZIP con lo que la ley obliga a conservar (art. 30
//! CCom, seis años; arts. 66 y 70 LGT, prescripción) en formatos abiertos, con un índice y la
//! huella SHA-256 de cada fichero y del ZIP entero. Queda en la auditoría.
//!
//! Lo que lleva (lo que no hay en TaxPain se dice en el índice como «falta»):
//! - `contabilidad/`: diario, mayor y sumas y saldos en CSV y PDF, y el libro en XLSX (diario,
//!   mayor, sumas y saldos, balance y pérdidas y ganancias); los documentos de los asientos.
//! - `iva/`: libros registro de facturas emitidas y recibidas en CSV y PDF, y el XLSX de la AEAT.
//! - `facturas_emitidas/`: cada factura en PDF y, si se generó, en Facturae (y UBL).
//! - `facturas_recibidas/`: el documento original de cada una.
//! - `modelos/`: los ficheros presentados y sus justificantes (presentados por TaxPain,
//!   descargados de la AEAT o importados).
//! - `cuentas_anuales/`: la memoria (PDF) y el balance y la cuenta de pérdidas y ganancias.
//! - `libros_societarios/`: el libro de actas del ejercicio.
//! - `documentos/`: los documentos de la empresa con fecha de ese ejercicio (actas, escrituras,
//!   cuentas depositadas, requerimientos…).
//! - `bancos/`: los movimientos bancarios del ejercicio en CSV (TaxPain no guarda los extractos
//!   originales: el índice lo recuerda).
//!
//! El ZIP no va cifrado (la crate `zip` del proyecto no tiene AES): la interfaz lo avisa y pide
//! la confirmación de presencia antes de escribirlo.

use std::io::Write;
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};
use taxpain_core::Money;
use taxpain_invoicing::pdf::{Para, text_document};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::params;
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::{accounting, invoicing, received};

/// Una parte del expediente del ejercicio y si está.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Parte {
    pub clave: String,
    pub titulo: String,
    /// `si`, `parcial` o `no`.
    pub estado: String,
    pub detalle: String,
    pub ficheros: usize,
}

/// Lo que se ha escrito.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hecho {
    pub fichero: String,
    pub sha256: String,
    pub bytes: u64,
    pub ficheros: usize,
    pub partes: Vec<Parte>,
}

struct Fichero {
    ruta: String,
    descripcion: String,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct Paquete {
    ficheros: Vec<Fichero>,
    partes: Vec<Parte>,
}

impl Paquete {
    fn add(&mut self, ruta: String, descripcion: impl Into<String>, bytes: Vec<u8>) {
        // Sin rutas repetidas: «F-1.pdf», «F-1 (2).pdf»…
        let mut ruta = ruta;
        let mut n = 2;
        while self.ficheros.iter().any(|f| f.ruta == ruta) {
            ruta = match ruta.rsplit_once('.') {
                Some((a, b)) if n == 2 => format!("{a} ({n}).{b}"),
                Some((a, b)) => {
                    let base = a.rsplit_once(" (").map_or(a, |(x, _)| x);
                    format!("{base} ({n}).{b}")
                }
                None => format!("{ruta} ({n})"),
            };
            n += 1;
        }
        self.ficheros.push(Fichero {
            ruta,
            descripcion: descripcion.into(),
            bytes,
        });
    }

    fn parte(&mut self, clave: &str, titulo: &str, ficheros: usize, falta: Option<String>) {
        let (estado, detalle) = match (ficheros, falta) {
            (0, f) => ("no", f.unwrap_or_else(|| "no hay nada en TaxPain".into())),
            (_, Some(f)) => ("parcial", f),
            (n, None) => ("si", format!("{n} ficheros")),
        };
        self.partes.push(Parte {
            clave: clave.into(),
            titulo: titulo.into(),
            estado: estado.into(),
            detalle,
            ficheros,
        });
    }

    fn contar(&self, prefijo: &str) -> usize {
        self.ficheros
            .iter()
            .filter(|f| f.ruta.starts_with(prefijo))
            .count()
    }
}

/// Importe en CSV: punto decimal y sin separador de miles (`-1234.56`).
pub fn importe(m: Money) -> String {
    let c = m.cents();
    let sign = if c < 0 { "-" } else { "" };
    let a = c.unsigned_abs();
    format!("{sign}{}.{:02}", a / 100, a % 100)
}

fn csv(filas: &[Vec<String>]) -> Vec<u8> {
    let mut out = String::from("\u{feff}");
    for f in filas {
        out.push_str(
            &f.iter()
                .map(|c| taxpain_vault::volcado::celda_csv(c))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("\r\n");
    }
    out.into_bytes()
}

/// Nombre de fichero seguro (sin barras ni caracteres raros).
pub fn seguro(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let t = t.trim().trim_start_matches('.').to_owned();
    if t.is_empty() {
        "sin_nombre".into()
    } else {
        t.chars().take(80).collect()
    }
}

fn pdf(titulo: &str, paras: Vec<Para>) -> Result<Vec<u8>> {
    text_document(titulo, &[paras]).map_err(AppError::Other)
}

fn nombres(v: &Vault, year: i32) -> std::collections::HashMap<String, String> {
    accounting::chart(v, year)
        .map(|c| {
            c.into_iter()
                .map(|a| (a.code.to_string(), a.name))
                .collect()
        })
        .unwrap_or_default()
}

fn contabilidad(v: &Vault, p: &mut Paquete, year: i32, today: Date) -> Result<()> {
    let nombre = nombres(v, year);
    let diario = accounting::journal(v, year)?;
    let mut filas = vec![
        [
            "asiento",
            "fecha",
            "concepto",
            "cuenta",
            "nombre",
            "debe",
            "haber",
            "descripcion",
        ]
        .map(String::from)
        .to_vec(),
    ];
    let mut paras = vec![Para::Title(format!("Libro diario · ejercicio {year}"))];
    for e in &diario {
        paras.push(Para::Gap);
        paras.push(Para::Bold(format!(
            "Asiento {} · {} · {}",
            e.number, e.entry.date, e.entry.concept
        )));
        for l in &e.entry.lines {
            let code = l.account.to_string();
            let n = nombre.get(&code).cloned().unwrap_or_default();
            filas.push(vec![
                e.number.to_string(),
                e.entry.date.to_string(),
                e.entry.concept.clone(),
                code.clone(),
                n.clone(),
                importe(l.debit),
                importe(l.credit),
                l.description.clone().unwrap_or_default(),
            ]);
            let (lado, m) = if l.debit.cents() != 0 {
                ("Debe", l.debit)
            } else {
                ("Haber", l.credit)
            };
            paras.push(Para::Row(format!("{code} {n} · {lado}"), m.to_string()));
        }
        for d in accounting::entry_documents(v, e.id).unwrap_or_default() {
            if let Ok((name, _, bytes)) = accounting::entry_document(v, d.id) {
                p.add(
                    format!(
                        "contabilidad/documentos_asientos/{}-{}",
                        e.number,
                        seguro(&name)
                    ),
                    format!("Documento del asiento {}", e.number),
                    bytes,
                );
            }
        }
    }
    if diario.is_empty() {
        p.parte(
            "diario",
            "Libro diario",
            0,
            Some("no hay asientos de este ejercicio".into()),
        );
    } else {
        p.add(
            "contabilidad/diario.csv".into(),
            "Libro diario (CSV)",
            csv(&filas),
        );
        p.add(
            "contabilidad/diario.pdf".into(),
            "Libro diario (PDF)",
            pdf(&format!("Diario {year}"), paras)?,
        );
        // Mayor.
        let mayor = accounting::ledger_of(v, year, "")?;
        let mut filas = vec![
            [
                "cuenta", "nombre", "asiento", "fecha", "concepto", "debe", "haber", "saldo",
            ]
            .map(String::from)
            .to_vec(),
        ];
        let mut paras = vec![Para::Title(format!("Libro mayor · ejercicio {year}"))];
        let mut cuenta_actual = String::new();
        for l in &mayor {
            let code = l.account.to_string();
            let n = nombre.get(&code).cloned().unwrap_or_default();
            if code != cuenta_actual {
                paras.push(Para::Gap);
                paras.push(Para::Bold(format!("{code} {n}")));
                cuenta_actual = code.clone();
            }
            filas.push(vec![
                code,
                n,
                l.number.to_string(),
                l.date.to_string(),
                l.concept.clone(),
                importe(l.debit),
                importe(l.credit),
                importe(l.balance),
            ]);
            paras.push(Para::Row(
                format!("{} · {} · {}", l.date, l.number, l.concept),
                format!("{} / {}", importe(l.debit - l.credit), l.balance),
            ));
        }
        p.add(
            "contabilidad/mayor.csv".into(),
            "Libro mayor (CSV)",
            csv(&filas),
        );
        p.add(
            "contabilidad/mayor.pdf".into(),
            "Libro mayor (PDF)",
            pdf(&format!("Mayor {year}"), paras)?,
        );
        // Sumas y saldos.
        let sys = accounting::trial_balance(v, year, None)?;
        let mut filas = vec![
            ["cuenta", "nombre", "debe", "haber", "saldo"]
                .map(String::from)
                .to_vec(),
        ];
        let mut paras = vec![Para::Title(format!("Sumas y saldos · ejercicio {year}"))];
        for b in &sys {
            let code = b.account.to_string();
            let n = nombre.get(&code).cloned().unwrap_or_default();
            filas.push(vec![
                code.clone(),
                n.clone(),
                importe(b.debit),
                importe(b.credit),
                importe(b.balance()),
            ]);
            paras.push(Para::Row(
                format!("{code} {n}  (D {} · H {})", b.debit, b.credit),
                b.balance().to_string(),
            ));
        }
        p.add(
            "contabilidad/sumas_y_saldos.csv".into(),
            "Balance de sumas y saldos (CSV)",
            csv(&filas),
        );
        p.add(
            "contabilidad/sumas_y_saldos.pdf".into(),
            "Balance de sumas y saldos (PDF)",
            pdf(&format!("Sumas y saldos {year}"), paras)?,
        );
        let n = p.contar("contabilidad/");
        p.parte("diario", "Diario, mayor y sumas y saldos", n, None);
    }
    // Libro en XLSX y cuentas anuales.
    let modelo = crate::models::settings(v)
        .map(|s| s.accounts_model)
        .unwrap_or_default();
    match crate::listings::workbook(v, year, modelo, today) {
        Ok((name, bytes)) => p.add(
            format!("contabilidad/{}", seguro(&name)),
            "Libro de contabilidad (XLSX: diario, mayor, sumas y saldos, balance y PyG)",
            bytes,
        ),
        Err(e) => tracing::info!(error = %e, "sin libro XLSX en el archivo"),
    }
    let mut ca = 0;
    if let Ok(cuentas) = accounting::annual_accounts_model(v, year, modelo)
        && let Ok(json) = serde_json::to_vec_pretty(&cuentas)
    {
        p.add(
            "cuentas_anuales/balance_y_pyg.json".into(),
            "Balance y cuenta de pérdidas y ganancias (JSON)",
            json,
        );
        ca += 1;
    }
    match crate::memoria::pdf(v, year) {
        Ok(bytes) => {
            p.add("cuentas_anuales/memoria.pdf".into(), "Memoria (PDF)", bytes);
            ca += 1;
        }
        Err(e) => tracing::info!(error = %e, "sin memoria en el archivo"),
    }
    let depositadas = crate::documentos::list(v)?.iter().any(|d| {
        d.kind == crate::documentos::DocKind::CuentasAnuales
            && d.date
                .is_some_and(|f| f.year() == year + 1 || f.year() == year)
    });
    p.parte(
        "cuentas_anuales",
        "Cuentas anuales",
        ca,
        (!depositadas).then(|| {
            "no hay en Documentos las cuentas depositadas en el Registro Mercantil: añádelas".into()
        }),
    );
    Ok(())
}

fn iva(v: &Vault, p: &mut Paquete, year: i32) -> Result<()> {
    let emitidas = invoicing::list_issued(v, year)?;
    let mut filas = vec![
        [
            "numero",
            "fecha",
            "tipo",
            "destinatario",
            "base",
            "iva",
            "total",
            "estado",
        ]
        .map(String::from)
        .to_vec(),
    ];
    let mut paras = vec![Para::Title(format!("Facturas emitidas · {year}"))];
    for f in &emitidas {
        filas.push(vec![
            f.number.clone(),
            f.issue_date.clone(),
            f.kind.clone(),
            f.recipient_name.clone().unwrap_or_default(),
            importe(f.base),
            importe(f.vat),
            importe(f.total),
            f.status.clone(),
        ]);
        paras.push(Para::Row(
            format!(
                "{} · {} · {}",
                f.number,
                f.issue_date,
                f.recipient_name.clone().unwrap_or_default()
            ),
            f.total.to_string(),
        ));
        let base = format!("facturas_emitidas/{}", seguro(&f.number));
        if let Ok((_, _, pdf)) = invoicing::invoice_document(v, f.id, false) {
            p.add(
                format!("{base}.pdf"),
                format!("Factura {} (PDF)", f.number),
                pdf,
            );
        }
        if f.has_facturae
            && let Ok((_, _, xml)) = invoicing::invoice_document(v, f.id, true)
        {
            p.add(
                format!("{base}.facturae.xml"),
                format!("Factura {} (Facturae 3.2.2, sin firmar)", f.number),
                xml,
            );
        }
        if f.has_ubl
            && let Ok((_, _, xml)) = invoicing::invoice_ubl(v, f.id)
        {
            p.add(
                format!("{base}.ubl.xml"),
                format!("Factura {} (UBL)", f.number),
                xml,
            );
        }
    }
    let n = p.contar("facturas_emitidas/");
    let sin_pdf = emitidas.len().saturating_sub(
        p.ficheros
            .iter()
            .filter(|f| f.ruta.starts_with("facturas_emitidas/") && f.ruta.ends_with(".pdf"))
            .count(),
    );
    p.parte(
        "emitidas",
        "Facturas emitidas",
        n,
        (sin_pdf > 0).then(|| {
            format!(
                "{sin_pdf} facturas sin PDF en TaxPain (las importadas): guarda el original aparte"
            )
        }),
    );
    let recibidas: Vec<_> = received::list_received(v, None)?
        .into_iter()
        .filter(|r| r.status != "descartada")
        .filter(|r| {
            r.registered_on.map(|d| d.year()).or_else(|| {
                r.issue_date
                    .as_deref()
                    .and_then(|d| d.get(..4))
                    .and_then(|y| y.parse().ok())
            }) == Some(year)
        })
        .collect();
    let mut filas_r = vec![
        [
            "proveedor_nif",
            "proveedor",
            "numero",
            "fecha",
            "registro",
            "total",
            "estado",
            "documento",
        ]
        .map(String::from)
        .to_vec(),
    ];
    let mut paras_r = vec![Para::Title(format!("Facturas recibidas · {year}"))];
    let mut sin_doc = 0;
    for r in &recibidas {
        let nombre = format!(
            "{}_{}_{}",
            r.issue_date.clone().unwrap_or_default(),
            r.supplier_name.clone().unwrap_or_default(),
            r.number.clone().unwrap_or_default()
        );
        let doc = match received::received_document(v, r.id) {
            Ok((name, _, bytes)) => {
                let ext = name.rsplit_once('.').map_or("pdf", |(_, e)| e);
                let ruta = format!("facturas_recibidas/{}.{}", seguro(&nombre), seguro(ext));
                p.add(
                    ruta.clone(),
                    format!("Recibida {} (original)", r.filename),
                    bytes,
                );
                ruta
            }
            Err(_) => {
                sin_doc += 1;
                String::new()
            }
        };
        filas_r.push(vec![
            r.supplier_nif.clone().unwrap_or_default(),
            r.supplier_name.clone().unwrap_or_default(),
            r.number.clone().unwrap_or_default(),
            r.issue_date.clone().unwrap_or_default(),
            r.registered_on.map(|d| d.to_string()).unwrap_or_default(),
            r.total.map(importe).unwrap_or_default(),
            r.status.clone(),
            doc,
        ]);
        paras_r.push(Para::Row(
            format!(
                "{} · {} · {}",
                r.issue_date.clone().unwrap_or_default(),
                r.supplier_name.clone().unwrap_or_default(),
                r.number.clone().unwrap_or_default()
            ),
            r.total.map(|t| t.to_string()).unwrap_or_default(),
        ));
    }
    p.parte(
        "recibidas",
        "Facturas recibidas con su original",
        p.contar("facturas_recibidas/"),
        (sin_doc > 0).then(|| format!("{sin_doc} recibidas sin documento original en TaxPain")),
    );
    if !emitidas.is_empty() || !recibidas.is_empty() {
        p.add(
            "iva/libro_emitidas.csv".into(),
            "Libro registro de facturas expedidas (CSV)",
            csv(&filas),
        );
        p.add(
            "iva/libro_emitidas.pdf".into(),
            "Libro registro de facturas expedidas (PDF)",
            pdf(&format!("Expedidas {year}"), paras)?,
        );
        p.add(
            "iva/libro_recibidas.csv".into(),
            "Libro registro de facturas recibidas (CSV)",
            csv(&filas_r),
        );
        p.add(
            "iva/libro_recibidas.pdf".into(),
            "Libro registro de facturas recibidas (PDF)",
            pdf(&format!("Recibidas {year}"), paras_r)?,
        );
    }
    let xlsx = match crate::books::lsi_workbook(v, year) {
        Ok((name, bytes)) => {
            p.add(
                format!("iva/{}", seguro(&name)),
                "Libros registro de IVA (XLSX, formato de la AEAT)",
                bytes,
            );
            true
        }
        Err(_) => false,
    };
    p.parte(
        "iva",
        "Libros registro de IVA",
        p.contar("iva/"),
        (!xlsx).then(|| "sin el XLSX de la AEAT (faltan los datos de facturación)".into()),
    );
    Ok(())
}

fn modelos(v: &Vault, p: &mut Paquete, year: i32) -> Result<()> {
    let conn = v.connection();
    // Presentados con TaxPain: fichero y justificante.
    let mut stmt = conn.prepare(
        "SELECT id, model, period, file, csv FROM tax_return
         WHERE year = ?1 AND environment = 'produccion' ORDER BY model, period",
    )?;
    let propios: Vec<(i64, String, String, String, String)> = stmt
        .query_map([year], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mut sin_justificante = 0;
    for (id, model, period, file, csv_code) in propios {
        let base = format!("modelos/{model}_{year}_{}", seguro(&period));
        p.add(
            format!("{base}.txt"),
            format!("Modelo {model} {period} (fichero presentado, CSV {csv_code})"),
            file.into_bytes(),
        );
        match crate::models::receipt_pdf(v, id)? {
            Some(pdf) => p.add(
                format!("{base}_justificante.pdf"),
                format!("Justificante del {model} {period}"),
                pdf,
            ),
            None => sin_justificante += 1,
        }
    }
    // Consultados en la AEAT (justificantes) e importados.
    let mut stmt = conn.prepare(
        "SELECT f.model, f.period, d.filename, d.content FROM aeat_filing f
           JOIN document d ON d.id = f.document_id WHERE f.year = ?1
         UNION ALL
         SELECT f.model, f.period, d.filename, d.content FROM filed_model f
           JOIN document d ON d.id = f.document_id WHERE f.year = ?1",
    )?;
    let otros: Vec<(String, String, String, Vec<u8>)> = stmt
        .query_map([year], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (model, period, name, bytes) in otros {
        let ext = name.rsplit_once('.').map_or("pdf", |(_, e)| e);
        p.add(
            format!(
                "modelos/{model}_{year}_{}_aeat.{}",
                seguro(&period),
                seguro(ext)
            ),
            format!("Modelo {model} {period} (de la AEAT o importado)"),
            bytes,
        );
    }
    p.parte(
        "modelos",
        "Modelos presentados y justificantes",
        p.contar("modelos/"),
        (sin_justificante > 0)
            .then(|| format!("{sin_justificante} modelos sin justificante en PDF")),
    );
    Ok(())
}

fn societario(v: &Vault, p: &mut Paquete, year: i32) -> Result<()> {
    if let Ok(bytes) = crate::libros_societarios::book(
        v,
        crate::libros_societarios::BookKind::Actas,
        Some(year),
        None,
        true,
    ) && !bytes.is_empty()
    {
        p.add(
            format!("libros_societarios/libro_de_actas_{year}.pdf"),
            "Libro de actas del ejercicio",
            bytes,
        );
    }
    let fy = accounting::fiscal_year(v, year)?;
    for d in crate::documentos::list(v)? {
        let en = d.date.is_some_and(|f| fy.contains(f));
        if !en {
            continue;
        }
        if let Ok((name, _, bytes)) = crate::documentos::content(v, d.id) {
            p.add(
                format!(
                    "documentos/{}/{}",
                    seguro(d.kind.title()),
                    seguro(if name.is_empty() { &d.filename } else { &name })
                ),
                d.title.clone(),
                bytes,
            );
        }
    }
    let actas = p.contar("libros_societarios/")
        + p.ficheros
            .iter()
            .filter(|f| f.ruta.starts_with("documentos/") && f.ruta.to_lowercase().contains("acta"))
            .count();
    p.parte(
        "actas",
        "Actas y libros societarios",
        actas,
        (actas == 0).then(|| "no hay actas del ejercicio en TaxPain".into()),
    );
    p.parte(
        "documentos",
        "Documentos de la empresa del ejercicio",
        p.contar("documentos/"),
        None,
    );
    Ok(())
}

fn bancos(v: &Vault, p: &mut Paquete, year: i32) -> Result<()> {
    let fy = accounting::fiscal_year(v, year)?;
    let movs: Vec<_> = crate::bank::movements(v, false)?
        .into_iter()
        .filter(|m| {
            Date::parse(
                &m.op_date,
                time::macros::format_description!("[year]-[month]-[day]"),
            )
            .is_ok_and(|d| fy.contains(d))
        })
        .collect();
    let mut por_cuenta: std::collections::BTreeMap<String, Vec<Vec<String>>> = Default::default();
    for m in &movs {
        por_cuenta
            .entry(m.bank_account.clone())
            .or_default()
            .push(vec![
                m.op_date.clone(),
                m.description.clone(),
                importe(m.amount),
                m.currency.clone(),
                m.balance.map(importe).unwrap_or_default(),
                m.matched.clone().unwrap_or_default(),
            ]);
    }
    for (cuenta, filas) in por_cuenta {
        let mut todo = vec![
            [
                "fecha",
                "concepto",
                "importe",
                "moneda",
                "saldo",
                "casado_con",
            ]
            .map(String::from)
            .to_vec(),
        ];
        todo.extend(filas);
        p.add(
            format!("bancos/movimientos_{}.csv", seguro(&cuenta)),
            format!("Movimientos de la cuenta {cuenta}"),
            csv(&todo),
        );
    }
    p.parte(
        "bancos",
        "Extractos bancarios",
        p.contar("bancos/"),
        Some(
            "TaxPain guarda los movimientos, no los extractos originales del banco: conserva \
             también los PDF o ficheros que te da el banco"
                .into(),
        ),
    );
    Ok(())
}

fn reunir(v: &Vault, year: i32, today: Date) -> Result<Paquete> {
    let mut p = Paquete::default();
    contabilidad(v, &mut p, year, today)?;
    iva(v, &mut p, year)?;
    modelos(v, &mut p, year)?;
    societario(v, &mut p, year)?;
    bancos(v, &mut p, year)?;
    Ok(p)
}

/// Qué hay y qué falta del ejercicio, sin escribir nada.
pub fn contenido(v: &Vault, year: i32, today: Date) -> Result<Vec<Parte>> {
    Ok(reunir(v, year, today)?.partes)
}

fn indice(v: &Vault, year: i32, p: &Paquete, now: OffsetDateTime) -> Result<(Vec<u8>, Vec<u8>)> {
    let info = crate::companies::info(v)?;
    let fy = accounting::fiscal_year(v, year)?;
    let mut t = String::new();
    t.push_str(&format!(
        "ARCHIVO DEL EJERCICIO {year}\n{}{}\nEjercicio del {} al {}\nGenerado por TaxPain el {}\n\n",
        info.name,
        info.nif.map(|n| format!(" · NIF {n}")).unwrap_or_default(),
        fy.start,
        fy.end,
        now.format(&Rfc3339).unwrap_or_default()
    ));
    t.push_str(
        "Conservación: los libros y justificantes, seis años desde el último asiento (art. 30 \
         Código de Comercio); lo fiscal, mientras no prescriba (cuatro años, arts. 66 y 70 LGT), \
         y diez años o más si el ejercicio generó bases negativas o deducciones pendientes (art. \
         66 bis LGT, art. 26.5 LIS).\n\nQué hay y qué falta:\n",
    );
    for parte in &p.partes {
        let marca = match parte.estado.as_str() {
            "si" => "[x]",
            "parcial" => "[~]",
            _ => "[ ]",
        };
        t.push_str(&format!("  {marca} {}: {}\n", parte.titulo, parte.detalle));
    }
    t.push_str("\nFicheros (SHA-256):\n");
    let mut filas = vec![
        ["fichero", "descripcion", "bytes", "sha256"]
            .map(String::from)
            .to_vec(),
    ];
    for f in &p.ficheros {
        let h = hex::encode(Sha256::digest(&f.bytes));
        t.push_str(&format!("  {h}  {}  ({})\n", f.ruta, f.descripcion));
        filas.push(vec![
            f.ruta.clone(),
            f.descripcion.clone(),
            f.bytes.len().to_string(),
            h,
        ]);
    }
    Ok((t.into_bytes(), csv(&filas)))
}

/// Escribe el ZIP del ejercicio en `dest`, su huella en `dest.sha256` y lo anota en la
/// auditoría y en `archivo_ejercicio`. `confirmed`: el usuario lo ha confirmado (el ZIP no va
/// cifrado).
pub fn exportar(
    v: &Vault,
    year: i32,
    dest: &Path,
    today: Date,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<Hecho> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let p = reunir(v, year, today)?;
    let (txt, idx) = indice(v, year, &p, now)?;
    let raiz = format!("Archivo {year}");
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    let mut add = |name: &str, bytes: &[u8]| -> Result<()> {
        zip.start_file(format!("{raiz}/{name}"), opts)
            .map_err(|e| AppError::Other(format!("ZIP: {e}")))?;
        zip.write_all(bytes)
            .map_err(|e| AppError::Other(format!("ZIP: {e}")))?;
        Ok(())
    };
    add("00_INDICE.txt", &txt)?;
    add("indice.csv", &idx)?;
    for f in &p.ficheros {
        add(&f.ruta, &f.bytes)?;
    }
    let bytes = zip
        .finish()
        .map_err(|e| AppError::Other(format!("ZIP: {e}")))?
        .into_inner();
    let sha = hex::encode(Sha256::digest(&bytes));
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("Archivo {year}.zip"));
    std::fs::write(dest, &bytes).map_err(taxpain_vault::VaultError::Io)?;
    let mut huella = dest.as_os_str().to_owned();
    huella.push(".sha256");
    std::fs::write(&huella, format!("{sha}  {name}\n")).map_err(taxpain_vault::VaultError::Io)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let at = now.format(&Rfc3339).unwrap_or_default();
    let n = p.ficheros.len() + 2;
    tx.execute(
        "INSERT INTO archivo_ejercicio(ejercicio, fichero, sha256, bytes, ficheros, creado_en)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            year,
            name,
            sha,
            i64::try_from(bytes.len()).unwrap_or(i64::MAX),
            i64::try_from(n).unwrap_or(0),
            at
        ],
    )?;
    audit::append(
        &tx,
        &at,
        Actor::User,
        "archivo.ejercicio",
        &name,
        &serde_json::json!({ "ejercicio": year, "sha256": sha, "bytes": bytes.len(), "ficheros": n }),
    )?;
    tx.commit()?;
    Ok(Hecho {
        fichero: name,
        sha256: sha,
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        ficheros: n,
        partes: p.partes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
    use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider};
    use time::macros::{date, datetime};

    fn vault() -> Vault {
        Vault::open_in_memory(
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
        )
        .unwrap()
    }

    #[test]
    fn el_importe_va_con_punto_y_sin_miles() {
        assert_eq!(importe(Money::from_cents(123456)), "1234.56");
        assert_eq!(importe(Money::from_cents(-5)), "-0.05");
    }

    #[test]
    fn el_nombre_de_fichero_no_sale_de_su_carpeta() {
        assert_eq!(seguro("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(seguro("F-2026/001"), "F-2026_001");
        assert_eq!(seguro(""), "sin_nombre");
    }

    #[test]
    fn el_archivo_lleva_diario_indice_y_huella_y_queda_en_la_auditoria() {
        let v = vault();
        let now = datetime!(2026-11-17 10:00 UTC);
        let e = JournalEntry {
            date: date!(2025 - 03 - 10),
            concept: "Aportación de los socios".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(AccountCode::new("572").unwrap(), Money::from_cents(300000)),
                EntryLine::credit(AccountCode::new("100").unwrap(), Money::from_cents(300000)),
            ],
        };
        accounting::manual_entry(&v, &e, now).unwrap();
        let partes = contenido(&v, 2025, date!(2026 - 11 - 17)).unwrap();
        let diario = partes.iter().find(|p| p.clave == "diario").unwrap();
        assert_eq!(diario.estado, "si");
        assert_eq!(
            partes.iter().find(|p| p.clave == "bancos").unwrap().estado,
            "no"
        );

        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("Archivo 2025.zip");
        assert!(matches!(
            exportar(&v, 2025, &dest, date!(2026 - 11 - 17), now, false),
            Err(AppError::NotConfirmed)
        ));
        let h = exportar(&v, 2025, &dest, date!(2026 - 11 - 17), now, true).unwrap();
        let bytes = std::fs::read(&dest).unwrap();
        assert_eq!(h.sha256, hex::encode(Sha256::digest(&bytes)));
        let huella = std::fs::read_to_string(tmp.path().join("Archivo 2025.zip.sha256")).unwrap();
        assert!(huella.starts_with(&h.sha256));
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let nombres: Vec<String> = z.file_names().map(|n| n.unwrap().into_owned()).collect();
        assert!(nombres.contains(&"Archivo 2025/00_INDICE.txt".to_owned()));
        assert!(nombres.contains(&"Archivo 2025/contabilidad/diario.csv".to_owned()));
        assert!(nombres.contains(&"Archivo 2025/contabilidad/diario.pdf".to_owned()));
        let mut s = String::new();
        std::io::Read::read_to_string(
            &mut z.by_name("Archivo 2025/contabilidad/diario.csv").unwrap(),
            &mut s,
        )
        .unwrap();
        assert!(s.contains("Aportación de los socios"));
        assert!(s.contains("3000.00"));
        let log = taxpain_vault::audit::entries(v.connection()).unwrap();
        let a = log
            .iter()
            .find(|e| e.action == "archivo.ejercicio")
            .unwrap();
        assert!(a.details.contains(&h.sha256));
        let c = super::super::conservacion::conservacion(&v, date!(2026 - 11 - 17)).unwrap();
        assert_eq!(c[0].archivado.as_ref().unwrap().sha256, h.sha256);
    }
}

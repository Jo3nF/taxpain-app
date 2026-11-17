//! Libros societarios de una sociedad de capital (G08 y G09):
//!
//! - [`registro`]: libro registro de socios (arts. 104-106 LSC). Movimientos de las
//!   participaciones (constitución, ampliación, reducción, transmisión, constitución y
//!   cancelación de cargas) y la situación actual que sale de ellos, cotejada con la ficha de
//!   socios (`shareholder`), a la que puede pasar los porcentajes.
//! - [`contratos`]: libro registro de contratos del socio único con la sociedad (art. 16 LSC),
//!   con propuestas de lo que TaxPain ya sabe (nóminas del socio único, dividendos).
//! - [`actas`]: libro de actas (arts. 26 CCom y 202 LSC; arts. 97-112 RRM) con plantillas
//!   (aprobación de cuentas, dividendos, dividendo a cuenta con el estado del art. 277 LSC,
//!   nombramiento y cese de administrador, retribución del art. 217 LSC, cambio de domicilio) y
//!   la certificación de los acuerdos.
//!
//! Los tres se exportan a PDF (con folios) o RTF y van, si se pide, en el ZIP de legalización
//! (`legalizacion::build`: `ACTASCON_nnn`, `SOCIOS_nnn` y `SOCUNICO_nnn`, Anexo I de la
//! Instrucción DGRN de 12/2/2015). Son documentos privados: las escrituras y las inscripciones
//! siguen pasando por la notaría y el Registro.

pub mod actas;
pub mod contratos;
pub mod registro;

use taxpain_core::Nif;
use taxpain_invoicing::pdf::{self, Para};
use taxpain_vault::Vault;
use taxpain_vault::rusqlite::Connection;
use time::Date;

use crate::error::{AppError, Result};

pub(crate) fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

pub(crate) fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).map_err(other)
}

/// «05/03/2026».
pub(crate) fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

const MONTHS: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

/// «5 de marzo de 2026».
pub(crate) fn long_date(d: Date) -> String {
    format!(
        "{} de {} de {}",
        d.day(),
        MONTHS[usize::from(u8::from(d.month()) - 1)],
        d.year()
    )
}

/// Nombre, NIF y domicilio de la sociedad. Error si es una persona física (estos libros son de
/// las sociedades).
pub(crate) struct Company {
    pub name: String,
    pub nif: String,
    pub address: String,
}

pub(crate) fn company(v: &Vault) -> Result<Company> {
    let info = crate::companies::info(v)?;
    let nif = info.nif.clone().unwrap_or_default();
    if Nif::parse(&nif).is_ok_and(|n| n.is_persona_fisica()) {
        return Err(other(
            "los libros de actas, de socios y de contratos con el socio único son de las sociedades",
        ));
    }
    let address = crate::invoicing::settings(v)?
        .and_then(|s| s.issuer.address)
        .map(|a| {
            [
                a.street,
                format!("{} {}", a.post_code, a.town).trim().to_owned(),
                a.province,
            ]
            .into_iter()
            .filter(|p| !p.trim().is_empty())
            .collect::<Vec<_>>()
            .join(", ")
        })
        .unwrap_or_default();
    Ok(Company {
        name: info.name,
        nif: if nif.is_empty() { "[NIF]".into() } else { nif },
        address,
    })
}

/// Saldo acreedor (haber menos debe) de las cuentas que empiezan por `prefix` a `date`: todos
/// los asientos hasta ese día sin los de cierre y sólo la apertura del primer ejercicio (que trae
/// los saldos anteriores), como `socios::pending_dividends`.
pub(crate) fn credit_balance_at(conn: &Connection, prefix: &str, date: Date) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.date <= ?1 AND e.origin_kind <> 'Cierre'
            AND (e.origin_kind <> 'Apertura'
                 OR e.fiscal_year = (SELECT MIN(fiscal_year) FROM journal_entry))
            AND l.account LIKE ?2 || '%'",
        (date.to_string(), prefix),
        |r| r.get(0),
    )?)
}

/// Texto con saltos de línea en párrafos del PDF (una línea vacía, una separación).
pub(crate) fn text_paras(out: &mut Vec<Para>, text: &str) {
    for line in text.lines() {
        if line.trim().is_empty() {
            out.push(Para::Gap);
        } else {
            out.push(Para::Text(line.to_owned()));
        }
    }
}

/// PDF (con folios en el pie si se da `footer`) o RTF de unos párrafos.
pub(crate) fn render(
    title: &str,
    footer: Option<&str>,
    docs: &[Vec<Para>],
    as_pdf: bool,
) -> Result<Vec<u8>> {
    if as_pdf {
        match footer {
            Some(f) => pdf::text_document_foliated(title, f, docs),
            None => pdf::text_document(title, docs),
        }
        .map_err(AppError::Other)
    } else {
        let all: Vec<Para> = docs
            .iter()
            .enumerate()
            .flat_map(|(i, d)| {
                let mut v = if i > 0 {
                    vec![Para::Gap, Para::Gap]
                } else {
                    vec![]
                };
                v.extend(d.iter().cloned());
                v
            })
            .collect();
        Ok(crate::memoria::to_rtf(&all).into_bytes())
    }
}

/// Cuál de los tres libros.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BookKind {
    Actas,
    Socios,
    Contratos,
}

impl BookKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Actas => "Libro de actas",
            Self::Socios => "Libro registro de socios",
            Self::Contratos => "Libro registro de contratos del socio único",
        }
    }

    /// Prefijo del fichero en el ZIP de Legalia (Anexo I de la Instrucción DGRN de 12/2/2015).
    pub fn legalia_prefix(self) -> &'static str {
        match self {
            Self::Actas => "ACTASCON",
            Self::Socios => "SOCIOS",
            Self::Contratos => "SOCUNICO",
        }
    }
}

/// El libro del ejercicio `year` (o, en el de socios y el de contratos, entero si es `None`) en
/// PDF con folios o en RTF. `number`: número del libro, si se va a legalizar.
pub fn book(
    v: &Vault,
    kind: BookKind,
    year: Option<i32>,
    number: Option<u16>,
    as_pdf: bool,
) -> Result<Vec<u8>> {
    let c = company(v)?;
    let docs = match kind {
        BookKind::Actas => {
            let y = year.ok_or_else(|| other("el libro de actas es de un ejercicio"))?;
            actas::book_docs(v, y, number)?
        }
        BookKind::Socios => vec![registro::book_paras(v, year, number)?],
        BookKind::Contratos => vec![contratos::book_paras(v, year, number)?],
    };
    let mut footer = format!("{} · NIF {} · {}", c.name, c.nif, kind.label());
    if let Some(n) = number {
        footer.push_str(&format!(" nº {n}"));
    }
    if let Some(y) = year {
        footer.push_str(&format!(" · Ejercicio {y}"));
    }
    render(kind.label(), Some(&footer), &docs, as_pdf)
}

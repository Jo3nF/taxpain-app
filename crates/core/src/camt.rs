//! Extractos bancarios ISO 20022 en XML: `camt.053` (extracto de cuenta), `camt.052` (informe
//! intradía) y `camt.054` (avisos de abono y cargo), versiones `.001.02` a `.001.08`. Muchos
//! bancos los dan ya en vez de la Norma 43.
//!
//! Se convierten al mismo modelo que la Norma 43 ([`N43Account`]), para conciliar igual:
//! - la cuenta se identifica por el IBAN (en los españoles, entidad + oficina + número, como en
//!   la Norma 43, así un mismo banco importado por las dos vías es la misma cuenta);
//! - sólo los apuntes contabilizados (`BOOK`); abonos (`CRDT`) en positivo y cargos (`DBIT`) en
//!   negativo. En una retrocesión (`RvslInd`) `CdtDbtInd` ya da la dirección del propio apunte
//!   (un adeudo devuelto viene como `CRDT`), así que `RvslInd` es sólo informativo;
//! - descripción: concepto libre (`Ustrd`), ordenante o beneficiario e información adicional.
//!
//! Lector propio (quick-xml), escrito tomando como referencia la estructura de los esquemas
//! ISO 20022; no copia código de terceros.

use quick_xml::Reader;
use quick_xml::events::Event;
use time::{Date, Month};

use crate::bank::{Movement, N43Account};
use crate::money::Money;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CamtError {
    #[error("XML no válido: {0}")]
    Xml(String),
    #[error("no es un extracto camt.052, camt.053 ni camt.054")]
    NotCamt,
    #[error("{0}")]
    Bad(String),
}

/// Nodo de un árbol XML sencillo (sólo nombres locales y texto).
#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    /// Nodo por ruta (`"Acct/Id/IBAN"`).
    fn at(&self, path: &str) -> Option<&Node> {
        path.split('/').try_fold(self, |n, p| n.child(p))
    }
    fn text_at(&self, path: &str) -> Option<&str> {
        self.at(path)
            .map(|n| n.text.trim())
            .filter(|t| !t.is_empty())
    }
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

fn parse_xml(text: &str) -> Result<Node, CamtError> {
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Node> = vec![Node::default()];
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let attrs = e
                    .attributes()
                    .flatten()
                    .map(|a| (a.key.local_name().as_ref().to_owned(), a.value.into_owned()))
                    .collect();
                stack.push(Node {
                    name: e.local_name().as_ref().to_owned(),
                    attrs,
                    ..Node::default()
                });
            }
            Ok(Event::Empty(e)) => {
                let n = Node {
                    name: e.local_name().as_ref().to_owned(),
                    ..Node::default()
                };
                if let Some(top) = stack.last_mut() {
                    top.children.push(n);
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&t.xml10_content());
                }
            }
            Ok(Event::CData(t)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(t.as_ref());
                }
            }
            Ok(Event::GeneralRef(g)) => {
                if let Some(top) = stack.last_mut() {
                    if let Ok(Some(c)) = g.resolve_char_ref() {
                        top.text.push(c);
                    } else {
                        top.text.push_str(match g.as_ref() {
                            "amp" => "&",
                            "lt" => "<",
                            "gt" => ">",
                            "quot" => "\"",
                            "apos" => "'",
                            _ => "",
                        });
                    }
                }
            }
            Ok(Event::End(_)) => {
                let done = stack
                    .pop()
                    .ok_or_else(|| CamtError::Xml("cierre de más".into()))?;
                stack
                    .last_mut()
                    .ok_or_else(|| CamtError::Xml("cierre de más".into()))?
                    .children
                    .push(done);
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(CamtError::Xml(e.to_string())),
            _ => {}
        }
    }
    stack
        .pop()
        .filter(|_| stack.is_empty())
        .ok_or_else(|| CamtError::Xml("elementos sin cerrar".into()))
}

/// `aaaa-mm-dd` (o fecha y hora ISO 8601, de la que se toma la fecha).
fn date(s: &str) -> Option<Date> {
    let s = s.trim();
    let y: i32 = s.get(0..4)?.parse().ok()?;
    let m: u8 = s.get(5..7)?.parse().ok()?;
    let d: u8 = s.get(8..10)?.parse().ok()?;
    Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()
}

/// Fecha de un `Dt`/`DtTm` hijo.
fn date_of(n: Option<&Node>) -> Option<Date> {
    let n = n?;
    n.text_at("Dt").or_else(|| n.text_at("DtTm")).and_then(date)
}

/// Importe decimal (`1234.5`) a céntimos.
fn amount(s: &str) -> Option<Money> {
    let s = s.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let (i, d) = s.split_once('.').unwrap_or((s, ""));
    if d.len() > 2 || i.is_empty() || !i.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let c = i
        .parse::<i64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(format!("{d:0<2}").parse::<i64>().ok()?)?;
    Some(Money::from_cents(if neg { -c } else { c }))
}

fn signed(node: &Node, amt_path: &str) -> Option<Money> {
    let a = amount(node.text_at(amt_path)?)?;
    Some(if node.text_at("CdtDbtInd") == Some("DBIT") {
        -a
    } else {
        a
    })
}

/// Lee un extracto camt. Devuelve una cuenta por extracto (`Stmt`, `Rpt` o `Ntfctn`).
pub fn parse_camt(text: &str) -> Result<Vec<N43Account>, CamtError> {
    let root = parse_xml(text.trim_start_matches('\u{feff}'))?;
    let doc = root.child("Document").ok_or(CamtError::NotCamt)?;
    let (msg, item) = [
        ("BkToCstmrStmt", "Stmt"),
        ("BkToCstmrAcctRpt", "Rpt"),
        ("BkToCstmrDbtCdtNtfctn", "Ntfctn"),
    ]
    .into_iter()
    .find_map(|(m, i)| doc.child(m).map(|n| (n, i)))
    .ok_or(CamtError::NotCamt)?;
    let mut out = Vec::new();
    for st in msg.all(item) {
        let iban = st
            .text_at("Acct/Id/IBAN")
            .or_else(|| st.text_at("Acct/Id/Othr/Id"))
            .ok_or_else(|| CamtError::Bad("extracto sin cuenta (IBAN)".into()))?
            .replace(' ', "");
        let (bank, office, account) = if iban.starts_with("ES") && iban.len() == 24 {
            (
                iban[4..8].to_owned(),
                iban[8..12].to_owned(),
                iban[14..24].to_owned(),
            )
        } else {
            (String::new(), String::new(), iban.clone())
        };
        let balance = |codes: &[&str]| -> Option<(Money, Option<Date>)> {
            st.all("Bal").find_map(|b| {
                let code = b
                    .text_at("Tp/CdOrPrtry/Cd")
                    .or_else(|| b.text_at("Tp/CdOrPrtry/Prtry"))?;
                codes
                    .contains(&code)
                    .then(|| signed(b, "Amt").map(|m| (m, date_of(b.child("Dt")))))
                    .flatten()
            })
        };
        let opening = balance(&["OPBD", "PRCD", "ITBD", "OPAV"]);
        let closing = balance(&["CLBD", "CLAV", "ITBD"]);
        let mut movements = Vec::new();
        for (i, e) in st.all("Ntry").enumerate() {
            let status = e
                .text_at("Sts/Cd")
                .or_else(|| e.text_at("Sts"))
                .unwrap_or("BOOK");
            if status != "BOOK" {
                continue;
            }
            // `RvslInd` no cambia el signo: en ISO 20022 `CdtDbtInd` ya es la dirección del
            // apunte de retrocesión (un adeudo devuelto llega como `CRDT`).
            let amt = signed(e, "Amt")
                .ok_or_else(|| CamtError::Bad(format!("apunte {}: importe no válido", i + 1)))?;
            let op = date_of(e.child("BookgDt"))
                .or_else(|| date_of(e.child("ValDt")))
                .ok_or_else(|| CamtError::Bad(format!("apunte {}: sin fecha", i + 1)))?;
            let value = date_of(e.child("ValDt")).unwrap_or(op);
            let mut details: Vec<String> = Vec::new();
            let mut end_to_end = String::new();
            for tx in e.all("NtryDtls").flat_map(|d| d.all("TxDtls")) {
                for u in tx.at("RmtInf").into_iter().flat_map(|r| r.all("Ustrd")) {
                    if !u.text.trim().is_empty() {
                        details.push(u.text.trim().to_owned());
                    }
                }
                let party = if amt.is_negative() {
                    tx.text_at("RltdPties/Cdtr/Nm")
                        .or_else(|| tx.text_at("RltdPties/Cdtr/Pty/Nm"))
                } else {
                    tx.text_at("RltdPties/Dbtr/Nm")
                        .or_else(|| tx.text_at("RltdPties/Dbtr/Pty/Nm"))
                };
                if let Some(p) = party {
                    details.push(p.to_owned());
                }
                if end_to_end.is_empty()
                    && let Some(r) = tx.text_at("Refs/EndToEndId")
                    && r != "NOTPROVIDED"
                {
                    end_to_end = r.to_owned();
                }
            }
            if let Some(a) = e.text_at("AddtlNtryInf") {
                details.push(a.to_owned());
            }
            details.dedup();
            movements.push(Movement {
                op_date: op,
                value_date: value,
                common_concept: String::new(),
                own_concept: e
                    .text_at("BkTxCd/Prtry/Cd")
                    .or_else(|| e.text_at("BkTxCd/Domn/Fmly/SubFmlyCd"))
                    .unwrap_or("")
                    .to_owned(),
                amount: amt,
                document: e
                    .text_at("AcctSvcrRef")
                    .or_else(|| e.text_at("NtryRef"))
                    .unwrap_or("")
                    .to_owned(),
                reference1: end_to_end,
                reference2: String::new(),
                details,
            });
        }
        let dates: Vec<Date> = movements.iter().map(|m| m.op_date).collect();
        let start = st
            .text_at("FrToDt/FrDtTm")
            .and_then(date)
            .or_else(|| st.text_at("FrToDt/FrDt").and_then(date))
            .or_else(|| opening.and_then(|o| o.1))
            .or_else(|| dates.iter().min().copied());
        let end = st
            .text_at("FrToDt/ToDtTm")
            .and_then(date)
            .or_else(|| st.text_at("FrToDt/ToDt").and_then(date))
            .or_else(|| closing.and_then(|c| c.1))
            .or_else(|| dates.iter().max().copied());
        let today = start
            .or(end)
            .ok_or_else(|| CamtError::Bad("extracto sin fechas".into()))?;
        let currency = st
            .text_at("Acct/Ccy")
            .map(str::to_owned)
            .or_else(|| {
                st.all("Bal").find_map(|b| {
                    b.child("Amt")
                        .and_then(|a| a.attr("Ccy"))
                        .map(str::to_owned)
                })
            })
            .unwrap_or_else(|| "EUR".into());
        let initial = opening.map_or(Money::ZERO, |o| o.0);
        let total = movements.iter().fold(Money::ZERO, |a, m| a + m.amount);
        out.push(N43Account {
            bank,
            office,
            account,
            start: start.unwrap_or(today),
            end: end.unwrap_or(today),
            initial_balance: initial,
            final_balance: closing.map_or(initial + total, |c| c.0),
            currency,
            holder: st
                .text_at("Acct/Ownr/Nm")
                .or_else(|| st.text_at("Acct/Ownr/Pty/Nm"))
                .unwrap_or("")
                .to_owned(),
            movements,
        });
    }
    if out.is_empty() {
        return Err(CamtError::NotCamt);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use time::macros::date;

    use super::*;

    const CAMT053: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.02">
 <BkToCstmrStmt>
  <GrpHdr><MsgId>EXT-1</MsgId><CreDtTm>2026-10-02T08:00:00</CreDtTm></GrpHdr>
  <Stmt>
   <Id>1</Id>
   <FrToDt><FrDtTm>2026-10-01T00:00:00</FrDtTm><ToDtTm>2026-10-01T23:59:59</ToDtTm></FrToDt>
   <Acct><Id><IBAN>ES9121000418450200051332</IBAN></Id><Ccy>EUR</Ccy><Ownr><Nm>EJEMPLO SL</Nm></Ownr></Acct>
   <Bal><Tp><CdOrPrtry><Cd>OPBD</Cd></CdOrPrtry></Tp><Amt Ccy="EUR">1000.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-10-01</Dt></Dt></Bal>
   <Bal><Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp><Amt Ccy="EUR">2131.40</Amt><CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-10-01</Dt></Dt></Bal>
   <Ntry>
    <Amt Ccy="EUR">1210.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts>BOOK</Sts>
    <BookgDt><Dt>2026-10-01</Dt></BookgDt><ValDt><Dt>2026-10-01</Dt></ValDt>
    <AcctSvcrRef>REF-0001</AcctSvcrRef>
    <NtryDtls><TxDtls>
     <Refs><EndToEndId>A2026-0001/1</EndToEndId></Refs>
     <RltdPties><Dbtr><Nm>Cliente &amp; Hijos SA</Nm></Dbtr></RltdPties>
     <RmtInf><Ustrd>FRA A2026-0001</Ustrd></RmtInf>
    </TxDtls></NtryDtls>
   </Ntry>
   <Ntry>
    <Amt Ccy="EUR">78.60</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts><Cd>BOOK</Cd></Sts>
    <BookgDt><DtTm>2026-10-01T10:00:00</DtTm></BookgDt>
    <AddtlNtryInf>RECIBO TELEFONIA</AddtlNtryInf>
   </Ntry>
   <Ntry>
    <Amt Ccy="EUR">5.00</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts>PDNG</Sts>
    <BookgDt><Dt>2026-10-01</Dt></BookgDt>
   </Ntry>
  </Stmt>
 </BkToCstmrStmt>
</Document>"#;

    #[test]
    fn extracto_camt053() {
        let a = parse_camt(CAMT053).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(a.len(), 1);
        let a = &a[0];
        assert_eq!(a.number(), "210004180200051332");
        assert_eq!(a.holder, "EJEMPLO SL");
        assert_eq!(
            (a.start, a.end),
            (date!(2026 - 10 - 01), date!(2026 - 10 - 01))
        );
        assert_eq!(a.initial_balance, Money::from_cents(100_000));
        assert_eq!(a.final_balance, Money::from_cents(213_140));
        assert_eq!(a.movements.len(), 2, "el pendiente no entra");
        let m = &a.movements[0];
        assert_eq!(m.amount, Money::from_cents(121_000));
        assert_eq!(m.document, "REF-0001");
        assert_eq!(m.reference1, "A2026-0001/1");
        assert_eq!(m.description(), "FRA A2026-0001 Cliente & Hijos SA");
        let m = &a.movements[1];
        assert_eq!(m.amount, Money::from_cents(-7860));
        assert_eq!(m.op_date, date!(2026 - 10 - 01));
        assert_eq!(m.description(), "RECIBO TELEFONIA");
    }

    #[test]
    fn otros_mensajes_y_errores() {
        let ntf = CAMT053
            .replace("camt.053.001.02", "camt.054.001.08")
            .replace("BkToCstmrStmt", "BkToCstmrDbtCdtNtfctn")
            .replace("<Stmt>", "<Ntfctn>")
            .replace("</Stmt>", "</Ntfctn>");
        assert_eq!(
            parse_camt(&ntf).unwrap_or_else(|e| panic!("{e}"))[0]
                .movements
                .len(),
            2
        );
        assert_eq!(
            parse_camt("<Document><Otro/></Document>"),
            Err(CamtError::NotCamt)
        );
        assert!(matches!(
            parse_camt("<Document><a>"),
            Err(CamtError::Xml(_))
        ));
        let bad = CAMT053.replace("1210.00", "12,10");
        assert!(matches!(parse_camt(&bad), Err(CamtError::Bad(_))));
    }

    #[test]
    fn retrocesion_respeta_cdtdbtind() {
        // Adeudo de 100 € devuelto: el banco lo anota como abono (`CRDT`) con `RvslInd`.
        let xml = CAMT053
            .replace(">2131.40<", ">2231.40<")
            .replace(
                "<Amt Ccy=\"EUR\">5.00</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts>PDNG</Sts>",
                "<Amt Ccy=\"EUR\">100.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><RvslInd>true</RvslInd><Sts>BOOK</Sts>",
            );
        let a = &parse_camt(&xml).unwrap_or_else(|e| panic!("{e}"))[0];
        assert_eq!(a.movements.len(), 3);
        assert_eq!(a.movements[2].amount, Money::from_cents(10_000));
        let suma = a
            .movements
            .iter()
            .fold(a.initial_balance, |s, m| s + m.amount);
        assert_eq!(suma, a.final_balance, "el saldo cuadra");
    }
}

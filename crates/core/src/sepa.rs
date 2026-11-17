//! SEPA: IBAN, identificador de acreedor y ficheros XML ISO 20022 para el banco.
//!
//! - Adeudos directos (recibos domiciliados): `pain.008.001.02`, Cuaderno AEB 19.14 (CORE) y
//!   19.44 (B2B).
//! - Transferencias: `pain.001.001.03`, Cuaderno AEB 34.14.
//!
//! Sin BIC (desde 2016 no hace falta en pagos SEPA): se usa `NOTPROVIDED`, como admiten los
//! cuadernos. Los textos se pasan al juego de caracteres latino básico de SEPA.
//! `TODO(VERIFY)`: validar los ficheros con el validador de un banco antes de usarlos de verdad.

use std::fmt::Write as _;

use time::{Date, PrimitiveDateTime};

use crate::money::Money;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SepaError {
    #[error("IBAN no válido: {0}")]
    Iban(String),
    #[error("{0}")]
    Invalid(String),
}

fn mod97(digits: &str) -> u32 {
    digits.chars().fold(0u32, |acc, c| {
        let d = c.to_digit(36).unwrap_or(0);
        if d >= 10 {
            (acc * 100 + d) % 97
        } else {
            (acc * 10 + d) % 97
        }
    })
}

/// IBAN sin espacios y en mayúsculas, si su dígito de control (ISO 13616, módulo 97) cuadra.
/// Los españoles han de tener 24 caracteres.
pub fn normalize_iban(s: &str) -> Result<String, SepaError> {
    let iban: String = s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .to_uppercase();
    let bad = || SepaError::Iban(s.trim().to_owned());
    if iban.len() < 15
        || iban.len() > 34
        || !iban.chars().all(|c| c.is_ascii_alphanumeric())
        || !iban[..2].chars().all(|c| c.is_ascii_alphabetic())
        || !iban[2..4].chars().all(|c| c.is_ascii_digit())
    {
        return Err(bad());
    }
    if iban.starts_with("ES") && iban.len() != 24 {
        return Err(bad());
    }
    let rotated = format!("{}{}", &iban[4..], &iban[..4]);
    if mod97(&rotated) != 1 {
        return Err(bad());
    }
    Ok(iban)
}

/// IBAN español a partir de la cuenta de 20 dígitos (entidad, oficina, control y número).
pub fn iban_from_ccc(ccc: &str) -> Result<String, SepaError> {
    let ccc: String = ccc.chars().filter(char::is_ascii_digit).collect();
    if ccc.len() != 20 {
        return Err(SepaError::Invalid(
            "la cuenta ha de tener 20 dígitos".into(),
        ));
    }
    let check = 98 - mod97(&format!("{ccc}ES00"));
    Ok(format!("ES{check:02}{ccc}"))
}

/// Dígitos de control de una cuenta española (CCC) a partir de entidad (4), oficina (4) y
/// número (10), como faltan en la Norma 43: módulo 11 con pesos 1, 2, 4, 8, 5, 10, 9, 7, 3, 6.
pub fn ccc_check_digits(bank: &str, office: &str, account: &str) -> Option<String> {
    let digit = |s: &str| -> Option<u32> {
        const W: [u32; 10] = [1, 2, 4, 8, 5, 10, 9, 7, 3, 6];
        if s.len() != 10 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let sum: u32 = s.bytes().zip(W).map(|(b, w)| u32::from(b - b'0') * w).sum();
        Some(match 11 - sum % 11 {
            11 => 0,
            10 => 1,
            d => d,
        })
    };
    let d1 = digit(&format!("00{bank}{office}"))?;
    let d2 = digit(account)?;
    Some(format!("{d1}{d2}"))
}

/// IBAN español desde la cuenta de la Norma 43 (entidad + oficina + número, sin control).
pub fn iban_from_n43(number: &str) -> Result<String, SepaError> {
    if number.len() != 18 {
        return Err(SepaError::Invalid(
            "la cuenta de la Norma 43 tiene 18 dígitos".into(),
        ));
    }
    let dc = ccc_check_digits(&number[0..4], &number[4..8], &number[8..18])
        .ok_or_else(|| SepaError::Invalid("cuenta no válida".into()))?;
    iban_from_ccc(&format!(
        "{}{}{dc}{}",
        &number[0..4],
        &number[4..8],
        &number[8..18]
    ))
}

/// Identificador de acreedor SEPA (AT-02, EPC262-08): país, 2 dígitos de control, sufijo de 3
/// caracteres (`000` si el banco no da otro) y NIF. El sufijo no entra en el control.
pub fn creditor_id(nif: &str, suffix: &str) -> Result<String, SepaError> {
    let nif: String = nif
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_uppercase();
    let suffix = suffix.trim().to_uppercase();
    if nif.is_empty() || nif.len() > 28 {
        return Err(SepaError::Invalid("NIF no válido para el acreedor".into()));
    }
    if suffix.len() != 3 || !suffix.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(SepaError::Invalid(
            "el sufijo del acreedor tiene 3 letras o números (normalmente 000)".into(),
        ));
    }
    let check = 98 - mod97(&format!("{nif}ES00"));
    Ok(format!("ES{check:02}{suffix}{nif}"))
}

/// Texto en el juego de caracteres SEPA (letras y números latinos, espacio y `/-?:().,'+`),
/// sin tildes y cortado a `max` caracteres.
pub fn sepa_text(s: &str, max: usize) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let mapped = match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' => 'a',
            'Á' | 'À' | 'Â' | 'Ä' | 'Ã' => 'A',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'É' | 'È' | 'Ê' | 'Ë' => 'E',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' => 'O',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
            'ñ' => 'n',
            'Ñ' => 'N',
            'ç' => 'c',
            'Ç' => 'C',
            'ª' => 'a',
            'º' => 'o',
            '&' => '+',
            c if c.is_ascii_alphanumeric() || " /-?:().,'+".contains(c) => c,
            _ => ' ',
        };
        out.push(mapped);
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .chars()
        .take(max)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Identificador (MsgId, EndToEndId, mandato…): sin espacios, sólo caracteres SEPA, ≤ 35.
fn sepa_id(s: &str) -> String {
    sepa_text(s, 35).replace(' ', "")
}

fn amount(m: Money) -> String {
    let c = m.cents();
    format!("{}.{:02}", c / 100, c % 100)
}

fn ctrl_sum(it: impl Iterator<Item = Money>) -> Money {
    it.fold(Money::ZERO, |a, b| a + b)
}

fn agent(bic: Option<&str>) -> String {
    match bic.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => format!("<FinInstnId><BIC>{}</BIC></FinInstnId>", sepa_id(b)),
        None => "<FinInstnId><Othr><Id>NOTPROVIDED</Id></Othr></FinInstnId>".into(),
    }
}

fn check_tx(amount: Money, iban: &str, who: &str) -> Result<String, SepaError> {
    if amount.cents() <= 0 {
        return Err(SepaError::Invalid(format!(
            "el importe de {who} ha de ser positivo"
        )));
    }
    normalize_iban(iban).map_err(|_| SepaError::Iban(format!("{iban} ({who})")))
}

/// Esquema de adeudo: CORE (particulares y empresas) o B2B (sólo empresas, Cuaderno 19.44).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Core,
    B2b,
}

/// Recibo de una remesa de adeudos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebitTx {
    pub end_to_end: String,
    pub amount: Money,
    pub mandate_id: String,
    pub mandate_date: Date,
    pub debtor_name: String,
    pub debtor_iban: String,
    pub remittance: String,
}

/// Remesa de adeudos directos (cobros).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectDebit {
    pub msg_id: String,
    pub created: PrimitiveDateTime,
    pub creditor_name: String,
    pub creditor_iban: String,
    pub creditor_bic: Option<String>,
    /// Identificador de acreedor (ver [`creditor_id`]).
    pub creditor_id: String,
    pub collection_date: Date,
    pub scheme: Scheme,
    pub txs: Vec<DebitTx>,
}

fn header(
    out: &mut String,
    msg_id: &str,
    created: PrimitiveDateTime,
    n: usize,
    sum: Money,
    name: &str,
    id: &str,
) {
    let _ = write!(
        out,
        "<GrpHdr><MsgId>{}</MsgId><CreDtTm>{:04}-{:02}-{:02}T{:02}:{:02}:{:02}</CreDtTm><NbOfTxs>{n}</NbOfTxs><CtrlSum>{}</CtrlSum>\
         <InitgPty><Nm>{}</Nm><Id><OrgId><Othr><Id>{}</Id></Othr></OrgId></Id></InitgPty></GrpHdr>",
        sepa_id(msg_id),
        created.year(),
        u8::from(created.month()),
        created.day(),
        created.hour(),
        created.minute(),
        created.second(),
        amount(sum),
        sepa_text(name, 70),
        sepa_id(id),
    );
}

/// Fichero `pain.008.001.02` de la remesa.
pub fn pain008(d: &DirectDebit) -> Result<String, SepaError> {
    if d.txs.is_empty() {
        return Err(SepaError::Invalid("la remesa no tiene recibos".into()));
    }
    let creditor_iban = normalize_iban(&d.creditor_iban)?;
    let sum = ctrl_sum(d.txs.iter().map(|t| t.amount));
    let n = d.txs.len();
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:pain.008.001.02\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><CstmrDrctDbtInitn>",
    );
    header(
        &mut out,
        &d.msg_id,
        d.created,
        n,
        sum,
        &d.creditor_name,
        &d.creditor_id,
    );
    let _ = write!(
        out,
        "<PmtInf><PmtInfId>{}</PmtInfId><PmtMtd>DD</PmtMtd><BtchBookg>true</BtchBookg><NbOfTxs>{n}</NbOfTxs><CtrlSum>{}</CtrlSum>\
         <PmtTpInf><SvcLvl><Cd>SEPA</Cd></SvcLvl><LclInstrm><Cd>{}</Cd></LclInstrm><SeqTp>RCUR</SeqTp></PmtTpInf>\
         <ReqdColltnDt>{}</ReqdColltnDt><Cdtr><Nm>{}</Nm></Cdtr><CdtrAcct><Id><IBAN>{creditor_iban}</IBAN></Id></CdtrAcct>\
         <CdtrAgt>{}</CdtrAgt><ChrgBr>SLEV</ChrgBr>\
         <CdtrSchmeId><Id><PrvtId><Othr><Id>{}</Id><SchmeNm><Prtry>SEPA</Prtry></SchmeNm></Othr></PrvtId></Id></CdtrSchmeId>",
        sepa_id(&format!("{}-1", d.msg_id)),
        amount(sum),
        match d.scheme {
            Scheme::Core => "CORE",
            Scheme::B2b => "B2B",
        },
        d.collection_date,
        sepa_text(&d.creditor_name, 70),
        agent(d.creditor_bic.as_deref()),
        sepa_id(&d.creditor_id),
    );
    for t in &d.txs {
        let iban = check_tx(t.amount, &t.debtor_iban, &t.debtor_name)?;
        if sepa_id(&t.mandate_id).is_empty() {
            return Err(SepaError::Invalid(format!(
                "falta la referencia del mandato de {}",
                t.debtor_name
            )));
        }
        let _ = write!(
            out,
            "<DrctDbtTxInf><PmtId><EndToEndId>{}</EndToEndId></PmtId><InstdAmt Ccy=\"EUR\">{}</InstdAmt>\
             <DrctDbtTx><MndtRltdInf><MndtId>{}</MndtId><DtOfSgntr>{}</DtOfSgntr></MndtRltdInf></DrctDbtTx>\
             <DbtrAgt>{}</DbtrAgt><Dbtr><Nm>{}</Nm></Dbtr><DbtrAcct><Id><IBAN>{iban}</IBAN></Id></DbtrAcct>\
             <RmtInf><Ustrd>{}</Ustrd></RmtInf></DrctDbtTxInf>",
            sepa_id(&t.end_to_end),
            amount(t.amount),
            sepa_id(&t.mandate_id),
            t.mandate_date,
            agent(None),
            sepa_text(&t.debtor_name, 70),
            sepa_text(&t.remittance, 140),
        );
    }
    out.push_str("</PmtInf></CstmrDrctDbtInitn></Document>\n");
    Ok(out)
}

/// Transferencia de una orden de pagos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreditTx {
    pub end_to_end: String,
    pub amount: Money,
    pub creditor_name: String,
    pub creditor_iban: String,
    pub remittance: String,
}

/// Orden de transferencias (pagos).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreditTransfer {
    pub msg_id: String,
    pub created: PrimitiveDateTime,
    pub debtor_name: String,
    /// NIF del ordenante (identificación en la cabecera).
    pub debtor_id: String,
    pub debtor_iban: String,
    pub debtor_bic: Option<String>,
    pub execution_date: Date,
    pub txs: Vec<CreditTx>,
}

/// Fichero `pain.001.001.03` de la orden.
pub fn pain001(c: &CreditTransfer) -> Result<String, SepaError> {
    if c.txs.is_empty() {
        return Err(SepaError::Invalid("la orden no tiene pagos".into()));
    }
    let debtor_iban = normalize_iban(&c.debtor_iban)?;
    let sum = ctrl_sum(c.txs.iter().map(|t| t.amount));
    let n = c.txs.len();
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:pain.001.001.03\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><CstmrCdtTrfInitn>",
    );
    header(
        &mut out,
        &c.msg_id,
        c.created,
        n,
        sum,
        &c.debtor_name,
        &c.debtor_id,
    );
    let _ = write!(
        out,
        "<PmtInf><PmtInfId>{}</PmtInfId><PmtMtd>TRF</PmtMtd><BtchBookg>true</BtchBookg><NbOfTxs>{n}</NbOfTxs><CtrlSum>{}</CtrlSum>\
         <PmtTpInf><SvcLvl><Cd>SEPA</Cd></SvcLvl></PmtTpInf><ReqdExctnDt>{}</ReqdExctnDt>\
         <Dbtr><Nm>{}</Nm></Dbtr><DbtrAcct><Id><IBAN>{debtor_iban}</IBAN></Id></DbtrAcct><DbtrAgt>{}</DbtrAgt><ChrgBr>SLEV</ChrgBr>",
        sepa_id(&format!("{}-1", c.msg_id)),
        amount(sum),
        c.execution_date,
        sepa_text(&c.debtor_name, 70),
        agent(c.debtor_bic.as_deref()),
    );
    for t in &c.txs {
        let iban = check_tx(t.amount, &t.creditor_iban, &t.creditor_name)?;
        let _ = write!(
            out,
            "<CdtTrfTxInf><PmtId><EndToEndId>{}</EndToEndId></PmtId><Amt><InstdAmt Ccy=\"EUR\">{}</InstdAmt></Amt>\
             <Cdtr><Nm>{}</Nm></Cdtr><CdtrAcct><Id><IBAN>{iban}</IBAN></Id></CdtrAcct>\
             <RmtInf><Ustrd>{}</Ustrd></RmtInf></CdtTrfTxInf>",
            sepa_id(&t.end_to_end),
            amount(t.amount),
            sepa_text(&t.creditor_name, 70),
            sepa_text(&t.remittance, 140),
        );
    }
    out.push_str("</PmtInf></CstmrCdtTrfInitn></Document>\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use time::macros::{date, datetime};

    use super::*;

    #[test]
    fn iban() {
        assert_eq!(
            normalize_iban("es91 2100 0418 4502 0005 1332").unwrap(),
            "ES9121000418450200051332"
        );
        assert!(normalize_iban("ES9221000418450200051332").is_err());
        assert!(normalize_iban("ES91 2100").is_err());
        assert_eq!(
            normalize_iban("DE89 3704 0044 0532 0130 00").unwrap(),
            "DE89370400440532013000"
        );
        assert_eq!(
            iban_from_ccc("2100 0418 45 0200051332").unwrap(),
            "ES9121000418450200051332"
        );
        assert!(iban_from_ccc("123").is_err());
        // ES91 2100 0418 45 0200051332: control 45.
        assert_eq!(
            ccc_check_digits("2100", "0418", "0200051332").as_deref(),
            Some("45")
        );
        assert_eq!(
            iban_from_n43("210004180200051332").unwrap(),
            "ES9121000418450200051332"
        );
    }

    #[test]
    fn identificador_de_acreedor() {
        // Identificador de pruebas alemán del EPC: DE98ZZZ09999999999.
        let check = 98 - mod97("09999999999DE00");
        assert_eq!(check, 98);
        let id = creditor_id("B12345674", "000").unwrap();
        assert!(id.starts_with("ES") && id.ends_with("000B12345674"));
        assert_eq!(mod97(&format!("B12345674ES{}", &id[2..4])), 1);
        assert!(creditor_id("B12345674", "0").is_err());
    }

    #[test]
    fn texto_sepa() {
        assert_eq!(
            sepa_text("Peñas & Cía. — «Diseño» nº 3", 70),
            "Penas + Cia. Diseno no 3"
        );
        assert_eq!(sepa_text("abcdef", 3), "abc");
        assert_eq!(sepa_id("FRA 2026/01"), "FRA2026/01");
    }

    fn dd() -> DirectDebit {
        DirectDebit {
            msg_id: "REM-2026-0001".into(),
            created: datetime!(2026-10-20 10:30:00),
            creditor_name: "Ejemplo Diseño SL".into(),
            creditor_iban: "ES91 2100 0418 4502 0005 1332".into(),
            creditor_bic: None,
            creditor_id: creditor_id("B12345674", "000").unwrap(),
            collection_date: date!(2026 - 11 - 05),
            scheme: Scheme::Core,
            txs: vec![
                DebitTx {
                    end_to_end: "A2026-0001/1".into(),
                    amount: Money::from_cents(121_000),
                    mandate_id: "MAND-001".into(),
                    mandate_date: date!(2026 - 01 - 15),
                    debtor_name: "Cliente Ejemplo SA".into(),
                    debtor_iban: "ES7921000813610123456789".into(),
                    remittance: "Factura A2026-0001 plazo 1/2".into(),
                },
                DebitTx {
                    end_to_end: "A2026-0002/1".into(),
                    amount: Money::from_cents(5_050),
                    mandate_id: "MAND-002".into(),
                    mandate_date: date!(2026 - 02 - 01),
                    debtor_name: "Otro Cliente SL".into(),
                    debtor_iban: "ES9121000418450200051332".into(),
                    remittance: "Factura A2026-0002".into(),
                },
            ],
        }
    }

    #[test]
    fn remesa_de_adeudos() {
        let x = pain008(&dd()).unwrap();
        assert!(x.contains("urn:iso:std:iso:20022:tech:xsd:pain.008.001.02"));
        assert!(x.contains("<NbOfTxs>2</NbOfTxs><CtrlSum>1260.50</CtrlSum>"));
        assert!(x.contains("<CreDtTm>2026-10-20T10:30:00</CreDtTm>"));
        assert!(x.contains("<ReqdColltnDt>2026-11-05</ReqdColltnDt>"));
        assert!(x.contains("<LclInstrm><Cd>CORE</Cd></LclInstrm><SeqTp>RCUR</SeqTp>"));
        assert!(x.contains("<InstdAmt Ccy=\"EUR\">50.50</InstdAmt>"));
        assert!(x.contains("<MndtId>MAND-001</MndtId><DtOfSgntr>2026-01-15</DtOfSgntr>"));
        assert!(x.contains("<Nm>Ejemplo Diseno SL</Nm>"));
        assert!(x.contains("<Othr><Id>NOTPROVIDED</Id></Othr>"));
        assert!(x.contains("<EndToEndId>A2026-0001/1</EndToEndId>"));
        assert_eq!(x.matches("<DrctDbtTxInf>").count(), 2);

        let mut bad = dd();
        bad.txs[1].debtor_iban = "ES0021000418450200051332".into();
        assert!(matches!(pain008(&bad), Err(SepaError::Iban(_))));
        let mut bad = dd();
        bad.txs[0].mandate_id = " ".into();
        assert!(pain008(&bad).is_err());
        let mut bad = dd();
        bad.txs[0].amount = Money::ZERO;
        assert!(pain008(&bad).is_err());
        let mut bad = dd();
        bad.txs.clear();
        assert!(pain008(&bad).is_err());
    }

    #[test]
    fn orden_de_transferencias() {
        let c = CreditTransfer {
            msg_id: "PAG-2026-0001".into(),
            created: datetime!(2026-10-20 10:30:00),
            debtor_name: "Ejemplo SL".into(),
            debtor_id: "B12345674".into(),
            debtor_iban: "ES9121000418450200051332".into(),
            debtor_bic: Some("CAIXESBBXXX".into()),
            execution_date: date!(2026 - 10 - 31),
            txs: vec![CreditTx {
                end_to_end: "P-7/1".into(),
                amount: Money::from_cents(24_200),
                creditor_name: "Proveedor SA".into(),
                creditor_iban: "ES7921000813610123456789".into(),
                remittance: "Factura P-7".into(),
            }],
        };
        let x = pain001(&c).unwrap();
        assert!(x.contains("urn:iso:std:iso:20022:tech:xsd:pain.001.001.03"));
        assert!(x.contains("<PmtMtd>TRF</PmtMtd>"));
        assert!(x.contains("<ReqdExctnDt>2026-10-31</ReqdExctnDt>"));
        assert!(x.contains("<BIC>CAIXESBBXXX</BIC>"));
        assert!(x.contains("<InstdAmt Ccy=\"EUR\">242.00</InstdAmt>"));
        assert!(x.contains("<IBAN>ES7921000813610123456789</IBAN>"));
    }
}

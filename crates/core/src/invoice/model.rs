//! Factura, líneas, partes y tratamiento fiscal de cada línea.

use serde::{Deserialize, Serialize};
use time::Date;

use crate::money::{Money, Rate};
use crate::nif::Nif;

/// Causa de exención (claves de la AEAT usadas en SII/VERI*FACTU).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Exemption {
    /// Art. 20 LIVA (operaciones interiores exentas: sanidad, enseñanza, seguros…).
    E1,
    /// Art. 21 LIVA (exportaciones).
    E2,
    /// Art. 22 LIVA (operaciones asimiladas a exportaciones).
    E3,
    /// Arts. 23 y 24 LIVA (zonas francas, depósitos…).
    E4,
    /// Art. 25 LIVA (entregas intracomunitarias).
    E5,
    /// Otras.
    E6,
}

impl Exemption {
    /// Mención obligatoria en la factura (RD 1619/2012 art. 6.1.j).
    /// `TODO(VERIFY)`: textos contra `docs/INVOICING_RULES.md`.
    pub fn legal_mention(self) -> &'static str {
        match self {
            Exemption::E1 => "Operación exenta de IVA (art. 20 Ley 37/1992)",
            Exemption::E2 => "Exportación exenta de IVA (art. 21 Ley 37/1992)",
            Exemption::E3 => "Operación exenta de IVA (art. 22 Ley 37/1992)",
            Exemption::E4 => "Operación exenta de IVA (arts. 23 y 24 Ley 37/1992)",
            Exemption::E5 => "Entrega intracomunitaria exenta de IVA (art. 25 Ley 37/1992)",
            Exemption::E6 => "Operación exenta de IVA",
        }
    }
}

/// Operación no sujeta (claves N1/N2 de la AEAT).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum NotSubject {
    /// Arts. 7 y 14 LIVA y otros.
    N1,
    /// Por reglas de localización (p. ej. servicios a empresas de fuera de España).
    N2,
}

/// Tratamiento del IVA de una línea.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum VatTreatment {
    /// Sujeta y no exenta, el emisor repercute (S1).
    Taxed {
        rate: Rate,
    },
    /// Sujeta y no exenta con inversión del sujeto pasivo (S2): sin cuota en la factura.
    ReverseCharge,
    Exempt(Exemption),
    NotSubject(NotSubject),
}

/// Tipo de documento de identificación de una parte extranjera (claves AEAT `IDType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdType {
    /// 02 NIF-IVA (operador intracomunitario).
    NifIva,
    /// 03 Pasaporte.
    Pasaporte,
    /// 04 Documento oficial de identificación del país de residencia.
    DocumentoOficial,
    /// 05 Certificado de residencia.
    CertificadoResidencia,
    /// 06 Otro documento probatorio.
    Otro,
    /// 07 No censado.
    NoCensado,
}

impl IdType {
    pub fn code(self) -> &'static str {
        match self {
            IdType::NifIva => "02",
            IdType::Pasaporte => "03",
            IdType::DocumentoOficial => "04",
            IdType::CertificadoResidencia => "05",
            IdType::Otro => "06",
            IdType::NoCensado => "07",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartyId {
    Nif(String),
    Foreign {
        country: String,
        id_type: IdType,
        id: String,
    },
}

/// Domicilio estructurado (lo exige Facturae).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    pub street: String,
    pub post_code: String,
    pub town: String,
    pub province: String,
    /// ISO 3166-1 alfa-2 (`ES`, `FR`…).
    pub country: String,
}

impl Address {
    /// Líneas para imprimir.
    pub fn lines(&self) -> Vec<String> {
        let mut v = vec![
            self.street.clone(),
            format!("{} {}", self.post_code, self.town),
        ];
        let mut last = self.province.clone();
        if self.country != "ES" {
            last = format!("{last} ({})", self.country);
        }
        if !last.trim().is_empty() && last != self.town {
            v.push(last);
        }
        v
    }
}

/// Nombre de persona física separado (Facturae: Name, FirstSurname, SecondSurname).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonName {
    pub name: String,
    pub first_surname: String,
    pub second_surname: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Party {
    pub id: PartyId,
    /// Nombre completo o razón social, tal como se imprime.
    pub name: String,
    pub address: Option<Address>,
    /// Sólo personas físicas.
    #[serde(default)]
    pub person: Option<PersonName>,
}

/// Tipo de factura (claves `TipoFactura` de VERI*FACTU).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvoiceKind {
    /// F1: factura completa (art. 6 RD 1619/2012).
    Completa,
    /// F2: factura simplificada (art. 7) sin identificar al destinatario.
    Simplificada,
    /// F3: factura emitida en sustitución de simplificadas.
    SustitutivaDeSimplificadas,
    /// R1–R5: rectificativa (art. 15).
    Rectificativa(Rectification),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RectificationMethod {
    /// Por sustitución (S): la rectificativa recoge el importe correcto completo.
    Sustitucion,
    /// Por diferencias (I): la rectificativa recoge sólo la diferencia.
    Diferencias,
}

/// Causa de rectificación (claves R1–R5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rectification {
    /// 1: error fundado en derecho y art. 80.Uno, Dos y Seis LIVA; 2: concurso (art. 80.Tres);
    /// 3: créditos incobrables (art. 80.Cuatro); 4: resto de causas; 5: de facturas
    /// simplificadas.
    pub code: u8,
    pub method: RectificationMethod,
}

impl InvoiceKind {
    pub fn code(self) -> String {
        match self {
            InvoiceKind::Completa => "F1".into(),
            InvoiceKind::Simplificada => "F2".into(),
            InvoiceKind::SustitutivaDeSimplificadas => "F3".into(),
            InvoiceKind::Rectificativa(r) => format!("R{}", r.code),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvoiceLine {
    pub description: String,
    /// Cantidad en milésimas (1 unidad = 1000).
    pub quantity_milli: i64,
    pub unit_price: Money,
    /// Descuento en centésimas de punto (10 % = 1000).
    pub discount: Rate,
    pub vat: VatTreatment,
    /// Recargo de equivalencia (sólo con `Taxed`).
    pub surcharge: Option<Rate>,
    /// Retención de IRPF.
    pub withholding: Option<Rate>,
    /// Anticipo: en la factura del anticipo (importe positivo) o al descontarlo en la final
    /// (negativo). Su base se contabiliza en la 438 (anticipos de clientes), no en ventas.
    #[serde(default)]
    pub advance: bool,
}

/// Datos de una factura emitida o recibida.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invoice {
    pub kind: InvoiceKind,
    /// Serie + número tal como figura en la factura (p. ej. "A2026-0001").
    pub number: String,
    pub issue_date: Date,
    /// Fecha de la operación si es distinta de la de expedición.
    pub operation_date: Option<Date>,
    pub issuer: Party,
    pub recipient: Option<Party>,
    pub lines: Vec<InvoiceLine>,
    /// Facturas rectificadas (número y fecha) si es rectificativa o F3.
    pub rectified: Vec<(String, Date)>,
    pub description: String,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("falta el número de factura")]
    MissingNumber,
    #[error("NIF del emisor no válido")]
    InvalidIssuerNif,
    #[error("la factura completa necesita destinatario identificado")]
    MissingRecipient,
    #[error("NIF del destinatario no válido")]
    InvalidRecipientNif,
    #[error("la factura no tiene líneas")]
    NoLines,
    #[error("línea {0}: cantidad o descripción vacía")]
    InvalidLine(usize),
    #[error("línea {0}: el recargo de equivalencia sólo se aplica a operaciones sujetas")]
    SurchargeWithoutVat(usize),
    #[error("la rectificativa debe indicar la factura rectificada")]
    MissingRectified,
    #[error("código de rectificativa no válido (R1–R5)")]
    InvalidRectificationCode,
    #[error(
        "la factura simplificada supera el límite de {0} (IVA incluido; art. 4 RD 1619/2012): \
         emite una factura completa"
    )]
    SimplifiedOverLimit(Money),
    #[error(
        "falta el domicilio del emisor: es obligatorio en las facturas completas y rectificativas \
         (art. 6.1.e RD 1619/2012); complétalo en los ajustes de facturación"
    )]
    MissingIssuerAddress,
    #[error(
        "falta el domicilio del destinatario: es obligatorio en las facturas completas y \
         rectificativas (art. 6.1.e RD 1619/2012); complétalo en los datos del cliente"
    )]
    MissingRecipientAddress,
    #[error(
        "la factura simplificada (F2) y su rectificativa (R5) no identifican al destinatario: \
         para identificarlo, emite una factura completa"
    )]
    RecipientNotAllowed,
}

/// Límite general de la factura simplificada: 400 €, IVA incluido (art. 4.1.a RD 1619/2012).
pub const SIMPLIFIED_LIMIT: Money = Money::from_cents(40_000);

/// Límite de la simplificada en las operaciones del art. 4.2 RD 1619/2012 (ventas al por menor,
/// ambulancia, a domicilio, transporte de personas, hostelería y restauración, peluquería…):
/// 3.000 €, IVA incluido.
pub const SIMPLIFIED_LIMIT_ART_4_2: Money = Money::from_cents(300_000);

impl Invoice {
    /// Comprobaciones de forma del art. 6 RD 1619/2012 (no sustituyen al criterio fiscal).
    /// `simplified_limit`: [`SIMPLIFIED_LIMIT`], o [`SIMPLIFIED_LIMIT_ART_4_2`] si la empresa
    /// hace operaciones del art. 4.2.
    pub fn validate(&self, total: Money, simplified_limit: Money) -> Result<(), ValidationError> {
        if self.number.trim().is_empty() {
            return Err(ValidationError::MissingNumber);
        }
        match &self.issuer.id {
            PartyId::Nif(n) if Nif::parse(n).is_ok() => {}
            _ => return Err(ValidationError::InvalidIssuerNif),
        }
        let needs_recipient = !matches!(
            self.kind,
            InvoiceKind::Simplificada | InvoiceKind::Rectificativa(Rectification { code: 5, .. })
        );
        match (&self.recipient, needs_recipient) {
            (None, true) => return Err(ValidationError::MissingRecipient),
            // VERI*FACTU (validaciones v1.2.2, 3.1.3.13): «Si TipoFactura es F2 o R5, la
            // agrupación Destinatarios no puede estar cumplimentada»; la AEAT rechaza el registro.
            (Some(_), false) => return Err(ValidationError::RecipientNotAllowed),
            (
                Some(Party {
                    id: PartyId::Nif(n),
                    ..
                }),
                _,
            ) if Nif::parse(n).is_err() => {
                return Err(ValidationError::InvalidRecipientNif);
            }
            _ => {}
        }
        // Domicilio del emisor y del destinatario en la factura completa (F1, F3) y en sus
        // rectificativas (R1–R4): art. 6.1.e RD 1619/2012.
        if needs_recipient {
            let has = |p: &Party| {
                p.address
                    .as_ref()
                    .is_some_and(|a| !a.street.trim().is_empty())
            };
            if !has(&self.issuer) {
                return Err(ValidationError::MissingIssuerAddress);
            }
            if !self.recipient.as_ref().is_some_and(has) {
                return Err(ValidationError::MissingRecipientAddress);
            }
        }
        if self.lines.is_empty() {
            return Err(ValidationError::NoLines);
        }
        for (i, l) in self.lines.iter().enumerate() {
            if l.quantity_milli == 0 || l.description.trim().is_empty() {
                return Err(ValidationError::InvalidLine(i + 1));
            }
            if l.surcharge.is_some() && !matches!(l.vat, VatTreatment::Taxed { .. }) {
                return Err(ValidationError::SurchargeWithoutVat(i + 1));
            }
        }
        match self.kind {
            InvoiceKind::Rectificativa(r) => {
                if !(1..=5).contains(&r.code) {
                    return Err(ValidationError::InvalidRectificationCode);
                }
                if self.rectified.is_empty() {
                    return Err(ValidationError::MissingRectified);
                }
            }
            InvoiceKind::SustitutivaDeSimplificadas if self.rectified.is_empty() => {
                return Err(ValidationError::MissingRectified);
            }
            // Límite de la simplificada: 400 € (art. 4.1.a RD 1619/2012) o 3.000 € en las
            // operaciones del art. 4.2.
            InvoiceKind::Simplificada if total.abs() > simplified_limit => {
                return Err(ValidationError::SimplifiedOverLimit(simplified_limit));
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn addr() -> Option<Address> {
        Some(Address {
            street: "Calle Inventada 1".into(),
            post_code: "46001".into(),
            town: "València".into(),
            province: "Valencia".into(),
            country: "ES".into(),
        })
    }

    fn base() -> Invoice {
        Invoice {
            kind: InvoiceKind::Completa,
            number: "A2026-0001".into(),
            issue_date: date!(2026 - 10 - 13),
            operation_date: None,
            issuer: Party {
                id: PartyId::Nif("12345678Z".into()),
                name: "Emisor".into(),
                address: addr(),
                person: None,
            },
            recipient: Some(Party {
                id: PartyId::Nif("B12345674".into()),
                name: "Cliente SL".into(),
                address: addr(),
                person: None,
            }),
            lines: vec![InvoiceLine {
                description: "Servicio".into(),
                quantity_milli: 1000,
                unit_price: Money::from_euros(100, 0),
                discount: Rate::ZERO,
                vat: VatTreatment::Taxed {
                    rate: Rate::percent(21),
                },
                surcharge: None,
                withholding: None,
                advance: false,
            }],
            rectified: vec![],
            description: "Servicios de consultoría".into(),
            notes: None,
        }
    }

    #[test]
    fn valida_factura_completa() {
        assert_eq!(
            base().validate(Money::from_euros(121, 0), SIMPLIFIED_LIMIT),
            Ok(())
        );
    }

    #[test]
    fn errores_de_forma() {
        let t = Money::from_euros(121, 0);
        let mut f = base();
        f.recipient = None;
        assert_eq!(
            f.validate(t, SIMPLIFIED_LIMIT),
            Err(ValidationError::MissingRecipient)
        );
        let mut f = base();
        f.issuer.id = PartyId::Nif("12345678A".into());
        assert_eq!(
            f.validate(t, SIMPLIFIED_LIMIT),
            Err(ValidationError::InvalidIssuerNif)
        );
        let mut f = base();
        f.lines[0].surcharge = Some(Rate::from_bp(520));
        f.lines[0].vat = VatTreatment::Exempt(Exemption::E1);
        assert_eq!(
            f.validate(t, SIMPLIFIED_LIMIT),
            Err(ValidationError::SurchargeWithoutVat(1))
        );
        let mut f = base();
        f.kind = InvoiceKind::Rectificativa(Rectification {
            code: 1,
            method: RectificationMethod::Diferencias,
        });
        assert_eq!(
            f.validate(t, SIMPLIFIED_LIMIT),
            Err(ValidationError::MissingRectified)
        );
        f.rectified
            .push(("A2026-0001".into(), date!(2026 - 10 - 01)));
        assert_eq!(f.validate(t, SIMPLIFIED_LIMIT), Ok(()));
        assert_eq!(f.kind.code(), "R1");
    }

    #[test]
    fn simplificada_sin_destinatario_y_limite() {
        let mut f = base();
        f.kind = InvoiceKind::Simplificada;
        f.recipient = None;
        assert_eq!(
            f.validate(Money::from_euros(121, 0), SIMPLIFIED_LIMIT),
            Ok(())
        );
        // 400 € con carácter general (art. 4.1.a RD 1619/2012)…
        assert_eq!(
            f.validate(Money::from_euros(400, 0), SIMPLIFIED_LIMIT),
            Ok(())
        );
        assert_eq!(
            f.validate(Money::from_euros(400, 1), SIMPLIFIED_LIMIT),
            Err(ValidationError::SimplifiedOverLimit(SIMPLIFIED_LIMIT))
        );
        // … y 3.000 € en las operaciones del art. 4.2 (comercio minorista, hostelería…).
        assert_eq!(
            f.validate(Money::from_euros(3000, 0), SIMPLIFIED_LIMIT_ART_4_2),
            Ok(())
        );
        assert_eq!(
            f.validate(Money::from_euros(3000, 1), SIMPLIFIED_LIMIT_ART_4_2),
            Err(ValidationError::SimplifiedOverLimit(
                SIMPLIFIED_LIMIT_ART_4_2
            ))
        );
        // Sin domicilio: la simplificada no lo exige.
        f.issuer.address = None;
        assert_eq!(
            f.validate(Money::from_euros(121, 0), SIMPLIFIED_LIMIT),
            Ok(())
        );
        assert!(
            ValidationError::SimplifiedOverLimit(SIMPLIFIED_LIMIT)
                .to_string()
                .contains("400,00 €")
        );
    }

    #[test]
    fn completa_y_rectificativa_llevan_domicilio_de_ambos() {
        // Art. 6.1.e RD 1619/2012.
        let t = Money::from_euros(121, 0);
        let r1 = InvoiceKind::Rectificativa(Rectification {
            code: 1,
            method: RectificationMethod::Diferencias,
        });
        for kind in [
            InvoiceKind::Completa,
            InvoiceKind::SustitutivaDeSimplificadas,
            r1,
        ] {
            let mut f = base();
            f.kind = kind;
            f.rectified = vec![("A2026-0000".into(), date!(2026 - 10 - 01))];
            assert_eq!(f.validate(t, SIMPLIFIED_LIMIT), Ok(()), "{kind:?}");
            let mut g = f.clone();
            g.issuer.address = None;
            assert_eq!(
                g.validate(t, SIMPLIFIED_LIMIT),
                Err(ValidationError::MissingIssuerAddress),
                "{kind:?}"
            );
            let mut g = f.clone();
            g.recipient.as_mut().unwrap().address = None;
            assert_eq!(
                g.validate(t, SIMPLIFIED_LIMIT),
                Err(ValidationError::MissingRecipientAddress),
                "{kind:?}"
            );
            let mut g = f;
            g.recipient
                .as_mut()
                .unwrap()
                .address
                .as_mut()
                .unwrap()
                .street = "  ".into();
            assert_eq!(
                g.validate(t, SIMPLIFIED_LIMIT),
                Err(ValidationError::MissingRecipientAddress),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn simplificada_y_r5_no_llevan_destinatario() {
        // Validación VERI*FACTU 3.1.3.13: con F2 o R5 el bloque Destinatarios se rechaza.
        let t = Money::from_euros(121, 0);
        let mut f = base();
        f.kind = InvoiceKind::Simplificada;
        assert_eq!(
            f.validate(t, SIMPLIFIED_LIMIT),
            Err(ValidationError::RecipientNotAllowed)
        );
        let mut f = base();
        f.kind = InvoiceKind::Rectificativa(Rectification {
            code: 5,
            method: RectificationMethod::Diferencias,
        });
        f.rectified = vec![("A2026-0000".into(), date!(2026 - 10 - 01))];
        assert_eq!(
            f.validate(t, SIMPLIFIED_LIMIT),
            Err(ValidationError::RecipientNotAllowed)
        );
        f.recipient = None;
        assert_eq!(f.validate(t, SIMPLIFIED_LIMIT), Ok(()));
    }

    #[test]
    fn destinatario_extranjero() {
        let mut f = base();
        f.recipient = Some(Party {
            id: PartyId::Foreign {
                country: "FR".into(),
                id_type: IdType::NifIva,
                id: "FR12345678901".into(),
            },
            name: "Client SARL".into(),
            address: addr(),
            person: None,
        });
        f.lines[0].vat = VatTreatment::Exempt(Exemption::E5);
        assert_eq!(
            f.validate(Money::from_euros(100, 0), SIMPLIFIED_LIMIT),
            Ok(())
        );
    }
}

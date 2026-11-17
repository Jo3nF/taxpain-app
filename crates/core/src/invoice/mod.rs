//! Facturación (Fase 3): dominio puro de facturas emitidas y recibidas.
//!
//! Normas: Reglamento de facturación (RD 1619/2012), Ley 37/1992 del IVA y, para los registros,
//! RD 1007/2023 y Orden HAC/1177/2024 (VERI*FACTU). Ver `docs/INVOICING_RULES.md`.

pub mod model;
pub mod oss;
pub mod series;
pub mod totals;

pub use model::{
    Address, Exemption, IdType, Invoice, InvoiceKind, InvoiceLine, NotSubject, Party, PartyId,
    PersonName, Rectification, RectificationMethod, SIMPLIFIED_LIMIT, SIMPLIFIED_LIMIT_ART_4_2,
    ValidationError, VatTreatment,
};
pub use oss::{OssSupply, OssVat};
pub use series::{NumberFormat, Series};
pub use totals::{RoundingMode, TaxBreakdown, Totals, compute_totals};

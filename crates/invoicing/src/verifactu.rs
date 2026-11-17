//! VERI*FACTU (RD 1007/2023, Orden HAC/1177/2024): registros de facturación de alta y anulación
//! encadenados con huella SHA-256, URL del QR, sobre SOAP y lectura de la respuesta.
//! Especificación y fuentes en `docs/VERIFACTU.md`; vectores oficiales en
//! `fixtures/aeat/verifactu/`.
//!
//! Este módulo no envía nada: el envío (mTLS, sólo tras confirmación del usuario) está en
//! `taxpain-aeat`.

use std::fmt::Write;

use quick_xml::Reader;
use quick_xml::events::Event;
use sha2::{Digest, Sha256};
use taxpain_core::invoice::{
    Exemption, Invoice, InvoiceKind, NotSubject, PartyId, RectificationMethod, Totals, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use time::{Date, OffsetDateTime};

pub const NS_SF: &str = "https://www2.agenciatributaria.gob.es/static_files/common/internet/dep/aplicaciones/es/aeat/tike/cont/ws/SuministroInformacion.xsd";
pub const NS_SFLR: &str = "https://www2.agenciatributaria.gob.es/static_files/common/internet/dep/aplicaciones/es/aeat/tike/cont/ws/SuministroLR.xsd";
pub const NS_SFLRC: &str = "https://www2.agenciatributaria.gob.es/static_files/common/internet/dep/aplicaciones/es/aeat/tike/cont/ws/ConsultaLR.xsd";
const NS_SOAP: &str = "http://schemas.xmlsoap.org/soap/envelope/";

/// Máximo de registros por envío ([SW] descripción de servicios web v1.0.3).
pub const MAX_RECORDS_PER_SUBMISSION: usize = 1000;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum VerifactuError {
    #[error("el emisor debe tener NIF español")]
    IssuerWithoutNif,
    #[error("el desglose supera 12 líneas")]
    TooManyBreakdownLines,
    #[error("número de serie no admitido (máx. 60, ASCII 32–126 sin \" ' < > =)")]
    InvalidSeriesNumber,
    #[error("una rectificativa por sustitución necesita el importe rectificado")]
    MissingRectifiedAmount,
    #[error("demasiados registros en un envío")]
    TooManyRecords,
    #[error("respuesta de la AEAT no válida: {0}")]
    InvalidResponse(String),
}

/// Datos del sistema informático (SIF) que identifican a TaxPain en cada registro.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SistemaInformatico {
    pub nombre_razon: String,
    pub nif: String,
    pub nombre_sistema: String,
    /// 2 caracteres.
    pub id_sistema: String,
    pub version: String,
    /// Identificador único de esta instalación.
    pub numero_instalacion: String,
    pub solo_verifactu: bool,
    pub multi_ot: bool,
    pub indicador_multiples_ot: bool,
}

impl SistemaInformatico {
    /// TaxPain, con su productor ([`taxpain_core::productor`]). `TODO(VERIFY)`: datos
    /// definitivos con la declaración responsable (Orden HAC/1177/2024 art. 15).
    pub fn taxpain(numero_instalacion: &str) -> Self {
        SistemaInformatico {
            nombre_razon: taxpain_core::productor::NOMBRE.into(),
            nif: taxpain_core::productor::NIF.into(),
            nombre_sistema: "TaxPain".into(),
            id_sistema: "TP".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            numero_instalacion: numero_instalacion.into(),
            solo_verifactu: true,
            multi_ot: false,
            indicador_multiples_ot: false,
        }
    }
}

/// Referencia al registro anterior de la cadena del obligado tributario.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainLink {
    pub emisor_nif: String,
    pub num_serie: String,
    /// `DD-MM-YYYY`.
    pub fecha: String,
    pub huella: String,
}

/// Registro generado: XML del elemento `sf:RegistroAlta`/`sf:RegistroAnulacion` y su huella.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub xml: String,
    pub huella: String,
    /// Eslabón que debe usar el siguiente registro.
    pub link: ChainLink,
}

/// `RechazoPrevio` de una subsanación (docs/VERIFACTU.md §9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RechazoPrevio {
    /// La AEAT rechazó una subsanación anterior; la clave existe en la AEAT.
    S,
    /// El registro no existe en la AEAT (se rechazó el alta o nunca se envió).
    X,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AltaOptions {
    /// Reenvío de un registro rechazado o con errores (`Subsanacion=S`).
    pub subsanacion: bool,
    /// Sólo con `subsanacion`: rechazo previo (`S`/`X`); `None` = `N`.
    pub rechazo_previo: Option<RechazoPrevio>,
    /// Para rectificativas por sustitución: (base rectificada, cuota rectificada), la suma de
    /// las facturas que sustituye. La cuota es sólo la de IVA.
    pub importe_rectificacion: Option<(Money, Money)>,
    /// Cuota de recargo de equivalencia de las facturas sustituidas (`CuotaRecargoRectificado`,
    /// opcional: sólo si la tenían).
    pub recargo_rectificado: Option<Money>,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Importe con dos decimales y punto: `123.45`, `-0.05`.
pub fn amount(m: Money) -> String {
    let c = m.cents();
    let sign = if c < 0 { "-" } else { "" };
    format!(
        "{sign}{}.{:02}",
        c.unsigned_abs() / 100,
        c.unsigned_abs() % 100
    )
}

fn rate(r: Rate) -> String {
    format!("{}.{:02}", r.bp() / 100, r.bp() % 100)
}

/// `DD-MM-YYYY`.
pub fn fecha(d: Date) -> String {
    format!("{:02}-{:02}-{:04}", d.day(), d.month() as u8, d.year())
}

/// `YYYY-MM-DDThh:mm:ss±hh:mm`.
pub fn timestamp(t: OffsetDateTime) -> String {
    let off = t.offset();
    let (h, m, _) = off.as_hms();
    let sign = if off.is_negative() { '-' } else { '+' };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
        t.year(),
        t.month() as u8,
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        h.unsigned_abs(),
        m.unsigned_abs()
    )
}

fn sha256_upper(input: &str) -> String {
    Sha256::digest(input.as_bytes())
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect()
}

fn chain_string(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", v.trim()))
        .collect::<Vec<_>>()
        .join("&")
}

/// Huella de un registro de alta ([HASH] §5, pág. 6).
#[allow(clippy::too_many_arguments)]
pub fn huella_alta(
    id_emisor: &str,
    num_serie: &str,
    fecha_expedicion: &str,
    tipo_factura: &str,
    cuota_total: &str,
    importe_total: &str,
    huella_anterior: &str,
    fecha_hora_huso: &str,
) -> String {
    sha256_upper(&chain_string(&[
        ("IDEmisorFactura", id_emisor),
        ("NumSerieFactura", num_serie),
        ("FechaExpedicionFactura", fecha_expedicion),
        ("TipoFactura", tipo_factura),
        ("CuotaTotal", cuota_total),
        ("ImporteTotal", importe_total),
        ("Huella", huella_anterior),
        ("FechaHoraHusoGenRegistro", fecha_hora_huso),
    ]))
}

/// Huella de un registro de anulación ([HASH] §5).
pub fn huella_anulacion(
    id_emisor: &str,
    num_serie: &str,
    fecha_expedicion: &str,
    huella_anterior: &str,
    fecha_hora_huso: &str,
) -> String {
    sha256_upper(&chain_string(&[
        ("IDEmisorFacturaAnulada", id_emisor),
        ("NumSerieFacturaAnulada", num_serie),
        ("FechaExpedicionFacturaAnulada", fecha_expedicion),
        ("Huella", huella_anterior),
        ("FechaHoraHusoGenRegistro", fecha_hora_huso),
    ]))
}

fn check_series(s: &str) -> Result<(), VerifactuError> {
    let ok = !s.trim().is_empty()
        && s.chars().count() <= 60
        && s.chars()
            .all(|c| (' '..='~').contains(&c) && !matches!(c, '"' | '\'' | '<' | '>' | '='));
    if ok {
        Ok(())
    } else {
        Err(VerifactuError::InvalidSeriesNumber)
    }
}

fn sistema_xml(x: &mut String, s: &SistemaInformatico) {
    let yn = |b: bool| if b { "S" } else { "N" };
    let _ = write!(
        x,
        "<sf:SistemaInformatico><sf:NombreRazon>{}</sf:NombreRazon><sf:NIF>{}</sf:NIF>\
         <sf:NombreSistemaInformatico>{}</sf:NombreSistemaInformatico>\
         <sf:IdSistemaInformatico>{}</sf:IdSistemaInformatico><sf:Version>{}</sf:Version>\
         <sf:NumeroInstalacion>{}</sf:NumeroInstalacion>\
         <sf:TipoUsoPosibleSoloVerifactu>{}</sf:TipoUsoPosibleSoloVerifactu>\
         <sf:TipoUsoPosibleMultiOT>{}</sf:TipoUsoPosibleMultiOT>\
         <sf:IndicadorMultiplesOT>{}</sf:IndicadorMultiplesOT></sf:SistemaInformatico>",
        esc(&s.nombre_razon),
        esc(&s.nif),
        esc(&s.nombre_sistema),
        esc(&s.id_sistema),
        esc(&s.version),
        esc(&s.numero_instalacion),
        yn(s.solo_verifactu),
        yn(s.multi_ot),
        yn(s.indicador_multiples_ot)
    );
}

fn chain_xml(x: &mut String, prev: Option<&ChainLink>) {
    match prev {
        None => x.push_str(
            "<sf:Encadenamiento><sf:PrimerRegistro>S</sf:PrimerRegistro></sf:Encadenamiento>",
        ),
        Some(p) => {
            let _ = write!(
                x,
                "<sf:Encadenamiento><sf:RegistroAnterior><sf:IDEmisorFactura>{}</sf:IDEmisorFactura>\
                 <sf:NumSerieFactura>{}</sf:NumSerieFactura>\
                 <sf:FechaExpedicionFactura>{}</sf:FechaExpedicionFactura>\
                 <sf:Huella>{}</sf:Huella></sf:RegistroAnterior></sf:Encadenamiento>",
                esc(&p.emisor_nif),
                esc(&p.num_serie),
                p.fecha,
                p.huella
            );
        }
    }
}

/// Clave de régimen por línea de desglose. `TODO(VERIFY)`: régimen del vendedor que repercute
/// recargo de equivalencia y de otras exenciones.
///
/// Ventanilla única (`oss`): las líneas no sujetas por reglas de localización (N2) de una venta
/// que tributa en el Estado miembro del consumidor llevan la clave 17 («Operación acogida a
/// alguno de los regímenes previstos en el Capítulo XI del Título IX», OSS e IOSS; lista L8A).
/// `TODO(VERIFY)`: que la AEAT acepte la clave 17 con N2 e `ImporteTotal` con el IVA de destino
/// (base + IVA del otro Estado, sin cuota española), y que no exija otra marca.
fn clave_regimen(v: VatTreatment, oss: bool) -> &'static str {
    match v {
        VatTreatment::Exempt(Exemption::E2) => "02",
        VatTreatment::NotSubject(taxpain_core::invoice::NotSubject::N2) if oss => "17",
        _ => "01",
    }
}

fn desglose_xml(x: &mut String, totals: &Totals) -> Result<(), VerifactuError> {
    if totals.breakdown.len() > 12 {
        return Err(VerifactuError::TooManyBreakdownLines);
    }
    x.push_str("<sf:Desglose>");
    for b in &totals.breakdown {
        let _ = write!(
            x,
            "<sf:DetalleDesglose><sf:Impuesto>01</sf:Impuesto><sf:ClaveRegimen>{}</sf:ClaveRegimen>",
            clave_regimen(b.vat, totals.oss.is_some())
        );
        match b.vat {
            VatTreatment::Taxed { rate: r } => {
                let _ = write!(
                    x,
                    "<sf:CalificacionOperacion>S1</sf:CalificacionOperacion><sf:TipoImpositivo>{}</sf:TipoImpositivo>\
                     <sf:BaseImponibleOimporteNoSujeto>{}</sf:BaseImponibleOimporteNoSujeto>\
                     <sf:CuotaRepercutida>{}</sf:CuotaRepercutida>",
                    rate(r),
                    amount(b.base),
                    amount(b.vat_amount)
                );
                if let Some(re) = b.surcharge_rate {
                    let _ = write!(
                        x,
                        "<sf:TipoRecargoEquivalencia>{}</sf:TipoRecargoEquivalencia>\
                         <sf:CuotaRecargoEquivalencia>{}</sf:CuotaRecargoEquivalencia>",
                        rate(re),
                        amount(b.surcharge_amount)
                    );
                }
            }
            // TODO(VERIFY): en S2 la AEAT espera tipo y cuota a cero.
            VatTreatment::ReverseCharge => {
                let _ = write!(
                    x,
                    "<sf:CalificacionOperacion>S2</sf:CalificacionOperacion><sf:TipoImpositivo>0.00</sf:TipoImpositivo>\
                     <sf:BaseImponibleOimporteNoSujeto>{}</sf:BaseImponibleOimporteNoSujeto>\
                     <sf:CuotaRepercutida>0.00</sf:CuotaRepercutida>",
                    amount(b.base)
                );
            }
            VatTreatment::Exempt(e) => {
                let _ = write!(
                    x,
                    "<sf:OperacionExenta>{e:?}</sf:OperacionExenta>\
                     <sf:BaseImponibleOimporteNoSujeto>{}</sf:BaseImponibleOimporteNoSujeto>",
                    amount(b.base)
                );
            }
            VatTreatment::NotSubject(n) => {
                let code = match n {
                    NotSubject::N1 => "N1",
                    NotSubject::N2 => "N2",
                };
                let _ = write!(
                    x,
                    "<sf:CalificacionOperacion>{code}</sf:CalificacionOperacion>\
                     <sf:BaseImponibleOimporteNoSujeto>{}</sf:BaseImponibleOimporteNoSujeto>",
                    amount(b.base)
                );
            }
        }
        x.push_str("</sf:DetalleDesglose>");
    }
    x.push_str("</sf:Desglose>");
    Ok(())
}

/// Registro de alta de `invoice`, encadenado a `prev` (`None` = primer registro del obligado).
pub fn alta(
    invoice: &Invoice,
    totals: &Totals,
    sistema: &SistemaInformatico,
    prev: Option<&ChainLink>,
    generated_at: OffsetDateTime,
    opts: &AltaOptions,
) -> Result<Record, VerifactuError> {
    let PartyId::Nif(emisor) = &invoice.issuer.id else {
        return Err(VerifactuError::IssuerWithoutNif);
    };
    check_series(&invoice.number)?;
    let tipo = invoice.kind.code();
    let f_exp = fecha(invoice.issue_date);
    let cuota = amount(totals.tax_total);
    let importe = amount(totals.invoice_total);
    let ts = timestamp(generated_at);
    let huella = huella_alta(
        emisor,
        &invoice.number,
        &f_exp,
        &tipo,
        &cuota,
        &importe,
        prev.map_or("", |p| p.huella.as_str()),
        &ts,
    );

    let mut x = String::with_capacity(2048);
    let _ = write!(
        x,
        "<sf:RegistroAlta><sf:IDVersion>1.0</sf:IDVersion><sf:IDFactura>\
         <sf:IDEmisorFactura>{}</sf:IDEmisorFactura><sf:NumSerieFactura>{}</sf:NumSerieFactura>\
         <sf:FechaExpedicionFactura>{f_exp}</sf:FechaExpedicionFactura></sf:IDFactura>\
         <sf:NombreRazonEmisor>{}</sf:NombreRazonEmisor>",
        esc(emisor),
        esc(&invoice.number),
        esc(&invoice.issuer.name.chars().take(120).collect::<String>())
    );
    if opts.subsanacion {
        x.push_str("<sf:Subsanacion>S</sf:Subsanacion>");
        match opts.rechazo_previo {
            Some(RechazoPrevio::S) => x.push_str("<sf:RechazoPrevio>S</sf:RechazoPrevio>"),
            Some(RechazoPrevio::X) => x.push_str("<sf:RechazoPrevio>X</sf:RechazoPrevio>"),
            None => {}
        }
    }
    let _ = write!(x, "<sf:TipoFactura>{tipo}</sf:TipoFactura>");
    if let InvoiceKind::Rectificativa(r) = invoice.kind {
        let t = match r.method {
            RectificationMethod::Sustitucion => "S",
            RectificationMethod::Diferencias => "I",
        };
        let _ = write!(x, "<sf:TipoRectificativa>{t}</sf:TipoRectificativa>");
    }
    if matches!(
        invoice.kind,
        InvoiceKind::Rectificativa(_) | InvoiceKind::SustitutivaDeSimplificadas
    ) && !invoice.rectified.is_empty()
    {
        let (wrapper, item) = if matches!(invoice.kind, InvoiceKind::Rectificativa(_)) {
            ("FacturasRectificadas", "IDFacturaRectificada")
        } else {
            ("FacturasSustituidas", "IDFacturaSustituida")
        };
        let _ = write!(x, "<sf:{wrapper}>");
        for (num, d) in &invoice.rectified {
            let _ = write!(
                x,
                "<sf:{item}><sf:IDEmisorFactura>{}</sf:IDEmisorFactura><sf:NumSerieFactura>{}</sf:NumSerieFactura>\
                 <sf:FechaExpedicionFactura>{}</sf:FechaExpedicionFactura></sf:{item}>",
                esc(emisor),
                esc(num),
                fecha(*d)
            );
        }
        let _ = write!(x, "</sf:{wrapper}>");
    }
    if let InvoiceKind::Rectificativa(r) = invoice.kind
        && r.method == RectificationMethod::Sustitucion
    {
        let (b, c) = opts
            .importe_rectificacion
            .ok_or(VerifactuError::MissingRectifiedAmount)?;
        let _ = write!(
            x,
            "<sf:ImporteRectificacion><sf:BaseRectificada>{}</sf:BaseRectificada>\
             <sf:CuotaRectificada>{}</sf:CuotaRectificada>",
            amount(b),
            amount(c)
        );
        if let Some(re) = opts.recargo_rectificado.filter(|m| !m.is_zero()) {
            let _ = write!(
                x,
                "<sf:CuotaRecargoRectificado>{}</sf:CuotaRecargoRectificado>",
                amount(re)
            );
        }
        x.push_str("</sf:ImporteRectificacion>");
    }
    if let Some(od) = invoice.operation_date.filter(|d| *d != invoice.issue_date) {
        let _ = write!(x, "<sf:FechaOperacion>{}</sf:FechaOperacion>", fecha(od));
    }
    let desc: String = invoice.description.chars().take(500).collect();
    let _ = write!(
        x,
        "<sf:DescripcionOperacion>{}</sf:DescripcionOperacion>",
        esc(if desc.trim().is_empty() {
            "Venta"
        } else {
            &desc
        })
    );
    // Macrodato: obligatorio si |ImporteTotal| ≥ 100.000.000,00 ([VAL] 3.1.3.10); si no, se omite.
    if totals.invoice_total.cents().abs() >= 10_000_000_000 {
        x.push_str("<sf:Macrodato>S</sf:Macrodato>");
    }
    if let Some(r) = &invoice.recipient {
        let _ = write!(
            x,
            "<sf:Destinatarios><sf:IDDestinatario><sf:NombreRazon>{}</sf:NombreRazon>",
            esc(&r.name.chars().take(120).collect::<String>())
        );
        match &r.id {
            PartyId::Nif(n) => {
                let _ = write!(x, "<sf:NIF>{}</sf:NIF>", esc(n));
            }
            PartyId::Foreign {
                country,
                id_type,
                id,
            } => {
                let _ = write!(
                    x,
                    "<sf:IDOtro><sf:CodigoPais>{}</sf:CodigoPais><sf:IDType>{}</sf:IDType><sf:ID>{}</sf:ID></sf:IDOtro>",
                    esc(country),
                    id_type.code(),
                    esc(id)
                );
            }
        }
        x.push_str("</sf:IDDestinatario></sf:Destinatarios>");
    }
    desglose_xml(&mut x, totals)?;
    let _ = write!(
        x,
        "<sf:CuotaTotal>{cuota}</sf:CuotaTotal><sf:ImporteTotal>{importe}</sf:ImporteTotal>"
    );
    chain_xml(&mut x, prev);
    sistema_xml(&mut x, sistema);
    let _ = write!(
        x,
        "<sf:FechaHoraHusoGenRegistro>{ts}</sf:FechaHoraHusoGenRegistro>\
         <sf:TipoHuella>01</sf:TipoHuella><sf:Huella>{huella}</sf:Huella></sf:RegistroAlta>"
    );
    Ok(Record {
        xml: x,
        link: ChainLink {
            emisor_nif: emisor.clone(),
            num_serie: invoice.number.clone(),
            fecha: f_exp,
            huella: huella.clone(),
        },
        huella,
    })
}

/// Registro de anulación de la factura (`emisor`, `num_serie`, `fecha_expedicion`).
/// Registro de anulación. `rechazo_previo`: reenvío tras el rechazo de una anulación anterior.
pub fn anulacion(
    emisor: &str,
    num_serie: &str,
    fecha_expedicion: Date,
    sistema: &SistemaInformatico,
    prev: Option<&ChainLink>,
    generated_at: OffsetDateTime,
    rechazo_previo: bool,
) -> Result<Record, VerifactuError> {
    check_series(num_serie)?;
    let f = fecha(fecha_expedicion);
    let ts = timestamp(generated_at);
    let huella = huella_anulacion(
        emisor,
        num_serie,
        &f,
        prev.map_or("", |p| p.huella.as_str()),
        &ts,
    );
    let mut x = String::with_capacity(1024);
    let _ = write!(
        x,
        "<sf:RegistroAnulacion><sf:IDVersion>1.0</sf:IDVersion><sf:IDFactura>\
         <sf:IDEmisorFacturaAnulada>{}</sf:IDEmisorFacturaAnulada>\
         <sf:NumSerieFacturaAnulada>{}</sf:NumSerieFacturaAnulada>\
         <sf:FechaExpedicionFacturaAnulada>{f}</sf:FechaExpedicionFacturaAnulada></sf:IDFactura>",
        esc(emisor),
        esc(num_serie)
    );
    if rechazo_previo {
        x.push_str("<sf:RechazoPrevio>S</sf:RechazoPrevio>");
    }
    chain_xml(&mut x, prev);
    sistema_xml(&mut x, sistema);
    let _ = write!(
        x,
        "<sf:FechaHoraHusoGenRegistro>{ts}</sf:FechaHoraHusoGenRegistro>\
         <sf:TipoHuella>01</sf:TipoHuella><sf:Huella>{huella}</sf:Huella></sf:RegistroAnulacion>"
    );
    Ok(Record {
        xml: x,
        link: ChainLink {
            emisor_nif: emisor.into(),
            num_serie: num_serie.into(),
            fecha: f,
            huella: huella.clone(),
        },
        huella,
    })
}

/// Cuerpo `sfLR:RegFactuSistemaFacturacion` (sin sobre SOAP).
pub fn submission(
    obligado_nombre: &str,
    obligado_nif: &str,
    records: &[&Record],
) -> Result<String, VerifactuError> {
    submission_with(obligado_nombre, obligado_nif, records, false)
}

/// Como [`submission`]; con `incidencia`, la cabecera lleva
/// `RemisionVoluntaria/Incidencia = S`: los registros no se pudieron remitir en su momento por
/// una incidencia técnica (Orden HAC/1177/2024 art. 16.4; docs/VERIFACTU.md §7).
pub fn submission_with(
    obligado_nombre: &str,
    obligado_nif: &str,
    records: &[&Record],
    incidencia: bool,
) -> Result<String, VerifactuError> {
    if records.is_empty() || records.len() > MAX_RECORDS_PER_SUBMISSION {
        return Err(VerifactuError::TooManyRecords);
    }
    let mut x = String::new();
    let _ = write!(
        x,
        "<sfLR:RegFactuSistemaFacturacion xmlns:sfLR=\"{NS_SFLR}\" xmlns:sf=\"{NS_SF}\">\
         <sfLR:Cabecera><sf:ObligadoEmision><sf:NombreRazon>{}</sf:NombreRazon><sf:NIF>{}</sf:NIF>\
         </sf:ObligadoEmision>{}</sfLR:Cabecera>",
        esc(&obligado_nombre.chars().take(120).collect::<String>()),
        esc(obligado_nif),
        if incidencia {
            "<sf:RemisionVoluntaria><sf:Incidencia>S</sf:Incidencia></sf:RemisionVoluntaria>"
        } else {
            ""
        }
    );
    for r in records {
        let _ = write!(x, "<sfLR:RegistroFactura>{}</sfLR:RegistroFactura>", r.xml);
    }
    x.push_str("</sfLR:RegFactuSistemaFacturacion>");
    Ok(x)
}

/// Sobre SOAP 1.1 (estilo document/literal, `soapAction=""`).
pub fn soap_envelope(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><soapenv:Envelope xmlns:soapenv=\"{NS_SOAP}\">\
         <soapenv:Header/><soapenv:Body>{body}</soapenv:Body></soapenv:Envelope>"
    )
}

/// Entorno del servicio de cotejo del QR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QrEnv {
    Pruebas,
    Produccion,
}

/// Codificación de `java.net.URLEncoder.encode(s, "UTF-8")`, como en la especificación del QR.
fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'*' | b'_' => {
                out.push(char::from(*b))
            }
            // TODO(VERIFY): espacio como '+' (URLEncoder); no hay ejemplo oficial.
            b' ' => out.push('+'),
            b => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// URL del QR de una factura VERI*FACTU ([QR] §5–6). El importe va con dos decimales.
/// `TODO(VERIFY)`: los ejemplos oficiales usan `241.4` y la plantilla `NNN.DD`; ambos se
/// aceptan según [QR], pero no hay regla explícita.
pub fn qr_url(
    env: QrEnv,
    nif: &str,
    num_serie: &str,
    fecha_expedicion: Date,
    importe_total: Money,
) -> String {
    qr_url_raw(
        env,
        nif,
        num_serie,
        &fecha(fecha_expedicion),
        &amount(importe_total),
    )
}

/// Igual que `qr_url` con los parámetros ya formateados.
pub fn qr_url_raw(env: QrEnv, nif: &str, num_serie: &str, fecha: &str, importe: &str) -> String {
    let base = match env {
        QrEnv::Pruebas => "https://prewww2.aeat.es/wlpl/TIKE-CONT/ValidarQR",
        QrEnv::Produccion => "https://www2.agenciatributaria.gob.es/wlpl/TIKE-CONT/ValidarQR",
    };
    format!(
        "{base}?nif={}&numserie={}&fecha={}&importe={}",
        url_encode(nif),
        url_encode(num_serie),
        url_encode(fecha),
        url_encode(importe)
    )
}

/// Leyenda obligatoria bajo el QR en modo VERI*FACTU.
pub const LEGEND: &str = "Factura verificable en la sede electrónica de la AEAT";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseLine {
    pub num_serie: String,
    pub operacion: String,
    pub estado: String,
    pub codigo_error: Option<String>,
    pub descripcion_error: Option<String>,
    /// `RegistroDuplicado/EstadoRegistroDuplicado` (`Correcta`, `AceptadaConErrores` o
    /// `Anulada`): la AEAT rechaza la línea porque ya tiene ese registro, y dice en qué estado
    /// lo tiene ([SW] 6.5.2). Pasa al reenviar tras un corte sin respuesta.
    pub duplicado: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub csv: Option<String>,
    /// `Correcto`, `ParcialmenteCorrecto` o `Incorrecto`.
    pub estado_envio: String,
    /// Segundos que hay que esperar antes del siguiente envío.
    pub tiempo_espera_envio: u32,
    pub lineas: Vec<ResponseLine>,
}

/// Lee la respuesta `RespuestaRegFactuSistemaFacturacion` (o un SOAP Fault).
pub fn parse_response(xml: &str) -> Result<Response, VerifactuError> {
    let mut reader = Reader::from_str(xml);
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut resp = Response {
        csv: None,
        estado_envio: String::new(),
        tiempo_espera_envio: 60,
        lineas: Vec::new(),
    };
    let mut line: Option<ResponseLine> = None;
    let mut fault: Option<String> = None;
    let mut fault_code: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.local_name().as_ref().to_owned();
                if name == "RespuestaLinea" {
                    line = Some(ResponseLine {
                        num_serie: String::new(),
                        operacion: String::new(),
                        estado: String::new(),
                        codigo_error: None,
                        descripcion_error: None,
                        duplicado: None,
                    });
                }
                path.push(name);
                text.clear();
            }
            Ok(Event::Text(t)) => text.push_str(&t.xml10_content()),
            Ok(Event::GeneralRef(r)) => match r.as_ref() {
                "amp" => text.push('&'),
                "lt" => text.push('<'),
                "gt" => text.push('>'),
                "quot" => text.push('"'),
                "apos" => text.push('\''),
                _ => {}
            },
            Ok(Event::End(e)) => {
                let name = e.local_name().as_ref().to_owned();
                let value = text.trim().to_owned();
                text.clear();
                path.pop();
                let in_line = path.iter().any(|p| p == "RespuestaLinea");
                if name == "RespuestaLinea" {
                    if let Some(l) = line.take() {
                        resp.lineas.push(l);
                    }
                } else if in_line {
                    if let Some(l) = line.as_mut() {
                        match name.as_str() {
                            "NumSerieFactura" => l.num_serie = value,
                            "TipoOperacion" => l.operacion = value,
                            "EstadoRegistro" => l.estado = value,
                            "CodigoErrorRegistro" => l.codigo_error = Some(value),
                            "DescripcionErrorRegistro" => l.descripcion_error = Some(value),
                            "EstadoRegistroDuplicado" => l.duplicado = Some(value),
                            _ => {}
                        }
                    }
                } else {
                    match name.as_str() {
                        "CSV" => resp.csv = Some(value),
                        "EstadoEnvio" => resp.estado_envio = value,
                        "TiempoEsperaEnvio" => {
                            resp.tiempo_espera_envio = value.parse().unwrap_or(60)
                        }
                        "faultstring" => fault = Some(value),
                        "faultcode" => fault_code = Some(value),
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(VerifactuError::InvalidResponse(e.to_string())),
            _ => {}
        }
    }
    if let Some(f) = fault {
        // Con el `faultcode`: `…:Server` se reenvía tal cual; `…:Client` hay que corregirlo
        // antes ([SW] 5.1). Ver [`fault_is_client`].
        return Err(VerifactuError::InvalidResponse(match fault_code {
            Some(c) => format!("SOAP Fault ({c}): {f}"),
            None => format!("SOAP Fault: {f}"),
        }));
    }
    if resp.estado_envio.is_empty() {
        return Err(VerifactuError::InvalidResponse("falta EstadoEnvio".into()));
    }
    Ok(resp)
}

/// Si el mensaje de error de [`parse_response`] es un SOAP Fault de tipo `Client`: el envío
/// tiene un error (de esquema o de cabecera) que hay que corregir antes de reenviar, así que no
/// se reintenta solo ([SW] 5.1, pág. 28). Cualquier otro fallo (`Server`, sin respuesta, sin
/// red) es una incidencia técnica y se reintenta.
#[must_use]
pub fn fault_is_client(message: &str) -> bool {
    message.contains("SOAP Fault (")
        && message
            .split_once("SOAP Fault (")
            .and_then(|(_, rest)| rest.split_once(')'))
            .is_some_and(|(code, _)| code.rsplit(':').next() == Some("Client"))
}

// ---------- Consulta de registros (ConsultaFactuSistemaFacturacion) ----------

/// Cuerpo `sfLRC:ConsultaFactuSistemaFacturacion`: los registros que tiene la AEAT del
/// obligado en un mes (`periodo` = `01`…`12`), opcionalmente de una factura, desde la clave de
/// paginación de la respuesta anterior.
pub fn consulta(
    obligado_nombre: &str,
    obligado_nif: &str,
    ejercicio: i32,
    periodo: u8,
    num_serie: Option<&str>,
    paginacion: Option<&ConsultaRegistro>,
) -> String {
    let mut x = String::new();
    let _ = write!(
        x,
        "<sfLRC:ConsultaFactuSistemaFacturacion xmlns:sfLRC=\"{NS_SFLRC}\" xmlns:sf=\"{NS_SF}\">\
         <sfLRC:Cabecera><sf:IDVersion>1.0</sf:IDVersion><sf:ObligadoEmision>\
         <sf:NombreRazon>{}</sf:NombreRazon><sf:NIF>{}</sf:NIF></sf:ObligadoEmision>\
         </sfLRC:Cabecera><sfLRC:FiltroConsulta><sfLRC:PeriodoImputacion><sf:Ejercicio>{ejercicio:04}\
         </sf:Ejercicio><sf:Periodo>{periodo:02}</sf:Periodo></sfLRC:PeriodoImputacion>",
        esc(&obligado_nombre.chars().take(120).collect::<String>()),
        esc(obligado_nif)
    );
    if let Some(n) = num_serie {
        let _ = write!(
            x,
            "<sfLRC:NumSerieFactura>{}</sfLRC:NumSerieFactura>",
            esc(n)
        );
    }
    if let Some(p) = paginacion {
        let _ = write!(
            x,
            "<sfLRC:ClavePaginacion><sf:IDEmisorFactura>{}</sf:IDEmisorFactura>\
             <sf:NumSerieFactura>{}</sf:NumSerieFactura><sf:FechaExpedicionFactura>{}\
             </sf:FechaExpedicionFactura></sfLRC:ClavePaginacion>",
            esc(obligado_nif),
            esc(&p.num_serie),
            esc(&p.fecha)
        );
    }
    x.push_str(
        "</sfLRC:FiltroConsulta><sfLRC:DatosAdicionalesRespuesta>\
         <sfLRC:MostrarSistemaInformatico>S</sfLRC:MostrarSistemaInformatico>\
         </sfLRC:DatosAdicionalesRespuesta></sfLRC:ConsultaFactuSistemaFacturacion>",
    );
    x
}

/// Un registro tal como lo tiene la AEAT.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsultaRegistro {
    pub num_serie: String,
    /// `dd-mm-aaaa`.
    pub fecha: String,
    pub tipo_factura: String,
    pub importe_total: Option<String>,
    pub huella: Option<String>,
    /// `Correcto`, `AceptadoConErrores` o `Anulado`.
    pub estado: String,
    pub codigo_error: Option<String>,
    pub descripcion_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsultaRespuesta {
    /// `ConDatos` o `SinDatos`.
    pub resultado: String,
    /// Hay más páginas (se piden desde el último registro).
    pub mas: bool,
    pub registros: Vec<ConsultaRegistro>,
}

/// Lee `RespuestaConsultaFactuSistemaFacturacion` (o un SOAP Fault).
pub fn parse_consulta(xml: &str) -> Result<ConsultaRespuesta, VerifactuError> {
    let mut reader = Reader::from_str(xml);
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut out = ConsultaRespuesta {
        resultado: String::new(),
        mas: false,
        registros: Vec::new(),
    };
    let mut reg: Option<ConsultaRegistro> = None;
    let mut fault: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.local_name().as_ref().to_owned();
                if name == "RegistroRespuestaConsultaFactuSistemaFacturacion" {
                    reg = Some(ConsultaRegistro {
                        num_serie: String::new(),
                        fecha: String::new(),
                        tipo_factura: String::new(),
                        importe_total: None,
                        huella: None,
                        estado: String::new(),
                        codigo_error: None,
                        descripcion_error: None,
                    });
                }
                path.push(name);
                text.clear();
            }
            Ok(Event::Text(t)) => text.push_str(&t.xml10_content()),
            Ok(Event::GeneralRef(r)) => match r.as_ref() {
                "amp" => text.push('&'),
                "lt" => text.push('<'),
                "gt" => text.push('>'),
                "quot" => text.push('"'),
                "apos" => text.push('\''),
                _ => {}
            },
            Ok(Event::End(e)) => {
                let name = e.local_name().as_ref().to_owned();
                let value = text.trim().to_owned();
                text.clear();
                path.pop();
                let parent = path.last().map(String::as_str).unwrap_or("");
                if name == "RegistroRespuestaConsultaFactuSistemaFacturacion" {
                    if let Some(r) = reg.take() {
                        out.registros.push(r);
                    }
                } else if let Some(r) = reg.as_mut() {
                    // Dentro de un registro: sólo los datos de la propia factura (no los de las
                    // rectificadas ni el bloque del sistema informático).
                    let in_id = parent == "IDFactura";
                    let in_estado = parent == "EstadoRegistro";
                    match name.as_str() {
                        "NumSerieFactura" if in_id => r.num_serie = value,
                        "FechaExpedicionFactura" if in_id => r.fecha = value,
                        "TipoFactura" => r.tipo_factura = value,
                        "ImporteTotal" if parent == "DatosRegistroFacturacion" => {
                            r.importe_total = Some(value)
                        }
                        "Huella" if parent == "DatosRegistroFacturacion" => r.huella = Some(value),
                        "EstadoRegistro" if in_estado => r.estado = value,
                        "CodigoErrorRegistro" if in_estado => r.codigo_error = Some(value),
                        "DescripcionErrorRegistro" if in_estado => {
                            r.descripcion_error = Some(value)
                        }
                        _ => {}
                    }
                } else {
                    match name.as_str() {
                        "ResultadoConsulta" => out.resultado = value,
                        "IndicadorPaginacion" => out.mas = value == "S",
                        "faultstring" => fault = Some(value),
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(VerifactuError::InvalidResponse(e.to_string())),
            _ => {}
        }
    }
    if let Some(f) = fault {
        return Err(VerifactuError::InvalidResponse(format!("SOAP Fault: {f}")));
    }
    if out.resultado.is_empty() {
        return Err(VerifactuError::InvalidResponse(
            "falta ResultadoConsulta".into(),
        ));
    }
    Ok(out)
}

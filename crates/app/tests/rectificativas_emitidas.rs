//! Rectificativas emitidas (G29): motivo R1–R5, sustitución con su efecto en VERI*FACTU, el 303,
//! el 349, el 347, la cartera y la contabilidad, rectificativas agrupadas y control de la
//! numeración; y créditos incobrables (G31): el procedimiento del art. 80.Cuatro LIVA hasta la
//! R3, su asiento sólo del IVA y el deterioro sólo de la base. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::{ajustes_cierre, cartera, incobrables, models, rectificativas_emitidas};
use taxpain_core::invoice::{
    InvoiceKind, InvoiceLine, Party, PartyId, Rectification, RectificationMethod, RoundingMode,
    VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::date;
use time::{Date, OffsetDateTime, Time};

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn at(d: Date) -> OffsetDateTime {
    d.with_time(Time::from_hms(10, 0, 0).unwrap()).assume_utc()
}

fn addr() -> Option<taxpain_core::invoice::Address> {
    Some(taxpain_core::invoice::Address {
        street: "Calle Inventada 1".into(),
        post_code: "46001".into(),
        town: "València".into(),
        province: "Valencia".into(),
        country: "ES".into(),
    })
}

fn setup(verifactu: bool) -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Estudio Demo SL".into(),
            address: addr(),
            person: None,
        },
        default_series: "A".into(),
        rectifying_series: "R".into(),
        simplified_series: "S".into(),
        simplified_rectifying_series: "RS".into(),
        language: Language::Es,
        default_vat: Rate::percent(21),
        default_withholding: None,
        outputs: Outputs {
            pdf: false,
            facturae: false,
            ubl: false,
        },
        destination: None,
        rounding: RoundingMode::PerRate,
        verifactu: VerifactuSettings {
            enabled: verifactu,
            installation_id: "prueba".into(),
            production: false,
        },
        footer: None,
        activity: None,
        simplified_up_to_3000: false,
    };
    invoicing::save_settings(&v, &s, at(date!(2025 - 01 - 01))).unwrap();
    v
}

fn client(nif: &str, name: &str) -> Party {
    Party {
        id: PartyId::Nif(nif.into()),
        name: name.into(),
        address: addr(),
        person: None,
    }
}

fn line(euros: i64) -> InvoiceLine {
    InvoiceLine {
        description: "Diseño".into(),
        quantity_milli: 1000,
        unit_price: eur(euros),
        discount: Rate::ZERO,
        vat: VatTreatment::Taxed {
            rate: Rate::percent(21),
        },
        surcharge: None,
        withholding: None,
        advance: false,
    }
}

fn draft(kind: InvoiceKind, d: Date, recipient: Option<Party>, euros: i64) -> InvoiceDraft {
    InvoiceDraft {
        kind,
        series: None,
        issue_date: d,
        operation_date: None,
        recipient,
        lines: vec![line(euros)],
        rectified: vec![],
        rectified_amounts: None,
        description: "Servicios".into(),
        notes: None,
        currency: None,
        oss: None,
    }
}

fn sale(v: &Vault, euros: i64, d: Date, nif: &str) -> (i64, String) {
    let i = issue(
        v,
        &draft(
            InvoiceKind::Completa,
            d,
            Some(client(nif, "Cliente Inventado SA")),
            euros,
        ),
        at(d),
        true,
    )
    .unwrap();
    (i.id, i.number)
}

fn rect(code: u8, method: RectificationMethod) -> InvoiceKind {
    InvoiceKind::Rectificativa(Rectification { code, method })
}

/// Líneas del asiento de una factura emitida: (cuenta, debe, haber).
fn entry(v: &Vault, id: i64) -> Vec<(String, i64, i64)> {
    let mut stmt = v
        .connection()
        .prepare(
            "SELECT l.account, l.debit, l.credit FROM journal_line l
             JOIN journal_entry e ON e.id = l.entry_id
             WHERE e.origin_kind = 'FacturaEmitida' AND e.origin_id = ?1 ORDER BY l.account",
        )
        .unwrap();
    stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn line_of(e: &[(String, i64, i64)], prefix: &str) -> (i64, i64) {
    e.iter()
        .filter(|l| l.0.starts_with(prefix))
        .fold((0, 0), |a, l| (a.0 + l.1, a.1 + l.2))
}

#[test]
fn sustitucion_lleva_lo_sustituido_y_solo_cuenta_la_diferencia() {
    let v = setup(true);
    let d0 = date!(2026 - 04 - 10);
    let (orig, num) = sale(&v, 1_000, d0, "A22222228");
    // La factura correcta era de 800 €: por sustitución, con el importe completo.
    let d1 = date!(2026 - 05 - 05);
    let mut r = draft(
        rect(1, RectificationMethod::Sustitucion),
        d1,
        Some(client("A22222228", "Cliente Inventado SA")),
        800,
    );
    r.rectified = vec![(num.clone(), d0)];
    let out = issue(&v, &r, at(d1), true).unwrap();
    assert!(out.number.starts_with('R'), "serie de rectificativas");

    // VERI*FACTU: R1, TipoRectificativa S e ImporteRectificacion con lo sustituido.
    let xml: String = v
        .connection()
        .query_row(
            "SELECT xml FROM verifactu_record WHERE invoice_id = ?1",
            [out.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(xml.contains("<sf:TipoFactura>R1</sf:TipoFactura>"));
    assert!(xml.contains("<sf:TipoRectificativa>S</sf:TipoRectificativa>"));
    assert!(xml.contains(
        "<sf:ImporteRectificacion><sf:BaseRectificada>1000.00</sf:BaseRectificada>\
         <sf:CuotaRectificada>210.00</sf:CuotaRectificada></sf:ImporteRectificacion>"
    ));
    assert!(!xml.contains("CuotaRecargoRectificado"));

    // Contabilidad: sólo la diferencia (−200 de base, −42 de IVA).
    let e = entry(&v, out.id);
    assert_eq!(line_of(&e, "430"), (0, 24_200));
    assert_eq!(line_of(&e, "705"), (20_000, 0));
    assert_eq!(line_of(&e, "477"), (4_200, 0));
    assert!(entry(&v, orig).iter().any(|l| l.0.starts_with("705")));

    // 303 del 2T: 800 de base y 168 de IVA en total.
    let m = models::draft(&v, "303", 2026, 2).unwrap();
    assert_eq!(m.result, eur(168));

    // Cartera: la rectificativa es un abono de 242 (lo que cambia el saldo del cliente).
    let items = cartera::list(&v, d1).unwrap();
    let it = items.iter().find(|i| i.invoice_id == out.id).unwrap();
    assert_eq!(it.amount, eur(-242));

    // Lo sustituido queda guardado para la subsanación y los totales por SQL.
    let s = rectificativas_emitidas::numeracion(&v, 2026).unwrap();
    assert!(s.iter().all(|x| x.correcta()), "{s:?}");
    let replaced: i64 = v
        .connection()
        .query_row(
            "SELECT replaced_base FROM invoice_issued WHERE id = ?1",
            [out.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(replaced, 100_000);

    // No se sustituye dos veces la misma factura.
    let e = issue(&v, &r, at(d1), true).unwrap_err().to_string();
    assert!(e.contains("ya está sustituida"), "{e}");
    // Ni se pueden indicar otros importes rectificados que los de la factura.
    let mut otro = r.clone();
    otro.rectified_amounts = Some((eur(900), eur(189)));
    let (_, n2) = sale(&v, 500, d1, "A22222228");
    otro.rectified = vec![(n2, d1)];
    let e = issue(&v, &otro, at(d1), true).unwrap_err().to_string();
    assert!(e.contains("no son los de las facturas sustituidas"), "{e}");
}

#[test]
fn sustitucion_de_una_entrega_intracomunitaria_en_el_349() {
    let v = setup(false);
    let fr = Party {
        id: PartyId::Foreign {
            country: "FR".into(),
            id_type: taxpain_core::invoice::IdType::NifIva,
            id: "FR12345678901".into(),
        },
        name: "Client Inventé SARL".into(),
        address: Some(taxpain_core::invoice::Address {
            street: "Rue Inventée 1".into(),
            post_code: "75001".into(),
            town: "Paris".into(),
            province: "Paris".into(),
            country: "FR".into(),
        }),
        person: None,
    };
    let exempt = |mut l: InvoiceLine| {
        l.vat = VatTreatment::Exempt(taxpain_core::invoice::Exemption::E5);
        l
    };
    let d0 = date!(2026 - 07 - 10);
    let mut a = draft(InvoiceKind::Completa, d0, Some(fr.clone()), 2_000);
    a.lines = vec![exempt(line(2_000))];
    let a = issue(&v, &a, at(d0), true).unwrap();
    let d1 = date!(2026 - 08 - 10);
    let mut r = draft(
        rect(1, RectificationMethod::Sustitucion),
        d1,
        Some(fr),
        1_500,
    );
    r.lines = vec![exempt(line(1_500))];
    r.rectified = vec![(a.number, d0)];
    issue(&v, &r, at(d1), true).unwrap();
    let m = models::draft(&v, "349", 2026, 3).unwrap();
    assert!(
        !m.warnings.iter().any(|w| w.contains("sustitución")),
        "{:?}",
        m.warnings
    );
    // Una sola entrega de 1.500 € (2.000 − 500), no 3.500.
    let total: Money = m.details.iter().map(|d| d.1).sum();
    assert_eq!(total, eur(1_500), "{:?}", m.details);
}

#[test]
fn agrupadas_del_mismo_cliente_y_claves() {
    let v = setup(false);
    let d0 = date!(2026 - 03 - 02);
    let (a1, n1) = sale(&v, 300, d0, "A22222228");
    let (a2, n2) = sale(&v, 200, d0, "A22222228");
    let (_, n3) = sale(&v, 100, d0, "B12345674");
    let d1 = date!(2026 - 03 - 20);
    let c = client("A22222228", "Cliente Inventado SA");
    // Un descuento por volumen sobre las dos (R1, por diferencias).
    let mut r = draft(
        rect(1, RectificationMethod::Diferencias),
        d1,
        Some(c.clone()),
        -50,
    );
    r.rectified = vec![(n1.clone(), d0), (n2.clone(), d0)];
    let out = issue(&v, &r, at(d1), true).unwrap();
    let ids: Vec<i64> = {
        let mut st = v
            .connection()
            .prepare(
                "SELECT rectified_id FROM invoice_rectified WHERE rectifying_id = ?1 ORDER BY 1",
            )
            .unwrap();
        st.query_map([out.id], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(ids, vec![a1, a2]);
    // De otro cliente no se agrupa.
    r.rectified = vec![(n1.clone(), d0), (n3, d0)];
    let e = issue(&v, &r, at(d1), true).unwrap_err().to_string();
    assert!(e.contains("otro cliente"), "{e}");
    // R5 sólo para simplificadas.
    let mut r5 = draft(rect(5, RectificationMethod::Diferencias), d1, None, -10);
    r5.rectified = vec![(n1.clone(), d0)];
    let e = issue(&v, &r5, at(d1), true).unwrap_err().to_string();
    assert!(e.contains("R5"), "{e}");
    // La fecha de la rectificada ha de ser la suya.
    let mut mal = draft(
        rect(4, RectificationMethod::Diferencias),
        d1,
        Some(c.clone()),
        -10,
    );
    mal.rectified = vec![(n1.clone(), date!(2026 - 03 - 03))];
    let e = issue(&v, &mal, at(d1), true).unwrap_err().to_string();
    assert!(e.contains("es del 2026-03-02"), "{e}");
    // Una R3 no se emite desde Facturas: hace falta el seguimiento del crédito incobrable.
    let mut r3 = draft(
        rect(3, RectificationMethod::Diferencias),
        d1,
        Some(c.clone()),
        -300,
    );
    r3.rectified = vec![(n1.clone(), d0)];
    let e = issue(&v, &r3, at(d1), true).unwrap_err().to_string();
    assert!(e.contains("Créditos incobrables"), "{e}");
    // Ni una R2 por sustitución.
    let mut r2 = draft(rect(2, RectificationMethod::Sustitucion), d1, Some(c), 0);
    r2.rectified = vec![(n1, d0)];
    assert!(issue(&v, &r2, at(d1), true).is_err());
}

#[test]
fn control_de_numeracion_detecta_huecos_y_fechas_fuera_de_orden() {
    let v = setup(false);
    sale(&v, 100, date!(2026 - 01 - 10), "A22222228");
    sale(&v, 100, date!(2026 - 01 - 11), "A22222228");
    let (id3, _) = sale(&v, 100, date!(2026 - 01 - 12), "A22222228");
    let s = rectificativas_emitidas::numeracion(&v, 2026).unwrap();
    assert_eq!(s.len(), 1);
    assert!(s[0].correcta());
    assert_eq!(s[0].emitidas, 3);
    // Datos dañados a propósito: la tercera con el número 5 y con fecha anterior.
    v.connection()
        .execute(
            "UPDATE invoice_issued SET seq = 5, issue_date = '2026-01-05' WHERE id = ?1",
            [id3],
        )
        .unwrap();
    let s = rectificativas_emitidas::numeracion(&v, 2026).unwrap();
    assert_eq!(s[0].huecos, vec![3, 4]);
    assert_eq!(s[0].fuera_de_orden.len(), 1);
}

#[test]
fn credito_incobrable_de_la_reclamacion_a_la_r3_y_su_comunicacion() {
    let v = setup(false);
    let d0 = date!(2025 - 01 - 10);
    let (orig, num) = sale(&v, 1_000, d0, "A22222228");
    // Abierto con el plazo de seis meses (volumen del año anterior: cero).
    let id = incobrables::abrir(
        &v,
        orig,
        None,
        incobrables::Exclusion::Ninguna,
        at(date!(2025 - 06 - 01)),
        true,
    )
    .unwrap();
    let c = incobrables::list(&v, date!(2025 - 06 - 01)).unwrap();
    let c = c.iter().find(|c| c.id == Some(id)).unwrap();
    assert_eq!(c.plazo_meses, 6);
    assert_eq!(c.calendario.desde, date!(2025 - 07 - 10));
    assert_eq!(c.calendario.limite_rectificar, date!(2026 - 01 - 10));
    assert_eq!(c.iva_pendiente, eur(210));
    assert!(!c.puede_rectificar);

    // Sin reclamación no hay R3.
    let e = incobrables::emitir_r3(
        &v,
        id,
        date!(2025 - 08 - 01),
        at(date!(2025 - 08 - 01)),
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("Falta reclamar"), "{e}");
    incobrables::reclamar(
        &v,
        id,
        date!(2025 - 07 - 20),
        incobrables::Medio::Fehaciente,
        "Burofax 0001",
        at(date!(2025 - 07 - 20)),
        true,
    )
    .unwrap();
    let p = incobrables::propuesta(&v, id, date!(2025 - 08 - 01)).unwrap();
    assert!(p.puede, "{:?}", p.requisitos);
    assert_eq!((p.base, p.iva), (eur(-1_000), eur(-210)));

    let r3 = incobrables::emitir_r3(
        &v,
        id,
        date!(2025 - 08 - 01),
        at(date!(2025 - 08 - 01)),
        true,
    )
    .unwrap();
    let kind: String = v
        .connection()
        .query_row(
            "SELECT kind FROM invoice_issued WHERE id = ?1",
            [r3.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kind, "R3");
    // Asiento sólo del IVA: 477 al debe y cliente al haber; la venta no se toca.
    let e = entry(&v, r3.id);
    assert_eq!(line_of(&e, "477"), (21_000, 0));
    assert_eq!(line_of(&e, "430"), (0, 21_000));
    assert_eq!(line_of(&e, "7"), (0, 0));
    // En la cartera, un abono por la cuota; el cliente sigue debiendo la base.
    let items = cartera::list(&v, date!(2025 - 08 - 01)).unwrap();
    assert_eq!(
        items.iter().find(|i| i.invoice_id == r3.id).unwrap().amount,
        eur(-210)
    );
    // 303 del 3T: la cuota que se recupera (rectificativas [14]/[15]).
    let m = models::draft(&v, "303", 2025, 3).unwrap();
    assert_eq!(m.result, eur(-210));
    // Al cierre, el deterioro propuesto es sólo por la base.
    let r = ajustes_cierre::resumen(&v, 2025, false).unwrap();
    let d = r
        .deteriorables
        .iter()
        .find(|d| d.factura_id == orig)
        .unwrap();
    assert_eq!(d.importe, eur(1_000));

    // Una segunda R3 de la misma factura no se puede.
    assert!(
        incobrables::emitir_r3(
            &v,
            id,
            date!(2025 - 08 - 02),
            at(date!(2025 - 08 - 02)),
            true
        )
        .is_err()
    );
    // Comunicación: datos para la sede y plazo de un mes.
    let com = incobrables::comunicacion(&v, id).unwrap();
    assert_eq!(com.plazo, Some(date!(2025 - 09 - 01)));
    assert!(com.campos.iter().any(|(_, v)| v == &num));
    incobrables::comunicada(
        &v,
        id,
        date!(2025 - 08 - 20),
        "CSV-PRUEBA",
        at(date!(2025 - 08 - 20)),
        true,
    )
    .unwrap();
    let c = incobrables::list(&v, date!(2025 - 08 - 20)).unwrap();
    assert_eq!(
        c.iter().find(|c| c.id == Some(id)).unwrap().estado,
        "comunicado"
    );
    // Cobrar después no cambia la base (cliente empresario): no se cierra como «cobrado».
    assert!(
        incobrables::cerrar(
            &v,
            id,
            "cobrado",
            date!(2025 - 10 - 01),
            "",
            at(date!(2025 - 10 - 01)),
            true
        )
        .is_err()
    );
}

#[test]
fn incobrable_fuera_de_plazo_o_excluido() {
    let v = setup(false);
    let d0 = date!(2024 - 01 - 10);
    let (orig, _) = sale(&v, 1_000, d0, "A22222228");
    let id = incobrables::abrir(
        &v,
        orig,
        Some(12),
        incobrables::Exclusion::Afianzado,
        at(date!(2025 - 03 - 01)),
        true,
    )
    .unwrap();
    incobrables::reclamar(
        &v,
        id,
        date!(2025 - 02 - 01),
        incobrables::Medio::Notarial,
        "",
        at(date!(2025 - 03 - 01)),
        true,
    )
    .unwrap();
    let p = incobrables::propuesta(&v, id, date!(2025 - 03 - 01)).unwrap();
    assert!(!p.puede);
    assert!(
        p.requisitos
            .iter()
            .any(|r| r.clave == "exclusion" && !r.cumplido)
    );
    // Un año desde el devengo más seis meses: el 10/7/2025 se acaba el plazo.
    incobrables::ajustar(
        &v,
        id,
        12,
        incobrables::Exclusion::Ninguna,
        at(date!(2025 - 03 - 01)),
        true,
    )
    .unwrap();
    assert!(
        incobrables::propuesta(&v, id, date!(2025 - 07 - 10))
            .unwrap()
            .puede
    );
    let tarde = incobrables::propuesta(&v, id, date!(2025 - 07 - 11)).unwrap();
    assert!(!tarde.puede);
    assert!(
        tarde
            .requisitos
            .iter()
            .any(|r| r.clave == "a_tiempo" && !r.cumplido)
    );
}

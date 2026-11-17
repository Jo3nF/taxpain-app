//! Modelos 216 y 296 (rentas a no residentes, certificados de residencia) y datos del inmueble
//! arrendado en el 180, su certificado y el 115. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use taxpain_app::certificados::{self, Detalle, Opciones};
use taxpain_app::inmuebles::{self, Inmueble};
use taxpain_app::no_residentes::{self, Certificado, NuevaRenta};
use taxpain_app::received::{Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::socios::{self, PayoutInput, Shareholder};
use taxpain_app::{accounting, invoicing, models};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Address, Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_models::irnr::{Exencion, Naturaleza, Regimen};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime, Time};

const AHORA: OffsetDateTime = datetime!(2026-11-17 10:00 UTC);

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn at(d: Date) -> OffsetDateTime {
    d.with_time(Time::from_hms(10, 0, 0).unwrap()).assume_utc()
}

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Estudio Demo SL".into(),
            address: Some(Address {
                street: "Calle Inventada 1".into(),
                post_code: "46001".into(),
                town: "València".into(),
                province: "Valencia".into(),
                country: "ES".into(),
            }),
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
            enabled: false,
            installation_id: "x".into(),
            production: false,
        },
        footer: None,
        activity: None,
        simplified_up_to_3000: false,
    };
    invoicing::save_settings(&v, &s, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

fn receive(
    v: &Vault,
    nif: &str,
    number: &str,
    (base, vat, wh): (i64, i64, i64),
    d: Date,
    nature: Nature,
    country: Option<&str>,
) {
    let rec = ingest_file(
        v,
        &format!("{number}.xml"),
        format!("<?xml version=\"1.0\"?><x n=\"{number}\"/>").as_bytes(),
        Source::Manual,
        at(d),
    )
    .unwrap();
    review(
        v,
        rec.id,
        &Reviewed {
            supplier_nif: nif.into(),
            supplier_name: format!("Proveedor {nif}"),
            number: number.into(),
            issue_date: d,
            base: eur(base),
            vat: eur(vat),
            withholding: eur(wh),
            total: eur(base + vat - wh),
            deductible_pct: 100,
            capital_good: false,
            nature,
            asset_class: None,
            vat_regime: Default::default(),
            supplier_country: country.map(str::to_owned),
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        at(d),
    )
    .unwrap();
}

fn opts() -> Opciones {
    Opciones {
        place: "València".into(),
        date: date!(2027 - 01 - 20),
        signer: String::new(),
        signer_role: String::new(),
        nifs: vec![],
        properties: BTreeMap::new(),
    }
}

fn oficina() -> Inmueble {
    Inmueble {
        nif: "A22222228".into(),
        situacion: 1,
        ref_catastral: "9872023VH5797S0001WX".into(),
        via: "Calle Ejemplo".into(),
        numero: "10".into(),
        municipio: "València".into(),
        provincia: "46".into(),
        codigo_postal: "46001".into(),
        exonerado_hasta: None,
    }
}

#[test]
fn el_180_genera_el_fichero_con_el_inmueble_guardado() {
    let v = setup();
    for (n, d) in [
        ("ALQ-1", date!(2026 - 02 - 01)),
        ("ALQ-2", date!(2026 - 03 - 01)),
    ] {
        receive(
            &v,
            "A22222228",
            n,
            (1_000, 210, 190),
            d,
            Nature::Alquiler,
            None,
        );
    }
    let d = models::draft(&v, "180", 2026, 0).unwrap();
    assert!(d.file.is_empty());
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("Inmuebles arrendados"))
    );
    let p = certificados::prepare(&v, "180", 2026).unwrap();
    assert!(p.notes.iter().any(|n| n.contains("Sin inmueble guardado")));

    inmuebles::save(&v, &oficina(), AHORA).unwrap();
    let d = models::draft(&v, "180", 2026, 0).unwrap();
    assert!(!d.file.is_empty(), "{:?}", d.warnings);
    assert!(d.file.contains("9872023VH5797S0001WX"));
    assert!(d.file.contains("CALLE EJEMPLO"));
    assert!(
        !d.warnings
            .iter()
            .any(|w| w.contains("faltan la referencia"))
    );
    let p = certificados::prepare(&v, "180", 2026).unwrap();
    assert!(p.notes.is_empty());

    // El certificado lleva el inmueble guardado; lo tecleado al generar manda.
    let c = certificados::certificates(&v, "180", 2026, &opts()).unwrap();
    assert!(matches!(&c[0].detalles[0],
        Detalle::Arrendamiento { inmueble, .. } if inmueble.contains("9872023VH5797S0001WX")));
    let mut o = opts();
    o.properties
        .insert("A22222228".into(), "Otra descripción".into());
    let c = certificados::certificates(&v, "180", 2026, &o).unwrap();
    assert!(matches!(&c[0].detalles[0],
        Detalle::Arrendamiento { inmueble, .. } if inmueble == "Otra descripción"));
}

#[test]
fn el_115_avisa_del_alquiler_sin_retencion_sin_acreditacion() {
    let v = setup();
    receive(
        &v,
        "A22222228",
        "ALQ-1",
        (1_000, 210, 0),
        date!(2026 - 02 - 01),
        Nature::Alquiler,
        None,
    );
    let aviso = |v: &Vault| {
        models::draft(v, "115", 2026, 1)
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.contains("75.3.g"))
    };
    assert!(aviso(&v));
    let mut i = oficina();
    i.exonerado_hasta = Some(date!(2026 - 12 - 31));
    inmuebles::save(&v, &i, AHORA).unwrap();
    assert!(!aviso(&v));
}

fn renta(nif: &str, pais: &str, n: Naturaleza, bruto: i64, d: Date) -> NuevaRenta {
    NuevaRenta {
        fecha: d,
        nif: nif.into(),
        nombre: format!("Perceptor {nif}"),
        pais: pais.into(),
        naturaleza: n,
        bruto: eur(bruto),
        tipo_convenio: None,
        exencion: None,
        concepto: String::new(),
        recibida_id: None,
    }
}

fn certificado(nif: &str, pais: &str, expedido: Date) -> Certificado {
    Certificado {
        id: 0,
        nif: nif.into(),
        nombre: format!("Perceptor {nif}"),
        pais: pais.into(),
        expedido,
        valido_hasta: None,
        convenio: true,
        documento: "certificado.pdf".into(),
        notas: String::new(),
    }
}

#[test]
fn el_216_y_el_296_con_convenio_exencion_y_dividendos() {
    let v = setup();
    // Socio no residente (EE. UU.) con el 100 %: dividendo al 19 % que viene de Socios.
    socios::save_shareholder(
        &v,
        &Shareholder {
            id: 0,
            nif: "US-123456789".into(),
            name: "Holding Ejemplo Inc".into(),
            address: "1 Example Street, Wilmington".into(),
            province: String::new(),
            country: "US".into(),
            pct: 1_000_000,
            legal_person: true,
            resident: false,
            exempt: false,
            active: true,
        },
        AHORA,
    )
    .unwrap();
    accounting::manual_entry(
        &v,
        &JournalEntry {
            date: date!(2026 - 04 - 30),
            concept: "Reparto acordado por la junta".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("113"), eur(1_000)),
                EntryLine::credit(acc("526"), eur(1_000)),
            ],
        },
        AHORA,
    )
    .unwrap();
    socios::pay(
        &v,
        &PayoutInput {
            date: date!(2026 - 05 - 15),
            gross: eur(1_000),
            treasury: "5720001".into(),
            agreement: None,
        },
        AHORA,
        true,
    )
    .unwrap();

    // Canon a Francia con certificado: 5 % del convenio.
    no_residentes::guardar_certificado(
        &v,
        &certificado("FR12345678901", "FR", date!(2026 - 01 - 10)),
        AHORA,
    )
    .unwrap();
    let canon = renta(
        "FR12345678901",
        "FR",
        Naturaleza::Canon,
        2_000,
        date!(2026 - 04 - 20),
    );
    assert!(no_residentes::apuntar(&v, &canon, false, AHORA).is_err());
    let (_, p) = no_residentes::apuntar(&v, &canon, true, AHORA).unwrap();
    assert_eq!((p.regimen, p.tipo), (Regimen::Convenio, Rate::percent(5)));
    // Intereses a Alemania exentos (art. 14.1.c) con certificado.
    no_residentes::guardar_certificado(
        &v,
        &certificado("DE123456789", "DE", date!(2026 - 03 - 01)),
        AHORA,
    )
    .unwrap();
    let mut int = renta(
        "DE123456789",
        "DE",
        Naturaleza::Interes,
        500,
        date!(2026 - 06 - 30),
    );
    int.exencion = Some(Exencion::Art14_1C);
    let (_, p) = no_residentes::apuntar(&v, &int, true, AHORA).unwrap();
    assert_eq!(p.regimen, Regimen::Exenta);
    // Servicio a un residente en un país sin certificado: 24 %.
    let serv = renta(
        "JP-1",
        "JP",
        Naturaleza::Servicio,
        1_000,
        date!(2026 - 05 - 05),
    );
    let (_, p) = no_residentes::apuntar(&v, &serv, true, AHORA).unwrap();
    assert_eq!(p.tipo, Rate::percent(24));
    // Los dividendos no se apuntan a mano.
    let div = renta("X", "US", Naturaleza::Dividendo, 1, date!(2026 - 05 - 05));
    assert!(no_residentes::apuntar(&v, &div, true, AHORA).is_err());

    let d = models::draft(&v, "216", 2026, 2).unwrap();
    // Con retención: dividendo (190), canon (100) y servicio (240); sin retención: intereses.
    let caja = |k: u16| d.boxes.iter().find(|(c, _)| *c == k).unwrap().1;
    assert_eq!(caja(2), eur(4_000));
    assert_eq!(caja(3), eur(530));
    assert_eq!(caja(5), eur(500));
    assert_eq!(caja(7), eur(530));
    assert!(d.payees.contains(&(1, 3)) && d.payees.contains(&(4, 1)));
    assert_eq!(d.declaration, "I");
    assert!(d.file.is_empty());
    assert!(
        models::draft(&v, "216", 2026, 3)
            .unwrap()
            .declaration
            .is_empty()
    );

    let d296 = models::draft(&v, "296", 2026, 0).unwrap();
    assert_eq!(d296.result, eur(530));
    assert_eq!(d296.details.len(), 4);
    assert!(!d296.warnings.iter().any(|w| w.contains("no cuadran")));

    let c = certificados::certificates(&v, "296", 2026, &opts()).unwrap();
    assert_eq!(c.len(), 4);
    let fr = c.iter().find(|c| c.nif == "FR12345678901").unwrap();
    assert_eq!(fr.retenido(), eur(100));
    assert!(
        matches!(&fr.detalles[0], Detalle::NoResidente { regimen, .. } if regimen.contains("convenio"))
    );
    let us = c.iter().find(|c| c.nif == "US-123456789").unwrap();
    assert_eq!(us.address.as_deref(), Some("1 Example Street, Wilmington"));
    let (_, pdf) = certificados::pdf_all(&v, "296", 2026, &opts()).unwrap();
    assert!(pdf.starts_with(b"%PDF"));

    let r = no_residentes::resumen(&v, 2026, date!(2026 - 11 - 17)).unwrap();
    assert_eq!(r.rentas.len(), 3);
    assert_eq!(r.dividendos.len(), 1);
    assert_eq!(r.certificados.len(), 2);
    assert!(r.certificados.iter().all(|c| c.vigente));
}

#[test]
fn las_recibidas_de_proveedores_extranjeros_son_candidatas() {
    let v = setup();
    receive(
        &v,
        "IE1234567T",
        "LIC-1",
        (3_000, 0, 0),
        date!(2026 - 03 - 15),
        Nature::General,
        Some("IE"),
    );
    receive(
        &v,
        "A22222228",
        "N-1",
        (100, 21, 0),
        date!(2026 - 03 - 15),
        Nature::General,
        None,
    );
    let c = no_residentes::candidatas(&v, 2026).unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].pais.as_str(), c[0].base), ("IE", eur(3_000)));
    let mut r = renta("IE1234567T", "IE", Naturaleza::Canon, 3_000, c[0].fecha);
    r.recibida_id = Some(c[0].recibida_id);
    no_residentes::apuntar(&v, &r, true, AHORA).unwrap();
    assert!(no_residentes::candidatas(&v, 2026).unwrap().is_empty());
    // Sin certificado: canon a la UE al 19 %.
    let rs = no_residentes::rentas_del_ano(&v, 2026).unwrap();
    assert_eq!(rs[0].tipo, Rate::percent(19));
    assert_eq!(rs[0].retencion, eur(570));
    // Quitar pide confirmación y deja auditoría.
    assert!(no_residentes::quitar_renta(&v, rs[0].id, false, AHORA).is_err());
    no_residentes::quitar_renta(&v, rs[0].id, true, AHORA).unwrap();
    assert_eq!(no_residentes::candidatas(&v, 2026).unwrap().len(), 1);
}

#[test]
fn el_certificado_de_residencia_vale_un_ano_y_avisa_al_caducar() {
    let v = setup();
    no_residentes::guardar_certificado(&v, &certificado("FR1", "FR", date!(2025 - 12 - 01)), AHORA)
        .unwrap();
    assert!(
        no_residentes::guardar_certificado(
            &v,
            &certificado("X", "ES", date!(2026 - 01 - 01)),
            AHORA
        )
        .is_err()
    );
    let c = &no_residentes::certificados(&v).unwrap()[0];
    assert_eq!(c.valido_hasta, Some(date!(2026 - 11 - 30)));
    let canon = renta("FR1", "FR", Naturaleza::Canon, 1_000, date!(2026 - 02 - 01));
    no_residentes::apuntar(&v, &canon, true, AHORA).unwrap();
    let avisos = no_residentes::avisos_certificados(&v, date!(2026 - 11 - 17)).unwrap();
    assert!(
        avisos.iter().any(|a| a.contains("caduca el 2026-11-30")),
        "{avisos:?}"
    );
    let avisos = no_residentes::avisos_certificados(&v, date!(2026 - 12 - 05)).unwrap();
    assert!(avisos.iter().any(|a| a.contains("caducó")));
    // Una renta posterior a la caducidad va al tipo general.
    let tarde = renta("FR1", "FR", Naturaleza::Canon, 1_000, date!(2026 - 12 - 10));
    let p = no_residentes::proponer(&v, &tarde).unwrap();
    assert_eq!(p.tipo, Rate::percent(19));
}

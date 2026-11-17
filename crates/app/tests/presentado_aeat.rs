//! «Comprobar lo presentado en el año»: lo que devuelve la AEAT, lo que faltaba según el perfil,
//! las complementarias, la descarga por CSV y la comparación con TaxPain. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use std::collections::BTreeMap;
use taxpain_app::invoicing;
use taxpain_app::models;

use taxpain_app::presentado_aeat::{self, Answer, Download, Presented, Status, Visor};
use taxpain_app::presentados;
use taxpain_app::received::{Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime, Time};

const NIF: &str = "B12345674";

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
            id: PartyId::Nif(NIF.into()),
            name: "EMPRESA DEMO SL".into(),
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
            enabled: false,
            installation_id: "x".into(),
            production: false,
        },
        footer: None,
        activity: None,
        simplified_up_to_3000: false,
    };
    invoicing::save_settings(&v, &s, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v.set_setting("taxpayer_profile", &TaxpayerProfile::sociedad_limitada())
        .unwrap();
    // Una compra del 1T: el 303 del 1T sale a compensar.
    let d = date!(2026 - 02 - 10);
    let rec = ingest_file(
        &v,
        "C-1.xml",
        b"<?xml version=\"1.0\"?><x n=\"C-1\"/>",
        Source::Manual,
        at(d),
    )
    .unwrap();
    review(
        &v,
        rec.id,
        &Reviewed {
            supplier_nif: "B98765431".into(),
            supplier_name: "Proveedor".into(),
            number: "C-1".into(),
            issue_date: d,
            base: Money::from_euros(1000, 0),
            vat: Money::from_euros(210, 0),
            withholding: Money::ZERO,
            total: Money::from_euros(1210, 0),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        at(d),
    )
    .unwrap();
    v
}

fn filing(
    model: &str,
    period: &str,
    csv: &str,
    receipt: &str,
    prev: Option<&str>,
    at: &str,
) -> Presented {
    Presented {
        model: model.into(),
        year: 2026,
        period: period.into(),
        csv: csv.into(),
        file_number: String::new(),
        receipt: receipt.into(),
        previous_receipt: prev.map(Into::into),
        presented_at: at.into(),
    }
}

fn aeat_list() -> Vec<Presented> {
    vec![
        filing(
            "303",
            "1T",
            "AAAAAAAAAAAAAAA1",
            "3030000000001",
            None,
            "2026-04-15T10:00:00",
        ),
        filing(
            "303",
            "2T",
            "AAAAAAAAAAAAAAA2",
            "3030000000002",
            None,
            "2026-07-15T10:00:00",
        ),
        // Complementaria del 2T.
        filing(
            "303",
            "2T",
            "AAAAAAAAAAAAAAA3",
            "3030000000003",
            Some("3030000000002"),
            "2026-09-01T10:00:00",
        ),
        // El 111 del 1T, presentado en mayo: fuera de plazo.
        filing(
            "111",
            "1T",
            "AAAAAAAAAAAAAAA4",
            "1110000000001",
            None,
            "2026-05-02T10:00:00",
        ),
    ]
}

fn queried() -> Vec<String> {
    ["303", "111", "349", "390"].map(String::from).to_vec()
}

fn answer(list: Vec<Presented>, errors: &[&str]) -> Answer {
    Answer {
        models: queried(),
        list,
        errors: errors.iter().map(|e| (*e).to_owned()).collect(),
        empty: BTreeMap::new(),
        unavailable: BTreeMap::new(),
    }
}

#[test]
fn modelos_que_se_consultan() {
    let v = setup();
    let m = presentado_aeat::models_to_query(&v, 2026).unwrap();
    for code in [
        "303", "390", "111", "115", "190", "180", "200", "202", "349", "347", "216", "296", "369",
        "720", "721",
    ] {
        assert!(m.contains(&code.to_string()), "{code}: {m:?}");
    }
    // Una sociedad no presenta Renta.
    assert!(!m.contains(&"100".to_string()));
    assert_eq!(m[0], "303");
}

#[test]
fn tabla_del_año_con_complementarias_faltas_y_fuera_de_plazo() {
    let v = setup();
    let now = datetime!(2027-10-27 10:00 UTC);
    let today = date!(2027 - 10 - 27);

    // Antes de consultar: nada que falte (no se sabe).
    let r = presentado_aeat::report(&v, 2026, today).unwrap();
    assert!(r.checked_at.is_none());
    assert!(r.rows.is_empty());
    assert_eq!(r.nif, NIF);

    presentado_aeat::record(&v, NIF, 2026, &answer(aeat_list(), &[]), now).unwrap();
    // Consultar dos veces no duplica.
    presentado_aeat::record(&v, NIF, 2026, &answer(aeat_list(), &[]), now).unwrap();
    assert_eq!(presentado_aeat::pending(&v, 2026).unwrap().len(), 4);

    // El 1T: PDF y fichero de la declaración (el mismo que calcula TaxPain).
    let file = models::draft(&v, "303", 2026, 1).unwrap().file;
    presentado_aeat::attach(
        &v,
        "AAAAAAAAAAAAAAA1",
        Download::Pdf(b"%PDF-1.4 justificante de prueba"),
        Visor::File(&file),
        now,
    )
    .unwrap();
    // La complementaria del 2T no se pudo descargar.
    presentado_aeat::attach(
        &v,
        "AAAAAAAAAAAAAAA3",
        Download::Failed("la AEAT no devolvió el PDF"),
        Visor::Unknown,
        now,
    )
    .unwrap();
    assert_eq!(presentado_aeat::pending(&v, 2026).unwrap().len(), 3);

    let r = presentado_aeat::report(&v, 2026, today).unwrap();
    assert!(r.checked_at.is_some());
    let row = |m: &str, p: &str| {
        let found: Vec<_> = r
            .rows
            .iter()
            .filter(|x| x.model == m && x.period == p)
            .collect();
        assert_eq!(found.len(), 1, "{m} {p}: {:?}", r.rows);
        found[0].clone()
    };
    let q1 = row("303", "1T");
    assert_eq!(q1.status, Status::Presented);
    assert!(q1.has_pdf && q1.boxes_read && q1.expected && !q1.late);
    assert_eq!(q1.mismatches, Some(0), "{:?}", q1.note);

    // El 2T: una sola fila, la complementaria, que sustituye a la primera. El plazo se juzga
    // con la original (a tiempo); la complementaria, presentada después, se señala aparte.
    let q2 = row("303", "2T");
    assert_eq!(q2.receipt.as_deref(), Some("3030000000003"));
    assert_eq!(q2.replaces, vec!["3030000000002".to_string()]);
    assert_eq!(q2.note.as_deref(), Some("la AEAT no devolvió el PDF"));
    assert!(!q2.has_pdf);
    assert!(!q2.late && q2.late_amendment);
    assert_eq!(
        q2.original_presented_at.as_deref(),
        Some("2026-07-15T10:00:00")
    );

    assert!(row("111", "1T").late);
    // El 303 y el 390 tocan siempre: faltan.
    assert_eq!(row("303", "3T").status, Status::Missing);
    assert_eq!(row("303", "4T").status, Status::Missing);
    assert_eq!(row("390", "0A").status, Status::Missing);
    // El 111 sólo toca si hubo retenciones, y TaxPain no calcula ninguna: no consta, sin
    // darlo por pendiente.
    let q111 = row("111", "2T");
    assert_eq!(q111.status, Status::NotFound);
    assert!(
        q111.note.as_deref().unwrap().contains("retención"),
        "{q111:?}"
    );
    // El 115 tocaba (local alquilado) pero no se consultó: no se dice nada.
    assert!(r.rows.iter().all(|x| x.model != "115"));
    // El 349 se consultó, no tocaba y no hay nada.
    assert_eq!(r.none_filed, vec!["349".to_string()]);
    assert_eq!(r.missing(), 2 + 1);
    // Orden: el 303 primero y sus trimestres en orden.
    assert_eq!(
        r.rows
            .iter()
            .take(4)
            .map(|x| x.period.as_str())
            .collect::<Vec<_>>(),
        ["1T", "2T", "3T", "4T"]
    );
    // El CSV no sale en el resultado.
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("AAAAAAAAAAAAAAA"), "{json}");

    // Con el plazo abierto: «en plazo», no «falta».
    let r = presentado_aeat::report(&v, 2026, date!(2026 - 10 - 01)).unwrap();
    assert_eq!(
        r.rows
            .iter()
            .find(|x| x.model == "303" && x.period == "3T")
            .unwrap()
            .status,
        Status::Open
    );

    // Los justificantes descargados, a una carpeta.
    let dir = tempfile::tempdir().unwrap();
    let paths = presentado_aeat::export(&v, 2026, dir.path()).unwrap();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].ends_with("Modelo_303_2026_1T_justificante_3030000000001.pdf"));
    assert!(
        std::fs::read(&paths[0])
            .unwrap()
            .starts_with(b"%PDF-1.4 justificante")
    );
}

#[test]
fn diferencias_con_lo_que_calcula_taxpain_y_errores_de_consulta() {
    let v = setup();
    let now = datetime!(2027-10-27 10:00 UTC);
    presentado_aeat::record(
        &v,
        NIF,
        2026,
        &answer(aeat_list(), &["390: la AEAT no respondió"]),
        now,
    )
    .unwrap();
    // El fichero del gestor, con otra compra que TaxPain no tiene.
    let other = setup();
    let d = date!(2026 - 03 - 03);
    let rec = ingest_file(&other, "C-2.xml", b"<x n=\"C-2\"/>", Source::Manual, at(d)).unwrap();
    review(
        &other,
        rec.id,
        &Reviewed {
            supplier_nif: "B98765431".into(),
            supplier_name: "Proveedor".into(),
            number: "C-2".into(),
            issue_date: d,
            base: Money::from_euros(500, 0),
            vat: Money::from_euros(105, 0),
            withholding: Money::ZERO,
            total: Money::from_euros(605, 0),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        at(d),
    )
    .unwrap();
    let file = models::draft(&other, "303", 2026, 1).unwrap().file;
    // Un fichero de otro modelo o periodo no se toma por el de esta declaración.
    presentado_aeat::attach(
        &v,
        "AAAAAAAAAAAAAAA4",
        Download::Failed("x"),
        Visor::File(&file),
        now,
    )
    .unwrap();
    presentado_aeat::attach(
        &v,
        "AAAAAAAAAAAAAAA1",
        Download::Failed("x"),
        Visor::File(&file),
        now,
    )
    .unwrap();
    let r = presentado_aeat::report(&v, 2026, date!(2027 - 10 - 27)).unwrap();
    let q1 = r
        .rows
        .iter()
        .find(|x| x.model == "303" && x.period == "1T")
        .unwrap();
    assert!(q1.mismatches.unwrap() > 0);
    assert!(!q1.has_pdf);
    assert!(!r.rows.iter().find(|x| x.model == "111").unwrap().boxes_read);
    assert_eq!(r.errors, vec!["390: la AEAT no respondió".to_string()]);
    assert!(!r.none_filed.contains(&"390".to_string()));
    // Sin respuesta de la AEAT no se dice que el 390 falte.
    assert!(r.rows.iter().all(|x| x.model != "390"), "{:?}", r.rows);
    // Un PDF que no lo es se rechaza.
    assert!(
        presentado_aeat::attach(
            &v,
            "AAAAAAAAAAAAAAA2",
            Download::Pdf(b"<html>"),
            Visor::Unknown,
            now
        )
        .is_err()
    );
    // CSV desconocido.
    assert!(
        presentado_aeat::attach(
            &v,
            "ZZZZZZZZZZZZZZZZ",
            Download::Stored,
            Visor::Unknown,
            now
        )
        .is_err()
    );
}

#[test]
fn el_segundo_plazo_de_la_renta_no_es_otra_declaracion() {
    let p = TaxpayerProfile::autonomo();
    let e = presentado_aeat::expected(&p, 2026);
    assert!(
        e.contains_key(&("130".to_string(), "1T".to_string())),
        "{e:?}"
    );
    assert!(e.keys().filter(|(m, _)| m == "100").count() <= 1, "{e:?}");
    assert!(!e.keys().any(|(m, p)| m == "100" && p != "0A"), "{e:?}");
}

#[test]
fn sin_perfil_solo_lo_presentado() {
    let v = setup();
    v.connection()
        .execute("DELETE FROM settings WHERE key = 'taxpayer_profile'", [])
        .unwrap();
    let now = datetime!(2027-10-27 10:00 UTC);
    presentado_aeat::record(&v, NIF, 2026, &answer(aeat_list(), &[]), now).unwrap();
    let r = presentado_aeat::report(&v, 2026, date!(2027 - 10 - 27)).unwrap();
    assert!(r.without_profile);
    assert_eq!(r.missing(), 0);
    assert_eq!(r.rows.len(), 3);
    assert!(r.rows.iter().all(|x| !x.expected && !x.late));
}

fn set_profile(v: &Vault, f: impl FnOnce(&mut TaxpayerProfile)) {
    let mut p = TaxpayerProfile::sociedad_limitada();
    f(&mut p);
    v.set_setting("taxpayer_profile", &p).unwrap();
}

#[test]
fn retenciones_solo_cuando_las_hubo_e_informativas_sin_comprobar() {
    let v = setup();
    // Una SL que reparte dividendos sólo en el 4T y tiene trabajadores.
    set_profile(&v, |p| p.retiene_capital_mobiliario = true);
    let now = datetime!(2027-10-27 10:00 UTC);
    // Presenta el IVA de todo el año (así consta que estaba dada de alta desde el 1T).
    let mut list: Vec<Presented> = ["1T", "2T", "3T", "4T"]
        .iter()
        .enumerate()
        .map(|(i, q)| {
            filing(
                "303",
                q,
                &format!("IIIIIIIIIIIIIII{i}"),
                &format!("303000000000{i}"),
                None,
                "2027-01-20T10:00:00",
            )
        })
        .collect();
    list.push(filing(
        "390",
        "0A",
        "IIIIIIIIIIIIIII9",
        "3900000000009",
        None,
        "2027-01-20T10:00:00",
    ));
    list.push(filing(
        "123",
        "4T",
        "BBBBBBBBBBBBBBB1",
        "1230000000004",
        None,
        "2027-01-19T10:00:00",
    ));
    let a = Answer {
        models: ["303", "390", "123", "193", "190", "202"]
            .map(String::from)
            .to_vec(),
        list,
        errors: Vec::new(),
        unavailable: BTreeMap::new(),
        empty: [
            ("193", "Usted no ha presentado la declaración solicitada"),
            ("190", "Usted no ha presentado la declaración solicitada"),
            ("202", "Usted no ha presentado la declaración solicitada"),
        ]
        .into_iter()
        .map(|(m, t)| (m.to_owned(), t.to_owned()))
        .collect(),
    };
    presentado_aeat::record(&v, NIF, 2026, &a, now).unwrap();
    let r = presentado_aeat::report(&v, 2026, date!(2027 - 10 - 27)).unwrap();
    let get = |m: &str, p: &str| {
        r.rows
            .iter()
            .find(|x| x.model == m && x.period == p)
            .unwrap_or_else(|| panic!("{m} {p}: {:?}", r.rows))
            .clone()
    };
    for q in ["1T", "2T", "3T"] {
        assert_eq!(get("123", q).status, Status::NotFound);
    }
    assert_eq!(get("123", "4T").status, Status::Presented);
    // Pyme sin cuota: el 202 no consta, no falta.
    assert_eq!(get("202", "1P").status, Status::NotFound);
    // El 190 sin ninguna declaración y sin retenciones en los 111: no consta, no falta.
    let i190 = get("190", "0A");
    assert_eq!(i190.status, Status::NotFound);
    assert!(
        i190.note.as_deref().unwrap().contains("sólo hacía falta"),
        "{i190:?}"
    );
    assert_eq!(r.missing(), 0);
    // El texto de la AEAT, agrupado.
    assert_eq!(r.empty_answers.len(), 1);
    assert_eq!(r.empty_answers[0].models, ["190", "193", "202"]);

    // Con más de 6 M€ de cifra de negocios (el ajuste del 202), se presenta siempre. El umbral
    // de gran empresa del IVA y las retenciones no cuenta para el 202.
    set_profile(&v, |p| {
        p.retiene_capital_mobiliario = true;
        p.gran_empresa = true;
    });
    let r = presentado_aeat::report(&v, 2026, date!(2027 - 10 - 27)).unwrap();
    assert!(
        !r.rows
            .iter()
            .any(|x| x.model == "202" && x.status == Status::Missing)
    );
    let mut s = taxpain_app::models::settings(&v).unwrap();
    s.turnover_over_6m = true;
    taxpain_app::models::save_settings(&v, &s, now).unwrap();
    let r = presentado_aeat::report(&v, 2026, date!(2027 - 10 - 27)).unwrap();
    assert!(
        r.rows
            .iter()
            .any(|x| x.model == "202" && x.status == Status::Missing),
        "{:?}",
        r.rows
    );
}

#[test]
fn otra_periodicidad_no_es_una_falta() {
    let v = setup();
    let now = datetime!(2027-10-27 10:00 UTC);
    // El perfil de hoy es trimestral, pero ese año la AEAT tiene el 303 mensual.
    let list = (1..=12)
        .map(|m| {
            filing(
                "303",
                &format!("{m:02}"),
                &format!("CCCCCCCCCCCCCC{m:02}"),
                &format!("30300000001{m:02}"),
                None,
                "2026-12-30T10:00:00",
            )
        })
        .collect();
    presentado_aeat::record(&v, NIF, 2026, &answer(list, &[]), now).unwrap();
    let r = presentado_aeat::report(&v, 2026, date!(2027 - 10 - 27)).unwrap();
    let q3 = r
        .rows
        .iter()
        .find(|x| x.model == "303" && x.period == "3T")
        .unwrap();
    assert_eq!(q3.status, Status::NotFound);
    assert!(q3.note.as_deref().unwrap().contains("periodicidad"));
}

#[test]
fn quitar_el_modelo_importado_y_reintentos() {
    let v = setup();
    let now = datetime!(2027-10-27 10:00 UTC);
    presentado_aeat::record(&v, NIF, 2026, &answer(aeat_list(), &[]), now).unwrap();
    let file = models::draft(&v, "303", 2026, 1).unwrap().file;
    presentado_aeat::attach(
        &v,
        "AAAAAAAAAAAAAAA1",
        Download::Pdf(b"%PDF-1.4 uno"),
        Visor::File(&file),
        now,
    )
    .unwrap();
    let r = presentado_aeat::report(&v, 2026, date!(2027 - 10 - 27)).unwrap();
    let id = r
        .rows
        .iter()
        .find(|x| x.model == "303" && x.period == "1T")
        .unwrap()
        .filed_model_id
        .unwrap();
    // «Quitar» en Importar → Comparar ya no choca con la declaración de la AEAT.
    presentados::delete(&v, id).unwrap();
    let r = presentado_aeat::report(&v, 2026, date!(2027 - 10 - 27)).unwrap();
    let q1 = r
        .rows
        .iter()
        .find(|x| x.model == "303" && x.period == "1T")
        .unwrap();
    // Quitarlo de la comparación no borra las casillas de la declaración (siguen para la
    // evolución de la empresa) ni hace que se vuelva a pedir a la AEAT.
    assert!(q1.has_pdf && q1.boxes_read && q1.filed_model_id.is_none());
    assert!(
        presentado_aeat::pending(&v, 2026)
            .unwrap()
            .iter()
            .all(|x| x.csv != "AAAAAAAAAAAAAAA1")
    );

    // Sin casillas (el visor no respondió), se reintentan al volver a comprobar, y el fichero
    // que dio el visor se guarda para leerlo después sin pedirlo otra vez.
    presentado_aeat::attach(
        &v,
        "AAAAAAAAAAAAAAA2",
        Download::Pdf(b"%PDF-1.4 dos"),
        Visor::Unknown,
        now,
    )
    .unwrap();
    let p = presentado_aeat::pending(&v, 2026).unwrap();
    // El 2T original está sustituido por su complementaria: no se leen sus casillas.
    assert!(p.iter().all(|x| x.csv != "AAAAAAAAAAAAAAA2"));
    presentado_aeat::attach(
        &v,
        "AAAAAAAAAAAAAAA3",
        Download::Pdf(b"%PDF-1.4 tres"),
        Visor::Unknown,
        now,
    )
    .unwrap();
    let again = presentado_aeat::pending(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|x| x.csv == "AAAAAAAAAAAAAAA3")
        .unwrap();
    assert!(again.has_pdf && !again.visor_done);

    // Una respuesta definitiva del cotejo no se vuelve a pedir.
    presentado_aeat::attach(
        &v,
        "AAAAAAAAAAAAAAA4",
        Download::Final {
            state: "no_encontrado",
            message: "La AEAT no encuentra el documento.",
        },
        Visor::Unknown,
        now,
    )
    .unwrap();
    assert!(
        presentado_aeat::pending(&v, 2026)
            .unwrap()
            .iter()
            .all(|x| x.csv != "AAAAAAAAAAAAAAA4")
    );
    let r = presentado_aeat::report(&v, 2026, date!(2027 - 10 - 27)).unwrap();
    let q111 = r.rows.iter().find(|x| x.model == "111").unwrap();
    assert_eq!(
        q111.note.as_deref(),
        Some("La AEAT no encuentra el documento.")
    );
}

#[test]
fn una_complementaria_se_compara_con_el_total_del_periodo() {
    let v = setup();
    let draft = models::draft(&v, "303", 2026, 1).unwrap();
    let mut boxes: BTreeMap<u16, Money> = draft.boxes.iter().copied().collect();
    let total = boxes.get(&71).copied().unwrap_or(Money::ZERO);
    // La complementaria descuenta 100 € ingresados antes.
    boxes.insert(70, Money::from_euros(100, 0));
    boxes.insert(71, total - Money::from_euros(100, 0));
    let read = presentados::Read {
        model: "303".into(),
        year: 2026,
        period: "1T".into(),
        source: "fichero".into(),
        boxes,
        boxes_read: true,
        ..presentados::Read::default()
    };
    let id = presentados::store_read(
        &v,
        "303_complementaria.txt",
        b"complementaria",
        &read,
        false,
        datetime!(2027-10-27 10:00 UTC),
    )
    .unwrap()
    .id();
    let c = presentados::compare(&v, id).unwrap();
    assert_eq!(c.mismatches, 0, "{:?}", c.rows);
    assert!(c.warnings.iter().any(|w| w.contains("complementaria")));
}

#[test]
fn el_nif_de_otra_empresa_no_se_guarda_ni_se_ensena() {
    let v = setup();
    let now = datetime!(2027-10-27 10:00 UTC);
    assert!(
        presentado_aeat::record(&v, "B98765431", 2026, &answer(aeat_list(), &[]), now).is_err()
    );
    let r = presentado_aeat::report(&v, 2026, date!(2027 - 10 - 27)).unwrap();
    assert!(r.checked_at.is_none() && r.rows.is_empty());
}

#[test]
fn justificantes_de_varios_años_en_subcarpetas() {
    let v = setup();
    let now = datetime!(2027-10-27 10:00 UTC);
    presentado_aeat::record(&v, NIF, 2026, &answer(aeat_list(), &[]), now).unwrap();
    let mut y24 = filing(
        "303",
        "4T",
        "DDDDDDDDDDDDDDD1",
        "3030000000024",
        None,
        "2026-01-30T10:00:00",
    );
    y24.year = 2025;
    presentado_aeat::record(&v, NIF, 2025, &answer(vec![y24], &[]), now).unwrap();
    presentado_aeat::attach(
        &v,
        "AAAAAAAAAAAAAAA1",
        Download::Pdf(b"%PDF-1"),
        Visor::Unknown,
        now,
    )
    .unwrap();
    presentado_aeat::attach(
        &v,
        "DDDDDDDDDDDDDDD1",
        Download::Pdf(b"%PDF-2"),
        Visor::Unknown,
        now,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let paths = presentado_aeat::export_years(&v, &[2024, 2025, 2026], dir.path()).unwrap();
    assert_eq!(paths.len(), 2);
    assert!(dir.path().join("2025").is_dir() && dir.path().join("2026").is_dir());
    assert!(!dir.path().join("2024").exists());
    let r = presentado_aeat::report(&v, 2025, date!(2027 - 10 - 27)).unwrap();
    let s = presentado_aeat::YearSummary::from(&r);
    assert_eq!((s.year, s.presented, s.pdfs), (2025, 1, 1));
}

/// Sin la fecha de alta: un año sin ninguna declaración no da «Falta»; con retenciones en los 111
/// sí falta el 190; y una complementaria cuya original no está en la lista no hace tardía la
/// primera presentación.
#[test]
fn alta_retenciones_y_complementaria_sin_original() {
    let v = setup();
    set_profile(&v, |_| {});
    let now = datetime!(2027-10-27 10:00 UTC);
    let models = ["303", "390", "111", "190"].map(String::from).to_vec();
    let empty = |m: &[&str]| -> BTreeMap<String, String> {
        m.iter()
            .map(|m| {
                (
                    (*m).to_owned(),
                    "Usted no ha presentado la declaración solicitada".to_owned(),
                )
            })
            .collect()
    };
    // 1. La AEAT no tiene nada del año: ¿existía la empresa? Nada sale como «Falta».
    let a = Answer {
        models: models.clone(),
        list: Vec::new(),
        errors: Vec::new(),
        unavailable: BTreeMap::new(),
        empty: empty(&["303", "390", "111", "190"]),
    };
    presentado_aeat::record(&v, NIF, 2026, &a, now).unwrap();
    let r = presentado_aeat::report(&v, 2026, date!(2027 - 10 - 27)).unwrap();
    assert_eq!(r.missing(), 0, "{:?}", r.rows);
    let n303 = r.rows.iter().find(|x| x.model == "303").unwrap();
    assert_eq!(n303.status, Status::NotFound);
    assert!(n303.note.as_deref().unwrap().contains("dada de alta"));

    // 2. Con declaraciones: los 111 con retenciones hacen obligatorio el 190.
    let mut list = Vec::new();
    for (i, q) in ["1T", "2T", "3T", "4T"].iter().enumerate() {
        list.push(filing(
            "303",
            q,
            &format!("JJJJJJJJJJJJJJJ{i}"),
            &format!("303100000000{i}"),
            None,
            "2027-01-20T10:00:00",
        ));
        list.push(filing(
            "111",
            q,
            &format!("KKKKKKKKKKKKKKK{i}"),
            &format!("111100000000{i}"),
            None,
            "2027-01-20T10:00:00",
        ));
    }
    list.push(filing(
        "390",
        "0A",
        "JJJJJJJJJJJJJJJ9",
        "3901000000009",
        None,
        "2027-01-20T10:00:00",
    ));
    // Complementaria del 303 del 2T presentada tarde; su original no está en la lista.
    list.push(filing(
        "303",
        "2T",
        "JJJJJJJJJJJJJJJ8",
        "3031000000008",
        Some("3039999999999"),
        "2026-09-01T10:00:00",
    ));
    let a = Answer {
        models,
        list,
        errors: Vec::new(),
        unavailable: BTreeMap::new(),
        empty: empty(&["190"]),
    };
    presentado_aeat::record(&v, NIF, 2026, &a, now).unwrap();
    v.connection()
        .execute(
            "UPDATE aeat_filing SET boxes = '{\"casillas\":{\"28\":12345}}', boxes_source = 'fichero'
             WHERE model = '111' AND period = '2T'",
            [],
        )
        .unwrap();
    let r = presentado_aeat::report(&v, 2026, date!(2027 - 10 - 27)).unwrap();
    let i190 = r.rows.iter().find(|x| x.model == "190").unwrap();
    assert_eq!(i190.status, Status::Missing, "{i190:?}");
    assert!(i190.note.as_deref().unwrap().contains("111"));
    let c = r
        .rows
        .iter()
        .find(|x| x.model == "303" && x.receipt.as_deref() == Some("3031000000008"))
        .unwrap();
    assert!(!c.late, "sin la original no se sabe si fue tarde: {c:?}");
    assert!(c.late_amendment);
}

/// Domicilio inventado: la factura completa lo exige (art. 6.1.e RD 1619/2012).
fn addr() -> Option<taxpain_core::invoice::Address> {
    Some(taxpain_core::invoice::Address {
        street: "Calle Inventada 1".into(),
        post_code: "46001".into(),
        town: "València".into(),
        province: "Valencia".into(),
        country: "ES".into(),
    })
}

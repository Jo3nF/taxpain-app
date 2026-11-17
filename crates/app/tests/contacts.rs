//! Clientes y proveedores: ficha automática al emitir, clientes de la UE con NIF-IVA, búsqueda
//! para el formulario de factura, historial y ficha a mano.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::contacts::{self, ContactData, ContactInput};
use taxpain_app::invoicing::{self, InvoiceDraft};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::invoice::{
    IdType, InvoiceKind, InvoiceLine, NotSubject, Party, PartyId, RoundingMode, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn settings() -> InvoicingSettings {
    InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Agencia de Prueba SL".into(),
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
            pdf: true,
            facturae: false,
            ubl: false,
        },
        destination: None,
        rounding: RoundingMode::PerRate,
        verifactu: VerifactuSettings {
            enabled: true,
            installation_id: "TP-test".into(),
            production: false,
        },
        footer: None,
        activity: None,
        simplified_up_to_3000: false,
    }
}

fn draft(recipient: Party, vat: VatTreatment, euros: i64, day: time::Date) -> InvoiceDraft {
    InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: day,
        operation_date: None,
        recipient: Some(recipient),
        lines: vec![InvoiceLine {
            description: "Desarrollo web".into(),
            quantity_milli: 1000,
            unit_price: Money::from_euros(euros, 0),
            discount: Rate::ZERO,
            vat,
            surcharge: None,
            withholding: None,
            advance: false,
        }],
        rectified: vec![],
        rectified_amounts: None,
        description: "Desarrollo web".into(),
        notes: None,
        currency: None,
        oss: None,
    }
}

#[test]
fn clientes_de_aqui_y_de_la_ue() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-10-24 10:00 UTC);
    invoicing::save_settings(&v, &settings(), now).unwrap();
    let spanish = Party {
        id: PartyId::Nif("12345678Z".into()),
        name: "Carmen Española".into(),
        address: addr(),
        person: None,
    };
    let french = Party {
        id: PartyId::Foreign {
            country: "FR".into(),
            id_type: IdType::NifIva,
            id: "FR12345678901".into(),
        },
        name: "Acme France SAS".into(),
        address: addr(),
        person: None,
    };
    invoicing::issue(
        &v,
        &draft(
            spanish.clone(),
            VatTreatment::Taxed {
                rate: Rate::percent(21),
            },
            1000,
            date!(2026 - 03 - 10),
        ),
        now,
        true,
    )
    .unwrap();
    // Cliente de la UE: servicio no sujeto (inversión del sujeto pasivo), factura en inglés.
    let fr = invoicing::issue_in(
        &v,
        &draft(
            french.clone(),
            VatTreatment::NotSubject(NotSubject::N2),
            3000,
            date!(2026 - 10 - 20),
        ),
        Some(Language::En),
        now,
        true,
    )
    .unwrap();
    assert_eq!(fr.totals.vat, Money::ZERO);

    // Los dos salen en la lista, con su ficha creada al emitir.
    let list = contacts::list(&v, 2026, date!(2026 - 10 - 24)).unwrap();
    assert_eq!(list.len(), 2, "{list:?}");
    let acme = list.iter().find(|r| r.key == "FR12345678901").unwrap();
    assert_eq!(acme.country, "FR");
    assert_eq!(acme.invoiced_year, Money::from_euros(3000, 0));
    assert!(acme.id.is_some() && acme.role == "cliente");
    let carmen = list.iter().find(|r| r.key == "12345678Z").unwrap();
    assert_eq!(carmen.invoiced_total, Money::from_euros(1210, 0));

    // Búsqueda para el formulario: por nombre o por NIF-IVA.
    let s = contacts::search(&v, "acme", 8).unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].party.id, french.id);
    assert_eq!(contacts::search(&v, "fr 1234", 8).unwrap().len(), 1);

    // Ficha a mano: correo e idioma. Guardarla otra vez sin id no la duplica ni la pisa: error.
    let mut input = ContactInput {
        id: None,
        role: "cliente".into(),
        data: ContactData {
            party: french.clone(),
            email: Some("billing@acme.example".into()),
            language: Some(Language::En),
            notes: Some("Paga a 30 días".into()),
            vies: None,
        },
    };
    let e = contacts::save(&v, &input, now).unwrap_err().to_string();
    assert!(e.contains("ya hay una ficha con ese NIF"), "{e}");
    input.id = acme.id;
    let id = contacts::save(&v, &input, now).unwrap();
    assert_eq!(Some(id), acme.id);
    // Una factura nueva no borra el correo ni el idioma.
    invoicing::issue(
        &v,
        &draft(
            french.clone(),
            VatTreatment::NotSubject(NotSubject::N2),
            500,
            date!(2026 - 10 - 22),
        ),
        now,
        true,
    )
    .unwrap();
    let d = contacts::detail(&v, "FR12345678901", 2026, date!(2026 - 10 - 24)).unwrap();
    assert_eq!(d.history.len(), 2);
    assert_eq!(
        d.history[0].date,
        date!(2026 - 10 - 22),
        "más reciente primero"
    );
    assert_eq!(d.row.email.as_deref(), Some("billing@acme.example"));
    assert_eq!(d.data.unwrap().language, Some(Language::En));
    assert_eq!(d.years[0].invoiced, Money::from_euros(3500, 0));
    assert_eq!(
        contacts::search(&v, "acme", 8).unwrap()[0].language,
        Some(Language::En)
    );

    // NIF-IVA de un país de fuera de la UE: no.
    let mut bad = input.clone();
    bad.data.party.id = PartyId::Foreign {
        country: "US".into(),
        id_type: IdType::NifIva,
        id: "123".into(),
    };
    assert!(contacts::save(&v, &bad, now).is_err());

    // Borrar la ficha no borra el historial.
    contacts::delete(&v, id, now).unwrap();
    let list = contacts::list(&v, 2026, date!(2026 - 10 - 24)).unwrap();
    let acme = list.iter().find(|r| r.key == "FR12345678901").unwrap();
    assert!(acme.id.is_none());
    assert_eq!(acme.invoices, 2);
}

#[test]
fn correo_de_la_factura_y_recordatorios() {
    use taxpain_app::correo;
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-10-24 10:00 UTC);
    let mut s = settings();
    s.footer = Some("IBAN ES00 0000 0000 0000 0000 0000".into());
    invoicing::save_settings(&v, &s, now).unwrap();
    let client = Party {
        id: PartyId::Nif("12345678Z".into()),
        name: "Carmen Española".into(),
        address: addr(),
        person: None,
    };
    contacts::save(
        &v,
        &ContactInput {
            id: None,
            role: "cliente".into(),
            data: ContactData {
                party: client.clone(),
                email: Some("carmen@example.com".into()),
                language: None,
                notes: None,
                vies: None,
            },
        },
        now,
    )
    .unwrap();
    let inv = invoicing::issue(
        &v,
        &draft(
            client,
            VatTreatment::Taxed {
                rate: Rate::percent(21),
            },
            1000,
            date!(2026 - 06 - 01),
        ),
        now,
        true,
    )
    .unwrap();

    let d = correo::compose(&v, inv.id, false, date!(2026 - 06 - 02)).unwrap();
    assert_eq!(d.to.as_deref(), Some("carmen@example.com"));
    assert_eq!(d.subject, format!("Factura {}", inv.number));
    assert!(
        d.body.contains("1.210,00 €") && d.body.contains("IBAN ES00"),
        "{}",
        d.body
    );
    let (name, eml) = correo::build(&v, inv.id, d.to.as_deref(), &d.subject, &d.body).unwrap();
    assert!(name.ends_with(".eml"));
    let text = String::from_utf8(eml).unwrap();
    assert!(text.starts_with("X-Unsent: 1\r\nTo: carmen@example.com\r\n"));
    assert!(text.contains("application/pdf"));
    assert!(correo::build(&v, inv.id, Some("no-es-correo"), "x", "y").is_err());

    // Vencida: el recordatorio sube de tono y se cuenta cuando se abre en Mail (no al
    // prepararlo); volver a abrirlo el mismo día no es otro aviso.
    let today = date!(2026 - 10 - 24);
    let r1 = correo::compose(&v, inv.id, true, today).unwrap();
    assert_eq!(r1.reminder, Some(1));
    assert!(r1.overdue_days > 0 && r1.pending == Money::from_euros(1210, 0));
    assert_eq!(r1.overdue, r1.pending);
    assert!(r1.subject.starts_with("Recordatorio de pago"));
    correo::build(&v, inv.id, r1.to.as_deref(), &r1.subject, &r1.body).unwrap();
    assert_eq!(
        correo::compose(&v, inv.id, true, today).unwrap().reminder,
        Some(1),
        "preparado pero sin abrir: no cuenta"
    );
    correo::record_sent(&v, inv.id, true, now).unwrap();
    let again = correo::compose(&v, inv.id, true, today).unwrap();
    assert_eq!(again.reminder, Some(1), "el mismo día es el mismo aviso");
    correo::record_sent(&v, inv.id, true, datetime!(2026-10-24 18:00 UTC)).unwrap();
    let later = date!(2026 - 10 - 31);
    let r2 = correo::compose(&v, inv.id, true, later).unwrap();
    assert_eq!(r2.reminder, Some(2));
    correo::record_sent(&v, inv.id, true, datetime!(2026-10-31 10:00 UTC)).unwrap();
    let r3 = correo::compose(&v, inv.id, true, date!(2026 - 11 - 09)).unwrap();
    assert_eq!(r3.reminder, Some(3));
    assert!(r3.last_reminder.is_some());
    assert!(r3.body.contains("Ley 3/2004"), "{}", r3.body);

    // Cliente de fuera: en inglés.
    let fr = invoicing::issue(
        &v,
        &draft(
            Party {
                id: PartyId::Foreign {
                    country: "FR".into(),
                    id_type: IdType::NifIva,
                    id: "FR12345678901".into(),
                },
                name: "Acme France SAS".into(),
                address: addr(),
                person: None,
            },
            VatTreatment::NotSubject(NotSubject::N2),
            2000,
            date!(2026 - 10 - 20),
        ),
        now,
        true,
    )
    .unwrap();
    let d = correo::compose(&v, fr.id, false, today).unwrap();
    assert_eq!(d.language, Language::En);
    assert!(d.body.contains("€2,000.00"), "{}", d.body);
    assert_eq!(d.to, None, "sin ficha con correo, Mail lo pide");

    // Cobrada: no hay recordatorio.
    taxpain_app::cartera::settle(&v, "emitida", inv.id, 1, today, "572", now).unwrap();
    assert!(correo::compose(&v, inv.id, true, today).is_err());
    assert!(correo::compose(&v, inv.id, false, today).is_ok());
}

#[test]
fn busqueda_global() {
    use taxpain_app::search;
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-10-24 10:00 UTC);
    invoicing::save_settings(&v, &settings(), now).unwrap();
    let inv = invoicing::issue(
        &v,
        &draft(
            Party {
                id: PartyId::Foreign {
                    country: "FR".into(),
                    id_type: IdType::NifIva,
                    id: "FR12345678901".into(),
                },
                name: "Acme France SAS".into(),
                address: addr(),
                person: None,
            },
            VatTreatment::NotSubject(NotSubject::N2),
            1210,
            date!(2026 - 10 - 20),
        ),
        now,
        true,
    )
    .unwrap();
    // Por nombre: la factura, el cliente y el asiento de la factura.
    let hits = search::search(&v, "acme", 10).unwrap();
    let kinds: Vec<&str> = hits.iter().map(|h| h.kind.as_str()).collect();
    assert!(
        kinds.contains(&"emitida") && kinds.contains(&"contacto"),
        "{hits:?}"
    );
    let e = hits.iter().find(|h| h.kind == "emitida").unwrap();
    assert_eq!(e.target, inv.id.to_string());
    // Por importe, escrito como sea.
    for q in ["1.210", "1210,00", "1.210,00 €"] {
        assert!(
            search::search(&v, q, 10)
                .unwrap()
                .iter()
                .any(|h| h.kind == "emitida"),
            "{q}"
        );
    }
    // Por cuenta.
    assert!(
        search::search(&v, "4300", 10)
            .unwrap()
            .iter()
            .any(|h| h.kind == "cuenta" || h.kind == "asiento")
            || search::search(&v, "Clientes", 10)
                .unwrap()
                .iter()
                .any(|h| h.kind == "cuenta")
    );
    // Los comodines de SQL no cuentan como tales.
    assert!(search::search(&v, "%%", 10).unwrap().is_empty());
    assert!(
        search::search(&v, "a", 10).unwrap().is_empty(),
        "demasiado corto"
    );
}

#[test]
fn comprobacion_vies_en_la_ficha() {
    use taxpain_app::contacts::ViesProof;
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-10-24 10:00 UTC);
    let proof = ViesProof {
        valid: true,
        checked_at: "2026-10-24T10:00:00Z".into(),
        request_id: Some("WAPIAAAAZ1".into()),
        name: Some("ACME SAS".into()),
    };
    // Sin ficha: la crea con el nombre de VIES.
    contacts::record_vies(&v, "fr", "12345678901", None, None, proof.clone(), now).unwrap();
    let d = contacts::detail(&v, "FR12345678901", 2026, date!(2026 - 10 - 24)).unwrap();
    assert_eq!(d.row.name, "ACME SAS");
    assert_eq!(
        d.data
            .as_ref()
            .unwrap()
            .vies
            .as_ref()
            .unwrap()
            .request_id
            .as_deref(),
        Some("WAPIAAAAZ1")
    );
    // Guardar la ficha desde el formulario (sin `vies`) no borra la prueba.
    let mut data = d.data.unwrap();
    data.vies = None;
    data.email = Some("a@acme.example".into());
    contacts::save(
        &v,
        &ContactInput {
            id: d.row.id,
            role: "cliente".into(),
            data,
        },
        now,
    )
    .unwrap();
    let d = contacts::detail(&v, "FR12345678901", 2026, date!(2026 - 10 - 24)).unwrap();
    assert!(d.data.unwrap().vies.is_some());
}

fn vault() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    invoicing::save_settings(&v, &settings(), datetime!(2026-10-24 10:00 UTC)).unwrap();
    v
}

fn address() -> taxpain_core::invoice::Address {
    taxpain_core::invoice::Address {
        street: "1 rue de l'Exemple".into(),
        post_code: "75001".into(),
        town: "Paris".into(),
        province: "Île-de-France".into(),
        country: "FR".into(),
    }
}

#[test]
fn una_factura_sin_domicilio_no_borra_la_ficha() {
    let v = vault();
    let now = datetime!(2026-10-24 10:00 UTC);
    let card = Party {
        id: PartyId::Foreign {
            country: "FR".into(),
            id_type: IdType::NifIva,
            id: "FR12345678901".into(),
        },
        name: "Acme France SAS".into(),
        address: Some(address()),
        person: None,
    };
    let id = contacts::save(
        &v,
        &ContactInput {
            id: None,
            role: "cliente".into(),
            data: ContactData {
                party: card.clone(),
                email: Some("billing@acme.example".into()),
                language: Some(Language::En),
                notes: Some("Paga a 30 días".into()),
                vies: None,
            },
        },
        now,
    )
    .unwrap();
    // Factura con el NIF-IVA tecleado sin prefijo (sin elegir la sugerencia, «Duplicar», una
    // plantilla…): la ficha conserva el NIF-IVA con «FR». Sin domicilio no se emite (art. 6.1.e
    // RD 1619/2012).
    let mut bare = card.clone();
    bare.id = PartyId::Foreign {
        country: "FR".into(),
        id_type: IdType::NifIva,
        id: "12345678901".into(),
    };
    bare.address = None;
    bare.name = "ACME France".into();
    assert!(matches!(
        invoicing::issue(
            &v,
            &draft(
                bare.clone(),
                VatTreatment::NotSubject(NotSubject::N2),
                100,
                date!(2026 - 10 - 20),
            ),
            now,
            true,
        ),
        Err(taxpain_app::AppError::Invalid(
            taxpain_core::invoice::ValidationError::MissingRecipientAddress
        ))
    ));
    bare.address = Some(address());
    invoicing::issue(
        &v,
        &draft(
            bare,
            VatTreatment::NotSubject(NotSubject::N2),
            100,
            date!(2026 - 10 - 20),
        ),
        now,
        true,
    )
    .unwrap();
    let d = contacts::detail(&v, "FR12345678901", 2026, date!(2026 - 10 - 24)).unwrap();
    assert_eq!(d.row.id, Some(id));
    let data = d.data.unwrap();
    assert_eq!(data.party.address, Some(address()));
    assert_eq!(data.party.id, card.id);
    assert_eq!(
        data.party.name, "ACME France",
        "el nombre, el de la última factura"
    );
    assert_eq!(data.email.as_deref(), Some("billing@acme.example"));
    assert_eq!(data.notes.as_deref(), Some("Paga a 30 días"));
    // Con domicilio nuevo, se pone al día.
    let mut moved = card.clone();
    moved.address = Some(taxpain_core::invoice::Address {
        street: "2 avenue Nouvelle".into(),
        ..address()
    });
    invoicing::issue(
        &v,
        &draft(
            moved.clone(),
            VatTreatment::NotSubject(NotSubject::N2),
            100,
            date!(2026 - 10 - 21),
        ),
        now,
        true,
    )
    .unwrap();
    let d = contacts::detail(&v, "FR12345678901", 2026, date!(2026 - 10 - 24)).unwrap();
    assert_eq!(d.data.unwrap().party.address, moved.address);
}

#[test]
fn nuevo_contacto_con_un_nif_que_ya_tiene_ficha() {
    let v = vault();
    let now = datetime!(2026-10-24 10:00 UTC);
    let data = |email: Option<&str>| ContactData {
        party: Party {
            id: PartyId::Nif("12345678Z".into()),
            name: "Carmen Española".into(),
            address: addr(),
            person: None,
        },
        email: email.map(str::to_owned),
        language: None,
        notes: Some("Buena clienta".into()),
        vies: None,
    };
    contacts::save(
        &v,
        &ContactInput {
            id: None,
            role: "cliente".into(),
            data: data(Some("carmen@example.com")),
        },
        now,
    )
    .unwrap();
    // «Nuevo contacto» con el mismo NIF (escrito de otra forma): error, y la ficha sigue igual.
    let mut other = data(None);
    other.party.id = PartyId::Nif("12345678-z".into());
    other.notes = None;
    let e = contacts::save(
        &v,
        &ContactInput {
            id: None,
            role: "proveedor".into(),
            data: other,
        },
        now,
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("ya hay una ficha con ese NIF"), "{e}");
    let d = contacts::detail(&v, "12345678Z", 2026, date!(2026 - 10 - 24)).unwrap();
    assert_eq!(d.row.role, "cliente");
    assert_eq!(d.row.email.as_deref(), Some("carmen@example.com"));
    assert_eq!(d.data.unwrap().notes.as_deref(), Some("Buena clienta"));
}

#[test]
fn comprobar_en_vies_un_proveedor_no_lo_hace_cliente() {
    use taxpain_app::contacts::ViesProof;
    let v = vault();
    let now = datetime!(2026-10-24 10:00 UTC);
    let proof = ViesProof {
        valid: true,
        checked_at: "2026-10-24T10:00:00Z".into(),
        request_id: Some("WAPIAAAAZ2".into()),
        name: Some("LIEFERANT GMBH".into()),
    };
    // Sin ficha, desde la fila de un proveedor: la ficha nueva es de proveedor y sin idioma.
    contacts::record_vies(
        &v,
        "DE",
        "123456789",
        None,
        Some("proveedor"),
        proof.clone(),
        now,
    )
    .unwrap();
    let d = contacts::detail(&v, "DE123456789", 2026, date!(2026 - 10 - 24)).unwrap();
    assert_eq!(d.row.role, "proveedor");
    assert_eq!(d.data.as_ref().unwrap().language, None);
    // Con ficha, conserva su tipo aunque se diga otro.
    contacts::record_vies(&v, "DE", "123456789", None, Some("cliente"), proof, now).unwrap();
    let d = contacts::detail(&v, "DE123456789", 2026, date!(2026 - 10 - 24)).unwrap();
    assert_eq!(d.row.role, "proveedor");
    // Un tipo que no existe, no.
    assert!(
        contacts::record_vies(
            &v,
            "FR",
            "12345678901",
            None,
            Some("jefe"),
            ViesProof {
                valid: false,
                checked_at: "2026-10-24T10:00:00Z".into(),
                request_id: None,
                name: None,
            },
            now,
        )
        .is_err()
    );
}

#[test]
fn la_busqueda_global_encuentra_un_proveedor_con_ficha_sin_facturas() {
    use taxpain_app::search;
    let v = vault();
    let now = datetime!(2026-10-24 10:00 UTC);
    contacts::save(
        &v,
        &ContactInput {
            id: None,
            role: "proveedor".into(),
            data: ContactData {
                party: Party {
                    id: PartyId::Nif("A22222228".into()),
                    name: "Imprenta Ejemplo SA".into(),
                    address: addr(),
                    person: None,
                },
                email: None,
                language: None,
                notes: None,
                vies: None,
            },
        },
        now,
    )
    .unwrap();
    for q in ["imprenta", "A-22222228", "22222"] {
        let hits = search::search(&v, q, 10).unwrap();
        let h = hits
            .iter()
            .find(|h| h.kind == "contacto")
            .unwrap_or_else(|| panic!("{q}: {hits:?}"));
        assert_eq!(h.target, "A22222228");
        assert!(h.subtitle.starts_with("Proveedor"), "{h:?}");
    }
    // El formulario de factura sigue sin proponerlo como cliente.
    assert!(contacts::search(&v, "imprenta", 8).unwrap().is_empty());
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

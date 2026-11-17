//! Privacidad de punta a punta (idea de `tests/e2e/escritorio.spec.ts` de reforma-digital): con
//! un almacén en disco, una factura recibida con datos «canario» y un proveedor de IA falso en
//! 127.0.0.1 que guarda lo que recibe, se hace la consulta real de la app y se comprueba que
//! (1) ningún dato de la empresa llega al proveedor y (2) ninguno está en claro en los ficheros
//! del almacén.

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use taxpain_app::assistant::{context, known_values};
use taxpain_app::invoicing;
use taxpain_app::received::{AI_EXTRACTION_PROMPT, Source, document_text, ingest_file};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_assistant::{Client, Message, Provider, Role, ask, external_document, prepare_with};
use taxpain_core::invoice::{Address, Party, PartyId, RoundingMode};
use taxpain_core::money::Rate;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const NAME: &str = "Quiñones Canario Ybarra SL";
const NIF: &str = "B12345674";
const STREET: &str = "Calle del Canario Escondido 77";
const IBAN: &str = "ES9121000418450200051332";

/// Proveedor «compatible con OpenAI» falso: responde siempre lo mismo y guarda cada petición.
async fn fake_provider(seen: Arc<Mutex<Vec<String>>>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).into_owned();
                    if let Some(h) = text.find("\r\n\r\n") {
                        let len = text[..h]
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if buf.len() >= h + 4 + len {
                            break;
                        }
                    }
                }
                seen.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf).into_owned());
                let body = r#"{"choices":[{"message":{"role":"assistant","content":"Hola, [DATO_1]: todo en orden."}}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}/v1")
}

fn settings() -> InvoicingSettings {
    InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif(NIF.into()),
            name: NAME.into(),
            address: Some(Address {
                street: STREET.into(),
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
    }
}

#[tokio::test]
async fn nada_de_la_empresa_sale_ni_queda_en_claro() {
    let dir = tempfile::tempdir().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let base_url = fake_provider(seen.clone()).await;
    let now = datetime!(2026-10-20 10:00 UTC);
    {
        let v = Vault::open(
            dir.path(),
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
        )
        .unwrap();
        invoicing::save_settings(&v, &settings(), now).unwrap();
        let mut ms = taxpain_app::models::settings(&v).unwrap();
        ms.iban = Some(IBAN.into());
        taxpain_app::models::save_settings(&v, &ms, now).unwrap();
        // Factura recibida (texto) con nuestros datos escritos de otra forma y el nombre de un
        // particular.
        let invoice = format!(
            "FACTURA 77/2026\nProveedor: Papelería Ejemplo SA (A22222228)\n\
             Cliente: QUIÑONES CANARIO YBARRA, S.L. - NIF B-12.345.674\n\
             {}, 46001 Valencia\nCargo en ES91 2100 0418 4502 0005 1332\nTotal 121,00 EUR\n\
             Pedido recogido en tienda por Lucía Villalobos Cuesta.",
            STREET.to_uppercase()
        );
        let r = ingest_file(&v, "factura.txt", invoice.as_bytes(), Source::Manual, now).unwrap();

        let provider = Provider::OpenAiCompatible {
            base_url,
            model: "falso".into(),
        };
        let client = Client::new().unwrap();
        let known = known_values(&v).unwrap();
        // Chat con el contexto real de la app.
        let ctx = context(&v, date!(2026 - 10 - 20)).unwrap();
        let q = [Message {
            role: Role::User,
            content: format!("Soy {NAME}, ¿cuándo presento el 303?"),
        }];
        let out = prepare_with(&ctx, &q, &known).unwrap();
        let answer = ask(&client, &provider, None, &out).await.unwrap();
        assert!(
            answer.text.contains(NAME),
            "la respuesta se restaura en el Mac: {}",
            answer.text
        );
        // Extracción de la factura recibida (como en `received_ai_extract`).
        let text = document_text(&v, r.id).unwrap();
        let m = Message {
            role: Role::User,
            content: format!(
                "{AI_EXTRACTION_PROMPT}{}",
                external_document("factura", &text)
            ),
        };
        let out = prepare_with("", &[m], &known).unwrap();
        ask(&client, &provider, None, &out).await.unwrap();
    }

    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    let canaries = [
        "Quiñones",
        "QUIÑONES",
        "Ybarra",
        "YBARRA",
        "12345674",
        "12.345.674",
        "Canario Escondido",
        "CANARIO ESCONDIDO",
        "0200051332",
        "2100 0418",
        // Un particular que no está registrado en ninguna parte: lo tapa el detector de nombres.
        "Lucía",
        "Villalobos",
    ];
    for r in &requests {
        for c in canaries {
            assert!(!r.contains(c), "llega «{c}» al proveedor:\n{r}");
        }
        // El proveedor sí recibe el resto (el proveedor ajeno y el importe) y los marcadores.
        assert!(r.contains("[DATO_"));
    }
    assert!(requests[1].contains("A22222228") || requests[1].contains("[NIF_"));

    // Nada en claro en los ficheros del almacén (SQLCipher).
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        for c in [
            "Quiñones",
            "QUIÑONES",
            "Canario",
            "CANARIO",
            "B12345674",
            IBAN,
        ] {
            assert!(
                !bytes.windows(c.len()).any(|w| w == c.as_bytes()),
                "«{c}» en claro en el almacén"
            );
        }
    }
}

/// Sin herramientas: el proveedor falso no las pide.
struct NoTools;

impl taxpain_assistant::LocalTools for NoTools {
    fn call(&self, _name: &str, _input: &serde_json::Value) -> Result<String, String> {
        Err("sin herramientas en esta prueba".into())
    }
}

#[tokio::test]
async fn una_conversacion_con_datos_y_documento_no_saca_nada_de_la_empresa() {
    use taxpain_app::assistant::{AssistantSettings, save_settings};
    use taxpain_app::conversaciones::{self, AskRequest, ReadBy, Topic};

    let dir = tempfile::tempdir().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let base_url = fake_provider(seen.clone()).await;
    let now = datetime!(2026-10-20 10:00 UTC);
    let hoy = date!(2026 - 10 - 20);
    let topic = Topic::Ahorro {
        id: "capitalizacion".into(),
        year: 2026,
    };
    let stored_answer = {
        let v = Mutex::new(
            Vault::open(
                dir.path(),
                Arc::new(MemoryKeyProvider::default()),
                Arc::new(AlwaysConfirm),
            )
            .unwrap(),
        );
        {
            let v = v.lock().unwrap();
            invoicing::save_settings(&v, &settings(), now).unwrap();
            let mut ms = taxpain_app::models::settings(&v).unwrap();
            ms.iban = Some(IBAN.into());
            taxpain_app::models::save_settings(&v, &ms, now).unwrap();
            save_settings(
                &v,
                &AssistantSettings {
                    provider: Provider::OpenAiCompatible {
                        base_url,
                        model: "falso".into(),
                    },
                    consent: false,
                },
                now,
            )
            .unwrap();
            // Un dato con un particular y nuestro NIF escrito de otra forma, y un documento con
            // el domicilio y la cuenta.
            conversaciones::add_fact(
                &v,
                now,
                &topic,
                None,
                "La administradora es Lucía Villalobos Cuesta y la sociedad, B-12.345.674.",
            )
            .unwrap();
            let doc = conversaciones::read_document(
                "certificado-banco.txt",
                format!(
                    "CERTIFICADO BANCARIO\nTitular: {}\nDomicilio: {}\nCuenta: ES91 2100 0418 \
                     4502 0005 1332\nSaldo a 30/09/2026: 12.000,00 €",
                    NAME.to_uppercase(),
                    STREET
                )
                .as_bytes(),
            )
            .unwrap();
            conversaciones::add_document(&v, now, &topic, None, &doc, ReadBy::Mac).unwrap();
        }
        let client = Client::new().unwrap();
        let mut id = None;
        for q in [
            format!("¿Le conviene a {NAME} dejar el beneficio?"),
            "¿Y si repartimos la mitad?".to_owned(),
        ] {
            let req = AskRequest {
                topic: &topic,
                conversation: id,
                question: &q,
                hoy,
            };
            let o = conversaciones::ask(
                || Ok(v.lock().unwrap()),
                || Ok(v.lock().unwrap()),
                &client,
                &NoTools,
                &req,
                || now,
            )
            .await
            .unwrap();
            id = o.conversation.map(|c| c.id);
        }
        let v = v.lock().unwrap();
        // En la auditoría, el tipo de tema y cuántos datos y documentos; nunca el texto.
        let chats: Vec<String> = taxpain_vault::audit::entries(v.connection())
            .unwrap()
            .into_iter()
            .filter(|e| e.action == "assistant.chat")
            .map(|e| e.details)
            .collect();
        assert_eq!(chats.len(), 2);
        let d: serde_json::Value = serde_json::from_str(&chats[0]).unwrap();
        assert_eq!(
            (&d["tema"], &d["datos"], &d["documentos"]),
            (&"ahorro".into(), &1.into(), &1.into())
        );
        assert!(!chats[0].contains("Villalobos") && !chats[0].contains("beneficio"));
        let c = conversaciones::open_by_id(&v, hoy, id.unwrap())
            .unwrap()
            .conversation
            .unwrap();
        c.messages[1].content.clone()
    };
    // En el Mac la respuesta se guarda restaurada (con los datos reales)…
    assert!(!stored_answer.contains("[DATO_"), "{stored_answer}");
    assert!(stored_answer.contains("12.345.674"), "{stored_answer}");

    // …y vuelve a salir anonimizada con el historial de la segunda pregunta.
    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    let canaries = [
        "Quiñones",
        "QUIÑONES",
        "Ybarra",
        "YBARRA",
        "12345674",
        "12.345.674",
        "Canario Escondido",
        "CANARIO ESCONDIDO",
        "0200051332",
        "2100 0418",
        "Lucía",
        "Villalobos",
        "certificado-banco",
    ];
    for r in &requests {
        for c in canaries {
            assert!(!r.contains(c), "llega «{c}» al proveedor:\n{r}");
        }
        assert!(r.contains("### Tema de esta conversación"));
        assert!(r.contains("<<<DOCUMENTO EXTERNO: documento 2 que he añadido, leído en el Mac>>>"));
    }
    assert!(
        requests[1].contains("todo en orden."),
        "la respuesta anterior va en el historial"
    );

    // Nada en claro en los ficheros del almacén (también el diario WAL).
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        for c in [
            "Quiñones",
            "QUIÑONES",
            "Canario",
            "CANARIO",
            "B12345674",
            "12.345.674",
            "Villalobos",
            "CERTIFICADO BANCARIO",
            IBAN,
        ] {
            assert!(
                !bytes.windows(c.len()).any(|w| w == c.as_bytes()),
                "«{c}» en claro en el almacén"
            );
        }
    }
}

#[tokio::test]
async fn un_hallazgo_de_la_carpeta_no_saca_rutas_ni_datos_de_la_empresa() {
    use taxpain_app::assistant::{AssistantSettings, save_settings};
    use taxpain_app::conversaciones::{self, AskRequest, Topic};
    use taxpain_app::revision::{
        self, Area, Gravedad, Hallazgo, Origen, Responsable, Resumen, Revision,
    };
    use taxpain_core::money::Money;

    let dir = tempfile::tempdir().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let base_url = fake_provider(seen.clone()).await;
    let now = datetime!(2026-10-20 10:00 UTC);
    let hoy = date!(2026 - 10 - 20);
    let titulo = "2 facturas del libro de recibidas de 2025 sin el PDF en la carpeta";
    let topic = Topic::Hallazgo {
        origin: Origen::Carpeta,
        code: "libros.sin_pdf".into(),
        year: Some(2025),
        title: titulo.into(),
    };
    {
        let v = Mutex::new(
            Vault::open(
                dir.path(),
                Arc::new(MemoryKeyProvider::default()),
                Arc::new(AlwaysConfirm),
            )
            .unwrap(),
        );
        {
            let v = v.lock().unwrap();
            invoicing::save_settings(&v, &settings(), now).unwrap();
            save_settings(
                &v,
                &AssistantSettings {
                    provider: Provider::OpenAiCompatible {
                        base_url,
                        model: "falso".into(),
                    },
                    consent: false,
                },
                now,
            )
            .unwrap();
            // La ruta del libro lleva el nombre de la empresa y el detalle, el de un particular
            // y nuestro domicilio.
            let libro = format!("2025/{NAME}/Libro recibidas 2025.xlsx");
            revision::guardar_ultima(
                &v,
                &Revision {
                    generada: "2026-10-19".into(),
                    empresa: NAME.into(),
                    nif: Some(NIF.into()),
                    carpeta: "/Users/canario/Gestoría".into(),
                    resumen: Resumen {
                        ficheros: 1,
                        con_detalle: 1,
                        sin_leer: 0,
                        no_admitidos: 0,
                        por_tipo: vec![],
                        errores: 0,
                        avisos: 1,
                        info: 0,
                        urgentes: 0,
                        en_juego_urgente: Money::ZERO,
                        recuperable: Money::ZERO,
                        a_favor_proximo_200: Money::ZERO,
                        retenciones_proximo_200: Money::ZERO,
                        exencion_proximo_200: Money::ZERO,
                        impuesto_dividendos_proximo_200: Money::ZERO,
                        pendiente_cobro: Money::ZERO,
                    },
                    hallazgos: vec![Hallazgo {
                        codigo: "libros.sin_pdf".into(),
                        area: Area::Libros,
                        gravedad: Gravedad::Aviso,
                        responsable: Responsable::Documentacion,
                        ejercicio: Some(2025),
                        titulo: titulo.into(),
                        detalle: format!(
                            "Están en el libro ({libro}) pero no hay su factura en la carpeta: \
                             F-0912 (Lucía Villalobos Cuesta), F-0288 (enviada a {STREET}). \
                             Pídelas a la gestoría."
                        ),
                        ficheros: vec![libro],
                        importe: None,
                        rectificable_hasta: None,
                        rectificable_aprox: false,
                        terceros: Some(vec![]),
                        urgente: None,
                    }],
                    tablas: vec![],
                    pedir: vec![],
                    devoluciones_pendientes: vec![],
                    dividendos: vec![],
                },
            )
            .unwrap();
        }
        let req = AskRequest {
            topic: &topic,
            conversation: None,
            question: "¿Qué le pido a la gestoría?",
            hoy,
        };
        conversaciones::ask(
            || Ok(v.lock().unwrap()),
            || Ok(v.lock().unwrap()),
            &Client::new().unwrap(),
            &NoTools,
            &req,
            || now,
        )
        .await
        .unwrap();
    }
    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    let r = &requests[0];
    assert!(r.contains("<<<DOCUMENTO EXTERNO: detalle del hallazgo>>>"));
    assert!(r.contains("Están en el libro (fichero 1)"));
    for c in [
        "Quiñones",
        "QUIÑONES",
        "Ybarra",
        "12345674",
        "Canario Escondido",
        "Lucía",
        "Villalobos",
        "Libro recibidas",
        ".xlsx",
        "/Users/canario",
    ] {
        assert!(!r.contains(c), "llega «{c}» al proveedor:\n{r}");
    }
    // La foto del tema se guarda en el almacén cifrado: nada en claro en sus ficheros.
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        for c in ["Quiñones", "Canario", "Villalobos", "Libro recibidas"] {
            assert!(
                !bytes.windows(c.len()).any(|w| w == c.as_bytes()),
                "«{c}» en claro en el almacén"
            );
        }
    }
}

#[tokio::test]
async fn un_borrador_no_saca_su_fichero_ni_los_datos_de_la_empresa() {
    use taxpain_app::assistant::{AssistantSettings, save_settings};
    use taxpain_app::conversaciones::{self, AskRequest, Topic};

    let dir = tempfile::tempdir().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let base_url = fake_provider(seen.clone()).await;
    let now = datetime!(2026-10-20 10:00 UTC);
    let topic = Topic::Modelo {
        model: "303".into(),
        year: 2026,
        period: 3,
    };
    let file_name = {
        let v = Mutex::new(
            Vault::open(
                dir.path(),
                Arc::new(MemoryKeyProvider::default()),
                Arc::new(AlwaysConfirm),
            )
            .unwrap(),
        );
        let file_name = {
            let v = v.lock().unwrap();
            invoicing::save_settings(&v, &settings(), now).unwrap();
            let mut ms = taxpain_app::models::settings(&v).unwrap();
            ms.iban = Some(IBAN.into());
            ms.domiciliar = true;
            taxpain_app::models::save_settings(&v, &ms, now).unwrap();
            save_settings(
                &v,
                &AssistantSettings {
                    provider: Provider::OpenAiCompatible {
                        base_url,
                        model: "falso".into(),
                    },
                    consent: false,
                },
                now,
            )
            .unwrap();
            // El fichero del modelo lleva el NIF y el nombre de la empresa (y su nombre, el NIF).
            let d = taxpain_app::models::draft(&v, "303", 2026, 3).unwrap();
            assert!(d.file.contains(NIF) && d.file_name.contains(NIF));
            d.file_name
        };
        let req = AskRequest {
            topic: &topic,
            conversation: None,
            question: "¿Qué reviso antes de presentarlo?",
            hoy: date!(2026 - 10 - 20),
        };
        conversaciones::ask(
            || Ok(v.lock().unwrap()),
            || Ok(v.lock().unwrap()),
            &Client::new().unwrap(),
            &NoTools,
            &req,
            || now,
        )
        .await
        .unwrap();
        file_name
    };
    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    let r = &requests[0];
    assert!(r.contains("Borrador del modelo 303 de TaxPain del 3T 2026"));
    for c in [
        "Quiñones",
        "QUIÑONES",
        "Ybarra",
        "YBARRA",
        "12345674",
        "0200051332",
        "Canario Escondido",
        file_name.as_str(),
    ] {
        assert!(!r.contains(c), "llega «{c}» al proveedor:\n{r}");
    }
}

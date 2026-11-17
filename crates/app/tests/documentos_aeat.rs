//! Documentos de la Sede de la AEAT en la carpeta de la empresa (certificados, requerimientos,
//! notificaciones, recibos de registro, liquidaciones): se reconocen por sus marcas (el código
//! seguro de verificación y el título), con su tipo y nunca como facturas; y los informes y los
//! correos no son nunca documentos oficiales. Documentos inventados con la forma de los de la
//! Sede; ningún dato es real.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use serde_json::json;
use taxpain_app::carpeta::{self, Found};
use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::documentos::DocKind;
use taxpain_app::revision::{self, Revision};
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_invoicing::pdf::{Para, text_document};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

const PROPIA: &str = "B12345674";
const GESTORIA: &str = "B11111119";

fn vault() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    companies::set_info(
        &v,
        &CompanyInfo {
            name: "Empresa Demo SL".into(),
            nif: Some(PROPIA.into()),
        },
        datetime!(2026-01-01 9:00 UTC),
    )
    .unwrap();
    v
}

/// Un PDF con una línea por párrafo.
fn pdf(lineas: &[&str]) -> Vec<u8> {
    text_document(
        "Documento",
        &[lineas.iter().map(|l| Para::Text((*l).to_owned())).collect()],
    )
    .unwrap()
}

/// El pie de los documentos de la Sede.
fn pie(csv: &str, fecha: &str) -> String {
    format!(
        "Documento firmado electrónicamente (Ley 40/2015) por la Agencia Estatal de \
         Administración Tributaria, con fecha {fecha}. Autenticidad verificable mediante Código \
         Seguro Verificación {csv} en sede.agenciatributaria.gob.es"
    )
}

fn escribir(root: &Path, ruta: &str, bytes: &[u8]) {
    let p = root.join(ruta);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

fn certificado_corriente(fecha: &str, csv: &str) -> Vec<u8> {
    pdf(&[
        "Delegación Especial de DEMO",
        "CERTIFICADO",
        "Nº REFERENCIA: 22500000001",
        "Presentada la solicitud de certificado acreditativo de estar al corriente en el \
         cumplimiento de las obligaciones tributarias, por:",
        &format!("N.I.F.: {PROPIA} RAZÓN SOCIAL: EMPRESA DEMO SL"),
        "DOMICILIO FISCAL: CALLE DEMO 1 46000 CIUDAD DEMO",
        "La Agencia Estatal de Administración Tributaria, CERTIFICA: Que conforme a los datos \
         que obran en la Agencia Tributaria, el solicitante se encuentra al corriente de sus \
         obligaciones tributarias.",
        "El presente certificado se expide a petición del interesado, tiene carácter de POSITIVO \
         y una validez de doce meses contados desde la fecha de su expedición.",
        &pie(csv, fecha),
    ])
}

fn situacion_censal(fecha: &str, csv: &str, obligaciones: &[&str], alta: &str) -> Vec<u8> {
    let mut l = vec![
        "Delegación Especial de DEMO".to_owned(),
        "CERTIFICADO DE SITUACIÓN CENSAL".to_owned(),
        "Nº DE REFERENCIA: 20250000002".to_owned(),
        "OBLIGADO TRIBUTARIO".to_owned(),
        format!("NIF: {PROPIA}"),
        "NOMBRE/RAZON SOCIAL: EMPRESA DEMO SL".to_owned(),
        "CERTIFICA: Que conforme a los datos que obran en la AEAT el obligado tributario \
         presenta la siguiente situación censal:"
            .to_owned(),
        "SITUACIÓN TRIBUTARIA".to_owned(),
        "IMPUESTO SOBRE EL VALOR AÑADIDO".to_owned(),
        "Registros".to_owned(),
        format!(
            "- Esta dado de alta en el Registro de operadores intracomunitarios, desde: {alta}"
        ),
        "Regímenes aplicables Fecha".to_owned(),
        format!("- General {alta}"),
        "IMPUESTO SOBRE SOCIEDADES".to_owned(),
        "ACTIVIDADES ECONÓMICAS".to_owned(),
        format!("Descripción: SERVICIOS DEMO Grupo/sección IAE: 849.9 Fecha de alta: {alta}"),
        "OBLIGACIONES PERIÓDICAS".to_owned(),
        "MODELO PERIODICIDAD".to_owned(),
    ];
    l.extend(obligaciones.iter().map(|o| (*o).to_owned()));
    l.push(
        "Y para que conste, a petición del interesado, se expide la presente certificación."
            .to_owned(),
    );
    l.push(
        "La presente certificación será válida y surtirá efectos mientras no se modifiquen las \
         circunstancias determinantes de su contenido."
            .to_owned(),
    );
    l.push(pie(csv, fecha));
    let refs: Vec<&str> = l.iter().map(String::as_str).collect();
    pdf(&refs)
}

fn requerimiento() -> Vec<u8> {
    requerimiento_con(
        "Justificación de los ingresos declarados en el año 2020 a 2022. B) Documentación de los \
         años 2020 a 2023",
        Some("CIUDAD DEMO, 03 de marzo de 2025"),
    )
}

/// Un requerimiento de la Inspección: lo que pide y su lugar y fecha (si los lleva).
fn requerimiento_con(pide: &str, lugar_y_fecha: Option<&str>) -> Vec<u8> {
    let pide = format!(
        "Conforme a las facultades otorgadas a la Inspección de los Tributos, se requiere la \
         siguiente información: A) {pide} sobre el Impuesto sobre Sociedades."
    );
    let mut l = vec![
        "Delegación Especial de DEMO",
        "INSPECCIÓN REGIONAL - SEDE DEMO",
        "9000000000 Nº Certificado: 2512345678901",
        "EMPRESA DEMO SL",
        "Nº de expediente: INSP - I123452025000123 ( cítese en la respuesta )",
        "REQUERIMIENTO DE INFORMACIÓN",
        &pide,
        "PLAZOS E INCUMPLIMIENTO",
        "Deberá suministrar la información requerida en el plazo máximo de DIEZ días hábiles \
         contados desde el siguiente al de recepción del presente documento.",
    ];
    l.extend(lugar_y_fecha);
    l.push(
        "Documento firmado electrónicamente (Ley 40/2015, artículo 43). Autenticidad verificable \
         mediante Código Seguro Verificación REQU1234DEMO5678 en sede.agenciatributaria.gob.es.",
    );
    let nif = format!("N.I.F: {PROPIA} Referencia: I12345202500012301");
    l.insert(1, &nif);
    pdf(&l)
}

fn recibo(asiento: &str, fecha: &str, asunto: &str, csv: &str) -> Vec<u8> {
    recibo_de(
        "IZ191 - Contestar requerimientos o presentar documentación relacionada con una \
         notificación recibida de la AEAT",
        asiento,
        fecha,
        asunto,
        csv,
    )
}

fn recibo_de(tramite: &str, asiento: &str, fecha: &str, asunto: &str, csv: &str) -> Vec<u8> {
    pdf(&[
        "Página 1",
        "Recibo de presentación",
        "Datos del asiento registral",
        &format!("Número de asiento registral: {asiento}"),
        &format!("Fecha: {fecha}"),
        "Hora: 12:00:00",
        "Datos del trámite",
        &format!("Trámite: {tramite}"),
        "Procedimiento: IZ19 - Requerimiento de información de Inspección",
        "CSV: REQU1234DEMO5678",
        "NCC asociado: 2512345678901",
        "Expediente / Referencia: I123452025000123",
        &format!("Asunto: {asunto}"),
        "Datos del Interesado",
        &format!("NIF: {PROPIA}"),
        "Nombre / Razón Social: EMPRESA DEMO SL",
        "Datos del Representante",
        &format!("NIF: {GESTORIA}"),
        "Nombre / Razón Social: ASESORES DEMO SL",
        &format!(
            "Documento firmado electrónicamente (Ley 40/2015). Autenticidad verificable mediante \
             Código Seguro de Verificación {csv} en https://sede.agenciatributaria.gob.es."
        ),
    ])
}

fn puesta_a_disposicion() -> Vec<u8> {
    pdf(&[
        "CERTIFICADO DE PUESTA A DISPOSICIÓN DE LA NOTIFICACIÓN EN DIRECCIÓN ELECTRÓNICA \
         HABILITADA ÚNICA",
        "IDENTIFICACION DEL DOCUMENTO",
        "Nº Certificado: 2512345678901",
        &format!("Titular: {PROPIA} EMPRESA DEMO SL"),
        "Concepto: Requerimiento - 03/03/2025",
        "Codigo seguro de verificación: REQU1234DEMO5678",
        "El acto objeto de notificación se ha puesto a disposición de con fecha 03-03-2025 y \
         hora 10:00 en el Servicio de Dirección Electrónica Habilitada Única.",
        &pie("DILI1234DEMO5678", "03 de marzo de 2025"),
    ])
}

fn liquidacion_recargo() -> Vec<u8> {
    pdf(&[
        "Delegación Especial de DEMO",
        "GESTION TRIBUTARIA",
        "Nº Certificado: 2112345678902",
        "LIQUIDACIÓN DE RECARGO POR PRESENTACIÓN FUERA DE PLAZO DE AUTOLIQUIDACIÓN",
        &format!("N.I.F.: {PROPIA}"),
        "Referencia: 2021RSC00000001AB",
        "Con fecha 12-05-2021 ha presentado autoliquidación con modelo 303 AUTOLIQUIDACIÓN \
         IMPUESTO VALOR AÑADIDO correspondiente al ejercicio 2020, período 3T, y un resultado a \
         ingresar de 1.000,00 euros. La factura no se paga aquí.",
        "Importe base del recargo 1.000,00 euros",
        "Importe del recargo: 60,00 euros",
        "Reducción del 25%: 15,00 euros",
        "Recargo reducido: 45,00 euros",
        &pie("LIQU1234DEMO5678", "20 de septiembre de 2021"),
    ])
}

/// Una carta sin marcas de la AEAT, con el NIF de la empresa y sin número ni importe: no es una
/// factura.
fn carta_sin_marcas() -> Vec<u8> {
    pdf(&[
        "A LA AGENCIA ESTATAL DE ADMINISTRACIÓN TRIBUTARIA",
        &format!(
            "Don Pedro Demo, como representante de EMPRESA DEMO SL, con NIF {PROPIA}, comparece y \
             EXPONE: que adjunta la documentación pedida."
        ),
        "En Ciudad Demo, a 6 de septiembre de 2025.",
    ])
}

/// Un informe que hace la revisión de TaxPain (o el asistente): habla de escrituras, notarios,
/// depósitos y legalizaciones, pero no es ninguno de ellos.
fn informe() -> Vec<u8> {
    pdf(&[
        "Revisión de la carpeta de EMPRESA DEMO SL (B12345674)",
        "Revisión automática hecha por TaxPain con lo que hay en la carpeta.",
        "La escritura de ampliación ante el notario Don Pedro Demo, protocolo 1234, se inscribió \
         en el Registro Mercantil de Ciudad Demo, asiento 12.",
        "Legalización de libros de 2023 en el Registro Mercantil de Ciudad Demo: presentada el \
         12/11/2024. Depósito de cuentas anuales del ejercicio 2023, entrada 1/2024/1.",
    ])
}

const CORREO: &str = "Asunto: Depósito de las cuentas anuales\n\nHola:\n\nEl depósito de cuentas \
    del ejercicio 2024 en el Registro Mercantil de Ciudad Demo (entrada 1/2025/77) se presentó el \
    15/09/2025. La escritura del notario, protocolo 55, está en la carpeta.\n\nUn saludo";

fn carpeta_de_la_sede(root: &Path) {
    escribir(
        root,
        "SEDE/Certificados/corriente.pdf",
        &certificado_corriente("10 de octubre de 2025", "CORR1234DEMO5678"),
    );
    escribir(
        root,
        "SEDE/Certificados/censal.pdf",
        &situacion_censal(
            "15 de abril de 2026",
            "CENS1234DEMO5678",
            &[
                "IRPF RETENCION TRABAJO PERSONAL TRIMESTRAL",
                "IMPUESTO DE SOCIEDADES ANUAL",
                "IMPUESTO SOBRE EL VALOR AÑADIDO TRIMESTRAL",
            ],
            "01/03/2019",
        ),
    );
    escribir(
        root,
        "SEDE/Requerimiento I123452025000123/Requerimiento.pdf",
        &requerimiento(),
    );
    escribir(
        root,
        "SEDE/Requerimiento I123452025000123/Diligencia.pdf",
        &puesta_a_disposicion(),
    );
    escribir(
        root,
        "SEDE/Requerimiento I123452025000123/Contestación RGE000000012025/Recibo.pdf",
        &recibo(
            "RGE000000012025",
            "14-03-2025",
            "APORTAR DOCUMENTACION",
            "RECI1234DEMO5678",
        ),
    );
    escribir(root, "SEDE/Recargo/Liquidacion.pdf", &liquidacion_recargo());
    escribir(root, "SEDE/Carta a Hacienda.pdf", &carta_sin_marcas());
    escribir(root, "Revisión de la carpeta.pdf", &informe());
    escribir(root, "Correo para la asesoría.txt", CORREO.as_bytes());
}

fn tipo(inv: &carpeta::Inventory, ruta: &str) -> Found {
    inv.items
        .iter()
        .find(|i| i.path == ruta)
        .unwrap_or_else(|| panic!("falta {ruta}: {:?}", inv.items))
        .found
        .clone()
}

fn es(f: &Found, kind: DocKind) -> bool {
    matches!(f, Found::Documento { doc_kind, by_name: false, .. } if *doc_kind == kind)
}

#[test]
fn los_documentos_de_la_sede_tienen_su_tipo_y_no_son_facturas() {
    let v = vault();
    let dir = tempfile::tempdir().unwrap();
    carpeta_de_la_sede(dir.path());
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    let f = |r: &str| tipo(&inv, r);
    assert!(
        es(
            &f("SEDE/Certificados/corriente.pdf"),
            DocKind::CertificadoAeat
        ),
        "{:?}",
        f("SEDE/Certificados/corriente.pdf")
    );
    // Habla del ROI, pero es el certificado de situación censal.
    assert!(
        es(
            &f("SEDE/Certificados/censal.pdf"),
            DocKind::CertificadoSituacionCensal
        ),
        "{:?}",
        f("SEDE/Certificados/censal.pdf")
    );
    assert!(es(
        &f("SEDE/Requerimiento I123452025000123/Requerimiento.pdf"),
        DocKind::RequerimientoAeat
    ));
    assert!(es(
        &f("SEDE/Requerimiento I123452025000123/Diligencia.pdf"),
        DocKind::NotificacionAeat
    ));
    // Con el NIF de la empresa y sin número ni importe: era una «factura emitida».
    assert!(
        es(
            &f("SEDE/Requerimiento I123452025000123/Contestación RGE000000012025/Recibo.pdf"),
            DocKind::RegistroAeat
        ),
        "{:?}",
        f("SEDE/Requerimiento I123452025000123/Contestación RGE000000012025/Recibo.pdf")
    );
    // Dice «factura» e importes, pero es una liquidación.
    assert!(es(
        &f("SEDE/Recargo/Liquidacion.pdf"),
        DocKind::LiquidacionAeat
    ));
    // Sin número y sin importe no es una factura, aunque lleve el NIF de la empresa.
    assert!(
        matches!(f("SEDE/Carta a Hacienda.pdf"), Found::Otro { .. }),
        "{:?}",
        f("SEDE/Carta a Hacienda.pdf")
    );
    // El informe y el correo hablan de escrituras y depósitos, pero no son documentos.
    assert!(
        matches!(f("Revisión de la carpeta.pdf"), Found::Otro { .. }),
        "{:?}",
        f("Revisión de la carpeta.pdf")
    );
    assert!(
        matches!(f("Correo para la asesoría.txt"), Found::Otro { .. }),
        "{:?}",
        f("Correo para la asesoría.txt")
    );
    // Los datos de un documento de la Sede: el NIF, el nombre y la fecha (no la dirección de
    // la oficina de la AEAT).
    let Found::Documento { facts, .. } = f("SEDE/Certificados/corriente.pdf") else {
        unreachable!()
    };
    assert_eq!(facts.nif.as_deref(), Some(PROPIA));
    assert_eq!(facts.date, Some(date!(2025 - 10 - 10)));
    assert!(facts.street.is_none() && facts.post_code.is_none());
}

#[test]
fn el_tipo_de_la_sede_manda_sobre_una_lectura_del_asistente() {
    // El asistente leyó el certificado de situación censal como uno de estar al corriente y el
    // requerimiento como «otro»: las marcas de la AEAT deciden.
    let v = vault();
    let dir = tempfile::tempdir().unwrap();
    carpeta_de_la_sede(dir.path());
    let lecturas = json!({
        "version": 1,
        "generado_por": "Claude Code",
        "lecturas": [
            {"fichero": "SEDE/Certificados/censal.pdf",
             "lectura": {"tipo": "certificado_aeat", "descripcion": "Certificado de la AEAT"}},
            {"fichero": "SEDE/Requerimiento I123452025000123/Requerimiento.pdf",
             "lectura": {"tipo": "otro", "descripcion": "Requerimiento de la Inspección"}},
        ]
    });
    std::fs::write(
        dir.path().join("taxpain-lecturas.json"),
        serde_json::to_vec(&lecturas).unwrap(),
    )
    .unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    assert!(es(
        &tipo(&inv, "SEDE/Certificados/censal.pdf"),
        DocKind::CertificadoSituacionCensal
    ));
    assert!(es(
        &tipo(
            &inv,
            "SEDE/Requerimiento I123452025000123/Requerimiento.pdf"
        ),
        DocKind::RequerimientoAeat
    ));
}

fn revisar(root: &Path) -> Revision {
    let v = vault();
    v.set_setting("taxpayer_profile", &TaxpayerProfile::sociedad_limitada())
        .unwrap();
    revision::revisar(&v, root, date!(2026 - 10 - 31)).unwrap()
}

#[test]
fn de_un_informe_o_un_correo_no_salen_depositos_ni_legalizaciones() {
    let dir = tempfile::tempdir().unwrap();
    carpeta_de_la_sede(dir.path());
    let r = revisar(dir.path());
    let deposito = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Depósito de cuentas y legalización de libros");
    // Ni el informe (legalización de 2023 «presentada el 12/11/2024») ni el correo (depósito de
    // 2024 «presentado el 15/09/2025») son justificantes del Registro.
    if let Some(t) = deposito {
        assert!(
            t.filas.iter().all(|f| !f
                .iter()
                .any(|c| c.contains("12/11/2024") || c.contains("15/09/2025"))),
            "{:?}",
            t.filas
        );
    }
    assert!(
        !r.hallazgos
            .iter()
            .any(|h| h.codigo.starts_with("cruces.legalizacion_fuera")
                || h.codigo.starts_with("cruces.deposito_fuera")),
        "{:?}",
        r.hallazgos.iter().map(|h| &h.titulo).collect::<Vec<_>>()
    );
    // Los documentos de la Sede no son facturas: no hay facturas en la revisión.
    let facturas = r
        .resumen
        .por_tipo
        .iter()
        .filter(|c| c.tipo.starts_with("Facturas"))
        .map(|c| c.n)
        .sum::<usize>();
    assert_eq!(facturas, 0, "{:?}", r.resumen.por_tipo);
}

fn hallazgo<'a>(r: &'a Revision, codigo: &str) -> &'a revision::Hallazgo {
    r.hallazgos
        .iter()
        .find(|h| h.codigo == codigo)
        .unwrap_or_else(|| {
            panic!(
                "falta {codigo}: {:#?}",
                r.hallazgos
                    .iter()
                    .map(|h| format!("{}: {} — {}", h.codigo, h.titulo, h.detalle))
                    .collect::<Vec<_>>()
            )
        })
}

fn tabla<'a>(r: &'a Revision, titulo: &str) -> &'a revision::Tabla {
    r.tablas
        .iter()
        .find(|t| t.titulo == titulo)
        .unwrap_or_else(|| panic!("falta la tabla {titulo}"))
}

/// Una declaración censal (036) de después del certificado de situación censal.
fn censal_036(fecha: &str) -> Vec<u8> {
    pdf(&[
        &format!(
            "DECLARACIÓN CENSAL de modificación (modelo 036) presentada el {fecha} por EMPRESA \
             DEMO SL, NIF {PROPIA}."
        ),
        "Causa: modificación de los datos relativos a retenciones e ingresos a cuenta.",
    ])
}

#[test]
fn certificados_con_su_validez() {
    let dir = tempfile::tempdir().unwrap();
    carpeta_de_la_sede(dir.path());
    escribir(
        dir.path(),
        "SEDE/Notificaciones/2026-07-06 Censo 2666000000001/036.pdf",
        &censal_036("06/07/2026"),
    );
    let r = revisar(dir.path());
    // El de estar al corriente es del 10/10/2025 y vale doce meses (lo dice él): caducó.
    let h = hallazgo(&r, "aeat.corriente_caducado");
    assert!(h.titulo.contains("caducó el 10/10/2026"), "{}", h.titulo);
    assert_eq!(h.area, revision::Area::Aeat);
    // El de situación censal vale mientras no cambien sus datos, y después hay un 036.
    let h = hallazgo(&r, "aeat.censal_posterior");
    assert!(h.titulo.contains("15/04/2026"), "{}", h.titulo);
    assert!(h.detalle.contains("06/07/2026"), "{}", h.detalle);
    let t = tabla(&r, "Certificados de la AEAT");
    let fila = |c: &str| {
        t.filas
            .iter()
            .find(|f| f[0] == c)
            .unwrap_or_else(|| panic!("falta {c}: {:?}", t.filas))
    };
    let corriente = fila("Estar al corriente");
    assert_eq!(corriente[1], "10/10/2025");
    assert_eq!(corriente[2], "positivo");
    assert!(corriente[3].contains("10/10/2026"), "{corriente:?}");
    let censo = fila("Situación censal");
    assert!(
        censo[2].contains("111") && censo[2].contains("303"),
        "{censo:?}"
    );
    assert!(censo[3].contains("mientras no cambien"), "{censo:?}");
}

/// El certificado de estar al corriente de la Sede (positivo y de los últimos doce meses) dice
/// lo que cubre en el texto de los cargos de Hacienda que no son de ningún modelo, igual que el
/// leído con IA. Uno caducado, no.
#[test]
fn el_certificado_de_la_sede_vale_para_los_cargos_sin_modelo() {
    let extracto = "{1:F01DEMOESMMAXXX0000000000}{4:\n:20:DEMO\n:25:ES9121000418450200051332\n\
                    :28C:1/1\n:60F:C260101EUR10000,00\n:61:260305D250,00NTRFNONREF\n\
                    :86:AEAT FRACCIONAMIENTO PLAZO 1\n:62F:C260331EUR9750,00\n-}";
    let con_certificado = |fecha: &str| {
        let dir = tempfile::tempdir().unwrap();
        escribir(
            dir.path(),
            "SEDE/Certificados/corriente.pdf",
            &certificado_corriente(fecha, "CORR1234DEMO0009"),
        );
        escribir(dir.path(), "BANCO/2026.sta", extracto.as_bytes());
        revisar(dir.path())
    };
    let r = con_certificado("2 de marzo de 2026");
    let h = hallazgo(&r, "cruces.cargos_sin_modelo");
    assert!(
        h.detalle
            .contains("al corriente con la AEAT del 02/03/2026"),
        "{}",
        h.detalle
    );
    assert!(h.detalle.contains("art. 75.2 RGAT"), "{}", h.detalle);
    let r = con_certificado("1 de septiembre de 2025");
    let h = hallazgo(&r, "cruces.cargos_sin_modelo");
    assert!(h.detalle.contains("recargos, sanciones"), "{}", h.detalle);
}

#[test]
fn el_requerimiento_con_sus_contestaciones_y_la_prescripcion() {
    let dir = tempfile::tempdir().unwrap();
    carpeta_de_la_sede(dir.path());
    let r = revisar(dir.path());
    let t = tabla(&r, "Requerimientos de la AEAT");
    assert_eq!(t.filas.len(), 1, "{:?}", t.filas);
    let f = &t.filas[0];
    assert_eq!(f[0], "I123452025000123");
    assert_eq!(f[1], "la Inspección");
    assert_eq!(f[2], "03/03/2025");
    // Notificado el día de la puesta a disposición en la DEHú (la diligencia, por su número de
    // notificación).
    assert_eq!(f[3], "03/03/2025");
    assert_eq!(f[5], "2020, 2021, 2022, 2023");
    assert!(f[7].contains("14/03/2025 (RGE000000012025)"), "{f:?}");
    // Puede haber interrumpido la prescripción: los cuatro años vuelven a empezar.
    let h = hallazgo(&r, "aeat.requerimiento");
    assert_eq!(h.gravedad, revision::Gravedad::Aviso);
    assert!(h.detalle.contains("art. 68.1.a LGT"), "{}", h.detalle);
    assert!(h.detalle.contains("hasta el 03/03/2029"), "{}", h.detalle);
    assert!(h.detalle.contains("14/03/2029"), "{}", h.detalle);
    assert!(
        !r.hallazgos
            .iter()
            .any(|h| h.codigo == "aeat.requerimiento_sin_contestar"),
        "{:?}",
        r.hallazgos
    );

    // Sin la contestación en la carpeta, se pide.
    let dir = tempfile::tempdir().unwrap();
    carpeta_de_la_sede(dir.path());
    std::fs::remove_dir_all(
        dir.path()
            .join("SEDE/Requerimiento I123452025000123/Contestación RGE000000012025"),
    )
    .unwrap();
    let r = revisar(dir.path());
    let h = hallazgo(&r, "aeat.requerimiento_sin_contestar");
    assert!(
        h.titulo.contains("la Inspección del 03/03/2025"),
        "{}",
        h.titulo
    );
}

#[test]
fn las_copias_en_los_expedientes_de_la_sede_solo_se_dicen() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let declaracion = pdf(&["Modelo 200 de 2024 de Empresa Demo SL, copia de la declaración."]);
    escribir(root, "MODELOS/2024/200 2024 DEMO.pdf", &declaracion);
    escribir(
        root,
        "SEDE/Expedientes/2025-08-14 IS Devolución 2024 202420012345678G/Declaración presentada.pdf",
        &declaracion,
    );
    // Una copia de una factura fuera de la Sede sigue siendo un aviso.
    let factura = pdf(&["Factura F-1 de Proveedor Demo SL. Total 121,00 EUR."]);
    escribir(root, "FACTURAS/f1.pdf", &factura);
    escribir(root, "FACTURAS/copia de f1.pdf", &factura);
    let r = revisar(root);
    let h = hallazgo(&r, "ficheros.copias_sede");
    assert_eq!(h.gravedad, revision::Gravedad::Info);
    assert!(
        h.ficheros
            .iter()
            .any(|f| f.ends_with("Declaración presentada.pdf"))
    );
    let h = hallazgo(&r, "ficheros.copias");
    assert!(
        h.ficheros.iter().all(|f| f.starts_with("FACTURAS/")),
        "{:?}",
        h.ficheros
    );
}

/// Un extracto MT940 con abonos: (aammdd, importe, concepto).
fn mt940(cuenta: &str, desde: &str, hasta: &str, abonos: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut saldo: i64 = 1_000_000;
    let mut s = format!(
        "{{1:F01DEMOESMMAXXX0000000000}}{{4:\n:20:DEMO\n:25:{cuenta}\n:28C:1/1\n:60F:C{desde}EUR10000,00\n"
    );
    for (f, imp, concepto) in abonos {
        saldo += imp.replace(',', "").parse::<i64>().unwrap();
        s.push_str(&format!(":61:{f}C{imp}NTRFNONREF\n:86:{concepto}\n"));
    }
    s.push_str(&format!(
        ":62F:C{hasta}EUR{},{:02}\n-}}",
        saldo / 100,
        saldo % 100
    ));
    s.into_bytes()
}

#[test]
fn las_devoluciones_frente_al_banco() {
    let v = vault();
    v.set_setting("taxpayer_profile", &TaxpayerProfile::sociedad_limitada())
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let modelo = |m: &str, y: i32, p: &str, casillas: serde_json::Value, presentado: &str| {
        json!({
            "tipo": "modelo_aeat",
            "descripcion": format!("Modelo {m} {p} {y}"),
            "modelo": {"modelo": m, "ejercicio": y, "periodo": p, "casillas": casillas,
                       "presentado": presentado}
        })
    };
    let lecturas = vec![
        // Cobrada a los tres meses.
        (
            "MODELOS/200-2023.pdf",
            modelo(
                "200",
                2023,
                "0A",
                json!({"01586": "-1234.56"}),
                "2024-07-20",
            ),
        ),
        // A devolver (aplica todo lo pendiente) y cobrada después de los seis meses, con algo
        // de intereses.
        (
            "MODELOS/303-4T-2024.pdf",
            modelo(
                "303",
                2024,
                "4T",
                json!({"71": "-500.00", "78": "120.00", "110": "120.00"}),
                "2025-01-25",
            ),
        ),
        // Sin abono en los extractos, que cubren esas fechas.
        (
            "MODELOS/200-2024.pdf",
            modelo("200", 2024, "0A", json!({"01586": "-800.00"}), "2025-07-21"),
        ),
    ];
    let mut paquete = Vec::new();
    for (ruta, l) in &lecturas {
        escribir(root, ruta, &pdf(&[&format!("Documento {ruta}")]));
        paquete.push(json!({"fichero": ruta, "lectura": l}));
    }
    std::fs::write(
        root.join("taxpain-lecturas.json"),
        serde_json::to_vec(
            &json!({"version": 1, "generado_por": "Claude Code", "lecturas": paquete}),
        )
        .unwrap(),
    )
    .unwrap();
    escribir(
        root,
        "BANCO/cuenta.sta",
        &mt940(
            "ES9121000418450200051332",
            "240701",
            "260630",
            &[
                (
                    "241015",
                    "1234,56",
                    "TRANSFERENCIA AEAT DEVOLUCION SOCIEDADES",
                ),
                ("250902", "503,10", "TRANSFERENCIA AEAT DEVOLUCION IVA"),
            ],
        ),
    );
    let r = revision::revisar(&v, root, date!(2026 - 10 - 31)).unwrap();
    let t = tabla(&r, "Devoluciones pedidas");
    let estado = |p: &str| {
        t.filas
            .iter()
            .find(|f| f[1] == p)
            .map(|f| f[6].clone())
            .unwrap_or_else(|| panic!("falta {p}: {:?}", t.filas))
    };
    assert_eq!(estado("2023"), "cobrada");
    assert_eq!(estado("4T 2024"), "cobrada después de los seis meses");
    assert_eq!(estado("2024"), "no se ve el abono");
    let h = hallazgo(&r, "aeat.devolucion_tarde");
    assert!(h.titulo.contains("303 del 4T 2024"), "{}", h.titulo);
    assert!(h.detalle.contains("30/07/2025"), "{}", h.detalle);
    assert!(h.detalle.contains("3,10 €"), "{}", h.detalle);
    assert!(h.detalle.contains("art. 31.2 LGT"), "{}", h.detalle);
    let h = hallazgo(&r, "aeat.devolucion_no_esta");
    assert!(h.titulo.contains("200 de 2024"), "{}", h.titulo);
}

fn escanear(ficheros: &[(&str, Vec<u8>)]) -> carpeta::Inventory {
    let v = vault();
    let dir = tempfile::tempdir().unwrap();
    for (ruta, bytes) in ficheros {
        escribir(dir.path(), ruta, bytes);
    }
    carpeta::scan(&v, dir.path()).unwrap()
}

#[test]
fn una_factura_que_cita_el_codigo_de_un_justificante_es_una_factura() {
    let factura = |agencia: &str| {
        pdf(&[
            "ASESORES DEMO SL",
            &format!("CIF {GESTORIA}"),
            "Factura nº: 2025-0042",
            "Fecha: 15/04/2025",
            &format!("Cliente: EMPRESA DEMO SL NIF {PROPIA}"),
            &format!(
                "Presentación del modelo 303 del 1T 2025 ante la {agencia} (código seguro de \
                 verificación del justificante: ABCD1234EFGH5678)"
            ),
            "Base imponible 100,00",
            "IVA 21% 21,00",
            "Total factura 121,00",
        ])
    };
    let inv = escanear(&[
        ("GASTOS/f1.pdf", factura("Agencia Tributaria")),
        (
            "GASTOS/f2.pdf",
            factura("Agencia Estatal de Administración Tributaria"),
        ),
    ]);
    for ruta in ["GASTOS/f1.pdf", "GASTOS/f2.pdf"] {
        let f = tipo(&inv, ruta);
        assert!(
            matches!(&f, Found::FacturaRecibida { number: Some(n), total: Some(_), .. } if n == "2025-0042"),
            "{ruta}: {f:?}"
        );
    }
}

#[test]
fn un_certificado_de_la_agencia_de_una_comunidad_no_es_de_la_aeat() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    escribir(
        root,
        "SEDE/corriente AEAT.pdf",
        &certificado_corriente("1 de octubre de 2025", "CORR1234DEMO0001"),
    );
    escribir(
        root,
        "SEDE/corriente Generalitat.pdf",
        &pdf(&[
            "AGÈNCIA TRIBUTÀRIA VALENCIANA",
            "CERTIFICADO DE ESTAR AL CORRIENTE DE LAS OBLIGACIONES TRIBUTARIAS",
            &format!("N.I.F.: {PROPIA} RAZÓN SOCIAL: EMPRESA DEMO SL"),
            "La Agencia Tributaria Valenciana CERTIFICA que la entidad se encuentra al corriente \
             de sus obligaciones tributarias con la Generalitat.",
            "CIUDAD DEMO, 10 de marzo de 2026",
            "Código Seguro de Verificación: ATVA1234DEMO5678",
        ]),
    );
    let inv = carpeta::scan(&vault(), root).unwrap();
    let f = tipo(&inv, "SEDE/corriente Generalitat.pdf");
    assert!(es(&f, DocKind::CertificadoOtraAdministracion), "{f:?}");
    let r = revisar(root);
    // El de la AEAT es el último de la AEAT: caducó a los doce meses.
    let h = hallazgo(&r, "aeat.corriente_caducado");
    assert!(h.titulo.contains("caducó el 01/10/2026"), "{}", h.titulo);
    let t = tabla(&r, "Certificados de la AEAT");
    assert_eq!(t.filas.len(), 1, "{:?}", t.filas);
    assert_eq!(t.filas[0][1], "01/10/2025");
}

#[test]
fn una_factura_sin_numero_ni_total_que_dice_serlo_sigue_siendo_una_factura() {
    let emitida = |cliente: &str| {
        pdf(&[
            "FACTURA",
            &format!("EMPRESA DEMO SL · {PROPIA}"),
            &format!("{cliente} CLIENTE DEMO SL · {GESTORIA}"),
            "Desarrollo de la web 1 1.000,00",
            "IVA 21 %",
        ])
    };
    let inv = escanear(&[
        ("INGRESOS/2025/factura_2025_0004.pdf", emitida("Cliente:")),
        ("INGRESOS/2025/factura_2025_0005.pdf", emitida("Para:")),
        // Con sólo el NIF de la empresa no se sabe quién la emite: no es una factura emitida.
        (
            "GASTOS/2025/recibo anuncios.pdf",
            pdf(&[
                "Receipt for EMPRESA DEMO SL",
                &format!("VAT: ES{PROPIA}"),
                "Ads invoice for the month",
            ]),
        ),
        // Ni con un código de artículo que pasa por NIF: el cliente no se dice.
        (
            "GASTOS/2025/cdn.pdf",
            pdf(&[
                "Invoice",
                "Customer Name: EMPRESA DEMO SL",
                &format!("CIF: ES{PROPIA}"),
                &format!("Item {GESTORIA} 100GB Recurring"),
            ]),
        ),
    ]);
    for ruta in [
        "INGRESOS/2025/factura_2025_0004.pdf",
        "INGRESOS/2025/factura_2025_0005.pdf",
    ] {
        let f = tipo(&inv, ruta);
        assert!(matches!(f, Found::FacturaEmitida { .. }), "{ruta}: {f:?}");
    }
    for ruta in ["GASTOS/2025/recibo anuncios.pdf", "GASTOS/2025/cdn.pdf"] {
        let f = tipo(&inv, ruta);
        assert!(matches!(f, Found::Otro { .. }), "{ruta}: {f:?}");
    }
}

#[test]
fn sin_la_notificacion_el_plazo_del_requerimiento_es_aproximado() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    escribir(
        root,
        "SEDE/Requerimiento/Requerimiento.pdf",
        &requerimiento_con(
            "Justificación de los ingresos del año 2023",
            Some("CIUDAD DEMO, 02 de marzo de 2026"),
        ),
    );
    escribir(
        root,
        "SEDE/Requerimiento/Contestación/Recibo.pdf",
        &recibo(
            "RGE000000012026",
            "18-03-2026",
            "APORTAR DOCUMENTACION",
            "RECI1234DEMO0001",
        ),
    );
    let r = revisar(root);
    let t = tabla(&r, "Requerimientos de la AEAT");
    let f = &t.filas[0];
    // Contando desde la firma, el plazo acabaría el 16/03/2026; pero no se sabe cuándo se
    // notificó (hay diez días para acceder a la notificación).
    assert_eq!(f[3], "", "{f:?}");
    assert_eq!(f[4], "hacia el 26/03/2026", "{f:?}");
    assert!(f[7].contains("18/03/2026"), "{f:?}");
    assert!(!f[7].contains("fuera de plazo"), "{f:?}");
}

#[test]
fn un_requerimiento_sin_fecha_y_sin_contestacion_se_dice() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    escribir(
        root,
        "SEDE/Requerimiento.pdf",
        &requerimiento_con("Documentación de los ejercicios 2022 a 2023", None),
    );
    let r = revisar(root);
    let h = hallazgo(&r, "aeat.requerimiento_sin_contestar");
    assert!(h.titulo.contains("(I123452025000123)"), "{}", h.titulo);
    assert!(
        h.detalle.contains("no se ha podido leer su fecha"),
        "{}",
        h.detalle
    );
}

#[test]
fn la_prescripcion_por_ejercicio_y_la_ampliacion_que_no_es_una_contestacion() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    escribir(
        root,
        "SEDE/Requerimiento/Requerimiento.pdf",
        &requerimiento_con(
            "Justificación de los ingresos de los ejercicios 2019, 2020, 2021 y 2022",
            Some("CIUDAD DEMO, 06 de marzo de 2023"),
        ),
    );
    escribir(
        root,
        "SEDE/Requerimiento/Ampliación/Recibo.pdf",
        &recibo_de(
            "RB012 - Solicitud de ampliación de plazo de contestación de requerimientos",
            "RGE000000022023",
            "20-03-2023",
            "SOLICITUD AMPLIACION PLAZO",
            "RECI1234DEMO0002",
        ),
    );
    escribir(
        root,
        "SEDE/Requerimiento/Contestación/Recibo.pdf",
        &recibo(
            "RGE000000032023",
            "11-04-2023",
            "APORTAR DOCUMENTACION",
            "RECI1234DEMO0003",
        ),
    );
    let r = revisar(root);
    let t = tabla(&r, "Requerimientos de la AEAT");
    let f = &t.filas[0];
    assert_eq!(f[5], "2019, 2020, 2021, 2022", "{f:?}");
    assert!(
        f[7].starts_with(
            "11/04/2023 (RGE000000032023); ampliación del plazo pedida el \
                          20/03/2023"
        ),
        "{f:?}"
    );
    // El IS de 2022 prescribe por su plazo ordinario el 25/07/2027, después de lo que alarga el
    // requerimiento (11/04/2027): sólo 2019 a 2021 siguen abiertos por él.
    let h = hallazgo(&r, "aeat.requerimiento");
    assert!(
        h.titulo.ends_with("2019 a 2021 pueden seguir abiertos"),
        "{}",
        h.titulo
    );
    assert!(h.detalle.contains("hasta el 06/03/2027"), "{}", h.detalle);
    assert!(h.detalle.contains("11/04/2027"), "{}", h.detalle);
    assert!(
        h.detalle.contains("De 2022 no cambia nada"),
        "{}",
        h.detalle
    );
    assert!(h.detalle.contains("25/07/2027"), "{}", h.detalle);
    assert!(!h.detalle.contains("20/03/2023"), "{}", h.detalle);
}

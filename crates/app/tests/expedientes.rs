//! Expedientes de una SL inventada (Estudio Demo SL, B12345674, en la Comunidad de Madrid): alta
//! a mano y desde una notificación del buzón, plazos con los festivos del perfil, actuaciones
//! que cambian el estado, recargos y reducciones, escritos en RTF y PDF, avisos del Mac y del
//! Panel y certificados con caducidad (G26 y G27).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::expedientes::certificados::{self, CertificadoAdmin, TipoCertificado};
use taxpain_app::expedientes::escritos::{self, Escrito, Opciones};
use taxpain_app::expedientes::{
    self, Actuacion, DocAportar, Estado, Expediente, Organismo, Tipo, TipoActuacion,
};
use taxpain_app::invoicing;
use taxpain_app::notices::{self, NoticeMeta};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::calendar::{Ccaa, TaxpayerProfile};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

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
            address: Some(taxpain_core::invoice::Address {
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
    let mut p = TaxpayerProfile::sociedad_limitada();
    p.ccaa = Some(Ccaa::Madrid);
    p.festivos_locales = vec![date!(2026 - 11 - 09)];
    v.set_setting("taxpayer_profile", &p).unwrap();
    v
}

fn now() -> time::OffsetDateTime {
    datetime!(2026-11-16 10:00 UTC)
}

fn requerimiento() -> Expediente {
    Expediente {
        organismo: Organismo::Aeat,
        organo: "Dependencia Regional de Inspección".into(),
        referencia: "2026INS00000001".into(),
        tipo: Tipo::Requerimiento,
        asunto: "Requerimiento de información".into(),
        concepto: "IVA 2025".into(),
        notificado_el: Some(date!(2026 - 11 - 23)),
        documentos: vec![
            DocAportar {
                descripcion: "Libro registro de facturas recibidas de 2025".into(),
                listo: true,
            },
            DocAportar {
                descripcion: "Contratos con proveedores".into(),
                listo: false,
            },
        ],
        ..Expediente::default()
    }
}

#[test]
fn requerimiento_con_plazos_en_madrid_actuacion_y_escrito() {
    let v = setup();
    let id = expedientes::create(&v, &requerimiento(), now()).unwrap();
    let hoy = date!(2026 - 11 - 30);
    let l = expedientes::list(&v, hoy).unwrap();
    assert_eq!(l.len(), 1);
    let c = &l[0].calculo;
    // Madrid: el 7 y el 8 de diciembre son festivos: 10 días hábiles acaban el 9/12.
    assert_eq!(c.plazos[0].clave, "contestar");
    assert_eq!(c.plazos[0].vence, date!(2026 - 12 - 09));
    assert!(c.festivos_completos);
    // El próximo es pedir la ampliación (tres días antes).
    let p = c.proximo.as_ref().unwrap();
    assert_eq!(p.clave, "pedir_ampliacion");
    assert_eq!(p.vence, date!(2026 - 12 - 06));
    assert_eq!(c.dias, Some(6));

    // Avisos del Panel (≤ 15 días) y del Mac (a 7, 3, 1 y 0 días).
    assert_eq!(expedientes::avisos(&v, hoy).unwrap().len(), 1);
    assert!(expedientes::reminders_due(&v, hoy).unwrap().is_empty());
    let r = expedientes::reminders_due(&v, date!(2026 - 12 - 03)).unwrap();
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].body.contains("2026INS00000001"));

    // El escrito de contestación lleva los datos y la relación de documentos.
    let paras =
        escritos::paragraphs(&v, id, Escrito::Contestacion, &Opciones::default(), hoy).unwrap();
    let rtf = String::from_utf8(
        escritos::generar(
            &v,
            id,
            Escrito::Contestacion,
            &Opciones::default(),
            hoy,
            false,
            now(),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(rtf.starts_with("{\\rtf1"));
    let texto: String = paras.iter().map(|p| format!("{p:?}\n")).collect();
    assert!(
        texto.contains("Estudio Demo SL, con NIF B12345674"),
        "{texto}"
    );
    assert!(texto.contains("09/12/2026"));
    assert!(texto.contains("Documento n.º 2. Contratos con proveedores [pendiente de reunir]"));
    let pdf = escritos::generar(
        &v,
        id,
        Escrito::Ampliacion,
        &Opciones::default(),
        hoy,
        true,
        now(),
    )
    .unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    // Un recurso no tiene sentido para un requerimiento.
    assert!(
        escritos::paragraphs(
            &v,
            id,
            Escrito::RecursoReposicion,
            &Opciones::default(),
            hoy
        )
        .is_err()
    );

    // Contestar lo deja contestado y ya no corre el plazo.
    expedientes::add_actuacion(
        &v,
        id,
        &Actuacion {
            id: 0,
            fecha: date!(2026 - 12 - 02),
            tipo: TipoActuacion::Contestacion,
            registro: "RGE000000012026".into(),
            descripcion: String::new(),
        },
        now(),
    )
    .unwrap();
    let l = expedientes::list(&v, hoy).unwrap();
    assert_eq!(l[0].expediente.estado, Estado::Contestado);
    assert!(l[0].calculo.proximo.is_none() && l[0].calculo.vencido.is_none());
    assert_eq!(l[0].actuaciones.len(), 1);
    assert!(expedientes::avisos(&v, hoy).unwrap().is_empty());

    // Borrar pide confirmación.
    assert!(expedientes::delete(&v, id, now(), false).is_err());
    expedientes::delete(&v, id, now(), true).unwrap();
    assert!(expedientes::list(&v, hoy).unwrap().is_empty());
}

#[test]
fn festivo_local_del_perfil_alarga_el_plazo() {
    let v = setup();
    // Notificado el viernes 6/11/2026; el lunes 9/11 es festivo local en el perfil.
    let e = Expediente {
        notificado_el: Some(date!(2026 - 11 - 06)),
        plazo_dias: Some(1),
        ..requerimiento()
    };
    let c = expedientes::preview(&v, &e, date!(2026 - 11 - 06)).unwrap();
    assert_eq!(c.plazos[0].vence, date!(2026 - 11 - 10));
}

#[test]
fn liquidacion_con_recargo_y_sancion_con_reducciones() {
    let v = setup();
    let liq = Expediente {
        tipo: Tipo::Liquidacion,
        referencia: "A2860026506000001".into(),
        notificado_el: Some(date!(2026 - 10 - 15)),
        importe: Some(Money::from_euros(2_000, 0)),
        ..Expediente::default()
    };
    expedientes::create(&v, &liq, now()).unwrap();
    let sancion = Expediente {
        tipo: Tipo::AcuerdoSancionador,
        referencia: "A2860026506000002".into(),
        notificado_el: Some(date!(2026 - 11 - 02)),
        sancion: Some(Money::from_euros(1_000, 0)),
        ..Expediente::default()
    };
    expedientes::create(&v, &sancion, now()).unwrap();
    let l = expedientes::list(&v, date!(2026 - 11 - 23)).unwrap();
    let liq = l
        .iter()
        .find(|x| x.expediente.tipo == Tipo::Liquidacion)
        .unwrap();
    // Pago en voluntario hasta el 20/11 (art. 62.2); reposición hasta el lunes 16/11.
    assert_eq!(liq.calculo.plazos[0].vence, date!(2026 - 11 - 20));
    assert_eq!(liq.calculo.plazos[1].vence, date!(2026 - 11 - 16));
    // El 23/11 ya ha pasado todo: recargo ejecutivo del 5 % si se paga hoy.
    assert!(liq.calculo.proximo.is_none());
    assert!(liq.calculo.vencido.is_some());
    let hoy = liq.calculo.recargo.iter().find(|e| e.hoy).unwrap();
    assert_eq!(hoy.porcentaje, 5);
    assert_eq!(hoy.total, Money::from_euros(2_100, 0));
    let s = l
        .iter()
        .find(|x| x.expediente.tipo == Tipo::AcuerdoSancionador)
        .unwrap();
    let r = s.calculo.reducciones.as_ref().unwrap();
    assert_eq!(r.ambas, Money::from_euros(420, 0));
    // Recurso hasta el 2/12; pago hasta el 21/12 (el 20 es domingo).
    assert_eq!(s.calculo.proximo.as_ref().unwrap().clave, "reposicion");
    let rtf = String::from_utf8(
        escritos::generar(
            &v,
            s.expediente.id,
            Escrito::Aplazamiento,
            &Opciones {
                motivo: "un cobro retrasado de un cliente".into(),
                plazos: Some(6),
                lugar: "Madrid".into(),
            },
            date!(2026 - 11 - 23),
            false,
            now(),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(rtf.contains("FRACCIONAMIENTO"));
    assert!(rtf.contains("6 plazos mensuales"));
}

#[test]
fn providencia_de_apremio_al_10_y_al_20() {
    let v = setup();
    let e = Expediente {
        tipo: Tipo::ProvidenciaApremio,
        notificado_el: Some(date!(2026 - 11 - 02)),
        importe: Some(Money::from_euros(1_000, 0)),
        fin_voluntario: Some(date!(2026 - 06 - 22)),
        ..Expediente::default()
    };
    let c = expedientes::preview(&v, &e, date!(2026 - 11 - 16)).unwrap();
    assert_eq!(c.plazos[0].vence, date!(2026 - 11 - 20));
    assert_eq!(c.recargo.iter().find(|e| e.hoy).unwrap().porcentaje, 10);
    let c = expedientes::preview(&v, &e, date!(2026 - 12 - 01)).unwrap();
    let hoy = c.recargo.iter().find(|e| e.hoy).unwrap();
    assert_eq!(hoy.porcentaje, 20);
    assert!(hoy.intereses > Money::ZERO);
}

#[test]
fn notificacion_de_otro_buzon_sin_abrir() {
    let v = setup();
    // DEHú, puesta a disposición el 28/7/2026: se da por notificada el 7/8 (agosto cuenta).
    let e = Expediente {
        organismo: Organismo::Dehu,
        tipo: Tipo::Requerimiento,
        asunto: "Requerimiento del ayuntamiento (inventado)".into(),
        puesto_el: Some(date!(2026 - 07 - 28)),
        ..Expediente::default()
    };
    let c = expedientes::preview(&v, &e, date!(2026 - 07 - 30)).unwrap();
    assert!(c.presunta);
    assert_eq!(c.notificado, Some(date!(2026 - 08 - 07)));
    // 10 días hábiles desde el 7/8: el 15/8 es sábado; 10, 11, 12, 13, 14, 17-21.
    assert_eq!(c.plazos[0].vence, date!(2026 - 08 - 21));
    assert!(c.avisos.iter().any(|a| a.contains("Ley 39/2015")));
    // Sin fecha alguna no se guarda.
    let mal = Expediente {
        puesto_el: None,
        ..e
    };
    assert!(expedientes::create(&v, &mal, now()).is_err());
}

#[test]
fn desde_una_notificacion_del_buzon() {
    let v = setup();
    notices::record_listed(
        &v,
        false,
        &[NoticeMeta {
            id: "0000000000001".into(),
            kind: "N".into(),
            state: "P".into(),
            role: "T".into(),
            subject: "Propuesta de liquidación provisional".into(),
            available_at: "2026-11-10T09:00:00".into(),
            holder_nif: "B12345674".into(),
        }],
        now(),
    )
    .unwrap();
    let e = expedientes::desde_notificacion(&v, "0000000000001").unwrap();
    assert_eq!(e.tipo, Tipo::PropuestaLiquidacion);
    assert_eq!(e.puesto_el, Some(date!(2026 - 11 - 10)));
    assert_eq!(e.notificado_el, None);
    let id = expedientes::create(&v, &e, now()).unwrap();
    let l = expedientes::list(&v, date!(2026 - 11 - 16)).unwrap();
    assert_eq!(l[0].expediente.id, id);
    assert_eq!(l[0].expediente.notice_id.as_deref(), Some("0000000000001"));
}

#[test]
fn certificados_que_caducan() {
    let v = setup();
    let hoy = date!(2026 - 11 - 16);
    let id = certificados::save(
        &v,
        &CertificadoAdmin {
            id: 0,
            tipo: TipoCertificado::CorrienteAeat,
            descripcion: "Para una subvención (inventada)".into(),
            emitido_el: Some(date!(2025 - 11 - 26)),
            caduca_el: Some(date!(2026 - 11 - 26)),
            document_id: None,
            notas: String::new(),
        },
        now(),
    )
    .unwrap();
    certificados::save(
        &v,
        &CertificadoAdmin {
            id: 0,
            tipo: TipoCertificado::Apoderamiento,
            descripcion: "Apoderamiento a la asesoría".into(),
            emitido_el: None,
            caduca_el: Some(date!(2027 - 06 - 30)),
            document_id: None,
            notas: String::new(),
        },
        now(),
    )
    .unwrap();
    let l = certificados::list(&v, hoy).unwrap();
    assert_eq!(l[0].estado, "caduca_pronto");
    assert_eq!(l[1].estado, "vigente");
    let a = expedientes::avisos(&v, hoy).unwrap();
    assert_eq!(a.len(), 1);
    assert!(a[0].texto.contains("26/11/2026"));
    // A 15 días avisa el Mac.
    let r = expedientes::reminders_due(&v, date!(2026 - 11 - 11)).unwrap();
    assert_eq!(r.len(), 1);
    // Sin caducidad no se guarda.
    let mut sin = l[0].certificado.clone();
    sin.id = 0;
    sin.caduca_el = None;
    assert!(certificados::save(&v, &sin, now()).is_err());
    certificados::delete(&v, id, now(), true).unwrap();
    assert_eq!(certificados::list(&v, hoy).unwrap().len(), 1);
}

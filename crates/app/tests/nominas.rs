//! Nóminas calculadas: ficha, cálculo y registro del recibo, recibo en PDF, seguros sociales del
//! mes con su pago y finiquito. Cifras inventadas.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::journal;
use taxpain_app::invoicing;
use taxpain_app::nominas::{
    FichaNomina, PagoSegurosInput, RegistroNomina, calculadas, calcular, calcular_finiquito, ficha,
    pagar_seguros, recibo_pdf, registrar, save_ficha, seguros_sociales,
};
use taxpain_app::payroll::{self, Employee, PayslipInput, payslips_of_year, save_employee};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::accounting::Origin;
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_core::nomina::finiquito::DatosFiniquito;
use taxpain_core::nomina::{ClaseConcepto, ClaveCuota, Concepto, Contrato, PagasExtra};
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
    v
}

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn ficha_demo(employee_id: i64) -> FichaNomina {
    FichaNomina {
        employee_id,
        naf: "46/12345678/90".into(),
        categoria: "Oficial administrativo".into(),
        ccc: "46/1234567/89".into(),
        antiguedad: Some(date!(2025 - 03 - 01)),
        cnae: "7111".into(),
        grupo: 5,
        contrato: Contrato::Indefinido,
        jornada: Rate::percent(100),
        salario_base: eur(1800),
        complementos: vec![Concepto {
            nombre: "Plus de convenio".into(),
            importe: eur(200),
            clase: ClaseConcepto::Salarial,
        }],
        pagas: PagasExtra {
            numero: 2,
            importe: eur(1800),
            prorrateadas: false,
            en_este_mes: 0,
        },
        at_it: Rate::from_bp(65),
        at_ims: Rate::from_bp(35),
        tipo_irpf: Rate::percent(12),
    }
}

#[test]
fn nomina_calculada_recibo_y_seguros_sociales() {
    let v = setup();
    let now = datetime!(2026-11-02 10:00 UTC);
    let id = save_employee(
        &v,
        &Employee {
            nif: "12345678Z".into(),
            name: "Española Española Carmen".into(),
            province: "46".into(),
            birth_year: 1990,
            family: 3,
            contract: 1,
            active: true,
            ..Default::default()
        },
        now,
    )
    .unwrap();
    assert_eq!(ficha(&v, id).unwrap(), None);
    let f = ficha_demo(id);
    save_ficha(&v, &f, now).unwrap();
    assert_eq!(ficha(&v, id).unwrap(), Some(f.clone()));

    let datos = f.datos_del_mes(2026, 10);
    let n = calcular(&datos).unwrap();
    assert_eq!(n.base_comunes, eur(2300));
    // 108,10 + 3,45 + 35,65 + 2,30.
    assert_eq!(n.total_trabajador, Money::from_cents(14950));
    let slip = registrar(
        &v,
        &RegistroNomina {
            employee_id: id,
            fecha: date!(2026 - 10 - 31),
            datos: datos.clone(),
        },
        now,
    )
    .unwrap();
    // Otra del mismo mes y trabajador: no.
    assert!(
        registrar(
            &v,
            &RegistroNomina {
                employee_id: id,
                fecha: date!(2026 - 10 - 31),
                datos: datos.clone(),
            },
            now,
        )
        .is_err()
    );
    assert_eq!(calculadas(&v, 2026).unwrap(), vec![slip]);
    let stored = payslips_of_year(&v, 2026).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].gross, eur(2000));
    assert_eq!(stored[0].ss_employee, n.total_trabajador);
    assert_eq!(stored[0].ss_company, n.total_empresa);
    assert_eq!(stored[0].irpf, eur(240));

    // Un recibo tecleado de otro trabajador en octubre suma sin desglose.
    let otro = save_employee(
        &v,
        &Employee {
            nif: "00000000T".into(),
            name: "Trabajador Inventado".into(),
            province: "46".into(),
            birth_year: 1985,
            family: 3,
            contract: 1,
            active: true,
            ..Default::default()
        },
        now,
    )
    .unwrap();
    payroll::register(
        &v,
        &PayslipInput {
            employee_id: otro,
            date: date!(2026 - 10 - 30),
            gross: eur(1500),
            irpf: eur(150),
            ss_employee: eur(100),
            ss_company: eur(500),
            small_entity: None,
        },
        now,
    )
    .unwrap();

    let r = seguros_sociales(&v, 2026, 10).unwrap();
    assert_eq!(r.trabajadores.len(), 2);
    assert_eq!(r.total_trabajador, n.total_trabajador + eur(100));
    assert_eq!(r.total_empresa, n.total_empresa + eur(500));
    assert_eq!(r.total, r.total_trabajador + r.total_empresa);
    let cc = r
        .conceptos
        .iter()
        .find(|c| c.clave == ClaveCuota::Comunes)
        .unwrap();
    assert_eq!(cc.total, Money::from_cents(10810 + 54280));
    // 30/11/2026 es lunes: vence ese día.
    assert_eq!(r.vence, date!(2026 - 11 - 30));
    let haber_476 = r.devengo.iter().find(|l| l.cuenta == "476").unwrap().haber;
    assert_eq!(haber_476, r.total);
    let debe: Money = r.devengo.iter().map(|l| l.debe).sum();
    let haber: Money = r.devengo.iter().map(|l| l.haber).sum();
    assert_eq!(debe, haber);
    assert!(r.pago.is_none());

    // Pago del RLC con confirmación.
    let pago = PagoSegurosInput {
        year: 2026,
        month: 10,
        fecha: date!(2026 - 11 - 30),
        importe: r.total,
        cuenta: "572".into(),
    };
    assert!(pagar_seguros(&v, &pago, false, now).is_err());
    let mal = PagoSegurosInput {
        cuenta: "430".into(),
        ..pago.clone()
    };
    assert!(pagar_seguros(&v, &mal, true, now).is_err());
    let entry = pagar_seguros(&v, &pago, true, now).unwrap();
    assert!(
        pagar_seguros(&v, &pago, true, now).is_err(),
        "una vez por mes"
    );
    let e = journal(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|e| e.id == entry)
        .unwrap();
    assert_eq!(e.entry.origin, Origin::Nomina);
    assert_eq!(e.entry.concept, "Seguros sociales de octubre de 2026");
    let l476 = e
        .entry
        .lines
        .iter()
        .find(|l| l.account.as_str() == "476")
        .unwrap();
    assert_eq!(l476.debit, r.total);
    let r = seguros_sociales(&v, 2026, 10).unwrap();
    assert_eq!(r.pago.unwrap().entry_id, Some(entry));

    // Recibo de salarios en PDF.
    let (name, pdf) = recibo_pdf(&v, slip).unwrap();
    assert_eq!(name, "Recibo de salarios 12345678Z 2026-10.pdf");
    assert!(pdf.starts_with(b"%PDF"));
    // El recibo tecleado no tiene recibo calculado.
    let tecleado = payslips_of_year(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|p| p.employee_id == otro)
        .unwrap();
    assert!(recibo_pdf(&v, tecleado.id).is_err());

    // Al borrar el recibo se va su cálculo.
    payroll::delete(&v, slip, now, true).unwrap();
    assert!(calculadas(&v, 2026).unwrap().is_empty());
}

#[test]
fn administrador_no_y_finiquito() {
    let v = setup();
    let now = datetime!(2026-11-02 10:00 UTC);
    let admin = save_employee(
        &v,
        &Employee {
            nif: "12345678Z".into(),
            name: "Española Española Carmen".into(),
            province: "46".into(),
            active: true,
            administrator: true,
            ..Default::default()
        },
        now,
    )
    .unwrap();
    assert!(save_ficha(&v, &ficha_demo(admin), now).is_err());
    let mut mala = ficha_demo(admin);
    mala.grupo = 0;
    assert!(save_ficha(&v, &mala, now).is_err());

    let f = calcular_finiquito(&DatosFiniquito {
        alta: date!(2026 - 01 - 01),
        baja: date!(2026 - 06 - 30),
        salario_mensual: eur(1500),
        dias_pendientes: 30,
        vacaciones_anuales: 30,
        vacaciones_disfrutadas: 0,
        pagas: vec![],
        fin_temporal: true,
        dias_por_ano: 12,
    })
    .unwrap();
    assert_eq!(f.total, f.sujeto + f.indemnizacion);
    assert!(f.indemnizacion > Money::ZERO);
}

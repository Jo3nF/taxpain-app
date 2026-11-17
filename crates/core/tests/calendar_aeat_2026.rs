//! Contraste del motor con el calendario oficial del contribuyente AEAT 2026
//! (`fixtures/calendar/aeat-2026.json`, 99 vencimientos con su URL de origen).

#![allow(clippy::unwrap_used)]

use taxpain_core::calendar::{
    Ccaa, Deadline, IrpfMethod, Periodicity, Persona, TaxpayerProfile, VatProfile, calendar_for,
    deadlines_in_year, official,
};
use time::macros::date;

/// Perfiles que, juntos, cubren todos los modelos del fixture.
fn profiles() -> Vec<TaxpayerProfile> {
    let base = TaxpayerProfile::autonomo();
    vec![
        TaxpayerProfile {
            retiene_trabajo_profesionales: true,
            retiene_alquileres: true,
            retiene_capital_mobiliario: true,
            intracomunitarias: Some(Periodicity::Trimestral),
            obligado_347: true,
            ..base.clone()
        },
        TaxpayerProfile {
            irpf_method: Some(IrpfMethod::Modulos),
            ..base.clone()
        },
        // Mensual implica SII (RIVA art. 62.6): sin 390 ni 347 (los cubre el primer perfil).
        TaxpayerProfile {
            iva: Some(VatProfile {
                periodicity: Periodicity::Mensual,
                sii: true,
            }),
            gran_empresa: true,
            retiene_capital_mobiliario: true,
            intracomunitarias: Some(Periodicity::Mensual),
            ..TaxpayerProfile::sociedad_limitada()
        },
        TaxpayerProfile {
            persona: Persona::EntidadAtribucion,
            irpf_method: None,
            ..base
        },
    ]
}

fn engine_2026(ccaa: Option<Ccaa>) -> Vec<Deadline> {
    let (cal, loaded) = calendar_for(2026, ccaa, &[]);
    assert!(loaded);
    profiles()
        .iter()
        .flat_map(|p| deadlines_in_year(p, 2026, &cal))
        .collect()
}

#[test]
fn todas_las_fechas_oficiales_2026_coinciden() {
    let ours = engine_2026(None);
    let entries = official::entries();
    assert_eq!(entries.len(), 99);
    let mut errors = Vec::new();
    for e in entries {
        let Some(d) = ours.iter().find(|d| {
            d.model.code() == e.model
                && d.fiscal_year == e.fiscal_year
                && d.period.code() == e.period
        }) else {
            errors.push(format!("falta {} {} {}", e.model, e.fiscal_year, e.period));
            continue;
        };
        let id = format!("{} {} {}", e.model, e.fiscal_year, e.period);
        if d.due_on != e.end {
            errors.push(format!("{id}: fin {} ≠ oficial {}", d.due_on, e.end));
        }
        if let Some(start) = e.start.filter(|s| *s != d.starts_on) {
            errors.push(format!("{id}: inicio {} ≠ oficial {start}", d.starts_on));
        }
        if let Some(dom) = e
            .end_domiciliacion
            .filter(|x| d.domiciliation_end != Some(*x))
        {
            errors.push(format!(
                "{id}: domiciliación {:?} ≠ oficial {dom}",
                d.domiciliation_end
            ));
        }
    }
    assert!(
        errors.is_empty(),
        "{} discrepancias:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

#[test]
fn renta_2025_empieza_el_8_de_abril_y_no_es_estimada() {
    let ours = engine_2026(None);
    let renta = ours.iter().find(|d| d.model.code() == "100").unwrap();
    assert_eq!(
        (renta.starts_on, renta.due_on),
        (date!(2026 - 04 - 08), date!(2026 - 06 - 30))
    );
    assert!(!renta.tentative);
    assert_eq!(renta.domiciliation_end, Some(date!(2026 - 06 - 25)));
}

#[test]
fn festivos_autonomicos_trasladan_el_plazo() {
    let find = |ds: &[Deadline], m: &str, p: &str| {
        ds.iter()
            .find(|d| d.model.code() == m && d.period.code() == p && d.fiscal_year == 2026)
            .unwrap()
            .due_on
    };
    let general = engine_2026(None);
    // 303 de enero: vence el lunes 02/03; en Illes Balears es festivo → 03/03.
    assert_eq!(find(&general, "303", "01"), date!(2026 - 03 - 02));
    assert_eq!(
        find(&engine_2026(Some(Ccaa::IllesBalears)), "303", "01"),
        date!(2026 - 03 - 03)
    );
    // 111 de febrero: vence el viernes 20/03; festivo en Melilla → lunes 23/03.
    assert_eq!(find(&general, "111", "02"), date!(2026 - 03 - 20));
    assert_eq!(
        find(&engine_2026(Some(Ccaa::Melilla)), "111", "02"),
        date!(2026 - 03 - 23)
    );
}

#[test]
fn sin_tabla_oficial_la_domiciliacion_es_estimada() {
    let (cal, _) = calendar_for(2027, None, &[]);
    let ds = deadlines_in_year(&TaxpayerProfile::autonomo(), 2027, &cal);
    let d = ds
        .iter()
        .find(|d| d.model.code() == "303" && d.period.code() == "1T")
        .unwrap();
    assert!(!d.domiciliation_official);
    // 20/04/2027 es martes: 15/04 (jueves).
    assert_eq!(d.domiciliation_end, Some(date!(2027 - 04 - 15)));
    let renta = ds.iter().find(|d| d.model.code() == "100").unwrap();
    assert!(
        renta.tentative,
        "la campaña de Renta 2026 aún no tiene fecha oficial"
    );
}

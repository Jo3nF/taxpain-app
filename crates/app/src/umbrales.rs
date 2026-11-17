//! Avisos de umbral con los datos de la app: cuándo una cifra que TaxPain ya conoce cambia las
//! obligaciones (347, 349 mensual, gran empresa y SII, 720, 232, tipo del Impuesto sobre
//! Sociedades, 202 e IAE, patrimonio neto frente al capital) y el perfil fiscal o los ajustes de Modelos aún no lo reflejan.
//!
//! Las cifras salen sólo de lo que hay en TaxPain (facturas emitidas, recibidas revisadas,
//! contabilidad, cuentas bancarias): si falta algo, el aviso puede no salir. Cada aviso dice qué
//! hacer y su norma.

use serde::Serialize;
use taxpain_core::calendar::{Periodicity, Persona, TaxpayerProfile, VatProfile};
use taxpain_core::money::Money;
use taxpain_models::m200::TaxRegime;
use taxpain_vault::Vault;
use time::{Date, Month};

use crate::error::Result;
use crate::models;

/// Umbral del 347 por tercero y año (RGAT art. 31.1).
pub const UMBRAL_347: Money = Money::from_cents(300_506);
/// Entregas y servicios intracomunitarios por trimestre del 349 mensual (RIVA art. 81.2).
pub const UMBRAL_349: Money = Money::from_euros(50_000, 0);
/// Volumen de operaciones de la gran empresa (RIVA art. 71.3; LIVA art. 121).
pub const UMBRAL_GRAN_EMPRESA: Money = Money::from_cents(601_012_104);
/// Saldo en el extranjero que obliga al 720 (RGAT art. 42 bis.4).
pub const UMBRAL_720: Money = Money::from_euros(50_000, 0);
/// Operaciones con una misma persona vinculada que obligan al 232 (RIS art. 13.4; los otros
/// dos umbrales están en `vinculadas`).
pub const UMBRAL_232: Money = crate::vinculadas::UMBRAL_PERSONA;
/// Cifra de negocios: microempresa e IAE (LIS art. 29.1; TRLRHL art. 82.1.c).
pub const UN_MILLON: Money = Money::from_euros(1_000_000, 0);
/// Cifra de negocios del 202 por la base del periodo (LIS art. 40.3).
pub const SEIS_MILLONES: Money = Money::from_euros(6_000_000, 0);
/// Cifra de negocios de la empresa de reducida dimensión (LIS art. 101.1).
pub const DIEZ_MILLONES: Money = Money::from_euros(10_000_000, 0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Hay que hacer algo (presentar, cambiar el perfil).
    Warning,
    /// Conviene revisarlo.
    Info,
}

/// Un aviso de umbral.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ThresholdAlert {
    /// `347`, `349`, `gran_empresa`, `720`, `232`, `is_regimen`, `202`, `iae`,
    /// `causa_disolucion`, `reduccion_obligatoria`, `oss`.
    pub code: &'static str,
    pub severity: Severity,
    pub title: String,
    /// Lo que dicen los datos.
    pub detail: String,
    /// Qué hacer.
    pub action: String,
    pub rule: &'static str,
}

/// Avisos de hoy para la empresa abierta. Sin perfil fiscal no hay nada que comparar.
pub fn alerts(v: &Vault, today: Date) -> Result<Vec<ThresholdAlert>> {
    let Some(p) = v.setting::<TaxpayerProfile>("taxpayer_profile")? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    // Cada comprobación es independiente: si a una le faltan datos (sin facturación
    // configurada, sin ejercicio contable…) se salta y siguen las demás.
    let _ = check_347(v, &p, today, &mut out);
    let _ = check_349(v, &p, today, &mut out);
    let _ = check_gran_empresa(v, &p, today, &mut out);
    let _ = check_720(v, &p, today, &mut out);
    let _ = check_721(v, &p, today, &mut out);
    let _ = check_232(v, &p, today, &mut out);
    let _ = check_cifra_de_negocios(v, &p, today, &mut out);
    let _ = check_patrimonio_neto(v, &p, today, &mut out);
    let _ = check_oss(v, &p, today, &mut out);
    Ok(out)
}

fn sii(p: &TaxpayerProfile) -> bool {
    p.iva.is_some_and(VatProfile::sii)
}

fn quarter_of(d: Date) -> u8 {
    (u8::from(d.month()) - 1) / 3 + 1
}

fn lista(nombres: &[String]) -> String {
    let mut s = nombres
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if nombres.len() > 3 {
        s.push_str(&format!(" y {} más", nombres.len() - 3));
    }
    s
}

/// 347: terceros por encima de 3.005,06 € en el año en curso y, hasta que vence (febrero), en
/// el anterior. Con SII no se presenta (RGAT art. 32.1.e).
fn check_347(
    v: &Vault,
    p: &TaxpayerProfile,
    today: Date,
    out: &mut Vec<ThresholdAlert>,
) -> Result<()> {
    if p.obligado_347 || sii(p) {
        return Ok(());
    }
    let mut years = vec![today.year()];
    if today.month() <= Month::February {
        years.insert(0, today.year() - 1);
    }
    for y in years {
        let r = models::result_347(v, y)?;
        if r.lines.is_empty() {
            continue;
        }
        let nombres: Vec<String> = r.lines.iter().map(|l| l.name.clone()).collect();
        let en_curso = y == today.year();
        out.push(ThresholdAlert {
            code: "347",
            severity: Severity::Warning,
            title: if en_curso {
                format!("Ya superas 3.005,06 € con {} en {y}", plural(r.lines.len()))
            } else {
                format!("Tienes que presentar el 347 de {y}")
            },
            detail: format!(
                "Según las facturas de TaxPain, en {y} las operaciones (IVA incluido) con {} \
                 pasan de 3.005,06 €: {}.",
                plural(r.lines.len()),
                lista(&nombres)
            ),
            action: format!(
                "Marca «Opero con algún cliente o proveedor por más de 3.005,06 €» en el perfil \
                 fiscal: el calendario te avisará del 347 de {y}, que se presenta en febrero de \
                 {}. El borrador está en Modelos.",
                y + 1
            ),
            rule: "RGAT arts. 31 a 33 (RD 1065/2007); Orden EHA/3012/2008 art. 10",
        });
    }
    Ok(())
}

fn plural(n: usize) -> String {
    if n == 1 {
        "1 cliente o proveedor".into()
    } else {
        format!("{n} clientes o proveedores")
    }
}

/// 349: operaciones con la UE sin el 349 en el perfil, o por encima de 50.000 € en el
/// trimestre o en alguno de los cuatro anteriores con el 349 trimestral.
fn check_349(
    v: &Vault,
    p: &TaxpayerProfile,
    today: Date,
    out: &mut Vec<ThresholdAlert>,
) -> Result<()> {
    if p.intracomunitarias == Some(Periodicity::Mensual) {
        return Ok(());
    }
    let (mut y, mut q) = (today.year(), quarter_of(today));
    let mut over = Vec::new();
    let mut any = false;
    for _ in 0..5 {
        let total = models::intra_quarter_total(v, y, q)?;
        any |= !total.is_zero();
        if total > UMBRAL_349 {
            over.push(format!("{q}T {y} ({total})"));
        }
        (y, q) = if q == 1 { (y - 1, 4) } else { (y, q - 1) };
    }
    let rule = "RIVA art. 81.2 (RD 1624/1992); Orden EHA/769/2010";
    if p.intracomunitarias.is_none() && any {
        out.push(ThresholdAlert {
            code: "349",
            severity: Severity::Warning,
            title: "Vendes a empresas de otros países de la UE: te toca el 349".into(),
            detail: "Hay facturas emitidas con entregas o servicios a operadores de la UE en el \
                     último año y el perfil fiscal dice que no haces operaciones intracomunitarias."
                .into(),
            action: if over.is_empty() {
                "En el perfil fiscal, elige «Sí, trimestral» en las operaciones con la UE (349)."
                    .into()
            } else {
                "En el perfil fiscal, elige «Sí, mensual» en las operaciones con la UE (349): \
                 superas 50.000 € en un trimestre."
                    .into()
            },
            rule,
        });
    } else if !over.is_empty() {
        out.push(ThresholdAlert {
            code: "349",
            severity: Severity::Warning,
            title: "El 349 pasa a ser mensual".into(),
            detail: format!(
                "Las entregas y servicios a operadores de la UE superan 50.000 € en {}.",
                over.join(", ")
            ),
            action: "Presenta el 349 cada mes desde el mes en que se superó (hasta el día 20 \
                     del mes siguiente) mientras no pasen cuatro trimestres por debajo, y cambia \
                     el perfil fiscal a «Sí, mensual»."
                .into(),
            rule,
        });
    }
    Ok(())
}

/// Volumen de operaciones aproximado de `year`: bases de las facturas emitidas no anuladas.
pub(crate) fn volumen(v: &Vault, year: i32) -> Result<Money> {
    let cents: i64 = v.connection().query_row(
        "SELECT COALESCE(SUM(base - COALESCE(replaced_base, 0)), 0) FROM invoice_issued
         WHERE status = 'emitida' AND substr(issue_date, 1, 4) = ?1",
        [year.to_string()],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(cents))
}

/// Gran empresa: más de 6.010.121,04 € de volumen de operaciones el año anterior obliga al IVA
/// mensual con SII y a las retenciones mensuales.
fn check_gran_empresa(
    v: &Vault,
    p: &TaxpayerProfile,
    today: Date,
    out: &mut Vec<ThresholdAlert>,
) -> Result<()> {
    let year = today.year();
    let rule = "RIVA arts. 71.3 y 62.6 (RD 1624/1992); LIVA art. 121; RIRPF art. 108.1";
    let mensual = p.iva.is_some_and(|i| i.periodicity == Periodicity::Mensual);
    let previous = volumen(v, year - 1)?;
    if previous > UMBRAL_GRAN_EMPRESA && (!p.gran_empresa || (p.iva.is_some() && !mensual)) {
        let mut action = format!(
            "En el perfil fiscal marca «Gran empresa» y el IVA «Mensual (con SII)». Desde enero \
             de {year} el 303 es mensual (hasta el día 30 del mes siguiente) y los libros de IVA \
             se llevan por el SII (en cuatro días); el 111, el 115 y el 123 son mensuales; ya no \
             se presentan el 390 ni el 347."
        );
        if p.persona == Persona::Juridica {
            action.push_str(
                " El 202 se calcula sobre la base del periodo si la cifra de negocios pasa de \
                 6 millones (art. 40.3 LIS).",
            );
        }
        out.push(ThresholdAlert {
            code: "gran_empresa",
            severity: Severity::Warning,
            title: format!("En {year} eres gran empresa"),
            detail: format!(
                "Las facturas emitidas en {} suman {previous} (sin IVA): más de 6.010.121,04 €.",
                year - 1
            ),
            action,
            rule,
        });
    } else if !p.gran_empresa {
        let current = volumen(v, year)?;
        if current > UMBRAL_GRAN_EMPRESA {
            out.push(ThresholdAlert {
                code: "gran_empresa",
                severity: Severity::Info,
                title: format!("En {} serás gran empresa", year + 1),
                detail: format!(
                    "Las facturas emitidas en {year} ya suman {current} (sin IVA): más de \
                     6.010.121,04 €."
                ),
                action: format!(
                    "Desde enero de {} el IVA será mensual con SII y las retenciones mensuales. \
                     Prepara el alta en el SII (036 en noviembre o, como tarde, antes de \
                     empezar el año) y cambia el perfil fiscal en enero.",
                    year + 1
                ),
                rule,
            });
        }
    }
    Ok(())
}

/// Saldo contable de una subcuenta al final del día `at` (sin los asientos de cierre).
pub(crate) fn saldo(v: &Vault, account: &str, at: Date) -> Result<Money> {
    let cents: i64 = v.connection().query_row(
        "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
         JOIN journal_entry e ON e.id = l.entry_id
         WHERE e.date <= ?1 AND e.origin_kind <> 'Cierre'
           AND (e.origin_kind <> 'Apertura'
                OR e.fiscal_year = (SELECT MIN(fiscal_year) FROM journal_entry))
           AND l.account = ?2",
        [at.to_string(), account.to_owned()],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(cents))
}

/// 720: cuentas con IBAN de otro país cuyo saldo conjunto a 31/12 pasa de 50.000 €.
fn check_720(
    v: &Vault,
    p: &TaxpayerProfile,
    today: Date,
    out: &mut Vec<ThresholdAlert>,
) -> Result<()> {
    if p.bienes_extranjero {
        return Ok(());
    }
    // Las personas jurídicas que registran en su contabilidad cada cuenta del extranjero, con su
    // NIF y su país, no declaran esas cuentas en el 720 (RGAT art. 42 bis.4.a): las de una
    // sociedad con la contabilidad en TaxPain lo están.
    if matches!(p.persona, taxpain_core::calendar::Persona::Juridica) {
        return Ok(());
    }
    let year = today.year() - 1;
    let end = Date::from_calendar_date(year, Month::December, 31)
        .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
    let mut stmt = v.connection().prepare(
        "SELECT iban, account, name FROM bank_account
         WHERE iban IS NOT NULL AND upper(substr(iban, 1, 2)) <> 'ES' AND account <> ''",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut total = Money::ZERO;
    let mut cuentas = Vec::new();
    for row in rows {
        let (iban, account, name) = row?;
        let s = saldo(v, &account, end)?;
        if s > Money::ZERO {
            total += s;
            cuentas.push(format!("{name} ({}…)", iban.get(..4).unwrap_or(&iban)));
        }
    }
    if total <= UMBRAL_720 {
        return Ok(());
    }
    out.push(ThresholdAlert {
        code: "720",
        severity: Severity::Warning,
        title: format!("Cuentas en el extranjero por más de 50.000 € a 31/12/{year}"),
        detail: format!(
            "Las cuentas con IBAN de otro país suman {total} a 31 de diciembre de {year}: {}.",
            lista(&cuentas)
        ),
        action: format!(
            "Presenta el 720 de {year} entre el 1 de enero y el 31 de marzo de {} (también \
             cuenta el saldo medio del último trimestre) y marca la casilla del 720 en el perfil \
             fiscal. Si ya lo presentaste otro año, sólo hay que repetirlo si el saldo ha subido \
             más de 20.000 € o has cancelado alguna cuenta.",
            year + 1
        ),
        rule: "RGAT art. 42 bis (RD 1065/2007); Orden HAP/72/2013",
    });
    Ok(())
}

/// 721: criptomonedas en plataformas del extranjero por más de 50.000 € a 31/12 (con los
/// precios de cierre tecleados o importados) y aviso si faltan precios para valorarlas.
fn check_721(
    v: &Vault,
    p: &TaxpayerProfile,
    today: Date,
    out: &mut Vec<ThresholdAlert>,
) -> Result<()> {
    let year = today.year() - 1;
    let Some(m) = crate::cripto::saldo_extranjero(v, year)? else {
        return Ok(());
    };
    if !m.sin_precio.is_empty() {
        out.push(ThresholdAlert {
            code: "cripto_precios",
            severity: Severity::Info,
            title: format!("Faltan precios de cierre de criptomonedas a 31/12/{year}"),
            detail: format!(
                "Tienes {} en plataformas del extranjero sin precio de cierre: no se puede saber \
                 si los saldos pasan de 50.000 € (721) ni valorar las posiciones.",
                m.sin_precio.join(", ")
            ),
            action: "Añade los precios en Contabilidad → Criptomonedas → «Precios de cierre» \
                     (la cotización a 31/12 de tu plataforma) o importa una hoja de saldos."
                .into(),
            rule: "RGAT art. 42 quater.3.c (RD 1065/2007)",
        });
    }
    if !m.sin_plataforma.is_empty() {
        out.push(ThresholdAlert {
            code: "cripto_sin_plataforma",
            severity: if p.persona == Persona::Fisica {
                Severity::Warning
            } else {
                Severity::Info
            },
            title: format!("Criptomonedas sin plataforma a 31/12/{year}"),
            detail: format!(
                "{} del punto de partida no {} plataforma: no se sabe si están en el extranjero, \
                 así que no cuentan para ver si los saldos pasan de 50.000 € (721).",
                m.sin_plataforma.join(", "),
                if m.sin_plataforma.len() == 1 {
                    "tiene"
                } else {
                    "tienen"
                }
            ),
            action: "Pon en Contabilidad → Criptomonedas → «Punto de partida» la plataforma o el \
                     monedero de cada moneda."
                .into(),
            rule: "RGAT art. 42 quater (RD 1065/2007)",
        });
    }
    if p.monedas_virtuales_extranjero || !m.supera {
        return Ok(());
    }
    out.push(ThresholdAlert {
        code: "721",
        severity: if p.persona == Persona::Fisica {
            Severity::Warning
        } else {
            Severity::Info
        },
        title: format!("Criptomonedas en el extranjero por más de 50.000 € a 31/12/{year}"),
        detail: format!(
            "Los saldos en plataformas del extranjero valorados con los precios de cierre suman \
             {} a 31 de diciembre de {year}: {}.",
            m.total_extranjero,
            lista(
                &m.plataformas
                    .iter()
                    .map(|x| format!("{} ({})", x.platform, x.valor))
                    .collect::<Vec<_>>()
            )
        ),
        action: if p.persona == Persona::Fisica {
            format!(
                "Presenta el 721 de {year} entre el 1 de enero y el 31 de marzo de {} y marca la \
                 casilla del 721 en el perfil fiscal. Los años siguientes sólo si el saldo sube \
                 más de 20.000 €.",
                year + 1
            )
        } else {
            format!(
                "Una sociedad no presenta el 721 si las tiene en la contabilidad identificadas por \
                 denominación, valor, custodio y país (art. 42 quater.5.b): contabilízalas desde \
                 Criptomonedas. Si no las contabilizas, presenta el 721 de {year} entre el 1 de \
                 enero y el 31 de marzo de {} y marca la casilla del perfil.",
                year + 1
            )
        },
        rule: "RGAT art. 42 quater (RD 1065/2007); Orden HFP/886/2023",
    });
    Ok(())
}

/// 232: operaciones vinculadas por encima de alguno de los tres umbrales (más de 250.000 € con
/// la misma persona o entidad, más de 100.000 € en operaciones específicas de un tipo, o más del
/// 50 % de la cifra de negocios en las del mismo tipo y método), según el registro de
/// `vinculadas`: facturas, nóminas, subcuentas asignadas y operaciones tecleadas.
fn check_232(
    v: &Vault,
    p: &TaxpayerProfile,
    today: Date,
    out: &mut Vec<ThresholdAlert>,
) -> Result<()> {
    if p.persona != Persona::Juridica || p.operaciones_vinculadas {
        return Ok(());
    }
    let current = crate::accounting::fiscal_year_at(v, today)?.year;
    for year in [current - 1, current] {
        let Ok(r) = crate::vinculadas::registro(v, year) else {
            continue;
        };
        if !r.obligado {
            continue;
        }
        out.push(ThresholdAlert {
            code: "232",
            severity: if year < current {
                Severity::Warning
            } else {
                Severity::Info
            },
            title: format!("Operaciones vinculadas de {year}: toca el 232"),
            detail: format!(
                "Según el registro de operaciones vinculadas de {year} hay {}. Los préstamos y \
                 cuentas corrientes sólo cuentan si su subcuenta está asignada a la vinculada.",
                lista(&r.motivos)
            ),
            action: format!(
                "Presenta el 232 de {year} en {} (el mes siguiente a los diez meses del cierre) \
                 y marca la casilla del 232 en el perfil fiscal. El borrador y la documentación \
                 simplificada están en Contabilidad → Vinculadas. Las operaciones deben \
                 valorarse a precio de mercado y documentarse (art. 18 LIS).",
                r.plazo
            ),
            rule: "LIS art. 18; RIS art. 13.4 (RD 634/2015); Orden HFP/816/2017 art. 2",
        });
    }
    Ok(())
}

/// Patrimonio neto por debajo de la mitad del capital (causa de disolución, art. 363.1.e LSC) o,
/// en la SA, de dos tercios durante un ejercicio (reducción obligatoria, art. 327 LSC), con el
/// último balance trimestral terminado del ejercicio en curso o, si aún no tiene asientos, del
/// anterior. El detalle está en Contabilidad → Cierre (`patrimonial`).
fn check_patrimonio_neto(
    v: &Vault,
    p: &TaxpayerProfile,
    today: Date,
    out: &mut Vec<ThresholdAlert>,
) -> Result<()> {
    if p.persona != Persona::Juridica {
        return Ok(());
    }
    let año = crate::accounting::fiscal_year_at(v, today)?.year;
    let año = if crate::accounting::journal(v, año)?.is_empty() {
        año - 1
    } else {
        año
    };
    if crate::accounting::journal(v, año)?.is_empty() {
        return Ok(());
    }
    let i = crate::patrimonial::informe(v, año, today)?;
    for a in i.patrimonio.avisos {
        if !matches!(a.codigo, "causa_disolucion" | "reduccion_obligatoria") {
            continue;
        }
        out.push(ThresholdAlert {
            code: a.codigo,
            severity: Severity::Warning,
            title: a.titulo,
            detail: a.detalle,
            action: format!(
                "Revisa el patrimonio neto en Contabilidad → Cierre → «Entidad patrimonial y \
                 patrimonio neto {año}» y consúltalo con tu asesor."
            ),
            rule: a.norma,
        });
    }
    Ok(())
}

/// Cifra de negocios del último ejercicio cerrado: tipo del IS (micro y reducida dimensión),
/// 202 por la base del periodo e IAE.
fn check_cifra_de_negocios(
    v: &Vault,
    p: &TaxpayerProfile,
    today: Date,
    out: &mut Vec<ThresholdAlert>,
) -> Result<()> {
    if p.persona == Persona::Fisica {
        return Ok(());
    }
    // La del 200 presentado o, sin él, la de un ejercicio completo en la contabilidad con
    // ventas (si no, la cifra no es fiable).
    let t = crate::payroll::director_turnover(v, today)?;
    let (Some(incn), Some(_)) = (t.turnover, t.small) else {
        return Ok(());
    };
    let year = t.year;
    let segun = t.source.label();
    if p.persona == Persona::Juridica {
        let s = models::settings(v)?;
        let (expected, label) = if incn < UN_MILLON {
            (TaxRegime::Micro, "microempresa")
        } else if incn < DIEZ_MILLONES {
            (
                TaxRegime::ReducidaDimension,
                "empresa de reducida dimensión",
            )
        } else {
            (TaxRegime::General, "tipo general")
        };
        if s.corporate_regime != TaxRegime::NuevaCreacion && s.corporate_regime != expected {
            out.push(ThresholdAlert {
                code: "is_regimen",
                severity: Severity::Info,
                title: format!(
                    "Revisa el tipo del Impuesto sobre Sociedades de {}",
                    year + 1
                ),
                detail: format!(
                    "La cifra de negocios de {year} fue {incn} (según {segun}): el ejercicio {} \
                     tributa como {label}, y en Modelos está elegido otro régimen.",
                    year + 1
                ),
                action: "Cambia el régimen del Impuesto sobre Sociedades en Modelos (salvo que \
                         seas entidad patrimonial, de nueva creación o formes grupo: entonces la \
                         cifra que cuenta es otra)."
                    .into(),
                rule: "LIS arts. 29.1 y 101 (Ley 27/2014)",
            });
        }
        let over_6m = incn >= SEIS_MILLONES;
        if over_6m != s.turnover_over_6m && p.pagos_fraccionados_is {
            out.push(ThresholdAlert {
                code: "202",
                severity: Severity::Warning,
                title: if over_6m {
                    "El 202 va por la base del periodo".into()
                } else {
                    "El 202 ya no va por la base del periodo".into()
                },
                detail: format!(
                    "La cifra de negocios de {year} fue {incn} (según {segun}; {} de 6 \
                     millones) y en Modelos está marcado lo contrario.",
                    if over_6m { "más" } else { "menos" }
                ),
                action: "Corrige la casilla «cifra de negocios superior a 6 millones» de los \
                         ajustes del 202 en Modelos."
                    .into(),
                rule: "LIS art. 40.3 (Ley 27/2014)",
            });
        }
    }
    // IAE: con 1 M€ o más, se paga dos años después (y el alta, en diciembre del anterior).
    if incn >= UN_MILLON && p.iae_desde.is_none_or(|d| d > year + 2) {
        let desde = year + 2;
        out.push(ThresholdAlert {
            code: "iae",
            severity: Severity::Warning,
            title: format!("Desde {desde} pagas el IAE"),
            detail: format!(
                "La cifra de negocios de {year} fue {incn} (según {segun}): 1 M€ o más. La \
                 exención del IAE se pierde dos años después."
            ),
            action: format!(
                "Presenta el alta en el IAE (modelo 840) en diciembre de {} si no lo has hecho \
                 ya, y pon {desde} como «Año desde el que pagas el IAE» en el perfil fiscal. Los \
                 dos primeros años de actividad siguen exentos.",
                desde - 1
            ),
            rule: "TRLRHL art. 82.1.b y c (RDLeg 2/2004); RD 243/1995 art. 5",
        });
    }
    Ok(())
}

/// Ventanilla única (OSS): ventas a distancia y servicios TBE a particulares de otros Estados
/// miembros por encima de 10.000 € (año anterior o en curso), o cerca, y alta en el régimen
/// (035) y perfil fiscal (369). Ver [`crate::oss`].
fn check_oss(
    v: &Vault,
    p: &TaxpayerProfile,
    today: Date,
    out: &mut Vec<ThresholdAlert>,
) -> Result<()> {
    let e = crate::oss::estado(v, today.year())?;
    let rule =
        "LIVA arts. 68.Tres.a, 70.Uno.4.º y 8.º, 73 y 163 unvicies y ss.; Orden HAC/610/2021";
    if e.tributa_en_destino {
        let mut action = String::new();
        if e.falta_035 {
            action.push_str("Presenta el 035 (alta en el régimen de la Unión) y apunta la fecha en Contabilidad → Impuestos → Ventanilla única. ");
        }
        if !p.oss {
            action.push_str("Marca la ventanilla única (369) en el perfil fiscal para que el calendario te avise del 369 trimestral. ");
        }
        if !e.con_iva_espanol.is_empty() {
            action.push_str("Rectifica las facturas que llevan IVA español. ");
        }
        if action.is_empty() {
            return Ok(());
        }
        out.push(ThresholdAlert {
            code: "oss",
            severity: Severity::Warning,
            title: "Ventas a particulares de la UE: tributan en el país del cliente".into(),
            detail: e.avisos.join(" "),
            action: action.trim_end().to_owned(),
            rule,
        });
    } else if e.umbral.en_curso
        >= taxpain_core::invoice::oss::UMBRAL.apply_rate(taxpain_core::money::Rate::percent(80))
    {
        out.push(ThresholdAlert {
            code: "oss",
            severity: Severity::Info,
            title: "Te acercas a los 10.000 € de ventas a particulares de la UE".into(),
            detail: format!(
                "En {} llevas {} en servicios electrónicos o de telecomunicaciones y ventas a \
                 distancia a particulares de otros países de la UE.",
                today.year(),
                e.umbral.en_curso
            ),
            action: "Prepara el alta en la ventanilla única (035): la venta con la que pases de \
                     10.000 € ya lleva el IVA del país del cliente."
                .into(),
            rule,
        });
    }
    Ok(())
}

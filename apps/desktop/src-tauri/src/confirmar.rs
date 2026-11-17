//! Touch ID (o la contraseña del Mac) antes de borrar datos que no se pueden recuperar o de
//! cambiar ajustes sensibles. La interfaz ya pregunta, pero no basta: una interfaz comprometida
//! podría llamar al comando directamente. La razón del aviso la construye Rust con lo que hay en
//! el almacén (p. ej. «Borrar el presupuesto P-2026-0003»), nunca con lo que diga la interfaz.
//!
//! Sin Touch ID a propósito (son de poco valor o tienen vuelta atrás):
//! - `received_discard`: descartar una recibida se deshace con `received_undiscard`.
//! - `apertura_discard`: tira el borrador del asistente de apertura (nada contabilizado).
//! - `template_delete` y `bank_rule_delete`: plantillas de asiento y reglas del banco, ayudas
//!   que se vuelven a crear en un momento y no cambian la contabilidad ni los modelos.
//! - Conversaciones del asistente (`assistant_conversation_delete`, `…_delete_all`,
//!   `assistant_material_delete`): no tienen valor fiscal y borrarlas es una medida de
//!   privacidad que no debe costar.
//! - Dejar de vigilar una carpeta o desactivar «Full IA»: sólo reducen lo que se lee.

use taxpain_app::assistant::AssistantSettings;
use taxpain_app::{
    accounting, analitica, assistant, atribucion, autonomo_societario, companies, contacts, cripto,
    dividendos, documentos, expedientes, leasing, lectura_ia, payroll, presentados, prestamos,
    quotes, recurring, socios, valores, viajes, vigiladas,
};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::rusqlite::{self, OptionalExtension};
use time::{Date, OffsetDateTime};

use crate::invoicing::reason_text;
use crate::state::{AppState, VaultGuard};
use crate::{CmdResult, CommandError};

/// Pide Touch ID con la razón que construye `razon` con el almacén de la empresa abierta y
/// devuelve el almacén de esa misma empresa para hacer el cambio. `None`: no hace falta
/// preguntar (no existe, no está confirmado en la interfaz o el cambio no es sensible) y el
/// comando sigue sin más (si no existe, el error lo da él). Durante el aviso el almacén no está
/// cogido; al volver se calcula otra vez la razón y, si ha cambiado (se ha cambiado de empresa o
/// el dato ha cambiado mientras), no se hace nada.
pub(crate) fn con_presencia<'a>(
    state: &'a AppState,
    razon: impl Fn(&Vault) -> CmdResult<Option<String>>,
) -> CmdResult<VaultGuard<'a>> {
    let (company, reason) = {
        let (company, v) = state.vault_with_id()?;
        match razon(&v)? {
            Some(r) => (company, r),
            None => return Ok(v),
        }
    };
    state.confirm_presence(&reason)?;
    let v = state.vault_of(&company)?;
    if razon(&v)?.as_deref() != Some(reason.as_str()) {
        return Err(CommandError::from(
            "ha cambiado mientras lo confirmabas: no se ha hecho nada, vuelve a intentarlo",
        ));
    }
    Ok(v)
}

fn ahora() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

/// Una fila que puede no estar (`None`).
fn fila<T>(r: rusqlite::Result<T>) -> CmdResult<Option<T>> {
    Ok(r.optional()?)
}

/// `2026-03-05` → `05/03/2026` (si no tiene esa forma, tal cual).
fn fecha(iso: &str) -> String {
    match (iso.get(0..4), iso.get(5..7), iso.get(8..10)) {
        (Some(y), Some(m), Some(d)) if iso.len() >= 10 => format!("{d}/{m}/{y}"),
        _ => reason_text(iso),
    }
}

/// Una ruta para el aviso: si es larga, el final (lo que la distingue).
fn ruta(p: &str) -> String {
    let one_line = reason_text(&p.replace('…', ""));
    let chars: Vec<char> = p.chars().filter(|c| !c.is_control()).collect();
    if chars.len() <= 60 {
        return one_line;
    }
    format!("…{}", chars[chars.len() - 59..].iter().collect::<String>())
}

/// Un dato con su NIF entre paréntesis, si lo tiene.
fn con_nif(name: &str, nif: &str) -> String {
    let nif = reason_text(nif);
    if nif.is_empty() {
        reason_text(name)
    } else {
        format!("{} ({nif})", reason_text(name))
    }
}

// ---------- Documentos y modelos presentados ----------

pub(crate) fn borrar_documento(state: &AppState, id: i64) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        Ok(fila(v.connection().query_row(
            "SELECT title FROM company_document WHERE id = ?1",
            [id],
            |r| r.get::<_, String>(0),
        ))?
        .map(|t| format!("Borrar el documento {}", reason_text(&t))))
    })?;
    Ok(documentos::delete(&v, id)?)
}

pub(crate) fn borrar_presentado(state: &AppState, id: i64) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        Ok(fila(v.connection().query_row(
            "SELECT model, year, period FROM filed_model WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        ))?
        .map(|(model, year, period)| {
            let model = reason_text(&model);
            if period == "0A" {
                format!("Borrar el modelo {model} presentado de {year}")
            } else {
                format!(
                    "Borrar el modelo {model} presentado del {} {year}",
                    reason_text(&period)
                )
            }
        }))
    })?;
    Ok(presentados::delete(&v, id)?)
}

// ---------- Presupuestos, contactos y facturas periódicas ----------

pub(crate) fn borrar_presupuesto(state: &AppState, id: i64) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        let row = fila(v.connection().query_row(
            "SELECT kind, number, status FROM quote WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        ))?;
        // Sólo se borran los borradores: si no lo es, el error lo da `quotes::delete` sin
        // pedir Touch ID para nada.
        Ok(row.filter(|(_, _, s)| s == "borrador").map(|(kind, n, _)| {
            let what = if kind == "proforma" {
                "la proforma"
            } else {
                "el presupuesto"
            };
            format!("Borrar {what} {}", reason_text(&n))
        }))
    })?;
    Ok(quotes::delete(&v, id, ahora())?)
}

pub(crate) fn borrar_contacto(state: &AppState, id: i64) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        Ok(fila(v.connection().query_row(
            "SELECT name, tax_id FROM contact WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ))?
        .map(|(name, nif)| format!("Borrar el contacto {}", con_nif(&name, &nif))))
    })?;
    Ok(contacts::delete(&v, id, ahora())?)
}

pub(crate) fn borrar_periodica(state: &AppState, id: &str) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        let today = taxpain_core::spain_time::peninsular_date(ahora());
        Ok(recurring::list(v, today)?
            .into_iter()
            .find(|t| t.template.id == id)
            .map(|t| {
                format!(
                    "Borrar la factura periódica {}",
                    reason_text(&t.template.name)
                )
            }))
    })?;
    Ok(recurring::delete(&v, id, ahora())?)
}

// ---------- Criptomonedas ----------

pub(crate) fn borrar_plataforma_cripto(state: &AppState, id: i64) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        // Con operaciones o en el punto de partida no se borra: el error lo da
        // `cripto::delete_platform` sin pedir Touch ID para nada.
        let en_uso: bool = v.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM crypto_op WHERE platform_id = ?1 OR to_platform_id = ?1)
                 OR EXISTS(SELECT 1 FROM crypto_opening_position WHERE platform_id = ?1)",
            [id],
            |r| r.get(0),
        )?;
        if en_uso {
            return Ok(None);
        }
        Ok(fila(v.connection().query_row(
            "SELECT name FROM crypto_platform WHERE id = ?1",
            [id],
            |r| r.get::<_, String>(0),
        ))?
        .map(|n| format!("Borrar la plataforma de cripto {}", reason_text(&n))))
    })?;
    Ok(cripto::delete_platform(&v, id)?)
}

pub(crate) fn borrar_op_cripto(state: &AppState, id: i64) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        Ok(fila(v.connection().query_row(
            "SELECT kind, qty, ticker, date FROM crypto_op WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        ))?
        .map(|(kind, qty, ticker, date)| {
            format!(
                "Borrar la operación de cripto del {} ({} de {} {})",
                fecha(&date),
                reason_text(&kind),
                reason_text(&qty),
                reason_text(&ticker)
            )
        }))
    })?;
    Ok(cripto::delete_op(&v, id, ahora())?)
}

pub(crate) fn borrar_precio_cripto(state: &AppState, date: Date, ticker: &str) -> CmdResult<()> {
    let ticker = cripto::ticker_norm(ticker);
    let v = con_presencia(state, |v| {
        Ok(fila(v.connection().query_row(
            "SELECT price FROM crypto_price WHERE date = ?1 AND ticker = ?2",
            rusqlite::params![date.to_string(), ticker],
            |r| r.get::<_, String>(0),
        ))?
        .map(|_| {
            format!(
                "Borrar el precio de {} del {}",
                reason_text(&ticker),
                fecha(&date.to_string())
            )
        }))
    })?;
    Ok(cripto::delete_price(&v, date, &ticker)?)
}

pub(crate) fn borrar_inicio_cripto(state: &AppState) -> CmdResult<Vec<String>> {
    let v = con_presencia(state, |v| {
        // `cripto::inicio::borrar` vacía las dos tablas: se pregunta si hay algo en cualquiera
        // (también posiciones sueltas sin la cabecera, de una importación a medias).
        let (date, posiciones) = v.connection().query_row(
            "SELECT (SELECT date FROM crypto_opening WHERE id = 1),
                    (SELECT COUNT(*) FROM crypto_opening_position)",
            [],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?)),
        )?;
        Ok(match date {
            Some(d) => Some(format!(
                "Quitar el punto de partida de las criptomonedas (a {})",
                fecha(&d)
            )),
            None if posiciones > 0 => Some(format!(
                "Quitar el punto de partida de las criptomonedas ({posiciones} {} sin fecha)",
                if posiciones == 1 { "saldo" } else { "saldos" }
            )),
            None => None,
        })
    })?;
    Ok(cripto::inicio::borrar(&v, ahora())?)
}

// ---------- Valores ----------

pub(crate) fn borrar_cuenta_valores(state: &AppState, id: i64) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        // Con operaciones no se borra: el error lo da `valores::borrar_cuenta` sin preguntar.
        let en_uso: bool = v.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM security_op WHERE account_id = ?1 OR to_account_id = ?1)",
            [id],
            |r| r.get(0),
        )?;
        if en_uso {
            return Ok(None);
        }
        Ok(fila(v.connection().query_row(
            "SELECT name FROM security_account WHERE id = ?1",
            [id],
            |r| r.get::<_, String>(0),
        ))?
        .map(|n| format!("Borrar la cuenta de valores {}", reason_text(&n))))
    })?;
    Ok(valores::borrar_cuenta(&v, id, ahora())?)
}

pub(crate) fn borrar_op_valores(state: &AppState, id: i64) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        Ok(fila(v.connection().query_row(
            "SELECT o.kind, o.date, s.ticker FROM security_op o
             LEFT JOIN security s ON s.id = o.security_id WHERE o.id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        ))?
        .map(|(kind, date, ticker)| {
            let what = match ticker.as_deref().map(reason_text) {
                Some(t) if !t.is_empty() => format!("{} de {t}", reason_text(&kind)),
                _ => reason_text(&kind),
            };
            format!(
                "Borrar la operación de valores del {} ({what})",
                fecha(&date)
            )
        }))
    })?;
    Ok(valores::borrar_op(&v, id, ahora())?)
}

pub(crate) fn borrar_precio_valores(
    state: &AppState,
    date: Date,
    security_id: i64,
) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        Ok(fila(v.connection().query_row(
            "SELECT s.ticker FROM security_price p JOIN security s ON s.id = p.security_id
             WHERE p.date = ?1 AND p.security_id = ?2",
            rusqlite::params![date.to_string(), security_id],
            |r| r.get::<_, String>(0),
        ))?
        .map(|t| {
            format!(
                "Borrar el precio de {} del {}",
                reason_text(&t),
                fecha(&date.to_string())
            )
        }))
    })?;
    Ok(valores::borrar_precio(&v, date, security_id)?)
}

// ---------- Contabilidad ----------

/// Sólo se borran los asientos manuales o importados: de los demás el error lo da
/// `accounting`, sin pedir Touch ID.
const BORRABLE: &str = "origin_kind IN ('Manual', 'Importado')";

fn razon_asiento(v: &Vault, id: i64) -> CmdResult<Option<String>> {
    Ok(fila(v.connection().query_row(
        &format!(
            "SELECT number, fiscal_year, date, concept FROM journal_entry
             WHERE id = ?1 AND {BORRABLE}"
        ),
        [id],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        },
    ))?
    .map(|(number, _year, date, concept)| {
        format!(
            "Borrar el asiento {number} del {} ({})",
            fecha(&date),
            reason_text(&concept)
        )
    }))
}

/// Borra un asiento manual. Con `confirmed` (el diálogo), Touch ID con su número y concepto.
pub(crate) fn borrar_asiento(state: &AppState, id: i64, confirmed: bool) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        if confirmed {
            razon_asiento(v, id)
        } else {
            Ok(None)
        }
    })?;
    Ok(accounting::delete_manual_entry(&v, id, ahora(), confirmed)?)
}

pub(crate) fn borrar_asientos(state: &AppState, ids: &[i64], confirmed: bool) -> CmdResult<usize> {
    let v = con_presencia(state, |v| {
        Ok(match ids {
            _ if !confirmed => None,
            [] => None,
            [id] => razon_asiento(v, *id)?,
            _ => {
                let mut distintos = ids.to_vec();
                distintos.sort_unstable();
                distintos.dedup();
                let mut todos = true;
                for id in &distintos {
                    todos &= v.connection().query_row(
                        &format!(
                            "SELECT EXISTS(SELECT 1 FROM journal_entry WHERE id = ?1 AND {BORRABLE})"
                        ),
                        [id],
                        |r| r.get::<_, bool>(0),
                    )?;
                }
                todos.then(|| format!("Borrar {} asientos del diario", distintos.len()))
            }
        })
    })?;
    Ok(accounting::delete_entries(&v, ids, ahora(), confirmed)?)
}

pub(crate) fn borrar_centro_coste(state: &AppState, code: &str, confirmed: bool) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        if !confirmed {
            return Ok(None);
        }
        Ok(fila(v.connection().query_row(
            "SELECT name FROM cost_center WHERE code = ?1",
            [code],
            |r| r.get::<_, String>(0),
        ))?
        .map(|n| {
            format!(
                "Borrar el centro de coste {} {}",
                reason_text(code),
                reason_text(&n)
            )
        }))
    })?;
    Ok(analitica::delete_center(&v, code, confirmed)?)
}

pub(crate) fn borrar_leasing(state: &AppState, id: i64, confirmed: bool) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        if !confirmed {
            return Ok(None);
        }
        // Lo mismo que mira `leasing::delete`: con alguna cuota contabilizada no se borra, y el
        // error lo da él sin pedir Touch ID para nada.
        Ok(leasing::list(v)?
            .into_iter()
            .find(|x| x.leasing.id == id)
            .filter(|x| !x.installments.iter().any(|q| q.posted))
            .map(|x| {
                format!(
                    "Borrar el contrato de leasing {}",
                    reason_text(&x.leasing.name)
                )
                .trim_end()
                .to_owned()
            }))
    })?;
    Ok(leasing::delete(&v, id, ahora(), confirmed)?)
}

pub(crate) fn borrar_expediente(state: &AppState, id: i64, confirmed: bool) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        if !confirmed {
            return Ok(None);
        }
        Ok(expedientes::name(v, id)?.map(|n| {
            format!("Borrar el expediente {}", reason_text(&n))
                .trim_end()
                .to_owned()
        }))
    })?;
    Ok(expedientes::delete(&v, id, ahora(), confirmed)?)
}

pub(crate) fn borrar_certificado_admin(
    state: &AppState,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        if !confirmed {
            return Ok(None);
        }
        Ok(expedientes::certificados::name(v, id)?
            .map(|n| format!("Borrar {}", reason_text(&n)).trim_end().to_owned()))
    })?;
    Ok(expedientes::certificados::delete(
        &v,
        id,
        ahora(),
        confirmed,
    )?)
}

pub(crate) fn borrar_prestamo(state: &AppState, id: i64, confirmed: bool) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        if !confirmed {
            return Ok(None);
        }
        // Con asientos no se borra, y el error lo da `prestamos::delete` sin pedir Touch ID.
        if prestamos::delete_blocker(v, id).ok().flatten().is_some() {
            return Ok(None);
        }
        Ok(prestamos::name(v, id)?.map(|n| {
            format!("Borrar el préstamo {}", reason_text(&n))
                .trim_end()
                .to_owned()
        }))
    })?;
    Ok(prestamos::delete(&v, id, ahora(), confirmed)?)
}

pub(crate) fn borrar_nomina(state: &AppState, id: i64, confirmed: bool) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        if !confirmed {
            return Ok(None);
        }
        Ok(fila(v.connection().query_row(
            "SELECT e.name, p.date FROM payslip p JOIN employee e ON e.id = p.employee_id
             WHERE p.id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ))?
        .map(|(name, date)| {
            format!(
                "Borrar la nómina de {} del {}",
                reason_text(&name),
                fecha(&date)
            )
        }))
    })?;
    Ok(payroll::delete(&v, id, ahora(), confirmed)?)
}

pub(crate) fn borrar_cuota_reta(state: &AppState, id: i64, confirmed: bool) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        if !confirmed {
            return Ok(None);
        }
        Ok(fila(v.connection().query_row(
            "SELECT e.name, c.date FROM reta_cuota c JOIN employee e ON e.id = c.employee_id
             WHERE c.id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ))?
        .map(|(name, date)| {
            format!(
                "Borrar la cuota de autónomos de {} del {}",
                reason_text(&name),
                fecha(&date)
            )
        }))
    })?;
    Ok(autonomo_societario::delete_cuota(
        &v,
        id,
        ahora(),
        confirmed,
    )?)
}

pub(crate) fn borrar_viaje(state: &AppState, id: i64, confirmed: bool) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        if !confirmed {
            return Ok(None);
        }
        Ok(fila(v.connection().query_row(
            "SELECT name, destination, date FROM travel_expense WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        ))?
        .map(|(name, dest, date)| {
            format!(
                "Borrar el viaje de {} a {} del {}",
                reason_text(&name),
                reason_text(&dest),
                fecha(&date)
            )
        }))
    })?;
    Ok(viajes::delete(&v, id, ahora(), confirmed)?)
}

// ---------- Socios y partícipes ----------

pub(crate) fn borrar_socio(state: &AppState, id: i64) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        Ok(fila(v.connection().query_row(
            "SELECT name, nif FROM shareholder WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ))?
        .map(|(name, nif)| format!("Borrar el socio {}", con_nif(&name, &nif))))
    })?;
    Ok(socios::delete_shareholder(&v, id, ahora())?)
}

pub(crate) fn borrar_participe(state: &AppState, id: i64) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        Ok(fila(v.connection().query_row(
            "SELECT name, nif FROM partner WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ))?
        .map(|(name, nif)| {
            format!(
                "Borrar el partícipe {} del modelo 184",
                con_nif(&name, &nif)
            )
        }))
    })?;
    Ok(atribucion::delete_partner(&v, id, ahora())?)
}

/// Deshace un pago de dividendos y borra su asiento (cuentan para el 123 y las retenciones):
/// con `confirmed` (el diálogo), Touch ID con la fecha, el bruto y la retención guardados. Si
/// su asiento es de un ejercicio cerrado, `socios::undo` no lo deshace, y el error lo da él sin
/// pedir Touch ID.
pub(crate) fn deshacer_dividendo(state: &AppState, id: i64, confirmed: bool) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        if !confirmed {
            return Ok(None);
        }
        Ok(fila(v.connection().query_row(
            "SELECT d.date, d.gross, d.withheld FROM dividend_payment d
             LEFT JOIN journal_entry e ON e.id = d.entry_id
             LEFT JOIN fiscal_year f ON f.year = e.fiscal_year
             WHERE d.id = ?1 AND COALESCE(f.closed, 0) = 0",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        ))?
        .map(|(date, gross, withheld)| {
            format!(
                "Deshacer el pago de dividendos del {} (bruto {}, retención de {})",
                fecha(&date),
                Money::from_cents(gross),
                Money::from_cents(withheld)
            )
        }))
    })?;
    Ok(socios::undo(&v, id, ahora(), confirmed)?)
}

/// Deshace un acuerdo de dividendos (borra su asiento y el acta si sigue en borrador): con
/// `confirmed`, Touch ID con la fecha y el importe guardados.
pub(crate) fn deshacer_acuerdo_dividendo(
    state: &AppState,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    let v = con_presencia(state, |v| {
        if !confirmed {
            return Ok(None);
        }
        Ok(fila(v.connection().query_row(
            "SELECT date, amount FROM dividend_agreement WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        ))?
        .map(|(date, amount)| {
            format!(
                "Deshacer el acuerdo de dividendos del {} ({})",
                fecha(&date),
                Money::from_cents(amount)
            )
        }))
    })?;
    Ok(dividendos::undo(&v, id, ahora(), confirmed)?)
}

// ---------- Ajustes sensibles ----------

/// El servidor al que van los datos (y la clave) con estos ajustes.
fn host_asistente(s: &AssistantSettings) -> String {
    s.provider
        .endpoint()
        .ok()
        .and_then(|u| u.host_str().map(reason_text))
        .unwrap_or_default()
}

/// Guarda los ajustes del asistente y, si llega, la clave de API a la vez. Con clave, Touch ID
/// con el servidor de `settings` (el proveedor que se guarda, al que irá la clave); si se
/// cancela, no cambia nada, ni el proveedor. Sin clave, como siempre: sin preguntar (cambiar de
/// proveedor ya obliga a aceptar otra vez el envío). `false`: lo aceptado no se ha guardado
/// porque el proveedor nuevo manda los datos a otro sitio.
pub(crate) fn guardar_asistente(
    state: &AppState,
    settings: &AssistantSettings,
    key: Option<&str>,
) -> CmdResult<bool> {
    let key = key.map(str::trim).filter(|k| !k.is_empty());
    let v = con_presencia(state, |_| {
        // Con un modelo local no hay clave: el error lo da `set_api_key`, sin preguntar.
        Ok(key.filter(|_| settings.provider.is_remote()).map(|_| {
            format!(
                "Guardar la clave de API del asistente para {}",
                host_asistente(settings)
            )
        }))
    })?;
    let tx = v.connection().unchecked_transaction()?;
    assistant::save_settings(&v, settings, ahora())?;
    if key.is_some() {
        assistant::set_api_key(&v, key, ahora())?;
    }
    tx.commit()?;
    Ok(assistant::status(&v)?.settings.consent == settings.consent)
}

/// Guarda (o quita, con `None` o vacía) la clave de API del proveedor guardado: Touch ID con el
/// servidor al que va. Con un modelo local no hay clave (el error lo da `set_api_key`).
pub(crate) fn guardar_clave_asistente(state: &AppState, key: Option<&str>) -> CmdResult<()> {
    let quitar = key.map(str::trim).is_none_or(str::is_empty);
    let v = con_presencia(state, |v| {
        let st = assistant::status(v)?;
        if !st.remote {
            return Ok(None);
        }
        let host = host_asistente(&st.settings);
        Ok(Some(if quitar {
            format!("Quitar la clave de API del asistente ({host})")
        } else {
            format!("Guardar la clave de API del asistente para {host}")
        }))
    })?;
    Ok(assistant::set_api_key(&v, key, ahora())?)
}

/// Guarda los ajustes de la lectura con IA. Activar «Full IA» (los PDF y las imágenes van al
/// asistente tal cual, sin anonimizar) pide Touch ID; desactivarlo o cambiar el modelo, no.
pub(crate) fn guardar_lectura_ia(
    state: &AppState,
    settings: &lectura_ia::Settings,
) -> CmdResult<lectura_ia::Settings> {
    let v = con_presencia(state, |v| {
        if !settings.full || lectura_ia::settings(v)?.full {
            return Ok(None);
        }
        let name = companies::info(v)?.name;
        Ok(Some(format!(
            "Activar «Full IA»: mandar los documentos de {} al asistente sin anonimizar",
            reason_text(&name)
        )))
    })?;
    Ok(lectura_ia::save_settings(&v, settings, ahora())?)
}

/// Cambia las carpetas vigiladas. Añadir una pide Touch ID con cuál (la app leerá lo que entre
/// en ella); quitarlas, no.
pub(crate) fn guardar_carpetas_vigiladas(
    state: &AppState,
    folders: &[String],
) -> CmdResult<Vec<String>> {
    let v = con_presencia(state, |v| {
        let before = vigiladas::folders(v)?;
        let mut nuevas: Vec<String> = Vec::new();
        for f in folders {
            // Como las normaliza `set_folders`.
            let f = f.trim().trim_end_matches('/').to_owned();
            if !f.is_empty() && !before.contains(&f) && !nuevas.contains(&f) {
                nuevas.push(f);
            }
        }
        Ok(match nuevas.as_slice() {
            [] => None,
            [f] => Some(format!("Vigilar la carpeta {}", ruta(f))),
            [f, ..] => Some(format!(
                "Vigilar {} carpetas nuevas: {} y otras",
                nuevas.len(),
                ruta(f)
            )),
        })
    })?;
    Ok(vigiladas::set_folders(&v, folders)?)
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use taxpain_secrets::{MemoryKeyProvider, UserPresence};

    use super::*;

    /// Touch ID de pruebas: apunta las razones y se puede negar.
    #[derive(Debug, Default)]
    pub(crate) struct Presencia {
        negar: AtomicBool,
        razones: Mutex<Vec<String>>,
    }

    impl Presencia {
        pub(crate) fn negar(&self, n: bool) {
            self.negar.store(n, Ordering::SeqCst);
        }
        pub(crate) fn razones(&self) -> Vec<String> {
            self.razones.lock().unwrap().clone()
        }
        fn olvidar(&self) {
            self.razones.lock().unwrap().clear();
        }
    }

    impl UserPresence for Presencia {
        fn confirm(&self, reason: &str) -> taxpain_secrets::Result<()> {
            self.razones.lock().unwrap().push(reason.to_owned());
            if self.negar.load(Ordering::SeqCst) {
                Err(taxpain_secrets::Error::NotConfirmed)
            } else {
                Ok(())
            }
        }
    }

    struct Env {
        dir: PathBuf,
        state: AppState,
        presencia: Arc<Presencia>,
    }

    impl Drop for Env {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn env(name: &str) -> Env {
        let dir =
            std::env::temp_dir().join(format!("taxpain-confirmar-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let presencia = Arc::new(Presencia::default());
        let state = AppState::open_with(
            &dir.join("app"),
            Arc::new(MemoryKeyProvider::default()),
            presencia.clone(),
            Arc::new(|_: &Path| Ok(())),
        )
        .unwrap();
        Env {
            dir,
            state,
            presencia,
        }
    }

    impl Env {
        fn sql(&self, sql: &str) {
            self.state
                .vault()
                .unwrap()
                .connection()
                .execute_batch(sql)
                .unwrap();
        }
        fn cuenta(&self, sql: &str) -> i64 {
            self.state
                .vault()
                .unwrap()
                .connection()
                .query_row(sql, [], |r| r.get(0))
                .unwrap()
        }
        /// Negando Touch ID no se hace nada y se ha visto `razon`; confirmando, sí.
        fn comprobar<T: std::fmt::Debug>(
            &self,
            razon: &str,
            quedan: &str,
            hacer: impl Fn(&AppState) -> CmdResult<T>,
        ) {
            let antes = self.cuenta(quedan);
            assert!(antes > 0, "{quedan}");
            self.presencia.olvidar();
            self.presencia.negar(true);
            let e = hacer(&self.state).unwrap_err();
            assert_eq!(e.message, "operación no confirmada por el usuario");
            assert_eq!(self.presencia.razones(), [razon]);
            assert_eq!(self.cuenta(quedan), antes, "sin confirmar no cambia nada");

            self.presencia.negar(false);
            hacer(&self.state).unwrap();
            assert_eq!(self.presencia.razones(), [razon, razon]);
            assert!(self.cuenta(quedan) < antes, "confirmando, se borra");
            self.presencia.olvidar();
        }
    }

    const NOW: &str = "2026-01-01T00:00:00Z";

    #[test]
    fn fechas_y_rutas_para_el_aviso() {
        assert_eq!(fecha("2026-03-05"), "05/03/2026");
        assert_eq!(fecha("raro"), "raro");
        assert_eq!(ruta("/Users/demo/Facturas"), "/Users/demo/Facturas");
        let larga = format!("/Users/demo/{}/Facturas", "x".repeat(80));
        let r = ruta(&larga);
        assert!(r.starts_with('…') && r.ends_with("/Facturas"), "{r}");
        assert_eq!(r.chars().count(), 60);
        assert_eq!(con_nif("ACME SL", "B12345674"), "ACME SL (B12345674)");
        assert_eq!(con_nif("ACME SL", ""), "ACME SL");
    }

    #[test]
    fn borrar_documentos_y_presentados_pide_touch_id() {
        let e = env("docs");
        e.sql(&format!(
            "INSERT INTO document(id, uuid, kind, filename, mime, sha256, size, content, created_at)
               VALUES (1, 'u1', 'otro', 'escritura.pdf', 'application/pdf', 'aa', 1, x'00',
               '{NOW}');
             INSERT INTO company_document(id, kind, title, filename, file_sha256, document_id,
               facts, imported_at)
               VALUES (7, 'escritura', 'Escritura de constitución', 'escritura.pdf', 'aa', 1,
               '{{}}', '{NOW}');
             INSERT INTO filed_model(id, model, year, period, filename, file_sha256, source,
               boxes, imported_at)
               VALUES (3, '303', 2026, '1T', '303.txt', 'bb', 'manual', '{{}}', '{NOW}'),
                      (4, '200', 2025, '0A', '200.txt', 'cc', 'manual', '{{}}', '{NOW}');"
        ));
        e.comprobar(
            "Borrar el documento Escritura de constitución",
            "SELECT COUNT(*) FROM company_document",
            |s| borrar_documento(s, 7),
        );
        e.comprobar(
            "Borrar el modelo 303 presentado del 1T 2026",
            "SELECT COUNT(*) FROM filed_model",
            |s| borrar_presentado(s, 3),
        );
        e.comprobar(
            "Borrar el modelo 200 presentado de 2025",
            "SELECT COUNT(*) FROM filed_model",
            |s| borrar_presentado(s, 4),
        );
        // Lo que no existe no pide nada: el error es el de siempre.
        assert!(borrar_documento(&e.state, 99).is_err());
        assert!(e.presencia.razones().is_empty());
    }

    #[test]
    fn borrar_presupuestos_contactos_y_periodicas_pide_touch_id() {
        let e = env("ventas");
        e.sql(&format!(
            "INSERT INTO quote(id, kind, number, year, seq, issue_date, status, total, draft,
               created_at, updated_at)
               VALUES (1, 'presupuesto', 'P-2026-0003', 2026, 3, '2026-02-01', 'borrador', 100,
               '{{}}', '{NOW}', '{NOW}'),
                      (2, 'presupuesto', 'P-2026-0004', 2026, 4, '2026-02-01', 'enviado', 100,
               '{{}}', '{NOW}', '{NOW}');
             INSERT INTO contact(id, kind, tax_id, name, data, created_at)
               VALUES (5, 'cliente', 'B12345674', 'EMPRESA DEMO SL', '{{}}', '{NOW}');"
        ));
        e.comprobar(
            "Borrar el presupuesto P-2026-0003",
            "SELECT COUNT(*) FROM quote",
            |s| borrar_presupuesto(s, 1),
        );
        // Uno enviado no se borra: lo dice sin pedir Touch ID.
        let err = borrar_presupuesto(&e.state, 2).unwrap_err();
        assert!(
            err.message.contains("ya no es un borrador"),
            "{}",
            err.message
        );
        assert!(e.presencia.razones().is_empty());
        e.comprobar(
            "Borrar el contacto EMPRESA DEMO SL (B12345674)",
            "SELECT COUNT(*) FROM contact",
            |s| borrar_contacto(s, 5),
        );
    }

    #[test]
    fn borrar_una_factura_periodica_pide_touch_id() {
        let e = env("periodica");
        let start = Date::from_calendar_date(2026, time::Month::January, 31).unwrap();
        let t = vec![recurring::InvoiceTemplate {
            id: "plantilla-1".into(),
            name: "Cuota mensual de soporte".into(),
            draft: taxpain_app::invoicing::InvoiceDraft {
                kind: taxpain_core::invoice::InvoiceKind::Completa,
                series: None,
                issue_date: start,
                operation_date: None,
                recipient: None,
                lines: Vec::new(),
                rectified: Vec::new(),
                rectified_amounts: None,
                description: "Soporte".into(),
                notes: None,
                currency: None,
                oss: None,
            },
            language: None,
            every_months: 1,
            start,
            issued: 0,
            day: None,
            end: None,
            active: true,
            last: None,
        }];
        e.state
            .vault()
            .unwrap()
            .set_setting("invoice_templates", &t)
            .unwrap();
        let n = |s: &AppState| {
            recurring::list(&s.vault().unwrap(), Date::MIN)
                .unwrap()
                .len()
        };
        assert_eq!(n(&e.state), 1);
        e.presencia.negar(true);
        assert!(borrar_periodica(&e.state, "plantilla-1").is_err());
        assert_eq!(
            e.presencia.razones(),
            ["Borrar la factura periódica Cuota mensual de soporte"]
        );
        assert_eq!(n(&e.state), 1);
        e.presencia.negar(false);
        borrar_periodica(&e.state, "plantilla-1").unwrap();
        assert_eq!(n(&e.state), 0);
    }

    #[test]
    fn borrar_cripto_y_valores_pide_touch_id() {
        let e = env("inversiones");
        e.sql(&format!(
            "INSERT INTO crypto_platform(id, name, kind, created_at)
               VALUES (1, 'Exchange Demo', 'exchange', '{NOW}'),
                      (2, 'Monedero Demo', 'monedero', '{NOW}');
             INSERT INTO crypto_op(id, date, kind, platform_id, ticker, qty, created_at)
               VALUES (9, '2026-03-05', 'compra', 1, 'BTC', '0.5', '{NOW}');
             INSERT INTO crypto_price(date, ticker, price, updated_at)
               VALUES ('2025-12-31', 'BTC', '80000', '{NOW}');
             INSERT INTO crypto_opening(id, date, updated_at) VALUES (1, '2025-12-31', '{NOW}');
             INSERT INTO security_account(id, broker, name, created_at)
               VALUES (1, 'xtb', 'Cuenta Demo', '{NOW}'), (2, 'ibkr', 'Otra Demo', '{NOW}');
             INSERT INTO security(id, ticker, created_at) VALUES (1, 'DEMO', '{NOW}');
             INSERT INTO security_op(id, account_id, security_id, date, kind, qty, created_at)
               VALUES (4, 1, 1, '2026-04-01', 'compra', '10', '{NOW}');
             INSERT INTO security_price(date, security_id, price, updated_at)
               VALUES ('2025-12-31', 1, '12.5', '{NOW}');"
        ));
        let dia = Date::from_calendar_date(2025, time::Month::December, 31).unwrap();
        // Con operaciones no se pueden borrar: el error de siempre, sin pedir Touch ID.
        assert!(borrar_plataforma_cripto(&e.state, 1).is_err());
        assert!(borrar_cuenta_valores(&e.state, 1).is_err());
        assert!(e.presencia.razones().is_empty());
        e.comprobar(
            "Borrar la plataforma de cripto Monedero Demo",
            "SELECT COUNT(*) FROM crypto_platform",
            |s| borrar_plataforma_cripto(s, 2),
        );
        e.comprobar(
            "Borrar la operación de cripto del 05/03/2026 (compra de 0.5 BTC)",
            "SELECT COUNT(*) FROM crypto_op",
            |s| borrar_op_cripto(s, 9),
        );
        e.comprobar(
            "Borrar el precio de BTC del 31/12/2025",
            "SELECT COUNT(*) FROM crypto_price",
            |s| borrar_precio_cripto(s, dia, " btc"),
        );
        e.comprobar(
            "Quitar el punto de partida de las criptomonedas (a 31/12/2025)",
            "SELECT COUNT(*) FROM crypto_opening",
            borrar_inicio_cripto,
        );
        e.comprobar(
            "Borrar la cuenta de valores Otra Demo",
            "SELECT COUNT(*) FROM security_account",
            |s| borrar_cuenta_valores(s, 2),
        );
        e.comprobar(
            "Borrar la operación de valores del 01/04/2026 (compra de DEMO)",
            "SELECT COUNT(*) FROM security_op",
            |s| borrar_op_valores(s, 4),
        );
        e.comprobar(
            "Borrar el precio de DEMO del 31/12/2025",
            "SELECT COUNT(*) FROM security_price",
            |s| borrar_precio_valores(s, dia, 1),
        );
    }

    #[test]
    fn borrar_en_la_contabilidad_pide_touch_id() {
        let e = env("contabilidad");
        e.sql(&format!(
            "INSERT INTO journal_entry(id, fiscal_year, number, date, concept, origin_kind,
               created_at)
               VALUES (1, 2026, 12, '2026-02-03', 'Ajuste de prueba', 'Manual', '{NOW}'),
                      (2, 2026, 13, '2026-02-04', 'Otro', 'Manual', '{NOW}'),
                      (3, 2026, 14, '2026-02-05', 'Y otro', 'Importado', '{NOW}'),
                      (4, 2026, 15, '2026-02-06', 'Factura F-1', 'Factura', '{NOW}');
             INSERT INTO cost_center(code, name, created_at) VALUES ('T1', 'Tienda', '{NOW}');"
        ));
        // Sin `confirmed`, o si el asiento no se puede borrar, no se pregunta: el error es el de
        // siempre.
        assert!(borrar_asiento(&e.state, 1, false).is_err());
        assert!(borrar_asiento(&e.state, 4, true).is_err());
        assert!(borrar_asientos(&e.state, &[1, 4], true).is_err());
        assert!(e.presencia.razones().is_empty());
        assert_eq!(e.cuenta("SELECT COUNT(*) FROM journal_entry"), 4);
        e.comprobar(
            "Borrar el asiento 12 del 03/02/2026 (Ajuste de prueba)",
            "SELECT COUNT(*) FROM journal_entry",
            |s| borrar_asiento(s, 1, true),
        );
        e.comprobar(
            "Borrar 2 asientos del diario",
            "SELECT COUNT(*) FROM journal_entry",
            |s| borrar_asientos(s, &[2, 3], true),
        );
        e.comprobar(
            "Borrar el centro de coste T1 Tienda",
            "SELECT COUNT(*) FROM cost_center",
            |s| borrar_centro_coste(s, "T1", true),
        );
    }

    #[test]
    fn borrar_nominas_viajes_socios_y_participes_pide_touch_id() {
        let e = env("personas");
        e.sql(&format!(
            "INSERT INTO employee(id, nif, name, updated_at)
               VALUES (1, '12345678Z', 'Persona Demo', '{NOW}');
             INSERT INTO payslip(id, employee_id, date, gross, irpf, ss_employee, ss_company,
               created_at)
               VALUES (1, 1, '2026-01-31', 200000, 0, 0, 0, '{NOW}');
             INSERT INTO travel_expense(id, date, nif, name, origin, destination, reason, exempt,
               taxable, counter_account, created_at)
               VALUES (1, '2026-02-10', '12345678Z', 'Persona Demo', 'Valencia', 'Madrid',
               'Feria', 0, 0, '5720000', '{NOW}');
             INSERT INTO shareholder(id, nif, name, pct, created_at)
               VALUES (1, '12345678Z', 'Persona Demo', 1000000, '{NOW}');
             INSERT INTO reta_cuota(id, employee_id, date, amount, pagador, created_at)
               VALUES (1, 1, '2026-01-30', 31400, 'Sociedad', '{NOW}');
             INSERT INTO partner(id, nif, name, province, pct, created_at)
               VALUES (1, '12345678Z', 'Persona Demo', '46', 1000000, '{NOW}');"
        ));
        let razones = |hacer: &dyn Fn(&AppState) -> CmdResult<()>| {
            e.presencia.olvidar();
            e.presencia.negar(true);
            let err = hacer(&e.state).unwrap_err();
            assert_eq!(err.message, "operación no confirmada por el usuario");
            e.presencia.razones()
        };
        assert_eq!(
            razones(&|s| borrar_nomina(s, 1, true)),
            ["Borrar la nómina de Persona Demo del 31/01/2026"]
        );
        assert_eq!(
            razones(&|s| borrar_viaje(s, 1, true)),
            ["Borrar el viaje de Persona Demo a Madrid del 10/02/2026"]
        );
        assert_eq!(e.cuenta("SELECT COUNT(*) FROM payslip"), 1);
        assert_eq!(e.cuenta("SELECT COUNT(*) FROM travel_expense"), 1);
        e.comprobar(
            "Borrar la nómina de Persona Demo del 31/01/2026",
            "SELECT COUNT(*) FROM payslip",
            |s| borrar_nomina(s, 1, true),
        );
        e.comprobar(
            "Borrar la cuota de autónomos de Persona Demo del 30/01/2026",
            "SELECT COUNT(*) FROM reta_cuota",
            |s| borrar_cuota_reta(s, 1, true),
        );
        e.comprobar(
            "Borrar el viaje de Persona Demo a Madrid del 10/02/2026",
            "SELECT COUNT(*) FROM travel_expense",
            |s| borrar_viaje(s, 1, true),
        );
        e.comprobar(
            "Borrar el socio Persona Demo (12345678Z)",
            "SELECT COUNT(*) FROM shareholder",
            |s| borrar_socio(s, 1),
        );
        e.comprobar(
            "Borrar el partícipe Persona Demo (12345678Z) del modelo 184",
            "SELECT COUNT(*) FROM partner",
            |s| borrar_participe(s, 1),
        );
    }

    #[test]
    fn la_clave_del_asistente_pide_touch_id() {
        let e = env("clave");
        let st = assistant::status(&e.state.vault().unwrap()).unwrap();
        if !st.remote {
            // Con un modelo local no hay clave: ni se pregunta.
            assert!(guardar_clave_asistente(&e.state, Some("sk-prueba")).is_err());
            assert!(e.presencia.razones().is_empty());
            return;
        }
        let tiene = |s: &AppState| assistant::status(&s.vault().unwrap()).unwrap().has_api_key;
        e.presencia.negar(true);
        assert!(guardar_clave_asistente(&e.state, Some("sk-prueba")).is_err());
        assert!(!tiene(&e.state));
        let r = e.presencia.razones();
        assert!(
            r.len() == 1 && r[0].starts_with("Guardar la clave de API del asistente para "),
            "{r:?}"
        );
        e.presencia.negar(false);
        guardar_clave_asistente(&e.state, Some("sk-prueba")).unwrap();
        assert!(tiene(&e.state));

        e.presencia.olvidar();
        e.presencia.negar(true);
        assert!(guardar_clave_asistente(&e.state, Some("  ")).is_err());
        assert!(tiene(&e.state), "sin confirmar no se quita");
        assert!(e.presencia.razones()[0].starts_with("Quitar la clave de API del asistente"));
        e.presencia.negar(false);
        guardar_clave_asistente(&e.state, None).unwrap();
        assert!(!tiene(&e.state));
    }

    #[test]
    fn activar_full_ia_pide_touch_id_y_desactivarlo_no() {
        let e = env("full-ia");
        let full = |s: &AppState| lectura_ia::settings(&s.vault().unwrap()).unwrap().full;
        let on = lectura_ia::Settings {
            full: true,
            ..Default::default()
        };
        e.presencia.negar(true);
        assert!(guardar_lectura_ia(&e.state, &on).is_err());
        assert!(!full(&e.state));
        let r = e.presencia.razones();
        assert!(
            r.len() == 1
                && r[0].starts_with("Activar «Full IA»")
                && r[0].contains("sin anonimizar"),
            "{r:?}"
        );
        e.presencia.negar(false);
        guardar_lectura_ia(&e.state, &on).unwrap();
        assert!(full(&e.state));
        // Ya activo (cambiar el modelo) o desactivarlo: sin preguntar.
        e.presencia.olvidar();
        e.presencia.negar(true);
        guardar_lectura_ia(
            &e.state,
            &lectura_ia::Settings {
                model: Some("otro".into()),
                ..on.clone()
            },
        )
        .unwrap();
        guardar_lectura_ia(&e.state, &lectura_ia::Settings::default()).unwrap();
        assert!(!full(&e.state));
        assert!(e.presencia.razones().is_empty());
    }

    #[test]
    fn vigilar_una_carpeta_nueva_pide_touch_id_y_quitarla_no() {
        let e = env("vigiladas");
        let a = e.dir.join("Facturas");
        let b = e.dir.join("Tickets");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let a = a.to_string_lossy().into_owned();
        let b = b.to_string_lossy().into_owned();
        let carpetas = |s: &AppState| vigiladas::folders(&s.vault().unwrap()).unwrap();

        e.presencia.negar(true);
        assert!(guardar_carpetas_vigiladas(&e.state, std::slice::from_ref(&a)).is_err());
        assert!(carpetas(&e.state).is_empty());
        assert_eq!(
            e.presencia.razones(),
            [format!("Vigilar la carpeta {}", ruta(&a))]
        );
        e.presencia.negar(false);
        guardar_carpetas_vigiladas(&e.state, &[format!("{a}/")]).unwrap();
        assert_eq!(carpetas(&e.state), std::slice::from_ref(&a));

        // La misma otra vez no pregunta; una más, sí.
        e.presencia.olvidar();
        e.presencia.negar(true);
        guardar_carpetas_vigiladas(&e.state, std::slice::from_ref(&a)).unwrap();
        assert!(guardar_carpetas_vigiladas(&e.state, &[a.clone(), b.clone()]).is_err());
        assert_eq!(
            e.presencia.razones(),
            [format!("Vigilar la carpeta {}", ruta(&b))]
        );
        assert_eq!(carpetas(&e.state), std::slice::from_ref(&a));
        // Quitar: sin preguntar.
        e.presencia.olvidar();
        guardar_carpetas_vigiladas(&e.state, &[]).unwrap();
        assert!(carpetas(&e.state).is_empty());
        assert!(e.presencia.razones().is_empty());
    }

    /// Una plataforma en el punto de partida no se borra (sin preguntar), y quitar el punto de
    /// partida pregunta también si sólo quedan saldos sueltos, sin la cabecera.
    #[test]
    fn punto_de_partida_de_cripto() {
        let e = env("inicio-cripto");
        e.sql(&format!(
            "INSERT INTO crypto_platform(id, name, kind, created_at)
               VALUES (1, 'Exchange Demo', 'exchange', '{NOW}');
             INSERT INTO crypto_opening_position(id, ticker, platform_id, qty, cost)
               VALUES (1, 'BTC', 1, '0.1', 300000);"
        ));
        assert!(borrar_plataforma_cripto(&e.state, 1).is_err());
        assert!(e.presencia.razones().is_empty());
        let quitados =
            "SELECT COUNT(*) FROM audit_log WHERE action = 'cripto.punto_de_partida_quitado'";
        assert_eq!(e.cuenta(quitados), 0);
        e.comprobar(
            "Quitar el punto de partida de las criptomonedas (1 saldo sin fecha)",
            "SELECT COUNT(*) FROM crypto_opening_position",
            borrar_inicio_cripto,
        );
        assert_eq!(e.cuenta(quitados), 1, "queda en la auditoría");
        // Ya no hay nada: no se pregunta.
        borrar_inicio_cripto(&e.state).unwrap();
        assert!(e.presencia.razones().is_empty());
    }

    #[test]
    fn leasing_con_cuotas_y_deshacer_dividendos() {
        let e = env("leasing-dividendos");
        // Como lo guarda `leasing::create`: con su id también dentro.
        let contrato = |id: i64, name: &str| {
            serde_json::to_string(&taxpain_core::accounting::leasing::Leasing {
                id,
                name: name.into(),
                start: Date::from_calendar_date(2026, time::Month::January, 1).unwrap(),
                principal: Money::from_cents(2_000_000),
                months: 36,
                rate: taxpain_core::money::Rate::from_bp(500),
                purchase_option: Money::from_cents(50_000),
                vat: taxpain_core::money::Rate::from_bp(2100),
                asset_account: taxpain_core::accounting::AccountCode::new("2180000").unwrap(),
                debt_account: taxpain_core::accounting::AccountCode::new("5240001").unwrap(),
                lessor: None,
            })
            .unwrap()
            .replace('\'', "''")
        };
        e.sql(&format!(
            "INSERT INTO leasing(id, data, created_at) VALUES (1, '{}', '{NOW}'),
               (2, '{}', '{NOW}');
             INSERT INTO leasing_installment(leasing_id, n, entry_id) VALUES (2, 1, NULL);
             INSERT INTO dividend_payment(id, date, gross, withheld, treasury, created_at)
               VALUES (1, '2026-03-31', 100000, 19000, '5720000', '{NOW}');
             INSERT INTO dividend_line(payment_id, nif, name, province, legal_person, resident,
               pct, gross, rate_bp, withheld)
               VALUES (1, '12345678Z', 'Persona Demo', '46', 0, 1, 1000000, 100000, 1900,
               19000);",
            contrato(1, "Furgoneta"),
            contrato(2, "Carretilla")
        ));
        // Con una cuota contabilizada no se borra: el error de siempre, sin pedir Touch ID.
        let err = borrar_leasing(&e.state, 2, true).unwrap_err();
        assert!(
            err.message.contains("cuotas contabilizadas"),
            "{}",
            err.message
        );
        assert!(e.presencia.razones().is_empty());
        e.presencia.negar(true);
        assert!(borrar_leasing(&e.state, 1, true).is_err());
        assert_eq!(
            e.presencia.razones(),
            ["Borrar el contrato de leasing Furgoneta"]
        );
        assert_eq!(e.cuenta("SELECT COUNT(*) FROM leasing"), 2);
        e.presencia.olvidar();
        e.presencia.negar(false);

        // Deshacer un pago de dividendos: sin `confirmed`, el error de siempre sin preguntar.
        assert!(deshacer_dividendo(&e.state, 1, false).is_err());
        assert!(deshacer_dividendo(&e.state, 99, true).is_err());
        assert!(e.presencia.razones().is_empty());
        e.comprobar(
            "Deshacer el pago de dividendos del 31/03/2026 (bruto 1.000,00 €, retención de \
             190,00 €)",
            "SELECT COUNT(*) FROM dividend_payment",
            |s| deshacer_dividendo(s, 1, true),
        );
        assert_eq!(e.cuenta("SELECT COUNT(*) FROM dividend_line"), 0);
    }

    /// Un pago con el asiento en un ejercicio cerrado no se deshace: el error de siempre, sin
    /// pedir Touch ID. Reabierto el ejercicio, sí pregunta.
    #[test]
    fn dividendo_de_un_ejercicio_cerrado() {
        let e = env("dividendo-cerrado");
        e.sql(&format!(
            "INSERT INTO fiscal_year(year, start_date, end_date)
               VALUES (2025, '2025-01-01', '2025-12-31');
             INSERT INTO journal_entry(id, fiscal_year, number, date, concept, origin_kind,
               created_at)
               VALUES (7, 2025, 40, '2025-06-30', 'Dividendos de prueba', 'Manual', '{NOW}');
             INSERT INTO journal_line(entry_id, account, debit, credit)
               VALUES (7, '5260000', 50000, 0), (7, '5720000', 0, 50000);
             UPDATE fiscal_year SET closed = 1, closed_at = '{NOW}' WHERE year = 2025;
             INSERT INTO dividend_payment(id, date, gross, withheld, treasury, entry_id,
               created_at)
               VALUES (1, '2025-06-30', 50000, 0, '5720000', 7, '{NOW}');"
        ));
        let err = deshacer_dividendo(&e.state, 1, true).unwrap_err();
        assert!(err.message.contains("cerrado"), "{}", err.message);
        assert!(e.presencia.razones().is_empty());
        assert_eq!(e.cuenta("SELECT COUNT(*) FROM dividend_payment"), 1);

        e.sql("UPDATE fiscal_year SET closed = 0, closed_at = NULL WHERE year = 2025;");
        e.comprobar(
            "Deshacer el pago de dividendos del 30/06/2025 (bruto 500,00 €, retención de \
             0,00 €)",
            "SELECT COUNT(*) FROM dividend_payment",
            |s| deshacer_dividendo(s, 1, true),
        );
        assert_eq!(e.cuenta("SELECT COUNT(*) FROM journal_entry"), 0);
    }

    /// El proveedor y la clave se guardan juntos: si se cancela Touch ID, no cambia ninguno.
    #[test]
    fn la_clave_se_guarda_con_el_proveedor_nuevo() {
        let e = env("clave-proveedor");
        let st = |s: &AppState| assistant::status(&s.vault().unwrap()).unwrap();
        let antes = st(&e.state).settings;
        let mistral = AssistantSettings {
            provider: taxpain_assistant::Provider::OpenAiCompatible {
                base_url: "https://api.mistral.ai/v1".into(),
                model: "mistral-large-latest".into(),
            },
            consent: true,
        };
        e.presencia.negar(true);
        let err = guardar_asistente(&e.state, &mistral, Some("sk-prueba")).unwrap_err();
        assert_eq!(err.message, "operación no confirmada por el usuario");
        assert_eq!(
            e.presencia.razones(),
            ["Guardar la clave de API del asistente para api.mistral.ai"]
        );
        assert_eq!(st(&e.state).settings, antes, "el proveedor no ha cambiado");
        assert!(!st(&e.state).has_api_key);

        e.presencia.negar(false);
        // `false`: el proveedor es de otro host y lo aceptado hay que guardarlo otra vez.
        assert!(!guardar_asistente(&e.state, &mistral, Some("sk-prueba")).unwrap());
        let ahora = st(&e.state);
        assert_eq!(ahora.settings.provider, mistral.provider);
        assert!(ahora.has_api_key, "la clave va con el proveedor nuevo");

        // Sin clave (aceptar el envío ya con este proveedor): sin preguntar.
        e.presencia.olvidar();
        e.presencia.negar(true);
        assert!(guardar_asistente(&e.state, &mistral, None).unwrap());
        assert!(guardar_asistente(&e.state, &mistral, Some("  ")).unwrap());
        assert!(st(&e.state).settings.consent);
        // Un modelo local no lleva clave: error sin preguntar y sin cambiar el proveedor.
        let local = AssistantSettings {
            provider: taxpain_assistant::Provider::OpenAiCompatible {
                base_url: "http://localhost:11434/v1".into(),
                model: "llama3.1".into(),
            },
            consent: false,
        };
        assert!(guardar_asistente(&e.state, &local, Some("sk-prueba")).is_err());
        assert_eq!(st(&e.state).settings.provider, mistral.provider);
        assert!(e.presencia.razones().is_empty());
    }

    /// Si el dato cambia mientras se confirma, no se borra otra cosa de la que se ha visto.
    #[test]
    fn si_cambia_mientras_se_confirma_no_se_hace_nada() {
        let e = env("carrera");
        e.sql(&format!(
            "INSERT INTO contact(id, kind, tax_id, name, data, created_at)
               VALUES (5, 'cliente', 'B12345674', 'EMPRESA DEMO SL', '{{}}', '{NOW}');"
        ));
        let veces = AtomicBool::new(false);
        let r = con_presencia(&e.state, |v| {
            // La segunda vez (al volver de Touch ID), el contacto ya se llama de otra forma.
            if veces.swap(true, Ordering::SeqCst) {
                v.connection()
                    .execute("UPDATE contact SET name = 'OTRA DEMO SL' WHERE id = 5", [])
                    .unwrap();
            }
            let n: String = v
                .connection()
                .query_row("SELECT name FROM contact WHERE id = 5", [], |r| r.get(0))
                .unwrap();
            Ok(Some(format!("Borrar el contacto {n}")))
        });
        let err = r.err().unwrap();
        assert!(err.message.contains("ha cambiado"), "{}", err.message);
        assert_eq!(e.cuenta("SELECT COUNT(*) FROM contact"), 1);
    }
}

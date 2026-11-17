//! Préstamos concedidos y recibidos (PGC de PYMES, NRV 9.ª: activos y pasivos financieros a
//! coste amortizado): cuadro de amortización, asientos del alta, de cada cuota, del devengo de
//! los intereses a una fecha (periodificación al cierre) y la parte que vence a más de doce
//! meses (reclasificación entre el largo y el corto plazo).
//!
//! Sistemas:
//! - **Francés**: cuota constante (intereses + capital), `A·i / (1 − (1 + i)^−m)`.
//! - **Lineal**: capital constante; los intereses bajan con el capital pendiente.
//! - **Intereses periódicos**: sólo intereses en cada cuota y todo el capital en la última.
//! - **Al vencimiento**: una sola cuota al final con el capital y los intereses (simples) de
//!   todo el plazo.
//!
//! Carencia (los primeros meses del plazo): **de capital** (se pagan sólo los intereses) o
//! **total** (no se paga nada y los intereses se suman al capital pendiente: se capitalizan).
//!
//! Tipo nominal anual repartido por periodos (`i` = tipo × meses del periodo / 12), como el
//! leasing ([`super::leasing`]), en coma fija de 10⁻¹² y redondeo al céntimo en cada cuota; la
//! última cuota devuelve exactamente el capital pendiente, de modo que el cuadro no descuadra.
//! El tipo efectivo (TAE) no se calcula: el coste amortizado con comisiones de apertura
//! (NRV 9.ª 3.1) queda fuera (`TODO(VERIFY)`: si hay comisiones relevantes, la gestoría ajusta
//! el tipo efectivo).

use serde::{Deserialize, Serialize};
use time::Date;

use super::account::AccountCode;
use super::entry::{EntryLine, JournalEntry, Origin};
use crate::money::{Money, Rate};

/// Escala de la coma fija (10⁻¹²).
pub(crate) const ESCALA: i128 = 1_000_000_000_000;

/// Cuenta de las retenciones que nos practican al cobrar intereses (pagos a cuenta del IS).
pub const CUENTA_RETENCION_SOPORTADA: &str = "473";
/// Cuenta de las retenciones que practicamos al pagar intereses (se ingresan con el 123).
pub const CUENTA_RETENCION_PRACTICADA: &str = "4751";

/// `d` más `months` meses (si el día no existe en ese mes, el último del mes).
pub(crate) fn sumar_meses(d: Date, months: u32) -> Date {
    let total = i64::from(u8::from(d.month())) - 1 + i64::from(months);
    let year = i32::try_from(i64::from(d.year()) + total.div_euclid(12)).unwrap_or(d.year());
    let month = time::Month::try_from(u8::try_from(total.rem_euclid(12) + 1).unwrap_or(1))
        .unwrap_or(time::Month::January);
    let day = d.day().min(month.length(year));
    Date::from_calendar_date(year, month, day).unwrap_or(d)
}

/// Redondea al entero (céntimos) un valor en coma fija, mitad lejos del cero.
pub(crate) fn redondear(x: i128) -> i64 {
    let q = x / ESCALA;
    let r = x % ESCALA;
    let q = if 2 * r.abs() >= ESCALA {
        q + r.signum()
    } else {
        q
    };
    i64::try_from(q).unwrap_or(i64::MAX)
}

/// `(1 + i)^n` en coma fija.
pub(crate) fn potencia(i: i128, n: u32) -> i128 {
    let mut pow = ESCALA;
    for _ in 0..n {
        pow = pow * (ESCALA + i) / ESCALA;
    }
    pow
}

/// Cuota constante que amortiza `capital` en `n` periodos al tipo por periodo `i` (coma fija).
pub(crate) fn anualidad(capital: Money, i: i128, n: u32) -> Money {
    let n = n.max(1);
    if i == 0 {
        return Money::from_cents(capital.cents() / i64::from(n));
    }
    let pow = potencia(i, n);
    // cuota = C · i · (1+i)^n / ((1+i)^n − 1)
    let num = i128::from(capital.cents()) * i * pow / ESCALA;
    let den = pow - ESCALA;
    Money::from_cents(redondear(num * ESCALA / den))
}

/// Concedido (la sociedad presta: activo, grupo 2/5) o recibido (le prestan: pasivo, 1/5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sentido {
    Concedido,
    Recibido,
}

impl Sentido {
    pub fn as_str(self) -> &'static str {
        match self {
            Sentido::Concedido => "concedido",
            Sentido::Recibido => "recibido",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sistema {
    Frances,
    Lineal,
    /// Intereses en cada cuota y el capital entero en la última («bullet»).
    InteresesPeriodicos,
    /// Capital e intereses en una sola cuota al final del plazo.
    AlVencimiento,
}

impl Sistema {
    pub fn as_str(self) -> &'static str {
        match self {
            Sistema::Frances => "frances",
            Sistema::Lineal => "lineal",
            Sistema::InteresesPeriodicos => "intereses_periodicos",
            Sistema::AlVencimiento => "al_vencimiento",
        }
    }

    pub fn nombre(self) -> &'static str {
        match self {
            Sistema::Frances => "francés (cuota constante)",
            Sistema::Lineal => "lineal (amortización constante)",
            Sistema::InteresesPeriodicos => "intereses periódicos y capital al vencimiento",
            Sistema::AlVencimiento => "capital e intereses al vencimiento",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoCarencia {
    /// Sólo se pagan los intereses.
    #[default]
    Capital,
    /// No se paga nada: los intereses se capitalizan.
    Total,
}

/// Vínculo con la contraparte (art. 18.2 LIS; en el PGC, empresas del grupo, asociadas y otras
/// partes vinculadas: NECA 13.ª y 15.ª). Decide las subcuentas que se proponen (…3 grupo,
/// …4 asociadas, …5 otras partes vinculadas) y los avisos de valor de mercado.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vinculo {
    #[default]
    Ninguno,
    Grupo,
    Asociada,
    /// Otra parte vinculada: socio con el 25 % o más, administrador, sus familiares…
    Otra,
}

impl Vinculo {
    pub fn as_str(self) -> &'static str {
        match self {
            Vinculo::Ninguno => "ninguno",
            Vinculo::Grupo => "grupo",
            Vinculo::Asociada => "asociada",
            Vinculo::Otra => "otra",
        }
    }
}

/// De dónde sale la contraparte: un contacto, un socio, una participada (aún no hay ficha de
/// participadas: va por su nombre y NIF) o un texto libre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrigenContraparte {
    Contacto,
    Socio,
    Participada,
    #[default]
    Texto,
}

impl OrigenContraparte {
    pub fn as_str(self) -> &'static str {
        match self {
            OrigenContraparte::Contacto => "contacto",
            OrigenContraparte::Socio => "socio",
            OrigenContraparte::Participada => "participada",
            OrigenContraparte::Texto => "texto",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contraparte {
    pub origen: OrigenContraparte,
    /// Id del contacto o del socio (si `origen` es uno de ellos).
    #[serde(default)]
    pub ref_id: Option<i64>,
    #[serde(default)]
    pub nif: String,
    pub nombre: String,
    /// Código de provincia del domicilio (01–53), para el 193.
    #[serde(default)]
    pub provincia: String,
    #[serde(default)]
    pub persona_fisica: bool,
    /// Banco o entidad de crédito (sin retención sobre sus intereses).
    #[serde(default)]
    pub entidad_credito: bool,
    #[serde(default)]
    pub vinculo: Vinculo,
}

impl Contraparte {
    pub fn vinculada(&self) -> bool {
        self.vinculo != Vinculo::Ninguno
    }
}

/// Subcuentas del préstamo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cuentas {
    /// Capital a más de doce meses (252, 2423…, 170, 171, 1633…). Si es la misma que `corto`
    /// (una cuenta corriente 551/552), no se reclasifica.
    pub largo: AccountCode,
    /// Capital a doce meses o menos (542, 5323…, 520, 521, 5133…).
    pub corto: AccountCode,
    /// Ingreso (762) o gasto (662) por intereses.
    pub intereses: AccountCode,
    /// Intereses devengados y no vencidos (547, 5343…; 527, 528, 5143…).
    pub devengo: AccountCode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prestamo {
    #[serde(default)]
    pub id: i64,
    /// Nombre corto («Préstamo a Participada Demo SL»).
    pub nombre: String,
    pub sentido: Sentido,
    pub contraparte: Contraparte,
    /// Fecha del contrato y de la entrega del dinero.
    pub inicio: Date,
    pub principal: Money,
    /// Tipo nominal anual (5 % = 500).
    pub tipo: Rate,
    pub sistema: Sistema,
    /// Meses entre cuotas: 1, 3, 6 o 12 (con «al vencimiento» no cuenta).
    pub periodicidad: u8,
    /// Plazo total en meses (incluida la carencia).
    pub plazo: u16,
    /// Meses de carencia al principio (múltiplo de la periodicidad).
    #[serde(default)]
    pub carencia: u16,
    #[serde(default)]
    pub carencia_tipo: TipoCarencia,
    pub cuentas: Cuentas,
    /// Retención sobre los intereses de cada cuota (19 % = 1900; 0 si no hay).
    #[serde(default)]
    pub retencion: Rate,
    #[serde(default)]
    pub notas: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cuota {
    /// Número de cuota (1…n).
    pub n: u16,
    /// Vencimiento.
    pub fecha: Date,
    /// Intereses del periodo.
    pub intereses: Money,
    /// Capital que se devuelve.
    pub capital: Money,
    /// Intereses que no se pagan y se suman al capital (carencia total).
    pub capitalizados: Money,
    /// Lo que se paga (o cobra) ese día: capital + intereses − capitalizados.
    pub pago: Money,
    /// Capital pendiente tras la cuota.
    pub pendiente: Money,
}

/// Datos que no cuadran para hacer el cuadro.
pub fn comprobar(p: &Prestamo) -> Result<(), String> {
    if p.principal.cents() <= 0 {
        return Err("el principal ha de ser mayor que cero".into());
    }
    if p.tipo.bp() < 0 || p.tipo.bp() > 10_000 {
        return Err("el tipo de interés anual va de 0 a 100 %".into());
    }
    if p.retencion.bp() < 0 || p.retencion.bp() > 10_000 {
        return Err("la retención va de 0 a 100 %".into());
    }
    if p.plazo == 0 || p.plazo > 600 {
        return Err("el plazo va de 1 a 600 meses".into());
    }
    if p.sistema == Sistema::AlVencimiento {
        if p.carencia > 0 {
            return Err("un préstamo con todo al vencimiento no lleva carencia".into());
        }
        return Ok(());
    }
    if ![1, 3, 6, 12].contains(&p.periodicidad) {
        return Err("las cuotas son mensuales, trimestrales, semestrales o anuales".into());
    }
    let per = u16::from(p.periodicidad);
    if !p.plazo.is_multiple_of(per) {
        return Err(format!(
            "el plazo ({} meses) ha de ser múltiplo de la periodicidad ({per} meses)",
            p.plazo
        ));
    }
    if !p.carencia.is_multiple_of(per) {
        return Err(format!(
            "la carencia ({} meses) ha de ser múltiplo de la periodicidad ({per} meses)",
            p.carencia
        ));
    }
    if p.carencia >= p.plazo {
        return Err("la carencia ha de ser menor que el plazo".into());
    }
    Ok(())
}

/// Meses de cada periodo y número de cuotas.
fn periodos(p: &Prestamo) -> (u32, u32) {
    if p.sistema == Sistema::AlVencimiento {
        return (u32::from(p.plazo.max(1)), 1);
    }
    let per = u32::from(p.periodicidad.max(1));
    (per, (u32::from(p.plazo) / per).max(1))
}

/// Tipo por periodo de `meses` meses en coma fija: bp / 10000 × meses / 12.
fn tipo_periodo(tipo: Rate, meses: u32) -> i128 {
    i128::from(tipo.bp()) * i128::from(meses) * ESCALA / 120_000
}

/// Cuadro de amortización. La última cuota devuelve el capital pendiente exacto.
pub fn cuadro(p: &Prestamo) -> Vec<Cuota> {
    let (per, n) = periodos(p);
    let i = tipo_periodo(p.tipo, per);
    let gracia = if p.sistema == Sistema::AlVencimiento {
        0
    } else {
        (u32::from(p.carencia) / per).min(n.saturating_sub(1))
    };
    let mut pendiente = p.principal;
    let mut out = Vec::with_capacity(usize::try_from(n).unwrap_or(0));
    let interes =
        |pendiente: Money| Money::from_cents(redondear(i128::from(pendiente.cents()) * i));
    for k in 1..=gracia {
        let intereses = interes(pendiente);
        let capitalizados = if p.carencia_tipo == TipoCarencia::Total {
            intereses
        } else {
            Money::ZERO
        };
        pendiente += capitalizados;
        out.push(Cuota {
            n: u16::try_from(k).unwrap_or(u16::MAX),
            fecha: sumar_meses(p.inicio, per * k),
            intereses,
            capital: Money::ZERO,
            capitalizados,
            pago: intereses - capitalizados,
            pendiente,
        });
    }
    let resto = n - gracia;
    let base = pendiente;
    let cuota_francesa = anualidad(base, i, resto);
    let capital_lineal = Money::from_cents(base.cents() / i64::from(resto.max(1)));
    for j in 1..=resto {
        let k = gracia + j;
        let intereses = interes(pendiente);
        let ultima = j == resto;
        let capital = if ultima {
            pendiente
        } else {
            match p.sistema {
                Sistema::Frances => (cuota_francesa - intereses).max(Money::ZERO).min(pendiente),
                Sistema::Lineal => capital_lineal.min(pendiente),
                Sistema::InteresesPeriodicos | Sistema::AlVencimiento => Money::ZERO,
            }
        };
        pendiente -= capital;
        out.push(Cuota {
            n: u16::try_from(k).unwrap_or(u16::MAX),
            fecha: sumar_meses(p.inicio, per * k),
            intereses,
            capital,
            capitalizados: Money::ZERO,
            pago: intereses + capital,
            pendiente,
        });
    }
    out
}

/// Capital pendiente según el cuadro el día `fecha` (tras las cuotas que vencen ese día o
/// antes). Antes del inicio, cero.
pub fn pendiente_a(p: &Prestamo, cuotas: &[Cuota], fecha: Date) -> Money {
    if fecha < p.inicio {
        return Money::ZERO;
    }
    cuotas
        .iter()
        .rfind(|q| q.fecha <= fecha)
        .map_or(p.principal, |q| q.pendiente)
}

/// Capital que vence en los doce meses siguientes a `fecha` (deuda o crédito a corto plazo).
pub fn corto_plazo_a(cuotas: &[Cuota], fecha: Date) -> Money {
    let limite = sumar_meses(fecha, 12);
    cuotas
        .iter()
        .filter(|q| q.fecha > fecha && q.fecha <= limite)
        .map(|q| q.capital)
        .sum()
}

/// Capital pendiente el día `fecha` que vence a más de doce meses (largo plazo). Los intereses
/// que se capitalizarán más adelante no cuentan (aún no son capital).
pub fn largo_plazo_a(p: &Prestamo, cuotas: &[Cuota], fecha: Date) -> Money {
    (pendiente_a(p, cuotas, fecha) - corto_plazo_a(cuotas, fecha)).max(Money::ZERO)
}

/// Intereses de la cuota en curso el día `fecha`, devengados desde el vencimiento anterior (o
/// desde el inicio) hasta ese día, en proporción a los días del periodo: (número de cuota,
/// importe). `None` si `fecha` no cae dentro de ningún periodo (antes del inicio, en un
/// vencimiento o después del último).
pub fn devengado_a(p: &Prestamo, cuotas: &[Cuota], fecha: Date) -> Option<(u16, Money)> {
    let pos = cuotas.iter().position(|q| q.fecha > fecha)?;
    let desde = if pos == 0 {
        p.inicio
    } else {
        cuotas.get(pos - 1)?.fecha
    };
    if fecha <= desde {
        return None;
    }
    let q = cuotas.get(pos)?;
    let total = (q.fecha - desde).whole_days();
    let hechos = (fecha - desde).whole_days();
    if total <= 0 {
        return None;
    }
    let importe = redondear(
        i128::from(q.intereses.cents()) * i128::from(hechos) * ESCALA / i128::from(total),
    );
    Some((q.n, Money::from_cents(importe)))
}

/// Retención de los intereses que se pagan en la cuota (los capitalizados no se pagan).
pub fn retencion_de(p: &Prestamo, q: &Cuota) -> Money {
    (q.intereses - q.capitalizados).apply_rate(p.retencion)
}

fn concepto(p: &Prestamo, que: &str) -> String {
    format!("Préstamo {}: {que}", p.nombre)
}

/// Asiento del alta: entrega (concedido) o recepción (recibido) del principal contra la
/// tesorería, con la parte que vence en doce meses en la cuenta de corto plazo y el resto en la
/// de largo plazo.
pub fn asiento_alta(p: &Prestamo, tesoreria: &AccountCode) -> JournalEntry {
    let cuotas = cuadro(p);
    let corto = corto_plazo_a(&cuotas, p.inicio).min(p.principal);
    let largo = p.principal - corto;
    let activo = p.sentido == Sentido::Concedido;
    let lines = if p.cuentas.largo == p.cuentas.corto {
        vec![
            EntryLine::signed(p.cuentas.corto.clone(), p.principal, activo),
            EntryLine::signed(tesoreria.clone(), p.principal, !activo),
        ]
    } else {
        vec![
            EntryLine::signed(p.cuentas.largo.clone(), largo, activo),
            EntryLine::signed(p.cuentas.corto.clone(), corto, activo),
            EntryLine::signed(tesoreria.clone(), p.principal, !activo),
        ]
    };
    JournalEntry {
        date: p.inicio,
        concept: concepto(
            p,
            if activo {
                "entrega del principal"
            } else {
                "recepción del principal"
            },
        ),
        origin: Origin::Prestamo(p.id),
        lines: lines.into_iter().flatten().collect(),
    }
}

/// Asiento de una cuota el día `fecha`: capital a la cuenta de corto plazo, intereses (menos lo
/// ya devengado, que sale de la cuenta de devengo) a la de intereses, retención a la 473
/// (concedido: nos la practican) o a la 4751 (recibido: la practicamos) y el neto contra la
/// tesorería. En carencia total, los intereses capitalizados van contra la cuenta de largo
/// plazo (aumentan el capital) y no se mueve tesorería.
pub fn asiento_cuota(
    p: &Prestamo,
    q: &Cuota,
    devengado: Money,
    tesoreria: &AccountCode,
    fecha: Date,
) -> JournalEntry {
    let activo = p.sentido == Sentido::Concedido;
    let devengado = devengado.min(q.intereses).max(Money::ZERO);
    let retencion = retencion_de(p, q);
    let neto = q.pago - retencion;
    let retencion_cta = AccountCode::new(if activo {
        CUENTA_RETENCION_SOPORTADA
    } else {
        CUENTA_RETENCION_PRACTICADA
    })
    .unwrap_or_else(|_| tesoreria.clone());
    // Concedido: cobramos (tesorería y 473 al debe; crédito e ingreso al haber). Recibido: al
    // revés.
    let lines = [
        EntryLine::signed(tesoreria.clone(), neto, activo),
        EntryLine::signed(retencion_cta, retencion, activo),
        EntryLine::signed(p.cuentas.corto.clone(), q.capital, !activo),
        EntryLine::signed(
            p.cuentas.intereses.clone(),
            q.intereses - devengado,
            !activo,
        ),
        EntryLine::signed(p.cuentas.devengo.clone(), devengado, !activo),
        EntryLine::signed(p.cuentas.largo.clone(), q.capitalizados, activo),
    ];
    let que = if q.capitalizados.is_zero() {
        format!("cuota {}", q.n)
    } else {
        format!("cuota {} (intereses capitalizados)", q.n)
    };
    JournalEntry {
        date: fecha,
        concept: concepto(p, &que),
        origin: Origin::Prestamo(p.id),
        lines: lines.into_iter().flatten().collect(),
    }
}

/// Asiento del devengo de `importe` de intereses de la cuota `n` el día `fecha`: concedido,
/// cuenta de devengo (547…) al debe contra el ingreso (762); recibido, gasto (662) al debe
/// contra la cuenta de devengo (527, 528…).
pub fn asiento_devengo(p: &Prestamo, n: u16, fecha: Date, importe: Money) -> JournalEntry {
    let activo = p.sentido == Sentido::Concedido;
    JournalEntry {
        date: fecha,
        concept: concepto(p, &format!("intereses devengados de la cuota {n}")),
        origin: Origin::Prestamo(p.id),
        lines: [
            EntryLine::signed(p.cuentas.devengo.clone(), importe, activo),
            EntryLine::signed(p.cuentas.intereses.clone(), importe, !activo),
        ]
        .into_iter()
        .flatten()
        .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::account::acc;
    use time::macros::date;

    fn prestamo(
        sistema: Sistema,
        principal: i64,
        bp: i32,
        periodicidad: u8,
        plazo: u16,
    ) -> Prestamo {
        Prestamo {
            id: 1,
            nombre: "Demo".into(),
            sentido: Sentido::Recibido,
            contraparte: Contraparte {
                nombre: "Banco Demo".into(),
                entidad_credito: true,
                ..Default::default()
            },
            inicio: date!(2026 - 01 - 31),
            principal: Money::from_euros(principal, 0),
            tipo: Rate::from_bp(bp),
            sistema,
            periodicidad,
            plazo,
            carencia: 0,
            carencia_tipo: TipoCarencia::Capital,
            cuentas: Cuentas {
                largo: acc("1700001"),
                corto: acc("5200001"),
                intereses: acc("662"),
                devengo: acc("527"),
            },
            retencion: Rate::ZERO,
            notas: String::new(),
        }
    }

    /// Lo que se comprueba en todos los cuadros: nada descuadra al final.
    fn cuadra(p: &Prestamo, s: &[Cuota]) {
        let capital: Money = s.iter().map(|q| q.capital).sum();
        let capitalizados: Money = s.iter().map(|q| q.capitalizados).sum();
        assert_eq!(capital, p.principal + capitalizados);
        assert_eq!(s.last().unwrap().pendiente, Money::ZERO);
        for q in s {
            assert_eq!(q.pago, q.capital + q.intereses - q.capitalizados);
            assert!(!q.capital.is_negative() && !q.intereses.is_negative());
        }
        let mut pendiente = p.principal;
        for q in s {
            pendiente = pendiente + q.capitalizados - q.capital;
            assert_eq!(q.pendiente, pendiente);
        }
    }

    #[test]
    fn frances_mensual() {
        // 100.000 € al 5 % en 10 años: cuota de referencia 1.060,6552…
        let p = prestamo(Sistema::Frances, 100_000, 500, 1, 120);
        let s = cuadro(&p);
        assert_eq!(s.len(), 120);
        assert_eq!(s[0].pago, Money::from_cents(106_066));
        assert_eq!(s[0].intereses, Money::from_cents(41_667));
        assert_eq!(s[0].capital, Money::from_cents(64_399));
        // Fin de mes: del 31 de enero al 28 de febrero y al 31 de marzo.
        assert_eq!(s[0].fecha, date!(2026 - 02 - 28));
        assert_eq!(s[1].fecha, date!(2026 - 03 - 31));
        // La última absorbe el redondeo (céntimos).
        assert!((s[119].pago - s[0].pago).abs() < Money::from_euros(1, 0));
        cuadra(&p, &s);
        for q in &s[..119] {
            assert_eq!(q.pago, s[0].pago);
        }
    }

    #[test]
    fn lineal() {
        let p = prestamo(Sistema::Lineal, 12_000, 600, 1, 12);
        let s = cuadro(&p);
        assert!(s.iter().all(|q| q.capital == Money::from_euros(1_000, 0)));
        assert_eq!(s[0].intereses, Money::from_euros(60, 0));
        assert_eq!(s[11].intereses, Money::from_euros(5, 0));
        let intereses: Money = s.iter().map(|q| q.intereses).sum();
        assert_eq!(intereses, Money::from_euros(390, 0));
        cuadra(&p, &s);
        // Un principal que no se divide exacto: la última lleva el resto.
        let p = prestamo(Sistema::Lineal, 100, 0, 1, 3);
        let s = cuadro(&p);
        assert_eq!(
            s.iter().map(|q| q.capital.cents()).collect::<Vec<_>>(),
            [3333, 3333, 3334]
        );
        cuadra(&p, &s);
    }

    #[test]
    fn intereses_periodicos_y_al_vencimiento() {
        let p = prestamo(Sistema::InteresesPeriodicos, 50_000, 400, 3, 24);
        let s = cuadro(&p);
        assert_eq!(s.len(), 8);
        assert!(s.iter().all(|q| q.intereses == Money::from_euros(500, 0)));
        assert!(s[..7].iter().all(|q| q.capital.is_zero()));
        assert_eq!(s[7].capital, Money::from_euros(50_000, 0));
        assert_eq!(s[7].fecha, date!(2028 - 01 - 31));
        cuadra(&p, &s);

        // Todo al vencimiento: una cuota a los 18 meses con 18 meses de intereses simples.
        let mut p = prestamo(Sistema::AlVencimiento, 10_000, 300, 1, 18);
        p.inicio = date!(2026 - 03 - 15);
        let s = cuadro(&p);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].fecha, date!(2027 - 09 - 15));
        assert_eq!(s[0].intereses, Money::from_euros(450, 0));
        assert_eq!(s[0].pago, Money::from_euros(10_450, 0));
        cuadra(&p, &s);
        p.carencia = 6;
        assert!(comprobar(&p).is_err());
    }

    #[test]
    fn carencia_de_capital() {
        let mut p = prestamo(Sistema::Lineal, 12_000, 600, 1, 24);
        p.carencia = 12;
        comprobar(&p).unwrap();
        let s = cuadro(&p);
        assert_eq!(s.len(), 24);
        for q in &s[..12] {
            assert_eq!(q.capital, Money::ZERO);
            assert_eq!(q.pago, Money::from_euros(60, 0));
            assert_eq!(q.pendiente, Money::from_euros(12_000, 0));
        }
        assert!(
            s[12..]
                .iter()
                .all(|q| q.capital == Money::from_euros(1_000, 0))
        );
        cuadra(&p, &s);
    }

    #[test]
    fn carencia_total_capitaliza() {
        let mut p = prestamo(Sistema::Frances, 10_000, 1200, 1, 24);
        p.carencia = 6;
        p.carencia_tipo = TipoCarencia::Total;
        let s = cuadro(&p);
        // 1 % al mes compuesto: 100,00; 101,00; 102,01; 103,03; 104,06; 105,10.
        assert_eq!(
            s[..6]
                .iter()
                .map(|q| q.capitalizados.cents())
                .collect::<Vec<_>>(),
            [10_000, 10_100, 10_201, 10_303, 10_406, 10_510]
        );
        assert!(s[..6].iter().all(|q| q.pago.is_zero()));
        assert_eq!(s[5].pendiente, Money::from_cents(1_061_520));
        // Las 18 siguientes amortizan 10.615,20 €.
        let resto = anualidad(Money::from_cents(1_061_520), tipo_periodo(p.tipo, 1), 18);
        assert_eq!(s[6].pago, resto);
        cuadra(&p, &s);
    }

    #[test]
    fn datos_que_no_valen() {
        let mut p = prestamo(Sistema::Frances, 1_000, 500, 3, 10);
        assert!(comprobar(&p).is_err());
        p.plazo = 12;
        p.carencia = 12;
        assert!(comprobar(&p).is_err());
        p.carencia = 0;
        p.periodicidad = 2;
        assert!(comprobar(&p).is_err());
        p.periodicidad = 3;
        comprobar(&p).unwrap();
        p.principal = Money::ZERO;
        assert!(comprobar(&p).is_err());
    }

    #[test]
    fn sin_intereses() {
        let p = prestamo(Sistema::Frances, 900, 0, 1, 3);
        let s = cuadro(&p);
        assert!(s.iter().all(|q| q.intereses.is_zero()));
        assert!(s.iter().all(|q| q.capital == Money::from_euros(300, 0)));
        cuadra(&p, &s);
    }

    #[test]
    fn largo_y_corto_plazo() {
        let p = prestamo(Sistema::Lineal, 36_000, 0, 1, 36);
        let s = cuadro(&p);
        // Al inicio: 12.000 a corto y 24.000 a largo.
        assert_eq!(corto_plazo_a(&s, p.inicio), Money::from_euros(12_000, 0));
        assert_eq!(
            largo_plazo_a(&p, &s, p.inicio),
            Money::from_euros(24_000, 0)
        );
        // Al 31/12/2026 se han pagado 11 cuotas (feb–dic): quedan 25.000, 12.000 a corto.
        let fin = date!(2026 - 12 - 31);
        assert_eq!(pendiente_a(&p, &s, fin), Money::from_euros(25_000, 0));
        assert_eq!(largo_plazo_a(&p, &s, fin), Money::from_euros(13_000, 0));
        assert_eq!(largo_plazo_a(&p, &s, date!(2028 - 06 - 30)), Money::ZERO);
        assert_eq!(pendiente_a(&p, &s, date!(2025 - 12 - 31)), Money::ZERO);
    }

    #[test]
    fn devengo_a_fecha() {
        // Intereses anuales de 1.200 € del 1/7/2026 al 1/7/2027: al 31/12 van 183 de 365 días.
        let mut p = prestamo(Sistema::InteresesPeriodicos, 20_000, 600, 12, 24);
        p.inicio = date!(2026 - 07 - 01);
        let s = cuadro(&p);
        let (n, importe) = devengado_a(&p, &s, date!(2026 - 12 - 31)).unwrap();
        assert_eq!(n, 1);
        assert_eq!(importe, Money::from_cents(60_164));
        // El día del vencimiento no hay nada en curso que devengar.
        assert!(devengado_a(&p, &s, date!(2027 - 07 - 01)).is_none());
        let (n, _) = devengado_a(&p, &s, date!(2027 - 07 - 02)).unwrap();
        assert_eq!(n, 2);
        assert!(devengado_a(&p, &s, date!(2026 - 06 - 30)).is_none());
        assert!(devengado_a(&p, &s, date!(2028 - 07 - 01)).is_none());
    }

    #[test]
    fn asientos_cuadran() {
        let mut p = prestamo(Sistema::Frances, 30_000, 450, 1, 36);
        p.sentido = Sentido::Concedido;
        p.retencion = Rate::percent(19);
        p.cuentas = Cuentas {
            largo: acc("2423001"),
            corto: acc("5323001"),
            intereses: acc("762"),
            devengo: acc("5343"),
        };
        let s = cuadro(&p);
        let banco = acc("5720001");
        let alta = asiento_alta(&p, &banco);
        alta.validate().unwrap();
        assert_eq!(alta.lines.len(), 3);
        let corto = corto_plazo_a(&s, p.inicio);
        assert_eq!(alta.lines[1].debit, corto);
        assert_eq!(alta.lines[2].credit, p.principal);

        // Cuota 1 con 50 € ya devengados: el ingreso es el resto y la 5343 se salda.
        let q = &s[0];
        let e = asiento_cuota(&p, q, Money::from_euros(50, 0), &banco, q.fecha);
        e.validate().unwrap();
        let linea = |c: &str| e.lines.iter().find(|l| l.account.as_str() == c).unwrap();
        let ret = q.intereses.apply_rate(Rate::percent(19));
        assert_eq!(linea("473").debit, ret);
        assert_eq!(linea("5720001").debit, q.pago - ret);
        assert_eq!(linea("5323001").credit, q.capital);
        assert_eq!(linea("762").credit, q.intereses - Money::from_euros(50, 0));
        assert_eq!(linea("5343").credit, Money::from_euros(50, 0));

        // Recibido con retención: 4751 al haber.
        let mut r = prestamo(Sistema::Lineal, 10_000, 500, 1, 10);
        r.retencion = Rate::percent(19);
        let q = &cuadro(&r)[0];
        let e = asiento_cuota(&r, q, Money::ZERO, &banco, q.fecha);
        e.validate().unwrap();
        assert!(
            e.lines
                .iter()
                .any(|l| l.account.as_str() == "4751" && !l.credit.is_zero())
        );

        // Devengo y capitalización.
        asiento_devengo(&p, 1, date!(2026 - 02 - 15), Money::from_euros(10, 0))
            .validate()
            .unwrap();
        let mut c = prestamo(Sistema::Frances, 10_000, 1200, 1, 24);
        c.carencia = 6;
        c.carencia_tipo = TipoCarencia::Total;
        let q = &cuadro(&c)[0];
        let e = asiento_cuota(&c, q, Money::ZERO, &banco, q.fecha);
        e.validate().unwrap();
        assert!(e.lines.iter().all(|l| l.account.as_str() != "5720001"));
        assert_eq!(
            e.lines
                .iter()
                .find(|l| l.account.as_str() == "1700001")
                .unwrap()
                .credit,
            Money::from_euros(100, 0)
        );
        // Cuenta corriente (misma cuenta a largo y a corto): el alta va en una línea.
        let mut cc = prestamo(Sistema::AlVencimiento, 5_000, 0, 1, 12);
        cc.cuentas.largo = acc("551");
        cc.cuentas.corto = acc("551");
        assert_eq!(asiento_alta(&cc, &banco).lines.len(), 2);
    }
}

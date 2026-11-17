//! Entidad patrimonial (art. 5.2 LIS) con la media de los balances trimestrales, y patrimonio
//! neto frente al capital social (arts. 273.2, 327, 363.1.e y 365 a 367 LSC).
//!
//! **Test del art. 5.2 LIS** (Ley 27/2014, texto consolidado del BOE): es patrimonial la entidad
//! en la que más de la mitad de su activo está constituido por valores o no está afecto a una
//! actividad económica (en los términos del art. 5.1). El valor del activo, de los valores y de
//! lo no afecto es el de la media de los balances trimestrales del ejercicio o, si la sociedad es
//! dominante de un grupo del art. 42 CCom, de los balances consolidados. No computan:
//! - como valores, los que se poseen para cumplir obligaciones legales y reglamentarias de la
//!   actividad (letra a), los que incorporan derechos de crédito nacidos de contratos de la
//!   actividad (letra b), los de sociedades de valores por su actividad (letra c) y los que
//!   otorgan al menos el 5 % del capital de una entidad y se poseen durante al menos un año con la
//!   finalidad de dirigir y gestionar la participación, con la correspondiente organización de
//!   medios materiales y personales, si la participada no es a su vez patrimonial (letra d);
//! - como valores ni como no afectos, el dinero o los derechos de crédito procedentes de la
//!   transmisión de elementos afectos o de los valores de la letra d) realizada en el periodo
//!   impositivo o en los dos anteriores.
//!
//! La ley no habla de la tesorería: TaxPain la trata como no afecta salvo la parte que el usuario
//! justifica como necesaria para la actividad. TODO(VERIFY): doctrina de la DGT sobre la
//! tesorería (consultas sobre el «exceso de liquidez») y si puede usarse un importe fijo para
//! todo el ejercicio.
//!
//! La regla de los **beneficios no distribuidos** de actividades económicas del propio año y de
//! los diez anteriores (no computa lo adquirido con ellos) está en el art. 4.Ocho.Dos de la Ley
//! 19/1991 del Impuesto sobre el Patrimonio (exención de las participaciones del socio), junto con
//! el cómputo «durante más de 90 días del ejercicio social» y con el 5 % «de los derechos de
//! voto» (no del capital, y sin el año de tenencia) para las participaciones que no cuentan como
//! valores. Cotejado el 2026-11-16 con los textos consolidados (LIS, BOE-A-2014-12328; LIP,
//! BOE-A-1991-14392): el art. 5.2 LIS no trae ni los 90 días ni los beneficios de diez años; usa
//! la media de los balances trimestrales y excluye sólo lo cobrado por transmisiones del periodo
//! y de los dos anteriores. Aquí la regla de la LIP sólo se menciona para el Patrimonio del
//! socio, no entra en el test del Impuesto sobre Sociedades.
//!
//! TaxPain toma el saldo de la contabilidad al último día de cada trimestre del ejercicio (sin el
//! asiento de cierre) y propone una clase para cada partida del activo por su cuenta del PGC; el
//! usuario la cambia con su motivo. No da un veredicto: enseña el cálculo.
//!
//! **Patrimonio neto a efectos mercantiles** (art. 36.1.c CCom): el de las cuentas anuales (grupos
//! 10 a 13 con el resultado, menos el dividendo a cuenta), más el capital suscrito no exigido
//! (103, 104) y el capital registrado como pasivo (150, 502), sin los ajustes por coberturas de
//! flujos de efectivo (1340), más los préstamos participativos (art. 20.1.d RDL 7/1996), que el
//! usuario indica porque no tienen cuenta propia en el PGC.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::entry::{JournalEntry, Origin};
use taxpain_core::accounting::fiscal_year::FiscalYear;
use taxpain_core::money::{Money, Rate};
use taxpain_core::nif::Nif;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, Duration, Month, OffsetDateTime};

use crate::accounting;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

// ---------------------------------------------------------------------------------------------
// Clasificación de las partidas del activo
// ---------------------------------------------------------------------------------------------

/// Clase de una partida del activo para el test.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Clase {
    /// Afecto a la actividad (también los créditos de la actividad, letra b, y lo que se tiene
    /// por obligaciones legales de la actividad, letra a).
    Afecto,
    /// No afecto: inmuebles sin actividad, terrenos, obras de arte, préstamos a socios…
    NoAfecto,
    /// Valores que computan (acciones, fondos, deuda).
    Valores,
    /// Participación de al menos el 5 % poseída un año con medios para dirigirla (letra d): no
    /// computa.
    Participacion,
    /// Tesorería: no afecta salvo la parte justificada como necesaria para la actividad.
    Tesoreria,
}

impl Clase {
    fn sql(self) -> &'static str {
        match self {
            Clase::Afecto => "afecto",
            Clase::NoAfecto => "no_afecto",
            Clase::Valores => "valores",
            Clase::Participacion => "participacion",
            Clase::Tesoreria => "tesoreria",
        }
    }

    fn from_sql(s: &str) -> Option<Self> {
        Some(match s {
            "afecto" => Clase::Afecto,
            "no_afecto" => Clase::NoAfecto,
            "valores" => Clase::Valores,
            "participacion" => Clase::Participacion,
            "tesoreria" => Clase::Tesoreria,
            _ => return None,
        })
    }
}

/// Cuentas correctoras del activo (amortizaciones, deterioros y desembolsos pendientes): restan
/// con su saldo acreedor.
fn correctora(cuenta: &str) -> bool {
    ["28", "29", "39", "49", "59", "249", "259", "539", "549"]
        .iter()
        .any(|p| cuenta.starts_with(p))
}

/// Partida del activo (sin las del patrimonio neto que van en el grupo 5, como el dividendo a
/// cuenta, 557).
fn de_activo(cuenta: &str) -> bool {
    matches!(cuenta.as_bytes().first(), Some(b'2'..=b'5')) && !cuenta.starts_with("557")
}

/// Clase que propone TaxPain por la cuenta del PGC (prefijo más largo).
pub fn clase_propuesta(cuenta: &str) -> Clase {
    const TABLA: &[(&str, Clase)] = &[
        // Inversiones inmobiliarias: inmuebles para rentas o plusvalías (PGC, 22). Sólo son
        // actividad si el arrendamiento tiene una persona empleada a jornada completa (art. 5.1).
        ("22", Clase::NoAfecto),
        ("282", Clase::NoAfecto),
        ("292", Clase::NoAfecto),
        // Participaciones y valores de partes vinculadas y de otras empresas.
        ("240", Clase::Valores),
        ("241", Clase::Valores),
        ("249", Clase::Valores),
        ("250", Clase::Valores),
        ("251", Clase::Valores),
        ("255", Clase::Valores),
        ("259", Clase::Valores),
        ("293", Clase::Valores),
        ("294", Clase::Valores),
        ("296", Clase::Valores),
        ("297", Clase::Valores),
        ("530", Clase::Valores),
        ("531", Clase::Valores),
        ("533", Clase::Valores),
        ("539", Clase::Valores),
        ("540", Clase::Valores),
        ("541", Clase::Valores),
        ("546", Clase::Valores),
        ("549", Clase::Valores),
        ("593", Clase::Valores),
        ("594", Clase::Valores),
        ("596", Clase::Valores),
        ("597", Clase::Valores),
        // Créditos que no nacen de la actividad: préstamos concedidos, a socios, por la venta del
        // inmovilizado…
        ("242", Clase::NoAfecto),
        ("252", Clase::NoAfecto),
        ("253", Clase::NoAfecto),
        ("254", Clase::NoAfecto),
        ("257", Clase::NoAfecto),
        ("295", Clase::NoAfecto),
        ("298", Clase::NoAfecto),
        ("532", Clase::NoAfecto),
        ("534", Clase::NoAfecto),
        ("535", Clase::NoAfecto),
        ("542", Clase::NoAfecto),
        ("543", Clase::NoAfecto),
        ("544", Clase::NoAfecto),
        ("545", Clase::NoAfecto),
        ("547", Clase::NoAfecto),
        ("551", Clase::NoAfecto),
        ("552", Clase::NoAfecto),
        ("595", Clase::NoAfecto),
        ("598", Clase::NoAfecto),
        // Imposiciones y tesorería.
        ("258", Clase::Tesoreria),
        ("548", Clase::Tesoreria),
        ("57", Clase::Tesoreria),
    ];
    TABLA
        .iter()
        .filter(|(p, _)| cuenta.starts_with(p))
        .max_by_key(|(p, _)| p.len())
        .map_or(Clase::Afecto, |(_, c)| *c)
}

/// Código con el que se agrupa una cuenta en la tabla: la subcuenta en los grupos 2 y 5 (donde
/// cada inmueble, participación o cuenta de banco puede ser distinto) y la cuenta de tres cifras
/// en los grupos 3 y 4 (existencias y créditos de la actividad).
fn clave_partida(cuenta: &str) -> String {
    match cuenta.as_bytes().first() {
        Some(b'3' | b'4') => cuenta.chars().take(3).collect(),
        _ => cuenta.to_owned(),
    }
}

/// Créditos por la venta de inmovilizado (253, 543): donde queda lo que aún no se ha cobrado.
fn credito_enajenacion(cuenta: &str) -> bool {
    cuenta.starts_with("253") || cuenta.starts_with("543")
}

// ---------------------------------------------------------------------------------------------
// Fechas y saldos
// ---------------------------------------------------------------------------------------------

fn sumar_meses(d: Date, meses: i32) -> Date {
    let total = d.year() * 12 + i32::from(u8::from(d.month())) - 1 + meses;
    let (y, m) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    let mes = Month::try_from(u8::try_from(m).unwrap_or(1)).unwrap_or(Month::January);
    let dia = d.day().min(time::util::days_in_month(mes, y));
    Date::from_calendar_date(y, mes, dia).unwrap_or(d)
}

/// Último día de cada trimestre del ejercicio (y el de cierre, si el ejercicio es más corto).
pub fn fechas_trimestrales(fy: &FiscalYear) -> Vec<Date> {
    let mut out: Vec<Date> = (1..=3)
        .map(|k| sumar_meses(fy.start, 3 * k) - Duration::days(1))
        .filter(|d| *d < fy.end)
        .collect();
    out.push(fy.end);
    out
}

/// Saldo de cada cuenta (deudor positivo) con los asientos hasta `fecha`, sin el de cierre.
pub fn saldos_a(asientos: &[JournalEntry], fecha: Date) -> BTreeMap<String, Money> {
    let mut m: BTreeMap<String, Money> = BTreeMap::new();
    for e in asientos {
        if e.date > fecha || matches!(e.origin, Origin::Cierre) {
            continue;
        }
        for l in &e.lines {
            *m.entry(l.account.as_str().to_owned()).or_default() += l.net();
        }
    }
    m
}

fn suma(saldos: &BTreeMap<String, Money>, prefijos: &[&str]) -> Money {
    saldos
        .iter()
        .filter(|(c, _)| prefijos.iter().any(|p| c.starts_with(p)))
        .map(|(_, m)| *m)
        .sum()
}

/// Porcentaje de `parte` sobre `total` en centésimas de punto.
fn porcentaje(parte: Money, total: Money) -> Rate {
    if total.cents() <= 0 {
        return Rate::ZERO;
    }
    let bp = i128::from(parte.cents()) * 10_000 / i128::from(total.cents());
    Rate::from_bp(i32::try_from(bp).unwrap_or(i32::MAX))
}

// ---------------------------------------------------------------------------------------------
// Cálculo
// ---------------------------------------------------------------------------------------------

/// Lo que marca el usuario para el ejercicio.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ajustes {
    /// Tesorería que se justifica como necesaria para la actividad (en cada trimestre, como
    /// mucho la que haya).
    pub tesoreria_afecta: Money,
    #[serde(default)]
    pub tesoreria_motivo: String,
    /// Beneficios no distribuidos de actividades económicas del año y de los diez anteriores
    /// (sólo para el cálculo del Patrimonio del socio). `None`: sin confirmar.
    #[serde(default)]
    pub beneficios_actividad: Option<Money>,
    #[serde(default)]
    pub beneficios_motivo: String,
    /// Préstamos participativos recibidos (patrimonio neto a efectos mercantiles).
    #[serde(default)]
    pub prestamos_participativos: Money,
    /// Dominante de un grupo del art. 42 CCom.
    #[serde(default)]
    pub dominante: bool,
}

/// Venta de un elemento afecto o de una participación de la letra d).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Venta {
    pub id: i64,
    pub fecha: Date,
    pub importe: Money,
    pub motivo: String,
}

/// Marca del usuario sobre una partida.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marca {
    pub clase: Clase,
    pub motivo: String,
    /// Ejercicio desde el que vale.
    pub desde: i32,
}

/// Datos del cálculo, ya sacados de la contabilidad y de lo marcado.
#[derive(Clone, Debug, Default)]
pub struct Entrada {
    pub fechas: Vec<Date>,
    /// Saldo de cada cuenta a cada fecha (todas las cuentas: también 1, 6 y 7).
    pub saldos: Vec<BTreeMap<String, Money>>,
    /// Trimestres que ya han terminado (los demás no entran en la media).
    pub cerrados: Vec<bool>,
    pub marcas: BTreeMap<String, Marca>,
    pub nombres: BTreeMap<String, String>,
    pub ajustes: Ajustes,
    /// Ventas dentro de la ventana (este ejercicio y los dos anteriores).
    pub ventas: Vec<Venta>,
}

/// Una partida del activo con su clase y sus saldos a cada fecha.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Partida {
    pub cuenta: String,
    pub nombre: String,
    pub clase: Clase,
    pub propuesta: Clase,
    /// El usuario la ha marcado (en este ejercicio o en uno anterior).
    pub marcada: bool,
    pub motivo: String,
    pub desde: Option<i32>,
    pub correctora: bool,
    pub saldos: Vec<Money>,
}

/// El activo a una fecha, por clases.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Trimestre {
    pub fecha: Date,
    pub cerrado: bool,
    pub activo: Money,
    pub afecto: Money,
    /// Valores que computan.
    pub valores: Money,
    /// No afecto que computa (con la tesorería no justificada y sin el dinero de las ventas).
    pub no_afecto: Money,
    pub tesoreria: Money,
    /// Participaciones de la letra d), que no computan.
    pub participaciones: Money,
    /// Tesorería justificada como afecta.
    pub tesoreria_justificada: Money,
    /// Dinero o créditos de las ventas de elementos afectos que no computan.
    pub ventas_excluidas: Money,
    /// Tesorería que computa como no afecta (ni justificada ni de ventas).
    pub tesoreria_no_justificada: Money,
    /// Valores + no afecto que computan.
    pub computa: Money,
    pub pct: Rate,
    /// Con los beneficios no distribuidos de actividad restados (cálculo del Patrimonio).
    pub computa_lip: Money,
    pub pct_lip: Rate,
}

/// Media de los balances trimestrales.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Media {
    pub trimestres: usize,
    pub activo: Money,
    pub valores: Money,
    pub no_afecto: Money,
    pub participaciones: Money,
    pub computa: Money,
    pub pct: Rate,
    /// Contando como valores también las participaciones marcadas de la letra d).
    pub pct_con_participaciones: Rate,
    /// Sin contar la tesorería no justificada (lo que computa seguro).
    pub pct_sin_tesoreria: Rate,
    /// Lo que separa del 50 % con el mismo activo (negativo: lo que sobra).
    pub margen: Money,
}

/// Nota explicativa con su norma.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Nota {
    pub titulo: String,
    pub texto: String,
    pub norma: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Gravedad {
    Warning,
    Info,
}

/// Aviso mercantil sobre el patrimonio neto.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AvisoMercantil {
    /// `causa_disolucion`, `reduccion_obligatoria`, `dos_tercios`, `sin_dividendos`,
    /// `patrimonio_negativo`.
    pub codigo: &'static str,
    pub gravedad: Gravedad,
    pub titulo: String,
    pub detalle: String,
    /// Fecha orientativa del plazo (dos meses desde el balance en que se ve).
    pub plazo: Option<Date>,
    pub norma: &'static str,
}

/// Forma jurídica, por la letra del NIF.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Forma {
    Sa,
    Sl,
    Otra,
}

fn forma_de(nif: Option<&str>) -> Forma {
    match nif.and_then(|n| n.trim().chars().next()) {
        Some('A' | 'a') => Forma::Sa,
        Some('B' | 'b') => Forma::Sl,
        _ => Forma::Otra,
    }
}

/// Patrimonio neto a una fecha, a efectos mercantiles.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PatrimonioFecha {
    pub fecha: Date,
    pub cerrado: bool,
    pub capital: Money,
    /// El de las cuentas anuales (grupos 10 a 13 con el resultado, menos la 557).
    pub contable: Money,
    /// Capital no exigido (103, 104) y registrado como pasivo (150, 502).
    pub capital_ajustes: Money,
    /// Ajustes por coberturas de flujos de efectivo (1340), que no cuentan.
    pub coberturas: Money,
    pub prestamos_participativos: Money,
    pub mercantil: Money,
    pub pct_capital: Rate,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Patrimonio {
    pub forma: Option<Forma>,
    pub fechas: Vec<PatrimonioFecha>,
    /// El del cierre del ejercicio anterior (para el art. 327 LSC).
    pub anterior: Option<PatrimonioFecha>,
    pub avisos: Vec<AvisoMercantil>,
    pub notas: Vec<Nota>,
}

/// Resultado completo.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Informe {
    pub year: i32,
    pub fechas: Vec<Date>,
    pub partidas: Vec<Partida>,
    pub trimestres: Vec<Trimestre>,
    pub media: Media,
    /// Más del 50 % en la media: entidad patrimonial según el cálculo con lo marcado.
    pub patrimonial: bool,
    /// Aún no ha terminado ningún trimestre: la media es con el balance de hoy.
    pub provisional: bool,
    /// Trimestres por encima del 50 % (con los beneficios restados), para el Patrimonio.
    pub trimestres_lip: usize,
    pub ajustes: Ajustes,
    pub ventas: Vec<Venta>,
    /// Reservas y resultados positivos del balance al cierre: tope de lo que se puede indicar
    /// como beneficios no distribuidos (hay que quitar lo que no venga de la actividad).
    pub beneficios_propuestos: Money,
    pub consecuencias: Vec<Nota>,
    pub que_revisar: Vec<Nota>,
    pub avisos: Vec<String>,
    pub patrimonio: Patrimonio,
}

/// Partidas del activo con su clase y saldos.
fn partidas(e: &Entrada) -> Vec<Partida> {
    let mut filas: BTreeMap<String, Vec<Money>> = BTreeMap::new();
    let n = e.fechas.len();
    for (i, s) in e.saldos.iter().enumerate() {
        for (cuenta, m) in s {
            if !de_activo(cuenta) || m.is_zero() {
                continue;
            }
            // Las correctoras restan con su saldo acreedor; las demás, sólo si son deudoras (una
            // 551 acreedora o un banco en descubierto son pasivo).
            let entra = if correctora(cuenta) {
                m.is_negative()
            } else {
                m.cents() > 0
            };
            if !entra {
                continue;
            }
            let fila = filas
                .entry(clave_partida(cuenta))
                .or_insert_with(|| vec![Money::ZERO; n]);
            fila[i] += *m;
        }
    }
    filas
        .into_iter()
        .map(|(cuenta, saldos)| {
            let propuesta = clase_propuesta(&cuenta);
            let marca = e.marcas.get(&cuenta);
            Partida {
                nombre: e.nombres.get(&cuenta).cloned().unwrap_or_default(),
                clase: marca.map_or(propuesta, |m| m.clase),
                propuesta,
                marcada: marca.is_some(),
                motivo: marca.map(|m| m.motivo.clone()).unwrap_or_default(),
                desde: marca.map(|m| m.desde),
                correctora: correctora(&cuenta),
                cuenta,
                saldos,
            }
        })
        .collect()
}

fn trimestre(e: &Entrada, partidas: &[Partida], i: usize) -> Trimestre {
    let fecha = e.fechas[i];
    let z = Money::ZERO;
    let mut t = Trimestre {
        fecha,
        cerrado: e.cerrados.get(i).copied().unwrap_or(true),
        activo: z,
        afecto: z,
        valores: z,
        no_afecto: z,
        tesoreria: z,
        participaciones: z,
        tesoreria_justificada: z,
        ventas_excluidas: z,
        tesoreria_no_justificada: z,
        computa: z,
        pct: Rate::ZERO,
        computa_lip: z,
        pct_lip: Rate::ZERO,
    };
    let mut creditos_venta = Money::ZERO;
    for p in partidas {
        let s = p.saldos[i];
        t.activo += s;
        match p.clase {
            Clase::Afecto => t.afecto += s,
            Clase::NoAfecto => {
                t.no_afecto += s;
                if credito_enajenacion(&p.cuenta) {
                    creditos_venta += s;
                }
            }
            Clase::Valores => t.valores += s,
            Clase::Participacion => t.participaciones += s,
            Clase::Tesoreria => t.tesoreria += s,
        }
    }
    let teso = t.tesoreria.max(Money::ZERO);
    t.tesoreria_justificada = e.ajustes.tesoreria_afecta.min(teso).max(Money::ZERO);
    let teso_computa = teso - t.tesoreria_justificada;
    // El dinero de las ventas hechas hasta esta fecha: primero la tesorería, luego el crédito
    // pendiente de cobro.
    let vendido: Money = e
        .ventas
        .iter()
        .filter(|v| v.fecha <= fecha)
        .map(|v| v.importe)
        .sum();
    let de_teso = vendido.min(teso_computa);
    let de_credito = (vendido - de_teso).min(creditos_venta.max(Money::ZERO));
    t.ventas_excluidas = de_teso + de_credito;
    t.tesoreria_no_justificada = teso_computa - de_teso;
    t.no_afecto = t.no_afecto - de_credito + (teso_computa - de_teso);
    t.computa = t.valores + t.no_afecto;
    t.pct = porcentaje(t.computa, t.activo);
    let beneficios = e.ajustes.beneficios_actividad.unwrap_or(Money::ZERO);
    t.computa_lip = (t.computa - beneficios).max(Money::ZERO);
    t.pct_lip = porcentaje(t.computa_lip, t.activo);
    t
}

fn media(trimestres: &[Trimestre]) -> Media {
    let cerrados: Vec<&Trimestre> = trimestres.iter().filter(|t| t.cerrado).collect();
    let usados: Vec<&Trimestre> = if cerrados.is_empty() {
        trimestres.iter().take(1).collect()
    } else {
        cerrados
    };
    let n = i64::try_from(usados.len().max(1)).unwrap_or(1);
    let avg = |f: &dyn Fn(&Trimestre) -> Money| -> Money {
        Money::from_cents(usados.iter().map(|t| f(t).cents()).sum::<i64>() / n)
    };
    let activo = avg(&|t| t.activo);
    let computa = avg(&|t| t.computa);
    let participaciones = avg(&|t| t.participaciones);
    let tesoreria = avg(&|t| t.tesoreria_no_justificada);
    Media {
        trimestres: usados.len(),
        activo,
        valores: avg(&|t| t.valores),
        no_afecto: avg(&|t| t.no_afecto),
        participaciones,
        computa,
        pct: porcentaje(computa, activo),
        pct_con_participaciones: porcentaje(computa + participaciones, activo),
        pct_sin_tesoreria: porcentaje(computa - tesoreria, activo),
        margen: Money::from_cents(activo.cents() / 2) - computa,
    }
}

fn nota(titulo: &str, texto: &str, norma: &str) -> Nota {
    Nota {
        titulo: titulo.to_owned(),
        texto: texto.to_owned(),
        norma: norma.to_owned(),
    }
}

/// Lo que cambia si la sociedad es patrimonial.
pub fn consecuencias() -> Vec<Nota> {
    vec![
        nota(
            "Tipo de gravamen",
            "No se aplica el tipo de las entidades de nueva creación (15 %), ni los de las empresas \
             con cifra de negocios inferior a 1 millón de euros (17 % y 20 %), ni el de las de \
             reducida dimensión (20 %): tributa al tipo general del 25 %. La ley lo dice de esos \
             tres tipos. TODO(VERIFY): no nombra los transitorios de 2025 a 2028 de la DT 44.ª, \
             que se dan «a efectos de lo dispuesto en el artículo 29.1»; TaxPain entiende que \
             tampoco se aplican.",
            "Art. 29.1 LIS, último párrafo; DT 44.ª LIS (Ley 7/2024)",
        ),
        nota(
            "Incentivos de empresa de reducida dimensión",
            "No se aplican los incentivos de las empresas de reducida dimensión (libertad de \
             amortización, amortización acelerada, deterioro global de créditos ni la reserva de \
             nivelación), aunque la cifra de negocios sea inferior a 10 millones.",
            "Arts. 101 a 105 LIS",
        ),
        nota(
            "Reserva de capitalización",
            "El art. 25 LIS no excluye expresamente a las entidades patrimoniales (se aplica a quien \
             tributa al tipo del art. 29.1). TODO(VERIFY): comprobarlo antes de aplicarla.",
            "Art. 25 LIS",
        ),
        nota(
            "Bases imponibles negativas",
            "Si alguien adquiere la mayoría del capital después de generarse las bases negativas y \
             la entidad es patrimonial, no se pueden compensar.",
            "Art. 26.4 LIS",
        ),
        nota(
            "Venta de participaciones en esta sociedad",
            "Si una sociedad socia vende su participación, la exención de la plusvalía se limita \
             a la parte que corresponde a los beneficios no distribuidos generados mientras la \
             tuvo. TODO(VERIFY): alcance exacto del art. 21.5 LIS.",
            "Art. 21.5 LIS",
        ),
        nota(
            "Patrimonio y Sucesiones del socio",
            "La exención de las participaciones en el Impuesto sobre el Patrimonio y la reducción \
             del 95 % en Sucesiones y Donaciones exigen que la sociedad no gestione un patrimonio \
             mobiliario o inmobiliario, con un cómputo propio: más de la mitad del activo en \
             valores o no afecto durante más de 90 días del ejercicio, según la contabilidad, y \
             sin contar lo adquirido con beneficios de actividades económicas del año y de los \
             diez anteriores. No es el test de arriba (media de los balances trimestrales).",
            "Art. 4.Ocho.Dos LIP; art. 20.2.c LISD",
        ),
        nota(
            "Modelo 200",
            "La declaración indica si la entidad es patrimonial. TODO(VERIFY): casilla del \
             modelo del ejercicio; TaxPain aún no la marca en el borrador del 200.",
            "Orden del modelo 200",
        ),
    ]
}

/// Lo que se suele revisar antes de concluir (sin asesorar: depende de cada caso).
pub fn que_revisar() -> Vec<Nota> {
    vec![
        nota(
            "Inmuebles",
            "Un inmueble alquilado sólo es actividad económica si para el arrendamiento hay al \
             menos una persona empleada con contrato laboral y a jornada completa. Uno sin uso, \
             cedido a un socio o un terreno sin explotar no está afecto.",
            "Art. 5.1 LIS",
        ),
        nota(
            "Tesorería",
            "La ley no menciona la tesorería. Se suele considerar afecta sólo la que necesita la \
             actividad, y conviene poder justificarla (previsiones de pagos, inversiones \
             aprobadas). TODO(VERIFY): doctrina de la DGT.",
            "Art. 5.2 LIS",
        ),
        nota(
            "Participaciones",
            "No computan si dan al menos el 5 % del capital, se tienen desde hace un año, se \
             poseen para dirigirlas y gestionarlas con medios materiales y personales, y la \
             participada no es a su vez patrimonial. Se mira con todo el grupo del art. 42 CCom.",
            "Art. 5.2.d LIS",
        ),
        nota(
            "Ventas de elementos afectos",
            "El dinero o el crédito de la venta de un elemento afecto (o de una participación que \
             no computa) del ejercicio o de los dos anteriores no computa. Anótalas abajo.",
            "Art. 5.2 LIS",
        ),
        nota(
            "Antes de mover nada",
            "Repartir dividendos, devolver aportaciones, invertir en la actividad o pasar \
             inmuebles a otra sociedad cambia el activo, pero cada operación tiene sus propios \
             efectos fiscales y mercantiles: consúltalo con tu asesor antes de hacerla.",
            "",
        ),
    ]
}

/// Patrimonio neto a una fecha.
pub fn patrimonio_a(
    saldos: &BTreeMap<String, Money>,
    fecha: Date,
    cerrado: bool,
    prestamos: Money,
) -> PatrimonioFecha {
    let capital = -suma(saldos, &["100", "101", "102"]);
    let contable = -suma(saldos, &["10", "11", "12", "13", "6", "7"]) - suma(saldos, &["557"]);
    let capital_ajustes = suma(saldos, &["103", "104"]) - suma(saldos, &["150", "502"]);
    let coberturas = -suma(saldos, &["1340"]);
    let mercantil = contable + capital_ajustes - coberturas + prestamos;
    PatrimonioFecha {
        fecha,
        cerrado,
        capital,
        contable,
        capital_ajustes,
        coberturas,
        prestamos_participativos: prestamos,
        mercantil,
        pct_capital: porcentaje(mercantil, capital),
    }
}

fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

/// Avisos mercantiles con el patrimonio neto del último balance cerrado.
pub fn avisos_mercantiles(
    actual: &PatrimonioFecha,
    anterior: Option<&PatrimonioFecha>,
    forma: Forma,
) -> Vec<AvisoMercantil> {
    let mut out = Vec::new();
    let capital = actual.capital;
    if capital.cents() <= 0 {
        return out;
    }
    let pn = actual.mercantil;
    let fecha = dmy(actual.fecha);
    let mitad = Money::from_cents(capital.cents() / 2);
    let dos_tercios = Money::from_cents(capital.cents() * 2 / 3);
    if pn.is_negative() {
        out.push(AvisoMercantil {
            codigo: "patrimonio_negativo",
            gravedad: Gravedad::Warning,
            titulo: "Patrimonio neto negativo".into(),
            detalle: format!(
                "A {fecha} el patrimonio neto es {pn}. Un patrimonio negativo no es por sí \
                 solo insolvencia, pero si la sociedad no puede cumplir regularmente sus \
                 obligaciones, el deudor debe pedir el concurso en los dos meses siguientes a \
                 conocer (o deber conocer) la insolvencia."
            ),
            plazo: None,
            norma: "Arts. 2 y 5 TRLC (RDLeg. 1/2020)",
        });
    }
    if pn < mitad {
        out.push(AvisoMercantil {
            codigo: "causa_disolucion",
            gravedad: Gravedad::Warning,
            titulo: "Causa de disolución por pérdidas".into(),
            detalle: format!(
                "A {fecha} el patrimonio neto ({pn}) es inferior a la mitad del capital social \
                 ({mitad} de {capital}). Es causa de disolución, salvo que se aumente o se reduzca \
                 el capital en la medida suficiente y siempre que no proceda el concurso. Los \
                 administradores deben convocar la junta en el plazo de dos meses para que acuerde \
                 la disolución o lo que la remedie (aumento o reducción de capital, aportaciones de \
                 socios para compensar pérdidas a la 118, préstamos participativos) o, si la \
                 sociedad es insolvente, instar el concurso; si la junta no se celebra o no lo \
                 acuerda, deben pedir la disolución judicial en dos meses. Si no lo hacen, responden \
                 solidariamente de las deudas sociales posteriores a la causa, que se presumen \
                 posteriores. El plazo corre desde que conocen o debían conocer la causa: con este \
                 balance, orientativamente hasta el {}. TODO(VERIFY): día inicial del plazo.",
                dmy(sumar_meses(actual.fecha, 2))
            ),
            plazo: Some(sumar_meses(actual.fecha, 2)),
            norma: "Arts. 363.1.e, 365, 366 y 367 LSC",
        });
    }
    if pn < dos_tercios {
        let anterior_bajo = anterior.is_some_and(|a| a.mercantil < dos_tercios);
        match forma {
            Forma::Sa if anterior_bajo => out.push(AvisoMercantil {
                codigo: "reduccion_obligatoria",
                gravedad: Gravedad::Warning,
                titulo: "Reducción de capital obligatoria".into(),
                detalle: format!(
                    "El patrimonio neto ({pn}) sigue por debajo de dos tercios del capital \
                     ({dos_tercios}) y ya lo estaba al cierre del ejercicio anterior: en la \
                     sociedad anónima la reducción de capital por pérdidas es obligatoria cuando \
                     ha transcurrido un ejercicio sin recuperarse."
                ),
                plazo: None,
                norma: "Art. 327 LSC",
            }),
            Forma::Sa => out.push(AvisoMercantil {
                codigo: "dos_tercios",
                gravedad: Gravedad::Info,
                titulo: "Patrimonio neto por debajo de dos tercios del capital".into(),
                detalle: format!(
                    "El patrimonio neto ({pn}) está por debajo de dos tercios del capital \
                     ({dos_tercios}). En la sociedad anónima, si transcurre un ejercicio sin \
                     recuperarse, la reducción de capital es obligatoria."
                ),
                plazo: None,
                norma: "Art. 327 LSC",
            }),
            _ => out.push(AvisoMercantil {
                codigo: "dos_tercios",
                gravedad: Gravedad::Info,
                titulo: "Patrimonio neto por debajo de dos tercios del capital".into(),
                detalle: format!(
                    "El patrimonio neto ({pn}) está por debajo de dos tercios del capital \
                     ({dos_tercios}). La reducción obligatoria del art. 327 LSC es de la sociedad \
                     anónima; en la limitada la reducción para restablecer el equilibrio entre el \
                     capital y el patrimonio neto es voluntaria. TODO(VERIFY): art. 320 LSC."
                ),
                plazo: None,
                norma: "Arts. 320 y 327 LSC",
            }),
        }
    }
    if pn < capital {
        out.push(AvisoMercantil {
            codigo: "sin_dividendos",
            gravedad: Gravedad::Info,
            titulo: "No se pueden repartir dividendos".into(),
            detalle: format!(
                "El patrimonio neto ({pn}) es inferior al capital ({capital}): no se pueden \
                 repartir dividendos y el beneficio debe compensar primero las pérdidas."
            ),
            plazo: None,
            norma: "Art. 273.2 LSC",
        });
    }
    out
}

/// Notas fijas del apartado de patrimonio neto.
fn notas_patrimonio(year: i32) -> Vec<Nota> {
    let mut n = vec![nota(
        "Qué patrimonio neto se usa",
        "El de las cuentas anuales, más el capital suscrito no exigido y el registrado como \
         pasivo, sin los ajustes por coberturas de flujos de efectivo, más los préstamos \
         participativos (que indicas tú).",
        "Art. 36.1.c CCom; art. 20.1.d RDL 7/1996",
    )];
    if year <= 2024 {
        n.push(nota(
            "Pérdidas de 2020 y 2021",
            "A los solos efectos de la causa de disolución del art. 363.1.e LSC no se tomaban en \
             consideración las pérdidas de 2020 y 2021 hasta el cierre del ejercicio que empezara \
             en 2024. TaxPain no las separa. TODO(VERIFY).",
            "Art. 13.1 Ley 3/2020 (redacción del RDL 20/2022)",
        ));
    }
    n
}

/// El cálculo completo con los datos ya reunidos.
pub fn calcular(year: i32, e: &Entrada, forma: Option<Forma>) -> Informe {
    let partidas = partidas(e);
    let trimestres: Vec<Trimestre> = (0..e.fechas.len())
        .map(|i| trimestre(e, &partidas, i))
        .collect();
    let media = media(&trimestres);
    let patrimonial = media.pct > Rate::percent(50);
    let provisional = !trimestres.iter().any(|t| t.cerrado);
    let trimestres_lip = trimestres
        .iter()
        .filter(|t| t.cerrado && t.pct_lip > Rate::percent(50))
        .count();
    let mut avisos = Vec::new();
    if e.ajustes.dominante {
        avisos.push(
            "La sociedad es dominante de un grupo (art. 42 CCom): el art. 5.2 LIS manda hacer el \
             cálculo con los balances consolidados; esta tabla usa los de la sociedad sola."
                .to_owned(),
        );
    }
    if provisional {
        avisos.push(
            "Aún no ha terminado ningún trimestre del ejercicio: el cálculo es con el balance de \
             hoy y es provisional."
                .to_owned(),
        );
    }
    let sin_motivo: Vec<&str> = partidas
        .iter()
        .filter(|p| p.marcada && p.clase != p.propuesta && p.motivo.trim().is_empty())
        .map(|p| p.cuenta.as_str())
        .collect();
    if !sin_motivo.is_empty() {
        avisos.push(format!(
            "Has cambiado la clase de {} sin decir por qué: anota el motivo para poder \
             justificarlo.",
            sin_motivo.join(", ")
        ));
    }
    let ultimo = e.saldos.last().cloned().unwrap_or_default();
    let beneficios_propuestos = (-suma(
        &ultimo,
        &[
            "112", "113", "114", "115", "119", "120", "121", "129", "6", "7",
        ],
    ))
    .max(Money::ZERO);

    // Patrimonio neto a cada fecha.
    let prestamos = e.ajustes.prestamos_participativos;
    let fechas_pn: Vec<PatrimonioFecha> = e
        .fechas
        .iter()
        .zip(&e.saldos)
        .zip(e.cerrados.iter().chain(std::iter::repeat(&true)))
        .map(|((f, s), c)| patrimonio_a(s, *f, *c, prestamos))
        .collect();
    let patrimonio = Patrimonio {
        forma,
        fechas: fechas_pn,
        anterior: None,
        avisos: Vec::new(),
        notas: notas_patrimonio(year),
    };
    Informe {
        year,
        fechas: e.fechas.clone(),
        partidas,
        trimestres,
        media,
        patrimonial,
        provisional,
        trimestres_lip,
        ajustes: e.ajustes.clone(),
        ventas: e.ventas.clone(),
        beneficios_propuestos,
        consecuencias: consecuencias(),
        que_revisar: que_revisar(),
        avisos,
        patrimonio,
    }
}

/// Completa los avisos mercantiles con el último balance cerrado y el cierre anterior.
fn con_avisos(mut i: Informe, anterior: Option<PatrimonioFecha>) -> Informe {
    let forma = i.patrimonio.forma.unwrap_or(Forma::Otra);
    let actual = i
        .patrimonio
        .fechas
        .iter()
        .rev()
        .find(|p| p.cerrado)
        .or_else(|| i.patrimonio.fechas.first())
        .cloned();
    if let Some(a) = actual {
        i.patrimonio.avisos = avisos_mercantiles(&a, anterior.as_ref(), forma);
    }
    i.patrimonio.anterior = anterior;
    i
}

// ---------------------------------------------------------------------------------------------
// Base de datos
// ---------------------------------------------------------------------------------------------

fn parse_fecha(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| other(e.to_string()))
}

fn es_sociedad(v: &Vault) -> Result<Option<String>> {
    let info = crate::companies::info(v)?;
    if info
        .nif
        .as_deref()
        .and_then(|n| Nif::parse(n).ok())
        .is_some_and(|n| n.is_persona_fisica())
    {
        return Err(other(
            "el test de entidad patrimonial y los avisos del capital son de las sociedades",
        ));
    }
    Ok(info.nif)
}

/// Marcas que valen en `year`: la más reciente de cada cuenta desde un ejercicio no posterior.
fn marcas(v: &Vault, year: i32) -> Result<BTreeMap<String, Marca>> {
    let mut st = v.connection().prepare(
        "SELECT cuenta, clase, motivo, year FROM patrimonial_marca WHERE year <= ?1
         ORDER BY cuenta, year",
    )?;
    let filas = st.query_map([year], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i32>(3)?,
        ))
    })?;
    let mut out = BTreeMap::new();
    for f in filas {
        let (cuenta, clase, motivo, desde) = f?;
        if let Some(clase) = Clase::from_sql(&clase) {
            out.insert(
                cuenta,
                Marca {
                    clase,
                    motivo,
                    desde,
                },
            );
        }
    }
    Ok(out)
}

/// Ajustes del ejercicio (o los por defecto).
pub fn ajustes(v: &Vault, year: i32) -> Result<Ajustes> {
    Ok(v.connection()
        .query_row(
            "SELECT tesoreria_afecta, tesoreria_motivo, beneficios_actividad, beneficios_motivo,
                    prestamos_participativos, dominante
               FROM patrimonial_ejercicio WHERE year = ?1",
            [year],
            |r| {
                Ok(Ajustes {
                    tesoreria_afecta: Money::from_cents(r.get(0)?),
                    tesoreria_motivo: r.get(1)?,
                    beneficios_actividad: r.get::<_, Option<i64>>(2)?.map(Money::from_cents),
                    beneficios_motivo: r.get(3)?,
                    prestamos_participativos: Money::from_cents(r.get(4)?),
                    dominante: r.get::<_, i64>(5)? != 0,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

/// Todas las ventas anotadas.
pub fn ventas(v: &Vault) -> Result<Vec<Venta>> {
    let mut st = v
        .connection()
        .prepare("SELECT id, fecha, importe, motivo FROM patrimonial_venta ORDER BY fecha, id")?;
    let filas = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for f in filas {
        let (id, fecha, importe, motivo) = f?;
        out.push(Venta {
            id,
            fecha: parse_fecha(&fecha)?,
            importe: Money::from_cents(importe),
            motivo,
        });
    }
    Ok(out)
}

fn nombre(v: &Vault, cuenta: &str) -> String {
    accounting::account_name(v.connection(), cuenta)
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Reúne los datos del ejercicio y calcula.
pub fn informe(v: &Vault, year: i32, hoy: Date) -> Result<Informe> {
    let nif = es_sociedad(v)?;
    let fy = accounting::fiscal_year(v, year)?;
    let asientos: Vec<JournalEntry> = accounting::journal(v, year)?
        .into_iter()
        .map(|s| s.entry)
        .collect();
    let fechas = fechas_trimestrales(&fy);
    let saldos: Vec<BTreeMap<String, Money>> =
        fechas.iter().map(|f| saldos_a(&asientos, *f)).collect();
    let cerrados = fechas.iter().map(|f| *f < hoy).collect();
    // La ventana de las ventas: desde el inicio del ejercicio dos antes.
    let inicio = accounting::fiscal_year(v, year - 2)
        .map(|f| f.start)
        .unwrap_or_else(|_| sumar_meses(fy.start, -24));
    let ventas: Vec<Venta> = ventas(v)?
        .into_iter()
        .filter(|x| x.fecha >= inicio && x.fecha <= fy.end)
        .collect();
    let mut nombres = BTreeMap::new();
    for s in &saldos {
        for c in s.keys() {
            let k = clave_partida(c);
            if de_activo(&k) && !nombres.contains_key(&k) {
                nombres.insert(k.clone(), nombre(v, &k));
            }
        }
    }
    let entrada = Entrada {
        fechas,
        saldos,
        cerrados,
        marcas: marcas(v, year)?,
        nombres,
        ajustes: ajustes(v, year)?,
        ventas,
    };
    let forma = forma_de(nif.as_deref());
    let i = calcular(year, &entrada, Some(forma));
    // El patrimonio neto al cierre del ejercicio anterior, si hay contabilidad de él.
    let anterior = {
        let previos: Vec<JournalEntry> = accounting::journal(v, year - 1)?
            .into_iter()
            .map(|s| s.entry)
            .collect();
        if previos.is_empty() {
            None
        } else {
            let fa = accounting::fiscal_year(v, year - 1)?;
            Some(patrimonio_a(
                &saldos_a(&previos, fa.end),
                fa.end,
                true,
                ajustes(v, year - 1)?.prestamos_participativos,
            ))
        }
    };
    Ok(con_avisos(i, anterior))
}

/// Señal para «Ahorrar impuestos»: (% del test con lo marcado, % sin la tesorería no
/// justificada). El segundo es lo que computa seguro: la afectación de la tesorería es
/// interpretable, así que sólo él quita las ventajas de las empresas pequeñas de las
/// recomendaciones; el primero da una nota para revisarla. Con el ejercicio de `hoy` si tiene
/// asientos; si no, el anterior.
pub fn senal(v: &Vault, year: i32, hoy: Date) -> Option<(u8, u8)> {
    let con_datos = |y: i32| {
        accounting::journal(v, y)
            .map(|j| !j.is_empty())
            .unwrap_or(false)
    };
    let y = if con_datos(year) {
        year
    } else if con_datos(year - 1) {
        year - 1
    } else {
        return None;
    };
    let i = informe(v, y, hoy).ok()?;
    if i.media.activo.cents() <= 0 {
        return None;
    }
    let pct = |r: Rate| u8::try_from((r.bp() / 100).clamp(0, 100)).unwrap_or(100);
    Some((pct(i.media.pct), pct(i.media.pct_sin_tesoreria)))
}

/// Marca la clase de una partida desde el ejercicio `year` (vale para los siguientes).
pub fn marcar(
    v: &Vault,
    year: i32,
    cuenta: &str,
    clase: Clase,
    motivo: &str,
    now: OffsetDateTime,
) -> Result<()> {
    let cuenta = cuenta.trim();
    if !de_activo(cuenta) || !cuenta.bytes().all(|b| b.is_ascii_digit()) {
        return Err(other(format!(
            "{cuenta} no es una cuenta del activo (grupos 2 a 5)"
        )));
    }
    if (cuenta.starts_with('3') || cuenta.starts_with('4')) && cuenta.len() != 3 {
        return Err(other(
            "en los grupos 3 y 4 se marca la cuenta de tres cifras",
        ));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO patrimonial_marca(year, cuenta, clase, motivo, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(year, cuenta) DO UPDATE SET clase = excluded.clase,
           motivo = excluded.motivo, updated_at = excluded.updated_at",
        params![year, cuenta, clase.sql(), motivo.trim(), now_str(now)],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "patrimonial.marcar",
        &format!("{year}/{cuenta}"),
        &serde_json::json!({ "clase": clase.sql(), "motivo": motivo.trim() }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Guarda los ajustes del ejercicio.
pub fn guardar_ajustes(v: &Vault, year: i32, a: &Ajustes, now: OffsetDateTime) -> Result<()> {
    if a.tesoreria_afecta.is_negative()
        || a.prestamos_participativos.is_negative()
        || a.beneficios_actividad.is_some_and(Money::is_negative)
    {
        return Err(other("los importes no pueden ser negativos"));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO patrimonial_ejercicio(year, tesoreria_afecta, tesoreria_motivo,
           beneficios_actividad, beneficios_motivo, prestamos_participativos, dominante, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(year) DO UPDATE SET tesoreria_afecta = excluded.tesoreria_afecta,
           tesoreria_motivo = excluded.tesoreria_motivo,
           beneficios_actividad = excluded.beneficios_actividad,
           beneficios_motivo = excluded.beneficios_motivo,
           prestamos_participativos = excluded.prestamos_participativos,
           dominante = excluded.dominante, updated_at = excluded.updated_at",
        params![
            year,
            a.tesoreria_afecta.cents(),
            a.tesoreria_motivo.trim(),
            a.beneficios_actividad.map(Money::cents),
            a.beneficios_motivo.trim(),
            a.prestamos_participativos.cents(),
            i64::from(a.dominante),
            now_str(now),
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "patrimonial.ajustes",
        &year.to_string(),
        &serde_json::json!({
            "tesoreria_afecta": a.tesoreria_afecta.cents(),
            "beneficios_actividad": a.beneficios_actividad.map(Money::cents),
            "prestamos_participativos": a.prestamos_participativos.cents(),
            "dominante": a.dominante,
        }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Anota la venta de un elemento afecto (o de una participación que no computa).
pub fn anadir_venta(
    v: &Vault,
    fecha: Date,
    importe: Money,
    motivo: &str,
    now: OffsetDateTime,
) -> Result<i64> {
    if importe.cents() <= 0 {
        return Err(other("el importe de la venta tiene que ser positivo"));
    }
    if motivo.trim().is_empty() {
        return Err(other(
            "di qué se vendió (el elemento afecto o la participación)",
        ));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO patrimonial_venta(fecha, importe, motivo, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            fecha.to_string(),
            importe.cents(),
            motivo.trim(),
            now_str(now)
        ],
    )?;
    let id = tx.last_insert_rowid();
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "patrimonial.venta",
        &id.to_string(),
        &serde_json::json!({ "fecha": fecha.to_string(), "importe": importe.cents() }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Quita una venta anotada (pide confirmación).
pub fn quitar_venta(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let n = tx.execute("DELETE FROM patrimonial_venta WHERE id = ?1", [id])?;
    if n == 0 {
        return Err(AppError::NotFound);
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "patrimonial.quitar_venta",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn m(e: i64) -> Money {
        Money::from_euros(e, 0)
    }

    #[test]
    fn trimestres_de_un_ejercicio_natural_y_de_uno_corto() {
        assert_eq!(
            fechas_trimestrales(&FiscalYear::natural(2026)),
            vec![
                date!(2026 - 03 - 31),
                date!(2026 - 06 - 30),
                date!(2026 - 09 - 30),
                date!(2026 - 12 - 31)
            ]
        );
        let corto = FiscalYear {
            year: 2026,
            start: date!(2026 - 05 - 01),
            end: date!(2026 - 12 - 31),
        };
        assert_eq!(
            fechas_trimestrales(&corto),
            vec![
                date!(2026 - 07 - 31),
                date!(2026 - 10 - 31),
                date!(2026 - 12 - 31)
            ]
        );
    }

    #[test]
    fn clases_propuestas_por_cuenta() {
        assert_eq!(clase_propuesta("2110001"), Clase::Afecto);
        assert_eq!(clase_propuesta("2200001"), Clase::NoAfecto);
        assert_eq!(clase_propuesta("2403001"), Clase::Valores);
        assert_eq!(clase_propuesta("5400001"), Clase::Valores);
        assert_eq!(clase_propuesta("5720001"), Clase::Tesoreria);
        assert_eq!(clase_propuesta("5510001"), Clase::NoAfecto);
        assert_eq!(clase_propuesta("430"), Clase::Afecto);
        assert_eq!(clase_propuesta("2811001"), Clase::Afecto);
        assert_eq!(clase_propuesta("2820001"), Clase::NoAfecto);
    }

    #[test]
    fn aviso_de_dos_tercios_en_sa_con_el_ejercicio_anterior() {
        let s: BTreeMap<String, Money> = [
            ("1000000".to_owned(), -m(60_000)),
            ("1210000".to_owned(), m(25_000)),
        ]
        .into();
        let p = patrimonio_a(&s, date!(2026 - 12 - 31), true, Money::ZERO);
        assert_eq!(p.mercantil, m(35_000));
        let a = avisos_mercantiles(&p, Some(&p), Forma::Sa);
        assert!(a.iter().any(|x| x.codigo == "reduccion_obligatoria"));
        assert!(!a.iter().any(|x| x.codigo == "causa_disolucion"));
        let a = avisos_mercantiles(&p, None, Forma::Sa);
        assert!(a.iter().any(|x| x.codigo == "dos_tercios"));
        let a = avisos_mercantiles(&p, Some(&p), Forma::Sl);
        assert!(!a.iter().any(|x| x.codigo == "reduccion_obligatoria"));
        assert!(a.iter().any(|x| x.codigo == "sin_dividendos"));
    }
}

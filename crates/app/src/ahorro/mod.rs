//! «Ahorrar impuestos»: incentivos fiscales que puede aprovechar la empresa (o el autónomo) según
//! su situación, con lo que ahorraría, los requisitos y los pasos para conseguirlo.
//!
//! Cada incentivo es una ficha fija ([`catalogo`]) con su texto, requisitos, pasos, riesgos y
//! norma; lo que cambia de una empresa a otra es la evaluación: si le conviene ya (los datos lo
//! muestran), si tiene que valorarlo (depende de algo que TaxPain no sabe), si ya lo aprovecha o
//! si no le aplica, con el motivo y, donde se puede calcular, el ahorro estimado.
//!
//! Las cifras de la empresa salen, por este orden, de:
//! - el borrador del 200 del ejercicio en curso con la contabilidad de TaxPain (hasta hoy);
//! - sin contabilidad del año, una estimación con sus libros registro (emitidas menos recibidas,
//!   sueldos de Nóminas o de los 111 presentados y amortización);
//! - si tampoco hay libros, el último 200 presentado (comprobado en la AEAT o importado de la
//!   gestoría), sólo como referencia;
//! - para un autónomo, el rendimiento de la actividad del borrador de la Renta o, sin
//!   contabilidad, la misma estimación con los libros registro.
//!
//! El ahorro es una estimación: la diferencia de cuota con el tipo de la empresa (la escala del
//! IS de su régimen o, en la Renta, la escala general aproximada). Nada se presenta ni se firma
//! desde aquí; las marcas del usuario («en marcha», «hecho», «no me interesa» y los pasos
//! hechos) se guardan en el almacén de la empresa.

mod catalogo;
mod ia;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use taxpain_core::calendar::{Persona, TaxpayerProfile};
use taxpain_core::money::Money;
use taxpain_core::nif::Nif;
use taxpain_models::m200::{TaxRegime, gross_tax, gross_tax_for};
use taxpain_vault::Vault;
use time::Date;

pub use catalogo::Ficha;
pub(crate) use ia::{tema_plan, tema_tarjeta};

use crate::error::Result;
use crate::{accounting, cartera, cripto, models, presentado_aeat, renta};

/// Clave del almacén con las marcas del usuario, por ejercicio (casi todo es anual: lo hecho en
/// 2026 no está hecho en 2027).
const MARCAS: &str = "ahorro_marcas";

type Marcas = BTreeMap<i32, BTreeMap<String, Marca>>;

/// Las marcas guardadas. Un formato anterior (sin ejercicio) no se entiende y se ignora; un fallo
/// al leer, no (si no, la siguiente marca borraría las demás).
fn leer_marcas(v: &Vault) -> Result<Marcas> {
    match v.setting::<Marcas>(MARCAS) {
        Ok(m) => Ok(m.unwrap_or_default()),
        Err(taxpain_vault::VaultError::InvalidInput(_)) => Ok(Marcas::default()),
        Err(e) => Err(e.into()),
    }
}

/// Para quién es un incentivo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Para {
    Sociedad,
    Autonomo,
    Ambos,
}

/// De qué va (para agrupar y filtrar).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    /// Reservas, tipos y bases del Impuesto sobre Sociedades.
    Beneficio,
    /// Inversiones con deducción (cine, I+D, empresas nuevas…).
    Inversion,
    /// Amortizaciones y deterioros.
    Gastos,
    /// Sueldos, planes de pensiones y retribución flexible.
    Personas,
    /// IVA.
    Iva,
    /// Donativos y mecenazgo.
    Donativos,
    /// Criptomonedas y otras inversiones de la base del ahorro.
    Cripto,
}

/// Cómo le encaja a esta empresa.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Encaje {
    /// Los datos muestran que le conviene ahora.
    Conviene,
    /// Puede interesarle: depende de algo que TaxPain no sabe (una decisión o un dato).
    Valorar,
    /// Ya lo aprovecha (consta en sus datos).
    Aprovechado,
    /// Con sus datos no le aplica (se enseña sólo si se pide).
    NoAplica,
}

/// Lo que ha marcado el usuario.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Estado {
    #[default]
    Pendiente,
    EnMarcha,
    Hecho,
    Descartado,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marca {
    pub estado: Estado,
    /// Pasos hechos (índices de `pasos`).
    #[serde(default)]
    pub pasos: BTreeSet<usize>,
}

/// Una cifra de la empresa con su origen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Cifra {
    pub valor: Money,
    pub fuente: String,
}

/// Lo que TaxPain sabe de la empresa y usa para recomendar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Situacion {
    pub año: i32,
    /// Último día del ejercicio en curso (lo que se haga después cuenta para el siguiente).
    pub cierre: Date,
    pub persona_fisica: bool,
    /// «Microempresa (cifra de negocios < 1 M€)», «Reducida dimensión»…
    pub regimen: Option<String>,
    pub cifra_negocios: Option<Cifra>,
    /// Sociedades: base imponible (antes de reservas); autónomos: rendimiento neto.
    pub base: Option<Cifra>,
    pub cuota: Option<Cifra>,
    /// Tipo marginal con el que se estima el ahorro (en %).
    pub tipo: Option<u8>,
    pub trabajadores: bool,
    pub cnae: Option<String>,
    /// Lo que falta para afinar (sin contabilidad, sin 200…).
    pub notas: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Recomendacion {
    pub id: &'static str,
    pub area: Area,
    pub titulo: &'static str,
    /// Qué es, en llano.
    pub resumen: &'static str,
    pub encaje: Encaje,
    /// Por qué le encaja (o no) a esta empresa, con sus cifras.
    pub motivo: String,
    /// Ahorro estimado de este ejercicio.
    pub ahorro: Option<Money>,
    /// Es impuesto aplazado (se paga más adelante), no ahorrado.
    pub aplazado: bool,
    /// Cómo se ha calculado el ahorro.
    pub calculo: Option<String>,
    /// Hasta cuándo hay que actuar para este ejercicio.
    pub plazo: Option<Date>,
    pub requisitos: &'static [&'static str],
    pub pasos: &'static [&'static str],
    pub riesgos: &'static [&'static str],
    pub norma: &'static str,
    pub enlace: &'static str,
    /// Otra pantalla de TaxPain que ayuda («cripto», «cartera», «modelos»…).
    pub pantalla: Option<&'static str>,
    pub marca: Marca,
    /// Lo que rebaja la base (para el total conjunto; no se envía a la interfaz).
    #[serde(skip)]
    rebaja: Option<Money>,
    #[serde(skip)]
    sin_tope: Option<Money>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Ahorro {
    pub situacion: Situacion,
    pub recomendaciones: Vec<Recomendacion>,
    /// Suma de los ahorros estimados de lo que conviene y no está hecho ni descartado.
    pub ahorro_posible: Money,
    /// Lo mismo, del impuesto que se aplaza.
    pub aplazado_posible: Money,
}

/// Una factura emitida cobrada sólo en parte (o nada) y vencida hace tiempo.
#[derive(Clone, Debug)]
struct Impagada {
    emision: Date,
    vencimiento: Date,
    /// Lo pendiente de cobro (con IVA).
    pendiente: Money,
    /// La parte de IVA de lo pendiente.
    iva: Money,
}

/// Datos para evaluar.
pub(crate) struct Ctx {
    hoy: Date,
    año: i32,
    cierre: Date,
    /// Días del ejercicio (en uno de menos de un año se prorratean el tramo del 19 % y el millón
    /// de las bases negativas y de la nivelación: arts. 29.1, 26.1 y 105.1 LIS).
    dias: i64,
    fisica: bool,
    regimen: TaxRegime,
    /// Régimen puesto en «Datos de los modelos» (el que usa el borrador del 200).
    regimen_ajustes: TaxRegime,
    /// Entidad de reducida dimensión (cifra de negocios del año anterior < 10 M€).
    erd: bool,
    incn: Option<Money>,
    /// Base imponible antes de reservas (sociedades) o rendimiento neto (autónomos).
    base: Option<Money>,
    cuota: Option<Money>,
    /// Resultado contable del año anterior (lo que puede quedarse en reservas este año).
    resultado_anterior: Option<(Money, String)>,
    /// Dividendos acordados este año (abonos a la 526, también los ya pagados).
    dividendos_acordados: Money,
    /// Aumento de reservas que cuenta, si la junta ya ha aplicado el resultado (ver
    /// `aumento_contabilizado`).
    aumento_contabilizado: Option<Money>,
    /// Lo que irá a la reserva legal del beneficio del año anterior (no cuenta como aumento de
    /// fondos propios). `None` si no se sabe.
    reserva_legal: Option<Money>,
    /// Bases imponibles negativas pendientes con las que se calcula: las indicadas en «Datos de los
    /// modelos» o, si no hay, las que arrastran los 200 presentados.
    bases_pendientes: Money,
    /// Las indicadas en «Datos de los modelos» para este año.
    bin_indicadas: Money,
    /// Las que quedan según los 200 presentados (año del último, importe).
    bin_arrastre: Option<(i32, Money)>,
    /// Reserva de capitalización ya indicada para este año en «Datos de los modelos».
    capitalizacion_indicada: Money,
    /// [00558] (tipo de gravamen, en céntimos de punto: 21,00 % → 2100) del último 200 presentado.
    tipo_presentado: Option<(i32, Money)>,
    /// [01032] y [01034] del último 200 presentado (año, importe).
    capitalizacion_anterior: Option<(i32, Money)>,
    nivelacion_anterior: Option<(i32, Money)>,
    /// Dividendos de participadas (760) del año y lo ya indicado como corrección en disminución.
    dividendos_participadas: Money,
    /// La parte de empresas del grupo, asociadas o vinculadas (7600, 7601 y 7602): las que con
    /// seguridad son participaciones de cierto peso.
    dividendos_grupo: Money,
    /// Los de otras empresas (7603): normalmente acciones de una cartera, sin exención.
    dividendos_otras: Money,
    /// El último 200 ya aplicó la exención de dividendos ([00370] o [02181]).
    exencion_anterior: bool,
    correcciones_disminucion: Money,
    trabajadores: bool,
    cnae: Option<String>,
    /// Primer año con datos de la empresa (contabilidad o declaraciones).
    primer_año: Option<i32>,
    impagadas: Vec<Impagada>,
    /// Saldo de clientes (430) hoy.
    clientes: Money,
    /// Ganancias realizadas este año y pérdidas latentes con los últimos precios (cripto).
    cripto_ganancias: Money,
    cripto_perdidas_latentes: Money,
    /// Las monedas que pierden en conjunto (moneda, pérdida), de más a menos.
    cripto_monedas: Vec<(String, Money)>,
    hay_cripto: bool,
    /// Autónomo en estimación objetiva (módulos).
    modulos: bool,
    /// Reducción por inicio de actividad ya marcada en «Datos de los modelos» (art. 32.3 LIRPF).
    inicio_marcado: bool,
    /// Volumen de operaciones de más de 6.010.121,04 € (perfil fiscal).
    gran_empresa: bool,
    /// Primer año anterior con beneficio (ver `primer_año_con_beneficio`).
    primer_beneficio: Option<Option<i32>>,
    /// Test de entidad patrimonial (`patrimonial::senal`), en %: (con la tesorería no
    /// justificada, sin ella).
    inversiones: Option<(u8, u8)>,
    /// Comunidad autónoma del domicilio fiscal (perfil).
    ccaa: Option<taxpain_core::calendar::Ccaa>,
    /// Prorrata general del IVA de los ajustes (%) y si se aplica la especial.
    prorrata: Option<u8>,
    prorrata_especial: bool,
    /// Lo pagado de más en ejercicios anteriores según la revisión (aún rectificable).
    recuperable: Vec<Recuperable>,
}

/// Un hallazgo de la revisión con dinero que recuperar.
#[derive(Clone, Debug)]
struct Recuperable {
    ejercicio: Option<i32>,
    titulo: String,
    importe: Money,
    hasta: Option<Date>,
}

impl Ctx {
    /// Cuota íntegra de `base` con `regimen` en este ejercicio (con su duración).
    fn cuota_de(&self, base: Money, regimen: TaxRegime) -> Money {
        gross_tax_for(base, regimen, self.año, self.dias).0
    }

    /// Un millón de euros, prorrateado si el ejercicio dura menos de un año.
    fn millon(&self) -> Money {
        let m = Money::from_euros(1_000_000, 0);
        if self.dias > 0 && self.dias < 365 {
            Money::from_cents(m.cents() * self.dias / 365)
        } else {
            m
        }
    }

    /// Base que tributa (sociedades) si la base previa baja `reduccion`: después de compensar las
    /// bases negativas pendientes, como mucho 1 M€ o el 70 % de la base previa (art. 26.1 LIS).
    fn base_liquidable(&self, reduccion: Money) -> Money {
        let Some(base) = self.base.filter(|b| b.cents() > 0) else {
            return Money::ZERO;
        };
        // La reserva de capitalización ya indicada en «Datos de los modelos», con su tope (como
        // el borrador del 200).
        let tope = base.apply_rate(taxpain_core::money::Rate::percent(
            if self
                .incn
                .is_some_and(|x| x < Money::from_euros(1_000_000, 0))
            {
                25
            } else {
                20
            },
        ));
        let indicada = self.capitalizacion_indicada.max(Money::ZERO).min(tope);
        let previa = (base - indicada - reduccion).max(Money::ZERO);
        let limite = self
            .millon()
            .max(base.apply_rate(taxpain_core::money::Rate::percent(70)));
        previa - self.bases_pendientes.min(limite).min(previa)
    }

    /// Lo que se ahorra en cuota haciendo a la vez las rebajas de la base previa `r550` (exención de
    /// dividendos, deterioro) y la reserva de capitalización `cap` (antes de su tope, que es el 20 %
    /// o el 25 % de la base previa ya rebajada: art. 25.1 LIS).
    fn ahorro_conjunto(&self, r550: Money, cap: Money) -> Money {
        let Some(base) = self.base.filter(|b| b.cents() > 0) else {
            return Money::ZERO;
        };
        let previa = (base - r550).max(Money::ZERO);
        let tope = previa.apply_rate(taxpain_core::money::Rate::percent(
            if self
                .incn
                .is_some_and(|x| x < Money::from_euros(1_000_000, 0))
            {
                25
            } else {
                20
            },
        ));
        let cap = cap.min(tope);
        (self.cuota_de(self.base_liquidable(Money::ZERO), self.regimen)
            - self.cuota_de(self.base_liquidable(r550.min(base) + cap), self.regimen))
        .max(Money::ZERO)
    }

    /// Lo que se ahorra en cuota si una base que ya tributa (`liquidable`) baja `reduccion`.
    fn ahorro_desde(&self, liquidable: Money, reduccion: Money) -> Option<Money> {
        let r = reduccion.min(liquidable).max(Money::ZERO);
        let a =
            self.cuota_de(liquidable, self.regimen) - self.cuota_de(liquidable - r, self.regimen);
        (a.cents() > 0).then_some(a)
    }

    /// Autónomos: el rendimiento menos el mínimo personal (lo que, a grandes rasgos, tributa en la
    /// Renta si no hay otras rentas).
    fn base_irpf(&self) -> Money {
        self.base
            .map_or(Money::ZERO, |b| (b - MINIMO_PERSONAL).max(Money::ZERO))
    }

    /// Lo que se ahorra en cuota este ejercicio si la base baja `reduccion`. Si las bases negativas
    /// pendientes ya se comen el beneficio, nada (sólo se guardarían para otro año).
    fn ahorro_base(&self, reduccion: Money) -> Option<Money> {
        let base = self.base.filter(|b| b.cents() > 0)?;
        let r = reduccion.min(base).max(Money::ZERO);
        if r.is_zero() {
            return None;
        }
        let ahorro = if self.fisica {
            // En la Renta la escala se aplica a toda la base y luego se resta la del mínimo
            // personal: el último euro tributa al tipo de la base entera, y lo que cae dentro del
            // mínimo no tributa.
            r.min(self.base_irpf())
                .apply_rate(taxpain_core::money::Rate::percent(i32::from(
                    marginal_irpf(base),
                )))
        } else {
            self.cuota_de(self.base_liquidable(Money::ZERO), self.regimen)
                - self.cuota_de(self.base_liquidable(r), self.regimen)
        };
        (ahorro.cents() > 0).then_some(ahorro)
    }

    fn tipo(&self) -> Option<u8> {
        let base = self.base.filter(|b| b.cents() > 0)?;
        Some(if self.fisica {
            marginal_irpf(base)
        } else {
            // Tipo de los últimos euros de la base que tributa (19 % hasta 50.000 € en una
            // microempresa); sin base después de las bases negativas, no hay tipo.
            let liquidable = self.base_liquidable(Money::ZERO);
            if liquidable.cents() <= 0 {
                return None;
            }
            let tramo = Money::from_euros(100, 0).min(liquidable);
            let diferencia = self.cuota_de(liquidable, self.regimen)
                - self.cuota_de(liquidable - tramo, self.regimen);
            u8::try_from((diferencia.cents() * 100 + tramo.cents() / 2) / tramo.cents())
                .unwrap_or(25)
        })
    }

    /// Actividad tecnológica (CNAE): edición de software y videojuegos (582), programación y
    /// consultoría informática (62), proceso de datos y portales (631), ingeniería e
    /// investigación (7112, 7120, 72).
    fn tech(&self) -> bool {
        self.cnae.as_deref().is_some_and(|c| {
            ["582", "62", "631", "7112", "7120", "72"]
                .iter()
                .any(|p| c.starts_with(p))
        })
    }

    /// Industria que suele tener patentes o diseños propios (química, farmacia, electrónica,
    /// maquinaria, vehículos, instrumentos médicos).
    fn industria(&self) -> bool {
        self.cnae.as_deref().is_some_and(|c| {
            ["20", "21", "26", "27", "28", "29", "30", "325"]
                .iter()
                .any(|p| c.starts_with(p))
        })
    }

    fn audiovisual(&self) -> bool {
        self.cnae
            .as_deref()
            .is_some_and(|c| c.starts_with("59") || c.starts_with("90"))
    }
}

/// Mínimo personal general de la Renta (art. 57 LIRPF).
const MINIMO_PERSONAL: Money = Money::from_cents(555_000);

/// Tipo marginal aproximado de la Renta (escala general estatal más la autonómica estándar) con
/// un rendimiento neto `base`; orientativo: cada comunidad tiene su escala.
pub(crate) fn marginal_irpf(base: Money) -> u8 {
    let e = base.cents() / 100;
    match e {
        ..=12_450 => 19,
        12_451..=20_200 => 24,
        20_201..=35_200 => 30,
        35_201..=60_000 => 37,
        60_001..=300_000 => 45,
        _ => 47,
    }
}

/// Tipo del ahorro (ganancias de criptomonedas, acciones…) sobre `ganancia` (escala de 2025).
pub(crate) fn tipo_ahorro(ganancia: Money) -> u8 {
    match ganancia.cents() / 100 {
        ..=6_000 => 19,
        6_001..=50_000 => 21,
        50_001..=200_000 => 23,
        200_001..=300_000 => 27,
        _ => 30,
    }
}

fn regimen_texto(r: TaxRegime, erd: bool) -> &'static str {
    match r {
        TaxRegime::Micro => "Microempresa (cifra de negocios menor de 1 M€)",
        TaxRegime::ReducidaDimension => "Empresa de reducida dimensión",
        TaxRegime::NuevaCreacion => "Entidad de nueva creación (15 %)",
        TaxRegime::General if erd => "Empresa de reducida dimensión (tipo general)",
        TaxRegime::General => "Tipo general",
    }
}

/// Bases imponibles negativas pendientes al terminar el último 200 presentado antes de `año`, con
/// la misma cadena que la revisión: lo pendiente menos lo compensado [00547] más la base negativa
/// del año [00552]. Cada año, el 200 comprobado en la AEAT o, si no, el de la gestoría. `None` si
/// no hay 200, si falta alguno entre el primero y el último (no se sabe qué pasó ese año) o si el
/// primero ya compensa bases de antes (no se sabe cuánto quedaba).
fn bases_arrastradas(v: &Vault, año: i32) -> Result<Option<(i32, Money)>> {
    let aeat: BTreeMap<i32, BTreeMap<u32, Money>> = presentado_aeat::vigentes(v)?
        .into_iter()
        .filter(|x| x.model == "200" && x.year < año)
        .filter_map(|x| Some((x.year, x.boxes?.casillas)))
        .collect();
    let desde = aeat
        .keys()
        .next()
        .copied()
        .unwrap_or(año - 10)
        .min(año - 10);
    let mut doscientos: BTreeMap<i32, BTreeMap<u32, Money>> = BTreeMap::new();
    for y in desde..año {
        if let Some(b) = aeat.get(&y) {
            doscientos.insert(y, b.clone());
        } else if let Some((_, b)) = models::filed_boxes(v, "200", y, "0A")? {
            doscientos.insert(y, b.into_iter().map(|(k, m)| (u32::from(k), m)).collect());
        }
    }
    let mut pendiente: Option<Money> = None;
    let mut ultimo: Option<i32> = None;
    for (y, b) in &doscientos {
        if ultimo.is_some_and(|u| u != y - 1) {
            return Ok(None);
        }
        let usada = b.get(&547).copied().unwrap_or(Money::ZERO);
        let base = b.get(&552).copied().unwrap_or(Money::ZERO);
        let generada = (-base).max(Money::ZERO);
        pendiente = match pendiente {
            Some(p) => Some((p - usada).max(Money::ZERO) + generada),
            None if usada.is_zero() => Some(generada),
            None => None,
        };
        ultimo = Some(*y);
    }
    Ok(ultimo.zip(pendiente))
}

/// Primer año anterior a `año` con beneficio: base imponible positiva en un 200 (de la AEAT o de
/// la gestoría) o resultado positivo en la contabilidad de TaxPain. `Some(None)`: hay datos de
/// años anteriores y en ninguno hubo beneficio; `None`: no hay datos.
fn primer_año_con_beneficio(v: &Vault, año: i32) -> Result<Option<Option<i32>>> {
    let mut hay_datos = false;
    let mut primero: Option<i32> = None;
    let mut ver = |y: i32, positivo: bool| {
        hay_datos = true;
        if positivo && primero.is_none_or(|p| y < p) {
            primero = Some(y);
        }
    };
    for x in presentado_aeat::vigentes(v)? {
        if x.model == "200"
            && x.year < año
            && let Some(b) = x.boxes
        {
            ver(x.year, b.casillas.get(&552).is_some_and(|m| m.cents() > 0));
        }
    }
    for y in año - 10..año {
        if let Some((_, b)) = models::filed_boxes(v, "200", y, "0A")? {
            ver(y, b.get(&552).is_some_and(|m| m.cents() > 0));
        }
    }
    // Ingresos y gastos (sin el impuesto) de cada año de la contabilidad, en una sola consulta.
    let mut st = v.connection().prepare(
        "SELECT e.fiscal_year,
                COALESCE(SUM(CASE WHEN l.account LIKE '7%' THEN l.credit - l.debit ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN l.account LIKE '6%'
                                   AND substr(l.account, 1, 3) NOT IN ('630', '633', '638')
                                  THEN l.debit - l.credit ELSE 0 END), 0)
         FROM journal_line l JOIN journal_entry e ON e.id = l.entry_id
         WHERE e.origin_kind NOT IN ('Regularizacion', 'Cierre')
           AND e.fiscal_year BETWEEN ?1 AND ?2
         GROUP BY e.fiscal_year",
    )?;
    let filas = st.query_map([año - 10, año - 1], |r| {
        Ok((
            r.get::<_, i32>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for fila in filas {
        let (y, ingresos, gastos) = fila?;
        if ingresos != 0 || gastos != 0 {
            ver(y, ingresos > gastos);
        }
    }
    Ok(hay_datos.then_some(primero))
}

/// El régimen para una frase («el tipo de microempresa», «el tipo general»).
fn regimen_corto(r: TaxRegime) -> &'static str {
    match r {
        TaxRegime::Micro => "de microempresa",
        TaxRegime::ReducidaDimension => "de reducida dimensión",
        TaxRegime::NuevaCreacion => "de nueva creación",
        TaxRegime::General => "general",
    }
}

/// Último 200 presentado hasta `año` (de la AEAT o de la gestoría), con su año.
fn ultimo_200(v: &Vault, año: i32) -> Result<Option<(i32, BTreeMap<u32, Money>)>> {
    let vigentes = presentado_aeat::vigentes(v)?;
    let aeat = vigentes
        .iter()
        .filter(|x| x.model == "200" && x.year <= año)
        .filter_map(|x| Some((x.year, x.boxes.as_ref()?.casillas.clone())))
        .max_by_key(|(y, _)| *y);
    let desde = aeat.as_ref().map_or(año - 3, |(y, _)| y + 1);
    for y in (desde..=año).rev() {
        if let Some((_, b)) = models::filed_boxes(v, "200", y, "0A")? {
            return Ok(Some((
                y,
                b.into_iter().map(|(k, m)| (u32::from(k), m)).collect(),
            )));
        }
    }
    Ok(aeat)
}

/// Base del ejercicio estimada sin contabilidad, con lo que hay del año en TaxPain.
#[derive(Debug)]
struct Estimacion {
    /// Ingresos menos gastos (puede ser negativa).
    resultado: Money,
    fuente: String,
    notas: Vec<String>,
}

/// Percepciones del trabajo ([02] dinerarias y [05] en especie) de los 111 presentados de los
/// periodos (trimestres o meses) que empiezan entre `desde` y `hasta`, con los periodos que hay.
fn trabajo_111(v: &Vault, desde: Date, hasta: Date) -> Result<(Money, Vec<String>)> {
    let vigentes = presentado_aeat::vigentes(v)?;
    let mut total = Money::ZERO;
    let mut periodos = Vec::new();
    for y in desde.year()..=hasta.year() {
        let trimestres = (1..=4u8).map(|q| (format!("{q}T"), 3 * q - 2));
        let meses = (1..=12u8).map(|m| (format!("{m:02}"), m));
        for (p, mes) in trimestres.chain(meses) {
            let Some(inicio) = time::Month::try_from(mes)
                .ok()
                .and_then(|m| Date::from_calendar_date(y, m, 1).ok())
            else {
                continue;
            };
            if inicio < desde || inicio > hasta {
                continue;
            }
            let casillas: Option<BTreeMap<u32, Money>> = match models::filed_boxes(v, "111", y, &p)?
            {
                Some((_, b)) => Some(b.into_iter().map(|(k, m)| (u32::from(k), m)).collect()),
                None => vigentes
                    .iter()
                    .find(|x| x.model == "111" && x.year == y && x.period == p)
                    .and_then(|x| x.boxes.as_ref().map(|b| b.casillas.clone())),
            };
            if let Some(b) = casillas {
                let get = |k: u32| b.get(&k).copied().unwrap_or(Money::ZERO);
                total += get(2) + get(5);
                periodos.push(format!("{p} de {y}"));
            }
        }
    }
    Ok((total, periodos))
}

/// Cobros del banco entre `desde` y `hasta` con el concepto de dividendo (no su devolución).
fn dividendos_banco(v: &Vault, desde: Date, hasta: Date) -> Result<(Money, usize)> {
    let mut stmt = v.connection().prepare(
        "SELECT amount, description FROM bank_tx
         WHERE amount > 0 AND op_date BETWEEN ?1 AND ?2",
    )?;
    let rows = stmt.query_map([desde.to_string(), hasta.to_string()], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let (mut total, mut n) = (Money::ZERO, 0);
    for row in rows {
        let (importe, concepto) = row?;
        if crate::revision::es_dividendo(&concepto) {
            total += Money::from_cents(importe);
            n += 1;
        }
    }
    Ok((total, n))
}

/// Sin contabilidad del ejercicio: el resultado estimado con los libros registro del año
/// (emitidas menos recibidas), menos los sueldos (de Nóminas o, si no hay, de los 111
/// presentados) y la amortización devengada, y, en una sociedad, el 5 % de los dividendos
/// cobrados en el banco. `None` si el año no tiene ninguna factura.
fn estimacion_libros(
    v: &Vault,
    año: i32,
    desde: Date,
    hasta: Date,
    fisica: bool,
) -> Result<Option<Estimacion>> {
    let f = models::invoiced_between(v, desde, hasta)?;
    if f.emitidas + f.recibidas == 0 {
        return Ok(None);
    }
    let fecha = |d: Date| format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year());
    let ultima = f.hasta.unwrap_or(hasta);
    let mut resultado = f.ingresos - f.gastos;
    let mut fuente = format!(
        "facturas emitidas menos recibidas de {año} hasta el {} (libros registro)",
        fecha(ultima)
    );
    let mut menos = Vec::new();
    let mut notas = vec![format!(
        "Sin contabilidad de {año} en TaxPain: la base es una estimación con los libros registro \
         ({} emitidas y {} recibidas, la última del {}): {} de ingresos menos {} de gastos. \
         Faltan los gastos sin factura (comisiones del banco, seguros sociales, tributos…), los \
         meses que quedan y los ajustes del cierre; importa la contabilidad de {año} para \
         afinarla.",
        f.emitidas,
        f.recibidas,
        fecha(ultima),
        f.ingresos,
        f.gastos
    )];

    let nominas = crate::payroll::payslips(v.connection(), desde, hasta)?;
    if nominas.is_empty() {
        let (trabajo, periodos) = trabajo_111(v, desde, hasta)?;
        if trabajo.cents() > 0 {
            resultado -= trabajo;
            menos.push(format!("los sueldos de los 111 ({})", periodos.join(", ")));
            notas.push(format!(
                "Los sueldos ({trabajo}) son las percepciones del trabajo de los 111 presentados \
                 ([02] y [05]); falta la Seguridad Social a cargo de la empresa."
            ));
        }
    } else {
        let sueldos: Money = nominas.iter().map(|n| n.gross + n.ss_company).sum();
        resultado -= sueldos;
        menos.push("las nóminas con su Seguridad Social".to_owned());
    }

    let amortizacion = accounting::accrued_depreciation(v, año, hasta)
        .unwrap_or(Money::ZERO)
        .max(Money::ZERO);
    if amortizacion.cents() > 0 {
        resultado -= amortizacion;
        menos.push(format!("{amortizacion} de amortización"));
    }
    if f.inversion.cents() > 0 {
        notas.push(format!(
            "Las facturas de bienes de inversión ({}) no restan enteras: sólo su amortización, \
             si el bien está dado de alta en el inmovilizado.",
            f.inversion
        ));
    }

    if !fisica {
        let (banco, n) = dividendos_banco(v, desde, hasta)?;
        // Los que la revisión de la carpeta confirma con los documentos de las participadas:
        // su bruto en vez de lo cobrado, y su retención, que se resta de la cuota del 200.
        let confirmados: Vec<crate::revision::DividendoConfirmado> = crate::revision::ultima(v)
            .map(|r| r.dividendos)
            .unwrap_or_default()
            .into_iter()
            .filter(|d| d.ejercicio == año)
            .collect();
        let neto_conf: Money = confirmados.iter().map(|d| d.neto).sum();
        let bruto_conf: Money = confirmados.iter().map(|d| d.bruto).sum();
        let retenido: Money = confirmados.iter().map(|d| d.retencion).sum();
        let dividendos = (banco - neto_conf).max(Money::ZERO) + bruto_conf;
        if dividendos.cents() > 0 {
            let tributa = dividendos.apply_rate(taxpain_core::money::Rate::percent(5));
            resultado += tributa;
            fuente.push_str(", más el 5 % de los dividendos cobrados");
            let cuales = if confirmados.is_empty() {
                format!("En el banco hay {n} cobro(s) de dividendos este año ({dividendos})")
            } else {
                format!(
                    "Este año hay {dividendos} de dividendos brutos ({bruto_conf} de {} \
                     confirmados con los documentos de las participadas en la revisión de la \
                     carpeta, que en el banco son {neto_conf} tras la retención)",
                    confirmados
                        .iter()
                        .map(|d| d.pagadora.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            notas.push(format!(
                "{cuales}: se suma sólo el 5 % ({tributa}), suponiendo la exención del art. 21 \
                 de la Ley 27/2014 (participación de al menos el 5 % durante un año). Si no la \
                 tienen (fondos, acciones de una cartera), tributan enteros y la base sería {} \
                 más alta.",
                dividendos - tributa
            ));
            if retenido.cents() > 0 {
                notas.push(format!(
                    "Las retenciones soportadas en esos dividendos ({retenido}) son un pago a \
                     cuenta: se restan de la cuota del 200 de {año} (y, si es menor, se \
                     devuelven). La cuota estimada no las descuenta."
                ));
            }
        }
    }
    if !menos.is_empty() {
        fuente.push_str(&format!(", menos {}", menos.join(" y ")));
    }
    Ok(Some(Estimacion {
        resultado,
        fuente,
        notas,
    }))
}

/// Debe y haber de las cuentas que empiezan por `prefijo` en los asientos `diario` de un
/// ejercicio, sin regularización ni cierre; con `apertura` (`Some(true)`) sólo el asiento de
/// apertura y con `Some(false)` todo menos la apertura.
fn movimientos(
    diario: &[accounting::StoredEntry],
    prefijo: &str,
    apertura: Option<bool>,
) -> (Money, Money) {
    use taxpain_core::accounting::Origin;
    let (mut debe, mut haber) = (Money::ZERO, Money::ZERO);
    for e in diario {
        let o = &e.entry.origin;
        if matches!(o, Origin::Regularizacion | Origin::Cierre) {
            continue;
        }
        if apertura.is_some_and(|a| a != matches!(o, Origin::Apertura)) {
            continue;
        }
        for l in e
            .entry
            .lines
            .iter()
            .filter(|l| l.account.as_str().starts_with(prefijo))
        {
            debe += l.debit;
            haber += l.credit;
        }
    }
    (debe, haber)
}

/// Saldo (debe − haber) de las cuentas que empiezan por `prefijo`, sin regularización ni cierre.
fn saldo(diario: &[accounting::StoredEntry], prefijo: &str) -> Money {
    let (d, h) = movimientos(diario, prefijo, None);
    d - h
}

/// Ingresos (grupo 7), gastos (grupo 6 sin el impuesto sobre beneficios) e impuesto sobre
/// beneficios (630, 633 y 638) del ejercicio, como `models::income_expenses`.
fn resultado(diario: &[accounting::StoredEntry]) -> (Money, Money, Money) {
    use taxpain_core::accounting::Origin;
    let (mut ingresos, mut gastos, mut impuesto) = (Money::ZERO, Money::ZERO, Money::ZERO);
    for e in diario {
        if matches!(e.entry.origin, Origin::Regularizacion | Origin::Cierre) {
            continue;
        }
        for l in &e.entry.lines {
            let cuenta = l.account.as_str();
            if models::is_profit_tax(cuenta) {
                impuesto += l.net();
            } else if cuenta.starts_with('7') {
                ingresos -= l.net();
            } else if cuenta.starts_with('6') {
                gastos += l.net();
            }
        }
    }
    (ingresos, gastos, impuesto)
}

/// Aumento de fondos propios del año que cuenta para la reserva de capitalización según el diario,
/// si la junta ya ha aplicado el resultado del año anterior (cargos a la 129 fuera de la apertura):
/// lo que ha ido a reservas voluntarias (113), a remanente (120) o a compensar pérdidas de años
/// anteriores (121), menos lo que se ha sacado de ellas. La reserva legal (112), las aportaciones
/// de los socios (118) y el capital no cuentan (art. 25.2 LIS).
fn aumento_contabilizado(diario: &[accounting::StoredEntry]) -> Option<Money> {
    let (aplicado, _) = movimientos(diario, "129", Some(false));
    if aplicado.is_zero() {
        return None;
    }
    // Con signo: si salen reservas (por ejemplo, para repartir dividendos), es negativo.
    Some(
        ["113", "120", "121"]
            .iter()
            .map(|p| {
                let (d, h) = movimientos(diario, p, Some(false));
                h - d
            })
            .sum(),
    )
}

/// Lo que irá a la reserva legal este año: lo ya dotado (abonos a la 112 fuera de la apertura) o,
/// si aún no, el 10 % del beneficio del año anterior hasta que la reserva llegue al 20 % del
/// capital (art. 274 de la Ley de Sociedades de Capital). `None` sin contabilidad del año.
fn reserva_legal(diario: &[accounting::StoredEntry], beneficio: Money) -> Option<Money> {
    if diario.is_empty() {
        return None;
    }
    let (_, dotado) = movimientos(diario, "112", Some(false));
    if dotado.cents() > 0 {
        return Some(dotado);
    }
    let (d, h) = movimientos(diario, "100", Some(true));
    let capital = h - d;
    if capital.cents() <= 0 {
        return None;
    }
    let (d, h) = movimientos(diario, "112", Some(true));
    let legal = h - d;
    let falta =
        (capital.apply_rate(taxpain_core::money::Rate::percent(20)) - legal).max(Money::ZERO);
    Some(
        beneficio
            .max(Money::ZERO)
            .apply_rate(taxpain_core::money::Rate::percent(10))
            .min(falta),
    )
}

fn primer_año(v: &Vault) -> Result<Option<i32>> {
    let diario: Option<i32> =
        v.connection()
            .query_row("SELECT MIN(fiscal_year) FROM journal_entry", [], |r| {
                r.get(0)
            })?;
    let aeat = presentado_aeat::vigentes(v)?.iter().map(|x| x.year).min();
    Ok(match (diario, aeat) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    })
}

/// Facturas emitidas vencidas y sin cobrar, una por factura: lo pendiente de sus plazos vencidos
/// menos los abonos pendientes que la rectifican (como `dashboard::receivables`). Las que ya
/// tienen una rectificativa por créditos incobrables (R3) o por concurso (R2) van sin IVA: ya se
/// ha modificado.
fn impagadas(v: &Vault, hoy: Date) -> Result<Vec<Impagada>> {
    let mut rectifica: BTreeMap<i64, (i64, String)> = BTreeMap::new();
    {
        let mut st = v.connection().prepare(
            "SELECT id, rectifies_id, kind FROM invoice_issued
             WHERE rectifies_id IS NOT NULL AND status = 'emitida'",
        )?;
        let filas = st.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for fila in filas {
            let (id, original, kind) = fila?;
            rectifica.insert(id, (original, kind));
        }
    }
    let modificadas: BTreeSet<i64> = rectifica
        .values()
        .filter(|(_, k)| k == "R2" || k == "R3")
        .map(|(o, _)| *o)
        .collect();
    let mut abonos: BTreeMap<i64, Money> = BTreeMap::new();
    // Por factura: (emisión, primer vencimiento, pendiente vencido, total de la factura).
    let mut vencidas: BTreeMap<i64, (Date, Date, Money, Money)> = BTreeMap::new();
    for d in cartera::list(v, hoy)? {
        if d.kind != "emitida" || d.status != "pendiente" {
            continue;
        }
        if d.amount.is_negative() {
            if let Some((original, _)) = rectifica.get(&d.invoice_id) {
                *abonos.entry(*original).or_default() += d.amount;
            }
            continue;
        }
        if d.overdue_days <= 0 {
            continue;
        }
        let e = vencidas.entry(d.invoice_id).or_insert((
            d.issue_date,
            d.due_date,
            Money::ZERO,
            d.invoice_amount,
        ));
        e.1 = e.1.min(d.due_date);
        e.2 += d.amount;
    }
    let mut out = Vec::new();
    for (id, (emision, vencimiento, pendiente, total)) in vencidas {
        let pendiente = pendiente + abonos.get(&id).copied().unwrap_or_default();
        if pendiente.cents() <= 0 || total.cents() <= 0 {
            continue;
        }
        // Con la rectificativa por impago o concurso ya emitida, el IVA está recuperado; lo que
        // siga pendiente puede seguir siendo gasto por deterioro.
        let vat: i64 = if modificadas.contains(&id) {
            0
        } else {
            v.connection()
                .query_row("SELECT vat FROM invoice_issued WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .unwrap_or(0)
        };
        // El IVA de la parte sin cobrar (la reducción es proporcional a lo impagado).
        let iva = i128::from(vat) * i128::from(pendiente.cents()) / i128::from(total.cents());
        out.push(Impagada {
            emision,
            vencimiento,
            pendiente,
            iva: Money::from_cents(i64::try_from(iva).unwrap_or(0)),
        });
    }
    Ok(out)
}

/// Lo que la revisión con lo presentado en la AEAT (`revision::revisar_aeat`) dice que se pagó de
/// más y aún se puede rectificar. Si la revisión falla, nada: no es imprescindible.
fn recuperable(v: &Vault, hoy: Date) -> Vec<Recuperable> {
    let Ok(r) = crate::revision::revisar_aeat(v, hoy) else {
        return Vec::new();
    };
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    r.hallazgos
        .into_iter()
        .filter(|h| h.importe.is_some_and(|i| i.cents() > 0))
        .filter_map(|h| {
            let hasta = h
                .rectificable_hasta
                .as_deref()
                .and_then(|d| Date::parse(d, &fmt).ok());
            if hasta.is_some_and(|d| d < hoy) {
                return None;
            }
            Some(Recuperable {
                ejercicio: h.ejercicio,
                titulo: h.titulo,
                importe: h.importe?,
                hasta,
            })
        })
        .collect()
}

/// Reúne los datos de la empresa para el ejercicio que contiene `hoy`.
pub(crate) fn contexto(v: &Vault, hoy: Date) -> Result<(Ctx, Situacion)> {
    let profile: Option<TaxpayerProfile> = v.setting("taxpayer_profile")?;
    let perfil = profile.as_ref().map(|p| p.persona);
    // El NIF de los ajustes de facturación o, si no factura desde TaxPain, el de la empresa.
    let nif = crate::carpeta::own_nif(v).and_then(|n| Nif::parse(&n).ok());
    // Comunidad de bienes, herencia yacente, sociedad civil sin objeto mercantil: no paga
    // Sociedades; su rendimiento va a la Renta de cada socio.
    let atribucion = perfil == Some(Persona::EntidadAtribucion)
        || (perfil.is_none() && nif.as_ref().is_some_and(|n| n.as_str().starts_with('E')));
    // El NIF manda: una sociedad nunca es persona física aunque el perfil esté mal puesto.
    let fisica = match &nif {
        Some(n) if n.is_persona_fisica() => true,
        Some(_) => atribucion,
        None => matches!(perfil, Some(Persona::Fisica | Persona::EntidadAtribucion)),
    };
    let fy = accounting::fiscal_year_at(v, hoy)?;
    let año = fy.year;
    let s = models::settings(v)?;
    let (ys, _) = s.year_settings(año);
    let cnae = Some(s.cnae.trim().to_owned()).filter(|c| !c.is_empty());
    let mut notas = Vec::new();
    if atribucion {
        notas.push(
            "Es una entidad en atribución de rentas (comunidad de bienes o similar): no paga \
             Sociedades, el rendimiento va a la Renta de cada socio en su parte, y las \
             recomendaciones de la Renta son para cada uno."
                .into(),
        );
    }

    let ultimo = ultimo_200(v, año - 1)?;
    let casilla = |c: u32| ultimo.as_ref().and_then(|(_, b)| b.get(&c).copied());
    // El diario de este año y del anterior, leídos una vez.
    let diario = accounting::journal(v, año)?;
    let diario_anterior = accounting::journal(v, año - 1)?;
    // «Con contabilidad» sólo si el año tiene ingresos o gastos (no sólo la apertura).
    let con_diario = {
        let (i, g, _) = resultado(&diario);
        !(i.is_zero() && g.is_zero())
    };

    // Cifra de negocios del año anterior (la que decide el tipo): la de su 200; si no, las ventas
    // (70) de su contabilidad; si no, la del último 200 que haya.
    let anterior = año - 1;
    let del_200 = match (casilla(255), &ultimo) {
        (Some(x), Some((y, _))) => Some((*y, x)),
        _ => None,
    };
    let ventas = -saldo(&diario_anterior, "70");
    let incn = match del_200 {
        Some((y, x)) if y == anterior => Some(Cifra {
            valor: x,
            fuente: format!("200 de {y} [00255]"),
        }),
        _ if ventas.cents() > 0 => Some(Cifra {
            valor: ventas,
            fuente: format!("ventas (70) de la contabilidad de {anterior}"),
        }),
        Some((y, x)) => Some(Cifra {
            valor: x,
            fuente: format!("200 de {y} [00255], el último que hay"),
        }),
        None => None,
    };
    // Si el ejercicio anterior duró menos de un año, la cifra se eleva al año (art. 101.2 LIS).
    let incn = incn.map(|c| match accounting::fiscal_year(v, anterior) {
        Ok(fa) if fa.days() > 0 && fa.days() < fa.full_year_days() => Cifra {
            valor: Money::from_cents(
                i64::try_from(
                    i128::from(c.valor.cents()) * i128::from(fa.full_year_days())
                        / i128::from(fa.days()),
                )
                .unwrap_or(i64::MAX),
            ),
            fuente: format!(
                "{}, elevada al año (el ejercicio duró {} días)",
                c.fuente,
                fa.days()
            ),
        },
        _ => c,
    });
    let diez = Money::from_euros(10_000_000, 0);
    let mut erd = incn.as_ref().is_some_and(|c| c.valor < diez);
    // Prórroga del art. 101.4 LIS: quien llega a los 10 M€ mantiene los incentivos de empresa de
    // reducida dimensión (no el tipo) ese año y los tres siguientes si en los tres anteriores lo
    // era. Con las cifras de los 200 (o de la contabilidad) de los años que haga falta.
    if !erd && incn.is_some() {
        let cifra = |y: i32| -> Option<Money> {
            let del_200 = presentado_aeat::vigentes(v)
                .ok()?
                .into_iter()
                .find_map(|x| {
                    (x.model == "200" && x.year == y)
                        .then(|| x.boxes.and_then(|b| b.casillas.get(&255).copied()))
                        .flatten()
                });
            del_200.or_else(|| {
                let d = accounting::journal(v, y).ok()?;
                let ventas = -saldo(&d, "70");
                (ventas.cents() > 0).then_some(ventas)
            })
        };
        // N: primer año del tramo seguido de 10 M€ o más que acaba en el año anterior.
        let mut n = año - 1;
        while n > año - 4 && cifra(n - 1).is_some_and(|x| x >= diez) {
            n -= 1;
        }
        let antes: Vec<Option<Money>> = (n - 3..n).map(cifra).collect();
        if n > año - 4 && antes.iter().all(|x| x.is_some_and(|m| m < diez)) {
            erd = true;
            notas.push(format!(
                "Tu cifra de negocios pasó de 10 millones en {n}: mantienes los incentivos de \
                 empresa de reducida dimensión (amortización acelerada, nivelación…) hasta {} \
                 (art. 101.4 de la Ley 27/2014), pero no los tipos reducidos.",
                n + 3
            ));
        }
    }
    let regimen_ajustes = ys.corporate_regime.unwrap_or(s.corporate_regime);
    let regimen = match regimen_ajustes {
        TaxRegime::NuevaCreacion => TaxRegime::NuevaCreacion,
        r => match incn.as_ref().map(|c| c.valor) {
            Some(x) if x < Money::from_euros(1_000_000, 0) => TaxRegime::Micro,
            Some(x) if x < Money::from_euros(10_000_000, 0) => TaxRegime::ReducidaDimension,
            Some(_) => TaxRegime::General,
            None => r,
        },
    };

    let mut base = None;
    let mut cuota = None;
    let borrador_200 = (!fisica && con_diario).then(|| models::draft_200(v, año));
    // Sin contabilidad del año: antes que el último 200 (otro año, que puede no parecerse en
    // nada), lo que digan los libros registro de este.
    let estimada = if con_diario {
        None
    } else {
        estimacion_libros(v, año, fy.start, hoy.min(fy.end), fisica)
            .ok()
            .flatten()
    };
    if fisica {
        match (con_diario, con_diario.then(|| renta::activity(v, año))) {
            (true, Some(Ok(a))) => {
                base = Some(Cifra {
                    valor: a.result.net_reduced,
                    fuente: format!(
                        "rendimiento neto de la actividad de {año} hasta hoy (borrador de la \
                         Renta)"
                    ),
                });
            }
            (true, Some(Err(e))) => {
                let (i, g, _) = resultado(&diario);
                base = Some(Cifra {
                    valor: i - g,
                    fuente: format!("ingresos menos gastos de la contabilidad de {año} hasta hoy"),
                });
                notas.push(format!(
                    "No se ha podido hacer el borrador de la Renta ({e}): el rendimiento sale de \
                     ingresos menos gastos, sin las reducciones de la Renta."
                ));
            }
            _ => match estimada {
                Some(e) => {
                    base = Some(Cifra {
                        valor: e.resultado.max(Money::ZERO),
                        fuente: format!("rendimiento estimado: {}", e.fuente),
                    });
                    notas.extend(e.notas);
                }
                None => notas.push(format!(
                    "Sin contabilidad de {año} en TaxPain: el ahorro de la Renta no se puede \
                     estimar."
                )),
            },
        }
    } else if let Some(Err(e)) = &borrador_200 {
        let (i, g, _) = resultado(&diario);
        let amortizacion = accounting::accrued_depreciation(v, año, hoy.min(fy.end))
            .unwrap_or(Money::ZERO)
            .max(Money::ZERO);
        let previa = i - g - amortizacion;
        base = Some(Cifra {
            valor: previa,
            fuente: format!(
                "ingresos menos gastos de la contabilidad de {año} hasta hoy (sin el borrador del \
                 200)"
            ),
        });
        cuota = Some(Cifra {
            valor: gross_tax_for(previa, regimen, año, fy.days()).0,
            fuente: format!("esa base con el tipo {}", regimen_corto(regimen)),
        });
        notas.push(format!(
            "No se ha podido hacer el borrador del 200 ({e}): la base sale de ingresos menos \
             gastos, sin los ajustes del impuesto."
        ));
    } else if let Some(Ok(d)) = borrador_200 {
        let get = |c: u16| {
            d.boxes
                .iter()
                .find(|(k, _)| *k == c)
                .map_or(Money::ZERO, |(_, m)| *m)
        };
        // La amortización de este año que aún no está en el diario (se contabiliza al cierre).
        let amortizacion = accounting::accrued_depreciation(v, año, hoy.min(fy.end))
            .unwrap_or(Money::ZERO)
            .max(Money::ZERO);
        let menos = if amortizacion.cents() > 0 {
            format!(", menos {amortizacion} de amortización aún sin contabilizar")
        } else {
            String::new()
        };
        base = Some(Cifra {
            valor: get(550) - amortizacion,
            fuente: format!(
                "borrador del 200 de {año} con la contabilidad hasta hoy [00550]{menos}"
            ),
        });
        // La cuota, con el tipo que corresponde por la cifra de negocios (el borrador usa el
        // régimen de «Datos de los modelos», que puede no estar puesto: ver `tipo_reducido`).
        cuota = Some(Cifra {
            valor: gross_tax_for(get(552) - amortizacion, regimen, año, fy.days()).0,
            fuente: format!(
                "base del borrador del 200 de {año} [00552]{menos}, con el tipo {}",
                regimen_corto(regimen)
            ),
        });
        notas.push(format!(
            "Las cifras de {año} son las de la contabilidad hasta hoy y pueden cambiar al cierre: \
             faltan los ingresos y gastos de los meses que quedan y los asientos de cierre. Vuelve \
             a mirarlo antes de decidir."
        ));
    } else if let Some(e) = estimada {
        let previa = e.resultado.max(Money::ZERO);
        base = Some(Cifra {
            valor: previa,
            fuente: e.fuente,
        });
        cuota = Some(Cifra {
            valor: gross_tax_for(previa, regimen, año, fy.days()).0,
            fuente: format!("esa base con el tipo {}", regimen_corto(regimen)),
        });
        if e.resultado.cents() < 0 {
            notas.push(format!(
                "Con esas cifras {año} va en pérdidas ({}): no hay impuesto que rebajar.",
                e.resultado
            ));
        }
        notas.extend(e.notas);
    } else if let Some((y, _)) = &ultimo {
        if let Some(b) = casilla(550).or(casilla(552)) {
            base = Some(Cifra {
                valor: b,
                fuente: format!(
                    "200 de {y} presentado [00550], de otro año: sólo como referencia para {año}"
                ),
            });
        }
        if let Some(c) = casilla(562) {
            cuota = Some(Cifra {
                valor: c,
                fuente: format!("200 de {y} presentado [00562]"),
            });
        }
        notas.push(format!(
            "Sin contabilidad ni libros registro de {año} en TaxPain: se usa el 200 de {y} como \
             referencia, que es otro año y puede no parecerse en nada. Si {año} va muy distinto, \
             importa la contabilidad o los libros de {año}."
        ));
    } else {
        notas.push(
            "Sin contabilidad del año ni Impuesto sobre Sociedades presentado: no se puede \
             estimar el ahorro. Importa la contabilidad o comprueba lo presentado en la AEAT."
                .into(),
        );
    }

    // Resultado del año anterior después del impuesto (lo que puede quedarse en reservas): el del
    // 200 [00500]; si no, el pendiente de aplicar en la apertura de este año (129); si no, el de
    // la contabilidad del año anterior, y si no tiene el impuesto contabilizado, con uno estimado
    // con la escala.
    let (d129, h129) = movimientos(&diario, "129", Some(true));
    let resultado_anterior = match (&ultimo, casilla(500)) {
        (Some((y, _)), Some(r)) if *y == anterior => Some((r, format!("200 de {y} [00500]"))),
        _ if h129 > d129 => Some((
            h129 - d129,
            format!("resultado de {anterior} en la apertura de {año} (129)"),
        )),
        _ if !diario_anterior.is_empty() => {
            let (i, g, impuesto) = resultado(&diario_anterior);
            let antes = i - g;
            if impuesto.is_zero() && antes.cents() > 0 {
                let estimado = gross_tax(antes, regimen, anterior).0;
                Some((
                    antes - estimado,
                    format!(
                        "contabilidad de {anterior}, después de un impuesto estimado de {estimado}"
                    ),
                ))
            } else {
                Some((antes - impuesto, format!("contabilidad de {anterior}")))
            }
        }
        _ => None,
    };
    let ult = |c: u32| {
        ultimo
            .as_ref()
            .and_then(|(y, b)| b.get(&c).filter(|m| m.cents() > 0).map(|m| (*y, *m)))
    };
    // Trabajadores con contrato: los de Nóminas que no son administradores, o Seguridad Social a
    // cargo de la empresa (642). Los sueldos (640) y la [00270] del 200 no sirven: incluyen lo que
    // cobra el socio-administrador sin contrato laboral.
    let trabajadores = profile.as_ref().is_some_and(|p| p.tiene_trabajadores)
        || crate::payroll::employees(v)
            .unwrap_or_default()
            .iter()
            .any(|e| e.active && !e.administrator)
        || saldo(&diario, "642").cents() > 0;
    let cr = cripto::report(v, año).ok();
    let (cripto_ganancias, cripto_perdidas_latentes, cripto_monedas, hay_cripto) = match &cr {
        // Un ejercicio anterior al punto de partida lo calculó la gestoría: nada que proponer.
        Some(r) if !r.antes_del_inicio && (!r.posiciones.is_empty() || r.ops_ejercicio > 0) => {
            let ganancias = if fisica {
                // La Renta usa FIFO global (no por plataforma), como `cripto::renta`.
                cripto::renta(v, año)
                    .ok()
                    .flatten()
                    .map_or(Money::ZERO, |x| x.neto)
            } else if fy.start.year() == fy.end.year() {
                if r.metodo_declaracion == taxpain_core::cripto::Metodo::Pmp {
                    r.resumen_pmp.neto
                } else {
                    r.resumen_fifo.neto
                }
            } else {
                // Ejercicio no natural: lo realizado entre el inicio y el cierre del ejercicio
                // (los informes van por año natural).
                let mut vistos = BTreeSet::new();
                [Some(r.clone()), cripto::report(v, año + 1).ok()]
                    .into_iter()
                    .flatten()
                    .flat_map(|x| x.realizados)
                    .filter(|x| fy.contains(x.fecha) && vistos.insert(x.op))
                    .map(|x| x.resultado)
                    .sum()
            };
            // Lo que pierde cada moneda en conjunto si se venden todas sus unidades: en la Renta,
            // con FIFO global (como `cripto::renta`, no por plataforma ni con el precio medio de
            // los ajustes); en una sociedad, con su método.
            let renta;
            let posiciones = if fisica {
                renta = cripto::posiciones_renta(v, hoy).unwrap_or_default();
                &renta
            } else {
                &r.posiciones
            };
            let mut por_moneda: BTreeMap<&str, (Money, taxpain_core::cripto::Qty)> =
                BTreeMap::new();
            for p in posiciones {
                if let Some(l) = p.latente {
                    let e = por_moneda.entry(p.ticker.as_str()).or_default();
                    e.0 += l;
                    e.1 = e.1.checked_add(p.cantidad).unwrap_or(e.1);
                }
            }
            let mut monedas: Vec<(String, Money)> = por_moneda
                .into_iter()
                .filter(|(_, (l, _))| l.cents() < 0)
                .map(|(t, (l, q))| (format!("{t}: {q} unidades"), -l))
                .collect();
            monedas.sort_by_key(|m| std::cmp::Reverse(m.1));
            let perdidas = monedas.iter().map(|(_, m)| *m).sum::<Money>();
            (ganancias, perdidas, monedas, true)
        }
        _ => (Money::ZERO, Money::ZERO, Vec::new(), false),
    };

    let bin_arrastre = if fisica {
        None
    } else {
        bases_arrastradas(v, año)?
    };
    // Test del art. 5.2 LIS con la media de los balances trimestrales y las partidas marcadas
    // en Contabilidad → Cierre (`patrimonial`): (el test, sin la tesorería no justificada).
    let inversiones = if fisica {
        None
    } else {
        crate::patrimonial::senal(v, año, hoy)
    };
    match inversiones {
        Some((_, seguro)) if seguro > 50 => notas.push(format!(
            "Con la media de los balances trimestrales, el {seguro} % del activo son valores o \
             bienes no afectos a la actividad (fondos, acciones, inmuebles sin actividad, \
             préstamos a socios…): la sociedad sería patrimonial, tributaría al 25 % y no \
             tendría los tipos ni las ventajas de las empresas pequeñas (art. 5.2 de la Ley \
             27/2014). Revisa las partidas en Contabilidad → Cierre → Entidad patrimonial y \
             consúltalo con tu asesor antes de seguir estas recomendaciones."
        )),
        Some((todo, seguro)) if todo > 50 => notas.push(format!(
            "Con la media de los balances trimestrales, el {todo} % del activo son valores, bienes \
             no afectos o tesorería que no has justificado como necesaria para la actividad (sin \
             esa tesorería, el {seguro} %). La ley no dice cómo se trata la tesorería: si no está \
             afecta, la sociedad sería patrimonial y tributaría al 25 % sin las ventajas de las \
             empresas pequeñas (art. 5.2 de la Ley 27/2014). Indica la que necesita la actividad \
             en Contabilidad → Cierre → Entidad patrimonial y revísalo con tu asesor."
        )),
        _ => {}
    }
    let legal = match &resultado_anterior {
        Some((r, _)) => reserva_legal(&diario, *r),
        None => None,
    };
    let ctx = Ctx {
        hoy,
        año,
        cierre: fy.end,
        dias: fy.days(),
        fisica,
        regimen,
        regimen_ajustes,
        erd,
        incn: incn.as_ref().map(|c| c.valor),
        base: base.as_ref().map(|c| c.valor),
        cuota: cuota.as_ref().map(|c| c.valor),
        resultado_anterior,
        dividendos_acordados: movimientos(&diario, "526", Some(false)).1,
        aumento_contabilizado: aumento_contabilizado(&diario),
        reserva_legal: legal,
        bases_pendientes: if ys.pending_losses.cents() > 0 {
            ys.pending_losses
        } else {
            bin_arrastre.map_or(Money::ZERO, |(_, m)| m.max(Money::ZERO))
        },
        bin_indicadas: ys.pending_losses.max(Money::ZERO),
        bin_arrastre,
        capitalizacion_indicada: ys.capitalization_reserve,
        capitalizacion_anterior: ult(1032),
        tipo_presentado: ult(558),
        nivelacion_anterior: ult(1034),
        dividendos_participadas: -saldo(&diario, "760"),
        dividendos_grupo: -(saldo(&diario, "7600")
            + saldo(&diario, "7601")
            + saldo(&diario, "7602")),
        dividendos_otras: -saldo(&diario, "7603"),
        exencion_anterior: [370, 2181]
            .iter()
            .any(|k| casilla(*k).is_some_and(|x| x.cents() > 0)),
        correcciones_disminucion: ys.corporate_decreases,
        trabajadores,
        cnae: cnae.clone(),
        primer_año: primer_año(v)?,
        impagadas: impagadas(v, hoy)?,
        clientes: saldo(&diario, "430").max(Money::ZERO),
        cripto_ganancias,
        cripto_perdidas_latentes,
        cripto_monedas,
        hay_cripto,
        modulos: fisica
            && profile.as_ref().is_some_and(|p| {
                p.irpf_method == Some(taxpain_core::calendar::IrpfMethod::Modulos)
            }),
        inicio_marcado: ys.activity_start,
        gran_empresa: profile.as_ref().is_some_and(|p| p.gran_empresa),
        primer_beneficio: primer_año_con_beneficio(v, año)?,
        inversiones,
        ccaa: profile.as_ref().and_then(|p| p.ccaa),
        prorrata: s.prorata,
        prorrata_especial: s.prorata_special,
        recuperable: recuperable(v, hoy),
    };
    let situacion = Situacion {
        año,
        cierre: fy.end,
        persona_fisica: fisica,
        regimen: (!fisica).then(|| {
            if ctx.incn.is_none() && regimen != TaxRegime::NuevaCreacion {
                format!(
                    "Sin confirmar: falta la cifra de negocios de {} (se calcula con el tipo {})",
                    año - 1,
                    regimen_corto(regimen)
                )
            } else {
                regimen_texto(regimen, erd).to_owned()
            }
        }),
        cifra_negocios: incn,
        base,
        cuota,
        tipo: ctx.tipo(),
        trabajadores,
        cnae,
        notas,
    };
    Ok((ctx, situacion))
}

/// Una ficha evaluada con los datos de la empresa y la marca del usuario.
fn recomendacion(f: &'static Ficha, ctx: &Ctx, marca: Marca) -> Recomendacion {
    let e = (f.evaluar)(ctx);
    Recomendacion {
        id: f.id,
        area: f.area,
        titulo: f.titulo,
        resumen: f.resumen,
        encaje: e.encaje,
        motivo: e.motivo,
        ahorro: e.ahorro.filter(|a| a.cents() > 0),
        aplazado: f.aplazado,
        calculo: e.calculo,
        plazo: e.plazo,
        requisitos: f.requisitos,
        pasos: f.pasos,
        riesgos: f.riesgos,
        norma: f.norma,
        enlace: f.enlace,
        pantalla: f.pantalla,
        marca,
        rebaja: e.rebaja,
        sin_tope: e.sin_tope,
    }
}

/// Las recomendaciones para la empresa abierta, con las marcas del usuario.
pub fn ahorro(v: &Vault, hoy: Date) -> Result<Ahorro> {
    let (ctx, situacion) = contexto(v, hoy)?;
    let marcas = leer_marcas(v)?.remove(&ctx.año).unwrap_or_default();
    let mut recomendaciones: Vec<Recomendacion> = catalogo::FICHAS
        .iter()
        .filter(|f| match f.para {
            Para::Ambos => true,
            Para::Sociedad => !ctx.fisica,
            Para::Autonomo => ctx.fisica,
        })
        .map(|f| recomendacion(f, &ctx, marcas.get(f.id).cloned().unwrap_or_default()))
        .collect();
    recomendaciones.sort_by(|a, b| {
        a.encaje
            .cmp(&b.encaje)
            .then(b.ahorro.cmp(&a.ahorro))
            .then(a.titulo.cmp(b.titulo))
    });
    let abierta = |r: &&Recomendacion| {
        r.encaje == Encaje::Conviene
            && !matches!(r.marca.estado, Estado::Hecho | Estado::Descartado)
    };
    let suma = |aplazado: bool| -> Money {
        recomendaciones
            .iter()
            .filter(abierta)
            .filter(|r| r.aplazado == aplazado)
            .filter_map(|r| r.ahorro)
            .sum()
    };
    // Las rebajas de base juntas no ahorran lo mismo que por separado: cruzan tramos y bajan el
    // tope de la reserva de capitalización. En una sociedad, su ahorro se calcula de una vez, en
    // el orden del 200 (exención y deterioro en la [00550], capitalización con su tope, bases
    // negativas).
    let ahorro_posible = if ctx.fisica {
        suma(false)
    } else {
        let con_rebaja = |r: &&Recomendacion| r.rebaja.is_some() && !r.aplazado;
        let (mut r550, mut cap) = (Money::ZERO, Money::ZERO);
        for r in recomendaciones.iter().filter(abierta).filter(con_rebaja) {
            match r.sin_tope {
                Some(t) => cap += t,
                None => r550 += r.rebaja.unwrap_or(Money::ZERO),
            }
        }
        let resto: Money = recomendaciones
            .iter()
            .filter(abierta)
            .filter(|r| !r.aplazado && r.rebaja.is_none())
            .filter_map(|r| r.ahorro)
            .sum();
        resto + ctx.ahorro_conjunto(r550, cap)
    };
    Ok(Ahorro {
        ahorro_posible,
        aplazado_posible: suma(true),
        situacion,
        recomendaciones,
    })
}

/// La ficha de un incentivo por su id (`None` si no existe).
pub fn ficha(id: &str) -> Option<&'static Ficha> {
    catalogo::FICHAS.iter().find(|f| f.id == id)
}

/// Guarda lo que ha marcado el usuario en una recomendación, para el ejercicio que contiene `hoy`.
pub fn marcar(v: &Vault, hoy: Date, id: &str, marca: &Marca) -> Result<()> {
    let Some(f) = ficha(id) else {
        return Err(crate::error::AppError::Other(format!(
            "recomendación desconocida: {id}"
        )));
    };
    let año = accounting::fiscal_year_at(v, hoy)?.year;
    let mut m = marca.clone();
    m.pasos.retain(|i| *i < f.pasos.len());
    let mut marcas = leer_marcas(v)?;
    let del_año = marcas.entry(año).or_default();
    if m == Marca::default() {
        del_año.remove(id);
    } else {
        del_año.insert(id.to_owned(), m);
    }
    marcas.retain(|_, x| !x.is_empty());
    v.set_setting(MARCAS, &marcas)?;
    Ok(())
}

#[cfg(test)]
mod tests;

//! «Revisión de la carpeta de la empresa»: con lo que hay en la carpeta (facturas, libros,
//! modelos presentados, escrituras…) y lo que TaxPain ya sabe de la empresa, busca lo que no
//! cuadra y lo cuenta como un informe:
//!
//! - **Ficheros**: copias idénticas, formatos que no se leen, lo que falta por leer.
//! - **Facturas**: repetidas, importes que no cuadran, NIF con la letra mal, a nombre de otro,
//!   proveedores extranjeros que cobran IVA español, divisas, trimestre equivocado.
//! - **Emitidas**: saltos y repeticiones en la numeración.
//! - **Libros**: facturas del libro sin PDF y PDF que no están en el libro.
//! - **Modelos**: los que tocaban y no están; el 303 frente a los libros registro y el diario
//!   del trimestre (o, sin libro, frente a las facturas); el 349 frente al 303; el 111 frente a
//!   las retenciones; y cada modelo presentado frente al que calcula TaxPain con sus datos, casilla a
//!   casilla y perceptor a perceptor (ver [`calculado`]).
//! - **Socios**: dividendos que declara el 123 sin el acta que acuerda el reparto.
//! - **Impuesto sobre Sociedades**: dividendos sin la exención del art. 21 LIS, bases negativas
//!   compensadas de más, reserva de nivelación, con una estimación de lo pagado de más.
//! - **Cruces** (segunda opinión, ver [`cruces`]): 390 frente a los 303 y cadena de compensación;
//!   190, 180 y 193 frente a 111, 115 y 123; 347, 349, 190 y 180 por NIF frente a las facturas;
//!   200 frente a los 202, las retenciones y las cuentas depositadas; presentaciones fuera de
//!   plazo con su recargo; depósito de cuentas y legalización de libros; pagos del banco frente a
//!   los modelos; administradores frente a las escrituras y saldos de la 551/552.
//! - **AEAT** (ver [`sede`]): certificados con su validez, requerimientos con sus contestaciones
//!   y lo que suponen para la prescripción, y las devoluciones pedidas frente al banco.
//!
//! Sólo lee: no importa ni cambia nada. Lo que encuentra es orientativo (lo hace un programa con
//! lo que hay en la carpeta) y el informe lo dice; las cifras de impuestos son estimaciones.

use std::ops::Deref;
use std::path::Path;

use serde::{Deserialize, Serialize};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use time::Date;

use crate::error::Result;

mod actas;
mod bancos;
mod calculado;
pub(crate) mod cruces;
mod dividendos;
mod emitidas;
pub(crate) mod expediente;
mod facturas;
mod ficheros;
mod ia;
pub mod informe;
mod iva;
pub(crate) mod modelos;
mod participadas;
mod registro;
mod sede;
mod sociedades;
pub mod urgente;

pub use urgente::Urgente;

pub(crate) use ia::{grupo, tema_hallazgo};
pub(crate) use sociedades::es_dividendo;

/// De qué revisión sale un hallazgo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origen {
    /// «Qué revisar» (Evolución): lo que tiene la AEAT frente a lo que sabe TaxPain.
    Aeat,
    /// La última revisión de la carpeta (Importar → Revisión).
    Carpeta,
}

impl Origen {
    pub fn as_str(self) -> &'static str {
        match self {
            Origen::Aeat => "aeat",
            Origen::Carpeta => "carpeta",
        }
    }
}

/// Importancia de un hallazgo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gravedad {
    /// Algo que está mal y cuesta dinero o tiene riesgo.
    Error,
    /// Algo que hay que revisar.
    Aviso,
    /// Para saberlo.
    Info,
}

/// De qué trata un hallazgo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    Ficheros,
    Facturas,
    Emitidas,
    Libros,
    Modelos,
    Sociedades,
    Bancos,
    /// Plazos: presentaciones fuera de plazo, depósito de cuentas y legalización de libros.
    Plazos,
    /// Lo de la AEAT que no son modelos: certificados, requerimientos y devoluciones.
    Aeat,
    /// Socios y administradores.
    Socios,
}

impl Area {
    pub fn titulo(self) -> &'static str {
        match self {
            Area::Ficheros => "Ficheros",
            Area::Facturas => "Facturas",
            Area::Emitidas => "Facturas emitidas",
            Area::Libros => "Libros registro",
            Area::Modelos => "Modelos presentados",
            Area::Sociedades => "Impuesto sobre Sociedades",
            Area::Bancos => "Bancos",
            Area::Plazos => "Plazos y Registro Mercantil",
            Area::Aeat => "AEAT: certificados, requerimientos y devoluciones",
            Area::Socios => "Socios y administradores",
        }
    }
}

/// Quién tiene que hacer algo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Responsable {
    /// La empresa (ordenar la carpeta, pedir una factura bien hecha…).
    Empresa,
    /// La gestoría: un error en un modelo o en un libro.
    Gestoria,
    /// Falta un documento que hay que conseguir.
    Documentacion,
}

impl Responsable {
    pub fn titulo(self) -> &'static str {
        match self {
            Responsable::Empresa => "Empresa",
            Responsable::Gestoria => "Gestoría",
            Responsable::Documentacion => "Documentación",
        }
    }
}

/// Algo que se ha encontrado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hallazgo {
    /// Código estable de la comprobación (`facturas.repetidas`, `sociedades.exencion_dividendos`…).
    pub codigo: String,
    pub area: Area,
    pub gravedad: Gravedad,
    pub responsable: Responsable,
    pub ejercicio: Option<i32>,
    pub titulo: String,
    pub detalle: String,
    /// Ficheros afectados (rutas relativas a la carpeta).
    pub ficheros: Vec<String>,
    /// Estimación de lo que se recupera si se corrige (positivo) o de lo que se pagaría de más
    /// (negativo).
    pub importe: Option<Money>,
    /// Hasta cuándo se puede rectificar lo del ejercicio (`aaaa-mm-dd`): cuatro años desde el fin
    /// del plazo de presentación (arts. 66 y 67.1 LGT). En el Impuesto sobre Sociedades, desde el
    /// último día del plazo del 200 (el 25 de julio del año siguiente con cierre el 31 de
    /// diciembre, trasladado al siguiente día hábil si cae en sábado, domingo o festivo); en los
    /// demás modelos, el plazo del primer trimestre del ejercicio (20 de abril) cuatro años
    /// después, el primero que prescribe. `None` si no aplica.
    #[serde(default)]
    pub rectificable_hasta: Option<String>,
    /// `rectificable_hasta` es aproximado: TaxPain no tiene los festivos del año en que acababa
    /// el plazo de presentación y es el último día sin trasladar (el de verdad puede ser algún día
    /// después, nunca antes).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rectificable_aprox: bool,
    /// Nombres e identificadores (NIF, NIF-IVA) de los terceros que nombran su título y su
    /// detalle: en una conversación con la IA van con marcadores. `None` en una revisión
    /// guardada por una versión anterior, que no los apuntaba.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terceros: Option<Vec<String>>,
    /// «Urgente: puede haber deuda» con Hacienda o la Seguridad Social, con el recargo de hoy y
    /// el siguiente escalón (ver [`urgente`]). `None` en los demás y en las revisiones
    /// guardadas por versiones anteriores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgente: Option<Urgente>,
}

/// Tabla del informe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tabla {
    pub titulo: String,
    /// Nombre corto para la hoja de Excel (como mucho 31 caracteres).
    #[serde(default)]
    pub hoja: String,
    pub columnas: Vec<String>,
    pub filas: Vec<Vec<String>>,
}

/// A quién hay que pedir algo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Destinatario {
    Gestoria,
    Registro,
    Aeat,
    Empresa,
}

impl Destinatario {
    pub fn titulo(self) -> &'static str {
        match self {
            Destinatario::Gestoria => "A la gestoría",
            Destinatario::Registro => "Al Registro Mercantil",
            Destinatario::Aeat => "En la Sede de la AEAT",
            Destinatario::Empresa => "En la empresa",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peticion {
    pub a_quien: Destinatario,
    pub que: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cuenta {
    pub tipo: String,
    pub n: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resumen {
    /// Ficheros de la carpeta que se han mirado.
    pub ficheros: usize,
    /// Leídos con sus datos (con el asistente, con Claude Code o por su formato).
    pub con_detalle: usize,
    /// Sin datos (habría que leerlos con IA o con Claude Code).
    pub sin_leer: usize,
    /// De un formato que TaxPain no lee (zip, Word, Pages…).
    pub no_admitidos: usize,
    pub por_tipo: Vec<Cuenta>,
    pub errores: usize,
    pub avisos: usize,
    pub info: usize,
    /// Hallazgos urgentes: pueden suponer una deuda con recargo.
    #[serde(default)]
    pub urgentes: usize,
    /// Suma de lo que está en juego en los urgentes (cuotas y recargos de hoy que se saben).
    #[serde(default)]
    pub en_juego_urgente: Money,
    /// Suma de lo que se estima recuperable corrigiendo los modelos.
    pub recuperable: Money,
    /// Lo que se recupera en el próximo 200 (el de un año que aún no se ha presentado) por los
    /// dividendos cobrados que confirman los documentos de las participadas: las retenciones
    /// soportadas menos el impuesto que corresponde a esos dividendos con la exención del 95 %
    /// (art. 21 LIS). No es corrigiendo modelos: va aparte de `recuperable`.
    #[serde(default)]
    pub a_favor_proximo_200: Money,
    /// De lo anterior, las retenciones soportadas.
    #[serde(default)]
    pub retenciones_proximo_200: Money,
    /// Lo que se pierde si no se aplica la exención (estimación): el impuesto que se ahorra con
    /// ella. No se suma a lo anterior.
    #[serde(default)]
    pub exencion_proximo_200: Money,
    /// Impuesto que corresponde a esos dividendos con la exención (estimación).
    #[serde(default)]
    pub impuesto_dividendos_proximo_200: Money,
}

/// Dividendo cobrado de una participada en un año cuyo 200 aún no se ha presentado, con el bruto
/// y la retención que confirman los documentos de la participada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DividendoConfirmado {
    pub ejercicio: i32,
    /// Día del cobro en el banco (`aaaa-mm-dd`).
    pub fecha: String,
    /// La participada que lo paga.
    pub pagadora: String,
    pub neto: Money,
    pub bruto: Money,
    pub retencion: Money,
    /// Los documentos dicen que la empresa tiene al menos el 5 % desde antes del año del cobro:
    /// el 95 % está exento y no se tenía que retener.
    pub grupo: bool,
}

/// Resultado de la revisión.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    /// Fecha de la revisión (AAAA-MM-DD).
    pub generada: String,
    pub empresa: String,
    pub nif: Option<String>,
    pub carpeta: String,
    pub resumen: Resumen,
    pub hallazgos: Vec<Hallazgo>,
    pub tablas: Vec<Tabla>,
    pub pedir: Vec<Peticion>,
    /// Dividendos cobrados para el próximo 200, confirmados con los documentos de las
    /// participadas (Ahorrar impuestos usa su bruto y su retención).
    #[serde(default)]
    pub dividendos: Vec<DividendoConfirmado>,
}

/// Lo que van dejando las comprobaciones.
#[derive(Debug, Default)]
pub(crate) struct Salida {
    pub hallazgos: Vec<Hallazgo>,
    pub tablas: Vec<Tabla>,
    pub pedir: Vec<Peticion>,
    pub dividendos: Vec<DividendoConfirmado>,
    /// Impuesto que se ahorra con la exención de los dividendos confirmados (estimación).
    pub exencion_proximo_200: Money,
    /// Impuesto que corresponde a esos dividendos con la exención (estimación).
    pub impuesto_dividendos_proximo_200: Money,
}

/// Un hallazgo a medio hacer: con [`Nuevo::detalle`] y [`Salida::add`] se completa.
#[derive(Debug)]
pub(crate) struct Nuevo {
    pub codigo: &'static str,
    pub area: Area,
    pub gravedad: Gravedad,
    pub responsable: Responsable,
    pub titulo: String,
    pub ficheros: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct ConDetalle {
    n: Nuevo,
    detalle: String,
}

impl Nuevo {
    pub fn detalle(self, detalle: String) -> ConDetalle {
        ConDetalle { n: self, detalle }
    }
}

impl Salida {
    pub fn add(&mut self, h: ConDetalle, ejercicio: Option<i32>, importe: Option<Money>) {
        let ConDetalle { n, detalle } = h;
        self.hallazgos.push(Hallazgo {
            codigo: n.codigo.to_owned(),
            area: n.area,
            gravedad: n.gravedad,
            responsable: n.responsable,
            ejercicio,
            titulo: n.titulo,
            detalle,
            ficheros: n.ficheros,
            importe,
            rectificable_hasta: None,
            rectificable_aprox: false,
            terceros: None,
            urgente: None,
        });
    }

    /// Marca como urgente el último hallazgo añadido.
    pub fn urgente(&mut self, u: Urgente) {
        if let Some(h) = self.hallazgos.last_mut() {
            h.urgente = Some(u);
        }
    }

    pub fn tabla(
        &mut self,
        titulo: impl Into<String>,
        hoja: impl Into<String>,
        columnas: &[&str],
        filas: Vec<Vec<String>>,
    ) {
        if filas.is_empty() {
            return;
        }
        self.tablas.push(Tabla {
            titulo: titulo.into(),
            hoja: hoja.into(),
            columnas: columnas.iter().map(|c| (*c).to_owned()).collect(),
            filas,
        });
    }

    pub fn pedir(&mut self, a_quien: Destinatario, que: impl Into<String>) {
        let que = que.into();
        if !self
            .pedir
            .iter()
            .any(|p| p.a_quien == a_quien && p.que == que)
        {
            self.pedir.push(Peticion { a_quien, que });
        }
    }
}

/// Ajuste con la última revisión de la empresa, para enseñarla al volver a la pestaña.
const ULTIMA: &str = "revision_carpeta";

/// Guarda la revisión como la última de la empresa.
pub fn guardar_ultima(v: &Vault, r: &Revision) -> Result<()> {
    v.set_setting(ULTIMA, r)?;
    Ok(())
}

/// La última revisión guardada (ninguna si no hay o si es de una versión que ya no se lee).
pub fn ultima(v: &Vault) -> Option<Revision> {
    v.setting(ULTIMA).ok().flatten()
}

/// Revisa la carpeta. No cambia nada (salvo cargar `taxpain-lecturas.json`, como al leerla).
pub fn revisar(v: &Vault, root: &Path, hoy: Date) -> Result<Revision> {
    revisar_with(root, || Ok(v), hoy)
}

/// Como [`revisar`], cogiendo el almacén con `vault` sólo para lo que lo necesita (en la app,
/// su cerrojo): lo lento se hace sin él.
pub fn revisar_with<G: Deref<Target = Vault>>(
    root: &Path,
    vault: impl Fn() -> Result<G>,
    hoy: Date,
) -> Result<Revision> {
    let inventario = crate::carpeta::scan_with(root, &vault)?;
    let exp = expediente::construir(root, &inventario, &vault, hoy)?;
    let mut s = Salida::default();
    ficheros::incompleta(inventario.over_limit, inventario.too_deep, &mut s);
    ficheros::revisar(&exp, &mut s);
    facturas::revisar(&exp, &mut s);
    emitidas::revisar(&exp, &mut s);
    registro::revisar(&exp, &mut s);
    modelos::revisar(&exp, hoy, &mut s);
    calculado::revisar(&exp, &mut s);
    sociedades::revisar(&exp, hoy, &mut s);
    dividendos::revisar(&exp, &mut s);
    bancos::revisar(&exp, hoy, &mut s);
    cruces::revisar(&exp, hoy, &mut s);
    sede::revisar(&exp, hoy, &mut s);
    Ok(cerrar(&exp, s, hoy))
}

/// La revisión de la que salen los hallazgos de `origen`, con los datos de ahora: la de la AEAT
/// se rehace; la de la carpeta es la última guardada (rehacerla necesita la carpeta y tarda).
pub(crate) fn de_origen(v: &Vault, hoy: Date, origen: Origen) -> Result<Option<Revision>> {
    match origen {
        Origen::Aeat => revisar_aeat(v, hoy).map(Some),
        Origen::Carpeta => Ok(ultima(v)),
    }
}

/// Revisa lo que tiene la AEAT de la empresa («Comprobar lo presentado») con lo que sabe
/// TaxPain, sin carpeta: modelos que faltan, cruces entre modelos (390 y 303, resúmenes anuales,
/// 349, 200 y 202…), plazos y el Impuesto sobre Sociedades. Lo que sale de las facturas y los
/// libros de la carpeta lo añade la revisión de la carpeta.
pub fn revisar_aeat(v: &Vault, hoy: Date) -> Result<Revision> {
    let exp = expediente::sin_carpeta(v, hoy)?;
    let mut s = Salida::default();
    modelos::revisar_sin_carpeta(&exp, hoy, &mut s);
    calculado::revisar(&exp, &mut s);
    sociedades::revisar(&exp, hoy, &mut s);
    cruces::revisar_sin_carpeta(&exp, hoy, &mut s);
    Ok(cerrar(&exp, s, hoy))
}

/// Hasta cuándo se puede rectificar lo de un ejercicio, según el área del hallazgo, y si es
/// aproximado (ver [`Hallazgo::rectificable_aprox`]).
fn rectificable_hasta(
    plazos: &sociedades::Plazos200,
    area: Area,
    ejercicio: Option<i32>,
) -> Option<(String, bool)> {
    let y = ejercicio?;
    match area {
        Area::Sociedades => plazos.rectificable(y).map(|d| (d.iso(), !d.exacto)),
        Area::Modelos | Area::Plazos => Some((format!("{:04}-04-20", y + 4), false)),
        _ => None,
    }
}

/// Hallazgos que no se arreglan rectificando un modelo y que, por eso, no llevan el plazo de
/// rectificación: la reserva de nivelación se arregla dotándola en la junta, y los dividendos
/// cobrados van en un 200 que aún no se ha presentado (si ya tenía que estarlo, el hallazgo trae
/// su plazo).
/// Hallazgos cuyo importe es para el próximo 200 (el de un año que aún no se ha presentado), no
/// lo que se recupera corrigiendo modelos: van en `a_favor_proximo_200`, no en `recuperable`.
pub(crate) const PROXIMO_200: &[&str] = &[
    "sociedades.retencion_dividendos_grupo",
    "sociedades.dividendos_confirmados",
];

const SIN_RECTIFICAR: &[&str] = &[
    "sociedades.reserva_nivelacion",
    "sociedades.reserva_nivelacion_sin_dotar",
    "sociedades.dividendos_cobrados",
    "sociedades.retencion_dividendos_grupo",
    "sociedades.dividendos_confirmados",
];

/// Nombres e identificadores de los terceros que conoce la revisión: los de las facturas, los
/// libros registro y los resúmenes anuales (de la carpeta o de la AEAT), los administradores de
/// Nóminas, los saldos con socios y lo que dicen las escrituras.
fn terceros_conocidos(exp: &expediente::Expediente) -> Vec<String> {
    fn escritura(f: &crate::documentos::Facts, add: &mut impl FnMut(Option<&str>)) {
        for x in [&f.name, &f.nif, &f.vat_number, &f.administrator, &f.notary] {
            add(x.as_deref());
        }
    }
    let mut out = std::collections::BTreeSet::new();
    // Un nombre de una sola palabra y sin cifras («Ventas», «Varios») suele ser genérico, y no es
    // el de una persona: taparlo estropearía el texto.
    let mut add = |s: Option<&str>| {
        if let Some(s) = s
            .map(str::trim)
            .filter(|s| s.chars().any(|c| c.is_ascii_digit()) || s.split_whitespace().count() >= 2)
        {
            out.insert(s.to_owned());
        }
    };
    for d in exp.docs.iter().chain(&exp.aeat) {
        if let Some(f) = &d.factura {
            for x in [
                &f.emisor_nif,
                &f.emisor_nombre,
                &f.destinatario_nif,
                &f.destinatario_nombre,
            ] {
                add(x.as_deref());
            }
        }
        for r in d.modelo.iter().flat_map(|m| &m.registros) {
            add(r.nif.as_deref());
            add(r.nombre.as_deref());
        }
        for i in d
            .libro
            .iter()
            .flat_map(|b| &b.invoices)
            .chain(&d.autorrepercusion)
        {
            add(i.nif.as_deref());
            add(Some(&i.name));
        }
        if let Some((_, f)) = &d.empresa {
            escritura(f, &mut add);
        }
    }
    for a in &exp.taxpain.administradores {
        add(Some(&a.nombre));
        add(Some(&a.nif));
    }
    for d in &exp.taxpain.documentos {
        escritura(&d.facts, &mut add);
    }
    for s in &exp.taxpain.saldos_socios {
        add(Some(&s.nombre));
    }
    out.into_iter().collect()
}

/// ¿Es una revisión guardada por una versión anterior, sin los terceros de sus hallazgos?
pub(crate) fn sin_terceros(r: &Revision) -> bool {
    r.hallazgos.iter().any(|h| h.terceros.is_none())
}

fn cerrar(exp: &expediente::Expediente, mut s: Salida, hoy: Date) -> Revision {
    let conocidos = terceros_conocidos(exp);
    let plazos = sociedades::Plazos200::de(exp);
    for h in &mut s.hallazgos {
        if h.rectificable_hasta.is_none()
            && !SIN_RECTIFICAR.contains(&h.codigo.as_str())
            && let Some((hasta, aprox)) = rectificable_hasta(&plazos, h.area, h.ejercicio)
        {
            h.rectificable_hasta = Some(hasta);
            h.rectificable_aprox = aprox;
        }
        let texto = format!("{}\n{}", h.titulo, h.detalle);
        h.terceros = Some(
            taxpain_assistant::pii::mentioned(&texto, &conocidos)
                .into_iter()
                .cloned()
                .collect(),
        );
    }
    urgente::ordenar(&mut s.hallazgos);
    s.pedir.sort_by_key(|p| p.a_quien);
    let contar = |g: Gravedad| s.hallazgos.iter().filter(|h| h.gravedad == g).count();
    let recuperable = s
        .hallazgos
        .iter()
        .filter(|h| h.area == Area::Sociedades || h.area == Area::Modelos)
        .filter(|h| !PROXIMO_200.contains(&h.codigo.as_str()))
        .filter_map(|h| h.importe)
        .fold(Money::ZERO, |a, m| a + m)
        .max(Money::ZERO);
    let retenciones_proximo_200 = s
        .dividendos
        .iter()
        .fold(Money::ZERO, |a, d| a + d.retencion);
    let resumen = Resumen {
        ficheros: exp.docs.len() + exp.no_admitidos.len(),
        con_detalle: exp.docs.iter().filter(|d| d.con_detalle()).count(),
        sin_leer: exp.docs.iter().filter(|d| !d.con_detalle()).count(),
        no_admitidos: exp.no_admitidos.len(),
        por_tipo: exp.por_tipo(),
        errores: contar(Gravedad::Error),
        avisos: contar(Gravedad::Aviso),
        info: contar(Gravedad::Info),
        urgentes: s.hallazgos.iter().filter(|h| h.urgente.is_some()).count(),
        en_juego_urgente: s
            .hallazgos
            .iter()
            .filter_map(|h| h.urgente.as_ref())
            .fold(Money::ZERO, |a, u| a + u.total()),
        recuperable,
        a_favor_proximo_200: (retenciones_proximo_200 - s.impuesto_dividendos_proximo_200)
            .max(Money::ZERO),
        retenciones_proximo_200,
        exencion_proximo_200: s.exencion_proximo_200,
        impuesto_dividendos_proximo_200: s.impuesto_dividendos_proximo_200,
    };
    Revision {
        generada: format!(
            "{:04}-{:02}-{:02}",
            hoy.year(),
            u8::from(hoy.month()),
            hoy.day()
        ),
        empresa: exp.empresa.clone(),
        nif: exp.own_nif.clone(),
        carpeta: exp.carpeta.clone(),
        resumen,
        hallazgos: s.hallazgos,
        tablas: s.tablas,
        pedir: s.pedir,
        dividendos: s.dividendos,
    }
}

// ---------- Utilidades comunes ----------

/// Importe en el formato español (`1.234,56 €`).
pub(crate) fn eur(m: Money) -> String {
    m.to_string()
}

/// Margen para dar dos importes por iguales (redondeos, céntimos de más o de menos).
pub(crate) const UN_EURO: Money = Money::from_euros(1, 0);

/// Importe con su moneda: «12,00 €» o, en otra moneda, «12,00 USD».
pub(crate) fn importe(m: Money, moneda: Option<&str>) -> String {
    match moneda {
        Some(c) if c != "EUR" => eur(m).replace(" €", &format!(" {c}")),
        _ => eur(m),
    }
}

/// Junta las letras con su tilde cuando llegan separadas: los nombres de fichero del Mac vienen
/// descompuestos (NFD: «N» + «◌̃»), y el PDF los escribiría como «N?». Sólo las letras del
/// español y de las lenguas cooficiales; lo demás queda igual.
pub(crate) fn componer(s: &str) -> String {
    const TABLA: [(char, char, char); 28] = [
        ('a', '\u{301}', 'á'),
        ('e', '\u{301}', 'é'),
        ('i', '\u{301}', 'í'),
        ('o', '\u{301}', 'ó'),
        ('u', '\u{301}', 'ú'),
        ('A', '\u{301}', 'Á'),
        ('E', '\u{301}', 'É'),
        ('I', '\u{301}', 'Í'),
        ('O', '\u{301}', 'Ó'),
        ('U', '\u{301}', 'Ú'),
        ('a', '\u{300}', 'à'),
        ('e', '\u{300}', 'è'),
        ('i', '\u{300}', 'ì'),
        ('o', '\u{300}', 'ò'),
        ('u', '\u{300}', 'ù'),
        ('A', '\u{300}', 'À'),
        ('E', '\u{300}', 'È'),
        ('O', '\u{300}', 'Ò'),
        ('i', '\u{308}', 'ï'),
        ('u', '\u{308}', 'ü'),
        ('I', '\u{308}', 'Ï'),
        ('U', '\u{308}', 'Ü'),
        ('n', '\u{303}', 'ñ'),
        ('N', '\u{303}', 'Ñ'),
        ('c', '\u{327}', 'ç'),
        ('C', '\u{327}', 'Ç'),
        ('l', '\u{b7}', 'ŀ'),
        ('L', '\u{b7}', 'Ŀ'),
    ];
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        let junta = it
            .peek()
            .and_then(|m| TABLA.iter().find(|(b, t, _)| *b == c && t == m));
        match junta {
            Some((_, _, j)) => {
                out.push(*j);
                it.next();
            }
            None => out.push(c),
        }
    }
    out
}

/// Primer ejercicio que Hacienda aún puede revisar (y la empresa rectificar): los cuatro
/// últimos y el que está en curso (art. 66 LGT).
pub(crate) fn revisable_desde(hoy: Date) -> i32 {
    hoy.year() - 4
}

/// Fecha `dd/mm/aaaa`.
pub(crate) fn fecha(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

/// Trimestre (1–4) de una fecha.
pub(crate) fn trimestre(d: Date) -> u8 {
    (u8::from(d.month()) - 1) / 3 + 1
}

/// Lista de ficheros para un detalle: como mucho `n`, con «y N más».
pub(crate) fn lista(ficheros: &[String], n: usize) -> String {
    if ficheros.len() <= n {
        return ficheros.join(", ");
    }
    format!("{} y {} más", ficheros[..n].join(", "), ficheros.len() - n)
}

/// «1 hallazgo» / «3 hallazgos».
pub(crate) fn n_hallazgos(n: usize) -> String {
    if n == 1 {
        "1 hallazgo".to_owned()
    } else {
        format!("{n} hallazgos")
    }
}

/// «1 factura» / «3 facturas».
pub(crate) fn plural(n: usize, uno: &str, varios: &str) -> String {
    if n == 1 {
        format!("1 {uno}")
    } else {
        format!("{n} {varios}")
    }
}

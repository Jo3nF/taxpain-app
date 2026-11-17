//! El IVA de los libros registro de la carpeta por trimestre, para cruzarlo con el 303: las
//! facturas recibidas por su destino en el modelo (interiores, adquisiciones intracomunitarias,
//! inversión del sujeto pasivo, IVA no deducible) y las emitidas sin la autorrepercusión de las
//! compras, que algunos programas de las gestorías ponen también en el libro de emitidas.

use std::collections::{BTreeMap, BTreeSet};

use taxpain_core::money::{Money, Rate};
use time::{Date, Month};

use super::expediente::{Doc, Expediente, Factura, nif_clave, nif_español};
use super::trimestre;
use crate::libros::{Book, BookInvoice, BookKind, number_key};

/// Dónde va una factura del libro de recibidas en el 303.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Destino {
    /// Operaciones interiores con IVA deducible: [28]+[30] y sus cuotas.
    Interior,
    /// IVA que cobra un proveedor de fuera de España: no debería cobrarlo (es inversión del
    /// sujeto pasivo) y no se puede deducir; unas gestorías lo deducen igual y otras no.
    Extranjero,
    /// Adquisición intracomunitaria con la cuota autorrepercutida: [10] (y [36] al deducirla).
    Intracomunitaria,
    /// Inversión del sujeto pasivo (servicios de fuera de la UE o compras del art. 84.Uno.2.º
    /// LIVA): [12] y, al deducirla, en [28].
    SujetoPasivo,
    /// Con IVA y en la hoja de IVA no deducible del libro (las autorrepercutidas de esa hoja van
    /// a la [10] o a la [12]: la cuota se devenga aunque no se deduzca).
    NoDeducible,
    /// Sin cuota de IVA (exenta, no sujeta o al 0 %).
    SinIva,
}

/// País de otro Estado miembro de la UE (para los bienes cuenta Irlanda del Norte).
fn de_la_ue(country: Option<&str>) -> bool {
    country.is_some_and(|c| taxpain_core::eu::is_other_member_state(c, true))
}

/// El destino en el 303 de una factura del libro de recibidas. El total del libro sin la cuota
/// marca la autorrepercusión: adquisición intracomunitaria si el proveedor es de otro país de la
/// UE e inversión del sujeto pasivo si no.
pub(crate) fn destino(i: &BookInvoice) -> Destino {
    if i.vat().is_zero() {
        return Destino::SinIva;
    }
    if i.vat_in_total == Some(false) {
        return if de_la_ue(i.country.as_deref()) {
            Destino::Intracomunitaria
        } else {
            Destino::SujetoPasivo
        };
    }
    if i.deductible_pct == 0 {
        return Destino::NoDeducible;
    }
    let de_fuera =
        i.country.is_some() || i.nif.as_deref().is_some_and(|n| nif_español(n).is_none());
    if de_fuera {
        Destino::Extranjero
    } else {
        Destino::Interior
    }
}

/// Bases y cuotas de unas facturas del libro.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Suma {
    pub base: Money,
    pub cuota: Money,
    pub n: usize,
}

impl Suma {
    /// Suma la factura con el porcentaje deducible del libro (100 casi siempre).
    fn deducible(&mut self, i: &BookInvoice) {
        let pct = i.deductible_pct.min(100);
        if pct == 100 {
            self.entera(i);
            return;
        }
        let r = Rate::percent(i32::from(pct));
        self.base += i.base().apply_rate(r);
        self.cuota += i.vat().apply_rate(r);
        self.n += 1;
    }

    fn entera(&mut self, i: &BookInvoice) {
        self.base += i.base();
        self.cuota += i.vat();
        self.n += 1;
    }
}

/// Las recibidas de un trimestre por su destino en el 303.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Recibidas {
    pub interior: Suma,
    pub extranjero: Suma,
    /// Adquisiciones intracomunitarias ([10]), se deduzcan o no.
    pub intra: Suma,
    /// Inversión del sujeto pasivo ([12]), se deduzca o no.
    pub isp: Suma,
    /// La parte de `isp` que se deduce (en la [28]).
    pub isp_deducible: Suma,
    /// Las de la hoja de IVA no deducible (también las autorrepercutidas).
    pub no_deducible: Suma,
}

/// Trimestres de `desde` a `hasta`, los dos incluidos.
fn trimestres(desde: Date, hasta: Date) -> Vec<(i32, u8)> {
    let mut out = Vec::new();
    let (mut y, mut q) = (desde.year(), trimestre(desde));
    let fin = (hasta.year(), trimestre(hasta));
    while (y, q) <= fin {
        out.push((y, q));
        if q == 4 {
            y += 1;
            q = 1;
        } else {
            q += 1;
        }
    }
    out
}

/// Primer y último día de un trimestre.
pub(crate) fn limites(y: i32, q: u8) -> Option<(Date, Date)> {
    let mes = |m: u8| Month::try_from(m).ok();
    let ultimo = mes(q.checked_mul(3)?)?;
    let desde = Date::from_calendar_date(y, mes(q * 3 - 2)?, 1).ok()?;
    let hasta = Date::from_calendar_date(y, ultimo, ultimo.length(y)).ok()?;
    Some((desde, hasta))
}

/// El periodo que cubre un libro: del primer día del trimestre de su primera factura (por la
/// fecha de la factura o la de registro, la que sea antes) al último del de la última.
pub(crate) fn periodo_del_libro(invoices: &[BookInvoice]) -> Option<(Date, Date)> {
    let desde = invoices
        .iter()
        .map(|i| i.issue_date.min(i.registered))
        .min()?;
    let hasta = invoices
        .iter()
        .map(|i| i.issue_date.max(i.registered))
        .max()?;
    Some((
        limites(desde.year(), trimestre(desde))?.0,
        limites(hasta.year(), trimestre(hasta))?.1,
    ))
}

/// El periodo propio de un libro, del primer día del trimestre de su primera factura al último
/// del de la última: en recibidas por la fecha de registro (la de la deducción), en emitidas por
/// la de la factura. Es el de las facturas de la carpeta que tendrían que estar en él.
pub(crate) fn periodo_propio(libro: &Book) -> Option<(Date, Date)> {
    let fecha = |i: &BookInvoice| {
        if libro.kind == BookKind::Recibidas {
            i.registered
        } else {
            i.issue_date
        }
    };
    let desde = libro.invoices.iter().map(fecha).min()?;
    let hasta = libro.invoices.iter().map(fecha).max()?;
    Some((
        limites(desde.year(), trimestre(desde))?.0,
        limites(hasta.year(), trimestre(hasta))?.1,
    ))
}

/// Del mismo tercero: el mismo NIF si los dos lo tienen; si no, el mismo nombre.
pub(crate) fn mismo_tercero(a: &BookInvoice, b: &BookInvoice) -> bool {
    match (a.nif.as_deref(), b.nif.as_deref()) {
        (Some(x), Some(y)) => nif_clave(x) == nif_clave(y),
        _ => crate::import::norm(&a.name) == crate::import::norm(&b.name),
    }
}

/// Clave de una factura del libro para no contarla dos veces si está en dos libros (o en dos
/// copias del mismo).
fn clave(i: &BookInvoice) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        number_key(&i.number),
        i.nif
            .as_deref()
            .map_or_else(|| crate::import::norm(&i.name), nif_clave),
        i.issue_date,
        i.base().cents(),
        i.vat().cents()
    )
}

/// Línea del libro de emitidas que es la autorrepercusión de una compra: está también en el
/// libro de recibidas, con el mismo número, el mismo proveedor y la misma base y cuota, y allí
/// va autorrepercutida (el total sin la cuota). Una venta y una compra al mismo tercero con el
/// mismo número (dos empresas que se facturan con la misma numeración) no lo son.
pub(crate) fn es_autorrepercusion(i: &BookInvoice, recibidas: &[&BookInvoice]) -> bool {
    let n = number_key(&i.number);
    let igual = |a: Money, b: Money| (a - b).abs() <= Money::from_cents(1);
    !n.is_empty()
        && recibidas.iter().any(|r| {
            r.vat_in_total == Some(false)
                && r.issue_date.year() == i.issue_date.year()
                && number_key(&r.number) == n
                && igual(r.base(), i.base())
                && igual(r.vat(), i.vat())
                && mismo_tercero(i, r)
        })
}

/// Las facturas de todos los libros de recibidas de la carpeta.
pub(crate) fn recibidas_de(exp: &Expediente) -> Vec<&BookInvoice> {
    exp.docs
        .iter()
        .filter_map(|d| d.libro.as_ref())
        .filter(|l| l.kind == BookKind::Recibidas)
        .flat_map(|l| &l.invoices)
        .collect()
}

/// El destino de una factura del libro de recibidas con lo que dice su PDF: una autorrepercutida
/// que el libro da por intracomunitaria (por el NIF-IVA de la UE que tiene el proveedor) es
/// inversión del sujeto pasivo de la [12] si su factura es de una empresa de fuera de la UE.
pub(crate) fn destino_con_pdf(i: &BookInvoice, pdf: Option<&Factura>) -> Destino {
    match destino(i) {
        Destino::Intracomunitaria if pdf.is_some_and(Factura::emisor_de_fuera_de_la_ue) => {
            Destino::SujetoPasivo
        }
        d => d,
    }
}

/// Una factura del libro de recibidas, con su destino en el 303 y el trimestre de la carpeta de
/// su PDF.
#[derive(Clone, Copy, Debug)]
struct Recibida<'a> {
    i: &'a BookInvoice,
    destino: Destino,
    /// El trimestre de la carpeta de su PDF («…/2026/T2/…») si es posterior al de la factura y
    /// el libro no trae otra fecha de registro: la gestoría la registró en ese trimestre.
    carpeta: Option<(i32, u8)>,
    /// Su retención, si está en algún libro que trae retenciones (aunque sea otra copia que la
    /// del libro del que se toma la factura); en los demás no se sabe si tienen esa columna.
    retencion: Option<Money>,
}

impl Recibida<'_> {
    /// El trimestre en que se dedujo por el libro (la fecha de registro, art. 99 LIVA).
    fn registro(&self) -> (i32, u8) {
        (self.i.registered.year(), trimestre(self.i.registered))
    }

    /// El trimestre en que se dedujo contando la carpeta de su PDF.
    fn por_carpeta(&self) -> (i32, u8) {
        self.carpeta.unwrap_or_else(|| self.registro())
    }
}

/// Cómo se cuentan las recibidas del libro en un trimestre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Segun {
    /// Con el destino que dice el PDF de cada una (si no, el que les da el libro).
    pub pdf: bool,
    /// En el trimestre de la carpeta de su PDF (si no, en el de su registro).
    pub carpeta: bool,
}

/// Una factura del libro que se cuenta en el trimestre de la carpeta de su PDF.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Movida {
    pub numero: String,
    pub nombre: String,
    pub fecha: Date,
    /// El trimestre de su registro en el libro y el de la carpeta.
    pub de: (i32, u8),
    pub a: (i32, u8),
}

/// Los libros registro de la carpeta: sus facturas, sin repetir, y los trimestres que cubre cada
/// uno (del primero al último con alguna factura).
#[derive(Debug, Default)]
pub(crate) struct Libros<'a> {
    recibidas: Vec<Recibida<'a>>,
    /// Sin la autorrepercusión de las compras.
    emitidas: Vec<&'a BookInvoice>,
    cubre_r: BTreeMap<(i32, u8), BTreeSet<String>>,
    cubre_e: BTreeMap<(i32, u8), BTreeSet<String>>,
    /// Los libros de recibidas con alguna retención: los que tienen esa columna (en los demás no
    /// se sabe si la tienen).
    con_retencion: BTreeSet<String>,
}

impl<'a> Libros<'a> {
    pub fn de(exp: &'a Expediente) -> Self {
        let mut out = Libros::default();
        let casados = super::registro::casados(exp);
        // Cada factura, con su posición en `recibidas` la primera vez que sale.
        let mut vistas: BTreeMap<String, usize> = BTreeMap::new();
        for c in casados
            .iter()
            .filter(|c| c.libro.kind == BookKind::Recibidas)
        {
            cubrir(
                &mut out.cubre_r,
                c.doc,
                c.lineas.iter().map(|i| i.registered),
            );
            let con_retencion = c.lineas.iter().any(|i| !i.withholding.is_zero());
            if con_retencion {
                out.con_retencion.insert(c.doc.ruta.clone());
            }
            for (n, i) in c.lineas.iter().enumerate() {
                let retencion = con_retencion.then_some(i.withholding);
                if let Some(&p) = vistas.get(&clave(i)) {
                    // Repetida en otro libro: si el primero no traía la retención, vale la de este.
                    if let Some(r) = out.recibidas.get_mut(p)
                        && r.retencion.is_none()
                    {
                        r.retencion = retencion;
                    }
                    continue;
                }
                vistas.insert(clave(i), out.recibidas.len());
                let pdf = c.pdf(n);
                out.recibidas.push(Recibida {
                    i,
                    destino: destino_con_pdf(i, pdf.map(|p| p.1)),
                    carpeta: pdf.and_then(|p| carpeta(i, p.0)),
                    retencion,
                });
            }
        }
        let mut vistas = BTreeSet::new();
        for c in casados
            .iter()
            .filter(|c| c.libro.kind == BookKind::Emitidas)
        {
            cubrir(
                &mut out.cubre_e,
                c.doc,
                c.libro.invoices.iter().map(|i| i.issue_date),
            );
            for i in &c.lineas {
                if vistas.insert(clave(i)) {
                    out.emitidas.push(i);
                }
            }
        }
        out
    }

    /// Ficheros de los libros de recibidas que cubren el trimestre (vacío si ninguno).
    pub fn de_recibidas(&self, y: i32, q: u8) -> Vec<String> {
        self.cubre_r
            .get(&(y, q))
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Retenciones de profesionales (las del 111; sin las de alquileres, al 19 %) de las
    /// recibidas registradas en el trimestre, si algún libro de recibidas que lo cubre trae
    /// retenciones. Si ninguno trae, `None`: puede que no tengan esa columna. También `None` si
    /// alguna factura del trimestre sólo está en libros sin retenciones: la suya no se sabe.
    pub fn retenciones(&self, y: i32, q: u8) -> Option<Money> {
        let cubre = self.cubre_r.get(&(y, q))?;
        if cubre.is_disjoint(&self.con_retencion) {
            return None;
        }
        let mut total = Money::ZERO;
        for r in self.recibidas.iter().filter(|r| r.registro() == (y, q)) {
            let retencion = r.retencion?;
            if !super::modelos::es_de_alquiler(r.i.base(), retencion) {
                total += retencion;
            }
        }
        Some(total)
    }

    /// Ficheros de los libros de emitidas que cubren el trimestre (vacío si ninguno).
    pub fn de_emitidas(&self, y: i32, q: u8) -> Vec<String> {
        self.cubre_e
            .get(&(y, q))
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Las recibidas registradas en el trimestre (la fecha de registro decide el periodo de la
    /// deducción, art. 99 LIVA), por el destino que dice el PDF de cada una.
    pub fn recibidas(&self, y: i32, q: u8) -> Recibidas {
        self.recibidas_segun(
            y,
            q,
            Segun {
                pdf: true,
                carpeta: false,
            },
        )
    }

    /// Las recibidas del trimestre contadas de otra manera: con el destino que les da el libro (no
    /// el de su PDF) o en el trimestre de la carpeta de su PDF (cuando el libro no trae la fecha
    /// de registro).
    pub fn recibidas_segun(&self, y: i32, q: u8, segun: Segun) -> Recibidas {
        self.sumar(segun.pdf, |r| {
            let t = if segun.carpeta {
                r.por_carpeta()
            } else {
                r.registro()
            };
            t == (y, q)
        })
    }

    /// ¿Hay en el trimestre alguna factura a la que su PDF le cambia el destino que le da el libro?
    pub fn cambia_por_pdf(&self, y: i32, q: u8) -> bool {
        self.recibidas.iter().any(|r| {
            (r.registro() == (y, q) || r.por_carpeta() == (y, q)) && r.destino != destino(r.i)
        })
    }

    /// Las facturas que entran en el trimestre o salen de él al contarlas por la carpeta.
    pub fn movidas(&self, y: i32, q: u8) -> Vec<Movida> {
        self.recibidas
            .iter()
            .filter(|r| {
                r.carpeta.is_some() && (r.registro() == (y, q) || r.por_carpeta() == (y, q))
            })
            .map(|r| Movida {
                numero: r.i.number.clone(),
                nombre: r.i.name.clone(),
                fecha: r.i.issue_date,
                de: r.registro(),
                a: r.por_carpeta(),
            })
            .collect()
    }

    fn sumar(&self, por_pdf: bool, en: impl Fn(&Recibida) -> bool) -> Recibidas {
        let mut r = Recibidas::default();
        for x in self.recibidas.iter().filter(|x| en(x)) {
            let i = x.i;
            let deduce = i.deductible_pct > 0;
            let destino = if por_pdf { x.destino } else { destino(i) };
            match destino {
                Destino::Interior => r.interior.deducible(i),
                Destino::Extranjero => r.extranjero.deducible(i),
                Destino::Intracomunitaria => r.intra.entera(i),
                Destino::SujetoPasivo => {
                    r.isp.entera(i);
                    if deduce {
                        r.isp_deducible.deducible(i);
                    }
                }
                Destino::NoDeducible => r.no_deducible.entera(i),
                Destino::SinIva => {}
            }
            if !deduce && matches!(destino, Destino::Intracomunitaria | Destino::SujetoPasivo) {
                r.no_deducible.entera(i);
            }
        }
        r
    }

    /// Las emitidas con IVA del trimestre (por la fecha de la factura), sin la autorrepercusión.
    pub fn emitidas(&self, y: i32, q: u8) -> Suma {
        let mut s = Suma::default();
        for i in self.emitidas.iter().filter(|i| {
            i.issue_date.year() == y && trimestre(i.issue_date) == q && !i.vat().is_zero()
        }) {
            s.entera(i);
        }
        s
    }
}

/// El trimestre de la carpeta de un PDF si es posterior al de la factura del libro y el libro no
/// trae la fecha de registro (es la de la factura).
fn carpeta(i: &BookInvoice, d: &Doc) -> Option<(i32, u8)> {
    if i.registered != i.issue_date {
        return None;
    }
    let t = (d.ruta_año?, d.ruta_trimestre?);
    (t > (i.registered.year(), trimestre(i.registered))).then_some(t)
}

fn cubrir(
    cubre: &mut BTreeMap<(i32, u8), BTreeSet<String>>,
    d: &Doc,
    fechas: impl Iterator<Item = Date>,
) {
    let fechas: Vec<Date> = fechas.collect();
    let (Some(desde), Some(hasta)) = (fechas.iter().min(), fechas.iter().max()) else {
        return;
    };
    for t in trimestres(*desde, *hasta) {
        cubre.entry(t).or_default().insert(d.ruta.clone());
    }
}

#[cfg(test)]
mod tests {
    use time::macros::date;

    use super::*;
    use crate::libros::BookLine;

    fn factura(numero: &str, nif: Option<&str>, base: i64, cuota: i64, total: i64) -> BookInvoice {
        let (nif, country) = crate::libros::normalize_nif(nif.unwrap_or_default(), None);
        BookInvoice {
            number: numero.into(),
            issue_date: date!(2025 - 05 - 10),
            registered: date!(2025 - 05 - 10),
            nif,
            country,
            name: "Proveedor Demo".into(),
            lines: vec![BookLine {
                base: Money::from_cents(base),
                rate_bp: 2100,
                vat: Money::from_cents(cuota),
                surcharge: Money::ZERO,
            }],
            withholding: Money::ZERO,
            total: Money::from_cents(total),
            vat_in_total: Some(total != base),
            deductible_pct: 100,
            self_assessed: false,
        }
    }

    #[test]
    fn destino_de_cada_factura_del_libro() {
        let interior = factura("A-1", Some("B12345674"), 10_000, 2_100, 12_100);
        assert_eq!(destino(&interior), Destino::Interior);
        // Total sin la cuota: autorrepercutida, de la UE o de fuera.
        let ue = factura("F-1", Some("FR12345678901"), 10_000, 2_100, 10_000);
        assert_eq!(destino(&ue), Destino::Intracomunitaria);
        let isp = factura("US-1", Some("EU372000041"), 10_000, 2_100, 10_000);
        assert_eq!(destino(&isp), Destino::SujetoPasivo);
        let sin_nif = factura("N-1", None, 10_000, 2_100, 10_000);
        assert_eq!(destino(&sin_nif), Destino::SujetoPasivo);
        // Con el IVA en el total, un proveedor de fuera de España que lo cobra.
        let fuera = factura("IE-1", Some("IE1234567T"), 10_000, 2_100, 12_100);
        assert_eq!(destino(&fuera), Destino::Extranjero);
        let mut no_ded = interior.clone();
        no_ded.deductible_pct = 0;
        assert_eq!(destino(&no_ded), Destino::NoDeducible);
        // Autorrepercutida en la hoja de no deducibles: se devenga igual.
        let mut isp_no_ded = isp.clone();
        isp_no_ded.deductible_pct = 0;
        assert_eq!(destino(&isp_no_ded), Destino::SujetoPasivo);
        let exenta = factura("E-1", Some("B12345674"), 10_000, 0, 10_000);
        assert_eq!(destino(&exenta), Destino::SinIva);
    }

    #[test]
    fn trimestres_de_un_libro() {
        assert_eq!(
            trimestres(date!(2025 - 11 - 03), date!(2026 - 02 - 01)),
            vec![(2025, 4), (2026, 1)]
        );
        assert_eq!(
            limites(2026, 3),
            Some((date!(2026 - 07 - 01), date!(2026 - 09 - 30)))
        );
        assert_eq!(limites(2026, 0), None);
        assert_eq!(limites(2026, 5), None);
        let mut a = factura("A-1", Some("B12345674"), 100, 21, 121);
        a.issue_date = date!(2026 - 01 - 05);
        a.registered = a.issue_date;
        let mut b = a.clone();
        b.issue_date = date!(2026 - 09 - 24);
        b.registered = date!(2026 - 09 - 24);
        assert_eq!(
            periodo_del_libro(&[a, b]),
            Some((date!(2026 - 01 - 01), date!(2026 - 09 - 30)))
        );
    }

    #[test]
    fn autorrepercusion_en_el_libro_de_emitidas() {
        let compra = factura("INV-7", Some("FR12345678901"), 10_000, 2_100, 10_000);
        let mut emitida = compra.clone();
        emitida.total = Money::from_cents(12_100);
        assert!(es_autorrepercusion(&emitida, &[&compra]));
        let mut venta = factura("2025-1", Some("B12345674"), 10_000, 2_100, 12_100);
        assert!(!es_autorrepercusion(&venta, &[&compra]));
        // El mismo número de otro proveedor no lo es.
        venta.number = "INV-7".into();
        assert!(!es_autorrepercusion(&venta, &[&compra]));
        // Ni una compra interior al cliente con el mismo número, ni otra base.
        let interior = factura("2025-1", Some("B12345674"), 10_000, 2_100, 12_100);
        let venta = factura("2025-1", Some("B12345674"), 10_000, 2_100, 12_100);
        assert!(!es_autorrepercusion(&venta, &[&interior]));
        let mut otra = emitida.clone();
        otra.lines[0].base = Money::from_cents(20_000);
        assert!(!es_autorrepercusion(&otra, &[&compra]));
    }
}

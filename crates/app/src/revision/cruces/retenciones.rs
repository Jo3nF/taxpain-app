//! Retenciones practicadas y no ingresadas: lo que el diario importado en TaxPain abona a la 4751
//! (H.P. acreedora por retenciones practicadas) en cada trimestre frente a lo que declara el 111
//! (casilla [28]). Las de alquileres, las del capital mobiliario y las de no residentes, que
//! también van a la 4751, se apartan (ver [`super::datos::Retenciones4751`]): se declaran en el
//! 115, el 123 y el 216.
//!
//! El diario apunta la retención con la factura o la nómina, pero la obligación de retener nace
//! con el pago (art. 78.1 RIRPF) y el 111 declara lo retenido en el trimestre (art. 108.1
//! RIRPF): lo que un 111 no recoge puede ir en el de uno de los dos trimestres siguientes (una
//! factura que se paga más tarde), y lo que declara de más, ser de uno de los dos anteriores.
//! Sólo lo que no se compensa así es una retención sin ingresar, y para saberlo hacen falta los
//! dos trimestres siguientes (el diario y el 111); si falta alguno, queda por comprobar.
//!
//! El 190 sólo se compara con el diario si faltan 111 del año: con todos, lo que no cuadre entre
//! el 190 y los 111 ya sale en los resúmenes anuales.
//!
//! Sólo los años en que el diario usa la 4751 y los trimestres con asientos: un diario resumido a
//! tres cifras (la 475) no dice qué son retenciones.

use std::collections::{BTreeMap, BTreeSet};

use taxpain_core::money::Money;
use time::{Date, Month};

use super::super::modelos::Declarado;
use super::super::{
    Area, Destinatario, Gravedad, Nuevo, Responsable, Salida, UN_EURO, Urgente, eur, fecha,
    revisable_desde,
};
use super::Contexto;
use super::anuales::{RESUMENES, retenciones_anuales, total_periodico};
use super::datos::Retenciones4751;
use super::plazos::recargo;

/// Trimestres siguientes en que puede ir lo que un 111 no recoge (una factura o una nómina que
/// se paga más tarde), y anteriores de los que puede ser lo que declara de más (una que se pagó
/// antes de apuntarla).
const HORIZONTE: i32 = 2;

/// Un trimestre con asientos del diario, con su 111 si se sabe lo que declara.
struct Trimestre<'a> {
    y: i32,
    q: u8,
    r: Retenciones4751,
    /// El 111 y su total ([28], o [30] más [29]).
    d: Option<(&'a Declarado, Money)>,
}

impl Trimestre<'_> {
    fn periodo(&self) -> String {
        format!("{}T {}", self.q, self.y)
    }

    /// Lo que el diario abona a la 4751 de más frente al 111 (negativo, si es de menos).
    fn diferencia(&self) -> Option<Money> {
        self.d.map(|(_, m)| self.r.trabajo_profesionales - m)
    }

    /// Lo que va a otros modelos: alquileres, capital mobiliario y no residentes.
    fn aparte(&self) -> Money {
        self.r.alquileres + self.r.capital + self.r.no_residentes
    }
}

/// Número de orden de un trimestre: el 1T de un año va justo después del 4T del anterior.
fn orden(y: i32, q: u8) -> i32 {
    y * 4 + i32::from(q) - 1
}

/// El año y el trimestre de un número de orden.
fn de_orden(n: i32) -> (i32, u8) {
    (
        n.div_euclid(4),
        u8::try_from(n.rem_euclid(4) + 1).unwrap_or(4),
    )
}

/// Lo que un 111 no recoge de lo que el diario abona a la 4751 en su trimestre: lo que queda
/// después de compensarlo y con qué trimestres se compensa (su posición y cuánto).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Desfase {
    i: usize,
    queda: Money,
    con: Vec<(usize, Money)>,
}

/// Compensa lo que falta en un 111 con lo que sobra en otro de no más de [`HORIZONTE`]
/// trimestres antes o después, empezando por el más antiguo. `difs`: el número de orden de cada
/// trimestre y lo que el diario abona de más frente a su 111 (`None` si no se sabe; un euro o
/// menos no cuenta). Devuelve los trimestres en que falta algo, también si queda compensado.
fn compensar(difs: &[(i32, Option<Money>)]) -> Vec<Desfase> {
    let mut faltas: Vec<Desfase> = Vec::new();
    let mut sobras: Vec<Desfase> = Vec::new();
    for (i, (n, dif)) in difs.iter().enumerate() {
        let Some(dif) = *dif else {
            continue;
        };
        if dif.abs() <= UN_EURO {
            continue;
        }
        let (nuevos, otros) = if dif > Money::ZERO {
            (&mut faltas, &mut sobras)
        } else {
            (&mut sobras, &mut faltas)
        };
        let mut d = Desfase {
            i,
            queda: dif.abs(),
            con: Vec::new(),
        };
        for o in otros.iter_mut() {
            if d.queda <= Money::ZERO {
                break;
            }
            if o.queda <= Money::ZERO || n - difs[o.i].0 > HORIZONTE {
                continue;
            }
            let toma = d.queda.min(o.queda);
            d.queda -= toma;
            o.queda -= toma;
            d.con.push((o.i, toma));
            o.con.push((i, toma));
        }
        nuevos.push(d);
    }
    faltas
}

/// Por qué no se sabe todavía lo que el 111 del trimestre `n` declara frente al diario, si no
/// se sabe.
fn sin_ver(cx: &Contexto<'_>, ts: &[Trimestre<'_>], n: i32) -> Option<String> {
    let t = ts.iter().find(|t| orden(t.y, t.q) == n);
    if t.is_some_and(|t| t.d.is_some()) {
        return None;
    }
    let (y, q) = de_orden(n);
    let periodo = format!("{q}T");
    let vence = cx.plazo("111", y, &periodo).map(|p| p.due_on);
    let acaba = Month::try_from(q * 3)
        .ok()
        .and_then(|m| Date::from_calendar_date(y, m, m.length(y)).ok());
    Some(
        if vence.is_some_and(|v| v >= cx.hoy) || acaba.is_some_and(|a| a >= cx.hoy) {
            match vence {
                Some(v) => format!("el 111 del {q}T {y} se presenta hasta el {}", fecha(v)),
                None => format!("el {q}T {y} aún no ha acabado"),
            }
        } else if t.is_none() {
            if ts.iter().any(|t| t.y == y) {
                format!("el diario no tiene asientos del {q}T {y}")
            } else {
                format!("el diario de {y} no está en TaxPain o no usa la 4751")
            }
        } else if cx.modelo("111", y, &periodo).is_some() {
            format!("del 111 del {q}T {y} no se ha leído el total")
        } else {
            format!("el 111 del {q}T {y} no está en la carpeta")
        },
    )
}

/// «a», «a y b», «a, b y c».
fn y_lista(cosas: &[String]) -> String {
    match cosas {
        [] => String::new(),
        [una] => una.clone(),
        [resto @ .., ultima] => format!("{} y {ultima}", resto.join(", ")),
    }
}

/// Lo que el 111 de un trimestre no recoge después de compensarlo con los de al lado.
struct Falta<'a> {
    t: &'a Trimestre<'a>,
    queda: Money,
    /// Los trimestres cuyo 111 recoge parte de lo que falta, y cuánto.
    con: Vec<(String, Money)>,
    /// Si todavía no se sabe si lo recoge un 111 siguiente, por qué.
    por_ver: Option<String>,
}

impl Falta<'_> {
    /// «1T 2025: 225,00 € en la 4751 y 150,00 € en el 111 (75,00 € sin ingresar)», con lo que
    /// recogen de más los 111 de al lado: «…, más 40,00 € en el del 3T 2025 (…)».
    fn texto(&self, que: &str) -> String {
        let mas = if self.con.is_empty() {
            String::new()
        } else {
            let partes: Vec<String> = self
                .con
                .iter()
                .map(|(p, m)| format!("{} en el del {p}", eur(*m)))
                .collect();
            format!(", más {}", y_lista(&partes))
        };
        format!(
            "{}: {} en la 4751 y {} en el 111{mas} ({} {que})",
            self.t.periodo(),
            eur(self.t.r.trabajo_profesionales),
            eur(self.t.d.map_or(Money::ZERO, |(_, m)| m)),
            eur(self.queda)
        )
    }
}

/// El 190 de un año y lo que declara, si declara menos de lo que el diario abona a la 4751.
type Corto<'a> = Option<(&'a Declarado, Money)>;

pub(super) fn revisar(cx: &Contexto<'_>, s: &mut Salida) {
    let desde = revisable_desde(cx.hoy);
    let ts: Vec<Trimestre<'_>> = cx
        .exp
        .taxpain
        .retenciones_4751
        .iter()
        .filter(|((y, _), _)| *y >= desde)
        .map(|(&(y, q), r)| Trimestre {
            y,
            q,
            r: *r,
            // Sin sus casillas leídas, no se sabe lo que declara.
            d: cx
                .modelo("111", y, &format!("{q}T"))
                .filter(|d| d.con_casillas())
                .and_then(|d| total_periodico(&RESUMENES[0], d).map(|m| (d, m))),
        })
        .collect();
    let difs: Vec<(i32, Option<Money>)> = ts
        .iter()
        .map(|t| (orden(t.y, t.q), t.diferencia()))
        .collect();
    let mut faltas: BTreeMap<usize, Falta<'_>> = BTreeMap::new();
    for d in compensar(&difs) {
        let t = &ts[d.i];
        let n = orden(t.y, t.q);
        faltas.insert(
            d.i,
            Falta {
                t,
                queda: d.queda,
                con: d.con.iter().map(|(j, m)| (ts[*j].periodo(), *m)).collect(),
                por_ver: (1..=HORIZONTE).find_map(|k| sin_ver(cx, &ts, n + k)),
            },
        );
    }
    let años: BTreeSet<i32> = ts.iter().map(|t| t.y).collect();
    for y in años {
        let (sin_ingresar, por_ver): (Vec<&Falta<'_>>, Vec<&Falta<'_>>) = faltas
            .values()
            .filter(|f| f.t.y == y && f.queda > UN_EURO)
            .partition(|f| f.por_ver.is_none());
        let del_año: Vec<&Trimestre<'_>> = ts.iter().filter(|t| t.y == y).collect();
        let diario: Money = del_año.iter().map(|t| t.r.trabajo_profesionales).sum();
        let sin_111: Vec<String> = (1..=4u8)
            .filter(|q| !del_año.iter().any(|t| t.q == *q && t.d.is_some()))
            .map(|q| format!("{q}T {y}"))
            .collect();
        // Un 190 complementario sin el anterior sólo trae lo que faltaba: no es el año entero.
        let corto: Corto<'_> = cx
            .modelo("190", y, "0A")
            .filter(|d| d.con_casillas())
            .and_then(|d| retenciones_anuales(d, 3).map(|m| (d, m)))
            .filter(|(_, m)| del_año.len() == 4 && diario - *m > UN_EURO);
        let aparte = del_año.iter().any(|t| !t.aparte().is_zero());
        if !sin_ingresar.is_empty() {
            no_ingresadas(cx, y, &sin_ingresar, corto, diario, aparte, s);
        }
        let corto = corto.filter(|_| sin_ingresar.is_empty() && !sin_111.is_empty());
        if !por_ver.is_empty() || corto.is_some() {
            por_comprobar(y, &por_ver, corto, diario, &sin_111, s);
        }
    }
    let filas = ts
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let sin_compensar = faltas
                .get(&i)
                .map(|f| {
                    if f.queda > UN_EURO && f.por_ver.is_some() {
                        format!("{} (por comprobar)", eur(f.queda))
                    } else {
                        eur(f.queda)
                    }
                })
                .unwrap_or_default();
            vec![
                t.periodo(),
                eur(t.r.trabajo_profesionales),
                t.d.map_or_else(|| "no está".into(), |(_, m)| eur(m)),
                t.diferencia().map(eur).unwrap_or_default(),
                eur(t.aparte()),
                sin_compensar,
            ]
        })
        .collect();
    s.tabla(
        "Retenciones del diario (4751) frente a los 111",
        "Retenciones 4751",
        &[
            "Periodo",
            "4751: trabajo y profesionales",
            "111 [28]",
            "Diferencia",
            "4751: alquileres, capital y no residentes (aparte)",
            "Sin compensar",
        ],
        filas,
    );
}

/// El aviso de las retenciones del año que ningún 111 recoge, con su recargo, y la petición de
/// las complementarias (y del 190, si también se queda corto).
fn no_ingresadas(
    cx: &Contexto<'_>,
    y: i32,
    faltas: &[&Falta<'_>],
    corto: Corto<'_>,
    diario: Money,
    aparte: bool,
    s: &mut Salida,
) {
    let mut recargos = Money::ZERO;
    let mut intereses = false;
    for f in faltas {
        if let Some(p) = cx.plazo("111", y, &format!("{}T", f.t.q))
            && let Some(x) = recargo(p.due_on, cx.hoy, f.queda)
        {
            recargos += x.importe;
            intereses |= x.intereses;
        }
    }
    let total: Money = faltas.iter().map(|f| f.queda).sum();
    let lista: Vec<String> = faltas.iter().map(|f| f.texto("sin ingresar")).collect();
    let mut ficheros: Vec<String> = faltas
        .iter()
        .filter_map(|f| f.t.d.map(|(d, _)| d.fichero.clone()))
        .collect();
    let mut detalle = format!(
        "El diario abona a la 4751 (retenciones practicadas) más de lo que declaran los 111 \
         (casilla [28]) y los dos trimestres siguientes no lo compensan: {}. Lo que el diario \
         apunta en un trimestre puede ir en el 111 de uno de los dos siguientes, si se pagó \
         entonces (la obligación de retener nace con el pago, art. 78.1 RIRPF, y el 111 declara \
         lo retenido en el trimestre, art. 108.1 RIRPF): ya está descontado lo que esos 111 \
         declaran de más.{} Lo que queda, o falta ingresarlo con una complementaria del 111, o \
         es una retención que no va en el 111 (una factura o una nómina mal contabilizada, o la \
         de un no residente, que va en el 216). Presentada después del plazo y sin requerimiento \
         previo, la complementaria lleva el recargo del art. 27 LGT (el 1 % más otro 1 % por \
         cada mes completo de retraso; el 15 % pasados doce meses, con intereses de demora desde \
         entonces) y no tiene sanción (art. 179.3 LGT){}. Si lo descubre Hacienda, es una \
         infracción del art. 191 LGT.",
        lista.join("; "),
        if aparte {
            " Las retenciones de alquileres, del capital mobiliario y de no residentes de la 4751 \
             van aparte (se declaran en el 115, el 123 y el 216)."
        } else {
            ""
        },
        if recargos.is_zero() {
            String::new()
        } else {
            format!(
                ": hoy, unos {} de recargo{}",
                eur(recargos),
                if intereses {
                    " más los intereses de demora"
                } else {
                    ""
                }
            )
        }
    );
    if let Some((d, m)) = corto {
        detalle.push_str(&format!(
            " El 190 de {y} declara {} de retenciones del trabajo y de profesionales y el diario \
             del año abona {} a la 4751 (sin las de alquileres, capital y no residentes): hay \
             que corregirlo también.",
            eur(m),
            eur(diario)
        ));
        ficheros.push(d.fichero.clone());
    }
    detalle.push_str(
        " Qué hacer: pide a la gestoría que lo compruebe con el mayor de la 4751 y, si falta \
         ingresar, que presente las complementarias.",
    );
    s.add(
        Nuevo {
            codigo: "cruces.retenciones_no_ingresadas",
            area: Area::Modelos,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Gestoria,
            titulo: format!(
                "Retenciones practicadas y no ingresadas en {y}: {}",
                eur(total)
            ),
            ficheros,
        }
        .detalle(detalle),
        Some(y),
        Some(-(total + recargos)),
    );
    s.urgente(Urgente {
        recargo_hoy: (!recargos.is_zero()).then_some(recargos),
        ..Urgente::sin_calculo(
            Some(total),
            "Retenciones practicadas que faltan por ingresar: presentadas ahora con una \
             complementaria sin requerimiento, llevan el recargo del art. 27 LGT de cada \
             trimestre (1 % más otro 1 % por mes completo; el 15 % e intereses pasados doce \
             meses), que sube cada mes que pasa. Si lo descubre Hacienda, sanción (art. 191 LGT).",
        )
    });
    let periodos: Vec<String> = faltas.iter().map(|f| f.t.periodo()).collect();
    let mut que = format!(
        "Presentar complementarias del 111 de {} por las retenciones practicadas y no \
         ingresadas ({})",
        periodos.join(", "),
        eur(total)
    );
    if corto.is_some() {
        que.push_str(&format!(" y corregir el 190 de {y}"));
    }
    s.pedir(Destinatario::Gestoria, que);
}

/// La nota de lo que todavía no se sabe si está sin ingresar: lo que el 111 de un trimestre no
/// recoge y aún puede ir en uno de los dos siguientes, y el 190 que se queda corto cuando faltan
/// 111 del año (`sin_111`).
fn por_comprobar(
    y: i32,
    faltas: &[&Falta<'_>],
    corto: Corto<'_>,
    diario: Money,
    sin_111: &[String],
    s: &mut Salida,
) {
    let mut partes: Vec<String> = Vec::new();
    let mut ficheros: Vec<String> = Vec::new();
    let total: Money = faltas.iter().map(|f| f.queda).sum();
    if !faltas.is_empty() {
        let lista: Vec<String> = faltas.iter().map(|f| f.texto("sin compensar")).collect();
        let mut razones: Vec<String> = Vec::new();
        for r in faltas.iter().filter_map(|f| f.por_ver.clone()) {
            if !razones.contains(&r) {
                razones.push(r);
            }
        }
        partes.push(format!(
            "El diario abona a la 4751 más de lo que declaran los 111: {}. Puede ir en el 111 de \
             uno de los dos trimestres siguientes, si se pagó entonces (la obligación de retener \
             nace con el pago, art. 78.1 RIRPF), pero aún no se puede ver: {}. Si no va en \
             ninguno, es una retención practicada y no ingresada, y hay que presentar una \
             complementaria del 111 con el recargo del art. 27 LGT.",
            lista.join("; "),
            razones.join("; ")
        ));
        ficheros.extend(
            faltas
                .iter()
                .filter_map(|f| f.t.d.map(|(d, _)| d.fichero.clone())),
        );
    }
    if let Some((d, m)) = corto {
        partes.push(format!(
            "El 190 de {y} declara {} de retenciones del trabajo y de profesionales y el diario \
             del año abona {} a la 4751 (sin las de alquileres, capital y no residentes): {} \
             más. {} {}: no se ve de qué trimestre es. Puede ser una retención apuntada a final \
             de año y pagada en el siguiente (entonces va en el 190 de ese año) o una retención \
             sin ingresar.",
            eur(m),
            eur(diario),
            eur(diario - m),
            if sin_111.len() == 1 {
                "Falta el 111 del"
            } else {
                "Faltan los 111 del"
            },
            y_lista(sin_111)
        ));
        ficheros.push(d.fichero.clone());
    }
    partes.push(
        "Qué hacer: vuelve a revisarlo cuando estén; si ningún 111 lo recoge, pide a la gestoría \
         que lo compruebe con el mayor de la 4751."
            .into(),
    );
    let titulo = match corto {
        Some((_, m)) if faltas.is_empty() => format!(
            "El 190 de {y} declara menos retenciones de las que recoge el diario: {}",
            eur(diario - m)
        ),
        _ => format!(
            "Retenciones de {y} por comprobar en los 111 siguientes: {}",
            eur(total)
        ),
    };
    s.add(
        Nuevo {
            codigo: "cruces.retenciones_por_comprobar",
            area: Area::Modelos,
            gravedad: Gravedad::Info,
            responsable: Responsable::Gestoria,
            titulo,
            ficheros,
        }
        .detalle(partes.join(" ")),
        Some(y),
        None,
    );
}

#[cfg(test)]
mod tests {
    use time::macros::date;

    use super::super::super::expediente::Expediente;
    use super::super::datos::DatosTaxPain;
    use super::*;

    fn m(euros: i64) -> Option<Money> {
        Some(Money::from_euros(euros, 0))
    }

    /// Lo que falta en un 111 se compensa con lo que sobra en uno de los dos trimestres de al
    /// lado (antes o después), del más antiguo al más reciente; más lejos, no. Un trimestre sin
    /// 111 no corta la cuenta y un euro o menos no cuenta.
    #[test]
    fn compensar_con_los_dos_trimestres_de_al_lado() {
        let f = compensar(&[(0, m(75)), (1, m(-75))]);
        assert_eq!(f.len(), 1);
        assert_eq!(
            (f[0].queda, f[0].con.clone()),
            (Money::ZERO, vec![(1, eur_(75))])
        );
        let f = compensar(&[(0, m(75)), (1, m(0)), (2, m(0)), (3, m(-75))]);
        assert_eq!(f[0].queda, eur_(75));
        let f = compensar(&[(0, m(75)), (1, None), (2, m(-75))]);
        assert_eq!(f[0].queda, Money::ZERO);
        // Lo que sobra antes también compensa (se pagó antes de apuntarla).
        let f = compensar(&[(0, m(-30)), (1, m(50))]);
        assert_eq!((f[0].i, f[0].queda), (1, eur_(20)));
        // Primero lo más antiguo.
        let f = compensar(&[(0, m(75)), (1, m(25)), (2, m(-90))]);
        assert_eq!(
            f.iter().map(|d| d.queda).collect::<Vec<_>>(),
            vec![Money::ZERO, eur_(10)]
        );
        assert!(compensar(&[(0, Some(Money::from_cents(50)))]).is_empty());
    }

    fn eur_(e: i64) -> Money {
        Money::from_euros(e, 0)
    }

    fn ciento_once(y: i32, q: u8, euros: i64) -> crate::presentados::Filed {
        crate::presentados::Filed {
            id: i64::from(q),
            model: "111".into(),
            year: y,
            period: format!("{q}T"),
            filename: format!("111-{q}T-{y}.pdf"),
            source: "aeat".into(),
            boxes: [(28, eur_(euros))].into_iter().collect(),
        }
    }

    fn expediente(
        diario: &[((i32, u8), i64)],
        presentados: Vec<crate::presentados::Filed>,
    ) -> Expediente {
        Expediente {
            carpeta: String::new(),
            empresa: "EMPRESA DEMO SL".into(),
            own_nif: Some("B12345674".into()),
            docs: Vec::new(),
            no_admitidos: Vec::new(),
            presentados,
            perfil: None,
            perfil_deducido: None,
            taxpain: DatosTaxPain {
                retenciones_4751: diario
                    .iter()
                    .map(|(t, e)| {
                        (
                            *t,
                            Retenciones4751 {
                                trabajo_profesionales: eur_(*e),
                                abonos: 1,
                                ..Retenciones4751::default()
                            },
                        )
                    })
                    .collect(),
                ..Default::default()
            },
            aeat: Vec::new(),
            comprobado: Default::default(),
        }
    }

    fn hallazgo(
        exp: &Expediente,
        hoy: Date,
        codigo: &str,
    ) -> Option<super::super::super::Hallazgo> {
        let mut s = Salida::default();
        revisar(&Contexto::new(exp, hoy), &mut s);
        s.hallazgos.into_iter().find(|h| h.codigo == codigo)
    }

    /// El 111 del 1T 2025 recoge 75 € menos que el diario y ni el del 2T ni el del 3T los
    /// recogen: sin ingresar, con el recargo de los meses que pasaron desde el plazo (21/04/2025):
    /// el 7 % a primeros de noviembre de 2025 y el 15 %, con intereses, un año después.
    #[test]
    fn el_recargo_de_lo_que_ningun_111_recoge() {
        let exp = expediente(
            &[((2025, 1), 225), ((2025, 2), 150), ((2025, 3), 150)],
            vec![
                ciento_once(2025, 1, 150),
                ciento_once(2025, 2, 150),
                ciento_once(2025, 3, 150),
            ],
        );
        let h = hallazgo(
            &exp,
            date!(2025 - 11 - 01),
            "cruces.retenciones_no_ingresadas",
        )
        .unwrap();
        assert!(
            h.detalle.contains("hoy, unos 5,25 € de recargo."),
            "{}",
            h.detalle
        );
        assert_eq!(h.importe, Some(Money::from_cents(-8_025)));
        let h = hallazgo(
            &exp,
            date!(2026 - 11 - 01),
            "cruces.retenciones_no_ingresadas",
        )
        .unwrap();
        assert!(
            h.detalle
                .contains("hoy, unos 11,25 € de recargo más los intereses de demora."),
            "{}",
            h.detalle
        );
        assert_eq!(h.importe, Some(Money::from_cents(-8_625)));
    }

    /// Un 190 complementario sin el original en la carpeta sólo trae lo que faltaba: sus
    /// retenciones no son las del año y no se comparan con el diario.
    #[test]
    fn un_190_parcial_no_se_compara_con_el_diario() {
        use super::super::super::expediente::{Clase, Doc, Modelo, TipoDeclaracion};
        let mut exp = expediente(
            &[
                ((2025, 1), 150),
                ((2025, 2), 150),
                ((2025, 3), 150),
                ((2025, 4), 150),
            ],
            vec![
                ciento_once(2025, 1, 150),
                ciento_once(2025, 2, 150),
                ciento_once(2025, 3, 150),
            ],
        );
        let ciento_noventa = |tipo| Doc {
            ruta: "MODELOS/190-2025-complementaria.pdf".into(),
            sha: "190".into(),
            clase: Clase::Modelo,
            descripcion: String::new(),
            con_ia: false,
            ruta_año: None,
            ruta_trimestre: None,
            factura: None,
            modelo: Some(Modelo {
                modelo: "190".into(),
                ejercicio: 2025,
                periodo: "0A".into(),
                casillas: [(3, eur_(100))].into_iter().collect(),
                presentado: Some(date!(2026 - 03 - 02)),
                justificante: Some("1900000000021".into()),
                anterior: Some("1900000000013".into()),
                tipo,
                ..Modelo::default()
            }),
            libro: None,
            autorrepercusion: Vec::new(),
            extractos: Vec::new(),
            extracto_error: None,
            empresa: None,
            tramite: None,
            aplicacion: None,
            nomina: None,
            mencion_sin_iva: None,
            oficial: None,
            texto_no_aeat: false,
            participada: None,
        };
        let hoy = date!(2026 - 11 - 01);
        // Con un 190 entero que declara menos, sí se dice.
        exp.docs = vec![ciento_noventa(TipoDeclaracion::Sustitutiva)];
        let h = hallazgo(&exp, hoy, "cruces.retenciones_por_comprobar").unwrap();
        assert!(
            h.detalle.contains("El 190 de 2025 declara 100,00 €"),
            "{}",
            h.detalle
        );
        exp.docs = vec![ciento_noventa(TipoDeclaracion::Complementaria)];
        let h = hallazgo(&exp, hoy, "cruces.retenciones_por_comprobar");
        assert!(
            h.as_ref().is_none_or(|h| !h.detalle.contains("El 190")),
            "{:?}",
            h.map(|h| h.detalle)
        );
    }

    /// Lo que el 111 del 3T 2026 no recoge puede ir en el del 4T, que aún no vence: queda por
    /// comprobar, sin aviso ni recargo.
    #[test]
    fn lo_que_el_111_siguiente_aun_puede_recoger() {
        let exp = expediente(&[((2026, 3), 225)], vec![ciento_once(2026, 3, 150)]);
        let hoy = date!(2026 - 11 - 01);
        assert!(hallazgo(&exp, hoy, "cruces.retenciones_no_ingresadas").is_none());
        let h = hallazgo(&exp, hoy, "cruces.retenciones_por_comprobar").unwrap();
        assert_eq!(h.gravedad, Gravedad::Info);
        assert!(
            h.detalle.contains(
                "3T 2026: 225,00 € en la 4751 y 150,00 € en el 111 (75,00 € sin compensar)"
            ) && h
                .detalle
                .contains("el 111 del 4T 2026 se presenta hasta el 20/01/2027"),
            "{}",
            h.detalle
        );
    }
}

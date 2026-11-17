//! Socios y administradores: quién cobra como administrador (Nóminas de TaxPain y clave E del
//! 190) frente a quién lo es según las escrituras y los estatutos, y los saldos de las cuentas
//! corrientes con socios y administradores (551) y con otras partes vinculadas (552).

use std::collections::BTreeSet;

use taxpain_core::money::{Money, Rate};
use time::Date;

use super::super::expediente::{Doc, Nomina, nif_clave, nif_de_persona};
use super::super::{
    Area, Destinatario, Gravedad, Nuevo, Responsable, Salida, UN_EURO, eur, fecha, lista, plural,
    revisable_desde,
};
use super::{Contexto, minuscula, primera};
use crate::documentos::DocKind;

pub(super) fn revisar(cx: &Contexto<'_>, s: &mut Salida) {
    if !cx.sociedad() {
        return;
    }
    let nombrados = nombramientos(cx);
    administradores(cx, &nombrados, s);
    como_empleado(cx, &nombrados, s);
    saldos(cx, s);
}

/// Un nombramiento leído de una escritura, los estatutos o el Registro.
struct Nombramiento {
    fecha: Option<Date>,
    documento: String,
    administrador: String,
    fichero: String,
}

/// Documentos que nombran administradores.
pub(crate) fn nombra(k: DocKind) -> bool {
    matches!(
        k,
        DocKind::EscrituraConstitucion
            | DocKind::EscrituraPoderes
            | DocKind::EscrituraModificacion
            | DocKind::Escritura
            | DocKind::Estatutos
            | DocKind::RegistroMercantil
            | DocKind::ActaJunta
    )
}

/// Palabras de un nombre para compararlo (sin tildes, en mayúsculas, de tres letras o más).
pub(crate) fn palabras(s: &str) -> BTreeSet<String> {
    s.to_uppercase()
        .chars()
        .map(|c| match c {
            'Á' | 'À' => 'A',
            'É' | 'È' => 'E',
            'Í' | 'Ì' => 'I',
            'Ó' | 'Ò' => 'O',
            'Ú' | 'Ù' | 'Ü' => 'U',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect::<String>()
        .split_whitespace()
        .filter(|w| w.chars().count() >= 3 && !["DEL", "LAS", "LOS", "DON", "DOÑA"].contains(w))
        .map(str::to_owned)
        .collect()
}

/// ¿Es `quien` (nombre y NIF) el administrador que dice `texto`?
pub(crate) fn es(texto: &str, nombre: &str, nif: Option<&str>) -> bool {
    if let Some(n) = nif.map(nif_clave).filter(|n| !n.is_empty())
        && nif_clave(texto).contains(&n)
    {
        return true;
    }
    let p = palabras(nombre);
    !p.is_empty() && p.is_subset(&palabras(texto))
}

/// ¿Es `quien` la persona que nombra `texto`? Si los dos tienen NIF, por el NIF (dos NIF
/// distintos son dos personas aunque se llamen igual); si no, como [`es`].
pub(crate) fn es_persona(texto: &str, nombre: &str, nif: Option<&str>) -> bool {
    let suyo = super::super::expediente::nif_de_persona(texto, None);
    match (suyo, nif.map(nif_clave).filter(|n| !n.is_empty())) {
        (Some(a), Some(n)) => a == n,
        _ => es(texto, nombre, nif),
    }
}

/// Los nombramientos de administrador de los documentos de la carpeta y de TaxPain, por fecha.
fn nombramientos(cx: &Contexto<'_>) -> Vec<Nombramiento> {
    let mut nombrados: Vec<Nombramiento> = Vec::new();
    for d in &cx.exp.docs {
        if let Some((k, f)) = &d.empresa
            && nombra(*k)
            && let Some(a) = f.administrator.as_deref().filter(|a| !a.trim().is_empty())
        {
            nombrados.push(Nombramiento {
                fecha: f.date.or(f.constitution_date),
                documento: k.title().to_owned(),
                administrador: a.trim().to_owned(),
                fichero: d.ruta.clone(),
            });
        }
    }
    for g in &cx.exp.taxpain.documentos {
        if nombra(g.kind)
            && let Some(a) = g
                .facts
                .administrator
                .as_deref()
                .filter(|a| !a.trim().is_empty())
        {
            nombrados.push(Nombramiento {
                fecha: g.fecha.or(g.facts.date).or(g.facts.constitution_date),
                documento: format!("{} (en TaxPain)", g.kind.title()),
                administrador: a.trim().to_owned(),
                fichero: g.fichero.clone(),
            });
        }
    }
    nombrados.sort_by_key(|n| n.fecha);
    nombrados
}

fn administradores(cx: &Contexto<'_>, nombrados: &[Nombramiento], s: &mut Salida) {
    s.tabla(
        "Administradores según los documentos",
        "Administradores",
        &["Fecha", "Documento", "Administrador", "Fichero"],
        nombrados
            .iter()
            .map(|n| {
                vec![
                    n.fecha.map(fecha).unwrap_or_default(),
                    n.documento.clone(),
                    n.administrador.clone(),
                    n.fichero.clone(),
                ]
            })
            .collect(),
    );
    // Quién cobra como administrador: Nóminas de TaxPain y la clave E del 190.
    let mut pagados: Vec<(String, Option<String>, String)> = cx
        .exp
        .taxpain
        .administradores
        .iter()
        .map(|a| {
            (
                a.nombre.clone(),
                Some(a.nif.clone()),
                "Nóminas de TaxPain".into(),
            )
        })
        .collect();
    for ((_, y, _), d) in cx.de("190") {
        for r in &d.registros {
            if r.clave.as_deref().is_some_and(|c| c.starts_with('E')) {
                pagados.push((
                    r.nombre.clone().unwrap_or_default(),
                    r.nif.clone(),
                    format!("190 de {y} (clave E)"),
                ));
            }
        }
    }
    let mut vistos = BTreeSet::new();
    pagados.retain(|(n, nif, _)| {
        vistos.insert(nif.as_deref().map(nif_clave).unwrap_or_else(|| n.clone()))
    });
    if pagados.is_empty() {
        return;
    }
    let Some(vigente) = nombrados.last() else {
        let quienes: Vec<String> = pagados
            .iter()
            .map(|(n, _, de)| format!("{n} ({de})"))
            .collect();
        s.add(
            Nuevo {
                codigo: "cruces.administrador_sin_escritura",
                area: Area::Socios,
                gravedad: Gravedad::Info,
                responsable: Responsable::Documentacion,
                titulo: "Falta la escritura con el nombramiento del administrador".into(),
                ficheros: vec![],
            }
            .detalle(format!(
                "Cobra como administrador {} y en la carpeta no hay ninguna escritura, estatutos \
                 o nota del Registro que diga quién es el administrador. Añade la escritura de \
                 nombramiento (o una nota simple del Registro Mercantil) para comprobarlo.",
                lista(&quienes, 6)
            )),
            None,
            None,
        );
        s.pedir(
            Destinatario::Registro,
            "Nota simple de la sociedad con el órgano de administración vigente",
        );
        return;
    };
    for (nombre, nif, de) in &pagados {
        if es(&vigente.administrador, nombre, nif.as_deref()) {
            continue;
        }
        let antes = nombrados
            .iter()
            .rev()
            .skip(1)
            .find(|n| es(&n.administrador, nombre, nif.as_deref()));
        let cuando = vigente
            .fecha
            .map(|f| format!(" del {}", fecha(f)))
            .unwrap_or_default();
        let historia = match antes {
            Some(a) => format!(
                " Lo era según {}{}: puede que ya no lo sea y siga cobrando como tal, o que \
                 falte en la carpeta la escritura que lo vuelve a nombrar.",
                minuscula(&a.documento),
                a.fecha
                    .map(|f| format!(" del {}", fecha(f)))
                    .unwrap_or_default()
            ),
            None => " No aparece como administrador en ningún documento de la carpeta.".into(),
        };
        s.add(
            Nuevo {
                codigo: "cruces.administrador_no_coincide",
                area: Area::Socios,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Empresa,
                titulo: format!(
                    "{nombre} cobra como administrador y no lo es según las escrituras"
                ),
                ficheros: vec![vigente.fichero.clone()],
            }
            .detalle(format!(
                "Figura como administrador en {de}, pero según {}{cuando} el administrador es {}.\
                 {historia} El cargo tiene que estar nombrado e inscrito en el Registro Mercantil \
                 (art. 214 y 215 LSC) y, si cobra por serlo, los estatutos tienen que decir que el \
                 cargo es retribuido (art. 217 LSC); si no, Hacienda puede discutir el gasto. Qué \
                 hacer: comprueba los nombramientos con una nota simple del Registro y corrige la \
                 ficha en Nóminas o el 190.",
                minuscula(&vigente.documento),
                vigente.administrador
            )),
            None,
            None,
        );
    }
}

/// El nombre de un administrador sin el NIF ni el cargo que le siguen en la escritura
/// («NOMBRE APELLIDO (12345678Z), administrador único» → «NOMBRE APELLIDO»). Lo que va tras una
/// coma sigue siendo el nombre si no es el cargo ni un NIF («APELLIDOS, NOMBRE», como en
/// Nóminas).
pub(crate) fn sin_cargo(texto: &str) -> &str {
    let antes = texto.split('(').next().unwrap_or(texto);
    let cargo = |trozo: &str| {
        let t = crate::documentos::fold(trozo);
        t.chars().any(|c| c.is_ascii_digit())
            || [
                "administr",
                "consejer",
                "apoderad",
                "president",
                "secretari",
                "vocal",
                "gerente",
                "liquidador",
                "cargo",
                "solidari",
                "mancomunad",
                "delegad",
            ]
            .iter()
            .any(|c| t.contains(c))
    };
    let fin = antes
        .match_indices(',')
        .map(|(i, _)| i)
        .find(|&i| cargo(antes[i + 1..].split(',').next().unwrap_or_default()))
        .unwrap_or(antes.len());
    let nombre = antes[..fin].trim().trim_end_matches(',').trim();
    if nombre.is_empty() {
        texto.trim()
    } else {
        nombre
    }
}

/// Un trimestre: año y número (1 a 4).
type Trim = (i32, u8);

fn trim_de(f: Date) -> Trim {
    (f.year(), super::super::trimestre(f))
}

fn siguiente((y, q): Trim) -> Trim {
    if q == 4 { (y + 1, 1) } else { (y, q + 1) }
}

fn nombre_trim((y, q): Trim) -> String {
    format!("{q}T {y}")
}

/// «1T 2024 a 4T 2024 y 2T 2025»: los trimestres seguidos, juntos.
fn rangos(ts: &[Trim]) -> String {
    let mut partes: Vec<String> = Vec::new();
    let mut i = 0;
    while i < ts.len() {
        let mut j = i;
        while j + 1 < ts.len() && ts[j + 1] == siguiente(ts[j]) {
            j += 1;
        }
        partes.push(if i == j {
            nombre_trim(ts[i])
        } else {
            format!("{} a {}", nombre_trim(ts[i]), nombre_trim(ts[j]))
        });
        i = j + 1;
    }
    match partes.as_slice() {
        [] => String::new(),
        [uno] => uno.clone(),
        [resto @ .., ultimo] => format!("{} y {ultimo}", resto.join(", ")),
    }
}

/// Lo cobrado y lo retenido en un trimestre, y de dónde sale.
struct Trimestre {
    t: Trim,
    base: Money,
    retenido: Money,
    /// El 111 del trimestre, si sale de él.
    fichero: Option<String>,
}

/// Desde cuándo es administrador el vigente: la fecha del primero de los nombramientos seguidos
/// que lo nombran a él (los de antes nombran a otro).
fn desde_cuando(nombrados: &[Nombramiento]) -> Option<Date> {
    let vigente = nombrados.last()?;
    let nombre = sin_cargo(&vigente.administrador);
    let nif = nif_de_persona(&vigente.administrador, None);
    let mut desde = vigente.fecha;
    for n in nombrados.iter().rev().skip(1) {
        if !es_persona(&n.administrador, nombre, nif.as_deref()) {
            break;
        }
        desde = n.fecha.or(desde);
    }
    desde
}

/// El tipo de retención en centésimas de punto (1900 es el 19 %), por los importes.
fn tipo_de(bruto: Money, retenido: Money) -> Option<i32> {
    (bruto.cents() > 0)
        .then(|| i32::try_from(retenido.cents() * 10_000 / bruto.cents()).ok())
        .flatten()
}

/// ¿Es un pago de nómina a esta persona, por el concepto del banco? («TRASPASO JUAN PÉREZ
/// GÓMEZ NÓMINA ENERO»).
fn nomina_en_el_banco(concepto: &str, nombre: &str, nif: Option<&str>) -> bool {
    let c = crate::documentos::fold(concepto);
    (c.contains("nomina") || c.contains("salario") || c.contains("sueldo"))
        && es(concepto, nombre, nif)
}

/// El administrador según las escrituras cobra como un empleado (clave A): con nóminas de la
/// carpeta o de TaxPain, o en el 190, con menos retención que la del cargo. Si lo que cobra es por
/// el cargo, va con la clave E y una retención fija del 35 % (19 % con menos de 100.000 € de cifra
/// de negocios en el último ejercicio cerrado antes del pago, elevada al año si duró menos; art.
/// 101.2 LIRPF y art. 80.1.3.º RIRPF), también la de lo que cobra en especie (art. 102.1 RIRPF), y
/// los estatutos tienen que decir que el cargo es retribuido (art. 217 LSC). Es dudoso (un trabajo
/// distinto del cargo puede ir con la clave A): lo decide la gestoría. Sólo cuenta lo de después
/// del nombramiento. Con los 111 de cada trimestre (o las nóminas con importes) se calcula la
/// retención que faltaría y hasta cuándo se puede reclamar; el 111 de un solo perceptor es suyo si
/// algo dice que cobró en el trimestre (una nómina, un pago de nómina a su nombre en el banco, el
/// 190 del año) o queda entre dos trimestres en los que cobró.
fn como_empleado(cx: &Contexto<'_>, nombrados: &[Nombramiento], s: &mut Salida) {
    let Some(vigente) = nombrados.last() else {
        return;
    };
    let nombre = sin_cargo(&vigente.administrador).to_owned();
    let nif = nif_de_persona(&vigente.administrador, None);
    let suyo = |n: Option<&str>, nif: Option<&str>| {
        es_persona(&vigente.administrador, n.unwrap_or_default(), nif)
    };
    let desde = desde_cuando(nombrados);
    let mes_desde = desde.and_then(|d| d.replace_day(1).ok());
    let despues = |f: Date| mes_desde.is_none_or(|m| f >= m);
    // El tipo del cargo según la cifra de negocios del último ejercicio cerrado antes del pago: la
    // del 200 del año anterior ([00255]), elevada al año si el periodo duró menos.
    let todos = super::super::modelos::declarados(cx.exp);
    let pequeña = |y: i32| {
        todos
            .get(&("200".to_owned(), y - 1, "0A".to_owned()))
            .filter(|d| d.casillas.contains_key(&255))
            .map(|d| {
                let cifra = if (1..365).contains(&d.dias) {
                    Money::from_cents(d.c(255).cents() * 365 / d.dias)
                } else {
                    d.c(255)
                };
                cifra < Money::from_euros(100_000, 0)
            })
    };
    let tipo_cargo = |y: i32| if pequeña(y) == Some(false) { 35 } else { 19 };
    // Con menos retención que la del cargo (o sin saber cuánta) es la de un empleado; con la del
    // cargo, ya va como administrador.
    let por_debajo = |tipo: Option<i32>, y: i32| tipo.is_none_or(|t| t + 10 < tipo_cargo(y) * 100);
    let mut pruebas: Vec<String> = Vec::new();
    let mut ficheros: Vec<String> = vec![vigente.fichero.clone()];
    // Nóminas de la carpeta, de después del nombramiento.
    let nominas: Vec<(&Doc, &Nomina)> = cx
        .exp
        .docs
        .iter()
        .filter_map(|d| d.nomina.as_ref().map(|n| (d, n)))
        .filter(|(_, n)| suyo(n.nombre.as_deref(), n.nif.as_deref()))
        .filter(|(d, n)| match (n.mes, mes_desde) {
            (Some(m), _) => despues(m),
            (None, Some(m)) => d.ruta_año.is_none_or(|y| y >= m.year()),
            (None, None) => true,
        })
        .collect();
    let año_de = |d: &Doc, n: &Nomina| n.mes.map(|m| m.year()).or(d.ruta_año);
    let trim_nomina = |d: &Doc, n: &Nomina| n.mes.map(trim_de).or(d.ruta_año.zip(d.ruta_trimestre));
    let de_empleado: Vec<&(&Doc, &Nomina)> = nominas
        .iter()
        .filter(|(d, n)| {
            let tipo = n.tipo.or_else(|| tipo_de(n.bruto?, n.retencion?));
            por_debajo(tipo, año_de(d, n).unwrap_or(cx.hoy.year()))
        })
        .collect();
    if !de_empleado.is_empty() {
        let años: BTreeSet<i32> = de_empleado
            .iter()
            .filter_map(|(d, n)| año_de(d, n))
            .collect();
        let tipos: BTreeSet<i32> = de_empleado.iter().filter_map(|(_, n)| n.tipo).collect();
        let mut t = plural(
            de_empleado.len(),
            "nómina de la carpeta",
            "nóminas de la carpeta",
        );
        match (años.first(), años.last()) {
            (Some(a), Some(b)) if a == b => t.push_str(&format!(" de {a}")),
            (Some(a), Some(b)) => t.push_str(&format!(" de {a} a {b}")),
            _ => {}
        }
        let tipos: Vec<String> = tipos
            .iter()
            .map(|t| format!("del {}", Rate::from_bp(*t)))
            .collect();
        match tipos.as_slice() {
            [] => {}
            [uno] => t.push_str(&format!(" con una retención del IRPF {uno}")),
            [resto @ .., ultimo] => t.push_str(&format!(
                " con retenciones del IRPF {} y {ultimo}",
                resto.join(", ")
            )),
        }
        pruebas.push(t);
        ficheros.extend(de_empleado.iter().map(|(d, _)| d.ruta.clone()));
    }
    // Lo que dice que cobró en un trimestre: sus nóminas, sus recibos de TaxPain, los pagos de
    // nómina a su nombre en el banco y el 190 del año si es el único perceptor con la clave A.
    let mut cobros: BTreeSet<Trim> = nominas
        .iter()
        .filter_map(|(d, n)| trim_nomina(d, n))
        .collect();
    // El 190 con la clave A.
    for ((_, y, _), d) in cx.de("190") {
        let con_a: Vec<_> = d
            .registros
            .iter()
            .filter(|r| r.clave.as_deref().is_some_and(|c| c.starts_with('A')))
            .collect();
        let suyos: Vec<_> = con_a
            .iter()
            .filter(|r| suyo(r.nombre.as_deref(), r.nif.as_deref()))
            .collect();
        if suyos.is_empty() {
            continue;
        }
        if suyos.len() == con_a.len() {
            cobros.extend((1..=4u8).map(|q| (*y, q)));
        }
        let por_debajo_190 = suyos.iter().any(|r| {
            let tipo = r
                .importe
                .zip(r.retencion)
                .and_then(|(b, ret)| tipo_de(b, ret));
            por_debajo(tipo, *y)
        });
        if por_debajo_190 {
            pruebas.push(format!("el 190 de {y}, con la clave A"));
            ficheros.push(d.fichero.clone());
        }
    }
    // Nóminas de TaxPain, en una ficha que no está marcada como administrador.
    let empleados: Vec<_> = cx
        .exp
        .taxpain
        .empleados
        .iter()
        .filter(|e| suyo(Some(&e.nombre), Some(&e.nif)))
        .collect();
    let recibos: Vec<(Date, Money, Money)> = empleados
        .iter()
        .flat_map(|e| e.recibos.iter().copied())
        .filter(|(f, _, _)| despues(*f))
        .collect();
    cobros.extend(recibos.iter().map(|(f, _, _)| trim_de(*f)));
    let sin_recibos = empleados.iter().all(|e| e.recibos.is_empty());
    if !empleados.is_empty()
        && (sin_recibos
            || recibos
                .iter()
                .any(|(f, b, r)| por_debajo(tipo_de(*b, *r), f.year())))
    {
        pruebas.push("Nóminas de TaxPain, sin marcar como administrador".into());
    }
    if pruebas.is_empty() {
        return;
    }
    // Los pagos de nómina a su nombre en el banco.
    let (movs, _) = super::datos::movimientos(cx.exp);
    for m in movs.iter().filter(|m| {
        m.importe < Money::ZERO
            && despues(m.fecha)
            && m.fecha.year() >= revisable_desde(cx.hoy)
            && nomina_en_el_banco(&m.concepto, &nombre, nif.as_deref())
    }) {
        if cobros.insert(trim_de(m.fecha)) {
            ficheros.push(m.origen.clone());
        }
    }
    // Los trimestres que se pueden revisar, desde el nombramiento y con el plazo del 111 ya
    // pasado; y, de ellos, los que tienen algo que diga que cobró y los que quedan entre dos así.
    let primero = desde
        .map(trim_de)
        .filter(|t| *t > (revisable_desde(cx.hoy), 1))
        .unwrap_or((revisable_desde(cx.hoy), 1));
    let mut revisables = Vec::new();
    let mut t = primero;
    while t.0 <= cx.hoy.year() {
        if cx
            .plazo("111", t.0, &format!("{}T", t.1))
            .is_some_and(|p| p.due_on < cx.hoy)
        {
            revisables.push(t);
        }
        t = siguiente(t);
    }
    let con_cobro: Vec<Trim> = revisables
        .iter()
        .copied()
        .filter(|t| cobros.contains(t))
        .collect();
    let entre = |t: Trim| {
        con_cobro.first().is_some_and(|a| *a <= t) && con_cobro.last().is_some_and(|b| t <= *b)
    };
    // El 111 del trimestre en que lo nombraron lleva los meses de antes, salvo que empiece con él.
    let a_medias = desde.filter(|d| d.day() != 1 || !matches!(u8::from(d.month()), 1 | 4 | 7 | 10));
    let en = |f: Date, t: Trim| trim_de(f) == t;
    let trimestres: Vec<Trimestre> = revisables
        .iter()
        .copied()
        .filter_map(|t| {
            let cobro = cobros.contains(&t) || entre(t);
            let del_111 = cx
                .modelo("111", t.0, &format!("{}T", t.1))
                .filter(|d| cobro && d.c(1) == Money::from_euros(1, 0) && d.c(2) > Money::ZERO)
                .filter(|_| a_medias.is_none_or(|d| trim_de(d) != t));
            if let Some(d) = del_111 {
                // Lo de en especie va con el mismo tipo (art. 102.1 RIRPF), si es de un solo
                // perceptor.
                let especie = d.c(4) <= Money::from_euros(1, 0);
                return Some(Trimestre {
                    t,
                    base: d.c(2) + if especie { d.c(5) } else { Money::ZERO },
                    retenido: d.c(3) + if especie { d.c(6) } else { Money::ZERO },
                    fichero: Some(d.fichero.clone()),
                });
            }
            let del_trimestre: Vec<_> = recibos.iter().filter(|(f, _, _)| en(*f, t)).collect();
            if !del_trimestre.is_empty() {
                return Some(Trimestre {
                    t,
                    base: del_trimestre.iter().map(|r| r.1).sum(),
                    retenido: del_trimestre.iter().map(|r| r.2).sum(),
                    fichero: None,
                });
            }
            let con_importes: Vec<_> = nominas
                .iter()
                .filter_map(|(_, n)| Some((n.mes?, n.bruto?, n.retencion?)))
                .filter(|(f, _, _)| en(*f, t))
                .collect();
            (!con_importes.is_empty()).then(|| Trimestre {
                t,
                base: con_importes.iter().map(|r| r.1).sum(),
                retenido: con_importes.iter().map(|r| r.2).sum(),
                fichero: None,
            })
        })
        .collect();
    let calculados: BTreeSet<Trim> = trimestres.iter().map(|t| t.t).collect();
    let prescribe = |t: Trim| {
        cx.plazo("111", t.0, &format!("{}T", t.1))
            .and_then(|p| p.due_on.replace_year(p.due_on.year() + 4).ok())
    };
    // Con sueldo y sin calcular (sin el 111 de un solo perceptor ni nóminas con importes), de los
    // que aún se pueden reclamar.
    let sin_calcular: Vec<Trim> = revisables
        .iter()
        .copied()
        .filter(|t| (cobros.contains(t) || entre(*t)) && !calculados.contains(t))
        .filter(|t| prescribe(*t).is_some_and(|p| cx.hoy <= p))
        .collect();
    let mut filas = Vec::new();
    let mut abiertos = Vec::new();
    let mut total = Money::ZERO;
    let mut prescritos = 0;
    let mut sin_cifra = BTreeSet::new();
    for t in &trimestres {
        let tipo = match pequeña(t.t.0) {
            Some(false) => 35,
            Some(true) => 19,
            None => {
                sin_cifra.insert(t.t.0 - 1);
                19
            }
        };
        let debida = t.base.apply_rate(Rate::percent(tipo));
        let falta = debida - t.retenido;
        if falta <= UN_EURO {
            continue;
        }
        ficheros.extend(t.fichero.clone());
        let prescribe = prescribe(t.t);
        let prescrito = prescribe.is_some_and(|p| cx.hoy > p);
        filas.push(vec![
            nombre_trim(t.t),
            eur(t.base),
            eur(t.retenido),
            format!("{tipo} %"),
            eur(debida),
            eur(falta),
            match (prescribe, prescrito) {
                (Some(p), true) => format!("prescribió el {}", fecha(p)),
                (Some(p), false) => format!("hasta el {}", fecha(p)),
                (None, _) => String::new(),
            },
        ]);
        if prescrito {
            prescritos += 1;
        } else {
            total += falta;
            abiertos.push(format!(
                "{}: {} (al {tipo} %{})",
                nombre_trim(t.t),
                eur(falta),
                prescribe
                    .map(|p| format!("; Hacienda puede reclamarla hasta el {}", fecha(p)))
                    .unwrap_or_default()
            ));
        }
    }
    for t in &sin_calcular {
        filas.push(vec![
            nombre_trim(*t),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            "sin calcular".into(),
            prescribe(*t)
                .map(|p| format!("hasta el {}", fecha(p)))
                .unwrap_or_default(),
        ]);
    }
    filas.sort_by_key(|f| {
        // «1T 2025» → (2025, 1), para ordenar por fecha.
        let (q, y) = f[0].split_once("T ").unwrap_or_default();
        (y.parse::<i32>().unwrap_or(0), q.parse::<u8>().unwrap_or(0))
    });
    let cuando = vigente
        .fecha
        .map(|f| format!(" del {}", fecha(f)))
        .unwrap_or_default();
    let mut detalle = format!(
        "Según {}{cuando}, {nombre} es el administrador de la sociedad, y cobra como un \
         empleado: {}. Si lo que cobra es por el cargo de administrador, va en el 190 con la \
         clave E y una retención fija del 35 %, o del 19 % si la cifra de negocios del último \
         ejercicio cerrado antes del pago fue inferior a 100.000 € (art. 101.2 LIRPF y art. \
         80.1.3.º RIRPF), y los estatutos tienen que decir que el cargo es retribuido (art. 217 \
         LSC). La clave A sólo vale si hace otro trabajo, distinto del cargo, con su contrato. Es \
         dudoso: que lo decida la gestoría con los estatutos y el contrato.",
        minuscula(&vigente.documento),
        lista(&pruebas, 4)
    );
    let primero_sin_calcular = sin_calcular
        .first()
        .and_then(|t| prescribe(*t))
        .map(|p| format!(" El primero se puede reclamar hasta el {}.", fecha(p)))
        .unwrap_or_default();
    if abiertos.is_empty() && prescritos == 0 && sin_calcular.is_empty() {
        detalle.push_str(
            " Con los 111 de cada trimestre (o las nóminas con sus importes) se calcula cuánta \
             retención faltaría.",
        );
    } else {
        if !abiertos.is_empty() {
            detalle.push_str(&format!(
                " Si es por el cargo, la retención que falta por trimestre es: {}; en total, {}{}.",
                lista(&abiertos, 8),
                if sin_calcular.is_empty() {
                    ""
                } else {
                    "al menos "
                },
                eur(total)
            ));
        }
        if !sin_calcular.is_empty() {
            detalle.push_str(&format!(
                " {} {}: también cobra entonces (hay nóminas o pagos suyos de esos trimestres, o \
                 de antes y de después), pero no el 111 de un solo perceptor ni las nóminas con \
                 sus importes para calcularlo.{primero_sin_calcular}",
                if abiertos.is_empty() {
                    "Si es por el cargo, falta calcular la retención de"
                } else {
                    "Falta calcular"
                },
                rangos(&sin_calcular)
            ));
        }
        if !abiertos.is_empty() || !sin_calcular.is_empty() {
            detalle.push_str(
                " Se regulariza con una complementaria de cada 111 (con el recargo del art. 27 \
                 LGT) y corrigiendo el 190.",
            );
        }
        if prescritos > 0 {
            detalle.push_str(&format!(
                " {} ya no se {} reclamar (art. 66 LGT).",
                primera(&plural(
                    prescritos,
                    "trimestre anterior",
                    "trimestres anteriores"
                )),
                if prescritos == 1 { "puede" } else { "pueden" }
            ));
        }
        if !sin_cifra.is_empty() {
            let años: Vec<String> = sin_cifra.iter().map(i32::to_string).collect();
            detalle.push_str(&format!(
                " Se ha calculado al 19 % donde falta el 200 de {} para saber la cifra de \
                 negocios: si llegó a 100.000 €, es el 35 %.",
                años.join(", ")
            ));
        }
        if trimestres.iter().any(|t| t.fichero.is_some()) {
            detalle.push_str(
                " (Con el 111 sólo cuando tiene un único perceptor de rendimientos del trabajo y \
                 algo dice que es él; con lo que cobra en especie, art. 102.1 RIRPF.)",
            );
        }
    }
    ficheros.sort();
    ficheros.dedup();
    s.add(
        Nuevo {
            codigo: "cruces.administrador_clave_a",
            area: Area::Socios,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Gestoria,
            titulo: format!("{nombre} es el administrador y cobra como empleado (clave A)"),
            ficheros,
        }
        .detalle(detalle),
        trimestres
            .iter()
            .map(|t| t.t)
            .chain(sin_calcular.iter().copied())
            .max()
            .map(|t| t.0),
        None,
    );
    s.tabla(
        "Retención del administrador: clave A frente a clave E",
        "Administrador clave A",
        &[
            "Trimestre",
            "Rendimientos",
            "Retenido",
            "Tipo del cargo",
            "Retención del cargo",
            "Diferencia",
            "Prescripción",
        ],
        filas,
    );
    s.pedir(
        Destinatario::Gestoria,
        format!(
            "Decidir si lo que cobra {nombre} es por el cargo de administrador (clave E, con la \
             retención del 35 % o del 19 %) y, si lo es, regularizar los 111 y el 190 que aún no \
             han prescrito"
        ),
    );
}

/// Saldos de las cuentas 551 y 552 al cierre de cada ejercicio con contabilidad en TaxPain.
fn saldos(cx: &Contexto<'_>, s: &mut Salida) {
    let desde = revisable_desde(cx.hoy);
    let saldos: Vec<_> = cx
        .exp
        .taxpain
        .saldos_socios
        .iter()
        .filter(|x| x.ejercicio >= desde)
        .collect();
    s.tabla(
        "Cuentas con socios y administradores (551 y 552) al cierre",
        "Cuentas con socios",
        &[
            "Ejercicio",
            "Cuenta",
            "Nombre",
            "Saldo",
            "Quién debe a quién",
        ],
        saldos
            .iter()
            .map(|x| {
                vec![
                    x.ejercicio.to_string(),
                    x.cuenta.clone(),
                    x.nombre.clone(),
                    eur(x.saldo.abs()),
                    if x.saldo > Money::ZERO {
                        "debe a la sociedad".into()
                    } else {
                        "la sociedad le debe".into()
                    },
                ]
            })
            .collect(),
    );
    let años: BTreeSet<i32> = saldos.iter().map(|x| x.ejercicio).collect();
    for y in años {
        let deudores: Vec<_> = saldos
            .iter()
            .filter(|x| x.ejercicio == y && x.saldo > UN_EURO)
            .collect();
        if deudores.is_empty() {
            continue;
        }
        let total: Money = deudores.iter().map(|x| x.saldo).sum();
        let cuentas: Vec<String> = deudores
            .iter()
            .map(|x| {
                let de_antes = saldos
                    .iter()
                    .any(|a| a.ejercicio == y - 1 && a.cuenta == x.cuenta && a.saldo > UN_EURO);
                format!(
                    "{}{} {}{}",
                    x.cuenta,
                    if x.nombre.is_empty() { "" } else { " " },
                    x.nombre,
                    if de_antes {
                        format!(" ({}, y ya debía el año anterior)", eur(x.saldo))
                    } else {
                        format!(" ({})", eur(x.saldo))
                    }
                )
            })
            .collect();
        s.add(
            Nuevo {
                codigo: "cruces.socios_deudores",
                area: Area::Socios,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Empresa,
                titulo: format!(
                    "Socios o administradores deben {} a la sociedad al cierre de {y}",
                    eur(total)
                ),
                ficheros: vec![],
            }
            .detalle(format!(
                "{}. Es dinero de la sociedad prestado a un socio, a un administrador o a una \
                 parte vinculada: es una operación vinculada que tiene que hacerse a valor de \
                 mercado, con interés (art. 18 LIS); si no, Hacienda puede imputar los intereses a \
                 la sociedad y tratarlo como una retribución o un dividendo encubierto del socio. \
                 Qué hacer: que se devuelva, o firmar un contrato de préstamo con interés de \
                 mercado y contabilizar los intereses; pregunta a la gestoría.",
                primera(&lista(&cuentas, 6))
            )),
            Some(y),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Con el NIF en los dos lados manda el NIF: el mismo nombre con otro DNI es otra persona.
    #[test]
    fn la_misma_persona_por_el_nif() {
        let admin = "PEREZ GOMEZ JUAN (12345678Z), administrador único";
        assert!(es_persona(admin, "Juan Pérez", Some("12345678Z")));
        assert!(!es_persona(admin, "Juan Pérez Gómez", Some("00000000T")));
        assert!(es_persona(admin, "Juan Pérez Gómez", None));
        assert!(!es_persona(admin, "", Some("B12345674")));
        // Sin NIF en la escritura, por el nombre.
        assert!(es_persona(
            "Juan Pérez Gómez",
            "PEREZ GOMEZ, JUAN",
            Some("00000000T")
        ));
    }

    /// El nombre sin el NIF ni el cargo; «APELLIDOS, NOMBRE» se queda entero. Antes se cortaba
    /// en la primera coma y quedaban sólo los apellidos.
    #[test]
    fn el_nombre_sin_el_cargo() {
        assert_eq!(
            sin_cargo("PEREZ GOMEZ JUAN (12345678Z), administrador único"),
            "PEREZ GOMEZ JUAN"
        );
        assert_eq!(sin_cargo("PEREZ GOMEZ, JUAN"), "PEREZ GOMEZ, JUAN");
        assert_eq!(
            sin_cargo("PEREZ GOMEZ, JUAN (12345678Z)"),
            "PEREZ GOMEZ, JUAN"
        );
        assert_eq!(
            sin_cargo("PEREZ GOMEZ, JUAN, administrador único"),
            "PEREZ GOMEZ, JUAN"
        );
        assert_eq!(
            sin_cargo("Ana García López, administradora solidaria"),
            "Ana García López"
        );
        assert_eq!(
            sin_cargo("PEREZ GOMEZ JUAN, 12345678Z, consejero delegado"),
            "PEREZ GOMEZ JUAN"
        );
    }

    #[test]
    fn nombres_de_administrador() {
        assert!(es("Doña María López García", "LÓPEZ GARCÍA, MARÍA", None));
        assert!(es("B12345674 EMPRESA DEMO SL", "Otra", Some("B12345674")));
        assert!(!es(
            "Juan Pérez Gómez",
            "María López García",
            Some("12345678Z")
        ));
    }
}

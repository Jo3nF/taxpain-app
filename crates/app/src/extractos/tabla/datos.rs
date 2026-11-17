//! Los datos de la cuenta que van encima y debajo de la tabla de movimientos: el IBAN o el
//! número de cuenta, la tarjeta, el titular, la divisa, el periodo y los saldos.
//!
//! Vienen de muchas formas: la etiqueta en una celda y el valor en la siguiente («Titular» |
//! «EMPRESA DEMO SL»), los dos en la misma celda («Número de cuenta: 0000/00.0000000»), varias
//! parejas en una celda («Saldo actual: 1,00 € Retenciones: 0,00 €»), o sin etiqueta («ES91
//! 2100 0418 4502 0005 1332 - EMPRESA DEMO SL.», «Fecha de 01/12/2024 hasta 15/01/2025»).

use std::sync::LazyLock;

use regex::Regex;
use taxpain_core::money::Money;
use time::Date;

use super::super::Clase;
use super::super::iban::{self, Enmascarada};
use super::bancos;
use super::diccionario::{Etiqueta, etiqueta_al_final, etiqueta_al_inicio};
use super::texto::{fechas_en, palabras, primer_importe, trozos};

/// Lo que se sabe de la cuenta por lo que hay alrededor de la tabla.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Datos {
    pub iban: Option<String>,
    /// Oficina y número de cuenta (diez cifras) de una cuenta española que no dice la entidad
    /// («0000/00.0000000»): con el banco se saca el IBAN.
    pub parcial: Option<(String, String)>,
    pub enmascarada: Option<Enmascarada>,
    /// Otro número de cuenta, tal cual.
    pub numero: Option<String>,
    /// Últimas cifras de la tarjeta.
    pub tarjeta: Option<String>,
    pub es_tarjeta: bool,
    pub clase: Option<Clase>,
    /// Código de entidad español, si lo dice un número de contrato.
    pub entidad: Option<String>,
    pub titular: Option<String>,
    pub divisa: Option<String>,
    pub desde: Option<Date>,
    pub hasta: Option<Date>,
    /// Cuándo se sacó el extracto.
    pub generado: Option<Date>,
    pub saldo_inicial: Option<Money>,
    pub saldo_final: Option<Money>,
    /// Saldo en el momento de sacar el extracto, con su rango (ver
    /// [`Etiqueta::SaldoActual`]).
    pub saldo_actual: Option<(u8, Money)>,
    pub banco: Option<&'static str>,
    /// El banco lo dice una etiqueta («Banco:») o la huella de su descarga, no un nombre suelto
    /// en el texto (que puede ser una ciudad: «39001 Santander»).
    pub banco_seguro: bool,
}

impl Datos {
    /// ¿Dice de qué cuenta o tarjeta es?
    pub(super) fn tiene_cuenta(&self) -> bool {
        self.iban.is_some()
            || self.parcial.is_some()
            || self.enmascarada.is_some()
            || self.numero.is_some()
            || self.tarjeta.is_some()
    }

    /// Completa lo que falta con lo de `otro` (los datos de debajo de la tabla, por ejemplo).
    pub(super) fn completar(&mut self, otro: &Datos) {
        macro_rules! si_falta {
            ($($c:ident),*) => {
                $(if self.$c.is_none() {
                    self.$c = otro.$c.clone();
                })*
            };
        }
        si_falta!(
            iban,
            parcial,
            enmascarada,
            numero,
            tarjeta,
            clase,
            entidad,
            titular,
            divisa,
            desde,
            hasta,
            generado,
            saldo_inicial,
            saldo_final
        );
        if self.banco.is_none() {
            self.banco = otro.banco;
            self.banco_seguro = otro.banco_seguro;
        }
        self.es_tarjeta |= otro.es_tarjeta;
        if let Some((r, m)) = otro.saldo_actual
            && self.saldo_actual.is_none_or(|(r0, _)| r < r0)
        {
            self.saldo_actual = Some((r, m));
        }
    }

    /// La cuenta de `previo` (la tabla anterior del mismo fichero), para una tabla que no dice
    /// la suya: una hoja partida en páginas, cada una con sus títulos.
    pub(super) fn heredar(&mut self, previo: &Datos) {
        if self.tiene_cuenta() {
            if self.banco.is_none() {
                self.banco = previo.banco;
                self.banco_seguro = previo.banco_seguro;
            }
            return;
        }
        self.iban.clone_from(&previo.iban);
        self.parcial.clone_from(&previo.parcial);
        self.enmascarada.clone_from(&previo.enmascarada);
        self.numero.clone_from(&previo.numero);
        self.tarjeta.clone_from(&previo.tarjeta);
        self.es_tarjeta |= previo.es_tarjeta;
        if self.clase.is_none() {
            self.clase = previo.clase;
        }
        if self.entidad.is_none() {
            self.entidad.clone_from(&previo.entidad);
        }
        if self.titular.is_none() {
            self.titular.clone_from(&previo.titular);
        }
        if self.divisa.is_none() {
            self.divisa.clone_from(&previo.divisa);
        }
        if self.banco.is_none() {
            self.banco = previo.banco;
            self.banco_seguro = previo.banco_seguro;
        }
    }
}

/// Código de divisa de un valor («EUR», «usd», «€», «Euros»).
pub(super) fn divisa(v: &str) -> Option<String> {
    let t = v.trim();
    match t {
        "€" => return Some("EUR".to_owned()),
        "$" => return Some("USD".to_owned()),
        "£" => return Some("GBP".to_owned()),
        _ => {}
    }
    let p = palabras(t);
    let w = p.first()?;
    match w.as_str() {
        "euro" | "euros" => Some("EUR".to_owned()),
        "dolares" | "dollars" | "dolar" | "dollar" => Some("USD".to_owned()),
        "libras" | "pounds" => Some("GBP".to_owned()),
        w if w.len() == 3 && w.bytes().all(|b| b.is_ascii_alphabetic()) && p.len() <= 2 => {
            Some(w.to_uppercase())
        }
        _ => None,
    }
}

/// Últimas cifras de una tarjeta («540000******1234», «5400 0000 0000 1234», «****1234»).
fn ultimas(v: &str) -> Option<String> {
    if let Some(m) = iban::enmascarada(v) {
        return Some(m.ultimos);
    }
    let d: String = v.chars().filter(char::is_ascii_digit).collect();
    (d.len() >= 4).then(|| d[d.len() - 4..].to_owned())
}

/// El titular tal cual, sin el punto del final ni envoltorios («[object Object] (EMPRESA)»).
fn titular(v: &str) -> Option<String> {
    let mut t = v.trim();
    if let Some(r) = t.strip_prefix("[object Object]") {
        t = r.trim();
    }
    if let Some(r) = t.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        t = r.trim();
    }
    let t = t.trim_end_matches('.').trim();
    (!t.is_empty() && t.chars().any(char::is_alphabetic)).then(|| t.to_owned())
}

/// El número de cuenta de un valor.
fn cuenta(d: &mut Datos, v: &str) {
    static PARCIAL: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"^(\d{4})\s*[/ -]\s*(\d[\d .-]{7,12}\d)$").ok());
    let v = v.trim();
    if v.is_empty() {
        return;
    }
    if let Some(i) = iban::en_texto(v).into_iter().next() {
        d.iban.get_or_insert(i);
        return;
    }
    if let Some(i) = iban::desde_ccc(v) {
        d.iban.get_or_insert(i);
        return;
    }
    if let Some(m) = iban::enmascarada(v) {
        d.enmascarada.get_or_insert(m);
        return;
    }
    if let Some(c) = PARCIAL.as_ref().and_then(|re| re.captures(v))
        && let (Some(of), Some(num)) = (c.get(1), c.get(2))
    {
        let n: String = num.as_str().chars().filter(char::is_ascii_digit).collect();
        if (9..=10).contains(&n.len()) {
            d.parcial
                .get_or_insert((of.as_str().to_owned(), format!("{n:0>10}")));
            return;
        }
    }
    let compacto = iban::compactar(v);
    if compacto.bytes().filter(u8::is_ascii_digit).count() >= 6 {
        d.numero.get_or_insert(compacto);
    }
}

/// Lo que dice una etiqueta con su valor. Un saldo cuyo valor no es un importe («Beginning
/// balance as of 01/01/2025») puede tenerlo en la celda `siguiente`.
fn aplicar(d: &mut Datos, e: Etiqueta, v: &str, siguiente: &str, con_periodo: bool) {
    let v = v.trim();
    let importe_ = || primer_importe(v).or_else(|| primer_importe(siguiente));
    match e {
        Etiqueta::Cuenta => cuenta(d, v),
        Etiqueta::Tarjeta => {
            if let Some(u) = ultimas(v) {
                d.es_tarjeta = true;
                d.tarjeta.get_or_insert(u);
            }
        }
        Etiqueta::TipoTarjeta => {
            let p = palabras(v);
            let hay = |ws: &[&str]| p.iter().any(|w| ws.contains(&w.as_str()));
            if hay(&["credito", "credit", "kredit", "credit card"]) {
                d.clase.get_or_insert(Clase::TarjetaCredito);
                d.es_tarjeta = true;
            } else if hay(&["debito", "debit"]) {
                d.clase.get_or_insert(Clase::TarjetaDebito);
                d.es_tarjeta = true;
            }
        }
        Etiqueta::Contrato => {
            let n: String = v.chars().filter(char::is_ascii_digit).collect();
            if n.len() >= 20
                && let Some(ent) = n.get(..4)
                && iban::nombre_entidad(ent).is_some()
            {
                d.entidad.get_or_insert(ent.to_owned());
            }
        }
        Etiqueta::Titular => {
            if d.titular.is_none() {
                d.titular = titular(v);
            }
        }
        Etiqueta::Divisa => {
            if d.divisa.is_none() {
                d.divisa = divisa(v);
            }
        }
        Etiqueta::Periodo => {
            let f = fechas_en(v);
            if let [a, b, ..] = f.as_slice() {
                d.desde.get_or_insert(*a.min(b));
                d.hasta.get_or_insert(*a.max(b));
            }
        }
        Etiqueta::Desde => {
            if let Some(f) = fechas_en(v).first() {
                d.desde.get_or_insert(*f);
            }
        }
        Etiqueta::Hasta => {
            if let Some(f) = fechas_en(v).first() {
                d.hasta.get_or_insert(*f);
            }
        }
        Etiqueta::Generado => {
            if !con_periodo && let [f] = fechas_en(v).as_slice() {
                d.generado.get_or_insert(*f);
            }
        }
        Etiqueta::Banco => {
            if d.banco.is_none() {
                d.banco = bancos::por_nombre(v);
                d.banco_seguro = d.banco.is_some();
            }
        }
        Etiqueta::SaldoInicial => {
            if d.saldo_inicial.is_none() {
                d.saldo_inicial = importe_();
            }
        }
        Etiqueta::SaldoFinal => {
            if d.saldo_final.is_none() {
                d.saldo_final = importe_();
            }
        }
        Etiqueta::SaldoActual(r) => {
            if let Some(m) = importe_()
                && d.saldo_actual.is_none_or(|(r0, _)| r < r0)
            {
                d.saldo_actual = Some((r, m));
            }
        }
        Etiqueta::Otra => {}
    }
}

/// Las parejas de etiqueta y valor de una celda con dos puntos: «Número de cuenta: 0000»,
/// «Saldo actual: 1,00 € Retenciones: 0,00 € Saldo disponible: 1,00 €». La etiqueta de cada
/// valor son las últimas palabras del trozo de antes de sus dos puntos.
fn pares(celda: &str) -> Vec<(Etiqueta, String)> {
    let trozos_: Vec<&str> = celda.split(':').collect();
    if trozos_.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let primero = palabras(trozos_[0]);
    let mut actual = etiqueta_al_final(&primero)
        .or_else(|| etiqueta_al_inicio(&primero))
        .map(|(e, _)| e);
    for (k, t) in trozos_.iter().enumerate().skip(1) {
        let (valor, siguiente) = if k + 1 < trozos_.len() {
            let w = trozos(t);
            let p: Vec<String> = w.iter().map(|(_, _, p)| p.clone()).collect();
            match etiqueta_al_final(&p) {
                Some((e, n)) => {
                    let ini = w.get(w.len() - n).map_or(t.len(), |x| x.0);
                    (&t[..ini], Some(e))
                }
                None => (*t, None),
            }
        } else {
            (*t, None)
        };
        if let Some(e) = actual {
            out.push((e, valor.trim().to_owned()));
        }
        actual = siguiente;
    }
    out
}

/// Las etiquetas de una celda con lo que dicen (vacío si el valor está en la celda de al
/// lado).
fn etiquetas(celda: &str) -> Vec<(Etiqueta, String)> {
    if celda.contains(':') {
        let p = pares(celda);
        if !p.is_empty() {
            return p;
        }
    }
    let w = trozos(celda);
    let p: Vec<String> = w.iter().map(|(_, _, p)| p.clone()).collect();
    match etiqueta_al_inicio(&p) {
        Some((e, n)) => {
            let resto = w.get(n).map_or("", |x| &celda[x.0..]);
            vec![(e, resto.trim().to_owned())]
        }
        None => Vec::new(),
    }
}

/// ¿Dice la fila un periodo («Periodo: 01/01/2025 - 31/03/2025», «Del 1 al 31…»)?
fn periodo(texto: &str, fechas: &[Date]) -> Option<(Date, Date)> {
    let [a, b, ..] = fechas else {
        return None;
    };
    let p = palabras(texto);
    let pista = p.iter().any(|w| {
        matches!(
            w.as_str(),
            "periodo"
                | "period"
                | "desde"
                | "hasta"
                | "del"
                | "al"
                | "from"
                | "to"
                | "between"
                | "entre"
                | "zeitraum"
                | "vom"
                | "von"
                | "bis"
                | "du"
                | "au"
                | "dal"
                | "fechas"
                | "range"
                | "consulta"
                | "intervalo"
                | "periode"
        )
    }) || texto.contains(" - ")
        || texto.contains('–');
    pista.then(|| (*a.min(b), *a.max(b)))
}

/// ¿Dice la fila de qué cuenta es (un IBAN, una cuenta enmascarada o una etiqueta de cuenta,
/// tarjeta, titular o divisa)? Sirve para saber dónde empiezan los datos de la tabla siguiente.
pub(super) fn identifica(fila: &[String]) -> bool {
    let texto = fila
        .iter()
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if !iban::en_texto(&texto).is_empty() {
        return true;
    }
    fila.iter().any(|c| {
        matches!(
            etiqueta_al_inicio(&palabras(c)),
            Some((
                Etiqueta::Cuenta | Etiqueta::Tarjeta | Etiqueta::Titular | Etiqueta::Divisa,
                _
            ))
        )
    })
}

/// Los datos de la cuenta de estas filas.
pub(super) fn leer<'a>(filas: impl IntoIterator<Item = &'a [String]>) -> Datos {
    let mut d = Datos::default();
    let mut todo = String::new();
    for fila in filas {
        let celdas: Vec<&str> = fila
            .iter()
            .map(|c| c.trim())
            .filter(|c| !c.is_empty())
            .collect();
        if celdas.is_empty() {
            continue;
        }
        let texto = celdas.join(" ");
        todo.push_str(&texto);
        todo.push('\n');
        if d.iban.is_none()
            && let Some(i) = iban::en_texto(&texto).into_iter().next()
        {
            d.iban = Some(i);
        }
        // «ES91 2100 0418 4502 0005 1332 - EMPRESA DEMO SL.»
        for c in &celdas {
            if d.titular.is_none()
                && let Some((antes, despues)) = c.split_once(" - ")
                && !iban::en_texto(antes).is_empty()
            {
                d.titular = titular(despues);
            }
        }
        let fechas = fechas_en(&texto);
        let con_periodo = match periodo(&texto, &fechas) {
            Some((a, b)) => {
                d.desde.get_or_insert(a);
                d.hasta.get_or_insert(b);
                true
            }
            None => false,
        };
        for (i, c) in celdas.iter().enumerate() {
            let siguiente = celdas.get(i + 1).copied().unwrap_or("");
            let lista = etiquetas(c);
            let n = lista.len();
            for (k, (e, v)) in lista.into_iter().enumerate() {
                // El valor de la última etiqueta de la celda puede ir en la celda siguiente.
                let ultima = k + 1 == n;
                let v = if v.is_empty() && ultima {
                    siguiente.to_owned()
                } else {
                    v
                };
                aplicar(
                    &mut d,
                    e,
                    &v,
                    if ultima { siguiente } else { "" },
                    con_periodo,
                );
            }
        }
        // Un listado de tarjeta lo dice su título («Listado de movimientos de tarjeta», «Credit
        // card statement»), no una mención suelta en un pie («consulte los movimientos de su
        // tarjeta»): por esa mención una cuenta corriente acababa en la 5201 en vez de la 572.
        for c in &celdas {
            let p = palabras(c);
            if titulo_de_tarjeta(&p) {
                d.es_tarjeta = true;
                let hay = |ws: &[&str]| p.iter().any(|w| ws.contains(&w.as_str()));
                if hay(&["credito", "credit", "kreditkarte"]) {
                    d.clase.get_or_insert(Clase::TarjetaCredito);
                } else if hay(&["debito", "debit"]) {
                    d.clase.get_or_insert(Clase::TarjetaDebito);
                }
            }
        }
    }
    if d.banco.is_none() {
        if let Some(b) = bancos::por_huella(&todo) {
            d.banco = Some(b);
            d.banco_seguro = true;
        } else {
            d.banco = bancos::por_nombre(&todo);
        }
    }
    if d.es_tarjeta && d.tarjeta.is_none() {
        // Un IBAN enmascarado («ES91 **** 1332») es una cuenta, no el número de la tarjeta.
        d.tarjeta = d
            .enmascarada
            .as_ref()
            .filter(|m| m.prefijo.is_none())
            .map(|m| m.ultimos.clone());
    }
    d
}

/// ¿Son las palabras de una celda el título de un listado de tarjeta («Listado de movimientos
/// de tarjeta», «Credit card statement», «Tarjetas de crédito»)? Un título es corto y no habla
/// de la cuenta: ni «Consulta de movimientos de cuenta y tarjeta» ni un pie largo que menciona
/// la tarjeta lo son.
fn titulo_de_tarjeta(p: &[String]) -> bool {
    let hay = |ws: &[&str]| p.iter().any(|w| ws.contains(&w.as_str()));
    p.len() <= 8
        && hay(&[
            "tarjeta",
            "tarjetas",
            "targeta",
            "targetes",
            "card",
            "cards",
            "carte",
            "karte",
            "kreditkarte",
        ])
        && !hay(&["cuenta", "cuentas", "compte", "account", "konto", "conto"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn filas(v: &[&[&str]]) -> Vec<Vec<String>> {
        v.iter()
            .map(|f| f.iter().map(|s| (*s).to_owned()).collect())
            .collect()
    }

    fn leer_(v: &[&[&str]]) -> Datos {
        let f = filas(v);
        leer(f.iter().map(Vec::as_slice))
    }

    #[test]
    fn etiqueta_y_valor_en_celdas_seguidas() {
        let d = leer_(&[
            &["Operación:", "Consulta de movimientos"],
            &["Número de cuenta:", "2100 0418 45 0200051332"],
            &["Titular:", "EMPRESA DEMO SL"],
            &["Apoderado:", "OTRA PERSONA"],
            &["Periodo de consulta:", "01/01/2019 - 31/03/2019"],
        ]);
        assert_eq!(d.iban.as_deref(), Some("ES9121000418450200051332"));
        assert_eq!(d.titular.as_deref(), Some("EMPRESA DEMO SL"));
        assert_eq!(
            (d.desde, d.hasta),
            (Some(date!(2019 - 01 - 01)), Some(date!(2019 - 03 - 31)))
        );
        assert!(!d.es_tarjeta);
    }

    #[test]
    fn varias_parejas_en_una_celda_e_iban_con_titular() {
        let d = leer_(&[
            &["Banca March S.A. - https://telemarch.bancamarch.es"],
            &["Cuentas a la vista - Extracto de movimientos - mié, 15 ene 2025, 10:20 CET"],
            &["Fecha de 01/12/2024 hasta 15/01/2025"],
            &["ES91 2100 0418 4502 0005 1332 - EMPRESA DEMO SL."],
            &["Saldo actual: 1.234,56 € Retenciones: 3,10 € Saldo disponible: 1.231,46 €"],
        ]);
        assert_eq!(d.banco, Some("Banca March"));
        assert_eq!(d.iban.as_deref(), Some("ES9121000418450200051332"));
        assert_eq!(d.titular.as_deref(), Some("EMPRESA DEMO SL"));
        assert_eq!(
            (d.desde, d.hasta),
            (Some(date!(2024 - 12 - 01)), Some(date!(2025 - 01 - 15)))
        );
        assert_eq!(d.saldo_actual, Some((1, Money::from_cents(123_456))));
    }

    #[test]
    fn cuenta_sin_entidad_tarjeta_y_saldos() {
        let d = leer_(&[
            &["Número de cuenta: 0000/10.0000001", "", ""],
            &["Empresa: EMPRESA DEMO SL"],
        ]);
        assert_eq!(
            d.parcial,
            Some(("0000".to_owned(), "0100000001".to_owned()))
        );
        assert_eq!(d.titular.as_deref(), Some("EMPRESA DEMO SL"));
        let d = leer_(&[
            &["", "", "Listado de movimientos de tarjeta"],
            &["", "", "Nº de tarjeta", "", "5400000000001234"],
            &["", "", "Tipo de tarjeta", "", "TARJETA NEGOCIOS CREDITO"],
            &["", "", "Nº de contrato", "", "01820000000000000000001234"],
            &["", "", "Fecha", "", "30/06/2026"],
            &["", "", "Periodo", "", "30/06/2024 - 30/06/2026"],
        ]);
        assert!(d.es_tarjeta);
        assert_eq!(d.tarjeta.as_deref(), Some("1234"));
        assert_eq!(d.clase, Some(Clase::TarjetaCredito));
        assert_eq!(d.entidad.as_deref(), Some("0182"));
        assert_eq!(d.generado, Some(date!(2026 - 06 - 30)));
        assert_eq!(d.desde, Some(date!(2024 - 06 - 30)));
        let d = leer_(&[
            &["Kontonummer:", "DE89 3704 0044 0532 0130 00"],
            &["Von:", "01.01.2025"],
            &["Bis:", "31.01.2025"],
            &["Anfangssaldo:", "1.000,00 EUR"],
            &["Kontostand vom 31.01.2025:", "1.234,56 EUR"],
            &["Währung:", "EUR"],
        ]);
        assert_eq!(d.iban.as_deref(), Some("DE89370400440532013000"));
        assert_eq!(
            (d.desde, d.hasta),
            (Some(date!(2025 - 01 - 01)), Some(date!(2025 - 01 - 31)))
        );
        assert_eq!(d.saldo_inicial, Some(Money::from_cents(100_000)));
        assert_eq!(d.saldo_actual, Some((2, Money::from_cents(123_456))));
        assert_eq!(d.divisa.as_deref(), Some("EUR"));
    }

    #[test]
    fn la_tarjeta_la_dice_el_titulo_no_una_mencion() {
        let d = leer_(&[
            &["Consulta de movimientos de cuenta y tarjeta"],
            &["Cuenta:", "ES91 2100 0418 4502 0005 1332"],
            &["Para más información sobre su tarjeta consulte la app"],
        ]);
        assert!(!d.es_tarjeta);
        assert_eq!(d.clase, None);
        let d = leer_(&[&["", "Extracto de tarjeta de crédito"]]);
        assert!(d.es_tarjeta);
        assert_eq!(d.clase, Some(Clase::TarjetaCredito));
        // Un IBAN enmascarado no es el número de la tarjeta.
        let d = leer_(&[
            &["Tarjetas y cuentas"],
            &["Cuenta:", "ES91 **** **** **** **** 1332"],
            &["Movimientos de tarjeta"],
        ]);
        assert!(d.es_tarjeta);
        assert_eq!(d.tarjeta, None);
    }
}

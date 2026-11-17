//! El banco de un extracto por su huella: el nombre que aparece en los datos de encima o de
//! debajo de la tabla («Banca March S.A.», «BANCO BILBAO VIZCAYA ARGENTARIA»), unas etiquetas
//! propias de su descarga o los títulos de las columnas de su CSV.

use super::super::iban;
use super::texto::palabras;

/// Nombres (en palabras sin tildes) que delatan al banco o la entidad de pago.
const NOMBRES: &[(&[&str], &str)] = &[
    (&["banco", "bilbao", "vizcaya"], "BBVA"),
    (&["bbva"], "BBVA"),
    (&["bankinter"], "Bankinter"),
    (&["banca", "march"], "Banca March"),
    (&["bancamarch"], "Banca March"),
    (&["telemarch"], "Banca March"),
    (&["ing", "direct"], "ING"),
    (&["ingdirect"], "ING"),
    (&["ing", "bank"], "ING"),
    (&["ing", "diba"], "ING"),
    // «Santander» y «Sabadell» solos son también ciudades (y apellidos): hace falta el nombre
    // entero o la huella de su descarga.
    (&["banco", "santander"], "Banco Santander"),
    (&["bancosantander"], "Banco Santander"),
    (&["openbank"], "Openbank"),
    (&["caixabank"], "CaixaBank"),
    (&["la", "caixa"], "CaixaBank"),
    (&["banco", "sabadell"], "Banco Sabadell"),
    (&["banc", "sabadell"], "Banco Sabadell"),
    (&["bancsabadell"], "Banco Sabadell"),
    (&["kutxabank"], "Kutxabank"),
    (&["ibercaja"], "Ibercaja"),
    (&["unicaja"], "Unicaja"),
    (&["abanca"], "Abanca"),
    (&["cajamar"], "Cajamar"),
    (&["laboral", "kutxa"], "Laboral Kutxa"),
    (&["caja", "rural"], "Caja Rural"),
    (&["caixa", "d", "enginyers"], "Caixa d'Enginyers"),
    (&["evo", "banco"], "EVO Banco"),
    (&["triodos"], "Triodos Bank"),
    (&["mediolanum"], "Banco Mediolanum"),
    (&["deutsche", "bank"], "Deutsche Bank"),
    (&["myinvestor"], "MyInvestor"),
    (&["wizink"], "WiZink"),
    (&["revolut"], "Revolut"),
    (&["transferwise"], "Wise"),
    (&["wise"], "Wise"),
    (&["n26"], "N26"),
    (&["qonto"], "Qonto"),
    (&["bunq"], "bunq"),
    (&["monzo"], "Monzo"),
    (&["starling"], "Starling Bank"),
    (&["paypal"], "PayPal"),
    (&["stripe"], "Stripe"),
    (&["hsbc"], "HSBC"),
    (&["barclays"], "Barclays"),
    (&["lloyds"], "Lloyds Bank"),
    (&["natwest"], "NatWest"),
    (&["bnp", "paribas"], "BNP Paribas"),
    (&["societe", "generale"], "Société Générale"),
    (&["credit", "agricole"], "Crédit Agricole"),
    (&["commerzbank"], "Commerzbank"),
    (&["sparkasse"], "Sparkasse"),
    (&["postbank"], "Postbank"),
    (&["rabobank"], "Rabobank"),
    (&["abn", "amro"], "ABN AMRO"),
    (&["unicredit"], "UniCredit"),
    (&["intesa", "sanpaolo"], "Intesa Sanpaolo"),
    (&["american", "express"], "American Express"),
];

/// ¿Aparecen estas palabras seguidas?
fn seguidas(p: &[String], buscadas: &[&str]) -> bool {
    p.windows(buscadas.len())
        .any(|w| w.iter().zip(buscadas).all(|(a, b)| a == b))
}

/// El banco que nombran los datos de la cuenta (las filas de encima y de debajo de la tabla).
/// Un nombre suelto en el texto puede ser otra cosa (una dirección en Santander): no basta para
/// fabricar el IBAN de una cuenta parcial.
pub(super) fn por_nombre(texto: &str) -> Option<&'static str> {
    let p = palabras(texto);
    NOMBRES
        .iter()
        .find(|(n, _)| seguidas(&p, n))
        .map(|(_, b)| *b)
}

/// El banco por las etiquetas propias de su descarga, que sí lo identifican con seguridad: la
/// de Bankinter acaba con «Abonos pendientes» y «Total retenciones».
pub(super) fn por_huella(texto: &str) -> Option<&'static str> {
    let p = palabras(texto);
    if seguidas(&p, &["abonos", "pendientes"]) && seguidas(&p, &["total", "retenciones"]) {
        return Some("Bankinter");
    }
    None
}

/// El banco por los títulos de las columnas de su CSV (las claves de [`super::texto::clave`]).
pub(super) fn por_titulos(titulos: &[String]) -> Option<&'static str> {
    let hay = |t: &str| titulos.iter().any(|x| x == t);
    if hay("transferwise id") {
        return Some("Wise");
    }
    if hay("started date") && hay("completed date") {
        return Some("Revolut");
    }
    if hay("partner iban") || hay("partner name") || (hay("payee") && hay("transaction type")) {
        return Some("N26");
    }
    if hay("settlement date utc") || hay("operation date utc") {
        return Some("Qonto");
    }
    if hay("gross") && hay("net") && (hay("from email address") || hay("to email address")) {
        return Some("PayPal");
    }
    if hay("available on utc") || hay("available on") {
        return Some("Stripe");
    }
    None
}

/// ¿Es un monedero o una pasarela de pago, no una cuenta de banco?
pub(super) fn es_monedero(banco: &str) -> bool {
    matches!(banco, "PayPal" | "Stripe")
}

/// Códigos de entidad españoles que conoce [`iban::nombre_entidad`].
const ENTIDADES: [&str; 22] = [
    "0019", "0049", "0061", "0073", "0075", "0081", "0128", "0182", "0186", "0239", "1465", "1491",
    "1583", "2038", "2080", "2085", "2095", "2100", "2103", "3025", "3035", "3058",
];

/// Código de entidad de un banco español por su nombre.
pub(super) fn entidad(banco: &str) -> Option<&'static str> {
    ENTIDADES
        .iter()
        .find(|c| iban::nombre_entidad(c) == Some(banco))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huellas_de_los_bancos() {
        assert_eq!(
            por_nombre("Banco BANCO BILBAO VIZCAYA ARGENTARIA S.A."),
            Some("BBVA")
        );
        assert_eq!(
            por_nombre("Banca March S.A. - https://telemarch.bancamarch.es"),
            Some("Banca March")
        );
        assert_eq!(
            por_huella("Saldo total 1 Abonos pendientes 0 Total retenciones 0"),
            Some("Bankinter")
        );
        assert_eq!(por_nombre("Saldo total 1 Abonos pendientes 0"), None);
        // «ING» suelto es una palabra de muchas cosas: hace falta el nombre entero.
        assert_eq!(por_nombre("Consulta de movimientos ING"), None);
        // «Santander» y «Sabadell» solos son ciudades.
        assert_eq!(
            por_nombre("EMPRESA DEMO SL - Calle Falsa 1 - 39001 Santander"),
            None
        );
        assert_eq!(por_nombre("Polígono Demo, 08201 Sabadell"), None);
        assert_eq!(por_nombre("Banco Santander, S.A."), Some("Banco Santander"));
        assert_eq!(por_nombre("Banc Sabadell"), Some("Banco Sabadell"));
        assert_eq!(
            por_nombre("https://www.bancsabadell.com"),
            Some("Banco Sabadell")
        );
        assert_eq!(por_nombre("Otherwise nothing"), None);
        let t = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            por_titulos(&t(&["type", "started date", "completed date", "amount"])),
            Some("Revolut")
        );
        assert_eq!(entidad("Bankinter"), Some("0128"));
        assert_eq!(entidad("BBVA"), Some("0182"));
        assert_eq!(entidad("Revolut"), Some("1583"));
        assert_eq!(entidad("Wise"), None);
    }
}

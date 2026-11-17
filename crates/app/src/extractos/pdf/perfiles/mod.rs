//! Bancos cuya maqueta de PDF se conoce.
//!
//! Cada perfil reconoce su banco por el contenido del texto (nunca por el nombre del fichero) y
//! lo lee con sus reglas: dónde están los datos de la cuenta, cómo parte el lector de PDF cada
//! movimiento en líneas, qué cabeceras, pies de página y resúmenes hay que saltarse y de dónde
//! sale el signo de cada importe. Si ningún perfil reconoce el texto, lo lee el lector genérico.

mod banca_march;
mod bankinter;
mod bbva;
mod bbva_tarjeta;
mod comun;
mod paypal;
mod revolut;

use crate::extractos::Extracto;

/// Un perfil: los extractos del texto, o `None` si no es de su banco.
type Perfil = fn(&str) -> Option<Vec<Extracto>>;

const PERFILES: [Perfil; 6] = [
    banca_march::leer,
    bbva::leer,
    bbva_tarjeta::leer,
    revolut::leer,
    bankinter::leer,
    paypal::leer,
];

/// Extractos del texto de un PDF, si es de un banco conocido. `None` si no lo es, o si el
/// perfil lo reconoce pero no saca de él ni un movimiento ni un saldo (entonces lo intenta el
/// lector genérico). El nombre del fichero no se usa: el banco se reconoce por el contenido.
pub(crate) fn leer(texto: &str, _nombre: &str) -> Option<Vec<Extracto>> {
    let texto = texto.replace("\r\n", "\n");
    let mut v = PERFILES.iter().find_map(|p| {
        p(&texto).filter(|v| {
            v.iter()
                .any(|e| !e.movimientos.is_empty() || e.saldo_final.is_some())
        })
    })?;
    for e in v.iter_mut().filter(|e| e.cuenta.is_empty()) {
        e.avisos.push(format!(
            "No se ha encontrado el número de la cuenta en el extracto de {}: hay que decir a \
             qué cuenta corresponde.",
            e.banco
        ));
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn otros_textos_no_son_de_ningun_banco_conocido() {
        for t in [
            "",
            "Factura 2025-001\nEMPRESA DEMO SL\nTotal 121,00 €",
            "\n\nExtracto de movimientos\n\n01/03/2025 01/03/2025 RECIBO -10,00 € 100,00 €",
            "Revolut Bank UAB\nDeclaración de titularidad real de EMPRESA DEMO SL",
            "Banca March - https://telemarch.bancamarch.es\n\nCertificado de titularidad",
            "Listado de movimientos\n\n2025-01-01 COMPRA -1,00 EUR",
            "PayPal (Europe) S.à r.l. et Cie\nRecibo de pago",
        ] {
            assert_eq!(leer(t, "extracto.pdf"), None, "{t}");
        }
    }

    #[test]
    fn avisa_si_no_encuentra_la_cuenta() {
        let t = "\n\nEmpresa\n\nEMPRESA DEMO SL\n\nMovimientos*\n\nFECHA CONTABLE FECHA VALOR \
                 CATEGORÍA DESCRIPCIÓN IMPORTE SALDO\n\n08.01.2024 06.01.2024 Gastos Comis. mant. \
                 -5,00 EUR 1.507,40 EUR";
        let v = leer(t, "extracto.pdf").unwrap();
        assert_eq!(v[0].banco, "Bankinter");
        assert_eq!(v[0].cuenta, "");
        assert_eq!(v[0].avisos.len(), 1, "{:?}", v[0].avisos);
    }
}

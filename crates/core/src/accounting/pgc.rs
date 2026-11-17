//! Cuadro de cuentas del PGC de PYMES (RD 1515/2007, cuarta parte): grupos, subgrupos y las
//! cuentas más usadas por autónomos y pequeñas empresas, con su nombre. Sirve para nombrar las
//! cuentas en los libros y para buscarlas al teclear un asiento. Las subcuentas (`4300001`) toman
//! el nombre de su cuenta salvo que la empresa les dé uno propio.
//!
//! `TODO(VERIFY)`: transcrito del cuadro de cuentas publicado en el BOE (BOE-A-2007-19966); no es
//! exhaustivo (faltan cuentas poco frecuentes, que se pueden dar de alta con nombre propio).

/// (código, nombre).
pub const ACCOUNTS: &[(&str, &str)] = &[
    // Grupo 1
    ("1", "Financiación básica"),
    ("10", "Capital"),
    ("100", "Capital social"),
    ("101", "Fondo social"),
    ("102", "Capital"),
    ("103", "Socios por desembolsos no exigidos"),
    ("11", "Reservas y otros instrumentos de patrimonio"),
    ("112", "Reserva legal"),
    ("113", "Reservas voluntarias"),
    ("118", "Aportaciones de socios o propietarios"),
    ("12", "Resultados pendientes de aplicación"),
    ("120", "Remanente"),
    ("121", "Resultados negativos de ejercicios anteriores"),
    ("129", "Resultado del ejercicio"),
    (
        "13",
        "Subvenciones, donaciones y ajustes por cambios de valor",
    ),
    ("130", "Subvenciones oficiales de capital"),
    ("14", "Provisiones"),
    ("141", "Provisión para impuestos"),
    ("142", "Provisión para otras responsabilidades"),
    (
        "143",
        "Provisión por desmantelamiento, retiro o rehabilitación del inmovilizado",
    ),
    ("145", "Provisión para actuaciones medioambientales"),
    ("16", "Deudas a largo plazo con partes vinculadas"),
    ("163", "Otras deudas a largo plazo con partes vinculadas"),
    (
        "17",
        "Deudas a largo plazo por préstamos recibidos y otros conceptos",
    ),
    ("170", "Deudas a largo plazo con entidades de crédito"),
    ("171", "Deudas a largo plazo"),
    ("173", "Proveedores de inmovilizado a largo plazo"),
    (
        "174",
        "Acreedores por arrendamiento financiero a largo plazo",
    ),
    (
        "18",
        "Pasivos por fianzas, garantías y otros conceptos a largo plazo",
    ),
    ("180", "Fianzas recibidas a largo plazo"),
    // Grupo 2
    ("2", "Activo no corriente"),
    ("20", "Inmovilizaciones intangibles"),
    ("200", "Investigación"),
    ("201", "Desarrollo"),
    ("202", "Concesiones administrativas"),
    ("203", "Propiedad industrial"),
    ("204", "Fondo de comercio"),
    ("205", "Derechos de traspaso"),
    ("206", "Aplicaciones informáticas"),
    ("21", "Inmovilizaciones materiales"),
    ("210", "Terrenos y bienes naturales"),
    ("211", "Construcciones"),
    ("212", "Instalaciones técnicas"),
    ("213", "Maquinaria"),
    ("214", "Utillaje"),
    ("215", "Otras instalaciones"),
    ("216", "Mobiliario"),
    ("217", "Equipos para procesos de información"),
    ("218", "Elementos de transporte"),
    ("219", "Otro inmovilizado material"),
    ("22", "Inversiones inmobiliarias"),
    ("220", "Inversiones en terrenos y bienes naturales"),
    ("221", "Inversiones en construcciones"),
    ("23", "Inmovilizaciones materiales en curso"),
    (
        "24",
        "Inversiones financieras a largo plazo en partes vinculadas",
    ),
    ("242", "Créditos a largo plazo a partes vinculadas"),
    ("25", "Otras inversiones financieras a largo plazo"),
    (
        "250",
        "Inversiones financieras a largo plazo en instrumentos de patrimonio",
    ),
    ("252", "Créditos a largo plazo"),
    ("26", "Fianzas y depósitos constituidos a largo plazo"),
    ("260", "Fianzas constituidas a largo plazo"),
    ("28", "Amortización acumulada del inmovilizado"),
    ("280", "Amortización acumulada del inmovilizado intangible"),
    ("281", "Amortización acumulada del inmovilizado material"),
    (
        "282",
        "Amortización acumulada de las inversiones inmobiliarias",
    ),
    ("2813", "Amortización acumulada de maquinaria"),
    ("2814", "Amortización acumulada de utillaje"),
    ("2815", "Amortización acumulada de otras instalaciones"),
    ("2816", "Amortización acumulada de mobiliario"),
    (
        "2817",
        "Amortización acumulada de equipos para procesos de información",
    ),
    ("2818", "Amortización acumulada de elementos de transporte"),
    ("29", "Deterioro de valor de activos no corrientes"),
    // Grupo 3
    ("3", "Existencias"),
    ("30", "Comerciales"),
    ("300", "Mercaderías"),
    ("31", "Materias primas"),
    ("32", "Otros aprovisionamientos"),
    ("33", "Productos en curso"),
    ("35", "Productos terminados"),
    // Grupo 4
    ("4", "Acreedores y deudores por operaciones comerciales"),
    ("40", "Proveedores"),
    ("400", "Proveedores"),
    (
        "4009",
        "Proveedores, facturas pendientes de recibir o de formalizar",
    ),
    ("401", "Proveedores, efectos comerciales a pagar"),
    ("406", "Envases y embalajes a devolver a proveedores"),
    ("407", "Anticipos a proveedores"),
    ("41", "Acreedores varios"),
    ("410", "Acreedores por prestaciones de servicios"),
    ("411", "Acreedores, efectos comerciales a pagar"),
    ("43", "Clientes"),
    ("430", "Clientes"),
    ("4309", "Clientes, facturas pendientes de formalizar"),
    ("431", "Clientes, efectos comerciales a cobrar"),
    ("4312", "Efectos comerciales en gestión de cobro"),
    ("436", "Clientes de dudoso cobro"),
    ("438", "Anticipos de clientes"),
    ("44", "Deudores varios"),
    ("440", "Deudores"),
    ("46", "Personal"),
    ("460", "Anticipos de remuneraciones"),
    ("465", "Remuneraciones pendientes de pago"),
    ("47", "Administraciones públicas"),
    ("470", "Hacienda Pública, deudora por diversos conceptos"),
    ("4700", "Hacienda Pública, deudora por IVA"),
    (
        "4708",
        "Hacienda Pública, deudora por subvenciones concedidas",
    ),
    (
        "4709",
        "Hacienda Pública, deudora por devolución de impuestos",
    ),
    ("471", "Organismos de la Seguridad Social, deudores"),
    ("472", "Hacienda Pública, IVA soportado"),
    ("473", "Hacienda Pública, retenciones y pagos a cuenta"),
    ("474", "Activos por impuesto diferido"),
    ("475", "Hacienda Pública, acreedora por conceptos fiscales"),
    ("4750", "Hacienda Pública, acreedora por IVA"),
    (
        "4751",
        "Hacienda Pública, acreedora por retenciones practicadas",
    ),
    (
        "4752",
        "Hacienda Pública, acreedora por impuesto sobre sociedades",
    ),
    ("476", "Organismos de la Seguridad Social, acreedores"),
    ("477", "Hacienda Pública, IVA repercutido"),
    ("479", "Pasivos por diferencias temporarias imponibles"),
    ("48", "Ajustes por periodificación"),
    ("480", "Gastos anticipados"),
    ("485", "Ingresos anticipados"),
    (
        "49",
        "Deterioro de valor de créditos comerciales y provisiones a corto plazo",
    ),
    (
        "490",
        "Deterioro de valor de créditos por operaciones comerciales",
    ),
    ("499", "Provisiones por operaciones comerciales"),
    // Grupo 5
    ("5", "Cuentas financieras"),
    ("51", "Deudas a corto plazo con partes vinculadas"),
    ("513", "Otras deudas a corto plazo con partes vinculadas"),
    (
        "514",
        "Intereses a corto plazo de deudas con partes vinculadas",
    ),
    (
        "52",
        "Deudas a corto plazo por préstamos recibidos y otros conceptos",
    ),
    ("520", "Deudas a corto plazo con entidades de crédito"),
    ("521", "Deudas a corto plazo"),
    ("523", "Proveedores de inmovilizado a corto plazo"),
    (
        "524",
        "Acreedores por arrendamiento financiero a corto plazo",
    ),
    ("526", "Dividendo activo a pagar"),
    (
        "527",
        "Intereses a corto plazo de deudas con entidades de crédito",
    ),
    ("528", "Intereses a corto plazo de deudas"),
    ("529", "Provisiones a corto plazo"),
    (
        "53",
        "Inversiones financieras a corto plazo en partes vinculadas",
    ),
    ("532", "Créditos a corto plazo a partes vinculadas"),
    (
        "534",
        "Intereses a corto plazo de inversiones financieras en partes vinculadas",
    ),
    ("54", "Otras inversiones financieras a corto plazo"),
    ("542", "Créditos a corto plazo"),
    (
        "543",
        "Créditos a corto plazo por enajenación de inmovilizado",
    ),
    ("547", "Intereses a corto plazo de créditos"),
    ("55", "Otras cuentas no bancarias"),
    ("551", "Cuenta corriente con socios y administradores"),
    (
        "552",
        "Cuenta corriente con otras personas y entidades vinculadas",
    ),
    ("555", "Partidas pendientes de aplicación"),
    (
        "56",
        "Fianzas y depósitos recibidos y constituidos a corto plazo",
    ),
    ("560", "Fianzas recibidas a corto plazo"),
    ("565", "Fianzas constituidas a corto plazo"),
    ("57", "Tesorería"),
    ("570", "Caja, euros"),
    ("572", "Bancos e instituciones de crédito c/c vista, euros"),
    // Grupo 6
    ("6", "Compras y gastos"),
    ("60", "Compras"),
    ("600", "Compras de mercaderías"),
    ("601", "Compras de materias primas"),
    ("602", "Compras de otros aprovisionamientos"),
    ("606", "Descuentos sobre compras por pronto pago"),
    ("607", "Trabajos realizados por otras empresas"),
    ("608", "Devoluciones de compras y operaciones similares"),
    ("609", "«Rappels» por compras"),
    ("61", "Variación de existencias"),
    ("62", "Servicios exteriores"),
    ("620", "Gastos en investigación y desarrollo del ejercicio"),
    ("621", "Arrendamientos y cánones"),
    ("622", "Reparaciones y conservación"),
    ("623", "Servicios de profesionales independientes"),
    ("624", "Transportes"),
    ("625", "Primas de seguros"),
    ("626", "Servicios bancarios y similares"),
    ("627", "Publicidad, propaganda y relaciones públicas"),
    ("628", "Suministros"),
    ("629", "Otros servicios"),
    ("63", "Tributos"),
    ("630", "Impuesto sobre beneficios"),
    ("631", "Otros tributos"),
    ("634", "Ajustes negativos en la imposición indirecta"),
    ("639", "Ajustes positivos en la imposición indirecta"),
    ("64", "Gastos de personal"),
    ("640", "Sueldos y salarios"),
    ("641", "Indemnizaciones"),
    ("642", "Seguridad Social a cargo de la empresa"),
    ("649", "Otros gastos sociales"),
    ("65", "Otros gastos de gestión"),
    ("650", "Pérdidas de créditos comerciales incobrables"),
    ("659", "Otras pérdidas en gestión corriente"),
    ("66", "Gastos financieros"),
    ("662", "Intereses de deudas"),
    (
        "665",
        "Intereses por descuento de efectos y operaciones de factoring",
    ),
    ("668", "Diferencias negativas de cambio"),
    ("669", "Otros gastos financieros"),
    (
        "67",
        "Pérdidas procedentes de activos no corrientes y gastos excepcionales",
    ),
    ("670", "Pérdidas procedentes del inmovilizado intangible"),
    ("671", "Pérdidas procedentes del inmovilizado material"),
    (
        "672",
        "Pérdidas procedentes de las inversiones inmobiliarias",
    ),
    ("678", "Gastos excepcionales"),
    ("68", "Dotaciones para amortizaciones"),
    ("680", "Amortización del inmovilizado intangible"),
    ("681", "Amortización del inmovilizado material"),
    ("682", "Amortización de las inversiones inmobiliarias"),
    ("69", "Pérdidas por deterioro y otras dotaciones"),
    (
        "694",
        "Pérdidas por deterioro de créditos por operaciones comerciales",
    ),
    ("695", "Dotación a la provisión por operaciones comerciales"),
    // Grupo 7
    ("7", "Ventas e ingresos"),
    (
        "70",
        "Ventas de mercaderías, de producción propia, de servicios, etc.",
    ),
    ("700", "Ventas de mercaderías"),
    ("701", "Ventas de productos terminados"),
    ("705", "Prestaciones de servicios"),
    ("706", "Descuentos sobre ventas por pronto pago"),
    ("708", "Devoluciones de ventas y operaciones similares"),
    ("709", "«Rappels» sobre ventas"),
    ("71", "Variación de existencias"),
    ("74", "Subvenciones, donaciones y legados"),
    ("740", "Subvenciones, donaciones y legados a la explotación"),
    ("75", "Otros ingresos de gestión"),
    ("752", "Ingresos por arrendamientos"),
    ("759", "Ingresos por servicios diversos"),
    ("76", "Ingresos financieros"),
    ("762", "Ingresos de créditos"),
    ("768", "Diferencias positivas de cambio"),
    ("769", "Otros ingresos financieros"),
    (
        "77",
        "Beneficios procedentes de activos no corrientes e ingresos excepcionales",
    ),
    ("770", "Beneficios procedentes del inmovilizado intangible"),
    ("771", "Beneficios procedentes del inmovilizado material"),
    (
        "772",
        "Beneficios procedentes de las inversiones inmobiliarias",
    ),
    ("778", "Ingresos excepcionales"),
    (
        "79",
        "Excesos y aplicaciones de provisiones y de pérdidas por deterioro",
    ),
    (
        "794",
        "Reversión del deterioro de créditos por operaciones comerciales",
    ),
    ("795", "Exceso de provisiones"),
    ("7951", "Exceso de provisión para impuestos"),
    ("7952", "Exceso de provisión para otras responsabilidades"),
    ("7954", "Exceso de provisión por operaciones comerciales"),
    (
        "7955",
        "Exceso de provisión para actuaciones medioambientales",
    ),
];

/// Nombre de la cuenta del cuadro, si está.
pub fn name(code: &str) -> Option<&'static str> {
    ACCOUNTS.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

/// Nombre de la cuenta del cuadro más larga que es prefijo de `code` (para subcuentas:
/// `4300001` → «Clientes»; `5720001` → «Bancos…»), si alguna lo es con al menos 3 dígitos.
pub fn name_for(code: &str) -> Option<&'static str> {
    (3..=code.len().min(4))
        .rev()
        .filter_map(|n| code.get(..n))
        .find_map(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuadro_ordenado_y_sin_repetidos() {
        let mut seen = std::collections::HashSet::new();
        for (c, n) in ACCOUNTS {
            assert!(c.bytes().all(|b| b.is_ascii_digit()), "{c}");
            assert!(!n.is_empty());
            assert!(seen.insert(*c), "{c} repetida");
            // Toda cuenta cuelga de su subgrupo y su grupo.
            for n in 1..c.len() {
                assert!(name(&c[..n]).is_some() || n >= 3, "{c}: falta {}", &c[..n]);
            }
        }
    }

    #[test]
    fn nombres_de_cuentas_y_subcuentas() {
        assert_eq!(name("705"), Some("Prestaciones de servicios"));
        assert_eq!(name_for("4300001"), Some("Clientes"));
        assert_eq!(
            name_for("543"),
            Some("Créditos a corto plazo por enajenación de inmovilizado")
        );
        assert_eq!(
            name_for("4751"),
            Some("Hacienda Pública, acreedora por retenciones practicadas")
        );
        assert_eq!(name_for("47510001"), name("4751"));
        // Resultado de la baja de cada grupo del inmovilizado (CONTABILIDAD-08).
        assert_eq!(
            name("772"),
            Some("Beneficios procedentes de las inversiones inmobiliarias")
        );
        assert_eq!(
            name("670"),
            Some("Pérdidas procedentes del inmovilizado intangible")
        );
        assert_eq!(name_for("2817"), name("2817"));
        assert_eq!(name_for("99"), None);
        assert_eq!(name_for("999"), None);
    }
}

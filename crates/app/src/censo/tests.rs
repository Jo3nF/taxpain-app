use std::sync::Arc;

use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider};
use time::macros::{date, datetime};

use super::*;

const AHORA: OffsetDateTime = datetime!(2026-11-16 10:00 UTC);
const HOY: Date = date!(2026 - 11 - 16);

fn vault() -> Vault {
    Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap()
}

/// Certificado inventado de Estudio Demo SL.
const CERTIFICADO: &str = "AGENCIA TRIBUTARIA\n\
CERTIFICADO DE SITUACIÓN CENSAL\n\
NIF: B12345674 ESTUDIO DEMO SL\n\
Impuesto sobre el Valor Añadido: régimen general desde 01-02-2020\n\
Actividad: Servicios de diseño. Epígrafe IAE: 844\n\
Actividad: Alquiler de locales industriales. Epígrafe: 861.2\n\
Registro de operadores intracomunitarios: NO\n\
OBLIGACIONES PERIÓDICAS\n\
Modelo Periodicidad\n\
IVA. Autoliquidación (Impuesto sobre el valor añadido) trimestral\n\
Retenciones e ingresos a cuenta. Rendimientos del trabajo y actividades profesionales\n\
Retenciones e ingresos a cuenta. Arrendamientos de inmuebles urbanos\n\
Impuesto sobre Sociedades. Pago fraccionado sociedades\n\
Y para que conste se expide el presente certificado.";

fn sl() -> TaxpayerProfile {
    TaxpayerProfile::sociedad_limitada()
}

#[test]
fn lee_obligaciones_epigrafes_y_roi_del_certificado() {
    let s = situacion_de_texto(CERTIFICADO, Some(HOY), "Certificado de situación censal");
    assert_eq!(s.origen, Origen::Certificado);
    assert_eq!(
        s.obligaciones,
        ["111", "115", "200", "303"]
            .iter()
            .map(|x| (*x).to_owned())
            .collect()
    );
    assert_eq!(s.epigrafes, vec!["844".to_owned(), "861.2".to_owned()]);
    assert_eq!(s.roi, Some(false));
    assert!(s.conocida());
}

#[test]
fn normaliza_lo_tecleado() {
    assert_eq!(obligacion("Modelo 190"), Some("111".into()));
    assert_eq!(obligacion("202"), Some("200".into()));
    assert_eq!(obligacion("hola"), None);
    assert_eq!(epigrafe(" 843.9 "), Some("843.9".into()));
    assert_eq!(epigrafe("8"), None);
}

fn entrada(s: Situacion) -> Entrada {
    Entrada {
        situacion: Some(s),
        perfil: Some(sl()),
        ..Entrada::default()
    }
}

fn claves(d: &[Diferencia]) -> Vec<&str> {
    d.iter().map(|d| d.clave.as_str()).collect()
}

#[test]
fn retienes_sin_alta_y_alquiler_sin_115_e_intracomunitarias_sin_roi() {
    let mut s = situacion_de_texto(CERTIFICADO, Some(HOY), "Certificado");
    // Sin el 111 ni el 115 en el censo.
    s.obligaciones.remove("111");
    s.obligaciones.remove("115");
    let mut e = entrada(s);
    e.hechos.profesionales = Cuenta {
        n: 3,
        importe: Money::from_euros(450, 0),
    };
    e.hechos.alquileres = Cuenta {
        n: 2,
        importe: Money::from_euros(380, 0),
    };
    e.hechos.intracom_emitidas = 4;
    let (filas, d) = cotejar(&e);
    let k = claves(&d);
    assert!(k.contains(&"111-sin-alta"), "{k:?}");
    assert!(k.contains(&"115-sin-alta"));
    assert!(k.contains(&"roi-sin-alta"));
    // El 349 tampoco consta, y el perfil no lo tiene.
    assert!(k.contains(&"349-sin-alta"));
    assert!(k.contains(&"349-perfil"));
    let d111 = d.iter().find(|d| d.clave == "111-sin-alta").unwrap();
    assert_eq!(d111.gravedad, Gravedad::Alta);
    assert_eq!(d111.supuesto.as_deref(), Some("alta-111"));
    assert!(d111.titulo.contains("3 facturas de profesionales"));
    // Lo grave primero.
    assert_eq!(d[0].gravedad, Gravedad::Alta);
    let f = filas.iter().find(|f| f.codigo == "111").unwrap();
    assert_eq!(f.censo, Some(false));
    assert_eq!(f.perfil, Some(true));
    // Una sociedad no tiene fila del IRPF.
    assert!(filas.iter().all(|f| f.codigo != "130"));
}

#[test]
fn obligacion_sin_uso_propone_la_baja() {
    let s = situacion_de_texto(CERTIFICADO, Some(HOY), "Certificado");
    let mut e = entrada(s);
    let mut p = sl();
    p.retiene_alquileres = false;
    e.perfil = Some(p);
    let (_, d) = cotejar(&e);
    let b = d.iter().find(|d| d.clave == "115-sin-uso").unwrap();
    assert_eq!(b.supuesto.as_deref(), Some("baja-115"));
    // El 111 está en el perfil: no se propone su baja.
    assert!(!claves(&d).contains(&"111-sin-uso"));
}

#[test]
fn epigrafes_que_no_corresponden_y_alquiler_sin_861() {
    let mut s = situacion_de_texto(CERTIFICADO, Some(HOY), "Certificado");
    let mut e = entrada(s.clone());
    e.epigrafes_actividad = vec!["844".into(), "843.9".into()];
    let (_, d) = cotejar(&e);
    let k = claves(&d);
    assert!(k.contains(&"epigrafe-sobra-861.2"), "{k:?}");
    assert!(k.contains(&"epigrafe-falta-843.9"));
    // Ingresos de la 752 sin epígrafe 861.
    s.epigrafes = vec!["844".into()];
    let mut e = entrada(s);
    e.hechos.ingresos_alquiler = Money::from_euros(12_000, 0);
    let (_, d) = cotejar(&e);
    assert!(claves(&d).contains(&"epigrafe-alquiler"));
}

#[test]
fn domicilio_por_codigo_postal_y_criterio_de_caja() {
    let mut s = de_tecleado(&CensoTecleado {
        obligaciones: vec!["303".into(), "200".into()],
        domicilio_fiscal: Some("Calle Demo 1, 46001 Valencia".into()),
        criterio_caja: Some(false),
        ..CensoTecleado::default()
    });
    let mut e = entrada(s.clone());
    e.domicilio_facturas = Some("Avenida Inventada 9, 28001 Madrid".into());
    e.criterio_caja = true;
    let (_, d) = cotejar(&e);
    let k = claves(&d);
    assert!(k.contains(&"domicilio"));
    let c = d.iter().find(|d| d.clave == "criterio-caja").unwrap();
    assert_eq!(c.supuesto.as_deref(), Some("criterio-caja-opcion"));
    // Mismo código postal: no hay diferencia.
    s.domicilio_fiscal = Some("C/ Demo, 1 - 46001 VALENCIA".into());
    let mut e = entrada(s);
    e.domicilio_facturas = Some("Calle Demo 1, 46001 Valencia".into());
    assert!(!claves(&cotejar(&e).1).contains(&"domicilio"));
}

#[test]
fn sin_censo_avisa_y_coteja_el_perfil() {
    let mut e = Entrada {
        perfil: Some(TaxpayerProfile::autonomo()),
        ..Entrada::default()
    };
    e.hechos.dividendos = Cuenta {
        n: 1,
        importe: Money::from_euros(190, 0),
    };
    e.hechos
        .presentados
        .insert("303".into(), [2025, 2026].into_iter().collect());
    let (filas, d) = cotejar(&e);
    let k = claves(&d);
    assert!(k.contains(&"sin-censo"));
    assert!(k.contains(&"123-perfil"));
    assert!(!k.iter().any(|k| k.ends_with("-sin-alta")));
    // Persona física: sin fila del IS.
    assert!(filas.iter().all(|f| f.codigo != "200"));
    assert!(filas.iter().all(|f| f.censo.is_none()));
}

#[test]
fn guarda_programa_marca_y_quita_con_auditoria() {
    let v = vault();
    v.set_setting(PERFIL, &sl()).unwrap();
    guardar(
        &v,
        Some(CensoTecleado {
            obligaciones: vec!["303".into(), "Modelo 111".into(), "111".into()],
            epigrafes: vec!["844".into()],
            roi: Some(false),
            ..CensoTecleado::default()
        }),
        vec!["844".into()],
        AHORA,
    )
    .unwrap();
    assert!(guardar(&v, None, vec!["diseño".into()], AHORA).is_err());
    // Un dividendo con retención: la contabilidad lo ve y el censo no tiene el 123.
    v.connection()
        .execute(
            "INSERT INTO dividend_payment(date, gross, withheld, treasury, created_at)
             VALUES ('2026-06-30', 100000, 19000, '5720001', '2026-06-30T10:00:00Z')",
            [],
        )
        .unwrap();
    let c = cotejo(&v, HOY).unwrap();
    assert_eq!(c.situacion.origen, Origen::Tecleado);
    assert_eq!(
        c.tecleado.as_ref().unwrap().obligaciones,
        vec!["111".to_owned(), "303".to_owned()]
    );
    assert_eq!(c.hechos.dividendos.n, 1);
    assert!(claves(&c.diferencias).contains(&"123-sin-alta"));
    assert!(!c.supuestos.is_empty());

    // Sin fecha no se puede apuntar el plazo de un mes.
    assert!(programar(&v, "alta-123", None, "", HOY, AHORA).is_err());
    let p = programar(
        &v,
        "alta-123",
        Some(date!(2026 - 06 - 30)),
        "dividendo de junio",
        HOY,
        AHORA,
    )
    .unwrap();
    assert_eq!(p.vence, date!(2026 - 07 - 30));
    // No se apunta dos veces.
    assert!(programar(&v, "alta-123", Some(HOY), "", HOY, AHORA).is_err());
    assert_eq!(avisos_calendario(&v, 2026).unwrap().len(), 1);
    assert!(avisos_calendario(&v, 2027).unwrap().is_empty());
    marcar_hecha(&v, p.id, AHORA).unwrap();
    assert!(avisos_calendario(&v, 2026).unwrap().is_empty());
    assert!(quitar(&v, p.id, false, AHORA).is_err());
    quitar(&v, p.id, true, AHORA).unwrap();
    assert!(guardado(&v).unwrap().comunicaciones.is_empty());
    // Volver al certificado: sin certificado, no consta nada.
    guardar(&v, None, vec![], AHORA).unwrap();
    assert_eq!(situacion(&v).unwrap().origen, Origen::Ninguno);
    let n: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE action LIKE 'censo.%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 5);
    assert!(guia_de(&v, "inventado", None, HOY).is_err());
    assert_eq!(
        guia_de(&v, "opcion-202-base", None, HOY)
            .unwrap()
            .plazo
            .desde,
        Some(date!(2027 - 02 - 01))
    );
}

//! Asistente «Quiero comunicar…»: para cada cambio censal, qué apartados del modelo 036 tocar,
//! el plazo (un mes desde el hecho, antes de la primera operación o la ventana de cada opción) y
//! la documentación. TaxPain no presenta el 036: se rellena en el formulario de la Sede.
//!
//! `TODO(VERIFY)`: los apartados se nombran por su título y no por el número de página ni de
//! casilla, porque no se han cotejado con el diseño del 036 de la Orden HAC/1526/2024. Antes
//! de dar números hay que comprobarlos con la orden vigente (y con el formulario de la Sede, que
//! guía por pantallas y no siempre sigue el orden del papel).

use serde::Serialize;
use taxpain_core::calendar::{Ccaa, Persona, calendar_for};
use time::{Date, Month};

/// Página del 036 en la Sede (procedimiento G322, «Declaración censal de alta, modificación y
/// baja»). `TODO(VERIFY)`: comprobar el código del procedimiento si la Sede lo cambia.
pub const SEDE_036: &str = "https://sede.agenciatributaria.gob.es/Sede/procedimientoini/G322.shtml";

/// Aviso común: de dónde salen los apartados y por qué no llevan número.
pub const AVISO_CASILLAS: &str = "Los apartados se nombran por su título: los números de página y \
     de casilla no se han cotejado todavía con el diseño del 036 de la Orden HAC/1526/2024 \
     (TODO(VERIFY)). En el formulario de la Sede busca el apartado por su nombre.";

/// Un supuesto del asistente, para la lista.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Supuesto {
    pub id: &'static str,
    pub titulo: &'static str,
    /// Grupo de la lista («IVA y operaciones intracomunitarias», «Retenciones»…).
    pub grupo: &'static str,
}

/// Cómo se cuenta el plazo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoPlazo {
    /// Un mes desde el hecho, de fecha a fecha (art. 10 RGAT).
    UnMes,
    /// Antes de la primera operación (alta en el ROI).
    Antes,
    /// Ventana de una opción (febrero, noviembre, diciembre).
    Ventana,
    /// No va en el 036 sino en el 303 del último periodo del año.
    En303,
}

/// El plazo de un supuesto, con las fechas si se pueden calcular.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Plazo {
    pub tipo: TipoPlazo,
    pub texto: String,
    pub desde: Option<Date>,
    /// Último día (trasladado al siguiente hábil si cae en festivo o fin de semana).
    pub vence: Option<Date>,
    /// Hace falta la fecha del hecho para calcularlo.
    pub pide_fecha: bool,
    /// El calendario ya trae la ventana si el perfil tiene los avisos de opciones censales.
    pub en_calendario: Option<&'static str>,
}

/// Un apartado del 036 y qué hacer en él.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Paso {
    pub apartado: String,
    pub que_hacer: String,
}

/// La guía de un supuesto.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Guia {
    pub id: String,
    pub titulo: String,
    pub resumen: String,
    /// Se comunica con el 036 (no, la prorrata especial: va en el 303).
    pub por_036: bool,
    /// Causa de presentación que se marca al principio del 036.
    pub causa: String,
    pub pasos: Vec<Paso>,
    pub plazo: Plazo,
    pub documentacion: Vec<String>,
    pub norma: String,
    pub avisos: Vec<String>,
    pub sede: Option<&'static str>,
    pub verificar: &'static str,
}

/// Lo que la guía necesita saber de la empresa.
#[derive(Clone, Debug, Default)]
pub struct Contexto {
    pub persona: Option<Persona>,
    /// Último día del ejercicio (mes, día).
    pub cierre: Option<(Month, u8)>,
    pub ccaa: Option<Ccaa>,
    pub festivos_locales: Vec<Date>,
    /// El 202 del ejercicio que viene va obligatoriamente por la base (más de 6 M€).
    pub base_202_obligatoria: bool,
    /// En TaxPain está elegida la base para el 202 del ejercicio que viene.
    pub base_202_elegida: bool,
    /// Tiene marcados los avisos de opciones censales en el perfil.
    pub avisos_opciones: bool,
}

const SUPUESTOS: &[Supuesto] = &[
    Supuesto {
        id: "alta-roi",
        titulo: "Alta en el Registro de operadores intracomunitarios (ROI)",
        grupo: "IVA y operaciones intracomunitarias",
    },
    Supuesto {
        id: "baja-roi",
        titulo: "Baja en el ROI",
        grupo: "IVA y operaciones intracomunitarias",
    },
    Supuesto {
        id: "criterio-caja-opcion",
        titulo: "Optar por el criterio de caja del IVA",
        grupo: "IVA y operaciones intracomunitarias",
    },
    Supuesto {
        id: "criterio-caja-renuncia",
        titulo: "Renunciar al criterio de caja del IVA",
        grupo: "IVA y operaciones intracomunitarias",
    },
    Supuesto {
        id: "prorrata-especial",
        titulo: "Optar por la prorrata especial (o dejarla)",
        grupo: "IVA y operaciones intracomunitarias",
    },
    Supuesto {
        id: "redeme",
        titulo: "Inscribirte o darte de baja en el REDEME (devolución mensual)",
        grupo: "IVA y operaciones intracomunitarias",
    },
    Supuesto {
        id: "alta-111",
        titulo: "Alta en las retenciones de trabajo y profesionales (111)",
        grupo: "Retenciones",
    },
    Supuesto {
        id: "baja-111",
        titulo: "Baja en las retenciones de trabajo y profesionales (111)",
        grupo: "Retenciones",
    },
    Supuesto {
        id: "alta-115",
        titulo: "Alta en las retenciones de alquileres (115)",
        grupo: "Retenciones",
    },
    Supuesto {
        id: "baja-115",
        titulo: "Baja en las retenciones de alquileres (115)",
        grupo: "Retenciones",
    },
    Supuesto {
        id: "alta-123",
        titulo: "Alta en las retenciones del capital mobiliario (123)",
        grupo: "Retenciones",
    },
    Supuesto {
        id: "baja-123",
        titulo: "Baja en las retenciones del capital mobiliario (123)",
        grupo: "Retenciones",
    },
    Supuesto {
        id: "opcion-202-base",
        titulo: "Optar por el 202 por la base del periodo (art. 40.3 LIS)",
        grupo: "Impuesto sobre Sociedades",
    },
    Supuesto {
        id: "renuncia-202-base",
        titulo: "Renunciar al 202 por la base del periodo",
        grupo: "Impuesto sobre Sociedades",
    },
    Supuesto {
        id: "alta-epigrafe",
        titulo: "Nueva actividad o nuevo epígrafe del IAE",
        grupo: "Actividad y datos de la empresa",
    },
    Supuesto {
        id: "baja-epigrafe",
        titulo: "Dejar una actividad (baja de un epígrafe)",
        grupo: "Actividad y datos de la empresa",
    },
    Supuesto {
        id: "domicilio-fiscal",
        titulo: "Cambio de domicilio fiscal",
        grupo: "Actividad y datos de la empresa",
    },
    Supuesto {
        id: "domicilio-social",
        titulo: "Cambio de domicilio social",
        grupo: "Actividad y datos de la empresa",
    },
    Supuesto {
        id: "administrador",
        titulo: "Cambio de administrador o de representante",
        grupo: "Actividad y datos de la empresa",
    },
];

/// Los supuestos del asistente.
pub fn supuestos() -> &'static [Supuesto] {
    SUPUESTOS
}

/// El título de un supuesto.
pub fn titulo(id: &str) -> Option<&'static str> {
    SUPUESTOS.iter().find(|s| s.id == id).map(|s| s.titulo)
}

/// El mismo día del mes siguiente (plazo por meses de fecha a fecha; si ese día no existe, el
/// último del mes: art. 30.4 de la Ley 39/2015, que aplica la LGT).
pub fn un_mes_despues(d: Date) -> Date {
    let (y, m) = if d.month() == Month::December {
        (d.year() + 1, Month::January)
    } else {
        (d.year(), d.month().next())
    };
    let ultimo = m.length(y);
    Date::from_calendar_date(y, m, d.day().min(ultimo)).unwrap_or(d)
}

fn dia(y: i32, m: Month, d: u8) -> Option<Date> {
    Date::from_calendar_date(y, m, d).ok()
}

fn fin_de_mes(y: i32, m: Month) -> Option<Date> {
    dia(y, m, m.length(y))
}

/// Traslada al siguiente día hábil con los festivos nacionales, de la comunidad y locales.
fn habil(d: Date, c: &Contexto) -> Date {
    calendar_for(d.year(), c.ccaa, &c.festivos_locales)
        .0
        .roll_forward(d)
}

/// La próxima ventana de un mes (`mes` de este año si no ha acabado; si no, del siguiente).
fn ventana(mes: Month, hoy: Date, c: &Contexto) -> (Option<Date>, Option<Date>) {
    let año = match fin_de_mes(hoy.year(), mes) {
        Some(fin) if hoy <= fin => hoy.year(),
        _ => hoy.year() + 1,
    };
    (dia(año, mes, 1), fin_de_mes(año, mes).map(|d| habil(d, c)))
}

fn fecha_es(d: Date) -> String {
    taxpain_core::calendar::reminders::format_es(d)
}

fn paso(apartado: &str, que_hacer: &str) -> Paso {
    Paso {
        apartado: apartado.into(),
        que_hacer: que_hacer.into(),
    }
}

/// Plazo de un mes desde el hecho.
fn un_mes(hecho: Option<Date>, que: &str, c: &Contexto) -> Plazo {
    let vence = hecho.map(|h| habil(un_mes_despues(h), c));
    Plazo {
        tipo: TipoPlazo::UnMes,
        texto: match (hecho, vence) {
            (Some(h), Some(v)) => format!(
                "Un mes desde {que} ({}): hasta el {} (art. 10 RGAT).",
                fecha_es(h),
                fecha_es(v)
            ),
            _ => format!("Un mes desde {que} (art. 10 RGAT). Pon la fecha para calcularlo."),
        },
        desde: hecho,
        vence,
        pide_fecha: hecho.is_none(),
        en_calendario: None,
    }
}

const CAUSA_MOD: &str = "Modificación (de datos de la declaración censal ya presentada)";

/// Retenciones: alta o baja de la obligación de un modelo.
fn retenciones(alta: bool, modelo: &str, hecho: Option<Date>, c: &Contexto) -> Guia {
    let (que, resumen, quien) = match modelo {
        "111" => (
            "las retenciones e ingresos a cuenta del trabajo y de actividades económicas \
             (profesionales)",
            "Quien paga nóminas o facturas de profesionales con retención presenta el 111 cada \
             trimestre (o mes) y el 190 en enero.",
            "el primer pago con retención (nómina o factura de un profesional)",
        ),
        "115" => (
            "las retenciones de arrendamientos de inmuebles urbanos",
            "Quien paga el alquiler de un local con retención presenta el 115 cada trimestre y \
             el 180 en enero.",
            "el primer pago del alquiler con retención",
        ),
        _ => (
            "las retenciones del capital mobiliario (dividendos, intereses)",
            "Quien paga dividendos o intereses con retención presenta el 123 del periodo y el \
             193 en enero.",
            "el primer pago de dividendos o intereses con retención",
        ),
    };
    let titulo = titulo(&format!("{}-{modelo}", if alta { "alta" } else { "baja" }))
        .unwrap_or_default()
        .to_owned();
    let mut avisos = Vec::new();
    if alta {
        avisos.push(format!(
            "Si ya has retenido sin la obligación dada de alta, preséntala cuanto antes y \
             presenta también los {modelo} de los periodos pasados: la AEAT los reclama por el \
             censo y por los 190/180/193 de los perceptores."
        ));
    } else {
        avisos.push(format!(
            "Mientras conste la obligación, la AEAT espera el {modelo} de cada periodo y puede \
             mandar requerimientos. El resumen anual del año de la baja se presenta igual."
        ));
    }
    Guia {
        id: format!("{}-{modelo}", if alta { "alta" } else { "baja" }),
        titulo,
        resumen: resumen.into(),
        por_036: true,
        causa: CAUSA_MOD.into(),
        pasos: vec![
            paso(
                "Causas de presentación",
                "Marca la modificación de datos relativos a las retenciones e ingresos a cuenta.",
            ),
            paso(
                "Retenciones e ingresos a cuenta",
                &format!(
                    "{} la obligación de {que} (modelo {modelo}), con la fecha {}.",
                    if alta { "Da de alta" } else { "Da de baja" },
                    if alta {
                        "del primer pago"
                    } else {
                        "desde la que ya no pagas esas rentas"
                    }
                ),
            ),
        ],
        plazo: un_mes(
            hecho,
            if alta {
                quien
            } else {
                "el último pago con retención"
            },
            c,
        ),
        documentacion: vec![],
        norma: "RGAT arts. 9 a 14 (declaración censal) y RIRPF arts. 101 y 108".into(),
        avisos,
        sede: Some(SEDE_036),
        verificar: AVISO_CASILLAS,
    }
}

/// La guía de un supuesto (`None` si no existe). `hecho` es la fecha del cambio (o de la
/// primera operación, en el ROI).
pub fn guia(id: &str, hecho: Option<Date>, hoy: Date, c: &Contexto) -> Option<Guia> {
    let titulo_de = |id: &str| titulo(id).unwrap_or_default().to_owned();
    let base = |id: &str, resumen: &str, pasos: Vec<Paso>, plazo: Plazo| Guia {
        id: id.into(),
        titulo: titulo_de(id),
        resumen: resumen.into(),
        por_036: true,
        causa: CAUSA_MOD.into(),
        pasos,
        plazo,
        documentacion: vec![],
        norma: "RGAT arts. 9 a 14 (declaración censal); Orden HAC/1526/2024 (modelo 036)".into(),
        avisos: vec![],
        sede: Some(SEDE_036),
        verificar: AVISO_CASILLAS,
    };
    let juridica = c.persona == Some(Persona::Juridica);
    let g = match id {
        "alta-111" | "alta-115" | "alta-123" => retenciones(true, &id[5..], hecho, c),
        "baja-111" | "baja-115" | "baja-123" => retenciones(false, &id[5..], hecho, c),
        "alta-roi" => {
            let mut g = base(
                id,
                "Para hacer entregas o adquisiciones intracomunitarias, o prestar o recibir \
                 servicios de empresarios de otros países de la UE, hay que estar en el ROI: sin \
                 él, tu NIF-IVA no sale en el VIES y el cliente europeo te cobra el IVA de su \
                 país (o tú no puedes facturar sin IVA).",
                vec![
                    paso(
                        "Causas de presentación",
                        "Marca la solicitud de alta en el Registro de operadores \
                         intracomunitarios.",
                    ),
                    paso(
                        "IVA (Registro de operadores intracomunitarios)",
                        "Marca el alta en el ROI e indica si haces entregas, adquisiciones o \
                         servicios intracomunitarios y la fecha prevista de la primera \
                         operación.",
                    ),
                ],
                Plazo {
                    tipo: TipoPlazo::Antes,
                    texto: match hecho {
                        Some(h) => format!(
                            "Antes de la primera operación intracomunitaria (la prevés el {}).",
                            fecha_es(h)
                        ),
                        None => "Antes de la primera operación intracomunitaria.".into(),
                    },
                    desde: None,
                    vence: hecho,
                    pide_fecha: false,
                    en_calendario: None,
                },
            );
            g.norma = "RGAT arts. 3.3 y 25 (Registro de operadores intracomunitarios); \
                       LIVA art. 25"
                .into();
            g.documentacion = vec![
                "La AEAT puede pedir que justifiques la actividad (contratos, facturas \
                 proforma, web): tenlo a mano."
                    .into(),
            ];
            g.avisos = vec![
                "Comprueba después en el VIES que tu NIF-IVA (ES + NIF) aparece como válido."
                    .into(),
                "El 349 se presenta en los periodos con operaciones.".into(),
            ];
            g
        }
        "baja-roi" => {
            let mut g = base(
                id,
                "Si ya no haces operaciones intracomunitarias, puedes pedir la baja. La AEAT \
                 también puede darte de baja de oficio si no las haces.",
                vec![
                    paso(
                        "Causas de presentación",
                        "Marca la baja en el Registro de operadores intracomunitarios.",
                    ),
                    paso(
                        "IVA (Registro de operadores intracomunitarios)",
                        "Marca la baja en el ROI con su fecha.",
                    ),
                ],
                un_mes(hecho, "que dejas de hacer operaciones intracomunitarias", c),
            );
            g.norma = "RGAT arts. 3.3, 10 y 25".into();
            g.avisos = vec![
                "Sin ROI no puedes facturar a empresas de la UE sin IVA: si vuelves a tener \
                 clientes europeos, hay que pedir el alta antes."
                    .into(),
            ];
            g
        }
        "criterio-caja-opcion" | "criterio-caja-renuncia" => {
            let opcion = id == "criterio-caja-opcion";
            let (desde, vence) = ventana(Month::December, hoy, c);
            let mut g = base(
                id,
                if opcion {
                    "Con el criterio de caja, el IVA de las ventas se declara al cobrar y el de \
                     las compras se deduce al pagar. Sólo si el volumen de operaciones del año \
                     anterior no pasó de 2 millones de euros."
                } else {
                    "La renuncia al criterio de caja vale para el año siguiente y obliga al \
                     menos tres años."
                },
                vec![
                    paso(
                        "Causas de presentación",
                        "Marca la modificación de datos del IVA.",
                    ),
                    paso(
                        "IVA (regímenes especiales)",
                        if opcion {
                            "Marca la opción por el régimen especial del criterio de caja."
                        } else {
                            "Marca la renuncia al régimen especial del criterio de caja."
                        },
                    ),
                ],
                Plazo {
                    tipo: TipoPlazo::Ventana,
                    texto: format!(
                        "En diciembre, para el año siguiente{} (o en la declaración de inicio de \
                         la actividad).",
                        match vence {
                            Some(v) => format!(": hasta el {}", fecha_es(v)),
                            None => String::new(),
                        }
                    ),
                    desde,
                    vence,
                    pide_fecha: false,
                    en_calendario: Some("036-diciembre"),
                },
            );
            g.norma = "LIVA arts. 163 decies y ss.; RIVA art. 61 septies".into();
            if !opcion {
                g.avisos.push(
                    "Después de renunciar no se puede volver al criterio de caja en tres años."
                        .into(),
                );
            }
            g
        }
        "prorrata-especial" => {
            let fin = dia(hoy.year() + 1, Month::January, 30).map(|d| habil(d, c));
            let mut g = base(
                id,
                "La opción por la prorrata especial (y su revocación) no va en el 036: se marca \
                 en el 303 del último periodo del año.",
                vec![paso(
                    "Modelo 303 del 4.º trimestre (o de diciembre)",
                    "Marca la casilla de opción o revocación de la prorrata especial en el \
                     303 del último periodo.",
                )],
                Plazo {
                    tipo: TipoPlazo::En303,
                    texto: format!(
                        "En el 303 del último periodo del año{}.",
                        match fin {
                            Some(v) => format!(", que vence el {}", fecha_es(v)),
                            None => String::new(),
                        }
                    ),
                    desde: dia(hoy.year() + 1, Month::January, 1),
                    vence: fin,
                    pide_fecha: false,
                    en_calendario: Some("prorrata-especial"),
                },
            );
            g.por_036 = false;
            g.causa = "No se presenta el 036".into();
            g.sede = None;
            g.norma = "LIVA art. 103.Dos; RIVA art. 28.1.1.º.a".into();
            g.avisos.push(
                "Es obligatoria si la deducción por la prorrata general supera en un 10 % o más \
                 la que daría la especial (LIVA art. 103.Dos.2.º)."
                    .into(),
            );
            g
        }
        "redeme" => {
            let (desde, vence) = ventana(Month::November, hoy, c);
            let mut g = base(
                id,
                "En el REDEME se pide la devolución del IVA cada mes en vez de al final del año; \
                 obliga al 303 mensual y al SII.",
                vec![
                    paso(
                        "Causas de presentación",
                        "Marca la modificación de datos del IVA.",
                    ),
                    paso(
                        "IVA (registro de devolución mensual)",
                        "Marca la solicitud de inscripción o de baja en el registro de \
                         devolución mensual.",
                    ),
                ],
                Plazo {
                    tipo: TipoPlazo::Ventana,
                    texto: format!(
                        "En noviembre, para el año siguiente{}.",
                        match vence {
                            Some(v) => format!(": hasta el {}", fecha_es(v)),
                            None => String::new(),
                        }
                    ),
                    desde,
                    vence,
                    pide_fecha: false,
                    en_calendario: Some("036-noviembre"),
                },
            );
            g.norma = "LIVA art. 116; RIVA art. 30".into();
            g
        }
        "opcion-202-base" | "renuncia-202-base" => {
            let opcion = id == "opcion-202-base";
            let natural = c.cierre.is_none_or(|c| c == (Month::December, 31));
            let (desde, vence) = if natural {
                ventana(Month::February, hoy, c)
            } else {
                (None, None)
            };
            let mut g = base(
                id,
                if opcion {
                    "Con la modalidad del art. 40.3 LIS, los pagos fraccionados se calculan sobre \
                     el resultado de cada periodo (a 31 de marzo, 30 de septiembre y 30 de \
                     noviembre) y no sobre la cuota del último Impuesto sobre Sociedades."
                } else {
                    "La renuncia devuelve los pagos fraccionados a la modalidad de la cuota (art. \
                     40.2 LIS) a partir del ejercicio que empieza."
                },
                vec![
                    paso(
                        "Causas de presentación",
                        "Marca la modificación de datos del Impuesto sobre Sociedades.",
                    ),
                    paso(
                        "Impuesto sobre Sociedades",
                        if opcion {
                            "Marca la opción por la modalidad de pagos fraccionados del art. \
                             40.3 LIS (base imponible del periodo)."
                        } else {
                            "Marca la renuncia a la modalidad de pagos fraccionados del art. \
                             40.3 LIS."
                        },
                    ),
                ],
                Plazo {
                    tipo: TipoPlazo::Ventana,
                    texto: if natural {
                        format!(
                            "En febrero del año en que debe surtir efecto{}.",
                            match vence {
                                Some(v) => format!(": hasta el {}", fecha_es(v)),
                                None => String::new(),
                            }
                        )
                    } else {
                        "En los dos primeros meses del periodo impositivo, o antes de que acabe el \
                         plazo del primer pago fraccionado si es más corto (ejercicio que no \
                         coincide con el año natural)."
                            .into()
                    },
                    desde,
                    vence,
                    pide_fecha: false,
                    en_calendario: Some("036-202"),
                },
            );
            g.norma = "LIS art. 40.3".into();
            if !juridica {
                g.avisos
                    .push("Sólo para sociedades: el perfil fiscal no es de una sociedad.".into());
            }
            if c.base_202_obligatoria {
                g.avisos.push(
                    "Con más de 6 millones de cifra de negocios la base del periodo es \
                     obligatoria: no hace falta optar (ni se puede renunciar)."
                        .into(),
                );
            }
            if opcion && c.base_202_elegida {
                g.avisos.push(
                    "En Modelos → 202 tienes elegida la base para el ejercicio que viene: sólo \
                     vale si la comunicas en el 036 en su plazo."
                        .into(),
                );
            }
            g.avisos.push(
                "Antes, compara las dos modalidades en Modelos → 202. La opción vale para los \
                 periodos siguientes mientras no renuncies."
                    .into(),
            );
            g
        }
        "alta-epigrafe" | "baja-epigrafe" => {
            let alta = id == "alta-epigrafe";
            let mut g = base(
                id,
                if alta {
                    "Cada actividad que haces tiene que constar en el censo con su epígrafe del \
                     IAE (aunque estés exento del impuesto)."
                } else {
                    "Si dejas una actividad, comunica la baja de su epígrafe; si era la única, es \
                     una baja de actividad y cambian también las obligaciones."
                },
                vec![
                    paso(
                        "Causas de presentación",
                        "Marca la modificación de datos de actividades económicas y locales.",
                    ),
                    paso(
                        "Actividades económicas y locales",
                        if alta {
                            "Añade la actividad con su epígrafe del IAE, la fecha de inicio y el \
                             local donde se hace (si lo hay)."
                        } else {
                            "Marca la baja del epígrafe con la fecha en que dejaste la actividad."
                        },
                    ),
                ],
                un_mes(
                    hecho,
                    if alta {
                        "el inicio de la nueva actividad"
                    } else {
                        "el cese de la actividad"
                    },
                    c,
                ),
            );
            g.norma = "RGAT arts. 10 y 11; TRLRHL arts. 78 a 91 (IAE)".into();
            if alta {
                g.avisos.push(
                    "TODO(VERIFY): si la nueva actividad empieza antes de comunicarla, el alta \
                     podría tener que hacerse antes del inicio (art. 9.3 RGAT)."
                        .into(),
                );
                g.avisos.push(
                    "Si pagas el IAE (cifra de negocios de 1 M€ o más), el alta en la matrícula \
                     va en el 840."
                        .into(),
                );
            }
            g
        }
        "domicilio-fiscal" => {
            let mut g = base(
                id,
                "El domicilio fiscal es donde está la gestión y dirección de la empresa (o tu \
                 vivienda habitual si eres autónomo). Sin comunicar el cambio, las \
                 notificaciones al antiguo valen.",
                vec![
                    paso(
                        "Causas de presentación",
                        "Marca la modificación del domicilio fiscal.",
                    ),
                    paso(
                        "Identificación y domicilios",
                        "Escribe el nuevo domicilio fiscal completo y la fecha del cambio.",
                    ),
                ],
                un_mes(hecho, "el cambio de domicilio", c),
            );
            g.norma = "LGT art. 48.3; RGAT arts. 10 y 17".into();
            g.avisos.push(
                "TODO(VERIFY): quien no tiene actividad económica comunica el cambio con el 030 \
                 y tres meses de plazo (art. 17.3 RGAT)."
                    .into(),
            );
            g.avisos.push(
                "Cambia también el domicilio en Ajustes → Facturación (sale en tus facturas) y, \
                 si es el social, en el Registro Mercantil."
                    .into(),
            );
            g
        }
        "domicilio-social" => {
            let mut g = base(
                id,
                "El cambio de domicilio social de una sociedad se acuerda, se eleva a escritura \
                 (salvo traslado dentro del mismo término municipal si los estatutos lo \
                 permiten) y se inscribe en el Registro Mercantil; después se comunica a la \
                 AEAT.",
                vec![
                    paso(
                        "Causas de presentación",
                        "Marca la modificación de los datos identificativos (domicilio \
                         social).",
                    ),
                    paso(
                        "Identificación y domicilios",
                        "Escribe el nuevo domicilio social; si también cambia el fiscal, \
                         márcalo en el mismo 036.",
                    ),
                ],
                un_mes(hecho, "el cambio (el acuerdo o su inscripción)", c),
            );
            g.norma = "LSC art. 285.2; RGAT art. 10".into();
            g.documentacion = vec![
                "Escritura del cambio de domicilio, inscrita en el Registro Mercantil.".into(),
            ];
            g.avisos.push(
                "TODO(VERIFY): si el mes cuenta desde el acuerdo o desde la inscripción en el \
                 Registro."
                    .into(),
            );
            if !juridica {
                g.avisos
                    .push("Sólo para sociedades: el perfil fiscal no es de una sociedad.".into());
            }
            g
        }
        "administrador" => {
            let mut g = base(
                id,
                "El nombramiento o cese de administradores (y de los representantes ante la \
                 AEAT) se comunica para que conste quién representa a la sociedad.",
                vec![
                    paso(
                        "Causas de presentación",
                        "Marca la modificación de los datos de representantes.",
                    ),
                    paso(
                        "Representantes",
                        "Da de alta al nuevo administrador (NIF, nombre, cargo, fecha) y de \
                         baja al anterior, con la escritura de nombramiento.",
                    ),
                ],
                un_mes(hecho, "el nombramiento o el cese", c),
            );
            g.norma = "RGAT arts. 10 y 11; LSC arts. 214 y 215".into();
            g.documentacion = vec![
                "Escritura o certificado del acuerdo de nombramiento, inscrito en el Registro \
                 Mercantil."
                    .into(),
            ];
            g.avisos.push(
                "El certificado electrónico de representante de la sociedad va a nombre del \
                 administrador: con el cambio hay que pedir uno nuevo."
                    .into(),
            );
            g
        }
        _ => return None,
    };
    let mut g = g;
    if c.avisos_opciones {
        if let Some(code) = g.plazo.en_calendario {
            g.avisos.push(format!(
                "El calendario ya avisa de esta ventana (aviso «{code}»)."
            ));
        }
    } else if g.plazo.en_calendario.is_some() {
        g.avisos.push(
            "Para que el calendario avise de esta ventana cada año, marca «Avisos de opciones \
             censales» en el perfil fiscal."
                .into(),
        );
    }
    Some(g)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn ctx() -> Contexto {
        Contexto {
            persona: Some(Persona::Juridica),
            ..Contexto::default()
        }
    }

    #[test]
    fn un_mes_de_fecha_a_fecha() {
        assert_eq!(un_mes_despues(date!(2026 - 03 - 15)), date!(2026 - 04 - 15));
        assert_eq!(un_mes_despues(date!(2026 - 01 - 31)), date!(2026 - 02 - 28));
        assert_eq!(un_mes_despues(date!(2026 - 12 - 10)), date!(2027 - 01 - 10));
    }

    #[test]
    fn todos_los_supuestos_tienen_guia_sin_numeros_de_casilla() {
        for s in supuestos() {
            let g = guia(
                s.id,
                Some(date!(2026 - 11 - 02)),
                date!(2026 - 11 - 16),
                &ctx(),
            )
            .unwrap_or_else(|| panic!("sin guía: {}", s.id));
            assert_eq!(g.titulo, s.titulo);
            assert!(!g.pasos.is_empty());
            // Ningún paso cita una casilla por número (no se han cotejado con la orden).
            for p in &g.pasos {
                assert!(!p.que_hacer.contains("casilla ["), "{}", s.id);
            }
        }
        assert!(guia("inventado", None, date!(2026 - 11 - 16), &ctx()).is_none());
    }

    #[test]
    fn alta_del_111_un_mes_con_traslado_al_habil() {
        // 7 de noviembre de 2026 + un mes = lunes 7 de diciembre; el 8 es festivo nacional.
        let g = guia(
            "alta-111",
            Some(date!(2026 - 11 - 07)),
            date!(2026 - 11 - 16),
            &ctx(),
        )
        .unwrap();
        assert_eq!(g.plazo.tipo, TipoPlazo::UnMes);
        assert_eq!(g.plazo.vence, Some(date!(2026 - 12 - 07)));
        // El 12 de octubre (fiesta nacional) pasa al día 13.
        let g = guia(
            "baja-115",
            Some(date!(2026 - 09 - 12)),
            date!(2026 - 11 - 16),
            &ctx(),
        )
        .unwrap();
        assert_eq!(g.plazo.vence, Some(date!(2026 - 10 - 13)));
        let g = guia("alta-123", None, date!(2026 - 11 - 16), &ctx()).unwrap();
        assert!(g.plazo.pide_fecha);
        assert_eq!(g.plazo.vence, None);
    }

    #[test]
    fn opcion_202_en_febrero_del_año_siguiente() {
        let mut c = ctx();
        c.base_202_elegida = true;
        let g = guia("opcion-202-base", None, date!(2026 - 11 - 16), &c).unwrap();
        assert_eq!(g.plazo.desde, Some(date!(2027 - 02 - 01)));
        // 28 de febrero de 2027 es domingo: pasa al lunes 1 de marzo.
        assert_eq!(g.plazo.vence, Some(date!(2027 - 03 - 01)));
        assert!(
            g.avisos
                .iter()
                .any(|a| a.contains("tienes elegida la base"))
        );
        assert!(
            g.avisos
                .iter()
                .any(|a| a.contains("Avisos de opciones censales"))
        );
        // En pleno febrero, la de este año.
        let g = guia("opcion-202-base", None, date!(2027 - 02 - 10), &c).unwrap();
        assert_eq!(g.plazo.desde, Some(date!(2027 - 02 - 01)));
        // Ejercicio que no es el natural: sin fechas, con el texto de los dos meses.
        c.cierre = Some((Month::June, 30));
        let g = guia("opcion-202-base", None, date!(2026 - 11 - 16), &c).unwrap();
        assert_eq!(g.plazo.vence, None);
        assert!(g.plazo.texto.contains("dos primeros meses"));
        // Persona física: aviso.
        let g = guia(
            "opcion-202-base",
            None,
            date!(2026 - 11 - 16),
            &Contexto::default(),
        )
        .unwrap();
        assert!(g.avisos.iter().any(|a| a.contains("Sólo para sociedades")));
    }

    #[test]
    fn criterio_de_caja_en_diciembre_y_prorrata_en_el_303() {
        let c = Contexto {
            avisos_opciones: true,
            ..ctx()
        };
        let g = guia("criterio-caja-renuncia", None, date!(2026 - 11 - 16), &c).unwrap();
        assert_eq!(g.plazo.desde, Some(date!(2026 - 12 - 01)));
        assert_eq!(g.plazo.vence, Some(date!(2026 - 12 - 31)));
        assert!(g.avisos.iter().any(|a| a.contains("036-diciembre")));
        let g = guia("prorrata-especial", None, date!(2026 - 11 - 16), &c).unwrap();
        assert!(!g.por_036);
        assert_eq!(g.plazo.tipo, TipoPlazo::En303);
        assert_eq!(g.plazo.vence, Some(date!(2027 - 02 - 01)));
        // El REDEME en noviembre: el de este año aún está abierto.
        let g = guia("redeme", None, date!(2026 - 11 - 16), &c).unwrap();
        assert_eq!(g.plazo.vence, Some(date!(2026 - 11 - 30)));
    }

    #[test]
    fn roi_antes_de_la_primera_operacion() {
        let g = guia(
            "alta-roi",
            Some(date!(2027 - 01 - 10)),
            date!(2026 - 11 - 16),
            &ctx(),
        )
        .unwrap();
        assert_eq!(g.plazo.tipo, TipoPlazo::Antes);
        assert_eq!(g.plazo.vence, Some(date!(2027 - 01 - 10)));
        assert!(g.norma.contains("25"));
    }
}

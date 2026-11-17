//! El «expediente» de la carpeta: cada fichero con sus datos en una forma común, venga de una
//! lectura (asistente o Claude Code) o de lo que sacan las reglas de [`crate::carpeta`].

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_core::money::{Money, Rate};
use taxpain_core::nif::{Nif, NifError};
use taxpain_vault::Vault;
use time::Date;

use crate::carpeta::{Found, Inventory, Item};
use crate::error::Result;
use crate::lectura_ia::Reading;
use crate::libros::{Book, BookKind};

/// Qué es un fichero, a efectos de la revisión.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Clase {
    FacturaRecibida,
    FacturaEmitida,
    Modelo,
    LibroEmitidas,
    LibroRecibidas,
    Diario,
    Extracto,
    Empresa,
    Contable,
    Nomina,
    /// Historiales y hojas de criptomonedas (los lee Criptomonedas).
    Cripto,
    Otro,
    SinLeer,
}

impl Clase {
    pub fn titulo(self) -> &'static str {
        match self {
            Clase::FacturaRecibida => "Facturas recibidas",
            Clase::FacturaEmitida => "Facturas emitidas",
            Clase::Modelo => "Modelos de la AEAT",
            Clase::LibroEmitidas => "Libros de facturas emitidas",
            Clase::LibroRecibidas => "Libros de facturas recibidas",
            Clase::Diario => "Diarios",
            Clase::Extracto => "Extractos del banco",
            Clase::Empresa => "Documentos de la empresa",
            Clase::Contable => "Balances y cuentas",
            Clase::Nomina => "Nóminas",
            Clase::Cripto => "Criptomonedas",
            Clase::Otro => "Otros documentos",
            Clase::SinLeer => "Sin leer",
        }
    }
}

/// Datos de una factura.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Factura {
    pub emisor_nif: Option<String>,
    pub emisor_nombre: Option<String>,
    pub destinatario_nif: Option<String>,
    pub destinatario_nombre: Option<String>,
    pub numero: Option<String>,
    pub fecha: Option<Date>,
    pub base: Option<Money>,
    pub iva: Option<Money>,
    pub retencion: Option<Money>,
    pub total: Option<Money>,
    /// Moneda ISO 4217 si no es el euro.
    pub moneda: Option<String>,
    /// País del emisor (ISO 3166-1 alfa-2) si la lectura lo trae y no es España.
    pub emisor_pais: Option<String>,
}

impl Factura {
    /// Tipo de IVA en porcentaje entero (21, 19…), si la factura tiene base e IVA.
    pub fn tipo(&self) -> Option<i64> {
        let (b, i) = (self.base?.cents(), self.iva?.cents());
        (b > 0 && i > 0).then(|| (i * 200 / b + 1) / 2)
    }

    /// El IVA es de un tipo español (21, 10, 5 o 4 %), con margen para los redondeos por línea.
    pub fn con_tipo_español(&self) -> bool {
        let (Some(b), Some(i)) = (self.base, self.iva) else {
            return false;
        };
        if b.cents() <= 0 || i.cents() <= 0 {
            return false;
        }
        let margen = (b.cents() / 400).max(2);
        [2100, 1000, 500, 400]
            .into_iter()
            .any(|bp| (b.apply_rate(Rate::from_bp(bp)) - i).abs().cents() <= margen)
    }

    /// El emisor es de fuera de España: su NIF no es español o, si no lo trae, la factura va
    /// en otra moneda o su nombre lleva una forma social de otro país (Inc., LLC, Ltd.…).
    pub fn emisor_extranjero(&self) -> bool {
        match self.emisor_nif.as_deref() {
            Some(n) => nif_español(n).is_none(),
            None => {
                self.emisor_pais.is_some()
                    || self.moneda.is_some()
                    || self.emisor_nombre.as_deref().is_some_and(forma_extranjera)
            }
        }
    }

    /// El emisor es de fuera de la UE por lo que dice la factura: un NIF que no es español ni de
    /// otro país de la UE o, sin NIF, su país o una forma social que sólo se usa fuera de la UE
    /// (Inc., LLC, PBC, Corp., Pte., Pty.).
    pub fn emisor_de_fuera_de_la_ue(&self) -> bool {
        match self.emisor_nif.as_deref() {
            Some(n) => nif_español(n).is_none() && !super::modelos::de_la_ue(n),
            None => match self.emisor_pais.as_deref() {
                Some(p) => p != "ES" && !taxpain_core::eu::is_other_member_state(p, false),
                None => self
                    .emisor_nombre
                    .as_deref()
                    .is_some_and(forma_de_fuera_de_la_ue),
            },
        }
    }

    /// El emisor es de otro país de la UE (por el prefijo de su NIF-IVA o, sin NIF, por su
    /// país).
    pub fn emisor_de_la_ue(&self) -> bool {
        match self.emisor_nif.as_deref() {
            Some(n) => super::modelos::de_la_ue(n),
            None => self
                .emisor_pais
                .as_deref()
                .is_some_and(|p| taxpain_core::eu::is_other_member_state(p, false)),
        }
    }

    /// Clave del emisor para comparar: su NIF normalizado o, si no hay, su nombre en minúsculas.
    pub fn emisor_clave(&self) -> Option<String> {
        self.emisor_nif
            .as_deref()
            .map(nif_clave)
            .filter(|n| !n.is_empty())
            .or_else(|| {
                self.emisor_nombre
                    .as_deref()
                    .map(|n| n.trim().to_lowercase())
                    .filter(|n| !n.is_empty())
            })
    }
}

/// Un modelo de la AEAT con sus casillas (sin ceros a la izquierda: [00370] es 370).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Modelo {
    pub modelo: String,
    pub ejercicio: i32,
    pub periodo: String,
    pub casillas: BTreeMap<u32, Money>,
    /// Fecha de presentación (la del justificante), si se sabe.
    pub presentado: Option<Date>,
    /// Resúmenes anuales (190, 180, 193, 347, 349): una línea por perceptor o tercero.
    pub registros: Vec<Registro>,
    /// Periodo impositivo del 200 (inicio y fin), si se sabe y no es el año natural: el primer
    /// ejercicio de una sociedad o uno de transición duran menos de un año y la LIS prorratea
    /// sus topes.
    pub periodo_desde: Option<Date>,
    pub periodo_hasta: Option<Date>,
    /// No se han podido leer sus casillas (un justificante de la AEAT que se reconoce pero
    /// cuyos importes no cuadran, o una declaración de la AEAT sin casillas): cuenta como
    /// presentado, pero no entra en los cruces. Sus casillas no son cero: no se saben.
    pub sin_casillas: bool,
    /// Casillas que no se han podido leer (no se sabe cuánto valen): las de la liquidación del
    /// 200 cuando no cuadran entre sí.
    pub dudosas: BTreeSet<u32>,
    /// Del justificante de la AEAT: su número y el de la declaración a la que complementa o
    /// sustituye.
    pub justificante: Option<String>,
    pub anterior: Option<String>,
    /// Normal, complementaria o sustitutiva de otra del mismo periodo (`Normal` si no se sabe):
    /// lo dice el justificante de la AEAT o, sin él, la lectura, su descripción, el nombre del
    /// fichero o el registro de declarante del fichero. En los resúmenes anuales la
    /// complementaria sólo trae lo que faltaba (ver [`super::modelos::declarados`]).
    pub tipo: TipoDeclaracion,
    /// Informativas: número de registros (perceptores, declarados u operadores), si se sabe.
    pub num_registros: Option<u32>,
    /// La portada del justificante dice si se pidió la devolución. `None` si no se sabe (no se
    /// ha leído el justificante).
    pub devolucion: Option<bool>,
    /// Si complementa o sustituye a otra (lo dice la AEAT): cuándo se presentó la primera del
    /// periodo, si la AEAT la tiene.
    pub original: Option<Date>,
}

/// Declaración normal, complementaria o sustitutiva de otra del mismo periodo: la del
/// justificante de la AEAT ([`crate::lectura_justificante::Tipo`]), con lo que dicen de ella los
/// textos y los ficheros de las que no lo son.
pub(crate) use crate::lectura_justificante::Tipo as TipoDeclaracion;

impl TipoDeclaracion {
    /// Por lo que dice un texto (la lectura, su descripción o el nombre del fichero): sólo si
    /// nombra una de las dos.
    pub(crate) fn de_texto(texto: &str) -> Option<Self> {
        let t = crate::documentos::fold(texto);
        match (t.contains("complementari"), t.contains("sustitutiv")) {
            (true, false) => Some(Self::Complementaria),
            (false, true) => Some(Self::Sustitutiva),
            _ => None,
        }
    }

    /// Por el registro de declarante de un fichero de 500 posiciones: «C» en la posición 121 o
    /// «S» en la 122 (art. 11 y anexo II de la Orden EHA/3127/2009 en el 190; los demás
    /// resúmenes llevan el mismo campo).
    pub(crate) fn de_fichero(bytes: &[u8]) -> Option<Self> {
        let texto = match std::str::from_utf8(bytes) {
            Ok(s) => s.to_owned(),
            Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
        };
        let primero: Vec<char> = texto.lines().next()?.chars().take(500).collect();
        match (primero.get(120), primero.get(121)) {
            (Some('C'), _) => Some(Self::Complementaria),
            (_, Some('S')) => Some(Self::Sustitutiva),
            _ => None,
        }
    }
}

/// Una línea de un resumen anual: un perceptor (190, 180, 193) o un tercero (347, 349).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Registro {
    pub nif: Option<String>,
    pub nombre: Option<String>,
    /// Clave de la operación o de la percepción («A», «B», «G», «E»…), en mayúsculas.
    pub clave: Option<String>,
    pub importe: Option<Money>,
    pub retencion: Option<Money>,
}

/// Una nómina de la carpeta: de quién es y, si se sabe, el mes, el bruto y la retención.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Nomina {
    pub nif: Option<String>,
    pub nombre: Option<String>,
    /// Mes de la nómina (su día 1).
    pub mes: Option<Date>,
    pub bruto: Option<Money>,
    pub retencion: Option<Money>,
    /// Tipo de retención del IRPF en centésimas de punto (1140 es el 11,40 %).
    pub tipo: Option<i32>,
}

/// Trámite del Registro Mercantil.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TipoTramite {
    DepositoCuentas,
    LegalizacionLibros,
}

/// Justificante del Registro Mercantil (o cuentas anuales) con lo que se ha podido leer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tramite {
    pub tipo: TipoTramite,
    pub ejercicio: Option<i32>,
    pub presentado: Option<Date>,
    /// Resultado del ejercicio de la cuenta de pérdidas y ganancias depositada.
    pub resultado: Option<Money>,
    pub cifra_negocios: Option<Money>,
}

impl Modelo {
    /// Importe de una casilla (cero si no está).
    pub fn c(&self, n: u32) -> Money {
        self.casillas.get(&n).copied().unwrap_or(Money::ZERO)
    }

    /// Días del periodo impositivo: 365 si no se sabe o es un año entero (365 o 366); menos en
    /// un periodo corto.
    pub fn dias_periodo(&self) -> i64 {
        match (self.periodo_desde, self.periodo_hasta) {
            (Some(d), Some(h)) if h >= d => ((h - d).whole_days() + 1).min(365),
            _ => 365,
        }
    }
}

/// Un fichero de la carpeta con lo que se sabe de él.
#[derive(Clone, Debug)]
pub(crate) struct Doc {
    pub ruta: String,
    pub sha: String,
    pub clase: Clase,
    pub descripcion: String,
    /// Leído con el asistente o con Claude Code (si no, por su formato o con las reglas).
    pub con_ia: bool,
    /// Año y trimestre que dice la ruta (`…/2025/T3/…`), si los dice.
    pub ruta_año: Option<i32>,
    pub ruta_trimestre: Option<u8>,
    pub factura: Option<Factura>,
    pub modelo: Option<Modelo>,
    pub libro: Option<Book>,
    /// Líneas de un libro de emitidas que son la autorrepercusión de compras y no ventas
    /// ([`crate::libros::BookInvoice::self_assessed`]): se sacan del libro para que no cuenten
    /// como ventas en los cruces.
    pub autorrepercusion: Vec<crate::libros::BookInvoice>,
    /// Extractos bancarios leídos del fichero (con lo que salió al comprobar sus saldos).
    pub extractos: Vec<crate::extractos::Leido>,
    /// Por qué no se ha podido leer, si es un extracto que no lee ningún lector.
    pub extracto_error: Option<String>,
    /// Documento de la empresa (escritura, estatutos…) con sus datos.
    pub empresa: Option<(crate::documentos::DocKind, crate::documentos::Facts)>,
    /// Justificante del Registro Mercantil (depósito de cuentas, legalización de libros).
    pub tramite: Option<Tramite>,
    /// Acta que aprueba las cuentas: cómo se aplica el resultado.
    pub aplicacion: Option<super::actas::Aplicacion>,
    /// Nómina: de quién es, el bruto y la retención.
    pub nomina: Option<Nomina>,
    /// Emitida en PDF, sin IVA, a un cliente de otro país de la UE: si su texto dice por qué no
    /// lo lleva (inversión del sujeto pasivo, exenta, no sujeta…). `None` si no se sabe.
    pub mencion_sin_iva: Option<bool>,
    /// Documento de la Sede de la AEAT (certificado, requerimiento, recibo de registro…) con lo
    /// que dice su texto.
    pub oficial: Option<crate::oficiales::Oficial>,
    /// Su nombre dice un modelo, pero tiene texto y no es de la AEAT (ni su código seguro de
    /// verificación ni su nombre): un cálculo o un correo de la gestoría, no el justificante.
    /// `false` si no tiene texto (un escaneado) o no se ha mirado.
    pub texto_no_aeat: bool,
    /// Documento de una participada o que habla de dividendos: su texto (ruta, descripción y
    /// texto del PDF) en minúsculas y sin tildes, para confirmar los dividendos cobrados (ver
    /// [`super::participadas`]).
    pub participada: Option<String>,
}

impl Doc {
    pub fn con_detalle(&self) -> bool {
        self.clase != Clase::SinLeer
    }
}

/// Todo lo que se revisa.
#[derive(Debug)]
pub(crate) struct Expediente {
    pub carpeta: String,
    pub empresa: String,
    pub own_nif: Option<String>,
    pub docs: Vec<Doc>,
    /// Ficheros de formatos que TaxPain no lee.
    pub no_admitidos: Vec<String>,
    /// Modelos del gestor ya guardados en TaxPain (Importar → Comparar).
    pub presentados: Vec<crate::presentados::Filed>,
    pub perfil: Option<TaxpayerProfile>,
    /// Sin perfil guardado, el deducido de lo presentado (ver [`crate::perfil_deducido`]): sólo
    /// lo usan los modelos que faltan, con su nota y sin pedir lo de años sin ese modelo cerca.
    pub perfil_deducido: Option<crate::perfil_deducido::PerfilDeducido>,
    /// Lo que TaxPain ya sabe de la empresa y sirve para los cruces (nóminas, banco, saldos).
    pub taxpain: super::cruces::datos::DatosTaxPain,
    /// Las declaraciones que tiene la AEAT («Comprobar lo presentado»), como modelos: no son
    /// ficheros de la carpeta, pero entran en los cruces. Si la misma declaración está además en
    /// la carpeta, cuenta una vez (`modelos::declarados`).
    pub aeat: Vec<Doc>,
    /// Lo que dijo «Comprobar lo presentado» de cada ejercicio que se comprobó (de los que aún
    /// se revisan): lo que tocaba y la AEAT no tiene.
    pub comprobado: BTreeMap<i32, Comprobado>,
}

/// Lo que la AEAT no tiene de un ejercicio comprobado con «Comprobar lo presentado»: lo que
/// tocaba seguro (o por las retenciones o los importes de TaxPain) y lo que sólo tocaba en
/// ciertos casos, con su nota. Sólo de lo que había vencido al comprobar y de los modelos de los
/// que contestó la AEAT: lo demás no lo dice la AEAT y se mira con el perfil fiscal.
#[derive(Clone, Debug, Default)]
pub(crate) struct Comprobado {
    /// Cuándo se consultó (`aaaa-mm-dd…`).
    pub cuando: String,
    /// El día de la consulta, si se puede leer.
    pub fecha: Option<Date>,
    pub faltan: Vec<(String, String)>,
    pub no_consta: Vec<(String, String, String)>,
    /// Modelos de los que contestó la AEAT (sin los que fallaron, los que no se consultaron y
    /// los que el servicio no permite consultar).
    pub respondidos: BTreeSet<String>,
}

/// Lo que dijo «Comprobar lo presentado» de los ejercicios que aún se revisan. Lo que no se
/// pueda leer queda fuera: la revisión sigue con el perfil fiscal.
fn comprobados(
    v: &Vault,
    hoy: Date,
    perfil: Option<&TaxpayerProfile>,
) -> BTreeMap<i32, Comprobado> {
    use crate::presentado_aeat::Status;
    let desde = super::revisable_desde(hoy);
    let mut out = BTreeMap::new();
    for y in crate::presentado_aeat::checked_years(v).unwrap_or_default() {
        if y < desde || y > hoy.year() {
            continue;
        }
        let Ok(r) = crate::presentado_aeat::report_for(v, y, hoy, perfil.cloned()) else {
            continue;
        };
        let Some(cuando) = r.checked_at.clone() else {
            continue;
        };
        let fecha = cuando.get(..10).and_then(crate::import::parse_date);
        let mut c = Comprobado {
            cuando,
            fecha,
            respondidos: r.answered.iter().cloned().collect(),
            ..Comprobado::default()
        };
        for row in r.rows {
            // Lo que vencía después de la consulta: la AEAT no dijo si se presentó.
            if row
                .due_on
                .zip(fecha)
                .is_none_or(|(vence, consulta)| vence >= consulta)
                && row.status != Status::Presented
            {
                continue;
            }
            match row.status {
                Status::Missing => c.faltan.push((row.model, row.period.to_uppercase())),
                Status::NotFound => c.no_consta.push((
                    row.model,
                    row.period.to_uppercase(),
                    row.note.unwrap_or_default(),
                )),
                Status::Presented | Status::Open => {}
            }
        }
        out.insert(y, c);
    }
    out
}

impl Expediente {
    pub fn por_tipo(&self) -> Vec<super::Cuenta> {
        let mut n: BTreeMap<Clase, usize> = BTreeMap::new();
        for d in &self.docs {
            *n.entry(d.clase).or_default() += 1;
        }
        let mut out: Vec<super::Cuenta> = n
            .into_iter()
            .map(|(c, n)| super::Cuenta {
                tipo: c.titulo().to_owned(),
                n,
            })
            .collect();
        if !self.no_admitidos.is_empty() {
            out.push(super::Cuenta {
                tipo: "Formatos que TaxPain no lee".into(),
                n: self.no_admitidos.len(),
            });
        }
        out
    }

    /// ¿Es de la empresa este NIF?
    pub fn es_propio(&self, nif: &str) -> bool {
        self.own_nif
            .as_deref()
            .is_some_and(|own| nif_clave(own) == nif_clave(nif))
    }

    pub fn facturas(&self) -> impl Iterator<Item = (&Doc, &Factura)> {
        self.docs
            .iter()
            .filter_map(|d| d.factura.as_ref().map(|f| (d, f)))
    }

    pub fn recibidas(&self) -> impl Iterator<Item = (&Doc, &Factura)> {
        self.facturas()
            .filter(|(d, _)| d.clase == Clase::FacturaRecibida)
    }

    pub fn emitidas(&self) -> impl Iterator<Item = (&Doc, &Factura)> {
        self.facturas()
            .filter(|(d, _)| d.clase == Clase::FacturaEmitida)
    }

    pub fn modelos(&self) -> impl Iterator<Item = (&Doc, &Modelo)> {
        self.docs
            .iter()
            .chain(self.aeat.iter())
            .filter_map(|d| d.modelo.as_ref().map(|m| (d, m)))
    }

    /// Los documentos de la Sede de la AEAT de la carpeta, con su tipo y lo que dice su texto.
    pub fn oficiales(
        &self,
    ) -> impl Iterator<Item = (&Doc, crate::documentos::DocKind, &crate::oficiales::Oficial)> {
        self.docs
            .iter()
            .filter_map(|d| match (&d.empresa, &d.oficial) {
                (Some((k, _)), Some(o)) => Some((d, *k, o)),
                _ => None,
            })
    }

    /// El certificado de situación censal más reciente de la carpeta (el que vale mientras no
    /// cambien sus datos).
    pub fn censo(&self) -> Option<(&Doc, &crate::oficiales::Oficial)> {
        self.oficiales()
            .filter(|(_, k, _)| *k == crate::documentos::DocKind::CertificadoSituacionCensal)
            .map(|(d, _, o)| (d, o))
            .max_by_key(|(_, o)| o.fecha)
    }

    /// La primera fecha de alta en el IVA (o en una actividad) de los certificados de situación
    /// censal de la carpeta: antes no tocaban los modelos de IVA.
    pub fn alta_iva(&self) -> Option<Date> {
        self.oficiales()
            .filter(|(_, k, _)| *k == crate::documentos::DocKind::CertificadoSituacionCensal)
            .filter_map(|(_, _, o)| o.alta_iva)
            .min()
    }
}

/// Construye el expediente de la carpeta a partir de su inventario.
pub(crate) fn construir<G: Deref<Target = Vault>>(
    root: &Path,
    inv: &Inventory,
    vault: &impl Fn() -> Result<G>,
    hoy: Date,
) -> Result<Expediente> {
    let (empresa, own_nif, presentados, perfil, taxpain, aeat, mut datos) = {
        let v = vault()?;
        let perfil = v.setting::<TaxpayerProfile>("taxpayer_profile")?;
        let datos = if perfil.is_none() {
            Some(crate::perfil_deducido::datos(&v)?)
        } else {
            None
        };
        (
            crate::companies::info(&v)
                .map(|i| i.name)
                .unwrap_or_default(),
            crate::carpeta::own_nif(&v),
            crate::presentados::list(&v, None)?,
            perfil,
            super::cruces::datos::cargar(&v, hoy),
            docs_aeat(&v)?,
            datos,
        )
    };
    let mut docs = Vec::new();
    // Un zip sin criptomonedas que leer es, para la revisión, un formato que no se lee (como
    // antes de abrir los zip): va con los no admitidos, no con los documentos.
    let zip_sin_leer =
        |i: &crate::carpeta::Item| es_zip(&i.path) && matches!(i.found, Found::Otro { .. });
    let mut no_admitidos = no_admitidos(root);
    no_admitidos.extend(
        inv.items
            .iter()
            .filter(|i| zip_sin_leer(i))
            .map(|i| super::componer(&i.path)),
    );
    no_admitidos.sort();
    for item in inv.items.iter().filter(|i| !zip_sin_leer(i)) {
        let Ok((nombre, bytes)) = crate::carpeta::read_file(root, &item.path) else {
            continue;
        };
        let sha = hex::encode(Sha256::digest(&bytes));
        let lectura = if matches!(item.found, Found::Ia { .. }) {
            crate::lectura_ia::stored(&*vault()?, &bytes)?
        } else {
            None
        };
        let mut d = doc(
            item,
            &nombre,
            &bytes,
            sha,
            lectura.clone(),
            own_nif.as_deref(),
        );
        // Las casillas de un justificante que no cuadraron al leerlo o que no lee (el número de
        // perceptores del 111), de la lectura de IA del mismo fichero, si la hay.
        if d.modelo.as_ref().is_some_and(|m| m.justificante.is_some())
            && let Some(r) = crate::lectura_ia::stored(&*vault()?, &bytes)?
            && let Some(m) = d.modelo.as_mut()
        {
            completar_con_la_lectura(m, &r);
        }
        if d.clase == Clase::Extracto {
            extractos(&mut d, &nombre, &bytes, lectura.as_ref());
        }
        if d.modelo.is_none() && super::modelos::modelo_por_nombre(&nombre).is_some() {
            d.texto_no_aeat = texto_no_aeat(&nombre, &bytes);
        }
        docs.push(d);
    }
    autorrepercusion(&mut docs);
    // El perfil deducido, también con los modelos y el certificado de situación censal de la
    // carpeta.
    let perfil_deducido = datos.as_mut().and_then(|d| {
        d.vistas.extend(
            docs.iter()
                .filter_map(|x| x.modelo.as_ref())
                .map(|m| (m.modelo.clone(), m.ejercicio, m.periodo.to_uppercase())),
        );
        let censo = docs
            .iter()
            .filter_map(|x| match (&x.empresa, &x.oficial) {
                (Some((crate::documentos::DocKind::CertificadoSituacionCensal, _)), Some(o)) => {
                    Some((o.fecha, o.modelos_obligados()))
                }
                _ => None,
            })
            .max_by_key(|(f, _)| *f);
        if let Some(c) = censo
            && d.censo.as_ref().is_none_or(|(f, _)| c.0 > *f)
        {
            d.censo = Some(c);
        }
        crate::perfil_deducido::de(d)
    });
    let comprobado = comprobados(
        &*vault()?,
        hoy,
        perfil
            .as_ref()
            .or(perfil_deducido.as_ref().map(|d| &d.perfil)),
    );
    Ok(Expediente {
        carpeta: root.to_string_lossy().into_owned(),
        empresa,
        own_nif,
        docs,
        no_admitidos,
        presentados,
        perfil,
        perfil_deducido,
        taxpain,
        aeat,
        comprobado,
    })
}

/// Saca de los libros de emitidas las líneas que son la autorrepercusión de compras: las que lo
/// son por sus importes o por su hoja, y las que son la misma factura que una de un libro de
/// recibidas de la carpeta.
fn autorrepercusion(docs: &mut [Doc]) {
    let recibidas: Vec<crate::libros::BookInvoice> = docs
        .iter()
        .filter(|d| d.clase == Clase::LibroRecibidas)
        .flat_map(|d| d.libro.iter().flat_map(|l| l.invoices.iter().cloned()))
        .collect();
    for d in docs.iter_mut() {
        if d.clase != Clase::LibroEmitidas {
            continue;
        }
        let Some(l) = d.libro.as_mut() else {
            continue;
        };
        if crate::libros::mark_self_assessed(l, &recibidas) == 0 {
            continue;
        }
        let (fuera, ventas): (Vec<_>, Vec<_>) = std::mem::take(&mut l.invoices)
            .into_iter()
            .partition(|i| i.self_assessed);
        l.invoices = ventas;
        d.autorrepercusion = fuera;
    }
}

/// ¿Tiene texto que no es de la AEAT? Un PDF sin texto (escaneado) o una foto pueden ser el
/// justificante; un PDF o un fichero de texto con texto, sólo si lleva el nombre de la Agencia
/// o su código seguro de verificación.
fn texto_no_aeat(nombre: &str, bytes: &[u8]) -> bool {
    let texto = if bytes.starts_with(b"%PDF") {
        crate::pdf_text::text(bytes).unwrap_or_default()
    } else if nombre.to_lowercase().ends_with(".txt") {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        return false;
    };
    if texto.trim().chars().count() <= 40 {
        return false;
    }
    let t = crate::documentos::fold(&texto);
    !(crate::oficiales::de_la_aeat(&t)
        || t.contains("agencia tributaria")
        || t.contains("agenciatributaria")
        || t.contains("agencia estatal de administracion tributaria"))
}

/// ¿Dicen lo mismo dos lecturas de una declaración en las casillas que traen las dos (salvo las
/// dudosas), con un céntimo de margen? Si no, una de las dos está mal leída y no se mezclan.
pub(crate) fn casillas_que_coinciden(
    a: &BTreeMap<u32, Money>,
    b: &BTreeMap<u32, Money>,
    dudosas: &BTreeSet<u32>,
) -> bool {
    a.iter()
        .filter(|(k, _)| !dudosas.contains(k))
        .all(|(k, x)| {
            b.get(k)
                .is_none_or(|y| (*x - *y).abs() <= Money::from_cents(1))
        })
}

/// Las casillas dudosas de un modelo (las que no cuadraron al leer su justificante) y las que no
/// trae (el lector del justificante no lee el número de perceptores del 111, por ejemplo), de la
/// lectura de IA del mismo fichero si es de la misma declaración (modelo, ejercicio y periodo).
/// Las que la lectura no trae siguen dudosas; las que ya se leyeron del justificante no cambian,
/// y si la lectura dice otra cosa en alguna de ellas (una reimpresión con las filas movidas), no
/// se toma ninguna más.
fn completar_con_la_lectura(m: &mut Modelo, r: &Reading) {
    let Some(otra) = r.modelo.as_ref().and_then(modelo) else {
        return;
    };
    if (otra.modelo.as_str(), otra.ejercicio, otra.periodo.as_str())
        != (m.modelo.as_str(), m.ejercicio, m.periodo.as_str())
    {
        return;
    }
    let dudosas = std::mem::take(&mut m.dudosas);
    for k in dudosas {
        match otra.casillas.get(&k) {
            Some(v) => {
                m.casillas.insert(k, *v);
            }
            None => {
                m.dudosas.insert(k);
            }
        }
    }
    if m.sin_casillas || !casillas_que_coinciden(&m.casillas, &otra.casillas, &m.dudosas) {
        return;
    }
    for (k, v) in otra.casillas {
        if !m.dudosas.contains(&k) {
            m.casillas.entry(k).or_insert(v);
        }
    }
}

/// El expediente de la empresa sin carpeta: sólo lo que tiene la AEAT y lo que sabe TaxPain.
pub(crate) fn sin_carpeta(v: &Vault, hoy: Date) -> Result<Expediente> {
    let perfil = v.setting::<TaxpayerProfile>("taxpayer_profile")?;
    let perfil_deducido = if perfil.is_none() {
        crate::perfil_deducido::deducir(v)?
    } else {
        None
    };
    let comprobado = comprobados(
        v,
        hoy,
        perfil
            .as_ref()
            .or(perfil_deducido.as_ref().map(|d| &d.perfil)),
    );
    Ok(Expediente {
        carpeta: String::new(),
        empresa: crate::companies::info(v)
            .map(|i| i.name)
            .unwrap_or_default(),
        own_nif: crate::carpeta::own_nif(v),
        docs: Vec::new(),
        no_admitidos: Vec::new(),
        presentados: crate::presentados::list(v, None)?,
        perfil,
        perfil_deducido,
        taxpain: super::cruces::datos::cargar(v, hoy),
        aeat: docs_aeat(v)?,
        comprobado,
    })
}

/// Las casillas de una declaración leída de la AEAT (de su fichero o de su PDF). Las del 123 de
/// hasta 2023 (diseño de 2015: [02] base y [03] retención) se pasan a las del de 2024 ([06] y
/// [09], los totales) para que los cruces las encuentren; las informativas llevan su base en la
/// [02] y sus retenciones en la [03], como los resúmenes leídos de la carpeta.
fn casillas_aeat(
    modelo: &str,
    ejercicio: i32,
    b: &crate::lectura_justificante::FilingBoxes,
) -> BTreeMap<u32, Money> {
    let mut casillas = b.casillas.clone();
    if modelo == "123" && ejercicio < 2024 {
        casillas = [(6, b.get(2)), (9, b.get(3))]
            .into_iter()
            .filter(|(_, m)| !m.is_zero())
            .collect();
    }
    if let Some(i) = b.importe {
        casillas.insert(2, i);
    }
    if let Some(r) = b.retenciones {
        casillas.insert(3, r);
    }
    casillas
}

/// Las líneas del 347 y el 349 leídas de la AEAT.
fn registros_aeat(b: &crate::lectura_justificante::FilingBoxes) -> Vec<Registro> {
    b.lineas
        .iter()
        .map(|l| Registro {
            nif: Some(nif_clave(&l.nif)).filter(|n| !n.is_empty()),
            nombre: Some(l.nombre.clone()).filter(|n| !n.is_empty()),
            clave: Some(l.clave.to_uppercase()).filter(|k| !k.is_empty()),
            importe: Some(l.importe),
            retencion: None,
        })
        .collect()
}

/// El modelo de una declaración de la AEAT (ver [`casillas_aeat`]), y el 347 y el 349 con sus
/// líneas. Sin casillas leídas, no entra en los cruces.
pub(crate) fn modelo_aeat(x: &crate::presentado_aeat::Vigente) -> Modelo {
    let b = x.boxes.clone().unwrap_or_default();
    Modelo {
        modelo: x.model.clone(),
        ejercicio: x.year,
        periodo: x.period.to_uppercase(),
        casillas: casillas_aeat(&x.model, x.year, &b),
        presentado: x.presented_at.get(..10).and_then(|d| {
            Date::parse(d, time::macros::format_description!("[year]-[month]-[day]")).ok()
        }),
        registros: registros_aeat(&b),
        sin_casillas: x.boxes.is_none(),
        dudosas: b.dudosas.clone(),
        justificante: Some(x.receipt.clone()),
        anterior: x.previous.clone(),
        num_registros: b.registros,
        original: x.original_presented_at.as_deref().and_then(|d| {
            Date::parse(
                d.get(..10)?,
                time::macros::format_description!("[year]-[month]-[day]"),
            )
            .ok()
        }),
        ..Modelo::default()
    }
}

/// El modelo de un justificante de la AEAT de la carpeta: el modelo, el ejercicio, el periodo,
/// la fecha de presentación y su número los da la portada; las casillas, sólo si se han podido
/// leer y cuadran.
pub(crate) fn modelo_justificante(j: &crate::lectura_justificante::Justificante) -> Modelo {
    let b = j.casillas.clone().unwrap_or_default();
    Modelo {
        modelo: j.modelo.clone(),
        ejercicio: j.ejercicio,
        periodo: j.periodo.clone(),
        casillas: casillas_aeat(&j.modelo, j.ejercicio, &b),
        presentado: j.presentado,
        registros: registros_aeat(&b),
        sin_casillas: j.casillas.is_none(),
        dudosas: b.dudosas,
        justificante: Some(j.numero.clone()),
        anterior: j.anterior.clone(),
        tipo: j.tipo,
        num_registros: b.registros,
        devolucion: Some(j.devolucion),
        ..Modelo::default()
    }
}

/// Las declaraciones vigentes que tiene la AEAT, como documentos del expediente.
fn docs_aeat(v: &Vault) -> Result<Vec<Doc>> {
    Ok(crate::presentado_aeat::vigentes(v)?
        .iter()
        .map(|x| Doc {
            ruta: format!(
                "AEAT · modelo {} {} {} · justificante {}",
                x.model, x.year, x.period, x.receipt
            ),
            sha: String::new(),
            clase: Clase::Modelo,
            descripcion: format!(
                "Modelo {} de {} {} presentado según la AEAT",
                x.model, x.year, x.period
            ),
            con_ia: false,
            ruta_año: Some(x.year),
            ruta_trimestre: None,
            factura: None,
            modelo: Some(modelo_aeat(x)),
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
        })
        .collect())
}

/// Base, IVA, retención y destinatario de una factura en PDF leída por reglas (sin lectura de
/// IA): sin ellos, la revisión toma una factura al 10 % por «sin IVA español» o no ve las
/// retenciones del 111. Lo que no se lee se queda sin dato (desconocido, no cero).
fn importes_del_pdf(f: &mut Factura, nombre: &str, bytes: &[u8], own: Option<&str>) {
    let e = crate::received::extract(nombre, bytes, own);
    f.base = e.base.map(|x| x.value);
    f.iva = e.vat.map(|x| x.value);
    f.retencion = e.withholding.map(|x| x.value);
    f.destinatario_nif = e.recipient_nif.map(|x| x.value);
}

/// Los extractos del fichero: con sus lectores o, si no lo reconoce ninguno (o no cuadra), con
/// lo que trae la lectura de IA.
fn extractos(d: &mut Doc, nombre: &str, bytes: &[u8], lectura: Option<&Reading>) {
    match crate::extractos::leer_con_lectura(nombre, bytes, lectura) {
        Ok(l) => d.extractos = l,
        Err(e) => d.extracto_error = Some(e.to_string()),
    }
}

fn doc(
    item: &Item,
    nombre: &str,
    bytes: &[u8],
    sha: String,
    lectura: Option<Reading>,
    own: Option<&str>,
) -> Doc {
    let (ruta_año, ruta_trimestre) = periodo_de_ruta(&item.path);
    let mut d = Doc {
        ruta: super::componer(&item.path),
        sha,
        clase: Clase::SinLeer,
        descripcion: String::new(),
        con_ia: false,
        ruta_año,
        ruta_trimestre,
        factura: None,
        modelo: None,
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
    if let Some(r) = &lectura {
        de_lectura(&mut d, nombre, bytes, r, own);
        de_otra_empresa(&mut d, nombre, bytes, own);
        super::cruces::datos::completar(&mut d, nombre, bytes);
        super::actas::completar(&mut d, bytes, Some(r));
        super::participadas::completar(&mut d, bytes);
        mencion_sin_iva(&mut d, bytes);
        return d;
    }
    match &item.found {
        Found::FacturaRecibida {
            supplier,
            nif,
            number,
            date,
            total,
            ..
        } => {
            d.clase = Clase::FacturaRecibida;
            let mut f = Factura {
                emisor_nif: nif.clone(),
                emisor_nombre: supplier.clone(),
                numero: number.clone(),
                fecha: *date,
                total: *total,
                ..Factura::default()
            };
            importes_del_pdf(&mut f, nombre, bytes, own);
            d.factura = Some(f);
        }
        Found::FacturaEmitida {
            number,
            date,
            total,
        } => {
            d.clase = Clase::FacturaEmitida;
            let mut f = Factura {
                emisor_nif: own.map(str::to_owned),
                numero: number.clone(),
                fecha: *date,
                total: *total,
                ..Factura::default()
            };
            importes_del_pdf(&mut f, nombre, bytes, own);
            d.factura = Some(f);
        }
        Found::Modelo { .. } => {
            d.clase = Clase::Modelo;
            if let Some(j) = crate::lectura_justificante::leer_justificante(bytes) {
                d.descripcion = format!(
                    "Justificante de la AEAT del modelo {} de {} {}",
                    j.modelo, j.ejercicio, j.periodo
                );
                // Un justificante de otra empresa (una participada) no es de ésta.
                if let (Some(nif), Some(own)) = (j.nif.as_deref(), own)
                    && nif_clave(nif) != nif_clave(own)
                {
                    d.clase = Clase::Otro;
                    d.descripcion = format!("Modelo presentado por otra empresa (NIF {nif})");
                    super::cruces::datos::completar(&mut d, nombre, bytes);
                    return d;
                }
                d.modelo = Some(modelo_justificante(&j));
                super::cruces::datos::completar(&mut d, nombre, bytes);
                return d;
            }
            d.modelo = crate::presentados::read(bytes)
                .map(|r| Modelo {
                    modelo: r.model,
                    ejercicio: r.year,
                    periodo: r.period.to_uppercase(),
                    casillas: r
                        .boxes
                        .into_iter()
                        .map(|(k, m)| (u32::from(k), m))
                        .collect(),
                    ..Modelo::default()
                })
                .or_else(|| {
                    crate::presentados::read_informative(bytes).map(|r| Modelo {
                        tipo: TipoDeclaracion::de_fichero(bytes).unwrap_or_default(),
                        ..de_registros(r)
                    })
                });
            if let Some(m) = d.modelo.as_mut()
                && m.tipo == TipoDeclaracion::Normal
                && let Some(t) = TipoDeclaracion::de_texto(&d.ruta)
            {
                m.tipo = t;
            }
        }
        // Que la lectura con IA no lo llame libro de emitidas no dice que sus líneas sean la
        // autorrepercusión de compras: eso sólo sale de sus importes, de su hoja o del libro de
        // recibidas ([`autorrepercusion`]).
        Found::Libro { book, .. } => {
            d.clase = match book {
                BookKind::Emitidas => Clase::LibroEmitidas,
                BookKind::Recibidas => Clase::LibroRecibidas,
            };
            d.libro = crate::libros::parse(nombre, bytes).ok().flatten();
        }
        Found::Diario { .. } => d.clase = Clase::Diario,
        Found::Banco { .. } => d.clase = Clase::Extracto,
        Found::Documento {
            title,
            doc_kind,
            facts,
            ..
        } => {
            d.clase = Clase::Empresa;
            d.descripcion.clone_from(title);
            d.empresa = Some((*doc_kind, (**facts).clone()));
            if doc_kind.de_la_aeat() {
                let texto = if bytes.starts_with(b"%PDF") {
                    crate::pdf_text::text(bytes)
                } else {
                    Some(String::from_utf8_lossy(bytes).into_owned())
                };
                d.oficial = texto.map(|t| crate::oficiales::leer(&t));
            } else {
                modelo_200_de_pdf(&mut d, nombre, bytes);
            }
        }
        Found::Ia { descripcion, .. } => d.descripcion.clone_from(descripcion),
        // Lo calcula Criptomonedas. Con punto de partida, las operaciones antiguas incompletas
        // son historial y no se señalan; sin él, se dice que faltan.
        Found::Cripto {
            resumen, huecos, ..
        } => {
            d.clase = Clase::Cripto;
            d.descripcion = format!("Criptomonedas: {resumen}");
            if *huecos > 0 {
                d.descripcion.push_str(&format!(
                    " · faltan operaciones ({huecos} ventas o traspasos de más unidades de las \
                     que había): pon el punto de partida en Criptomonedas con los cálculos de la \
                     gestoría"
                ));
            }
        }
        Found::Valores {
            formato, resumen, ..
        } => {
            d.descripcion = format!("Extracto de bróker, {formato}: {resumen}");
        }
        Found::Otro { reason } => {
            d.descripcion.clone_from(reason);
            modelo_200_de_pdf(&mut d, nombre, bytes);
        }
    }
    de_otra_empresa(&mut d, nombre, bytes, own);
    super::cruces::datos::completar(&mut d, nombre, bytes);
    super::actas::completar(&mut d, bytes, None);
    super::participadas::completar(&mut d, bytes);
    mencion_sin_iva(&mut d, bytes);
    d
}

/// En una emitida en PDF sin IVA a un cliente de otro país de la UE, ¿dice el texto por qué no
/// lo lleva? «Inversión del sujeto pasivo» (o «reverse charge») en los servicios, o la exención
/// de las entregas de bienes (art. 6.1.j y m del Reglamento de facturación).
fn mencion_sin_iva(d: &mut Doc, bytes: &[u8]) {
    let Some(f) = d
        .factura
        .as_ref()
        .filter(|_| d.clase == Clase::FacturaEmitida)
    else {
        return;
    };
    let a_la_ue = f
        .destinatario_nif
        .as_deref()
        .is_some_and(super::modelos::de_la_ue);
    // Sin IVA: la cuota leída es cero o, sin cuota leída, el total es la base. Una con IVA de
    // la que no se leyó la cuota no lo es.
    let sin_iva = match (f.iva, f.base, f.total) {
        (Some(i), ..) => i.is_zero(),
        (None, Some(b), Some(t)) => (t - b).abs() <= Money::from_cents(1),
        _ => false,
    };
    if !a_la_ue || !sin_iva || !bytes.starts_with(b"%PDF") {
        return;
    }
    let Some(texto) = crate::pdf_text::text(bytes) else {
        return;
    };
    if texto.trim().is_empty() {
        return;
    }
    d.mencion_sin_iva = Some(menciona_sin_iva(&texto));
}

/// ¿Dice un texto por qué una factura no lleva IVA? En castellano, catalán y valenciano,
/// gallego, euskera o inglés (la factura puede ir en cualquier lengua, art. 12.2 del Reglamento
/// de facturación), o abreviado («ISP», «inv. suj. pasivo»).
pub(crate) fn menciona_sin_iva(texto: &str) -> bool {
    let t: String = texto
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' => 'a',
            'é' | 'è' => 'e',
            'í' | 'ì' => 'i',
            'ó' | 'ò' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            c if c.is_whitespace() => ' ',
            c => c,
        })
        .collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let isp = t.split(|c: char| !c.is_alphanumeric()).any(|w| w == "isp");
    isp || [
        "sujeto pasivo",
        "subjecte passiu",
        "suxeito pasivo",
        "subjektu pasibo",
        "suj. pasivo",
        "suj pasivo",
        "s. pasivo",
        "inversion del s.p",
        "reverse charge",
        "reverse-charge",
        "autoliquidacion",
        "exenta",
        "exento",
        "exempt",
        "no sujeta",
        "no sujeto",
        "no subjecta",
        "no subjecte",
        "no suxeita",
        "not subject",
        "2006/112",
        "art. 25",
        "articulo 25",
        "art. 69",
        "articulo 69",
        "article 44",
        "article 138",
        "article 196",
        "art. 196",
    ]
    .iter()
    .any(|m| t.contains(m))
}

/// Un modelo en PDF que presentó otra empresa (el de una participada que está en la carpeta,
/// por ejemplo) no es de esta: pasa a «otro documento» para no tomarlo por suyo.
fn de_otra_empresa(d: &mut Doc, nombre: &str, bytes: &[u8], own: Option<&str>) {
    if d.clase != Clase::Modelo || !nombre.to_lowercase().ends_with(".pdf") {
        return;
    }
    let (Some(own), Some(texto)) = (own, crate::pdf_text::text(bytes)) else {
        return;
    };
    if let Some(otro) = declarante_distinto(&texto, own) {
        d.clase = Clase::Otro;
        d.modelo = None;
        d.descripcion = format!("Modelo presentado por otra empresa (NIF {otro})");
    }
}

/// NIF de quien presenta el modelo, si no es `own`: el NIF que más se repite en el texto (va en
/// la cabecera de cada página; los de socios, participadas o perceptores salen una o dos
/// veces). `None` si es el de la empresa o no se sabe.
pub(crate) fn declarante_distinto(texto: &str, own: &str) -> Option<String> {
    static NIF: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(r"\b(?:[ABCDEFGHJNPQRSUVW]\d{7}[0-9A-J]|\d{8}[A-Z]|[XYZ]\d{7}[A-Z])\b").ok()
    });
    let mut veces: BTreeMap<String, usize> = BTreeMap::new();
    for m in NIF.as_ref()?.find_iter(texto) {
        if matches!(nif_español(m.as_str()), Some(Ok(_))) {
            *veces.entry(m.as_str().to_owned()).or_default() += 1;
        }
    }
    let max = veces.values().copied().max()?;
    if veces.get(&nif_clave(own)).copied().unwrap_or(0) == max {
        return None;
    }
    let mut mas: Vec<String> = veces
        .into_iter()
        .filter(|(_, n)| *n == max)
        .map(|(k, _)| k)
        .collect();
    (mas.len() == 1).then(|| mas.remove(0))
}

/// Lo que dice la lectura del asistente o de Claude Code.
fn de_lectura(d: &mut Doc, nombre: &str, bytes: &[u8], r: &Reading, own: Option<&str>) {
    d.con_ia = true;
    d.descripcion.clone_from(&r.descripcion);
    d.clase = match r.tipo.as_str() {
        "factura_recibida" => Clase::FacturaRecibida,
        "factura_emitida" => Clase::FacturaEmitida,
        "modelo_aeat" => Clase::Modelo,
        "libro_emitidas" => Clase::LibroEmitidas,
        "libro_recibidas" => Clase::LibroRecibidas,
        "diario" => Clase::Diario,
        "extracto_bancario" => Clase::Extracto,
        "sumas_saldos" | "cuentas_anuales" => Clase::Contable,
        "nomina" => Clase::Nomina,
        "otro" => Clase::Otro,
        _ => Clase::Empresa,
    };
    d.tramite = r
        .registro
        .as_ref()
        .and_then(|j| super::cruces::datos::tramite_de_lectura(j, &r.tipo));
    match d.clase {
        Clase::FacturaRecibida | Clase::FacturaEmitida => {
            d.factura = r.factura.as_ref().map(|f| factura(f, &r.descripcion));
        }
        Clase::Modelo => {
            d.modelo = r.modelo.as_ref().and_then(modelo);
            // Sin el tipo en el bloque, lo que digan la descripción o el nombre del fichero.
            if let Some(m) = d.modelo.as_mut()
                && m.tipo == TipoDeclaracion::Normal
                && let Some(t) = TipoDeclaracion::de_texto(&r.descripcion)
                    .or_else(|| TipoDeclaracion::de_texto(&d.ruta))
            {
                m.tipo = t;
            }
        }
        Clase::Nomina => d.nomina = Some(nomina(r, nombre, bytes, own)),
        Clase::Empresa | Clase::Contable => {
            if let Some(kind) = crate::lectura_ia::doc_kind(&r.tipo) {
                let facts = r
                    .empresa
                    .as_ref()
                    .map(|e| crate::documentos::parse_ai(&e.to_string()))
                    .unwrap_or_default();
                d.empresa = Some((kind, facts));
            }
        }
        Clase::LibroEmitidas | Clase::LibroRecibidas => {
            use crate::lectura_ia::Contenido;
            let (titulo, contenido) = if d.clase == Clase::LibroEmitidas {
                ("Libro registro de facturas expedidas", Contenido::Emitidas)
            } else {
                ("Libro registro de facturas recibidas", Contenido::Recibidas)
            };
            d.libro = r.hoja.as_ref().and_then(|plan| {
                let hojas = crate::import::sheets(nombre, bytes).ok()?;
                let (filas, _) =
                    crate::lectura_ia::remap_hojas(&hojas, plan, contenido, Some(titulo));
                crate::libros::parse_rows(nombre, &filas)
            });
        }
        _ => {}
    }
}

/// La nómina: lo que trae el bloque `nomina` de la lectura y, si falta, lo que se lee en el texto
/// del PDF o en la descripción (el NIF del trabajador, que es el único de una persona que no es
/// el de la empresa, y el tipo de retención del IRPF) y el mes que dicen la descripción o el
/// nombre del fichero («Nómina de enero de 2025», «2025-01.pdf»).
fn nomina(r: &Reading, nombre: &str, bytes: &[u8], own: Option<&str>) -> Nomina {
    let j = r.nomina.as_ref();
    let campo = |k: &str| j.and_then(|j| texto(j, k));
    let mut n = Nomina {
        nif: campo("nif_trabajador").map(|n| nif_clave(&n)),
        nombre: campo("nombre_trabajador"),
        mes: campo("periodo").and_then(|p| {
            crate::import::parse_date(&p).or_else(|| crate::import::parse_date(&format!("{p}-01")))
        }),
        bruto: j.and_then(|j| importe(j, "bruto")),
        retencion: j.and_then(|j| importe(j, "retencion")).map(Money::abs),
        tipo: campo("tipo_retencion").and_then(|t| tipo_irpf(&format!("IRPF {t}"))),
    };
    if n.mes.is_none() {
        n.mes = mes_de(&r.descripcion).or_else(|| mes_de(nombre));
    }
    if n.nif.is_some() && n.tipo.is_some() {
        return n;
    }
    let pdf = bytes
        .starts_with(b"%PDF")
        .then(|| crate::pdf_text::text(bytes))
        .flatten()
        .unwrap_or_default();
    for t in [pdf.as_str(), r.descripcion.as_str()] {
        if n.nif.is_none() {
            n.nif = nif_de_persona(t, own);
        }
        if n.tipo.is_none() {
            n.tipo = tipo_irpf(t);
        }
    }
    n
}

/// El mes que dice un texto, su día 1: «enero de 2025», «ENERO 2025», «2025-01», «01/2025».
fn mes_de(texto: &str) -> Option<Date> {
    static NOMBRE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r"(?:^|[^a-z])(enero|febrero|marzo|abril|mayo|junio|julio|agosto|septiembre|setiembre|octubre|noviembre|diciembre)[\s_-]*(?:de[\s_-]+)?(20\d{2})(?:$|[^\d])",
        )
        .ok()
    });
    static NUMERO: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(r"(?:^|[^\d])(?:(20\d{2})[-_.](0[1-9]|1[0-2])|(0[1-9]|1[0-2])[-_/.](20\d{2}))(?:$|[^\d])").ok()
    });
    let t = crate::documentos::fold(texto);
    let dia = |y: &str, m: u8| {
        Date::from_calendar_date(y.parse().ok()?, time::Month::try_from(m).ok()?, 1).ok()
    };
    if let Some(c) = NOMBRE.as_ref()?.captures(&t) {
        const MESES: [&str; 12] = [
            "enero",
            "febrero",
            "marzo",
            "abril",
            "mayo",
            "junio",
            "julio",
            "agosto",
            "septiembre",
            "octubre",
            "noviembre",
            "diciembre",
        ];
        let m = if &c[1] == "setiembre" {
            9
        } else {
            MESES.iter().position(|x| *x == &c[1])? + 1
        };
        return dia(&c[2], u8::try_from(m).ok()?);
    }
    let c = NUMERO.as_ref()?.captures(&t)?;
    match (c.get(1), c.get(2), c.get(3), c.get(4)) {
        (Some(y), Some(m), _, _) | (_, _, Some(m), Some(y)) => {
            dia(y.as_str(), m.as_str().parse().ok()?)
        }
        _ => None,
    }
}

/// El NIF de una persona (DNI o NIE con su letra bien) que sale en el texto, si es uno solo y no
/// es el de la empresa.
pub(crate) fn nif_de_persona(texto: &str, own: Option<&str>) -> Option<String> {
    static NIF: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"\b(?:\d{8}[A-Z]|[XYZ]\d{7}[A-Z])\b").ok());
    let mut vistos = std::collections::BTreeSet::new();
    for m in NIF.as_ref()?.find_iter(&texto.to_uppercase()) {
        let n = m.as_str();
        if own.is_some_and(|o| nif_clave(o) == n) {
            continue;
        }
        if matches!(nif_español(n), Some(Ok(x)) if x.is_persona_fisica()) {
            vistos.insert(n.to_owned());
        }
    }
    (vistos.len() == 1)
        .then(|| vistos.into_iter().next())
        .flatten()
}

/// Tipo de retención del IRPF que dice el texto («I.R.P.F. 11,40», «IRPF 15 %»), en centésimas
/// de punto.
fn tipo_irpf(texto: &str) -> Option<i32> {
    static TIPO: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r"(?i)i\.?\s?r\.?\s?p\.?\s?f\.?[^0-9\n]{0,25}?(\d{1,2}(?:[.,]\d{1,2})?)(?:\s*%|[\s)]|$)",
        )
            .ok()
    });
    let c = TIPO.as_ref()?.captures(texto)?;
    let m = crate::import::parse_amount(&c[1].replace('.', ","))?;
    i32::try_from(m.cents())
        .ok()
        .filter(|c| (1..=4700).contains(c))
}

fn texto(j: &serde_json::Value, k: &str) -> Option<String> {
    j.get(k)
        .and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.trim().to_owned()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .filter(|s| !s.is_empty() && s != "null")
}

/// Importe escrito por el asistente (el último separador es el decimal: «1,234.56» no es 1,23).
fn importe(j: &serde_json::Value, k: &str) -> Option<Money> {
    texto(j, k).and_then(|s| crate::received::parse_amount(&s))
}

fn factura(f: &serde_json::Value, descripcion: &str) -> Factura {
    Factura {
        emisor_nif: texto(f, "nif_emisor"),
        emisor_nombre: texto(f, "nombre_emisor"),
        destinatario_nif: texto(f, "nif_destinatario"),
        destinatario_nombre: texto(f, "nombre_destinatario"),
        numero: texto(f, "numero"),
        fecha: texto(f, "fecha").and_then(|s| crate::import::parse_date(&s)),
        base: importe(f, "base"),
        iva: importe(f, "iva"),
        retencion: importe(f, "retencion").map(Money::abs),
        total: importe(f, "total"),
        moneda: texto(f, "moneda")
            .map(|m| m.to_uppercase())
            .filter(|m| m.len() == 3 && m != "EUR")
            .or_else(|| moneda_de_texto(descripcion)),
        emisor_pais: texto(f, "pais_emisor")
            .map(|p| p.to_uppercase())
            .filter(|p| p.len() == 2 && p != "ES" && taxpain_core::iso3166::is_country(p)),
    }
}

/// Moneda que menciona una descripción («Importes en USD», «en dólares»…), si no es el euro.
pub(crate) fn moneda_de_texto(s: &str) -> Option<String> {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r"(?i)\b(USD|US\$|d[oó]lar(?:es)?|GBP|libras? esterlinas?|CHF|francos? suizos?|CAD)\b",
        )
        .ok()
    });
    let m = RE.as_ref()?.find(s)?.as_str().to_lowercase();
    Some(
        if m.starts_with("usd") || m.starts_with("us$") || m.starts_with('d') {
            "USD"
        } else if m.starts_with("gbp") || m.starts_with("libra") {
            "GBP"
        } else if m.starts_with("chf") || m.starts_with("franco") {
            "CHF"
        } else {
            "CAD"
        }
        .to_owned(),
    )
}

fn modelo(m: &serde_json::Value) -> Option<Modelo> {
    let modelo = texto(m, "modelo")?;
    let ejercicio = texto(m, "ejercicio")?.parse().ok()?;
    let casillas: BTreeMap<u32, Money> = m
        .get("casillas")
        .and_then(|c| c.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| {
                    let n: u32 = k.trim_matches(['[', ']']).parse().ok()?;
                    let s = match v {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Number(x) => x.to_string(),
                        _ => return None,
                    };
                    Some((n, crate::received::parse_amount(&s)?))
                })
                .collect()
        })
        .unwrap_or_default();
    // En los resúmenes anuales, la [01] es el número de registros (perceptores o declarados).
    let num_registros = ["180", "190", "193", "347", "349"]
        .contains(&modelo.as_str())
        .then(|| casillas.get(&1).copied())
        .flatten()
        .filter(|n| n.cents() % 100 == 0 && n.cents() >= 0)
        .and_then(|n| u32::try_from(n.cents() / 100).ok());
    Some(Modelo {
        modelo,
        ejercicio,
        periodo: texto(m, "periodo").unwrap_or_default().to_uppercase(),
        num_registros,
        casillas,
        presentado: texto(m, "presentado")
            .or_else(|| texto(m, "fecha_presentacion"))
            .and_then(|s| crate::import::parse_date(&s)),
        periodo_desde: texto(m, "inicio").and_then(|s| crate::import::parse_date(&s)),
        periodo_hasta: texto(m, "fin").and_then(|s| crate::import::parse_date(&s)),
        registros: m
            .get("registros")
            .and_then(|r| r.as_array())
            .map(|l| l.iter().filter_map(registro).collect())
            .unwrap_or_default(),
        tipo: ["tipo", "tipo_declaracion", "declaracion"]
            .iter()
            .find_map(|k| texto(m, k).and_then(|t| TipoDeclaracion::de_texto(&t)))
            .or_else(|| {
                let si = |k: &str| m.get(k).and_then(serde_json::Value::as_bool) == Some(true);
                if si("complementaria") {
                    Some(TipoDeclaracion::Complementaria)
                } else if si("sustitutiva") {
                    Some(TipoDeclaracion::Sustitutiva)
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        ..Modelo::default()
    })
}

/// Un 347 o un 349 leído de su fichero: sus terceros u operadores, para cruzarlos por NIF.
fn de_registros(r: taxpain_models::informative::ReadFile) -> Modelo {
    // Del 349, su total va en la casilla [02] (importe de las operaciones), la que se compara con
    // el 303.
    // De los resúmenes de retenciones (190, 180, 193), la [02] (percepciones o bases) y la
    // [03] (retenciones), las casillas que usan los cruces con el 111, el 115 y el 123.
    let casillas = match (r.model.as_str(), r.withheld) {
        ("349", _) => BTreeMap::from([(2, r.total)]),
        ("190" | "180" | "193", Some(w)) => BTreeMap::from([(2, r.total), (3, w)]),
        _ => BTreeMap::new(),
    };
    Modelo {
        casillas,
        modelo: r.model,
        ejercicio: r.year,
        periodo: r.period.to_uppercase(),
        registros: r
            .lines
            .into_iter()
            .map(|l| Registro {
                nif: Some(nif_clave(&l.nif)).filter(|n| !n.is_empty()),
                nombre: Some(l.name).filter(|n| !n.is_empty()),
                clave: Some(l.key.to_uppercase()).filter(|k| !k.is_empty()),
                importe: Some(l.amount),
                retencion: l.withheld,
            })
            .collect(),
        ..Modelo::default()
    }
}

/// Una línea de un resumen anual leída por el asistente.
fn registro(j: &serde_json::Value) -> Option<Registro> {
    let r = Registro {
        nif: texto(j, "nif").map(|n| nif_clave(&n)),
        nombre: texto(j, "nombre"),
        clave: texto(j, "clave").map(|c| c.to_uppercase()),
        importe: importe(j, "importe"),
        retencion: importe(j, "retencion").map(Money::abs),
    };
    (r.nif.is_some() || r.nombre.is_some()).then_some(r)
}

/// Un Modelo 200 en PDF que no se ha leído con el asistente: sus casillas de cinco cifras con
/// importe («00501 12.345,67»). La primera vez que sale una casilla es la que vale (la
/// cuenta de pérdidas y ganancias va antes que la liquidación, que repite algún número).
fn modelo_200_de_pdf(d: &mut Doc, nombre: &str, bytes: &[u8]) {
    if !nombre.to_lowercase().ends_with(".pdf") {
        return;
    }
    let Some(text) = crate::pdf_text::text(bytes) else {
        return;
    };
    if let Some(m) = modelo_200_de_texto(&text) {
        d.clase = Clase::Modelo;
        d.descripcion = format!("Modelo 200 del ejercicio {}", m.ejercicio);
        d.modelo = Some(m);
    }
}

pub(crate) fn modelo_200_de_texto(text: &str) -> Option<Modelo> {
    static CASILLA: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"\b(\d{5})\s+(-?\d{1,3}(?:\.\d{3})*,\d{2,4})\b").ok());
    static EJERCICIO: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"(?i)ejercicio\D{0,80}?(20\d{2})").ok());
    let lower = text.to_lowercase();
    if !(lower.contains("modelo 200") || lower.contains("impuesto sobre sociedades")) {
        return None;
    }
    let mut casillas = BTreeMap::new();
    for c in CASILLA.as_ref()?.captures_iter(text) {
        let (Ok(n), Some(m)) = (c[1].parse::<u32>(), crate::import::parse_amount(&c[2])) else {
            continue;
        };
        if !m.is_zero() {
            casillas.entry(n).or_insert(m);
        }
    }
    // Un 200 de verdad tiene decenas de casillas; con pocas, es otra cosa que lo menciona.
    if casillas.len() < 15 {
        return None;
    }
    let ejercicio = EJERCICIO
        .as_ref()?
        .captures(text)
        .and_then(|c| c[1].parse().ok())?;
    Some(Modelo {
        modelo: "200".into(),
        ejercicio,
        periodo: "0A".into(),
        casillas,
        ..Modelo::default()
    })
}

/// Año y trimestre de la ruta: el último tramo que es un año («2025») y, detrás de él, uno
/// que es un trimestre («T3», «Q3», «3T»).
pub(crate) fn periodo_de_ruta(ruta: &str) -> (Option<i32>, Option<u8>) {
    let tramos: Vec<&str> = ruta.split('/').collect();
    let carpetas = &tramos[..tramos.len().saturating_sub(1)];
    let mut año = None;
    let mut trimestre = None;
    for t in carpetas {
        let t = t.trim();
        if t.len() == 4
            && let Ok(y) = t.parse::<i32>()
            && (2000..=2100).contains(&y)
        {
            año = Some(y);
            trimestre = None;
            continue;
        }
        if año.is_some() {
            let u = t.to_uppercase();
            let b = u.as_bytes();
            let q = match b {
                [b'T' | b'Q', n] | [n, b'T'] if (b'1'..=b'4').contains(n) => Some(*n - b'0'),
                _ => None,
            };
            if q.is_some() {
                trimestre = q;
            }
        }
    }
    (año, trimestre.filter(|_| año.is_some()))
}

/// NIF para comparar: mayúsculas, sin espacios, puntos ni guiones, y sin «ES» delante de un NIF
/// español.
pub(crate) fn nif_clave(s: &str) -> String {
    let n: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_uppercase();
    match n.strip_prefix("ES") {
        Some(rest) if rest.len() == 9 => rest.to_owned(),
        _ => n,
    }
}

/// Formas sociales (españolas y de otros países) que se quitan del final de un nombre para
/// compararlo: «Bar Pepe, S.L.» = «bar pepe»; «Demo Pte. Ltd.» = «demo».
const FORMAS_SOCIALES: &[&str] = &[
    "sl",
    "slu",
    "sll",
    "slp",
    "sa",
    "sau",
    "sad",
    "cb",
    "sc",
    "scp",
    "coop",
    "sociedad",
    "limitada",
    "anonima",
    "unipersonal",
    "limited",
    "ltd",
    "llc",
    "inc",
    "incorporated",
    "corp",
    "corporation",
    "co",
    "company",
    "gmbh",
    "ag",
    "kg",
    "bv",
    "nv",
    "sarl",
    "sas",
    "srl",
    "spa",
    "plc",
    "pte",
    "pty",
    "pbc",
    "lp",
    "llp",
    "oy",
    "ab",
    "aps",
    "as",
    "sro",
    "kft",
];

/// Formas sociales de otros países (las que no se usan en España).
const FORMAS_EXTRANJERAS: &[&str] = &[
    "limited",
    "ltd",
    "llc",
    "inc",
    "incorporated",
    "corp",
    "corporation",
    "gmbh",
    "ag",
    "kg",
    "bv",
    "nv",
    "sarl",
    "sas",
    "spa",
    "plc",
    "pte",
    "pty",
    "pbc",
    "lp",
    "llp",
    "oy",
    "ab",
    "aps",
    "sro",
    "kft",
];

/// Las palabras de un nombre, sin tildes ni signos.
fn palabras(nombre: &str) -> Vec<String> {
    nombre
        .split(|c: char| !c.is_alphanumeric())
        .map(crate::import::norm)
        .filter(|t| !t.is_empty())
        .collect()
}

/// Las formas sociales del final de un nombre («Demo Pte. Ltd.» → pte y ltd; «Demo GmbH & Co.
/// KG» → kg, co y gmbh), con las siglas que llegan en letras sueltas («S.p.A.» → spa) marcadas.
/// No cuenta lo que va entre paréntesis ni el nombre comercial («Demo Inc. (Producto)», «Demo,
/// LLC dba Producto»).
fn formas_del_final(nombre: &str) -> Vec<(String, bool)> {
    let mut limpio = String::new();
    let mut dentro = 0usize;
    for c in nombre.chars() {
        match c {
            '(' => dentro += 1,
            ')' => dentro = dentro.saturating_sub(1),
            c if dentro == 0 => limpio.push(c),
            _ => {}
        }
    }
    let minusculas = limpio.to_lowercase();
    if let Some(i) = [" dba ", " d/b/a ", " t/a ", " trading as "]
        .iter()
        .filter_map(|x| minusculas.find(x))
        .min()
    {
        limpio.truncate(i);
    }
    let mut t = palabras(&limpio);
    let mut out = Vec::new();
    while t.len() > 1 {
        if FORMAS_SOCIALES.contains(&t[t.len() - 1].as_str()) {
            out.extend(t.pop().map(|f| (f, false)));
            continue;
        }
        let sueltas = t
            .iter()
            .rev()
            .take_while(|x| x.chars().count() == 1)
            .count();
        let siglas: String = t[t.len() - sueltas..].concat();
        if sueltas > 1 && sueltas < t.len() && FORMAS_SOCIALES.contains(&siglas.as_str()) {
            t.truncate(t.len() - sueltas);
            out.push((siglas, true));
            continue;
        }
        break;
    }
    out
}

/// ¿Acaba el nombre en una forma social que sólo se usa fuera de la UE («Demo Inc.», «Demo,
/// PBC», «Demo Pte. Ltd.»)? Las de Irlanda, Malta o Chipre (Ltd., Limited, plc) no cuentan.
pub(crate) fn forma_de_fuera_de_la_ue(nombre: &str) -> bool {
    const FUERA_UE: &[&str] = &[
        "inc",
        "incorporated",
        "llc",
        "pbc",
        "corp",
        "corporation",
        "pte",
        "pty",
    ];
    formas_del_final(nombre)
        .iter()
        .any(|(f, _)| FUERA_UE.contains(&f.as_str()))
}

/// ¿Acaba el nombre en una forma social de otro país («Demo Inc.», «Demo Pte. Ltd.», «Demo
/// GmbH»)? Sirve para saber que un emisor sin NIF es de fuera de España. «Spa» sólo cuenta en
/// siglas («S.p.A.», «SpA»): a secas es el de un hotel o un balneario.
pub(crate) fn forma_extranjera(nombre: &str) -> bool {
    const ESPAÑOLAS: &[&str] = &["sl", "slu", "sll", "slp", "sa", "sau", "sad", "cb", "coop"];
    let formas = formas_del_final(nombre);
    let spa = nombre
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w == "SpA");
    formas.iter().any(|(f, siglas)| {
        FORMAS_EXTRANJERAS.contains(&f.as_str()) && (f != "spa" || *siglas || spa)
    }) && !formas.iter().any(|(f, _)| ESPAÑOLAS.contains(&f.as_str()))
}

/// Nombre para comparar: minúsculas, sin tildes ni signos y sin la forma social del final.
pub(crate) fn nombre_clave(s: &str) -> String {
    let mut t: Vec<String> = s
        .split(|c: char| !c.is_alphanumeric())
        .map(crate::import::norm)
        .filter(|t| !t.is_empty())
        .collect();
    while t.len() > 1 {
        if FORMAS_SOCIALES.contains(&t[t.len() - 1].as_str()) {
            t.pop();
            continue;
        }
        // Las siglas con puntos llegan en letras sueltas: «S.L.U.» → «s», «l», «u».
        let sueltas = t
            .iter()
            .rev()
            .take_while(|x| x.chars().count() == 1)
            .count();
        let siglas: String = t[t.len() - sueltas..].concat();
        if sueltas > 1 && sueltas < t.len() && FORMAS_SOCIALES.contains(&siglas.as_str()) {
            t.truncate(t.len() - sueltas);
            continue;
        }
        break;
    }
    t.concat()
}

/// ¿El mismo nombre? Iguales sin la forma social, o el corto (de 5 letras o más) dentro del largo.
pub(crate) fn nombres_parecidos(a: &str, b: &str) -> bool {
    let (a, b) = (nombre_clave(a), nombre_clave(b));
    let (corto, largo) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    corto.len() >= 3 && (corto == largo || (corto.len() >= 5 && largo.contains(corto.as_str())))
}

/// Un NIF con formato español (9 caracteres) y lo que dice su letra de control. `None` si no
/// tiene formato español (NIF-IVA de otro país, identificador extranjero…).
pub(crate) fn nif_español(s: &str) -> Option<std::result::Result<Nif, NifError>> {
    let n = nif_clave(s);
    if n.len() != 9 {
        return None;
    }
    // El DNI, el NIE y los K, L y M acaban en letra: nueve cifras seguidas (o una X seguida de
    // ocho) son un número de otro país, como un EIN de EE. UU., no un NIF mal escrito.
    let b = n.as_bytes();
    if (b[0].is_ascii_digit() || b"XYZKLM".contains(&b[0])) && b[8].is_ascii_digit() {
        return None;
    }
    match Nif::parse(&n) {
        Err(NifError::Format | NifError::Length) => None,
        r => Some(r),
    }
}

/// Formatos que TaxPain no lee (zip, Word, Pages…): no se miran, pero se cuentan y se listan.
fn es_zip(path: &str) -> bool {
    path.to_lowercase().ends_with(".zip")
}

fn no_admitidos(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>, depth: u8) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name.starts_with("~$") || name.starts_with("taxpain-") {
                continue;
            }
            let path = e.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                if depth < 8 {
                    walk(root, &path, out, depth + 1);
                }
                continue;
            }
            let ext = path
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if meta.is_file() && !crate::carpeta::admitted_ext(&ext) {
                out.push(super::componer(
                    &path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/"),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Complementaria o sustitutiva: por el texto, sólo si nombra una de las dos (los impresos
    /// llevan las dos casillas), o por el registro de declarante del fichero («C» en la posición
    /// 121, «S» en la 122).
    #[test]
    fn complementaria_o_sustitutiva() {
        use TipoDeclaracion::{Complementaria, Sustitutiva};
        assert_eq!(
            TipoDeclaracion::de_texto("Modelo 190 de 2024, declaración complementaria"),
            Some(Complementaria)
        );
        assert_eq!(
            TipoDeclaracion::de_texto("MODELOS/190-2024-SUSTITUTIVA.pdf"),
            Some(Sustitutiva)
        );
        assert_eq!(
            TipoDeclaracion::de_texto("Declaración complementaria [ ] Declaración sustitutiva [ ]"),
            None
        );
        let registro = |c: char, s: char| {
            let mut r: Vec<char> = format!("13472024B12345674EMPRESA DEMO SL{:<485}", "")
                .chars()
                .collect();
            r[120] = c;
            r[121] = s;
            let mut t: String = r.into_iter().collect();
            t.push_str("\r\n2347");
            t.into_bytes()
        };
        assert_eq!(
            TipoDeclaracion::de_fichero(&registro('C', ' ')),
            Some(Complementaria)
        );
        assert_eq!(
            TipoDeclaracion::de_fichero(&registro(' ', 'S')),
            Some(Sustitutiva)
        );
        assert_eq!(TipoDeclaracion::de_fichero(&registro(' ', ' ')), None);
    }

    #[test]
    fn las_dudosas_de_un_justificante_de_su_lectura_de_ia() {
        let mut m = Modelo {
            modelo: "200".into(),
            ejercicio: 2023,
            periodo: "0A".into(),
            casillas: [(552, Money::from_cents(1_000_000))].into_iter().collect(),
            dudosas: [599, 603, 1586].into_iter().collect(),
            ..Modelo::default()
        };
        let lectura = |y: i32| -> Reading {
            serde_json::from_value(serde_json::json!({
                "tipo": "modelo_aeat",
                "modelo": {"modelo": "200", "ejercicio": y, "periodo": "0A",
                           "casillas": {"00599": "2100.00", "01586": "-1500.00"}}
            }))
            .unwrap()
        };
        // De otra declaración (otro ejercicio), no.
        completar_con_la_lectura(&mut m, &lectura(2022));
        assert_eq!(m.dudosas.len(), 3);
        completar_con_la_lectura(&mut m, &lectura(2023));
        assert_eq!(m.c(1586), Money::from_cents(-150_000));
        assert_eq!(m.c(599), Money::from_cents(210_000));
        // La que la lectura no trae sigue sin saberse.
        assert_eq!(m.dudosas, BTreeSet::from([603]));
    }

    /// El lector del justificante del 111 no lee el número de perceptores: sale de la lectura de
    /// IA del mismo fichero. Lo que ya se leyó del justificante no cambia.
    #[test]
    fn las_casillas_que_no_lee_el_justificante_de_su_lectura_de_ia() {
        let eur = Money::from_cents;
        let mut m = Modelo {
            modelo: "111".into(),
            ejercicio: 2025,
            periodo: "1T".into(),
            casillas: [(2, eur(300_000)), (3, eur(45_000)), (30, eur(45_000))]
                .into_iter()
                .collect(),
            justificante: Some("1110000000013".into()),
            ..Modelo::default()
        };
        let lectura: Reading = serde_json::from_value(serde_json::json!({
            "tipo": "modelo_aeat",
            "modelo": {"modelo": "111", "ejercicio": 2025, "periodo": "1T",
                       "casillas": {"01": "1", "02": "3000.00", "30": "450.00"}}
        }))
        .unwrap();
        completar_con_la_lectura(&mut m, &lectura);
        assert_eq!(m.c(1), eur(100));
        assert_eq!(m.c(2), eur(300_000));
        assert_eq!(m.c(30), eur(45_000));
        // Si en lo que leen los dos no dicen lo mismo, no se mezclan.
        let mut m = Modelo {
            casillas: [(2, eur(300_000)), (30, eur(45_000))].into_iter().collect(),
            ..m
        };
        let otra: Reading = serde_json::from_value(serde_json::json!({
            "tipo": "modelo_aeat",
            "modelo": {"modelo": "111", "ejercicio": 2025, "periodo": "1T",
                       "casillas": {"01": "1", "02": "3100.00", "30": "450.00"}}
        }))
        .unwrap();
        completar_con_la_lectura(&mut m, &otra);
        assert!(!m.casillas.contains_key(&1));
    }

    #[test]
    fn año_y_trimestre_de_la_ruta() {
        assert_eq!(
            periodo_de_ruta("FACTURAS/2023/T4/GASTOS/a.pdf"),
            (Some(2023), Some(4))
        );
        assert_eq!(
            periodo_de_ruta("FACTURAS/2017/Q2/INGRESOS/b.pdf"),
            (Some(2017), Some(2))
        );
        assert_eq!(periodo_de_ruta("2026/3T/c.pdf"), (Some(2026), Some(3)));
        // El año del nombre de la carpeta no cuenta si no es el tramo entero.
        assert_eq!(periodo_de_ruta("MODELOS AÑO 2025/d.pdf"), (None, None));
        // Un trimestre sin año delante no vale.
        assert_eq!(periodo_de_ruta("T3/e.pdf"), (None, None));
        assert_eq!(periodo_de_ruta("f.pdf"), (None, None));
    }

    #[test]
    fn nif_para_comparar() {
        assert_eq!(nif_clave("ES B-12345674"), "B12345674");
        assert_eq!(nif_clave("b12345674"), "B12345674");
        assert_eq!(nif_clave("IE1234567T"), "IE1234567T");
        assert!(nif_español("IE1234567T").is_none());
        assert!(nif_español("EU123456789").is_none());
        assert!(nif_español("12-3456789").is_none());
        assert!(nif_español("512345678").is_none());
        assert!(matches!(nif_español("B12345674"), Some(Ok(_))));
        assert!(matches!(
            nif_español("12345678A"),
            Some(Err(NifError::Control))
        ));
    }

    #[test]
    fn mencion_de_por_que_no_lleva_iva() {
        assert!(menciona_sin_iva(
            "Total 1.000,00 €\nInversión del Sujeto Pasivo"
        ));
        assert!(menciona_sin_iva(
            "VAT: reverse charge (Article 196 VAT Directive)"
        ));
        assert!(menciona_sin_iva(
            "Entrega intracomunitaria exenta, art. 25 LIVA"
        ));
        assert!(!menciona_sin_iva(
            "Base 1.000,00 € IVA 0,00 € Total 1.000,00 €"
        ));
        // En otra lengua oficial o abreviado.
        assert!(menciona_sin_iva("Inversió del subjecte passiu"));
        assert!(menciona_sin_iva("Investimento do suxeito pasivo"));
        assert!(menciona_sin_iva("VAT: ISP (inv. suj. pasivo)"));
        assert!(!menciona_sin_iva("Servicio de hosting del ISPConfig"));
    }

    #[test]
    fn emisores_de_fuera_de_espana_sin_nif() {
        assert!(forma_extranjera("Software Demo, Inc."));
        assert!(forma_extranjera("Videos Demo Pte. Ltd."));
        assert!(forma_extranjera("Hosting Demo GmbH"));
        assert!(!forma_extranjera("Papelería Demo, S.L."));
        assert!(!forma_extranjera("Limited Edition Demo SL"));
        assert!(!forma_extranjera("Talleres AB Motor SL"));
        // La forma social va al final: un hotel con spa o «Company» como nombre comercial no.
        assert!(!forma_extranjera("Hotel Spa Demo"));
        assert!(!forma_extranjera("Balneario Demo Spa"));
        assert!(!forma_extranjera("La Burger Company"));
        assert!(forma_extranjera("Pasta Demo S.p.A."));
        assert!(forma_extranjera("Muebles Demo B.V."));
        assert!(forma_extranjera("Demo Inc. (Producto)"));
        assert!(forma_extranjera("Demo, LLC dba Producto"));
        let f = |nif: Option<&str>, nombre: &str, moneda: Option<&str>| Factura {
            emisor_nif: nif.map(str::to_owned),
            emisor_nombre: Some(nombre.into()),
            moneda: moneda.map(str::to_owned),
            ..Factura::default()
        };
        assert!(f(None, "Software Demo Inc.", None).emisor_extranjero());
        assert!(f(None, "Interior Demo", Some("USD")).emisor_extranjero());
        assert!(!f(None, "Bar Demo", None).emisor_extranjero());
        assert!(!f(Some("B12345674"), "Demo Inc.", None).emisor_extranjero());
        assert!(f(Some("IE1234567T"), "Nube Demo", None).emisor_de_la_ue());
        assert!(!f(Some("EU372000041"), "Nube Demo", None).emisor_de_la_ue());
        // Sin NIF, el país que dice la lectura.
        let sg = Factura {
            emisor_pais: Some("SG".into()),
            ..f(None, "Videos Demo", None)
        };
        assert!(sg.emisor_extranjero() && !sg.emisor_de_la_ue());
        let de = Factura {
            emisor_pais: Some("DE".into()),
            ..f(None, "Hosting Demo", None)
        };
        assert!(de.emisor_extranjero() && de.emisor_de_la_ue());
        // De fuera de la UE: por el país, por un NIF que no es de la UE o por una forma social
        // que sólo se usa fuera (las de Irlanda o Malta no cuentan).
        assert!(sg.emisor_de_fuera_de_la_ue() && !de.emisor_de_fuera_de_la_ue());
        assert!(f(None, "Software Demo, PBC", None).emisor_de_fuera_de_la_ue());
        assert!(f(None, "Videos Demo Pte. Ltd.", None).emisor_de_fuera_de_la_ue());
        assert!(!f(None, "Nube Demo Ltd", None).emisor_de_fuera_de_la_ue());
        assert!(f(Some("EU372000041"), "Nube Demo", None).emisor_de_fuera_de_la_ue());
        assert!(!f(Some("IE1234567T"), "Nube Demo Inc.", None).emisor_de_fuera_de_la_ue());
        assert!(!f(Some("B12345674"), "Demo Inc.", None).emisor_de_fuera_de_la_ue());
    }

    #[test]
    fn nombres_sin_la_forma_social() {
        assert_eq!(nombre_clave("Bar Pepe, S.L."), "barpepe");
        assert_eq!(nombre_clave("Demo Pte. Ltd."), "demo");
        assert_eq!(nombre_clave("Teléfonos Demo, S.A.U."), "telefonosdemo");
        assert_eq!(
            nombre_clave("Software Demo Holdco, LLC"),
            "softwaredemoholdco"
        );
        // La forma social sola se queda: no hay otro nombre.
        assert_eq!(nombre_clave("S.L."), "sl");
        assert!(nombres_parecidos(
            "SOFTWARE DEMO, LLC",
            "Software Demo Ireland Limited"
        ));
        assert!(nombres_parecidos("Q.AI LLC", "Q.AI Inc."));
        assert!(!nombres_parecidos(
            "Nube Demo Commerce Ltd",
            "Nube Demo Cloud Ltd"
        ));
    }

    #[test]
    fn modelo_presentado_por_otra_empresa() {
        // El NIF de quien presenta sale en cada página; el de la socia, una vez.
        let mut texto = String::new();
        for p in 1..=5 {
            texto.push_str(&format!("Página {p} B12345674 PARTICIPADA EJEMPLO SL\n"));
        }
        texto.push_str("Socios: B98765431 EMPRESA EJEMPLO SL 50,00 %\n");
        assert_eq!(
            declarante_distinto(&texto, "B98765431").as_deref(),
            Some("B12345674")
        );
        assert_eq!(declarante_distinto(&texto, "B12345674"), None);
        // Sin NIF claro (empate), no se decide.
        assert_eq!(
            declarante_distinto("B12345674 y B11111119", "B98765431"),
            None
        );
    }

    /// El trabajador de una nómina: el único NIF de una persona que no es el de la empresa.
    #[test]
    fn nif_del_trabajador_y_tipo_del_irpf() {
        let texto = "EMPRESA DEMO SL NIF. B12345674\nTRABAJADOR PEREZ GOMEZ JUAN D.N.I. 12345678Z\n\
                     TRIBUTACION I.R.P.F.11,40        245,10\nBASE I.R.P.F.\n 2.150,00";
        assert_eq!(
            nif_de_persona(texto, Some("B12345674")).as_deref(),
            Some("12345678Z")
        );
        // Con dos personas distintas, no se sabe cuál es; con la letra mal, no es un NIF.
        assert_eq!(nif_de_persona("12345678Z y X1234567L", None), None);
        assert_eq!(nif_de_persona("12345678A", None), None);
        assert_eq!(tipo_irpf(texto), Some(1140));
        assert_eq!(tipo_irpf("Retención IRPF 15 % sobre 1.000,00"), Some(1500));
        // Un importe detrás de «IRPF» no es el tipo.
        assert_eq!(tipo_irpf("BASE I.R.P.F.\n 2.150,00"), None);
        assert_eq!(tipo_irpf("Retención IRPF: 2.150,00"), None);
    }

    /// El mes de una nómina sin bloque, por la descripción o el nombre del fichero.
    #[test]
    fn el_mes_de_la_nomina() {
        let d = |y, m: time::Month| Date::from_calendar_date(y, m, 1).ok();
        assert_eq!(
            mes_de("Nómina de enero de 2025 de Juan Pérez"),
            d(2025, time::Month::January)
        );
        assert_eq!(
            mes_de("NOMINAS/2023/NOMINA DICIEMBRE 2023.pdf"),
            d(2023, time::Month::December)
        );
        assert_eq!(
            mes_de("123_45678_NOMINAS_OCTUBRE_2019.pdf"),
            d(2019, time::Month::October)
        );
        assert_eq!(
            mes_de("NOMINAS/2023-12.pdf"),
            d(2023, time::Month::December)
        );
        assert_eq!(mes_de("recibo 03/2024"), d(2024, time::Month::March));
        assert_eq!(mes_de("Nómina del administrador"), None);
        assert_eq!(mes_de("NOMINAS/20231.pdf"), None);
    }

    #[test]
    fn moneda_en_la_descripcion() {
        assert_eq!(moneda_de_texto("Importes en USD").as_deref(), Some("USD"));
        assert_eq!(moneda_de_texto("pagado en dólares").as_deref(), Some("USD"));
        assert_eq!(
            moneda_de_texto("factura en libras esterlinas").as_deref(),
            Some("GBP")
        );
        assert_eq!(moneda_de_texto("Factura de luz"), None);
    }

    #[test]
    fn casillas_del_200_en_el_texto_del_pdf() {
        let mut t = String::from("Modelo 200 Impuesto sobre Sociedades\nEjercicio ..... 2025\n");
        for i in 0..20 {
            t.push_str(&format!("Casilla {} 0{:04}  1.{:03},50\n", i, 100 + i, i));
        }
        t.push_str("Exención 00370  23.456,00\nOtra vez 00370  1,00\n");
        let m = modelo_200_de_texto(&t).unwrap();
        assert_eq!(m.ejercicio, 2025);
        assert_eq!(m.c(370), Money::from_cents(2_345_600));
        assert_eq!(m.c(101), Money::from_cents(100_150));
        // Un texto que sólo menciona el 200 no es un 200.
        assert!(modelo_200_de_texto("Pago del modelo 200: 00599 1.234,56").is_none());
    }
}

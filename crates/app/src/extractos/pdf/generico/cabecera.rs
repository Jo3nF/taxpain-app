//! La cabecera de la tabla de movimientos: dónde está, qué columnas tiene y dónde empieza y
//! acaba cada una.
//!
//! La cabecera se reconoce por los nombres de sus columnas en varios idiomas (fecha, concepto,
//! importe, cargo, abono, saldo…), aunque venga partida en dos o tres líneas («F.» sobre
//! «operación», «Dinero» sobre «saliente»). Los límites entre columnas se ponen donde los datos
//! de las filas dejan un hueco: los títulos van centrados o alineados de cualquier manera, los
//! datos no se cruzan de una columna a otra.

use super::maqueta::{Linea, Trozo};
use super::piezas::{self, Pieza};
use crate::extractos::numeros::sin_tildes;

/// Qué hay en una columna.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tipo {
    /// Fecha de la operación (o contable).
    Fecha,
    FechaValor,
    Concepto,
    Referencia,
    /// Importe con su signo (o sin él, y entonces lo da el saldo).
    Importe,
    /// Cargos (sale dinero).
    Cargo,
    /// Abonos (entra dinero).
    Abono,
    Saldo,
    /// Marca de debe o haber («D»/«H», «+»/«-»).
    Signo,
    /// Cualquier otra cosa (oficina, código, categoría, estado, comisión, bruto…).
    Otro,
}

impl Tipo {
    pub(crate) fn es_dinero(self) -> bool {
        matches!(self, Self::Importe | Self::Cargo | Self::Abono)
    }
}

/// Una columna de la cabecera.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Columna {
    pub tipo: Tipo,
    pub titulo: String,
    /// Dónde está su título.
    pub x0: i32,
    pub x1: i32,
}

/// La cabecera de una tabla de movimientos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cabecera {
    /// De izquierda a derecha.
    pub columnas: Vec<Columna>,
    /// Líneas (índices) que ocupa la cabecera.
    pub desde: usize,
    pub hasta: usize,
    /// Límites entre columnas (uno menos que columnas): la columna `i` va de `limites[i - 1]` a
    /// `limites[i]`.
    pub limites: Vec<i32>,
}

impl Cabecera {
    /// Columna en la que cae un tramo `[x0, x1)`: la que más lo solapa.
    pub(crate) fn columna(&self, x0: i32, x1: i32) -> usize {
        let n = self.columnas.len();
        let mut mejor = (0, i32::MIN);
        for i in 0..n {
            let a = if i == 0 {
                i32::MIN
            } else {
                self.limites[i - 1]
            };
            let b = if i + 1 == n {
                i32::MAX
            } else {
                self.limites[i]
            };
            let solape = x1.min(b).saturating_sub(x0.max(a));
            if solape > mejor.1 {
                mejor = (i, solape);
            }
        }
        mejor.0
    }

    pub(crate) fn tipo(&self, i: usize) -> Tipo {
        self.columnas.get(i).map_or(Tipo::Otro, |c| c.tipo)
    }

    /// Índice de la columna de la fecha de la operación.
    pub(crate) fn col_fecha(&self) -> Option<usize> {
        self.columnas.iter().position(|c| c.tipo == Tipo::Fecha)
    }

    pub(crate) fn tiene(&self, t: Tipo) -> bool {
        self.columnas.iter().any(|c| c.tipo == t)
    }
}

/// Palabras de los títulos de columna (sin tildes y en minúsculas) en español, catalán,
/// euskera, gallego, inglés, francés, alemán, italiano, portugués y neerlandés.
const FECHAS: [&str; 33] = [
    "fecha",
    "f",
    "fec",
    "date",
    "datum",
    "data",
    "dia",
    "day",
    "buchungstag",
    "buchungsdatum",
    "buchung",
    "booking",
    "posted",
    "posting",
    "tag",
    "giorno",
    "transaktionsdatum",
    "umsatzdatum",
    "belegdatum",
    "boekdatum",
    "boekingsdatum",
    "transactiedatum",
    "dateop",
    "created",
    "completed",
    "started",
    "settlement",
    "datums",
    "fechas",
    "eguna",
    "entry",
    "trans",
    "txn",
];
const VALOR: [&str; 12] = [
    "valor",
    "valuta",
    "value",
    "valeur",
    "wertstellung",
    "wert",
    "valutadatum",
    "valoracion",
    "wertstellungsdatum",
    "rentedatum",
    "balio",
    "dateval",
];
const SALDOS: [&str; 7] = [
    "saldo",
    "balance",
    "solde",
    "kontostand",
    "saldos",
    "saldoa",
    "kontosaldo",
];
const CARGOS: [&str; 52] = [
    "cargo",
    "cargos",
    "debe",
    "debit",
    "debits",
    "debito",
    "debitos",
    "debet",
    "withdrawal",
    "withdrawals",
    "out",
    "saliente",
    "salientes",
    "salidas",
    "salida",
    "soll",
    "belastung",
    "lastschrift",
    "ausgang",
    "ausgaben",
    "abgang",
    "uscite",
    "uscita",
    "addebiti",
    "addebito",
    "dare",
    "spent",
    "afschrijving",
    "uitgaven",
    "gasto",
    "gastos",
    "reintegro",
    "reintegros",
    "disposicion",
    "disposiciones",
    "pagos",
    "carrec",
    "carrecs",
    "deure",
    "sortida",
    "sortides",
    "pagaments",
    "despesa",
    "zorra",
    "kargua",
    "irteera",
    "irteerak",
    "saida",
    "saidas",
    "outflow",
    "charges",
    "sorties",
];
const ABONOS: [&str; 44] = [
    "abono",
    "abonos",
    "haber",
    "credit",
    "credits",
    "credito",
    "creditos",
    "deposit",
    "deposits",
    "in",
    "entrante",
    "entrantes",
    "entradas",
    "entrada",
    "ingresos",
    "ingreso",
    "haben",
    "gutschrift",
    "eingang",
    "einnahmen",
    "zugang",
    "entrate",
    "entrata",
    "accrediti",
    "accredito",
    "avere",
    "received",
    "bijschrijving",
    "inkomsten",
    "abonament",
    "abonaments",
    "haver",
    "ingressos",
    "entrades",
    "hartzekoa",
    "abonua",
    "sarrera",
    "sarrerak",
    "inflow",
    "receipts",
    "entree",
    "entrees",
    "versement",
    "bij",
];
const IMPORTES: [&str; 26] = [
    "importe",
    "importes",
    "amount",
    "amounts",
    "betrag",
    "montant",
    "importo",
    "cantidad",
    "monto",
    "neto",
    "net",
    "bedrag",
    "umsatz",
    "quantia",
    "import",
    "quantitat",
    "zenbatekoa",
    "kopurua",
    "cantidade",
    "somme",
    "summe",
    "ammontare",
    "somma",
    "montante",
    "importancia",
    "transactiebedrag",
];
const CONCEPTOS: [&str; 93] = [
    "concepto",
    "conceptos",
    "descripcion",
    "description",
    "detalle",
    "detalles",
    "details",
    "concept",
    "libelle",
    "verwendungszweck",
    "buchungstext",
    "beschreibung",
    "text",
    "texto",
    "narrative",
    "particulars",
    "transaccion",
    "transaction",
    "movimiento",
    "operacion",
    "beneficiario",
    "ordenante",
    "benef",
    "establecimiento",
    "comercio",
    "merchant",
    "payee",
    "payer",
    "nombre",
    "causale",
    "descrizione",
    "omschrijving",
    "informacion",
    "information",
    "observaciones",
    "remitente",
    "destinatario",
    "contrapartida",
    "contraparte",
    "counterparty",
    "name",
    "memo",
    "motivo",
    "operation",
    "operations",
    "concepte",
    "descripcio",
    "detall",
    "moviment",
    "kontzeptua",
    "deskribapena",
    "azalpena",
    "descricion",
    "movemento",
    "purpose",
    "remittance",
    "notes",
    "note",
    "label",
    "nature",
    "motif",
    "communication",
    "intitule",
    "vorgang",
    "mitteilung",
    "zahlungsgrund",
    "dettagli",
    "operazione",
    "descricao",
    "descritivo",
    "historico",
    "designacao",
    "detalhes",
    "mededelingen",
    "beschrijving",
    "literal",
    "comentario",
    "comentarios",
    "datos",
    "naam",
    "partner",
    "beneficiary",
    "recipient",
    "sender",
    "empfanger",
    "empfaenger",
    "auftraggeber",
    "ordinante",
    "controparte",
    "esercente",
    "beneficiaire",
    "tegenpartij",
    "begunstigde",
];
const REFERENCIAS: [&str; 22] = [
    "referencia",
    "ref",
    "reference",
    "documento",
    "doc",
    "cheque",
    "check",
    "numero",
    "riferimento",
    "referenz",
    "justificante",
    "remesa",
    "erreferentzia",
    "dokumentua",
    "belegnummer",
    "mandatsreferenz",
    "kundenreferenz",
    "rif",
    "referentie",
    "kenmerk",
    "volgnummer",
    "fitid",
];
const OTROS: [&str; 61] = [
    "oficina",
    "codigo",
    "cod",
    "categoria",
    "category",
    "subcategoria",
    "subcategory",
    "categorie",
    "kategorie",
    "etiqueta",
    "etiquetas",
    "tags",
    "tag",
    "tipo",
    "type",
    "estado",
    "status",
    "estat",
    "statut",
    "stato",
    "situacao",
    "cuenta",
    "account",
    "bruto",
    "gross",
    "comision",
    "comisiones",
    "fee",
    "fees",
    "tasa",
    "tax",
    "iva",
    "vat",
    "divisa",
    "moneda",
    "currency",
    "wahrung",
    "waehrung",
    "devise",
    "monnaie",
    "moeda",
    "munt",
    "ccy",
    "clave",
    "hora",
    "time",
    "id",
    "branch",
    "sucursal",
    "filial",
    "card",
    "tarjeta",
    "entidad",
    "iban",
    "cambio",
    "exchange",
    "code",
    "codi",
    "tipus",
    "mota",
    "clase",
];

/// Palabras de las columnas con el importe en la moneda original.
const ORIGINAL: [&str; 9] = [
    "original",
    "origen",
    "orig",
    "foreign",
    "fremdwahrung",
    "fremdwaehrung",
    "divisa",
    "devise",
    "valuta",
];

/// Qué es la columna por su título, en cualquiera de los idiomas habituales.
pub(crate) fn tipo_de_titulo(titulo: &str) -> Option<Tipo> {
    let t = sin_tildes(titulo);
    let palabras: Vec<&str> = t
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if palabras.is_empty() {
        return None;
    }
    let tiene = |w: &str| palabras.contains(&w);
    // Marca de debe y haber en su propia columna.
    let junto: String = t
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '/' && *c != '.')
        .collect();
    if matches!(
        junto.as_str(),
        "dh" | "+-"
            | "-+"
            | "signo"
            | "sign"
            | "crdr"
            | "drcr"
            | "sh"
            | "debehaber"
            | "debitcredit"
            | "dc"
            | "afbij"
            | "sollhaben"
            | "direction"
            | "side"
            | "cargoabono"
    ) {
        return Some(Tipo::Signo);
    }
    let es_fecha = palabras.iter().any(|w| FECHAS.contains(w));
    if palabras.iter().any(|w| VALOR.contains(w)) && (es_fecha || palabras.len() == 1) {
        return Some(Tipo::FechaValor);
    }
    if es_fecha && (tiene("recepcion") || tiene("hora") || tiene("time")) {
        // Fecha de recepción de una tarjeta, hora: no es la de la operación.
        return Some(Tipo::Otro);
    }
    // La primera palabra que dice algo decide («Tipo de movimiento» no es el concepto,
    // «Dinero saliente» son los cargos), salvo que sea de concepto y detrás vaya una de fecha
    // o de referencia («Transaction date», «Transaction id»).
    for w in &palabras {
        let tipo = if FECHAS.contains(w) {
            Tipo::Fecha
        } else if SALDOS.contains(w) {
            Tipo::Saldo
        } else if CARGOS.contains(w) {
            Tipo::Cargo
        } else if ABONOS.contains(w) {
            Tipo::Abono
        } else if IMPORTES.contains(w) {
            // El importe en la moneda original de un pago en otra divisa no es el del
            // movimiento.
            if palabras.iter().any(|x| ORIGINAL.contains(x)) {
                Tipo::Otro
            } else {
                Tipo::Importe
            }
        } else if CONCEPTOS.contains(w) {
            if es_fecha {
                Tipo::Fecha
            } else if palabras
                .iter()
                .any(|x| REFERENCIAS.contains(x) || *x == "id")
            {
                Tipo::Referencia
            } else {
                Tipo::Concepto
            }
        } else if REFERENCIAS.contains(w) {
            Tipo::Referencia
        } else if OTROS.contains(w) {
            Tipo::Otro
        } else if *w == "disponible" {
            Tipo::Saldo
        } else {
            continue;
        };
        return Some(tipo);
    }
    None
}

/// Una línea de títulos con los trozos que juntan varios títulos («Fecha Concepto Importe
/// Saldo», cuando las columnas van muy pegadas) partidos en uno por título: cada palabra que
/// es un título empieza una celda; las demás («(UTC)», «de», «saliente») van con la de
/// delante, salvo detrás de una preposición («Tipo de movimiento» es un solo título).
fn partir_titulos(l: &Linea) -> Linea {
    let mut out = l.clone();
    out.trozos.clear();
    for t in &l.trozos {
        let con_tipo = t
            .palabras
            .iter()
            .filter(|p| tipo_de_titulo(&p.texto).is_some())
            .count();
        // Sólo si junta claramente varios títulos («F. operación» o «Fecha valor» son uno).
        if con_tipo < 3 {
            out.trozos.push(t.clone());
            continue;
        }
        let mut actual: Option<Trozo> = None;
        let mut previa = String::new();
        for p in &t.palabras {
            let preposicion = matches!(
                sin_tildes(&previa).as_str(),
                "de" | "del" | "of" | "da" | "do" | "di" | "du" | "y" | "and" | "/" | "|"
            );
            // «Fecha operación», «Fecha valor», «F. contable»: un solo título.
            let de_fecha = actual
                .as_ref()
                .is_some_and(|a| tipo_de_titulo(&a.texto) == Some(Tipo::Fecha))
                && matches!(
                    sin_tildes(&p.texto).trim_end_matches('.'),
                    "operacion" | "operation" | "contable" | "valor" | "value" | "oper" | "op"
                );
            let nueva = tipo_de_titulo(&p.texto).is_some()
                && !preposicion
                && !de_fecha
                && actual
                    .as_ref()
                    .is_some_and(|a| tipo_de_titulo(&a.texto).is_some());
            match actual.as_mut() {
                Some(a) if !nueva => {
                    a.texto.push(' ');
                    a.texto.push_str(&p.texto);
                    a.x1 = p.x1;
                    a.palabras.push(p.clone());
                }
                _ => {
                    if let Some(a) = actual.take() {
                        out.trozos.push(a);
                    }
                    actual = Some(Trozo {
                        x0: p.x0,
                        x1: p.x1,
                        texto: p.texto.clone(),
                        palabras: vec![p.clone()],
                    });
                }
            }
            previa.clone_from(&p.texto);
        }
        if let Some(a) = actual {
            out.trozos.push(a);
        }
    }
    out
}

/// Una celda de la cabecera mientras se monta: sus trozos de texto, de arriba abajo.
#[derive(Clone, Debug)]
struct Celda {
    x0: i32,
    x1: i32,
    partes: Vec<(usize, String)>,
    /// Línea que manda (la de más trozos): su texto decide el tipo antes que el de las otras.
    base: usize,
}

/// ¿Puede ser una línea de la cabecera (sólo títulos, sin fechas ni importes)?
fn solo_titulos(l: &Linea) -> bool {
    !l.trozos.is_empty() && piezas::de_linea(l).iter().all(Pieza::es_texto)
}

/// Celdas de una cabecera de varias líneas: la línea con más trozos manda; los trozos de las
/// otras se suman a la celda que solapan (o, si solapan varias, cada palabra a la suya) o son
/// una columna nueva.
fn celdas(lineas: &[(usize, &Linea)]) -> Vec<Celda> {
    let partidas: Vec<(usize, Linea)> = lineas
        .iter()
        .map(|(i, l)| (*i, partir_titulos(l)))
        .collect();
    let lineas: Vec<(usize, &Linea)> = partidas.iter().map(|(i, l)| (*i, l)).collect();
    let lineas = lineas.as_slice();
    let Some((base_i, base)) = lineas.iter().max_by_key(|(_, l)| l.trozos.len()) else {
        return Vec::new();
    };
    let mut out: Vec<Celda> = base
        .trozos
        .iter()
        .map(|t| Celda {
            x0: t.x0,
            x1: t.x1,
            partes: vec![(*base_i, t.texto.clone())],
            base: *base_i,
        })
        .collect();
    let solapa = |c: &Celda, x0: i32, x1: i32| x1 > c.x0 && x0 < c.x1;
    for (i, l) in lineas {
        if i == base_i {
            continue;
        }
        for t in &l.trozos {
            let tocadas: Vec<usize> = (0..out.len())
                .filter(|k| solapa(&out[*k], t.x0, t.x1))
                .collect();
            match tocadas.as_slice() {
                [] => out.push(Celda {
                    x0: t.x0,
                    x1: t.x1,
                    partes: vec![(*i, t.texto.clone())],
                    base: *base_i,
                }),
                [k] => {
                    let c = &mut out[*k];
                    c.partes.push((*i, t.texto.clone()));
                    c.x0 = c.x0.min(t.x0);
                    c.x1 = c.x1.max(t.x1);
                }
                _ => {
                    for p in &t.palabras {
                        let k = (0..out.len())
                            .filter(|k| solapa(&out[*k], p.x0, p.x1))
                            .max_by_key(|k| p.x1.min(out[*k].x1) - p.x0.max(out[*k].x0))
                            .or_else(|| {
                                (0..out.len()).min_by_key(|k| {
                                    (out[*k].x0 - p.x1).max(p.x0 - out[*k].x1).max(0)
                                })
                            });
                        if let Some(k) = k {
                            let c = &mut out[k];
                            match c.partes.last_mut() {
                                Some((li, s)) if li == i => {
                                    s.push(' ');
                                    s.push_str(&p.texto);
                                }
                                _ => c.partes.push((*i, p.texto.clone())),
                            }
                            c.x0 = c.x0.min(p.x0);
                            c.x1 = c.x1.max(p.x1);
                        }
                    }
                }
            }
        }
    }
    for c in &mut out {
        c.partes.sort_by_key(|(i, _)| *i);
    }
    out.sort_by_key(|c| c.x0);
    out
}

/// Columnas de unas líneas de cabecera, si lo son: tiene que haber una de fecha y una de
/// importe (o de cargos o abonos). Devuelve también cuántas columnas se han reconocido.
fn columnas(lineas: &[(usize, &Linea)]) -> Option<(Vec<Columna>, usize)> {
    let cs = celdas(lineas);
    let mut cols = Vec::with_capacity(cs.len());
    let mut reconocidas = 0;
    for c in cs {
        let titulo = c
            .partes
            .iter()
            .map(|(_, s)| s.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        // Primero el texto de la línea que manda: un título encima («Movimientos de la cuenta»)
        // no cambia lo que dice la cabecera.
        let mut por_base: Vec<&str> = c
            .partes
            .iter()
            .filter(|(i, _)| *i == c.base)
            .map(|(_, s)| s.as_str())
            .collect();
        por_base.extend(
            c.partes
                .iter()
                .filter(|(i, _)| *i != c.base)
                .map(|(_, s)| s.as_str()),
        );
        let tipo = tipo_de_titulo(&por_base.join(" "));
        if tipo.is_some() {
            reconocidas += 1;
        }
        cols.push(Columna {
            tipo: tipo.unwrap_or(Tipo::Otro),
            titulo,
            x0: c.x0,
            x1: c.x1,
        });
    }
    // Una sola columna de fecha, aunque se llame «Fecha valor», es la de la operación.
    if !cols.iter().any(|c| c.tipo == Tipo::Fecha)
        && let Some(c) = cols.iter_mut().find(|c| c.tipo == Tipo::FechaValor)
    {
        c.tipo = Tipo::Fecha;
    }
    // Si hay dos de fecha, la segunda es la de valor (o, si ya la hay, otra cualquiera).
    let mut vista_fecha = false;
    let hay_valor = cols.iter().any(|c| c.tipo == Tipo::FechaValor);
    for c in &mut cols {
        if c.tipo == Tipo::Fecha {
            if vista_fecha {
                c.tipo = if hay_valor {
                    Tipo::Otro
                } else {
                    Tipo::FechaValor
                };
            }
            vista_fecha = true;
        }
    }
    let fecha = cols.iter().any(|c| c.tipo == Tipo::Fecha);
    let dinero = cols.iter().any(|c| c.tipo.es_dinero());
    (fecha && dinero && cols.len() >= 3).then_some((cols, reconocidas))
}

/// Las cabeceras de tablas de movimientos que hay en las líneas, en orden.
pub(crate) fn buscar(lineas: &[Linea]) -> Vec<Cabecera> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lineas.len() {
        let Some((n, cols, rec)) = mejor_grupo(lineas, i) else {
            i += 1;
            continue;
        };
        // Si sin la primera línea sale lo mismo, la primera era un título de encima.
        let (desde, n, cols) = match mejor_grupo(lineas, i + 1) {
            Some((n2, cols2, rec2)) if n > 1 && rec2 >= rec => (i + 1, n2, cols2),
            _ => (i, n, cols),
        };
        out.push(Cabecera {
            columnas: cols,
            desde,
            hasta: desde + n - 1,
            limites: Vec::new(),
        });
        i = desde + n;
    }
    out
}

/// La mejor cabecera que empieza en la línea `i` (de una a tres líneas): cuántas líneas, sus
/// columnas y cuántas se han reconocido.
fn mejor_grupo(lineas: &[Linea], i: usize) -> Option<(usize, Vec<Columna>, usize)> {
    let primera = lineas.get(i)?;
    if !solo_titulos(primera) {
        return None;
    }
    let mut mejor: Option<(usize, Vec<Columna>, usize)> = None;
    let mut grupo: Vec<(usize, &Linea)> = vec![(i, primera)];
    for n in 1..=3 {
        if n > 1 {
            let j = i + n - 1;
            let (Some(l), Some((_, previa))) = (lineas.get(j), grupo.last()) else {
                break;
            };
            let cerca = l.pagina == previa.pagina
                && l.y - previa.y <= (previa.tam.max(l.tam) * 16 / 10).max(25);
            if !cerca || !solo_titulos(l) {
                break;
            }
            grupo.push((j, l));
        }
        // Una línea más vale si reconoce más columnas o si sólo completa los títulos que ya
        // había («F.» sobre «operación»).
        if let Some((cols, rec)) = columnas(&grupo)
            && mejor
                .as_ref()
                .is_none_or(|(_, c, r)| rec > *r || (rec == *r && cols.len() == c.len()))
        {
            mejor = Some((n, cols, rec));
        }
    }
    mejor
}

/// Pone los límites entre columnas con los datos de las filas que hay debajo de la cabecera:
/// en el hueco entre los títulos de dos columnas, donde menos datos se cruzan (y, si hay varios
/// sitios así, en medio del más ancho).
pub(crate) fn poner_limites(c: &mut Cabecera, filas: &[Vec<Pieza>]) {
    // Tramos ocupados por los datos: cada fecha o importe, y cada grupo de texto seguido.
    let mut tramos: Vec<(i32, i32)> = Vec::new();
    for f in filas {
        let mut k = 0;
        while k < f.len() {
            let g = f[k].grupo;
            let fin = f[k..]
                .iter()
                .position(|p| p.grupo != g)
                .map_or(f.len(), |p| k + p);
            let x0 = f[k..fin].iter().map(|p| p.x0).min().unwrap_or(0);
            let x1 = f[k..fin].iter().map(|p| p.x1).max().unwrap_or(0);
            tramos.push((x0, x1));
            k = fin;
        }
    }
    let mut limites = Vec::with_capacity(c.columnas.len().saturating_sub(1));
    for par in c.columnas.windows(2) {
        let (a, b) = (&par[0], &par[1]);
        let (lo, hi) = (a.x1, b.x0);
        if hi - lo < 4 {
            limites.push((a.x1 + b.x0) / 2);
            continue;
        }
        let mut puntos: Vec<i32> = vec![lo, hi];
        for (x0, x1) in &tramos {
            for x in [*x0, *x1] {
                if x > lo && x < hi {
                    puntos.push(x);
                }
            }
        }
        puntos.sort_unstable();
        puntos.dedup();
        // Huecos con los menos cruces: si la columna de la derecha es de importes (alineados a
        // la derecha), el más cercano a ella; si es de texto o fechas (alineados a la
        // izquierda), el más cercano a la de la izquierda.
        let mut huecos: Vec<(usize, i32, i32)> = Vec::new();
        for w in puntos.windows(2) {
            let (p, q) = (w[0], w[1]);
            if q <= p {
                continue;
            }
            let medio = p + (q - p) / 2;
            let cruces = tramos
                .iter()
                .filter(|(x0, x1)| *x0 < medio && *x1 > medio)
                .count();
            huecos.push((cruces, p, q));
        }
        // Los tramos seguidos con los mismos cruces, juntos.
        let mut juntos: Vec<(usize, i32, i32)> = Vec::new();
        for h in huecos {
            match juntos.last_mut() {
                Some(u) if u.0 == h.0 && u.2 == h.1 => u.2 = h.2,
                _ => juntos.push(h),
            }
        }
        let minimo = juntos.iter().map(|h| h.0).min().unwrap_or(0);
        // Un hueco estrecho (el espacio entre dos palabras de una celda) no separa columnas si
        // hay otro ancho.
        const ANCHO: i32 = 35;
        let hay_ancho = juntos.iter().any(|h| h.0 == minimo && h.2 - h.1 >= ANCHO);
        let buenos = juntos
            .iter()
            .filter(|h| h.0 == minimo && (!hay_ancho || h.2 - h.1 >= ANCHO));
        let elegido = if matches!(
            b.tipo,
            Tipo::Importe | Tipo::Cargo | Tipo::Abono | Tipo::Saldo
        ) {
            buenos.max_by_key(|h| h.1)
        } else {
            buenos.min_by_key(|h| h.1)
        };
        limites.push(elegido.map_or((lo + hi) / 2, |(_, p, q)| p + (q - p) / 2));
    }
    c.limites = limites;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titulos_en_varios_idiomas() {
        let t = |s: &str| tipo_de_titulo(s);
        assert_eq!(t("F. operación"), Some(Tipo::Fecha));
        assert_eq!(t("F. VALOR"), Some(Tipo::FechaValor));
        assert_eq!(t("Fecha valor"), Some(Tipo::FechaValor));
        assert_eq!(t("Valuta"), Some(Tipo::FechaValor));
        assert_eq!(t("Buchungstag"), Some(Tipo::Fecha));
        assert_eq!(t("Date opération"), Some(Tipo::Fecha));
        assert_eq!(t("CONCEPTO|BENEF./ORDENANTE"), Some(Tipo::Concepto));
        assert_eq!(t("Verwendungszweck"), Some(Tipo::Concepto));
        assert_eq!(t("Libellé"), Some(Tipo::Concepto));
        assert_eq!(t("Tipo de movimiento"), Some(Tipo::Otro));
        assert_eq!(t("Importe (€)"), Some(Tipo::Importe));
        assert_eq!(t("Betrag"), Some(Tipo::Importe));
        assert_eq!(t("Dinero saliente"), Some(Tipo::Cargo));
        assert_eq!(t("Dinero entrante"), Some(Tipo::Abono));
        assert_eq!(t("Money out"), Some(Tipo::Cargo));
        assert_eq!(t("Paid in"), Some(Tipo::Abono));
        assert_eq!(t("Debe"), Some(Tipo::Cargo));
        assert_eq!(t("Haber"), Some(Tipo::Abono));
        assert_eq!(t("Soll"), Some(Tipo::Cargo));
        assert_eq!(t("Débit"), Some(Tipo::Cargo));
        assert_eq!(t("Crédit"), Some(Tipo::Abono));
        assert_eq!(t("Solde"), Some(Tipo::Saldo));
        assert_eq!(t("Balance"), Some(Tipo::Saldo));
        assert_eq!(t("D/H"), Some(Tipo::Signo));
        assert_eq!(t("Oficina"), Some(Tipo::Otro));
        assert_eq!(t("F. recepción"), Some(Tipo::Otro));
        assert_eq!(t("Neto"), Some(Tipo::Importe));
        assert_eq!(t("Bruto"), Some(Tipo::Otro));
        assert_eq!(t("Nombre \\ Correo electrónico"), Some(Tipo::Concepto));
        assert_eq!(t("Empresa"), None);
        assert_eq!(t("Transaction date"), Some(Tipo::Fecha));
        assert_eq!(t("Transaction ID"), Some(Tipo::Referencia));
        assert_eq!(t("Booking date"), Some(Tipo::Fecha));
        assert_eq!(t("Data operazione"), Some(Tipo::Fecha));
        assert_eq!(t("Data valuta"), Some(Tipo::FechaValor));
        assert_eq!(t("Uscite"), Some(Tipo::Cargo));
        assert_eq!(t("Entrate"), Some(Tipo::Abono));
        assert_eq!(t("Omschrijving"), Some(Tipo::Concepto));
        assert_eq!(t("Af Bij"), Some(Tipo::Signo));
        assert_eq!(t("Bedrag (EUR)"), Some(Tipo::Importe));
        assert_eq!(t("Kontostand"), Some(Tipo::Saldo));
        assert_eq!(t("Saldo a fecha"), Some(Tipo::Saldo));
        assert_eq!(t("Importe divisa"), Some(Tipo::Otro));
        assert_eq!(t("Más datos"), Some(Tipo::Concepto));
        assert_eq!(t("COMENTARIO"), Some(Tipo::Concepto));
        assert_eq!(t("Naam / Omschrijving"), Some(Tipo::Concepto));
        assert_eq!(t("Payment reference"), Some(Tipo::Referencia));
        assert_eq!(t("Data contabile"), Some(Tipo::Fecha));
        assert_eq!(t("Dare"), Some(Tipo::Cargo));
        assert_eq!(t("Avere"), Some(Tipo::Abono));
        assert_eq!(t("F. Operativa"), Some(Tipo::Fecha));
        assert_eq!(t("Original amount"), Some(Tipo::Otro));
    }

    #[test]
    fn cabecera_partida_en_dos_lineas() {
        let ls = vec![
            Linea::de_trozos(1, 100, &[(500, "Titular: EMPRESA DEMO SL")]),
            Linea::de_trozos(
                1,
                300,
                &[
                    (587, "F."),
                    (924, "F."),
                    (1100, "Oficina"),
                    (1967, "Concepto"),
                    (4677, "Importe"),
                    (5206, "Saldo"),
                ],
            ),
            Linea::de_trozos(1, 392, &[(428, "operación valor")]),
            Linea::de_trozos(1, 520, &[(420, "05/06/2024 03/06"), (4746, "-12,00 €")]),
        ];
        let c = buscar(&ls);
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].desde, c[0].hasta), (1, 2));
        let tipos: Vec<(Tipo, &str)> = c[0]
            .columnas
            .iter()
            .map(|c| (c.tipo, c.titulo.as_str()))
            .collect();
        assert_eq!(
            tipos,
            [
                (Tipo::Fecha, "F. operación"),
                (Tipo::FechaValor, "F. valor"),
                (Tipo::Otro, "Oficina"),
                (Tipo::Concepto, "Concepto"),
                (Tipo::Importe, "Importe"),
                (Tipo::Saldo, "Saldo"),
            ]
        );
    }

    #[test]
    fn titulos_pegados_en_un_solo_trozo() {
        let ls = vec![Linea::de_trozos(
            1,
            100,
            &[(400, "Fecha Fecha valor Concepto Importe Saldo")],
        )];
        let c = buscar(&ls);
        assert_eq!(c.len(), 1);
        let tipos: Vec<(Tipo, &str)> = c[0]
            .columnas
            .iter()
            .map(|c| (c.tipo, c.titulo.as_str()))
            .collect();
        assert_eq!(
            tipos,
            [
                (Tipo::Fecha, "Fecha"),
                (Tipo::FechaValor, "Fecha valor"),
                (Tipo::Concepto, "Concepto"),
                (Tipo::Importe, "Importe"),
                (Tipo::Saldo, "Saldo"),
            ]
        );
        // Un título de dos palabras no se parte.
        let ls = vec![Linea::de_trozos(
            1,
            100,
            &[(400, "F. operación"), (1500, "Concepto"), (3000, "Importe")],
        )];
        assert_eq!(buscar(&ls)[0].columnas[0].titulo, "F. operación");
    }

    #[test]
    fn cabecera_en_tres_lineas_sin_ninguna_completa() {
        let ls = vec![
            Linea::de_trozos(
                1,
                2875,
                &[(435, "Fecha"), (4707, "Dinero"), (5294, "Dinero")],
            ),
            Linea::de_trozos(1, 2928, &[(1134, "Descripción"), (3621, "Estado")]),
            Linea::de_trozos(
                1,
                2982,
                &[(435, "(UTC)"), (4678, "saliente"), (5241, "entrante")],
            ),
        ];
        let c = buscar(&ls);
        assert_eq!(c.len(), 1);
        let tipos: Vec<Tipo> = c[0].columnas.iter().map(|c| c.tipo).collect();
        assert_eq!(
            tipos,
            [
                Tipo::Fecha,
                Tipo::Concepto,
                Tipo::Otro,
                Tipo::Cargo,
                Tipo::Abono
            ]
        );
    }

    #[test]
    fn limites_donde_los_datos_dejan_hueco() {
        // El título «Concepto» va centrado sobre una columna ancha cuyo texto empieza mucho
        // antes; la oficina va justo a la izquierda.
        let ls = vec![
            Linea::de_trozos(
                1,
                100,
                &[
                    (500, "Fecha"),
                    (1520, "Oficina"),
                    (2890, "Concepto"),
                    (4470, "Importe"),
                ],
            ),
            Linea::de_trozos(
                1,
                200,
                &[
                    (527, "14/10/2019"),
                    (1569, "0999"),
                    (1877, "O.PAGO EXT DEMO"),
                    (4481, "12.345,67 €"),
                ],
            ),
        ];
        let mut c = buscar(&ls).remove(0);
        let filas = vec![piezas::de_linea(&ls[1])];
        poner_limites(&mut c, &filas);
        let p = &filas[0];
        let col = |k: usize| c.tipo(c.columna(p[k].x0, p[k].x1));
        assert_eq!(col(0), Tipo::Fecha);
        assert_eq!(col(1), Tipo::Otro);
        assert_eq!(col(2), Tipo::Concepto);
        assert_eq!(col(5), Tipo::Importe);
    }
}

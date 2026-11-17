//! Lectura de operaciones con criptomonedas desde ficheros: una hoja genérica (columnas fecha,
//! tipo, activo, cantidad, euros, comisión, plataforma…), la «Trade List» de CoinTracking y el
//! «ledgers» CSV de Kraken; y una hoja de saldos por fecha (como «CoinTracking - Saldos»: fecha
//! y, por moneda, «Valor en EUR» y «Cantidad») como tabla de precios de cierre. Nada se guarda
//! aquí: se devuelve una vista previa que el usuario confirma.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use taxpain_core::cripto::{Precio, Qty, Tipo};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use time::{Date, OffsetDateTime};

use super::{Op, Price, ticker_norm};
use crate::error::{AppError, Result};
use crate::import::{self, Hoja, norm, parse_amount, parse_date};

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

/// Nombre de la plataforma que representa lo que está fuera de las registradas (monederos de
/// terceros, origen de un depósito, destino de una retirada).
pub const EXTERNO: &str = "Externo";

/// Operación leída, con las plataformas por nombre (se crean al confirmar).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpDraft {
    #[serde(flatten)]
    pub op: Op,
    pub platform: String,
    #[serde(default)]
    pub to_platform: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Lectura {
    /// `cointracking`, `kraken`, `hoja` o `saldos`.
    pub kind: String,
    pub ops: Vec<OpDraft>,
    pub prices: Vec<Price>,
    /// Operaciones ya importadas antes (misma huella).
    pub duplicates: usize,
    /// Plataformas que no existen aún y se crearían.
    pub new_platforms: Vec<String>,
    pub warnings: Vec<String>,
    /// De una hoja de saldos: unidades por fecha (y el coste por moneda si la hoja lo da) para
    /// proponer el punto de partida ([`super::inicio`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoja_saldos: Option<super::inicio::HojaSaldos>,
}

/// País y si cuenta como «del extranjero» para el 721 según el nombre de la plataforma.
/// TODO(VERIFY): un exchange cuya entidad custodia sea residente en España presenta el 172/173
/// y entonces no sería «del extranjero» (art. 42 quater.2 RGAT). Buscado el 2026-10-27: ni la
/// AEAT ni la Orden HFP/887/2023 publican una lista de obligados, y los registros del Banco de
/// España y de la CNMV (MiCA) dicen qué filial está inscrita, no qué entidad custodia cada
/// cuenta (en Kraken, Binance, Coinbase o Crypto.com el contrato suele ser con la entidad
/// europea, no con la española). Por eso se marcan como del extranjero por defecto y el usuario
/// lo corrige mirando con qué entidad tiene el contrato.
pub(crate) fn pais_de_plataforma(name: &str) -> (String, bool) {
    let n = norm(name);
    let tabla: &[(&str, &str, bool)] = &[
        ("kraken", "US", true),
        ("binance", "XX", true),
        ("coinbase", "IE", true),
        ("bitstamp", "LU", true),
        ("bitfinex", "VG", true),
        ("kucoin", "SC", true),
        ("bybit", "AE", true),
        ("okx", "SC", true),
        ("cryptocom", "SG", true),
        ("bitpanda", "AT", true),
        ("revolut", "LT", true),
        ("nexo", "CH", true),
        ("gemini", "US", true),
        ("bit2me", "ES", false),
        ("criptan", "ES", false),
        ("externo", "ES", false),
        ("ledger", "ES", false),
        ("trezor", "ES", false),
        ("metamask", "ES", false),
        ("monedero", "ES", false),
        ("wallet", "ES", false),
    ];
    for (k, pais, fuera) in tabla {
        if n.contains(k) {
            return ((*pais).into(), *fuera);
        }
    }
    ("ES".into(), false)
}

fn huella(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update(b"|");
    }
    hex::encode(h.finalize())
}

/// Fecha y hora de una celda: «10.03.2026 14:05:33», «2026-03-10 14:05:33.1234»,
/// «10/03/2026», un número de serie de Excel con fracción…
pub(crate) fn fecha_hora(s: &str) -> Option<(Date, String)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // Número de serie de Excel con fracción de día.
    if s.bytes()
        .all(|b| b.is_ascii_digit() || b == b'.' || b == b',')
        && !s.contains(' ')
        && let Some(d) = parse_date(s)
    {
        let frac = s.split(['.', ',']).nth(1).unwrap_or("");
        let micro: u64 = format!("{:0<6}", frac.get(..6).unwrap_or(frac))
            .parse()
            .unwrap_or(0);
        let secs = micro * 86_400 / 1_000_000;
        return Some((
            d,
            format!(
                "{:02}:{:02}:{:02}",
                secs / 3600,
                secs % 3600 / 60,
                secs % 60
            ),
        ));
    }
    let (fecha, hora) = match s.split_once([' ', 'T']) {
        Some((f, h)) => (f, h.trim()),
        None => (s, ""),
    };
    let d = parse_date(fecha)?;
    let mut partes = hora
        .split('.')
        .next()
        .unwrap_or("")
        .split(':')
        .map(|p| p.trim().parse::<u8>().unwrap_or(0));
    let h = partes.next().unwrap_or(0).min(23);
    let m = partes.next().unwrap_or(0).min(59);
    let sec = partes.next().unwrap_or(0).min(59);
    Some((d, format!("{h:02}:{m:02}:{sec:02}")))
}

/// Fecha y hora `HH:MM:SS` en UTC pasadas a la hora oficial peninsular.
fn utc_a_peninsular(fecha: Date, hora: &str) -> (Date, String) {
    let mut p = hora.split(':').map(|x| x.parse::<u8>().unwrap_or(0));
    let (h, m, s) = (
        p.next().unwrap_or(0),
        p.next().unwrap_or(0),
        p.next().unwrap_or(0),
    );
    let Ok(t) = time::Time::from_hms(h, m, s) else {
        return (fecha, hora.to_owned());
    };
    let utc = time::PrimitiveDateTime::new(fecha, t).assume_utc();
    let local = utc.to_offset(taxpain_core::spain_time::peninsular_offset(utc));
    (
        local.date(),
        format!(
            "{:02}:{:02}:{:02}",
            local.hour(),
            local.minute(),
            local.second()
        ),
    )
}

pub(super) fn col(head: &[String], names: &[&str]) -> Option<usize> {
    let hn: Vec<String> = head.iter().map(|h| norm(h)).collect();
    names
        .iter()
        .find_map(|n| hn.iter().position(|h| h == &norm(n)))
        .or_else(|| {
            names.iter().find_map(|n| {
                hn.iter()
                    .position(|h| !h.is_empty() && h.contains(&norm(n)))
            })
        })
}

pub(super) fn celda(r: &[String], c: Option<usize>) -> String {
    c.and_then(|i| r.get(i))
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

pub(super) fn es_fiat(t: &str) -> bool {
    matches!(t, "EUR" | "USD" | "GBP" | "CHF" | "JPY" | "USDT" | "USDC")
}

fn es_eur(t: &str) -> bool {
    t == "EUR"
}

/// Ticker de CoinTracking en nombre común: CoinTracking añade un «2» a las monedas cuyo símbolo ya
/// usaba otra más antigua (SOL2 es Solana, FLR2 Flare, SGB2 Songbird). Sólo las que se sabe que
/// son la misma moneda; el resto, tal cual.
pub(crate) fn ticker_cointracking(s: &str) -> String {
    let t = ticker_norm(s);
    match t.as_str() {
        "SOL2" => "SOL".into(),
        "FLR2" => "FLR".into(),
        "SGB2" => "SGB".into(),
        _ => t,
    }
}

/// Ticker de Kraken en nombre común (XXBT → BTC, ZEUR → EUR, DOT.S → DOT…).
pub(crate) fn ticker_kraken(s: &str) -> String {
    let t = ticker_norm(s);
    let base = t
        .split_once('.')
        .map(|(b, _)| b.to_owned())
        .unwrap_or(t.clone());
    let base = match base.as_str() {
        "XXBT" | "XBT" => "BTC",
        "XETH" => "ETH",
        "ETH2" => "ETH",
        "ZEUR" => "EUR",
        "ZUSD" => "USD",
        "ZGBP" => "GBP",
        "XXRP" => "XRP",
        "XLTC" => "LTC",
        "XXDG" => "DOGE",
        "XXLM" => "XLM",
        "XETC" => "ETC",
        "XZEC" => "ZEC",
        "XMLN" => "MLN",
        "XREP" => "REP",
        "XXMR" => "XMR",
        b if b.len() == 4 && (b.starts_with('X') || b.starts_with('Z')) => &b[1..],
        b => b,
    };
    base.to_owned()
}

fn op_base(fecha: Date, hora: String, tipo: Tipo, ticker: &str, qty: Qty, source: &str) -> Op {
    Op {
        id: 0,
        date: fecha,
        time: hora,
        kind: tipo,
        platform_id: 0,
        to_platform_id: None,
        ticker: ticker_norm(ticker),
        qty,
        ticker2: None,
        qty2: None,
        eur: Money::ZERO,
        fee_eur: Money::ZERO,
        ref_price: None,
        note: String::new(),
        source: source.into(),
        fingerprint: None,
        entry_id: None,
    }
}

/// Comisión en cripto convertida a euros con el precio implícito de la operación.
fn fee_en_euros(fee: Qty, qty: Qty, eur: Money) -> Option<Money> {
    if qty.is_zero() || fee.is_zero() {
        return Some(Money::ZERO);
    }
    fee.parte_de(eur, qty)
}

/// Compra con euros y comisión en la misma cripto comprada (el exportador la descuenta del saldo
/// aparte): llegan `cantidad − comisión` unidades y el coste total sigue siendo lo pagado. La
/// comisión, valorada al precio de la operación, pasa del contravalor a la comisión: así el
/// coste de adquisición es «importe + gastos» (art. 35.1.b LIRPF; NRV 10.ª PGC) sin contar dos
/// veces las unidades que se fueron en la comisión.
///
/// Devuelve `false` (y deja la operación como estaba) si no se puede descontar: una comisión
/// igual o mayor que lo comprado no tiene sentido y el llamador avisa para revisar la fila.
#[must_use]
fn fee_cripto_en_compra(op: &mut Op, fee: Qty) -> bool {
    if fee.is_zero() {
        return true;
    }
    let Some(neta) = op
        .qty
        .checked_sub(fee.abs())
        .filter(|q| !q.is_negative() && !q.is_zero())
    else {
        return false;
    };
    let valor = fee_en_euros(fee.abs(), op.qty, op.eur).unwrap_or(Money::ZERO);
    op.qty = neta;
    op.eur -= valor;
    op.fee_eur += valor;
    true
}

/// Venta a euros con la comisión en la misma cripto vendida: salen `cantidad + comisión`
/// unidades. Su valor de mercado es lo cobrado más la comisión valorada al precio de la
/// operación; restada la comisión, el valor de transmisión es lo cobrado (art. 35.2 LIRPF).
///
/// Devuelve `false` (y deja la operación como estaba) si la suma desborda; el llamador avisa.
#[must_use]
fn fee_cripto_en_venta(op: &mut Op, fee: Qty) -> bool {
    if fee.is_zero() {
        return true;
    }
    let Some(total) = op.qty.checked_add(fee.abs()) else {
        return false;
    };
    let valor = fee_en_euros(fee.abs(), op.qty, op.eur).unwrap_or(Money::ZERO);
    op.qty = total;
    op.eur += valor;
    op.fee_eur += valor;
    true
}

/// Aviso de una comisión en la misma cripto que no se ha podido descontar.
fn aviso_fee_sin_descontar(filas: &str, fee: Qty, ticker: &str, op: &Op) -> String {
    let motivo = if op.kind == Tipo::Compra {
        "es igual o mayor que lo comprado"
    } else {
        "la cantidad total no cabe"
    };
    format!(
        "{filas}: la comisión de {fee} {ticker} no se ha podido descontar de la {} de {} {} \
         ({motivo}); la operación queda con la cantidad del fichero y sin esa comisión: \
         revísala a mano",
        op.kind.label().to_lowercase(),
        op.qty,
        op.ticker
    )
}

/// Comisión de una compra o una venta pagada en una tercera cripto (BNB, p. ej.): es otra
/// operación, la entrega de esas unidades, que lleva la huella de la compra o la venta más
/// «:fee». Al calcular, su valor en euros (precio de cierre de ese día) se suma al coste de la
/// compra o se resta del valor de la venta (ver `super::operaciones`).
fn comision_aparte(
    op: &Op,
    feec: &str,
    fee: Qty,
    plataforma: &str,
    huella: &str,
    de: &str,
) -> OpDraft {
    let mut c = op_base(
        op.date,
        op.time.clone(),
        Tipo::Comision,
        feec,
        fee,
        &op.source,
    );
    c.note = format!("comisión de la {de} de {}", op.ticker);
    c.fingerprint = Some(format!("{huella}:fee"));
    OpDraft {
        op: c,
        platform: plataforma.to_owned(),
        to_platform: None,
    }
}

// ---------------- CoinTracking ----------------

fn es_cointracking(head: &[String]) -> bool {
    col(head, &["Buy Amount", "Cantidad de compra"]).is_some()
        && col(head, &["Sell Currency", "Moneda de venta"]).is_some()
        && col(head, &["Exchange", "Plataforma"]).is_some()
}

fn tipo_cointracking(t: &str) -> Option<&'static str> {
    let n = norm(t);
    Some(
        if n.contains("trade") || n.contains("operacion") || n.contains("intercambio") {
            "trade"
        // «Ingreso» (y «Ingresos por intereses»…) es «Income» en la versión española: va abajo,
        // con los ingresos; el depósito es «Depósito».
        } else if n.contains("deposit") || n.contains("deposito") {
            "deposit"
        } else if n.contains("withdraw") || n.contains("retir") {
            "withdrawal"
        } else if n.contains("lost")
            || n.contains("stolen")
            || n.contains("perdid")
            || n.contains("rob")
        {
            "lost"
        } else if n.contains("fee") || n.contains("comision") {
            "fee"
        } else if n.contains("spend")
            || n.contains("expense")
            || n.contains("gasto")
            || n.contains("donation")
            || n.contains("gift")
            || n.contains("regalo")
        {
            "spend"
        } else if n.contains("income")
            || n.contains("staking")
            || n.contains("airdrop")
            || n.contains("mining")
            || n.contains("mineria")
            || n.contains("reward")
            || n.contains("bonus")
            || n.contains("interest")
            || n.contains("lending")
            || n.contains("recompensa")
            || n.contains("ingreso")
        {
            "income"
        } else {
            return None;
        },
    )
}

fn leer_cointracking(filas: &[Vec<String>], head: usize) -> (Vec<OpDraft>, Vec<String>) {
    let h = &filas[head];
    let c_tipo = col(h, &["Type", "Tipo"]);
    let c_buy = col(h, &["Buy Amount", "Cantidad de compra"]);
    let c_buyc = col(h, &["Buy Currency", "Moneda de compra"]);
    let c_sell = col(h, &["Sell Amount", "Cantidad de venta"]);
    let c_sellc = col(h, &["Sell Currency", "Moneda de venta"]);
    let c_fee = col(h, &["Fee", "Comisión"]);
    let c_feec = col(h, &["Fee Currency", "Moneda de la comisión"]);
    let c_ex = col(h, &["Exchange", "Plataforma"]);
    let c_com = col(h, &["Comment", "Comentario"]);
    let c_date = col(h, &["Date", "Fecha"]);
    let mut ops = Vec::new();
    let mut warnings = Vec::new();
    let mut sin_valor = 0;
    for (i, r) in filas.iter().enumerate().skip(head + 1) {
        let raw_tipo = celda(r, c_tipo);
        if raw_tipo.is_empty() {
            continue;
        }
        let Some((fecha, hora)) = fecha_hora(&celda(r, c_date)) else {
            warnings.push(format!("fila {}: sin fecha; se salta", i + 1));
            continue;
        };
        let Some(tipo) = tipo_cointracking(&raw_tipo) else {
            warnings.push(format!(
                "fila {}: tipo «{raw_tipo}» no reconocido; se salta",
                i + 1
            ));
            continue;
        };
        let buy = Qty::parse(&celda(r, c_buy)).unwrap_or(Qty::ZERO);
        let buyc = ticker_norm(&celda(r, c_buyc));
        let sell = Qty::parse(&celda(r, c_sell)).unwrap_or(Qty::ZERO);
        let sellc = ticker_norm(&celda(r, c_sellc));
        let fee = Qty::parse(&celda(r, c_fee)).unwrap_or(Qty::ZERO);
        let feec = ticker_norm(&celda(r, c_feec));
        let exchange = celda(r, c_ex);
        let plataforma = if exchange.is_empty() {
            EXTERNO.to_owned()
        } else {
            exchange
        };
        let nota = celda(r, c_com);
        let huella = huella(&[
            "cointracking",
            &raw_tipo,
            &buy.to_text(),
            &buyc,
            &sell.to_text(),
            &sellc,
            &fee.to_text(),
            &feec,
            &plataforma,
            &nota,
            &fecha.to_string(),
            &hora,
        ]);
        // La huella con el símbolo de CoinTracking (lo ya importado sigue reconociéndose); la
        // operación, con el nombre común.
        let (buyc, sellc, feec) = (
            ticker_cointracking(&buyc),
            ticker_cointracking(&sellc),
            ticker_cointracking(&feec),
        );
        let mut extra: Option<OpDraft> = None;
        let (mut op, to) = match tipo {
            "trade" => {
                if es_eur(&sellc) && !buyc.is_empty() {
                    let eur =
                        Money::from_cents(i64::try_from(sell.raw() / 10i128.pow(16)).unwrap_or(0));
                    let mut op = op_base(fecha, hora, Tipo::Compra, &buyc, buy, "cointracking");
                    op.eur = eur;
                    if es_eur(&feec) {
                        op.fee_eur = cents_de(fee);
                    } else if feec == buyc {
                        if !fee_cripto_en_compra(&mut op, fee) {
                            warnings.push(aviso_fee_sin_descontar(
                                &format!("fila {}", i + 1),
                                fee,
                                &feec,
                                &op,
                            ));
                        }
                    } else if !fee.is_zero() && !feec.is_empty() {
                        sin_valor += 1;
                        extra = Some(comision_aparte(
                            &op,
                            &feec,
                            fee,
                            &plataforma,
                            &huella,
                            "compra",
                        ));
                    }
                    (op, None)
                } else if es_eur(&buyc) && !sellc.is_empty() {
                    let eur =
                        Money::from_cents(i64::try_from(buy.raw() / 10i128.pow(16)).unwrap_or(0));
                    let mut op = op_base(fecha, hora, Tipo::Venta, &sellc, sell, "cointracking");
                    op.eur = eur;
                    if es_eur(&feec) {
                        op.fee_eur = cents_de(fee);
                    } else if feec == sellc {
                        if !fee_cripto_en_venta(&mut op, fee) {
                            warnings.push(aviso_fee_sin_descontar(
                                &format!("fila {}", i + 1),
                                fee,
                                &feec,
                                &op,
                            ));
                        }
                    } else if !fee.is_zero() && !feec.is_empty() {
                        sin_valor += 1;
                        extra = Some(comision_aparte(
                            &op,
                            &feec,
                            fee,
                            &plataforma,
                            &huella,
                            "venta",
                        ));
                    }
                    (op, None)
                } else {
                    let mut op = op_base(fecha, hora, Tipo::Permuta, &sellc, sell, "cointracking");
                    op.ticker2 = Some(buyc.clone());
                    op.qty2 = Some(buy);
                    sin_valor += 1;
                    if es_fiat(&sellc) || es_fiat(&buyc) {
                        warnings.push(format!(
                            "fila {}: cambio con {} (no euros): se trata como permuta; pon su \
                             valor en euros",
                            i + 1,
                            if es_fiat(&sellc) { &sellc } else { &buyc }
                        ));
                    }
                    if !fee.is_zero() && !feec.is_empty() && !es_eur(&feec) {
                        let mut c = op_base(
                            fecha,
                            op.time.clone(),
                            Tipo::Comision,
                            &feec,
                            fee,
                            "cointracking",
                        );
                        c.note = "comisión de la permuta".into();
                        c.fingerprint = Some(huella.clone() + ":fee");
                        extra = Some(OpDraft {
                            op: c,
                            platform: plataforma.clone(),
                            to_platform: None,
                        });
                    }
                    (op, None)
                }
            }
            "deposit" => {
                if es_eur(&buyc) {
                    continue;
                }
                (
                    op_base(fecha, hora, Tipo::Traspaso, &buyc, buy, "cointracking"),
                    Some(plataforma.clone()),
                )
            }
            "withdrawal" => {
                if es_eur(&sellc) {
                    continue;
                }
                let op = op_base(fecha, hora, Tipo::Traspaso, &sellc, sell, "cointracking");
                if !fee.is_zero() && !es_eur(&feec) {
                    let mut c = op_base(
                        fecha,
                        op.time.clone(),
                        Tipo::Comision,
                        &feec,
                        fee,
                        "cointracking",
                    );
                    c.note = "comisión de red de la retirada".into();
                    c.fingerprint = Some(huella.clone() + ":fee");
                    extra = Some(OpDraft {
                        op: c,
                        platform: plataforma.clone(),
                        to_platform: None,
                    });
                }
                (op, Some(EXTERNO.to_owned()))
            }
            "income" => {
                if es_eur(&buyc) {
                    continue;
                }
                sin_valor += 1;
                (
                    op_base(fecha, hora, Tipo::Recompensa, &buyc, buy, "cointracking"),
                    None,
                )
            }
            "spend" => {
                if es_eur(&sellc) {
                    continue;
                }
                sin_valor += 1;
                (
                    op_base(fecha, hora, Tipo::Pago, &sellc, sell, "cointracking"),
                    None,
                )
            }
            "lost" => (
                op_base(fecha, hora, Tipo::Perdida, &sellc, sell, "cointracking"),
                None,
            ),
            _ => {
                sin_valor += 1;
                (
                    op_base(fecha, hora, Tipo::Comision, &sellc, sell, "cointracking"),
                    None,
                )
            }
        };
        if op.ticker.is_empty() || op.qty.is_zero() {
            warnings.push(format!("fila {}: sin activo o cantidad; se salta", i + 1));
            continue;
        }
        op.note = nota;
        op.fingerprint = Some(huella);
        let (platform, to_platform) = match (tipo, to) {
            ("deposit", Some(dest)) => (EXTERNO.to_owned(), Some(dest)),
            (_, to) => (plataforma.clone(), to),
        };
        ops.push(OpDraft {
            op,
            platform,
            to_platform,
        });
        if let Some(e) = extra {
            ops.push(e);
        }
    }
    if sin_valor > 0 {
        warnings.push(format!(
            "{sin_valor} operaciones (permutas, recompensas, pagos o comisiones en cripto) no \
             traen valor en euros: se toma el precio de cierre de ese día si lo hay o hay que \
             ponerlo a mano."
        ));
    }
    (ops, warnings)
}

// ---------------- Kraken ----------------

fn es_kraken(head: &[String]) -> bool {
    col(head, &["txid"]).is_some()
        && col(head, &["refid"]).is_some()
        && col(head, &["asset"]).is_some()
        && col(head, &["amount"]).is_some()
}

struct FilaKraken {
    fila: usize,
    txid: String,
    refid: String,
    fecha: Date,
    hora: String,
    tipo: String,
    /// Subtipo del libro de Kraken («allocation», «deallocation», «migration», «reward»…).
    subtipo: String,
    asset: String,
    amount: Qty,
    fee: Qty,
}

fn cents_de(q: Qty) -> Money {
    Money::from_cents(i64::try_from(q.raw() / 10i128.pow(16)).unwrap_or(0))
}

fn leer_kraken(filas: &[Vec<String>], head: usize) -> (Vec<OpDraft>, Vec<String>) {
    let h = &filas[head];
    let c = |n: &str| col(h, &[n]);
    let (c_txid, c_refid, c_time, c_type, c_asset, c_amount, c_fee) = (
        c("txid"),
        c("refid"),
        c("time"),
        c("type"),
        c("asset"),
        c("amount"),
        c("fee"),
    );
    let c_subtype = c("subtype");
    let mut warnings = Vec::new();
    let mut rows: Vec<FilaKraken> = Vec::new();
    for (i, r) in filas.iter().enumerate().skip(head + 1) {
        let txid = celda(r, c_txid);
        if txid.is_empty() {
            continue; // movimientos pendientes sin txid
        }
        let Some((fecha, hora)) = fecha_hora(&celda(r, c_time)) else {
            warnings.push(format!("fila {}: sin fecha; se salta", i + 1));
            continue;
        };
        // Los ledgers de Kraken van en UTC: la fecha de la operación es la de Madrid.
        let (fecha, hora) = utc_a_peninsular(fecha, &hora);
        rows.push(FilaKraken {
            fila: i + 1,
            txid,
            refid: celda(r, c_refid),
            fecha,
            hora,
            tipo: norm(&celda(r, c_type)),
            subtipo: norm(&celda(r, c_subtype)),
            asset: ticker_kraken(&celda(r, c_asset)),
            amount: Qty::parse(&celda(r, c_amount)).unwrap_or(Qty::ZERO),
            fee: Qty::parse(&celda(r, c_fee)).unwrap_or(Qty::ZERO),
        });
    }
    let mut ops = Vec::new();
    let mut por_ref: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, r) in rows.iter().enumerate() {
        if matches!(r.tipo.as_str(), "trade" | "spend" | "receive" | "margin") {
            por_ref.entry(r.refid.clone()).or_default().push(i);
        }
    }
    let mut usados = vec![false; rows.len()];
    let mut sin_valor = 0;
    let mut ignorados = 0;
    // Movimientos de operaciones con margen: el cierre (euros contra 0 de cripto), los intereses
    // («rollover») y las patas sueltas.
    let mut margen = 0;
    // Pares de un mismo refid: compra, venta o permuta.
    for (refid, idx) in &por_ref {
        if idx.len() != 2 {
            continue;
        }
        let (a, b) = (&rows[idx[0]], &rows[idx[1]]);
        usados[idx[0]] = true;
        usados[idx[1]] = true;
        // El cierre de una posición con margen liquida el resultado en euros contra una pata de
        // cripto a cero: no es una compraventa.
        if a.amount.is_zero() || b.amount.is_zero() {
            margen += 1;
            continue;
        }
        let huella = huella(&["kraken", refid, &a.txid, &b.txid]);
        let (eur_leg, cripto_leg) = if es_eur(&a.asset) && !es_eur(&b.asset) {
            (Some(a), b)
        } else if es_eur(&b.asset) && !es_eur(&a.asset) {
            (Some(b), a)
        } else {
            (None, a)
        };
        let mut op = match eur_leg {
            Some(e) => {
                let eur = cents_de(e.amount.abs());
                let qty = cripto_leg.amount.abs();
                let mut op = if cripto_leg.amount.is_negative() {
                    op_base(
                        a.fecha,
                        a.hora.clone(),
                        Tipo::Venta,
                        &cripto_leg.asset,
                        qty,
                        "kraken",
                    )
                } else {
                    op_base(
                        a.fecha,
                        a.hora.clone(),
                        Tipo::Compra,
                        &cripto_leg.asset,
                        qty,
                        "kraken",
                    )
                };
                op.eur = eur;
                op.fee_eur = cents_de(e.fee);
                // En el libro de Kraken la comisión sale del saldo aparte del importe.
                let descontada = if cripto_leg.amount.is_negative() {
                    fee_cripto_en_venta(&mut op, cripto_leg.fee)
                } else {
                    fee_cripto_en_compra(&mut op, cripto_leg.fee)
                };
                if !descontada {
                    warnings.push(aviso_fee_sin_descontar(
                        &format!("filas {} y {}", a.fila, b.fila),
                        cripto_leg.fee.abs(),
                        &cripto_leg.asset,
                        &op,
                    ));
                }
                op
            }
            None => {
                let (entregado, recibido) = if a.amount.is_negative() {
                    (a, b)
                } else {
                    (b, a)
                };
                // La comisión sale del saldo aparte del importe: se entregan más unidades y se
                // reciben menos.
                let entregada = entregado.amount.abs().checked_add(entregado.fee.abs());
                let recibida = recibido
                    .amount
                    .abs()
                    .checked_sub(recibido.fee.abs())
                    .filter(|q| !q.is_negative() && !q.is_zero());
                let mut op = op_base(
                    a.fecha,
                    a.hora.clone(),
                    Tipo::Permuta,
                    &entregado.asset,
                    entregada.unwrap_or(entregado.amount.abs()),
                    "kraken",
                );
                op.ticker2 = Some(recibido.asset.clone());
                op.qty2 = Some(recibida.unwrap_or(recibido.amount.abs()));
                sin_valor += 1;
                if entregada.is_none() || recibida.is_none() {
                    warnings.push(format!(
                        "filas {} y {}: la comisión de la permuta ({} {} / {} {}) no se ha \
                         podido descontar de las cantidades (es igual o mayor que lo recibido, \
                         o no cabe); la permuta queda con las cantidades del fichero: revísala \
                         a mano",
                        a.fila,
                        b.fila,
                        entregado.fee,
                        entregado.asset,
                        recibido.fee,
                        recibido.asset
                    ));
                } else if !entregado.fee.is_zero() || !recibido.fee.is_zero() {
                    warnings.push(format!(
                        "filas {} y {}: la comisión de la permuta ({} {} / {} {}) ya se ha \
                         quitado de las cantidades; su valor queda dentro del de la permuta",
                        a.fila,
                        b.fila,
                        entregado.fee,
                        entregado.asset,
                        recibido.fee,
                        recibido.asset
                    ));
                }
                op
            }
        };
        op.fingerprint = Some(huella);
        op.note = format!("Kraken {refid}");
        ops.push(OpDraft {
            op,
            platform: "Kraken".into(),
            to_platform: None,
        });
    }
    for (i, r) in rows.iter().enumerate() {
        if usados[i] {
            continue;
        }
        let huella = huella(&["kraken", &r.txid, &r.refid]);
        let qty = r.amount.abs();
        let (op, platform, to) = match r.tipo.as_str() {
            "deposit" if !es_eur(&r.asset) => (
                op_base(
                    r.fecha,
                    r.hora.clone(),
                    Tipo::Traspaso,
                    &r.asset,
                    qty,
                    "kraken",
                ),
                EXTERNO.to_owned(),
                Some("Kraken".to_owned()),
            ),
            "withdrawal" if !es_eur(&r.asset) => {
                if !r.fee.is_zero() {
                    let mut c = op_base(
                        r.fecha,
                        r.hora.clone(),
                        Tipo::Comision,
                        &r.asset,
                        r.fee,
                        "kraken",
                    );
                    c.note = format!("comisión de red de la retirada {}", r.txid);
                    c.fingerprint = Some(huella.clone() + ":fee");
                    sin_valor += 1;
                    ops.push(OpDraft {
                        op: c,
                        platform: "Kraken".into(),
                        to_platform: None,
                    });
                }
                (
                    op_base(
                        r.fecha,
                        r.hora.clone(),
                        Tipo::Traspaso,
                        &r.asset,
                        qty,
                        "kraken",
                    ),
                    "Kraken".to_owned(),
                    Some(EXTERNO.to_owned()),
                )
            }
            // En el libro nuevo, «earn» también son los movimientos entre la cuenta y el
            // staking («allocation», «deallocation», «autoallocation», «migration»): no son
            // ingresos. Sólo es recompensa el subtipo «reward» (o el tipo antiguo «staking»).
            "staking" | "earn" | "reward" | "dividend"
                if !es_eur(&r.asset)
                    && !r.amount.is_negative()
                    && !(r.tipo == "earn"
                        && matches!(
                            r.subtipo.as_str(),
                            "allocation" | "deallocation" | "autoallocation" | "migration"
                        )) =>
            {
                sin_valor += 1;
                (
                    op_base(
                        r.fecha,
                        r.hora.clone(),
                        Tipo::Recompensa,
                        &r.asset,
                        qty,
                        "kraken",
                    ),
                    "Kraken".to_owned(),
                    None,
                )
            }
            "deposit" | "withdrawal" | "transfer" | "staking" | "earn" | "reward" | "dividend"
            | "settled" | "adjustment" | "credit" => {
                ignorados += 1;
                continue;
            }
            "margin" | "rollover" => {
                margen += 1;
                continue;
            }
            "trade" | "spend" | "receive" => {
                warnings.push(format!(
                    "fila {}: operación {} sin pareja (refid {}); se salta",
                    r.fila, r.tipo, r.refid
                ));
                continue;
            }
            t => {
                warnings.push(format!(
                    "fila {}: tipo «{t}» no reconocido; se salta",
                    r.fila
                ));
                continue;
            }
        };
        let mut op = op;
        op.fingerprint = Some(huella);
        op.note = format!("Kraken {}", r.txid);
        ops.push(OpDraft {
            op,
            platform,
            to_platform: to,
        });
    }
    if ignorados > 0 {
        warnings.push(format!(
            "{ignorados} movimientos de euros, traspasos internos o ajustes de Kraken no se \
             importan (no cambian lo que tienes)."
        ));
    }
    if margen > 0 {
        warnings.push(format!(
            "{margen} movimientos de operaciones con margen de Kraken (cierres e intereses \
             «rollover») no se importan: son el resultado de derivados, no compraventas de \
             cripto; contabilízalos aparte."
        ));
    }
    if sin_valor > 0 {
        warnings.push(format!(
            "{sin_valor} operaciones (permutas, recompensas o comisiones en cripto) no traen \
             valor en euros: se toma el precio de cierre de ese día si lo hay o hay que ponerlo a \
             mano."
        ));
    }
    (ops, warnings)
}

// ---------------- Hoja genérica ----------------

fn tipo_generico(s: &str) -> Option<Tipo> {
    let n = norm(s);
    if let Some(t) = Tipo::parse(&n) {
        return Some(t);
    }
    Some(if n.starts_with("compra") || n == "buy" {
        Tipo::Compra
    } else if n.starts_with("venta") || n == "sell" {
        Tipo::Venta
    } else if n.contains("permuta")
        || n.contains("swap")
        || n.contains("intercambio")
        || n == "trade"
    {
        Tipo::Permuta
    } else if n.contains("traspaso") || n.contains("transfer") || n.contains("envio") {
        Tipo::Traspaso
    } else if n.contains("comision") || n.contains("fee") {
        Tipo::Comision
    } else if n.contains("recompensa")
        || n.contains("staking")
        || n.contains("airdrop")
        || n.contains("reward")
        || n.contains("interes")
    {
        Tipo::Recompensa
    } else if n.contains("perdida")
        || n.contains("robo")
        || n.contains("lost")
        || n.contains("stolen")
    {
        Tipo::Perdida
    } else if n.contains("pago") || n.contains("gasto") || n.contains("spend") {
        Tipo::Pago
    } else if n.contains("cobro") || n.contains("factura") {
        Tipo::Cobro
    } else {
        return None;
    })
}

struct ColsHoja {
    fecha: usize,
    hora: Option<usize>,
    tipo: usize,
    activo: usize,
    cantidad: usize,
    euros: Option<usize>,
    comision: Option<usize>,
    plataforma: Option<usize>,
    destino: Option<usize>,
    activo2: Option<usize>,
    cantidad2: Option<usize>,
    precio: Option<usize>,
    nota: Option<usize>,
}

fn cols_hoja(h: &[String]) -> Option<ColsHoja> {
    Some(ColsHoja {
        fecha: col(h, &["fecha", "date"])?,
        hora: col(h, &["hora", "time"]),
        tipo: col(h, &["tipo", "operacion", "type"])?,
        activo: col(
            h,
            &["activo", "moneda", "cripto", "ticker", "asset", "coin"],
        )?,
        cantidad: col(h, &["cantidad", "unidades", "amount", "qty"])?,
        euros: col(h, &["euros", "importe", "contravalor", "eur", "valor"]),
        comision: col(h, &["comision", "fee"]),
        plataforma: col(h, &["plataforma", "exchange", "monedero", "wallet"]),
        destino: col(h, &["destino", "a plataforma", "to"]),
        activo2: col(h, &["activo recibido", "moneda recibida", "recibido"]),
        cantidad2: col(h, &["cantidad recibida", "unidades recibidas"]),
        precio: col(h, &["precio", "price"]),
        nota: col(h, &["nota", "concepto", "comentario", "comment"]),
    })
}

fn leer_hoja(filas: &[Vec<String>], head: usize, c: &ColsHoja) -> (Vec<OpDraft>, Vec<String>) {
    let mut ops = Vec::new();
    let mut warnings = Vec::new();
    for (i, r) in filas.iter().enumerate().skip(head + 1) {
        let f = celda(r, Some(c.fecha));
        if f.is_empty() {
            continue;
        }
        let Some((fecha, mut hora)) = fecha_hora(&f) else {
            warnings.push(format!(
                "fila {}: fecha «{f}» no reconocida; se salta",
                i + 1
            ));
            continue;
        };
        if let Some(h) = c.hora.map(|h| celda(r, Some(h))).filter(|h| !h.is_empty())
            && let Some((_, hh)) = fecha_hora(&format!("{fecha} {h}"))
        {
            hora = hh;
        }
        let raw_tipo = celda(r, Some(c.tipo));
        let Some(tipo) = tipo_generico(&raw_tipo) else {
            warnings.push(format!(
                "fila {}: tipo «{raw_tipo}» no reconocido (compra, venta, permuta, traspaso, \
                 comisión, recompensa, pérdida, pago, cobro); se salta",
                i + 1
            ));
            continue;
        };
        let ticker = ticker_norm(&celda(r, Some(c.activo)));
        let Some(qty) = Qty::parse(&celda(r, Some(c.cantidad))) else {
            warnings.push(format!("fila {}: cantidad no válida; se salta", i + 1));
            continue;
        };
        let mut op = op_base(fecha, hora, tipo, &ticker, qty.abs(), "hoja");
        op.eur = c
            .euros
            .and_then(|k| parse_amount(&celda(r, Some(k))))
            .unwrap_or(Money::ZERO)
            .abs();
        op.fee_eur = c
            .comision
            .and_then(|k| parse_amount(&celda(r, Some(k))))
            .unwrap_or(Money::ZERO)
            .abs();
        op.ref_price = c.precio.and_then(|k| Precio::parse(&celda(r, Some(k))));
        op.note = celda(r, c.nota);
        if tipo == Tipo::Permuta {
            op.ticker2 = Some(ticker_norm(&celda(r, c.activo2)));
            op.qty2 = Qty::parse(&celda(r, c.cantidad2));
        }
        let plataforma = {
            let p = celda(r, c.plataforma);
            if p.is_empty() { EXTERNO.to_owned() } else { p }
        };
        let destino = celda(r, c.destino);
        op.fingerprint = Some(huella(&[
            "hoja",
            &fecha.to_string(),
            &op.time,
            tipo.code(),
            &ticker,
            &op.qty.to_text(),
            &op.eur.cents().to_string(),
            &op.fee_eur.cents().to_string(),
            &plataforma,
            &destino,
        ]));
        ops.push(OpDraft {
            op,
            platform: plataforma,
            to_platform: if tipo == Tipo::Traspaso && !destino.is_empty() {
                Some(destino)
            } else {
                None
            },
        });
    }
    (ops, warnings)
}

// ---------------- Hoja de saldos → precios ----------------

/// Columnas «Cantidad» de una hoja de saldos y, para cada una, el ticker (en la misma celda,
/// «BTC Cantidad», o en la fila de encima) y la columna «Valor en EUR» de esa moneda.
pub(super) fn cols_saldos(filas: &[Vec<String>], head: usize) -> Vec<(String, usize, usize)> {
    let h = &filas[head];
    let arriba = head.checked_sub(1).map(|i| &filas[i]);
    let mut out = Vec::new();
    for (j, cell) in h.iter().enumerate() {
        let n = norm(cell);
        if !n.contains("cantidad") && !n.contains("amount") {
            continue;
        }
        // Ticker en la propia celda («BTC Cantidad») o en la fila de arriba (misma columna o
        // la última no vacía hacia la izquierda).
        let propio = cell.split_whitespace().map(ticker_norm).find(|w| {
            !matches!(norm(w).as_str(), "cantidad" | "amount")
                && w.chars().all(|c| c.is_ascii_alphanumeric())
        });
        let ticker = propio.or_else(|| {
            let fila = arriba?;
            (0..=j).rev().find_map(|k| {
                fila.get(k)
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(ticker_norm)
            })
        });
        let Some(ticker) = ticker.map(|t| ticker_cointracking(&t)) else {
            continue;
        };
        // «Valor en EUR» de esa moneda: la columna vecina (izquierda o derecha).
        let valor = [j.checked_sub(1), Some(j + 1)]
            .into_iter()
            .flatten()
            .find(|&k| {
                let v = norm(h.get(k).map(String::as_str).unwrap_or(""));
                v.contains("valor") || v.contains("eur") || v.contains("value")
            });
        if let Some(valor) = valor {
            out.push((ticker, j, valor));
        }
    }
    out
}

fn leer_saldos(
    filas: &[Vec<String>],
    head: usize,
    cols: &[(String, usize, usize)],
) -> (Vec<Price>, Vec<String>) {
    let c_fecha = col(&filas[head], &["fecha", "date"]).unwrap_or(0);
    let mut prices = Vec::new();
    let mut warnings = Vec::new();
    for r in filas.iter().skip(head + 1) {
        let Some((fecha, _)) = fecha_hora(&celda(r, Some(c_fecha))) else {
            continue;
        };
        for (ticker, cj, vj) in cols {
            let Some(q) = Qty::parse(&celda(r, Some(*cj))) else {
                continue;
            };
            let Some(valor) = parse_amount(&celda(r, Some(*vj))) else {
                continue;
            };
            if q.is_zero() || valor.is_zero() {
                continue;
            }
            match q.precio_de(valor) {
                Some(p) if !p.is_zero() => prices.push(Price {
                    date: fecha,
                    ticker: ticker.clone(),
                    price: p,
                    source: "saldos".into(),
                }),
                _ => warnings.push(format!("{ticker} a {fecha}: no sale un precio")),
            }
        }
    }
    (prices, warnings)
}

// ---------------- Entrada ----------------

/// Lee el fichero y devuelve la vista previa (nada se guarda).
pub fn leer(v: &Vault, filename: &str, bytes: &[u8]) -> Result<Lectura> {
    let hojas: Vec<Hoja> = import::sheets(filename, bytes)?;
    let mut lectura: Option<Lectura> = None;
    for h in &hojas {
        if let Some(l) = leer_hoja_cualquiera(&h.filas) {
            lectura = Some(l);
            break;
        }
    }
    let mut l = lectura.ok_or_else(|| {
        other(
            "no lo reconozco: hace falta la «Trade List» de CoinTracking, el ledgers CSV de \
             Kraken, una hoja con columnas Fecha, Tipo, Activo, Cantidad, Euros, Comisión y \
             Plataforma, o una hoja de saldos por fecha (Valor en EUR y Cantidad por moneda)",
        )
    })?;
    // Duplicados y plataformas nuevas.
    let conn = v.connection();
    let mut dup = 0;
    let mut avisos = Vec::new();
    for d in &l.ops {
        match super::reimportada(conn, &d.op)? {
            None => {}
            Some(r) => {
                dup += 1;
                match r {
                    super::Reimportada::Igual => {}
                    super::Reimportada::Corregir { aviso, .. }
                    | super::Reimportada::NoSeToca { aviso } => avisos.push(aviso),
                }
            }
        }
    }
    l.duplicates = dup;
    // Primero lo que hay que corregir en lo ya guardado, que es lo que más importa.
    avisos.append(&mut l.warnings);
    l.warnings = avisos;
    let existentes: Vec<String> = super::platforms(v)?
        .into_iter()
        .map(|p| p.name.to_lowercase())
        .collect();
    let mut nuevas: Vec<String> = Vec::new();
    for d in &l.ops {
        for name in std::iter::once(&d.platform).chain(d.to_platform.iter()) {
            if !existentes.contains(&name.to_lowercase())
                && !nuevas.iter().any(|n| n.eq_ignore_ascii_case(name))
            {
                nuevas.push(name.clone());
            }
        }
    }
    l.new_platforms = nuevas;
    if l.kind == "saldos" {
        l.hoja_saldos = super::inicio::leer_hoja(filename, &hojas);
    }
    Ok(l)
}

fn leer_hoja_cualquiera(filas: &[Vec<String>]) -> Option<Lectura> {
    for (i, h) in filas.iter().enumerate().take(15) {
        if es_cointracking(h) {
            let (ops, warnings) = leer_cointracking(filas, i);
            return Some(Lectura {
                kind: "cointracking".into(),
                ops,
                warnings,
                ..Default::default()
            });
        }
        if es_kraken(h) {
            let (ops, warnings) = leer_kraken(filas, i);
            return Some(Lectura {
                kind: "kraken".into(),
                ops,
                warnings,
                ..Default::default()
            });
        }
        if let Some(c) = cols_hoja(h) {
            let (ops, warnings) = leer_hoja(filas, i, &c);
            return Some(Lectura {
                kind: "hoja".into(),
                ops,
                warnings,
                ..Default::default()
            });
        }
        let saldos = cols_saldos(filas, i);
        if !saldos.is_empty() && col(h, &["fecha", "date"]).is_some() {
            let (prices, warnings) = leer_saldos(filas, i, &saldos);
            if !prices.is_empty() {
                return Some(Lectura {
                    kind: "saldos".into(),
                    prices,
                    warnings,
                    ..Default::default()
                });
            }
        }
    }
    None
}

/// Guarda lo confirmado de una vista previa: crea las plataformas que falten, importa las
/// operaciones (sin duplicados) y los precios. Devuelve (operaciones nuevas, duplicadas,
/// precios).
pub fn aplicar(v: &Vault, l: &Lectura, now: OffsetDateTime) -> Result<(usize, usize, usize)> {
    let ts = crate::invoicing::now_str(now);
    let mut ops = Vec::with_capacity(l.ops.len());
    {
        let conn = v.connection();
        for d in &l.ops {
            let mut op = d.op.clone();
            op.platform_id = super::ensure_platform(conn, &d.platform, &ts)?;
            op.to_platform_id = match &d.to_platform {
                Some(n) => Some(super::ensure_platform(conn, n, &ts)?),
                None => None,
            };
            ops.push(op);
        }
    }
    let (nuevas, dup) = super::import_ops(v, &ops, now)?;
    let precios = super::set_prices(v, &l.prices, now)?;
    if let Some(h) = &l.hoja_saldos {
        super::inicio::recordar_hoja(v, h)?;
    }
    Ok((nuevas, dup, precios))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn fechas_con_hora_de_cointracking_kraken_y_excel() {
        assert_eq!(
            fecha_hora("10.03.2026 14:05:33"),
            Some((date!(2026 - 03 - 10), "14:05:33".into()))
        );
        assert_eq!(
            fecha_hora("2026-03-10 14:05:33.1234"),
            Some((date!(2026 - 03 - 10), "14:05:33".into()))
        );
        assert_eq!(
            fecha_hora("10/03/2026"),
            Some((date!(2026 - 03 - 10), "00:00:00".into()))
        );
        // 46091 = 10/03/2026; 0,5 = mediodía.
        assert_eq!(
            fecha_hora("46091.5"),
            Some((date!(2026 - 03 - 10), "12:00:00".into()))
        );
        assert_eq!(fecha_hora(""), None);
    }

    #[test]
    fn tickers_de_kraken() {
        assert_eq!(ticker_kraken("XXBT"), "BTC");
        assert_eq!(ticker_kraken("ZEUR"), "EUR");
        assert_eq!(ticker_kraken("DOT.S"), "DOT");
        assert_eq!(ticker_kraken("ETH2.S"), "ETH");
        assert_eq!(ticker_kraken("SOL"), "SOL");
        assert_eq!(ticker_kraken("XTZ"), "XTZ");
    }

    /// CoinTracking llama SOL2, FLR2 y SGB2 a Solana, Flare y Songbird: son la misma moneda que
    /// SOL, FLR y SGB de otros sitios. Otros símbolos con cifras se quedan como están.
    #[test]
    fn tickers_de_cointracking_con_sufijo() {
        assert_eq!(ticker_cointracking("sol2"), "SOL");
        assert_eq!(ticker_cointracking("FLR2"), "FLR");
        assert_eq!(ticker_cointracking("SGB2"), "SGB");
        assert_eq!(ticker_cointracking("API3"), "API3");
        assert_eq!(ticker_cointracking("L2"), "L2");
        let f = filas(
            "Type;Buy Amount;Buy Currency;Sell Amount;Sell Currency;Fee;Fee Currency;Exchange;Trade Group;Comment;Date\n\
             Trade;10;SOL2;1000;EUR;;;Kraken;;;10.01.2026 10:00:00\n\
             Staking;5;FLR2;;;;;Kraken;;;11.01.2026 10:00:00\n\
             Trade;100;SGB2;1;SOL2;;;Kraken;;;12.01.2026 10:00:00",
        );
        let l = leer_hoja_cualquiera(&f).unwrap();
        let t: Vec<(&str, Option<&str>)> = l
            .ops
            .iter()
            .map(|o| (o.op.ticker.as_str(), o.op.ticker2.as_deref()))
            .collect();
        assert_eq!(t, vec![("SOL", None), ("FLR", None), ("SOL", Some("SGB"))]);
        // Hoja de saldos de CoinTracking.
        let f = filas(
            "Fecha;SOL2 Valor en EUR;SOL2 Cantidad;FLR2 Valor en EUR;FLR2 Cantidad\n\
             31.12.2025;1000;10;3;150",
        );
        let cols = cols_saldos(&f, 0);
        let t: Vec<&str> = cols.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(t, vec!["SOL", "FLR"]);
    }

    fn filas(csv: &str) -> Vec<Vec<String>> {
        csv.lines()
            .map(|l| l.split(';').map(str::to_owned).collect())
            .collect()
    }

    #[test]
    fn trade_list_de_cointracking() {
        let f = filas(
            "Type;Buy Amount;Buy Currency;Sell Amount;Sell Currency;Fee;Fee Currency;Exchange;Trade Group;Comment;Date\n\
             Trade;0.5;BTC;20000;EUR;10;EUR;Kraken;;compra;10.01.2026 10:00:00\n\
             Trade;10;ETH;0.25;BTC;0.0005;BTC;Kraken;;;15.02.2026 11:00:00\n\
             Withdrawal;;;0.1;BTC;0.0001;BTC;Kraken;;;20.02.2026 12:00:00\n\
             Deposit;0.1;BTC;;;;;Ledger;;;20.02.2026 13:00:00\n\
             Staking;2;DOT;;;;;Kraken;;;01.03.2026 00:00:00\n\
             Trade;30000;EUR;0.3;BTC;15;EUR;Kraken;;;01.04.2026 09:30:00\n\
             Deposit;500;EUR;;;;;Kraken;;;02.04.2026 09:30:00",
        );
        let l = leer_hoja_cualquiera(&f).unwrap();
        assert_eq!(l.kind, "cointracking");
        let tipos: Vec<Tipo> = l.ops.iter().map(|o| o.op.kind).collect();
        assert_eq!(
            tipos,
            vec![
                Tipo::Compra,
                Tipo::Permuta,
                Tipo::Comision,
                Tipo::Traspaso,
                Tipo::Comision,
                Tipo::Traspaso,
                Tipo::Recompensa,
                Tipo::Venta
            ]
        );
        let compra = &l.ops[0].op;
        assert_eq!(
            (
                compra.ticker.as_str(),
                compra.qty,
                compra.eur,
                compra.fee_eur
            ),
            (
                "BTC",
                Qty::parse("0.5").unwrap(),
                Money::from_euros(20_000, 0),
                Money::from_euros(10, 0)
            )
        );
        let permuta = &l.ops[1].op;
        assert_eq!(permuta.ticker, "BTC");
        assert_eq!(permuta.ticker2.as_deref(), Some("ETH"));
        assert_eq!(permuta.qty2, Qty::parse("10"));
        let retirada = &l.ops[3];
        assert_eq!(
            (retirada.platform.as_str(), retirada.to_platform.as_deref()),
            ("Kraken", Some(EXTERNO))
        );
        let deposito = &l.ops[5];
        assert_eq!(
            (deposito.platform.as_str(), deposito.to_platform.as_deref()),
            (EXTERNO, Some("Ledger"))
        );
        let venta = &l.ops[7].op;
        assert_eq!(
            (venta.qty, venta.eur, venta.fee_eur),
            (
                Qty::parse("0.3").unwrap(),
                Money::from_euros(30_000, 0),
                Money::from_euros(15, 0)
            )
        );
        assert!(l.ops.iter().all(|o| o.op.fingerprint.is_some()));
        assert!(
            l.warnings
                .iter()
                .any(|w| w.contains("no traen valor en euros"))
        );
    }

    /// Una comisión en la misma cripto que no se puede descontar (igual o mayor que lo
    /// comprado) no se pierde sin más: la fila queda como en el fichero y sale un aviso.
    #[test]
    fn comision_en_cripto_que_no_se_puede_descontar_avisa() {
        let f = filas(
            "Type;Buy Amount;Buy Currency;Sell Amount;Sell Currency;Fee;Fee Currency;Exchange;Trade Group;Comment;Date\n\
             Trade;0.001;BTC;40;EUR;0.001;BTC;Kraken;;;10.01.2026 10:00:00\n\
             Trade;0.5;BTC;20000;EUR;0.0008;BTC;Kraken;;;11.01.2026 10:00:00",
        );
        let l = leer_hoja_cualquiera(&f).unwrap();
        let mala = &l.ops[0].op;
        assert_eq!(
            (mala.qty, mala.eur, mala.fee_eur),
            (
                Qty::parse("0.001").unwrap(),
                Money::from_euros(40, 0),
                Money::ZERO
            )
        );
        let avisos: Vec<&String> = l
            .warnings
            .iter()
            .filter(|w| w.contains("no se ha podido descontar"))
            .collect();
        assert_eq!(avisos.len(), 1, "{:?}", l.warnings);
        assert!(avisos[0].starts_with("fila 2:"), "{}", avisos[0]);
        // La buena sí la descuenta.
        assert_eq!(l.ops[1].op.qty, Qty::parse("0.4992").unwrap());
        // Y la función de compra avisa con `false` sin tocar nada.
        let mut op = mala.clone();
        assert!(!fee_cripto_en_compra(&mut op, Qty::parse("0.002").unwrap()));
        assert_eq!(&op, mala);
    }

    #[test]
    fn ledgers_de_kraken() {
        let f = filas(
            "txid;refid;time;type;subtype;aclass;asset;amount;fee;balance\n\
             L1;T1;2026-01-10 10:00:00.0000;trade;;currency;ZEUR;-20000.00;0.00;0\n\
             L2;T1;2026-01-10 10:00:00.0000;trade;;currency;XXBT;0.5000000000;0.0008000000;0.5\n\
             L3;T2;2026-02-01 09:00:00.0000;trade;;currency;XXBT;-0.2000000000;0;0.3\n\
             L4;T2;2026-02-01 09:00:00.0000;trade;;currency;XETH;4.0000000000;0.0100000000;4\n\
             L5;W1;2026-03-01 09:00:00.0000;withdrawal;;currency;XXBT;-0.1000000000;0.0000500000;0.2\n\
             L6;S1;2026-03-05 09:00:00.0000;staking;;currency;DOT.S;1.5;0;1.5\n\
             L7;D1;2026-03-06 09:00:00.0000;deposit;;currency;ZEUR;1000;0;1000\n\
             L8;E1;2026-03-07 09:00:00.0000;earn;allocation;currency;SOL;2;0;2\n\
             L9;E2;2026-03-08 09:00:00.0000;earn;migration;currency;SOL;2;0;2\n\
             L10;E3;2026-03-09 09:00:00.0000;earn;reward;currency;SOL;0.01;0;2.01",
        );
        let l = leer_hoja_cualquiera(&f).unwrap();
        assert_eq!(l.kind, "kraken");
        let compra = l.ops.iter().find(|o| o.op.kind == Tipo::Compra).unwrap();
        assert_eq!(compra.op.ticker, "BTC");
        // Comisión de 0,0008 BTC al precio implícito (40.000 €/BTC) = 32 €: llegan 0,4992 BTC
        // y el coste total sigue siendo 20.000 € (19.968 + 32).
        assert_eq!(compra.op.qty, Qty::parse("0.4992").unwrap());
        assert_eq!(compra.op.eur, Money::from_euros(19_968, 0));
        assert_eq!(compra.op.fee_eur, Money::from_euros(32, 0));
        let permuta = l.ops.iter().find(|o| o.op.kind == Tipo::Permuta).unwrap();
        assert_eq!(
            (permuta.op.ticker.as_str(), permuta.op.ticker2.as_deref()),
            ("BTC", Some("ETH"))
        );
        // La comisión de 0,01 ETH sale de lo recibido.
        assert_eq!(permuta.op.qty2, Qty::parse("3.99"));
        let traspaso = l.ops.iter().find(|o| o.op.kind == Tipo::Traspaso).unwrap();
        assert_eq!(traspaso.to_platform.as_deref(), Some(EXTERNO));
        assert!(
            l.ops
                .iter()
                .any(|o| o.op.kind == Tipo::Comision && o.op.qty == Qty::parse("0.00005").unwrap())
        );
        let rec = l
            .ops
            .iter()
            .find(|o| o.op.kind == Tipo::Recompensa)
            .unwrap();
        assert_eq!(rec.op.ticker, "DOT");
        // De «earn», sólo la recompensa (la asignación y la migración no son ingresos).
        let sol: Vec<_> = l
            .ops
            .iter()
            .filter(|o| o.op.kind == Tipo::Recompensa && o.op.ticker == "SOL")
            .collect();
        assert_eq!(sol.len(), 1);
        assert_eq!(sol[0].op.qty, Qty::parse("0.01").unwrap());
        assert_eq!(l.ops.len(), 6);
    }

    #[test]
    fn kraken_en_hora_de_madrid() {
        let f = filas(
            "txid;refid;time;type;subtype;aclass;asset;amount;fee;balance\n\
             L1;S1;2025-12-31 23:30:00.0000;staking;;currency;DOT.S;1.5;0;1.5\n\
             L2;S2;2026-07-10 22:15:00.0000;staking;;currency;DOT.S;1;0;2.5",
        );
        let l = leer_hoja_cualquiera(&f).unwrap();
        let mut ops: Vec<_> = l
            .ops
            .iter()
            .map(|o| (o.op.date, o.op.time.as_str()))
            .collect();
        ops.sort();
        assert_eq!(
            ops,
            [
                (time::macros::date!(2026 - 01 - 01), "00:30:00"),
                (time::macros::date!(2026 - 07 - 11), "00:15:00"),
            ]
        );
    }

    #[test]
    fn tipos_de_cointracking_en_espanol() {
        assert_eq!(tipo_cointracking("Ingreso"), Some("income"));
        assert_eq!(tipo_cointracking("Ingresos por intereses"), Some("income"));
        assert_eq!(tipo_cointracking("Depósito"), Some("deposit"));
        assert_eq!(tipo_cointracking("Deposit"), Some("deposit"));
        assert_eq!(tipo_cointracking("Income"), Some("income"));
    }

    #[test]
    fn hoja_generica_y_saldos() {
        let f = filas(
            "Fecha;Tipo;Activo;Cantidad;Euros;Comisión;Plataforma;Nota\n\
             10/01/2026;Compra;btc;0,5;20.000,00;10,00;Bit2Me;primera\n\
             15/06/2026;Venta;BTC;0,2;12.000,00;;Bit2Me;",
        );
        let l = leer_hoja_cualquiera(&f).unwrap();
        assert_eq!(l.kind, "hoja");
        assert_eq!(l.ops.len(), 2);
        assert_eq!(l.ops[0].op.ticker, "BTC");
        assert_eq!(l.ops[0].op.eur, Money::from_euros(20_000, 0));
        assert_eq!(l.ops[1].op.kind, Tipo::Venta);
        assert_eq!(l.ops[1].platform, "Bit2Me");

        let s = filas(
            ";BTC;;ETH;\n\
             Fecha;Valor en EUR;Cantidad;Valor en EUR;Cantidad\n\
             31/12/2025;45.000,00;0,5;4.000,00;2\n\
             31/12/2026;60.000,00;0,5;;0",
        );
        let l = leer_hoja_cualquiera(&s).unwrap();
        assert_eq!(l.kind, "saldos");
        assert_eq!(l.prices.len(), 3);
        assert_eq!(l.prices[0].ticker, "BTC");
        assert_eq!(l.prices[0].price, Precio::from_euros(90_000));
        assert_eq!(l.prices[1].price, Precio::from_euros(2_000));
        assert_eq!(l.prices[2].date, date!(2026 - 12 - 31));
    }

    #[test]
    fn plataformas_conocidas() {
        assert_eq!(pais_de_plataforma("Kraken"), ("US".into(), true));
        assert_eq!(pais_de_plataforma("Bit2Me"), ("ES".into(), false));
        assert_eq!(pais_de_plataforma("Mi Ledger"), ("ES".into(), false));
    }
}

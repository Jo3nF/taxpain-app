//! Diccionario de los títulos de las columnas y de las etiquetas de los datos de la cuenta, en
//! español, catalán, euskera, gallego, inglés, francés, alemán, italiano, portugués y
//! neerlandés. Las claves van como las deja [`super::texto::clave`]: en minúsculas, sin tildes y
//! con un espacio entre palabras («F. Operación» → «f operacion»).

use super::texto::{clave, palabras};

/// Qué es una columna de la tabla de movimientos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Campo {
    /// Fecha sin más («Fecha», «Date», «Datum»).
    Fecha,
    /// Fecha de la operación («F. operación», «Transaction date», «Started date»).
    FechaOperacion,
    /// Fecha contable o de cargo («F. contable», «Booking date», «Buchungstag»,
    /// «Completed date»): la que sigue el saldo.
    FechaContable,
    FechaValor,
    /// «Valor» o «Valuta»: fecha valor, importe o divisa, según lo que traiga la columna.
    Valor,
    Concepto,
    /// Importe con signo.
    Importe,
    /// «Net»/«Neto»: el importe ya sin las comisiones (PayPal, Stripe).
    ImporteNeto,
    /// «Gross»/«Bruto»: antes de las comisiones; sólo si no hay otro.
    ImporteBruto,
    Cargo,
    Abono,
    Saldo,
    /// «Saldo disponible»: sólo si no hay otro saldo.
    SaldoDisponible,
    Divisa,
    Referencia,
    /// Signo del importe: «D/H», «+/-», «Debit/Credit», «Af Bij».
    Signo,
    /// Comisión aparte (Revolut, Wise…).
    Comision,
    /// La divisa de la comisión («Fee currency»): si no es la del movimiento, la comisión no se
    /// resta del importe.
    DivisaComision,
    /// Estado («Completed», «Pending», «Declined»).
    Estado,
    /// Tipo de operación o código: no va al concepto, pero puede llevar el signo.
    Tipo,
    /// La cuenta propia, repetida en cada fila (bunq, ING de los Países Bajos…); si cambia,
    /// el fichero trae varias cuentas.
    Cuenta,
    /// La divisa de origen de la operación («Orig currency» de Revolut Business, «Original
    /// currency»): si es el euro, su importe es el contravalor de una cuenta en otra divisa.
    DivisaOrigen,
    /// El importe en la divisa de origen («Orig amount», «Importe original»).
    ImporteOrigen,
    /// De qué divisa se cambió («Exchange From» de Wise).
    CambioDe,
    /// A qué divisa se cambió («Exchange To» de Wise).
    CambioA,
    /// Lo que salió en la divisa a la que se cambió («Exchange To Amount» de Wise).
    ImporteCambioA,
    /// Tipo de cambio («Exchange Rate»): unidades de la divisa de llegada por una de la de
    /// salida.
    TipoCambio,
    /// Contravalor en euros que da el banco («Contravalor», «Contravalor EUR»).
    Contravalor,
    /// Columnas que no hay que confundir con otra (oficina, categoría, IBAN de la otra parte…).
    Ignorar,
    /// Columnas propias de un libro de contabilidad (asiento, apunte, contrapartida…): una
    /// tabla con ellas es un diario o un mayor, no un extracto del banco.
    Libro,
}

use Campo::*;

/// Títulos de columna conocidos.
const TITULOS: &[(&str, Campo)] = &[
    // Fechas.
    ("fecha", Fecha),
    ("fechas", Fecha),
    ("date", Fecha),
    ("datum", Fecha),
    ("data", Fecha),
    ("dia", Fecha),
    ("day", Fecha),
    ("fec", Fecha),
    ("fecha movimiento", Fecha),
    ("fecha mov", Fecha),
    ("fecha apunte", Fecha),
    ("fecha registro", Fecha),
    ("fecha anotacion", Fecha),
    ("fecha y hora", Fecha),
    ("date time", Fecha),
    ("datetime", Fecha),
    ("date utc", Fecha),
    ("created", Fecha),
    ("created utc", Fecha),
    ("data moviment", Fecha),
    ("data apunt", Fecha),
    ("data movimento", Fecha),
    ("data movemento", Fecha),
    ("data movimiento", Fecha),
    ("date mouvement", Fecha),
    ("data mov", Fecha),
    ("mugimendu data", Fecha),
    ("fecha operacion", FechaOperacion),
    ("fecha de inicio", FechaOperacion),
    ("fecha inicio", FechaOperacion),
    ("f operacion", FechaOperacion),
    ("fecha operativa", FechaOperacion),
    ("f operativa", FechaOperacion),
    ("fecha oper", FechaOperacion),
    ("f oper", FechaOperacion),
    ("fec oper", FechaOperacion),
    ("f op", FechaOperacion),
    ("fecha op", FechaOperacion),
    ("fecha transaccion", FechaOperacion),
    ("fecha compra", FechaOperacion),
    ("fecha consumo", FechaOperacion),
    ("data operacio", FechaOperacion),
    ("data operacion", FechaOperacion),
    ("data transaccio", FechaOperacion),
    ("eragiketa data", FechaOperacion),
    ("data eragiketa", FechaOperacion),
    ("transaction date", FechaOperacion),
    ("trans date", FechaOperacion),
    ("txn date", FechaOperacion),
    ("operation date", FechaOperacion),
    ("operation date utc", FechaOperacion),
    ("started date", FechaOperacion),
    ("date started", FechaOperacion),
    ("date started utc", FechaOperacion),
    ("purchase date", FechaOperacion),
    ("date operation", FechaOperacion),
    ("date transaction", FechaOperacion),
    ("transaktionsdatum", FechaOperacion),
    ("umsatzdatum", FechaOperacion),
    ("belegdatum", FechaOperacion),
    ("data operazione", FechaOperacion),
    ("data operacao", FechaOperacion),
    ("transactiedatum", FechaOperacion),
    ("datum transactie", FechaOperacion),
    ("fecha contable", FechaContable),
    ("fecha de finalizacion", FechaContable),
    ("fecha finalizacion", FechaContable),
    ("f contable", FechaContable),
    ("fecha ctble", FechaContable),
    ("f ctble", FechaContable),
    ("fecha contabilizacion", FechaContable),
    ("fecha cargo", FechaContable),
    ("fecha proceso", FechaContable),
    ("fecha liquidacion", FechaContable),
    ("fecha recepcion", FechaContable),
    ("f recepcion", FechaContable),
    ("data comptable", FechaContable),
    ("data contable", FechaContable),
    ("kontabilitate data", FechaContable),
    ("booking date", FechaContable),
    ("posting date", FechaContable),
    ("posted date", FechaContable),
    ("date posted", FechaContable),
    ("entry date", FechaContable),
    ("book date", FechaContable),
    ("accounting date", FechaContable),
    ("processing date", FechaContable),
    ("completed date", FechaContable),
    ("date completed", FechaContable),
    ("date completed utc", FechaContable),
    ("completed date utc", FechaContable),
    ("settlement date", FechaContable),
    ("settlement date utc", FechaContable),
    ("date comptable", FechaContable),
    ("date comptabilisation", FechaContable),
    ("buchungstag", FechaContable),
    ("buchungsdatum", FechaContable),
    ("buchung", FechaContable),
    ("data contabile", FechaContable),
    ("data registrazione", FechaContable),
    ("data contabilizzazione", FechaContable),
    ("data lancamento", FechaContable),
    ("data contabilistica", FechaContable),
    ("boekdatum", FechaContable),
    ("boekingsdatum", FechaContable),
    ("fecha valor", FechaValor),
    ("f valor", FechaValor),
    ("fec valor", FechaValor),
    ("f val", FechaValor),
    ("fecha valoracion", FechaValor),
    ("f valoracion", FechaValor),
    ("data valor", FechaValor),
    ("value date", FechaValor),
    ("val date", FechaValor),
    ("valutadatum", FechaValor),
    ("wertstellung", FechaValor),
    ("wertstellungsdatum", FechaValor),
    ("wert", FechaValor),
    ("date valeur", FechaValor),
    ("data valuta", FechaValor),
    ("rentedatum", FechaValor),
    ("balio data", FechaValor),
    ("effective date", FechaValor),
    ("valor", Valor),
    ("value", Valor),
    ("valeur", Valor),
    ("valuta", Valor),
    // Concepto, detalle y la otra parte.
    ("concepto", Concepto),
    ("conceptos", Concepto),
    ("concept", Concepto),
    ("label", Concepto),
    ("comment", Concepto),
    ("nombre", Concepto),
    ("concepto movimiento", Concepto),
    ("concepto operacion", Concepto),
    ("concepto ampliado", Concepto),
    ("concepto propio", Concepto),
    ("concepto ordenante", Concepto),
    ("concepto beneficiario", Concepto),
    ("descripcion", Concepto),
    ("descripcion ampliada", Concepto),
    ("descripcion movimiento", Concepto),
    ("descripcion operacion", Concepto),
    ("detalle", Concepto),
    ("detalles", Concepto),
    ("detalle movimiento", Concepto),
    ("movimiento", Concepto),
    ("operacion", Concepto),
    ("observaciones", Concepto),
    ("observacion", Concepto),
    ("texto", Concepto),
    ("mas datos", Concepto),
    ("informacion adicional", Concepto),
    ("info adicional", Concepto),
    ("glosa", Concepto),
    ("motivo", Concepto),
    ("literal", Concepto),
    ("comentario", Concepto),
    ("comentarios", Concepto),
    ("beneficiario", Concepto),
    ("ordenante", Concepto),
    ("beneficiario ordenante", Concepto),
    ("ordenante beneficiario", Concepto),
    ("contraparte", Concepto),
    ("destinatario", Concepto),
    ("remitente", Concepto),
    ("emisor", Concepto),
    ("comercio", Concepto),
    ("establecimiento", Concepto),
    ("establecimiento cajero", Concepto),
    ("concepte", Concepto),
    ("descripcio", Concepto),
    ("detall", Concepto),
    ("moviment", Concepto),
    ("observacions", Concepto),
    ("mes dades", Concepto),
    ("beneficiari", Concepto),
    ("ordenant", Concepto),
    ("kontzeptua", Concepto),
    ("deskribapena", Concepto),
    ("azalpena", Concepto),
    ("oharrak", Concepto),
    ("descricion", Concepto),
    ("movemento", Concepto),
    ("description", Concepto),
    ("details", Concepto),
    ("transaction details", Concepto),
    ("transaction description", Concepto),
    ("narrative", Concepto),
    ("memo", Concepto),
    ("particulars", Concepto),
    ("text", Concepto),
    ("payment details", Concepto),
    ("payment reference", Concepto),
    ("remittance information", Concepto),
    ("purpose", Concepto),
    ("notes", Concepto),
    ("note", Concepto),
    ("payee", Concepto),
    ("payer", Concepto),
    ("payee name", Concepto),
    ("payer name", Concepto),
    ("counterparty", Concepto),
    ("counterparty name", Concepto),
    ("beneficiary", Concepto),
    ("recipient", Concepto),
    ("sender", Concepto),
    ("merchant", Concepto),
    ("merchant name", Concepto),
    ("name", Concepto),
    ("partner name", Concepto),
    ("libelle", Concepto),
    ("libelle operation", Concepto),
    ("libelle de l operation", Concepto),
    ("libelle simplifie", Concepto),
    ("detail", Concepto),
    ("nature de l operation", Concepto),
    ("motif", Concepto),
    ("communication", Concepto),
    ("intitule", Concepto),
    ("beneficiaire", Concepto),
    ("contrepartie", Concepto),
    ("tiers", Concepto),
    ("verwendungszweck", Concepto),
    ("buchungstext", Concepto),
    ("beschreibung", Concepto),
    ("vorgang", Concepto),
    ("mitteilung", Concepto),
    ("zahlungsgrund", Concepto),
    ("auftraggeber", Concepto),
    ("empfanger", Concepto),
    ("empfaenger", Concepto),
    ("zahlungsempfanger", Concepto),
    ("auftraggeber empfanger", Concepto),
    ("auftraggeber begunstigter", Concepto),
    ("begunstigter", Concepto),
    ("name zahlungsbeteiligter", Concepto),
    ("descrizione", Concepto),
    ("causale", Concepto),
    ("descrizione operazione", Concepto),
    ("dettagli", Concepto),
    ("operazione", Concepto),
    ("ordinante", Concepto),
    ("controparte", Concepto),
    ("esercente", Concepto),
    ("descricao", Concepto),
    ("descritivo", Concepto),
    ("historico", Concepto),
    ("designacao", Concepto),
    ("detalhes", Concepto),
    ("observacoes", Concepto),
    ("omschrijving", Concepto),
    ("mededelingen", Concepto),
    ("mededeling", Concepto),
    ("beschrijving", Concepto),
    ("naam omschrijving", Concepto),
    ("naam tegenpartij", Concepto),
    ("tegenpartij", Concepto),
    ("naam", Concepto),
    // Importes.
    ("importe", Importe),
    ("importes", Importe),
    ("importe movimiento", Importe),
    ("importe operacion", Importe),
    ("imp", Importe),
    ("cantidad", Importe),
    ("monto", Importe),
    ("euros", Importe),
    ("import", Importe),
    ("import moviment", Importe),
    ("quantitat", Importe),
    ("zenbatekoa", Importe),
    ("kopurua", Importe),
    ("cantidade", Importe),
    ("amount", Importe),
    ("transaction amount", Importe),
    ("total amount", Importe),
    ("total amount incl vat", Importe),
    ("sum", Importe),
    ("montant", Importe),
    ("montant operation", Importe),
    ("somme", Importe),
    ("betrag", Importe),
    ("umsatz", Importe),
    ("summe", Importe),
    ("importo", Importe),
    ("ammontare", Importe),
    ("somma", Importe),
    ("montante", Importe),
    ("quantia", Importe),
    ("importancia", Importe),
    ("bedrag", Importe),
    ("transactiebedrag", Importe),
    ("net", ImporteNeto),
    ("neto", ImporteNeto),
    ("importe neto", ImporteNeto),
    ("net amount", ImporteNeto),
    ("netto", ImporteNeto),
    ("nettobetrag", ImporteNeto),
    ("montant net", ImporteNeto),
    ("gross", ImporteBruto),
    ("bruto", ImporteBruto),
    ("importe bruto", ImporteBruto),
    ("gross amount", ImporteBruto),
    ("brutto", ImporteBruto),
    ("montant brut", ImporteBruto),
    ("cargo", Cargo),
    ("cargos", Cargo),
    ("debe", Cargo),
    ("debito", Cargo),
    ("debitos", Cargo),
    ("salida", Cargo),
    ("salidas", Cargo),
    ("pagos", Cargo),
    ("reintegro", Cargo),
    ("reintegros", Cargo),
    ("importe cargo", Cargo),
    ("importe debe", Cargo),
    ("importe debito", Cargo),
    ("disposicion", Cargo),
    ("disposiciones", Cargo),
    ("carrec", Cargo),
    ("carrecs", Cargo),
    ("deure", Cargo),
    ("sortida", Cargo),
    ("sortides", Cargo),
    ("pagaments", Cargo),
    ("zorra", Cargo),
    ("kargua", Cargo),
    ("irteera", Cargo),
    ("saida", Cargo),
    ("saidas", Cargo),
    ("debit", Cargo),
    ("debits", Cargo),
    ("debit amount", Cargo),
    ("withdrawal", Cargo),
    ("withdrawals", Cargo),
    ("paid out", Cargo),
    ("money out", Cargo),
    ("out", Cargo),
    ("outflow", Cargo),
    ("payments", Cargo),
    ("dr", Cargo),
    ("charges", Cargo),
    ("spent", Cargo),
    ("amount out", Cargo),
    ("sortie", Cargo),
    ("sorties", Cargo),
    ("montant debit", Cargo),
    ("retrait", Cargo),
    ("soll", Cargo),
    ("belastung", Cargo),
    ("ausgang", Cargo),
    ("ausgaben", Cargo),
    ("abgang", Cargo),
    ("betrag soll", Cargo),
    ("dare", Cargo),
    ("addebiti", Cargo),
    ("addebito", Cargo),
    ("uscite", Cargo),
    ("uscita", Cargo),
    ("importo dare", Cargo),
    ("levantamento", Cargo),
    ("af", Cargo),
    ("debet", Cargo),
    ("uitgaven", Cargo),
    ("afschrijving", Cargo),
    ("abono", Abono),
    ("abonos", Abono),
    ("haber", Abono),
    ("credito", Abono),
    ("creditos", Abono),
    ("ingreso", Abono),
    ("ingresos", Abono),
    ("entrada", Abono),
    ("entradas", Abono),
    ("importe abono", Abono),
    ("importe haber", Abono),
    ("importe credito", Abono),
    ("abonament", Abono),
    ("abonaments", Abono),
    ("haver", Abono),
    ("ingres", Abono),
    ("ingressos", Abono),
    ("entrades", Abono),
    ("hartzekoa", Abono),
    ("sarrera", Abono),
    ("sarrerak", Abono),
    ("credit", Abono),
    ("credits", Abono),
    ("credit amount", Abono),
    ("deposit", Abono),
    ("deposits", Abono),
    ("paid in", Abono),
    ("money in", Abono),
    ("in", Abono),
    ("inflow", Abono),
    ("receipts", Abono),
    ("cr", Abono),
    ("received", Abono),
    ("amount in", Abono),
    ("entree", Abono),
    ("entrees", Abono),
    ("montant credit", Abono),
    ("versement", Abono),
    ("haben", Abono),
    ("gutschrift", Abono),
    ("eingang", Abono),
    ("einnahmen", Abono),
    ("zugang", Abono),
    ("betrag haben", Abono),
    ("avere", Abono),
    ("accrediti", Abono),
    ("accredito", Abono),
    ("entrate", Abono),
    ("entrata", Abono),
    ("importo avere", Abono),
    ("deposito", Abono),
    ("bij", Abono),
    ("inkomsten", Abono),
    ("bijschrijving", Abono),
    // Saldo.
    ("saldo", Saldo),
    ("saldos", Saldo),
    ("saldo contable", Saldo),
    ("saldo tras movimiento", Saldo),
    ("saldo resultante", Saldo),
    ("saldo final", Saldo),
    ("saldo actual", Saldo),
    ("saldo comptable", Saldo),
    ("saldoa", Saldo),
    ("balance", Saldo),
    ("running balance", Saldo),
    ("running bal", Saldo),
    ("bal", Saldo),
    ("book balance", Saldo),
    ("ledger balance", Saldo),
    ("balance after", Saldo),
    ("account balance", Saldo),
    ("solde", Saldo),
    ("solde comptable", Saldo),
    ("solde apres operation", Saldo),
    ("kontostand", Saldo),
    ("saldo nach buchung", Saldo),
    ("kontosaldo", Saldo),
    ("stand", Saldo),
    ("saldo contabile", Saldo),
    ("saldo contabilistico", Saldo),
    ("saldo apos movimento", Saldo),
    ("saldo na mutatie", Saldo),
    ("saldo na boeking", Saldo),
    ("saldo disponible", SaldoDisponible),
    ("disponible", SaldoDisponible),
    ("available balance", SaldoDisponible),
    ("available", SaldoDisponible),
    ("solde disponible", SaldoDisponible),
    ("saldo disponibile", SaldoDisponible),
    ("saldo disponivel", SaldoDisponible),
    ("saldo erabilgarria", SaldoDisponible),
    // Divisa.
    ("divisa", Divisa),
    ("moneda", Divisa),
    ("div", Divisa),
    ("mon", Divisa),
    ("currency", Divisa),
    ("ccy", Divisa),
    ("cur", Divisa),
    ("curr", Divisa),
    ("currency code", Divisa),
    ("devise", Divisa),
    ("monnaie", Divisa),
    ("wahrung", Divisa),
    ("waehrung", Divisa),
    ("whrg", Divisa),
    ("moeda", Divisa),
    ("munt", Divisa),
    ("muntsoort", Divisa),
    ("dibisa", Divisa),
    ("txanpona", Divisa),
    ("moneta", Divisa),
    // Referencia.
    ("referencia", Referencia),
    ("referencias", Referencia),
    ("ref", Referencia),
    ("referencia 1", Referencia),
    ("referencia 2", Referencia),
    ("num documento", Referencia),
    ("n documento", Referencia),
    ("numero documento", Referencia),
    ("documento", Referencia),
    ("numero operacion", Referencia),
    ("n operacion", Referencia),
    ("num operacion", Referencia),
    ("justificante", Referencia),
    ("num referencia", Referencia),
    ("codigo operacion", Referencia),
    ("id operacion", Referencia),
    ("remesa", Referencia),
    ("num document", Referencia),
    ("document", Referencia),
    ("erreferentzia", Referencia),
    ("reference", Referencia),
    ("reference number", Referencia),
    ("transaction id", Referencia),
    ("id de transaccion", Referencia),
    ("id transaccion", Referencia),
    ("transaction reference", Referencia),
    ("id", Referencia),
    ("txn id", Referencia),
    ("receipt id", Referencia),
    ("end to end id", Referencia),
    ("invoice id", Referencia),
    ("order id", Referencia),
    ("check number", Referencia),
    ("cheque number", Referencia),
    ("transferwise id", Referencia),
    ("numero de piece", Referencia),
    ("n piece", Referencia),
    ("piece", Referencia),
    ("referenz", Referencia),
    ("belegnummer", Referencia),
    ("mandatsreferenz", Referencia),
    ("kundenreferenz", Referencia),
    ("riferimento", Referencia),
    ("rif", Referencia),
    ("numero operazione", Referencia),
    ("cro", Referencia),
    ("referentie", Referencia),
    ("kenmerk", Referencia),
    ("betalingskenmerk", Referencia),
    ("volgnummer", Referencia),
    ("volgnr", Referencia),
    // Signo.
    ("d h", Signo),
    ("dh", Signo),
    ("debe haber", Signo),
    ("haber debe", Signo),
    ("signo", Signo),
    ("sign", Signo),
    ("sens", Signo),
    ("segno", Signo),
    ("af bij", Signo),
    ("cr dr", Signo),
    ("dr cr", Signo),
    ("debit credit", Signo),
    ("credit debit", Signo),
    ("debito credito", Signo),
    ("credito debito", Signo),
    ("cargo abono", Signo),
    ("abono cargo", Signo),
    ("soll haben", Signo),
    ("s h", Signo),
    ("d c", Signo),
    ("c d", Signo),
    ("c a", Signo),
    ("dc", Signo),
    ("cd", Signo),
    ("debit credit indicator", Signo),
    ("direction", Signo),
    ("direccion", Signo),
    ("credit debit indicator", Signo),
    // Comisión, estado y tipo.
    ("comision", Comision),
    ("comisiones", Comision),
    ("fee", Comision),
    ("fee currency", DivisaComision),
    ("fees currency", DivisaComision),
    ("commission currency", DivisaComision),
    ("divisa comision", DivisaComision),
    ("moneda comision", DivisaComision),
    ("tarifa", Comision),
    ("tarifas", Comision),
    ("fees", Comision),
    ("total fees", Comision),
    ("commission", Comision),
    ("comissao", Comision),
    ("frais", Comision),
    ("gebuhr", Comision),
    ("gebuehr", Comision),
    ("gebuhren", Comision),
    ("commissione", Comision),
    ("estado", Estado),
    ("status", Estado),
    ("state", Estado),
    ("situacion", Estado),
    ("estat", Estado),
    ("statut", Estado),
    ("etat", Estado),
    ("stato", Estado),
    ("balance impact", Estado),
    ("impacto en el saldo", Estado),
    ("tipo", Tipo),
    ("type", Tipo),
    ("tipo movimiento", Tipo),
    ("tipo de movimiento", Tipo),
    ("tipo operacion", Tipo),
    ("tipo de operacion", Tipo),
    ("transaction type", Tipo),
    ("tx type", Tipo),
    ("kind", Tipo),
    ("clase", Tipo),
    ("naturaleza", Tipo),
    ("tipus", Tipo),
    ("tipus operacio", Tipo),
    ("mota", Tipo),
    ("eragiketa mota", Tipo),
    ("type d operation", Tipo),
    ("type operation", Tipo),
    ("buchungsart", Tipo),
    ("umsatzart", Tipo),
    ("vorgangsart", Tipo),
    ("transactiesoort", Tipo),
    ("mutatiesoort", Tipo),
    ("codigo", Tipo),
    ("cod", Tipo),
    ("cod concepto", Tipo),
    ("codigo concepto", Tipo),
    ("concepto comun", Tipo),
    ("c comun", Tipo),
    ("code", Tipo),
    ("transaction code", Tipo),
    ("codice", Tipo),
    ("codi", Tipo),
    ("kodea", Tipo),
    ("product", Tipo),
    ("producto", Tipo),
    ("payment method", Tipo),
    ("metodo de pago", Tipo),
    // Libros de contabilidad.
    ("asiento", Libro),
    ("asientos", Libro),
    ("n asiento", Libro),
    ("num asiento", Libro),
    ("numero asiento", Libro),
    ("numero de asiento", Libro),
    ("apunte", Libro),
    ("n apunte", Libro),
    ("contrapartida", Libro),
    ("descripcion de la cuenta", Libro),
    ("descripcion cuenta", Libro),
    ("nombre de la cuenta", Libro),
    ("nombre cuenta", Libro),
    ("titulo cuenta", Libro),
    ("titulo de la cuenta", Libro),
    ("subcuenta", Libro),
    ("titulo subcuenta", Libro),
    ("cuenta contable", Libro),
    ("punt", Libro),
    ("punteo", Libro),
    ("assentament", Libro),
    ("journal", Libro),
    ("journal entry", Libro),
    ("entry number", Libro),
    ("ledger account", Libro),
    ("gl account", Libro),
    ("ecriture", Libro),
    ("n ecriture", Libro),
    ("numero d ecriture", Libro),
    ("compte general", Libro),
    ("sachkonto", Libro),
    ("gegenkonto nr", Libro),
    // Lo que no hay que tomar por otra cosa.
    ("oficina", Ignorar),
    ("ofic", Ignorar),
    ("sucursal", Ignorar),
    ("entidad", Ignorar),
    ("cuenta", Cuenta),
    ("numero cuenta", Cuenta),
    ("n cuenta", Cuenta),
    ("iban", Cuenta),
    ("iban bban", Cuenta),
    ("account", Cuenta),
    ("account number", Cuenta),
    ("account iban", Cuenta),
    ("account num", Cuenta),
    ("international account number", Cuenta),
    ("rekening", Cuenta),
    ("rekeningnummer", Cuenta),
    ("eigen rekening", Cuenta),
    ("kontonummer", Cuenta),
    ("konto", Cuenta),
    ("compte", Cuenta),
    ("numero de compte", Cuenta),
    ("bic", Ignorar),
    ("swift", Ignorar),
    ("sort code", Ignorar),
    ("account name", Ignorar),
    ("account label", Ignorar),
    ("accountbalance", Ignorar),
    ("partner iban", Ignorar),
    ("counterparty iban", Ignorar),
    ("counterparty account", Ignorar),
    ("payee account number", Ignorar),
    ("iban contraparte", Ignorar),
    ("iban beneficiario", Ignorar),
    ("iban ordenante", Ignorar),
    ("blz", Ignorar),
    ("iban gegenkonto", Ignorar),
    ("gegenkonto", Ignorar),
    ("tegenrekening", Ignorar),
    ("categoria", Ignorar),
    ("subcategoria", Ignorar),
    ("category", Ignorar),
    ("subcategory", Ignorar),
    ("categorie", Ignorar),
    ("kategorie", Ignorar),
    ("kategoria", Ignorar),
    ("etiqueta", Ignorar),
    ("etiquetas", Ignorar),
    ("tags", Ignorar),
    ("tag", Ignorar),
    ("labels", Ignorar),
    ("hora", Ignorar),
    ("time", Ignorar),
    ("timezone", Ignorar),
    ("time zone", Ignorar),
    ("zona horaria", Ignorar),
    ("tarjeta", Ignorar),
    ("card", Ignorar),
    ("card number", Ignorar),
    ("numero tarjeta", Ignorar),
    ("card last four digits", Ignorar),
    ("card holder full name", Ignorar),
    ("exchange rate", TipoCambio),
    ("tipo de cambio", TipoCambio),
    ("cambio", Ignorar),
    ("tax", Ignorar),
    ("vat", Ignorar),
    ("vat amount", Ignorar),
    ("iva", Ignorar),
    ("attachment", Ignorar),
    ("receipt", Ignorar),
    ("initiator", Ignorar),
    ("from email address", Ignorar),
    ("to email address", Ignorar),
    ("email", Ignorar),
    ("contravalor", Contravalor),
    ("importe divisa", Ignorar),
    ("importe original", ImporteOrigen),
    ("original amount", ImporteOrigen),
    ("orig amount", ImporteOrigen),
    ("original currency", DivisaOrigen),
    ("orig currency", DivisaOrigen),
    ("amount foreign currency", Ignorar),
    ("type foreign currency", Ignorar),
    ("foreign currency", Ignorar),
    ("exchange from", CambioDe),
    ("exchange to", CambioA),
    ("exchange to amount", ImporteCambioA),
    ("available on", Ignorar),
    ("available on utc", Ignorar),
    ("usuario", Ignorar),
    ("user", Ignorar),
];

/// Palabras de relleno que se quitan antes de comparar un título por segunda vez.
const RELLENO: [&str; 15] = [
    "de", "del", "la", "el", "of", "the", "du", "des", "der", "die", "das", "da", "do", "di", "l",
];

/// Códigos de divisa que pueden ir en el título de una columna («Importe (EUR)»).
const DIVISAS: [&str; 16] = [
    "eur", "usd", "gbp", "chf", "jpy", "cad", "aud", "sek", "nok", "dkk", "pln", "czk", "mxn",
    "huf", "ron", "bgn",
];

/// La divisa que dice un título de columna («Importe (EUR)», «Amount in USD», «Importe €»).
pub(super) fn divisa_de_titulo(titulo: &str) -> Option<String> {
    if titulo.contains('€') {
        return Some("EUR".to_owned());
    }
    let p = palabras(titulo);
    if p.len() < 2 {
        return None;
    }
    p.iter().skip(1).find_map(|w| match w.as_str() {
        "euros" | "euro" => Some("EUR".to_owned()),
        w if DIVISAS.contains(&w) => Some(w.to_uppercase()),
        _ => None,
    })
}

/// Busca la clave en la tabla.
fn exacto(k: &str) -> Option<Campo> {
    TITULOS.iter().find(|(t, _)| *t == k).map(|(_, c)| *c)
}

/// Qué es una columna por su título.
pub(super) fn campo(titulo: &str) -> Option<Campo> {
    let t = titulo.trim();
    let separado;
    let t = if !t.contains(|c: char| !c.is_ascii_alphanumeric())
        && t.bytes().any(|b| b.is_ascii_lowercase())
        && t.bytes().skip(1).any(|b| b.is_ascii_uppercase())
    {
        // Un título en camelCase («dateOp», «accountNum»): una palabra por joroba.
        let mut out = String::new();
        let mut previa_minuscula = false;
        for c in t.chars() {
            if c.is_ascii_uppercase() && previa_minuscula {
                out.push(' ');
            }
            previa_minuscula = c.is_ascii_lowercase();
            out.push(c);
        }
        separado = out;
        separado.as_str()
    } else {
        t
    };
    if matches!(t, "+/-" | "+-" | "-/+" | "+ / -") {
        return Some(Signo);
    }
    let mut p = palabras(t);
    // Sin la divisa del final («importe eur», «amount in usd», «importe en euros»).
    if p.len() >= 2
        && p.last()
            .is_some_and(|w| DIVISAS.contains(&w.as_str()) || w == "euros" || w == "euro")
    {
        p.pop();
        if p.len() >= 2 && p.last().is_some_and(|w| w == "in" || w == "en") {
            p.pop();
        }
    }
    if p.is_empty() || p.len() > 6 {
        return None;
    }
    let k = p.join(" ");
    if let Some(c) = exacto(&k) {
        return Some(c);
    }
    let sin_relleno: Vec<&str> = p
        .iter()
        .map(String::as_str)
        .filter(|w| !RELLENO.contains(w))
        .collect();
    if let Some(c) = exacto(&sin_relleno.join(" ")) {
        return Some(c);
    }
    por_raiz(&sin_relleno)
}

/// Qué es una columna por las palabras que lleva su título, si no está en la tabla.
fn por_raiz(p: &[&str]) -> Option<Campo> {
    let tiene = |ws: &[&str]| p.iter().any(|w| ws.contains(w));
    if tiene(&[
        "original",
        "foreign",
        "fremdwahrung",
        "exchange",
        "cambio",
        "contravalor",
        "iva",
        "vat",
        "tax",
        "rate",
        "orig",
        "email",
        "categoria",
        "category",
        "oficina",
    ]) {
        return Some(Ignorar);
    }
    if tiene(&[
        "iban", "cuenta", "account", "rekening", "konto", "compte", "conto", "conta",
    ]) {
        let otra_parte = tiene(&[
            "partner",
            "counterparty",
            "contraparte",
            "tegenrekening",
            "tegenpartij",
            "beneficiario",
            "ordenante",
            "payee",
            "payer",
            "gegenkonto",
            "empfanger",
            "auftraggeber",
            "recipient",
            "sender",
            "beneficiary",
            "contrepartie",
            "destino",
            "origen",
            "name",
            "nombre",
        ]);
        return Some(if otra_parte { Ignorar } else { Cuenta });
    }
    let fecha = tiene(&["fecha", "date", "datum", "data", "fec", "dia"]);
    if fecha {
        return Some(
            if tiene(&["valor", "value", "valuta", "valeur", "wertstellung", "val"]) {
                FechaValor
            } else if tiene(&[
                "contable",
                "booking",
                "posting",
                "posted",
                "buchung",
                "contabilizacion",
                "completed",
                "settlement",
                "cargo",
                "liquidacion",
                "comptable",
            ]) {
                FechaContable
            } else if tiene(&[
                "operacion",
                "transaction",
                "operation",
                "oper",
                "compra",
                "started",
                "operazione",
                "operacao",
            ]) {
                FechaOperacion
            } else {
                Fecha
            },
        );
    }
    if tiene(&["saldo", "balance", "solde", "kontostand", "saldoa"]) {
        return Some(if tiene(&["disponible", "available", "disponibile"]) {
            SaldoDisponible
        } else {
            Saldo
        });
    }
    if tiene(&[
        "importe", "amount", "betrag", "montant", "importo", "bedrag", "import", "monto",
        "montante",
    ]) {
        return Some(if tiene(&["neto", "net", "netto"]) {
            ImporteNeto
        } else if tiene(&["bruto", "gross", "brutto"]) {
            ImporteBruto
        } else if tiene(&["cargo", "debe", "debit", "debito", "soll", "out", "dare"]) {
            Cargo
        } else if tiene(&[
            "abono", "haber", "credit", "credito", "haben", "in", "avere",
        ]) {
            Abono
        } else {
            Importe
        });
    }
    if tiene(&[
        "concepto",
        "descripcion",
        "description",
        "libelle",
        "verwendungszweck",
        "descrizione",
        "omschrijving",
        "concepte",
        "descripcio",
        "detalle",
        "details",
        "observaciones",
        "beneficiario",
        "ordenante",
        "payee",
        "merchant",
        "descricao",
        "causale",
        "buchungstext",
    ]) {
        return Some(Concepto);
    }
    if tiene(&["referencia", "reference", "referenz", "riferimento"]) {
        return Some(Referencia);
    }
    if tiene(&["divisa", "moneda", "currency", "devise", "wahrung"]) {
        return Some(Divisa);
    }
    None
}

/// Etiquetas de los datos de la cuenta que van encima o debajo de la tabla.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Etiqueta {
    Cuenta,
    Tarjeta,
    TipoTarjeta,
    /// Número de contrato (el de una tarjeta de BBVA empieza por la entidad).
    Contrato,
    Titular,
    Divisa,
    Periodo,
    Desde,
    Hasta,
    /// Fecha en que se sacó el extracto.
    Generado,
    Banco,
    SaldoInicial,
    SaldoFinal,
    /// Saldo en el momento de sacar el extracto: «saldo actual» (1), «saldo» (2),
    /// «saldo disponible» (3); cuanto más bajo, más se parece al contable.
    SaldoActual(u8),
    /// Etiquetas que se reconocen para que su valor no se tome por otra cosa.
    Otra,
}

use Etiqueta as E;

/// Etiquetas conocidas.
const ETIQUETAS: &[(&str, Etiqueta)] = &[
    ("cuenta", E::Cuenta),
    ("n cuenta", E::Cuenta),
    ("n de cuenta", E::Cuenta),
    ("no cuenta", E::Cuenta),
    ("num cuenta", E::Cuenta),
    ("numero cuenta", E::Cuenta),
    ("numero de cuenta", E::Cuenta),
    ("cuenta corriente", E::Cuenta),
    ("cuenta cargo", E::Cuenta),
    ("cuenta de cargo", E::Cuenta),
    ("cuenta asociada", E::Cuenta),
    ("cuenta iban", E::Cuenta),
    ("iban", E::Cuenta),
    ("codigo iban", E::Cuenta),
    ("ccc", E::Cuenta),
    ("compte", E::Cuenta),
    ("n de compte", E::Cuenta),
    ("numero de compte", E::Cuenta),
    ("numero compte", E::Cuenta),
    ("account", E::Cuenta),
    ("account number", E::Cuenta),
    ("account no", E::Cuenta),
    ("account iban", E::Cuenta),
    ("account name", E::Cuenta),
    ("iban number", E::Cuenta),
    ("konto", E::Cuenta),
    ("kontonummer", E::Cuenta),
    ("iban konto", E::Cuenta),
    ("compte bancaire", E::Cuenta),
    ("conto", E::Cuenta),
    ("numero conto", E::Cuenta),
    ("conto corrente", E::Cuenta),
    ("conta", E::Cuenta),
    ("numero de conta", E::Cuenta),
    ("rekening", E::Cuenta),
    ("rekeningnummer", E::Cuenta),
    ("kontua", E::Cuenta),
    ("tarjeta", E::Tarjeta),
    ("n tarjeta", E::Tarjeta),
    ("n de tarjeta", E::Tarjeta),
    ("numero de tarjeta", E::Tarjeta),
    ("numero tarjeta", E::Tarjeta),
    ("targeta", E::Tarjeta),
    ("n de targeta", E::Tarjeta),
    ("card", E::Tarjeta),
    ("card number", E::Tarjeta),
    ("card no", E::Tarjeta),
    ("carte", E::Tarjeta),
    ("numero de carte", E::Tarjeta),
    ("n de carte", E::Tarjeta),
    ("karte", E::Tarjeta),
    ("kartennummer", E::Tarjeta),
    ("carta", E::Tarjeta),
    ("numero carta", E::Tarjeta),
    ("pan", E::Tarjeta),
    ("tipo de tarjeta", E::TipoTarjeta),
    ("tipo tarjeta", E::TipoTarjeta),
    ("card type", E::TipoTarjeta),
    ("type de carte", E::TipoTarjeta),
    ("kartenart", E::TipoTarjeta),
    ("modalidad", E::TipoTarjeta),
    ("contrato", E::Contrato),
    ("n contrato", E::Contrato),
    ("n de contrato", E::Contrato),
    ("numero de contrato", E::Contrato),
    ("contract", E::Contrato),
    ("contract number", E::Contrato),
    ("titular", E::Titular),
    ("titulares", E::Titular),
    ("titular de la cuenta", E::Titular),
    ("titular cuenta", E::Titular),
    ("titular de la tarjeta", E::Titular),
    ("holder", E::Titular),
    ("account holder", E::Titular),
    ("cardholder", E::Titular),
    ("card holder", E::Titular),
    ("nombre", E::Titular),
    ("nombre del titular", E::Titular),
    ("cliente", E::Titular),
    ("empresa", E::Titular),
    ("razon social", E::Titular),
    ("kontoinhaber", E::Titular),
    ("inhaber", E::Titular),
    ("karteninhaber", E::Titular),
    ("titulaire", E::Titular),
    ("titulaire du compte", E::Titular),
    ("intestatario", E::Titular),
    ("intestazione", E::Titular),
    ("titular da conta", E::Titular),
    ("rekeninghouder", E::Titular),
    ("customer", E::Titular),
    ("customer name", E::Titular),
    ("client", E::Titular),
    ("name", E::Titular),
    ("divisa", E::Divisa),
    ("moneda", E::Divisa),
    ("currency", E::Divisa),
    ("account currency", E::Divisa),
    ("divisa de la cuenta", E::Divisa),
    ("wahrung", E::Divisa),
    ("devise", E::Divisa),
    ("moeda", E::Divisa),
    ("valuta", E::Divisa),
    ("periodo", E::Periodo),
    ("periodo de consulta", E::Periodo),
    ("period", E::Periodo),
    ("statement period", E::Periodo),
    ("zeitraum", E::Periodo),
    ("periode", E::Periodo),
    ("fechas", E::Periodo),
    ("intervalo", E::Periodo),
    ("rango de fechas", E::Periodo),
    ("date range", E::Periodo),
    ("desde", E::Desde),
    ("fecha desde", E::Desde),
    ("fecha inicio", E::Desde),
    ("fecha inicial", E::Desde),
    ("from", E::Desde),
    ("start date", E::Desde),
    ("von", E::Desde),
    ("du", E::Desde),
    ("date de debut", E::Desde),
    ("hasta", E::Hasta),
    ("fecha hasta", E::Hasta),
    ("fecha fin", E::Hasta),
    ("fecha final", E::Hasta),
    ("to", E::Hasta),
    ("end date", E::Hasta),
    ("bis", E::Hasta),
    ("au", E::Hasta),
    ("date de fin", E::Hasta),
    ("fecha", E::Generado),
    ("fecha de emision", E::Generado),
    ("fecha emision", E::Generado),
    ("fecha de generacion", E::Generado),
    ("fecha de descarga", E::Generado),
    ("fecha de consulta", E::Generado),
    ("fecha consulta", E::Generado),
    ("fecha de exportacion", E::Generado),
    ("fecha del extracto", E::Generado),
    ("date", E::Generado),
    ("generated", E::Generado),
    ("generated on", E::Generado),
    ("date generated", E::Generado),
    ("export date", E::Generado),
    ("created", E::Generado),
    ("erstellt am", E::Generado),
    ("datum", E::Generado),
    ("date d edition", E::Generado),
    ("banco", E::Banco),
    ("entidad", E::Banco),
    ("bank", E::Banco),
    ("banque", E::Banco),
    ("entidad financiera", E::Banco),
    ("saldo inicial", E::SaldoInicial),
    ("saldo anterior", E::SaldoInicial),
    ("saldo de apertura", E::SaldoInicial),
    ("saldo apertura", E::SaldoInicial),
    ("saldo inicio", E::SaldoInicial),
    ("saldo al inicio", E::SaldoInicial),
    ("saldo previo", E::SaldoInicial),
    ("saldo inicial disponible", E::SaldoInicial),
    ("opening balance", E::SaldoInicial),
    ("start balance", E::SaldoInicial),
    ("starting balance", E::SaldoInicial),
    ("beginning balance", E::SaldoInicial),
    ("previous balance", E::SaldoInicial),
    ("balance brought forward", E::SaldoInicial),
    ("brought forward", E::SaldoInicial),
    ("balance forward", E::SaldoInicial),
    ("anfangssaldo", E::SaldoInicial),
    ("alter kontostand", E::SaldoInicial),
    ("alter saldo", E::SaldoInicial),
    ("solde initial", E::SaldoInicial),
    ("solde precedent", E::SaldoInicial),
    ("ancien solde", E::SaldoInicial),
    ("solde d ouverture", E::SaldoInicial),
    ("saldo iniziale", E::SaldoInicial),
    ("saldo precedente", E::SaldoInicial),
    ("beginsaldo", E::SaldoInicial),
    ("saldo final", E::SaldoFinal),
    ("saldo total", E::SaldoFinal),
    ("saldo cierre", E::SaldoFinal),
    ("saldo al cierre", E::SaldoFinal),
    ("saldo de cierre", E::SaldoFinal),
    ("nuevo saldo", E::SaldoFinal),
    ("saldo final disponible", E::SaldoFinal),
    ("closing balance", E::SaldoFinal),
    ("ending balance", E::SaldoFinal),
    ("end balance", E::SaldoFinal),
    ("final balance", E::SaldoFinal),
    ("new balance", E::SaldoFinal),
    ("balance carried forward", E::SaldoFinal),
    ("carried forward", E::SaldoFinal),
    ("endsaldo", E::SaldoFinal),
    ("neuer kontostand", E::SaldoFinal),
    ("neuer saldo", E::SaldoFinal),
    ("schlusssaldo", E::SaldoFinal),
    ("solde final", E::SaldoFinal),
    ("nouveau solde", E::SaldoFinal),
    ("solde de cloture", E::SaldoFinal),
    ("saldo finale", E::SaldoFinal),
    ("eindsaldo", E::SaldoFinal),
    ("saldo actual", E::SaldoActual(1)),
    ("saldo contable", E::SaldoActual(1)),
    ("current balance", E::SaldoActual(1)),
    ("account balance", E::SaldoActual(1)),
    ("ledger balance", E::SaldoActual(1)),
    ("aktueller kontostand", E::SaldoActual(1)),
    ("solde actuel", E::SaldoActual(1)),
    ("saldo attuale", E::SaldoActual(1)),
    ("huidig saldo", E::SaldoActual(1)),
    ("saldo", E::SaldoActual(2)),
    ("balance", E::SaldoActual(2)),
    ("kontostand", E::SaldoActual(2)),
    ("solde", E::SaldoActual(2)),
    ("saldo disponible", E::SaldoActual(3)),
    ("disponible", E::SaldoActual(3)),
    ("available balance", E::SaldoActual(3)),
    ("solde disponible", E::SaldoActual(3)),
    ("saldo disponibile", E::SaldoActual(3)),
    ("apoderado", E::Otra),
    ("autorizado", E::Otra),
    ("usuario", E::Otra),
    ("user", E::Otra),
    ("operacion", E::Otra),
    ("filtros", E::Otra),
    ("importe", E::Otra),
    ("importes", E::Otra),
    ("tipo de movimiento", E::Otra),
    ("hora", E::Otra),
    ("retenciones", E::Otra),
    ("total retenciones", E::Otra),
    ("abonos pendientes", E::Otra),
    ("limite", E::Otra),
    ("limite de credito", E::Otra),
    ("credit limit", E::Otra),
];

fn etiqueta_exacta(k: &str) -> Option<Etiqueta> {
    ETIQUETAS.iter().find(|(t, _)| *t == k).map(|(_, e)| *e)
}

/// La etiqueta más larga con la que empiezan estas palabras, y cuántas palabras ocupa.
pub(super) fn etiqueta_al_inicio(p: &[String]) -> Option<(Etiqueta, usize)> {
    (1..=p.len().min(5))
        .rev()
        .find_map(|n| etiqueta_exacta(&p[..n].join(" ")).map(|e| (e, n)))
}

/// La etiqueta más larga con la que acaban estas palabras, y cuántas palabras ocupa.
pub(super) fn etiqueta_al_final(p: &[String]) -> Option<(Etiqueta, usize)> {
    (1..=p.len().min(5))
        .rev()
        .find_map(|n| etiqueta_exacta(&p[p.len() - n..].join(" ")).map(|e| (e, n)))
}

/// Si el texto entero es una etiqueta de saldo (para las filas «Saldo inicial», «SALDO
/// ANTERIOR» o «Saldo final» de dentro de la tabla), o una de ellas seguida sólo de una fecha
/// («Saldo final al 31/12/2024»).
pub(super) fn etiqueta_de_saldo(texto: &str) -> Option<Etiqueta> {
    let p = palabras(texto);
    let (e, n) = etiqueta_al_inicio(&p)?;
    if !matches!(e, E::SaldoInicial | E::SaldoFinal | E::SaldoActual(_)) {
        return None;
    }
    let resto = &p[n..];
    let solo_fecha = resto.iter().all(|w| {
        w.bytes().all(|b| b.is_ascii_digit())
            || matches!(
                w.as_str(),
                "a" | "al"
                    | "el"
                    | "de"
                    | "del"
                    | "at"
                    | "on"
                    | "as"
                    | "of"
                    | "per"
                    | "du"
                    | "au"
                    | "le"
                    | "vom"
                    | "zum"
                    | "am"
            )
            || super::super::numeros::mes(w).is_some()
    });
    solo_fecha.then_some(e)
}

/// ¿Es una fila de totales («Total», «Totales», «Suma y sigue»…), que no es un movimiento?
pub(super) fn es_total(texto: &str) -> bool {
    matches!(
        clave(texto).as_str(),
        "total"
            | "totales"
            | "totals"
            | "suma"
            | "sumas"
            | "suma y sigue"
            | "suma anterior"
            | "subtotal"
            | "total cargos"
            | "total abonos"
            | "total debe"
            | "total haber"
            | "total movimientos"
            | "total debits"
            | "total credits"
            | "sum"
            | "summe"
            | "somme"
            | "totale"
            | "totaal"
            | "abonos pendientes"
            | "total retenciones"
            | "retenciones"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titulos_en_varios_idiomas() {
        let c = |s: &str| campo(s);
        assert_eq!(c("F. operación"), Some(FechaOperacion));
        assert_eq!(c(" FECHA CONTABLE "), Some(FechaContable));
        assert_eq!(c("Fecha de operación"), Some(FechaOperacion));
        assert_eq!(c("F. VALOR"), Some(FechaValor));
        assert_eq!(c("Buchungstag"), Some(FechaContable));
        assert_eq!(c("Valuta"), Some(Valor));
        assert_eq!(c("Date opération"), Some(FechaOperacion));
        assert_eq!(c("Completed Date"), Some(FechaContable));
        assert_eq!(c("BENEFICIARIO/ORDENANTE"), Some(Concepto));
        assert_eq!(c("Concepto ordenante"), Some(Concepto));
        assert_eq!(c("Verwendungszweck"), Some(Concepto));
        assert_eq!(c("Libellé"), Some(Concepto));
        assert_eq!(c("Amount (EUR)"), Some(Importe));
        assert_eq!(c("Importe en euros"), Some(Importe));
        assert_eq!(c("Betrag (EUR)"), Some(Importe));
        assert_eq!(c("Amount (Foreign Currency)"), Some(Ignorar));
        assert_eq!(c("Paid out"), Some(Cargo));
        assert_eq!(c("Débit"), Some(Cargo));
        assert_eq!(c("Crédit"), Some(Abono));
        assert_eq!(c("Haben"), Some(Abono));
        assert_eq!(c("Importe cargo"), Some(Cargo));
        assert_eq!(c("Kontostand"), Some(Saldo));
        assert_eq!(c("Saldo disponible"), Some(SaldoDisponible));
        assert_eq!(c("Net"), Some(ImporteNeto));
        assert_eq!(c("Gross"), Some(ImporteBruto));
        assert_eq!(c("D/H"), Some(Signo));
        assert_eq!(c("+/-"), Some(Signo));
        assert_eq!(c("TIPO DE MOVIMIENTO"), Some(Tipo));
        assert_eq!(c("Oficina"), Some(Ignorar));
        assert_eq!(c("Categoría"), Some(Ignorar));
        assert_eq!(c("Importe de la operación (EUR)"), Some(Importe));
        assert_eq!(c("Saldo tras la operación"), Some(Saldo));
        assert_eq!(c(""), None);
        assert_eq!(c("Lo que sea"), None);
        assert_eq!(divisa_de_titulo("Amount (EUR)").as_deref(), Some("EUR"));
        assert_eq!(divisa_de_titulo("Importe €").as_deref(), Some("EUR"));
        assert_eq!(divisa_de_titulo("Saldo en USD").as_deref(), Some("USD"));
        assert_eq!(divisa_de_titulo("Importe"), None);
    }

    #[test]
    fn etiquetas_de_los_datos_de_la_cuenta() {
        let p = |s: &str| palabras(s);
        assert_eq!(
            etiqueta_al_inicio(&p("Número de cuenta:")),
            Some((E::Cuenta, 3))
        );
        assert_eq!(
            etiqueta_al_inicio(&p("Titular de la tarjeta")),
            Some((E::Titular, 4))
        );
        assert_eq!(
            etiqueta_al_final(&p("61.234,57 € Saldo disponible")),
            Some((E::SaldoActual(3), 2))
        );
        assert_eq!(etiqueta_de_saldo("SALDO INICIAL"), Some(E::SaldoInicial));
        assert_eq!(
            etiqueta_de_saldo("Saldo final al 31/12/2024"),
            Some(E::SaldoFinal)
        );
        assert_eq!(etiqueta_de_saldo("Saldo a favor de Pepe"), None);
        assert_eq!(etiqueta_de_saldo("TRANSFERENCIA"), None);
        assert!(es_total("TOTAL"));
        assert!(es_total("Suma y sigue"));
        assert!(!es_total("TOTAL ENERGIES GASOLINERA"));
    }
}

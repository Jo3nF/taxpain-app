//! Contabilidad (Fase 4): partida doble estricta en céntimos según el PGC (RD 1514/2007) y el
//! PGC de PYMES (RD 1515/2007). Ver `docs/ACCOUNTING_RULES.md`.

pub mod account;
pub mod closing;
pub mod depreciation;
pub mod entry;
pub mod fiscal_year;
pub mod leasing;
pub mod ledger;
pub mod payroll;
pub mod pgc;
pub mod posting;
pub mod prestamo;
pub mod statements;
pub mod suenlace;
pub mod suenlace_in;

pub use account::{AccountCode, AccountError};
pub use entry::{EntryError, EntryLine, JournalEntry, Origin};
pub use fiscal_year::FiscalYear;
pub use ledger::{AccountBalance, trial_balance};
pub use posting::{
    PostingAccounts, ReceivedInvoice, post_issued, post_issued_with_advance, post_received,
};

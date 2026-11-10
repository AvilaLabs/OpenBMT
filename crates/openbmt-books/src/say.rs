//! What the owner is told when the books or an export cannot be made
//! (`docs/SCREEN-RULES.md`, SC-01 to SC-12).
//!
//! The reader is the owner. Every refusal says which export it is, that
//! no file was written (the books only reach a file through
//! [`crate::write_whole`], which writes the whole file or none), and what
//! to do. A counter is named by the name the owner gave it, which the
//! caller passes in; its key never appears. Record numbers, sale numbers
//! and times go in the details.

use openbmt_core::records::ChainError;
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::wire::DecodeError;

use crate::error::{BooksError, ExportError, OpenError, SealError};

/// Which export the owner asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    /// The accounting export, one CSV file for each month.
    Accounting,
    /// The full ledger, which shows the shop's payments on the blockchain.
    FullLedger,
}

impl ExportKind {
    /// The export as a sentence names it, with a capital for the start.
    fn named(self) -> &'static str {
        match self {
            Self::Accounting => "The accounting export",
            Self::FullLedger => "The full ledger",
        }
    }
}

fn stop(heading: &str, what: impl Into<String>, who: Who, next: impl Into<String>) -> Words {
    Words::new(Tone::Stop, heading, what, who, next)
}

/// How a record that could not be read is described.
fn read_cause(e: &DecodeError) -> &'static str {
    match e {
        DecodeError::Truncated => "ends too early",
        DecodeError::TrailingBytes => "has extra data after its end",
        DecodeError::BadMagic => "is not a kind of record the books know",
        DecodeError::UnsupportedVersion(_) => {
            "comes from a version of the app the books do not read"
        }
        DecodeError::TooLarge => "is larger than the books allow",
        DecodeError::NonCanonical(_) => "is written in a form the books do not accept",
        DecodeError::InvalidKey => "holds a key that is not valid",
        DecodeError::UnknownKind(_) => "holds an event the books do not know",
    }
}

/// How a counter's chain of records that does not check out is described.
fn chain_cause(e: &ChainError) -> &'static str {
    match e {
        ChainError::UnknownAuthor { .. } => "a record is from a device the shop does not know",
        ChainError::BadSignature { .. } => "a record's signature does not match it",
        ChainError::SeqMismatch { .. } => "a record is missing or out of place",
        ChainError::PrevMismatch { .. } => "a record does not follow the one before it",
        ChainError::BadCheckIn { .. } => "a check-in does not match the records around it",
        ChainError::Truncated { .. } => "records were cut off the end",
        ChainError::Diverged { .. } => "an earlier record was changed",
        ChainError::BodyTooLarge => "a record is larger than allowed",
        ChainError::NotACheckIn => "the app compared a record that is not a check-in",
    }
}

impl BooksError {
    /// What the owner is told when the books could not be built. No export
    /// file was written. `counter` is the name the owner gave the counter
    /// the refusal is about; for a refusal about no counter it is not used.
    pub fn words(&self, counter: &str) -> Words {
        match self {
            Self::CounterNotOnList { .. } => stop(
                "Counter not on the list",
                format!("The books cannot include the counter \"{counter}\", because it is not on the shop's signed list of devices."),
                Who::Owner,
                "No file was written. Pair the counter again, or leave it out, then build the books again.",
            )
            .code("BK-01"),
            Self::DuplicateCounter { .. } => stop(
                "Counter given twice",
                format!("The records of the counter \"{counter}\" were handed to the books twice."),
                Who::Owner,
                "No file was written. Try again; if it repeats, stop and tell support, and read them the details.",
            )
            .code("BK-01"),
            Self::BadChain { error, .. } => stop(
                "Counter's records refused",
                format!(
                    "The records of the counter \"{counter}\" do not check out: {}.",
                    chain_cause(error)
                ),
                Who::Owner,
                "No file was written. Copy the records from the counter again; if they still fail, stop and tell support.",
            )
            .code("BK-01")
            .detail(error.to_string()),
            Self::BadEvent { seq, error, .. } => stop(
                "A record could not be read",
                format!(
                    "A record of the counter \"{counter}\" could not be read as a sale event: it {}.",
                    read_cause(error)
                ),
                Who::Owner,
                "No file was written. Copy the records from the counter again; if it repeats, stop and tell support.",
            )
            .code("BK-03")
            .detail(format!("record {seq}: {error}")),
            Self::KindMismatch { seq, .. } => stop(
                "A record has the wrong kind",
                format!("A record of the counter \"{counter}\" is marked as one kind of event and holds another."),
                Who::Owner,
                "No file was written. Copy the records from the counter again; if it repeats, stop and tell support.",
            )
            .code("BK-03")
            .detail(format!("record {seq}")),
            Self::UnknownSale { seq, index, .. } => stop(
                "Event for an unopened sale",
                format!("The counter \"{counter}\" has an event for a sale that was never opened."),
                Who::Owner,
                "No file was written. Copy the records from the counter again; if it repeats, stop and tell support.",
            )
            .code("BK-04")
            .detail(format!("record {seq}, sale {index}")),
            Self::OpenedTwice { seq, index, .. } => stop(
                "Sale opened twice",
                format!("The counter \"{counter}\" opens the same sale twice."),
                Who::Owner,
                "No file was written. Copy the records from the counter again; if it repeats, stop and tell support.",
            )
            .code("BK-04")
            .detail(format!("record {seq}, sale {index}")),
            Self::TimeOutOfRange { at } => stop(
                "A sale date is impossible",
                "A record gives a time outside the years 1970 to 9999, so the books cannot place it.",
                Who::Owner,
                "No file was written. Check the clock of the counter and copy its records again; if it repeats, tell support.",
            )
            .code("BK-04")
            .detail(format!("time {at} seconds")),
            Self::SaleNotInBooks { sale } => stop(
                "A line names an unknown sale",
                format!("A line names a sale of the counter \"{counter}\" that is not in the books."),
                Who::Owner,
                "No file was written. Build the books again from all the counters' records, then add the line again.",
            )
            .code("BK-12")
            .detail(format!("sale {}", sale.index)),
            Self::Overflow => stop(
                "A total is too large",
                "A total in the books is too large to add up, so the books were not built.",
                Who::Owner,
                "No file was written. Stop and tell support, and read them the details.",
            )
            .code("BK-14"),
            Self::TooMany => stop(
                "The books are too big",
                "The books would hold more lines than the app keeps.",
                Who::Owner,
                "No file was written. Stop and tell support: the books have too many lines.",
            )
            .code("BK-14"),
            Self::BadOffset { minutes } => stop(
                "Time zone setting not usable",
                "The shop's time difference from UTC is outside 12 hours behind to 14 hours ahead.",
                Who::Owner,
                "No file was written. Correct the shop's time zone setting and try again.",
            )
            .code("BK-16")
            .detail(format!("offset {minutes} minutes")),
        }
    }
}

impl ExportError {
    /// What the owner is told when an export could not be made. No file was
    /// written. `kind` is the export asked for; `counter` is the name the
    /// owner gave the counter that a missing sale belongs to.
    pub fn words(&self, kind: ExportKind, counter: &str) -> Words {
        let export = kind.named();
        match self {
            Self::DisposalValueUnknown { position } => stop(
                "A dollar value is missing",
                format!("{export} needs a dollar value for a refund, cash-out or fee, and one has none."),
                Who::Owner,
                "No file was written. Enter the dollar value, then export again.",
            )
            .code("BK-17")
            .detail(format!("disposal {position}")),
            Self::MixedCurrencies => stop(
                "More than one currency",
                format!("{export} holds lines in more than one currency, and its file has no column for the currency."),
                Who::Owner,
                "No file was written. Export one currency at a time, or ask support how to separate them.",
            )
            .code("BK-17"),
            Self::TimeOutOfRange { at } => stop(
                "A line date is impossible",
                format!("{export} has a line dated outside the years 1970 to 9999, so no file was made."),
                Who::Owner,
                "No file was written. Check the clock of the counter the line came from, then export again.",
            )
            .code("BK-16")
            .detail(format!("time {at} seconds")),
            Self::SaleMissing { sale } => stop(
                "A return has no sale",
                format!("{export} has a cash return for a sale of the counter \"{counter}\" that is not in the books."),
                Who::Owner,
                "No file was written. Build the books again from all the counters' records, then export again.",
            )
            .code("BK-13")
            .detail(format!("sale {}", sale.index)),
            Self::Overflow => stop(
                "An export total is too large",
                format!("A total in {} is too large to add up, so no file was made.", export.to_lowercase()),
                Who::Owner,
                "No file was written. Stop and tell support, and read them the details.",
            )
            .code("BK-14"),
        }
    }
}

impl SealError {
    /// What the owner is told when an export file could not be locked with
    /// its passphrase. No file was written.
    pub fn words(&self) -> Words {
        match self {
            Self::EmptyPassphrase => stop(
                "Passphrase needed",
                "The export file is locked with a passphrase, and the one given is empty.",
                Who::Owner,
                "No file was written. Choose a passphrase and try again.",
            )
            .code("BK-27"),
            Self::Randomness => stop(
                "No random data available",
                "The phone could not give the random data the lock needs, so no file was made.",
                Who::Owner,
                "No file was written. Try again; if it repeats, stop and tell support, and read them the details.",
            )
            .code("BK-25"),
            Self::Key => stop(
                "The lock could not be made",
                "The app could not make the key from the passphrase, so no file was made.",
                Who::Owner,
                "No file was written. Try again; if it repeats, stop and tell support, and read them the details.",
            )
            .code("BK-24"),
            Self::Cipher => stop(
                "The file could not be locked",
                "The app could not lock the export file, so no file was made.",
                Who::Owner,
                "No file was written. Try again; if it repeats, stop and tell support, and read them the details.",
            )
            .code("BK-26"),
        }
    }
}

impl OpenError {
    /// What the owner is told when a locked export file could not be opened.
    /// The file was not changed.
    pub fn words(&self) -> Words {
        match self {
            Self::EmptyPassphrase => stop(
                "Passphrase needed",
                "To open the export file you must give its passphrase, and the one given is empty.",
                Who::Owner,
                "Nothing was changed. Type the passphrase the file was locked with and try again.",
            )
            .code("BK-27"),
            Self::WrongPassphraseOrDamaged => stop(
                "The file did not open",
                "Either the passphrase is wrong or the file is damaged; the app cannot tell which.",
                Who::Owner,
                "Nothing was changed. Check the passphrase and try again; if it still fails, use another copy of the file.",
            )
            .code("BK-26"),
        }
    }
}

/// A device to stand in for a counter in a sample: the generator point.
fn sample_device() -> Option<openbmt_core::identity::DeviceId> {
    let mut bytes = [0u8; 32];
    for (b, v) in bytes.iter_mut().zip([
        0x79u8, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce, 0x87, 0x0b,
        0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81, 0x5b, 0x16, 0xf8,
        0x17, 0x98,
    ]) {
        *b = v;
    }
    openbmt_core::identity::DeviceId::from_bytes(&bytes).ok()
}

/// SC-12: the words of every refusal of this crate, with at least one value
/// for every variant and every variant inside it. Built by hand so that a new
/// variant is noticed. The counter is called "Corner".
pub fn all_words() -> Vec<(String, Words)> {
    let Some(counter) = sample_device() else {
        return Vec::new();
    };
    let name = "Corner";
    let sale = crate::SaleKey { counter, index: 4 };
    let mut all: Vec<(String, Words)> = Vec::new();
    let mut add = |n: &str, w: Words| all.push((n.to_string(), w));

    add(
        "BooksError::CounterNotOnList",
        BooksError::CounterNotOnList { counter }.words(name),
    );
    add(
        "BooksError::DuplicateCounter",
        BooksError::DuplicateCounter { counter }.words(name),
    );
    let chains = [
        ("UnknownAuthor", ChainError::UnknownAuthor { seq: 3 }),
        ("BadSignature", ChainError::BadSignature { seq: 3 }),
        (
            "SeqMismatch",
            ChainError::SeqMismatch {
                expected: 4,
                found: 6,
            },
        ),
        ("PrevMismatch", ChainError::PrevMismatch { seq: 3 }),
        ("BadCheckIn", ChainError::BadCheckIn { seq: 3 }),
        ("Truncated", ChainError::Truncated { have: 3, need: 5 }),
        ("Diverged", ChainError::Diverged { seq: 3 }),
        ("BodyTooLarge", ChainError::BodyTooLarge),
        ("NotACheckIn", ChainError::NotACheckIn),
    ];
    for (n, error) in chains {
        add(
            &format!("BooksError::BadChain/{n}"),
            BooksError::BadChain { counter, error }.words(name),
        );
    }
    let decodes = [
        ("Truncated", DecodeError::Truncated),
        ("TrailingBytes", DecodeError::TrailingBytes),
        ("BadMagic", DecodeError::BadMagic),
        ("UnsupportedVersion", DecodeError::UnsupportedVersion(9)),
        ("TooLarge", DecodeError::TooLarge),
        ("NonCanonical", DecodeError::NonCanonical("a sample")),
        ("InvalidKey", DecodeError::InvalidKey),
        ("UnknownKind", DecodeError::UnknownKind(77)),
    ];
    for (n, error) in decodes {
        add(
            &format!("BooksError::BadEvent/{n}"),
            BooksError::BadEvent {
                counter,
                seq: 7,
                error,
            }
            .words(name),
        );
    }
    add(
        "BooksError::KindMismatch",
        BooksError::KindMismatch { counter, seq: 7 }.words(name),
    );
    add(
        "BooksError::UnknownSale",
        BooksError::UnknownSale {
            counter,
            seq: 7,
            index: 4,
        }
        .words(name),
    );
    add(
        "BooksError::OpenedTwice",
        BooksError::OpenedTwice {
            counter,
            seq: 7,
            index: 4,
        }
        .words(name),
    );
    add(
        "BooksError::TimeOutOfRange",
        BooksError::TimeOutOfRange { at: u64::MAX }.words(name),
    );
    add(
        "BooksError::SaleNotInBooks",
        BooksError::SaleNotInBooks { sale }.words(name),
    );
    add("BooksError::Overflow", BooksError::Overflow.words(name));
    add("BooksError::TooMany", BooksError::TooMany.words(name));
    add(
        "BooksError::BadOffset",
        BooksError::BadOffset { minutes: 900 }.words(name),
    );

    let kinds = [
        ("Accounting", ExportKind::Accounting),
        ("FullLedger", ExportKind::FullLedger),
    ];
    for (k, kind) in kinds {
        let errors = [
            (
                "DisposalValueUnknown",
                ExportError::DisposalValueUnknown { position: 2 },
            ),
            ("MixedCurrencies", ExportError::MixedCurrencies),
            (
                "TimeOutOfRange",
                ExportError::TimeOutOfRange { at: u64::MAX },
            ),
            ("SaleMissing", ExportError::SaleMissing { sale }),
            ("Overflow", ExportError::Overflow),
        ];
        for (n, e) in errors {
            add(&format!("ExportError::{n}/{k}"), e.words(kind, name));
        }
    }

    for (n, e) in [
        ("EmptyPassphrase", SealError::EmptyPassphrase),
        ("Randomness", SealError::Randomness),
        ("Key", SealError::Key),
        ("Cipher", SealError::Cipher),
    ] {
        add(&format!("SealError::{n}"), e.words());
    }
    for (n, e) in [
        ("EmptyPassphrase", OpenError::EmptyPassphrase),
        (
            "WrongPassphraseOrDamaged",
            OpenError::WrongPassphraseOrDamaged,
        ),
    ] {
        add(&format!("OpenError::{n}"), e.words());
    }
    all
}

/// SC-12: the rows of this crate's words in the one table of
/// `docs/screens/owner.md`, one line each, in the order of [`all_words`].
pub fn screens_markdown() -> String {
    let cell = |t: &str| t.replace('|', "/");
    let mut out = String::new();
    for (name, w) in all_words() {
        out.push_str(&format!(
            "| books: {name} | {} | {} | {} | {:?} | {} |\n",
            cell(&w.heading),
            cell(&w.what),
            cell(&w.next),
            w.who,
            w.code.unwrap_or("")
        ));
    }
    out
}

#[cfg(test)]
#[path = "say_tests.rs"]
mod tests;

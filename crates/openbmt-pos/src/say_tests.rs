//! Tests of the owner's words for the till (SC-01, SC-05, SC-07, SC-08,
//! SC-12).

use super::*;
use crate::alerts::TenderKey;
use crate::facts::SaleKey;
use crate::model::Id;
use openbmt_core::say::{check, check_distinct};

#[path = "../../openbmt-owner/src/screens_file.rs"]
mod screens_file;

fn alert_name(k: AlertKind) -> String {
    let n = match k {
        AlertKind::TenderWithoutPayment => "TenderWithoutPayment",
        AlertKind::ClosedBeforePaid => "ClosedBeforePaid",
        AlertKind::PaymentWithoutTender => "PaymentWithoutTender",
        AlertKind::AmountDiffers => "AmountDiffers",
        AlertKind::TenderOnUnpaidSale => "TenderOnUnpaidSale",
        AlertKind::VoidedAfterPayment => "VoidedAfterPayment",
        AlertKind::DuplicateReference => "DuplicateReference",
        AlertKind::BitcoinTip => "BitcoinTip",
        AlertKind::UnknownCurrency => "UnknownCurrency",
    };
    format!("Alert::{n}")
}

fn mismatch_name(e: SaleMismatch) -> &'static str {
    match e {
        SaleMismatch::Amount => "SaleMismatch::Amount",
        SaleMismatch::Currency => "SaleMismatch::Currency",
        SaleMismatch::OpenedAt => "SaleMismatch::OpenedAt",
    }
}

fn pos_error_name(e: &PosError) -> &'static str {
    match e {
        PosError::Unavailable => "PosError::Unavailable",
        PosError::SignInRefused => "PosError::SignInRefused",
        PosError::RateLimited { .. } => "PosError::RateLimited",
        PosError::BadResponse(_) => "PosError::BadResponse",
        PosError::Unsupported => "PosError::Unsupported",
    }
}

fn settings_name(e: SettingsError) -> &'static str {
    match e {
        SettingsError::TenderNames => "SettingsError::TenderNames",
        SettingsError::Currencies => "SettingsError::Currencies",
        SettingsError::Locations => "SettingsError::Locations",
        SettingsError::Timing => "SettingsError::Timing",
    }
}

fn model_name(e: ModelError) -> &'static str {
    match e {
        ModelError::BadId => "ModelError::BadId",
        ModelError::BadText => "ModelError::BadText",
        ModelError::TooManyTenders => "ModelError::TooManyTenders",
        ModelError::DuplicateTender => "ModelError::DuplicateTender",
        ModelError::RefundTooLarge => "ModelError::RefundTooLarge",
        ModelError::CursorTooLong => "ModelError::CursorTooLong",
    }
}

fn what_key(what: &str) -> &'static str {
    match what {
        "tracked orders" => "tracked orders",
        "matches" => "matches",
        "alerts" => "alerts",
        _ => "other",
    }
}

fn blocker_key(b: Option<&Blocker>) -> &'static str {
    match b {
        None => "none",
        Some(Blocker::OpenAlert(k)) => match k.subject {
            Subject::Tender(_) => "OpenAlert",
            Subject::Sale(_) => "OpenAlert on a sale",
        },
        Some(Blocker::SaleUnsettled { .. }) => "SaleUnsettled",
        Some(Blocker::TooRecent { .. }) => "TooRecent",
    }
}

/// The part of a name after `LedgerError::Full/`, for a refusal.
fn full_key(what: &str, oldest: &[Blocker]) -> String {
    format!("{}/{}", what_key(what), blocker_key(oldest.first()))
}

fn ledger_name(e: &LedgerError) -> String {
    match e {
        LedgerError::Settings(s) => settings_name(*s).to_string(),
        LedgerError::Order(m) => model_name(*m).to_string(),
        LedgerError::TooManySales => "LedgerError::TooManySales".to_string(),
        LedgerError::DuplicateSale(_) => "LedgerError::DuplicateSale".to_string(),
        LedgerError::Full { what, oldest } => {
            format!("LedgerError::Full/{}", full_key(what, oldest))
        }
        LedgerError::NotOpen => "LedgerError::NotOpen".to_string(),
    }
}

fn sync_name(e: &SyncError<String>) -> String {
    match e {
        SyncError::Pos(p) => pos_error_name(p).to_string(),
        SyncError::PageTooLarge => "SyncError::PageTooLarge".to_string(),
        SyncError::NoProgress => "SyncError::NoProgress".to_string(),
        SyncError::Ledger(l) => ledger_name(l),
        SyncError::Persist(_) => "SyncError::Persist".to_string(),
    }
}

fn write_name(e: &LedgerWriteError<String>) -> String {
    match e {
        LedgerWriteError::Refused(l) => ledger_name(l),
        // The same words as a failed store while reading the till.
        LedgerWriteError::Persist(_) => "SyncError::Persist".to_string(),
    }
}

fn facts_name(e: &FactsError) -> String {
    match e {
        FactsError::Chain(c) => format!("FactsError::Chain/{}", chain_name(c)),
        FactsError::BadEvent { error, .. } => {
            format!("FactsError::BadEvent/{}", decode_name(error))
        }
        FactsError::UnknownSale { .. } => "FactsError::UnknownSale".to_string(),
        FactsError::OpenedTwice { .. } => "FactsError::OpenedTwice".to_string(),
        FactsError::TooManySales => "LedgerError::TooManySales".to_string(),
    }
}

fn item_name(e: &PosItem) -> String {
    match e {
        PosItem::Match { matched, .. } => {
            let how = match matched.how {
                MatchHow::Reference => "Reference",
                MatchHow::AmountAndTime => "AmountAndTime",
            };
            let turn = if matched.by_order { " in turn" } else { "" };
            format!("PosItem::Match/{how}{turn}")
        }
        PosItem::Alert(a) => alert_name(a.key.kind),
        PosItem::Closed { .. } => "PosItem::Closed".to_string(),
        PosItem::UnpaidAfterClose { .. } => "PosItem::UnpaidAfterClose".to_string(),
    }
}

fn close_name(e: CloseRefused) -> &'static str {
    match e {
        CloseRefused::SaleNotPaid => "CloseRefused::SaleNotPaid",
    }
}

fn sample() -> (SaleKey, TenderKey, OrderId) {
    let mut bytes = [0u8; 32];
    for (b, v) in bytes.iter_mut().zip([
        0x79u8, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce, 0x87, 0x0b,
        0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81, 0x5b, 0x16, 0xf8,
        0x17, 0x98,
    ]) {
        *b = v;
    }
    let counter = openbmt_core::identity::DeviceId::from_bytes(&bytes).expect("a fixed key parses");
    let order = Id::new("ord-1042").expect("a fixed id");
    let tender = TenderKey {
        order: order.clone(),
        tender: Id::new("t-1").expect("a fixed id"),
    };
    (SaleKey { counter, index: 4 }, tender, order)
}

#[test]
fn sc_12_every_word_is_checked_on_its_own() {
    let bad: Vec<String> = all_words()
        .iter()
        .filter_map(|(name, w)| check(w).err().map(|e| format!("{name}: {e}")))
        .collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn sc_12_every_word_keeps_the_rules() {
    assert_eq!(check_distinct(&all_words()), Ok(()));
}

#[test]
fn sc_01_no_variant_is_missing() {
    let names: Vec<String> = all_words().into_iter().map(|(n, _)| n).collect();
    let (sale, tender, order) = sample();
    let mut wanted: Vec<String> = Vec::new();
    wanted.extend(ALL_KINDS.iter().map(|k| alert_name(*k)));
    wanted.extend(
        [
            SaleMismatch::Amount,
            SaleMismatch::Currency,
            SaleMismatch::OpenedAt,
        ]
        .into_iter()
        .map(|e| mismatch_name(e).to_string()),
    );
    let pos_errors = [
        PosError::Unavailable,
        PosError::SignInRefused,
        PosError::RateLimited {
            retry_after_secs: 1,
        },
        PosError::BadResponse("x"),
        PosError::Unsupported,
    ];
    wanted.extend(pos_errors.iter().map(|e| pos_error_name(e).to_string()));
    wanted.push(close_name(CloseRefused::SaleNotPaid).to_string());
    for s in [
        SettingsError::TenderNames,
        SettingsError::Currencies,
        SettingsError::Locations,
        SettingsError::Timing,
    ] {
        wanted.push(settings_name(s).to_string());
    }
    for m in [
        ModelError::BadId,
        ModelError::BadText,
        ModelError::TooManyTenders,
        ModelError::DuplicateTender,
        ModelError::RefundTooLarge,
        ModelError::CursorTooLong,
    ] {
        wanted.push(model_name(m).to_string());
    }
    let syncs: Vec<SyncError<String>> = vec![
        SyncError::Pos(PosError::Unavailable),
        SyncError::PageTooLarge,
        SyncError::NoProgress,
        SyncError::Ledger(LedgerError::NotOpen),
        SyncError::Persist("x".to_string()),
    ];
    wanted.extend(syncs.iter().map(sync_name));
    let writes: Vec<LedgerWriteError<String>> = vec![
        LedgerWriteError::Refused(LedgerError::NotOpen),
        LedgerWriteError::Persist("x".to_string()),
    ];
    wanted.extend(writes.iter().map(write_name));
    let ledgers = [
        LedgerError::Settings(SettingsError::Timing),
        LedgerError::Order(ModelError::BadId),
        LedgerError::TooManySales,
        LedgerError::DuplicateSale(sale),
        LedgerError::NotOpen,
    ];
    wanted.extend(ledgers.iter().map(ledger_name));
    let facts = [
        FactsError::Chain(ChainError::BodyTooLarge),
        FactsError::BadEvent {
            seq: 0,
            error: DecodeError::TooLarge,
        },
        FactsError::UnknownSale { index: 0 },
        FactsError::OpenedTwice { index: 0 },
        FactsError::TooManySales,
    ];
    wanted.extend(facts.iter().map(facts_name));
    for c in [
        ChainError::UnknownAuthor { seq: 0 },
        ChainError::BadSignature { seq: 0 },
        ChainError::SeqMismatch {
            expected: 0,
            found: 1,
        },
        ChainError::PrevMismatch { seq: 0 },
        ChainError::BadCheckIn { seq: 0 },
        ChainError::Truncated { have: 0, need: 1 },
        ChainError::Diverged { seq: 0 },
        ChainError::BodyTooLarge,
        ChainError::NotACheckIn,
    ] {
        wanted.push(facts_name(&FactsError::Chain(c)));
    }
    for e in [
        DecodeError::Truncated,
        DecodeError::TrailingBytes,
        DecodeError::BadMagic,
        DecodeError::UnsupportedVersion(1),
        DecodeError::TooLarge,
        DecodeError::NonCanonical("x"),
        DecodeError::InvalidKey,
        DecodeError::UnknownKind(1),
    ] {
        wanted.push(facts_name(&FactsError::BadEvent { seq: 0, error: e }));
    }
    for how in [MatchHow::Reference, MatchHow::AmountAndTime] {
        for by_order in [false, true] {
            let matched = Match {
                sale,
                how,
                by_order,
                matched_at: 0,
            };
            wanted.push(item_name(&PosItem::Match {
                tender: tender.clone(),
                matched,
            }));
        }
    }
    wanted.push(item_name(&PosItem::Closed {
        sale,
        order: order.clone(),
    }));
    wanted.push(item_name(&PosItem::UnpaidAfterClose { sale, order }));
    for name in &wanted {
        assert!(names.contains(name), "{name} has no words");
    }
    // Every entry of the list is one a variant maps to, and the list of the
    // full-ledger refusals holds every kind of blocker and every bound.
    for name in &names {
        if name.starts_with("LedgerError::Full/") {
            continue;
        }
        assert!(wanted.contains(name), "{name} is not a variant");
    }
    let fulls: Vec<&String> = names
        .iter()
        .filter(|n| n.starts_with("LedgerError::Full/"))
        .collect();
    for b in [
        "OpenAlert",
        "OpenAlert on a sale",
        "SaleUnsettled",
        "TooRecent",
        "none",
    ] {
        assert!(fulls.iter().any(|n| n.ends_with(&format!("/{b}"))), "{b}");
    }
    for w in ["tracked orders", "matches", "alerts", "other"] {
        assert!(
            fulls
                .iter()
                .any(|n| n.starts_with(&format!("LedgerError::Full/{w}/"))),
            "{w}"
        );
    }
}

#[test]
fn sc_05_someone_is_named() {
    for (name, w) in all_words() {
        assert!(
            !(w.who == Who::Nobody && w.tone == Tone::Stop),
            "{name} stops and names nobody"
        );
    }
}

#[test]
fn sc_07_the_two_alerts_about_a_bitcoin_payment_read_differently() {
    let all = all_words();
    let find = |n: &str| all.iter().find(|(name, _)| name == n).map(|(_, w)| w);
    let a = find("Alert::TenderWithoutPayment").expect("listed");
    let b = find("Alert::TenderOnUnpaidSale").expect("listed");
    assert_ne!(a.heading, b.heading);
    assert!(a.what.contains("no bitcoin sale matches it"));
    assert!(b.what.contains("for a sale that has not been paid"));
}

#[test]
fn sc_08_the_order_number_is_quoted_and_not_in_the_sentences() {
    let all = all_words();
    for (name, w) in &all {
        if name == "Alert::TenderOnUnpaidSale" {
            let q = w.quoted.as_ref().expect("an order number is quoted");
            assert_eq!(q.from, "The till's order number");
            assert_eq!(q.text, "ord-1042");
            assert!(!w.what.contains("ord-1042") && !w.next.contains("ord-1042"));
        }
        // An alert about a sale only has no order to quote.
        if name == "Alert::PaymentWithoutTender" {
            assert!(w.quoted.is_none());
        }
    }
}

#[test]
fn sc_03_times_go_in_at_and_amounts_in_the_shop_currency() {
    let all = all_words();
    let (_, w) = all
        .iter()
        .find(|(n, _)| n == "Alert::AmountDiffers")
        .expect("listed");
    assert_eq!(w.at, Some(1_700_000_000));
    assert!(w.what.contains("12.50 USD") && w.what.contains("10.00 USD"));
}

#[test]
fn sc_12_the_list_on_disk_is_this_one() {
    screens_file::check_or_write("pos", &screens_markdown());
}

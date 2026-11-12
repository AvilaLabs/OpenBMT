//! Tests of the owner's words for requests (RQ-35, RQ-36, SC-01, SC-05,
//! SC-07, SC-12).

use super::*;
use openbmt_core::currency::ShopCurrency;
use openbmt_core::say::{check, check_distinct, MAX_HEADING, MAX_SENTENCE};

/// What the samples gave: words, and the names that share words.
type Sampled = (Vec<(String, Words)>, Vec<&'static str>);

fn make_name(e: &MakeRefused) -> &'static str {
    match e {
        MakeRefused::NoteNeeded => "MakeRefused::NoteNeeded",
        MakeRefused::BitcoinOnly => "MakeRefused::BitcoinOnly",
        MakeRefused::DueOutOfRange => "MakeRefused::DueOutOfRange",
        MakeRefused::TooManyOpen => "MakeRefused::TooManyOpen",
        MakeRefused::TooSmall { .. } => "MakeRefused::TooSmall",
        MakeRefused::TooLarge { .. } => "MakeRefused::TooLarge",
        MakeRefused::StaleRate => "MakeRefused::StaleRate",
        MakeRefused::IndexUsed => "MakeRefused::IndexUsed",
        MakeRefused::BookFull => "MakeRefused::BookFull",
    }
}

fn share_name(e: &ShareRefused) -> &'static str {
    match e {
        ShareRefused::NoSuchRequest => "ShareRefused::NoSuchRequest",
        ShareRefused::NotOpen => "ShareRefused::NotOpen",
        ShareRefused::StaleRate => "ShareRefused::StaleRate",
    }
}

fn credit_name(e: &CreditRefused) -> &'static str {
    match e {
        CreditRefused::NoSuchRequest => "CreditRefused::NoSuchRequest",
        CreditRefused::NotInBlockOrder => "CreditRefused::NotInBlockOrder",
        CreditRefused::NoRate => "CreditRefused::NoRate",
        CreditRefused::TooManyPayments => "CreditRefused::TooManyPayments",
    }
}

fn end_name(e: &EndRefused) -> &'static str {
    match e {
        EndRefused::NoSuchRequest => "EndRefused::NoSuchRequest",
        EndRefused::AlreadyEnded => "EndRefused::AlreadyEnded",
        EndRefused::HasPayments => "EndRefused::HasPayments",
        EndRefused::NotPartPaid => "EndRefused::NotPartPaid",
        EndRefused::NotAlmost => "EndRefused::NotAlmost",
        EndRefused::NoRepeat => "EndRefused::NoRepeat",
    }
}

fn repeat_name(e: &RepeatRefused) -> &'static str {
    match e {
        RepeatRefused::NotDue => "RepeatRefused::NotDue",
        RepeatRefused::TooManyOpen => "RepeatRefused::TooManyOpen",
        RepeatRefused::IndexUsed => "RepeatRefused::IndexUsed",
        RepeatRefused::BookFull => "RepeatRefused::BookFull",
    }
}

fn return_name(e: &ReturnRefused) -> &'static str {
    match e {
        ReturnRefused::NoSuchRequest => "ReturnRefused::NoSuchRequest",
        ReturnRefused::TooMuch { .. } => "ReturnRefused::TooMuch",
        ReturnRefused::TooSmall { .. } => "ReturnRefused::TooSmall",
        ReturnRefused::StaleRate => "ReturnRefused::StaleRate",
        ReturnRefused::BadScript => "ReturnRefused::BadScript",
        ReturnRefused::QueueFull => "ReturnRefused::QueueFull",
        ReturnRefused::BookFull => "ReturnRefused::BookFull",
    }
}

fn address_name(e: &AddressRefused) -> &'static str {
    match e {
        AddressRefused::NotAnAddress => "AddressRefused::NotAnAddress",
        AddressRefused::TestNetwork => "AddressRefused::TestNetwork",
        AddressRefused::Own => "AddressRefused::Own",
    }
}

/// One value of every variant of every refusal, with what is said for it.
///
/// A refusal that is the same fault as another (a stale rate, too many open
/// requests) has the same words, and is listed once (SC-07); the name of the
/// others is in `shared`.
fn samples() -> Sampled {
    let mut words: Vec<(String, Words)> = Vec::new();
    let mut shared: Vec<&'static str> = Vec::new();
    let mut take = |name: &'static str, w: Words| {
        if words.iter().any(|(_, have)| *have == w) {
            shared.push(name);
        } else {
            words.push((name.to_string(), w));
        }
    };
    for e in [
        MakeRefused::NoteNeeded,
        MakeRefused::BitcoinOnly,
        MakeRefused::DueOutOfRange,
        MakeRefused::TooManyOpen,
        MakeRefused::TooSmall { min_sats: 5_000 },
        MakeRefused::TooLarge {
            max: Limit::Sats(2_100_000_000_000_000),
        },
        MakeRefused::TooLarge {
            max: Limit::Units(ShopCurrency::Eur, 1_000_000),
        },
        MakeRefused::StaleRate,
        MakeRefused::IndexUsed,
        MakeRefused::BookFull,
    ] {
        take(make_name(&e), make_refused(&e));
    }
    for e in [
        ShareRefused::NoSuchRequest,
        ShareRefused::NotOpen,
        ShareRefused::StaleRate,
    ] {
        take(share_name(&e), share_refused(&e));
    }
    for e in [
        CreditRefused::NoSuchRequest,
        CreditRefused::NotInBlockOrder,
        CreditRefused::NoRate,
        CreditRefused::TooManyPayments,
    ] {
        take(credit_name(&e), credit_refused(&e));
    }
    for e in [
        EndRefused::NoSuchRequest,
        EndRefused::AlreadyEnded,
        EndRefused::HasPayments,
        EndRefused::NotPartPaid,
        EndRefused::NotAlmost,
        EndRefused::NoRepeat,
    ] {
        take(end_name(&e), end_refused(&e));
    }
    for e in [
        RepeatRefused::NotDue,
        RepeatRefused::TooManyOpen,
        RepeatRefused::IndexUsed,
        RepeatRefused::BookFull,
    ] {
        take(repeat_name(&e), repeat_refused(&e));
    }
    for e in [
        ReturnRefused::NoSuchRequest,
        ReturnRefused::TooMuch { most_sats: 12_345 },
        ReturnRefused::TooSmall { min_sats: 294 },
        ReturnRefused::StaleRate,
        ReturnRefused::BadScript,
        ReturnRefused::QueueFull,
        ReturnRefused::BookFull,
    ] {
        take(return_name(&e), return_refused(&e));
    }
    for e in [
        AddressRefused::NotAnAddress,
        AddressRefused::TestNetwork,
        AddressRefused::Own,
    ] {
        take(address_name(&e), address_refused(&e));
    }
    words.push((
        "BookError::Io".into(),
        book_unreadable(&BookError::Io("read: x".into())),
    ));
    // The same words, with another detail for the small print.
    let corrupt = book_unreadable(&BookError::Corrupt("it is cut short"));
    assert_eq!(corrupt.what, words[words.len() - 1].1.what);
    assert_ne!(corrupt.detail, words[words.len() - 1].1.detail);
    shared.push("BookError::Corrupt");
    words.push(("old price".into(), old_price()));
    words.push(("return added".into(), return_added("12,345 sats", 7)));
    (words, shared)
}

#[test]
fn sc_12_every_refusal_and_state_has_words_that_pass_the_check() {
    let (words, shared) = samples();
    assert!(shared.len() >= 3);
    assert!(words.len() >= 30);
    check_distinct(&words).unwrap();
    for (name, w) in &words {
        check(w).unwrap_or_else(|e| panic!("{name}: {e}"));
        let code = w.code.unwrap_or_else(|| panic!("{name} has no rule"));
        assert!(code.starts_with("RQ-"), "{name}: {code}");
    }
}

#[test]
fn sc_12_the_steps_of_a_return_pass_the_check() {
    for w in [ask_step(), check_step(), returns_waiting(3, "$120")] {
        check(&w).unwrap_or_else(|e| panic!("{}: {e}", w.heading));
    }
    assert_eq!(
        returns_waiting(3, "$120").what,
        "3 returns, $120 in all, go in one payment from your wallet."
    );
}

#[test]
fn sc_12_a_variant_missing_from_the_list_is_found() {
    let (words, shared) = samples();
    let listed: Vec<&str> = words
        .iter()
        .map(|(n, _)| n.as_str())
        .chain(shared.iter().copied())
        .collect();
    let all = [
        "MakeRefused::NoteNeeded",
        "MakeRefused::BitcoinOnly",
        "MakeRefused::DueOutOfRange",
        "MakeRefused::TooManyOpen",
        "MakeRefused::TooSmall",
        "MakeRefused::TooLarge",
        "MakeRefused::StaleRate",
        "MakeRefused::IndexUsed",
        "MakeRefused::BookFull",
        "ShareRefused::NoSuchRequest",
        "ShareRefused::NotOpen",
        "ShareRefused::StaleRate",
        "CreditRefused::NoSuchRequest",
        "CreditRefused::NotInBlockOrder",
        "CreditRefused::NoRate",
        "CreditRefused::TooManyPayments",
        "EndRefused::NoSuchRequest",
        "EndRefused::AlreadyEnded",
        "EndRefused::HasPayments",
        "EndRefused::NotPartPaid",
        "EndRefused::NotAlmost",
        "EndRefused::NoRepeat",
        "RepeatRefused::NotDue",
        "RepeatRefused::TooManyOpen",
        "RepeatRefused::IndexUsed",
        "RepeatRefused::BookFull",
        "ReturnRefused::NoSuchRequest",
        "ReturnRefused::TooMuch",
        "ReturnRefused::TooSmall",
        "ReturnRefused::StaleRate",
        "ReturnRefused::BadScript",
        "ReturnRefused::QueueFull",
        "ReturnRefused::BookFull",
        "AddressRefused::NotAnAddress",
        "AddressRefused::TestNetwork",
        "AddressRefused::Own",
        "BookError::Io",
        "BookError::Corrupt",
    ];
    for name in all {
        assert!(listed.contains(&name), "{name} has no words in the list");
    }
}

#[test]
fn sc_12_the_owner_is_not_told_to_tell_the_owner_of_a_stale_rate() {
    let w = stale_rate();
    assert_eq!(w.heading, "Exchange price too old");
    assert!(!w.next.contains("owner"));
}

#[test]
fn rq_36_the_texts_are_the_table_as_it_is_written() {
    let w = make_refused(&MakeRefused::TooSmall { min_sats: 5_000 });
    assert_eq!(w.heading, "Amount too small");
    assert_eq!(
        w.what,
        "Requests must be at least 5,000 sats, the smallest bitcoin sale in your settings."
    );
    assert_eq!(w.next, "Ask for more, or change the setting.");
    let w = make_refused(&MakeRefused::TooLarge {
        max: Limit::Units(ShopCurrency::Usd, 1_000_000),
    });
    assert_eq!(w.what, "A request can be at most $1,000,000.");
    assert_eq!(
        book_unreadable(&BookError::Corrupt("x")).next,
        "Do not clear the app's data. The file is kept as it is, and your money is in your wallet, not in this file."
    );
    assert_eq!(
        share_line("Dunmore Books", "$40", 12, "3 March"),
        "Dunmore Books asks for $40. Request No. 12, due 3 March."
    );
    assert_eq!(
        overdue_line("Dunmore Books", "$40", 12, "3 March"),
        "Dunmore Books still asks for $40. Request No. 12 was due 3 March."
    );
    assert_eq!(
        ask_line("Dunmore Books", "$40", 12),
        "Dunmore Books would like to return $40 for request No. 12. Please reply with a bitcoin address of yours."
    );
    assert_eq!(overdue_row(1), "1 overdue");
    assert_eq!(overdue_row(3), "3 overdue");
    assert_eq!(
        list_row(12, "$40", "3 March"),
        "No. 12 \u{b7} $40 still owed \u{b7} due 3 March"
    );
}

#[test]
fn rq_36_the_state_lines_and_marks_are_the_table_as_it_is_written() {
    let amount = |n: u64| format!("{n} units");
    assert_eq!(state_line(&Status::Waiting, &amount), "Waiting for payment");
    assert_eq!(
        state_line(&Status::PartPaid { owed: 5 }, &amount),
        "5 units still owed"
    );
    assert_eq!(state_line(&Status::Paid, &amount), "Paid");
    assert_eq!(
        state_line(&Status::Overpaid { excess: 2 }, &amount),
        "Paid, with 2 units extra"
    );
    assert_eq!(
        state_line(&Status::Almost { short: 3 }, &amount),
        "Almost paid: 3 units short"
    );
    let all = Marks {
        late: true,
        old_price: true,
        after_cancel: true,
        after_close: true,
    };
    assert_eq!(
        mark_lines(&all),
        [
            "Paid late",
            "On an old price",
            "Paid after it was cancelled",
            "Paid after it was closed"
        ]
    );
    assert!(mark_lines(&Marks::default()).is_empty());
}

#[test]
fn sc_12_the_dialogs_keep_the_lengths_the_rules_give() {
    let dialogs = [
        cancel_confirm(1),
        cancel_confirm(1_000_000),
        count_as_paid_confirm(1, "$0.40"),
        close_confirm(1, "$40"),
        close_confirm(100_000, "$1,000,000"),
    ];
    for d in &dialogs {
        assert!(d.title.chars().count() <= MAX_HEADING, "{}", d.title);
        for sentence in d.text.split_inclusive(". ") {
            assert!(sentence.chars().count() <= MAX_SENTENCE, "{sentence}");
        }
        assert!(d.text.ends_with('.'));
    }
}

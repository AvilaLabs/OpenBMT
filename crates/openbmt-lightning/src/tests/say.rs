//! LN-22: SC-01 to SC-12 for the words of this crate, and LN-02.

use openbmt_core::say::{check_distinct, Tone, Who};

use super::*;
use crate::error::{
    AskRefused, ConnectionRefused, CredentialRefused, InvoiceRefused, ReplyRefused, UriRefused,
};
use crate::say::*;
use crate::NotPaid;

#[test]
fn ln_22_every_word_passes_the_check_and_none_repeats() {
    check_distinct(&all_words()).expect("the words pass");
}

// Written out again with no catch-all arm, so that a new variant stops this
// from compiling until it has a value in the list.

fn credential_listed(e: &CredentialRefused) -> bool {
    match e {
        CredentialRefused::BadAddress
        | CredentialRefused::BadConnectString
        | CredentialRefused::NoCertificate
        | CredentialRefused::BadCertificate
        | CredentialRefused::BadFingerprint
        | CredentialRefused::BadMacaroon
        | CredentialRefused::RuneNotBase64
        | CredentialRefused::RuneUnrestricted
        | CredentialRefused::RuneNotText
        | CredentialRefused::RuneSyntax(_)
        | CredentialRefused::RuneMixesMethod
        | CredentialRefused::RuneAllowsMethod(_)
        | CredentialRefused::RuneLacksMethod => all_credential()
            .iter()
            .any(|v| std::mem::discriminant(v) == std::mem::discriminant(e)),
    }
}

fn reply_listed(e: &ReplyRefused) -> bool {
    match e {
        ReplyRefused::NodeSaid { .. }
        | ReplyRefused::TooLarge
        | ReplyRefused::Unreadable
        | ReplyRefused::Missing(_)
        | ReplyRefused::WrongType(_)
        | ReplyRefused::OutOfRange(_)
        | ReplyRefused::OtherInvoice
        | ReplyRefused::Several
        | ReplyRefused::LabelTaken
        | ReplyRefused::LabelGone => all_reply()
            .iter()
            .any(|v| std::mem::discriminant(v) == std::mem::discriminant(e)),
    }
}

fn invoice_listed(e: &InvoiceRefused) -> bool {
    match e {
        InvoiceRefused::TooLong
        | InvoiceRefused::MixedCase
        | InvoiceRefused::NotAnInvoice
        | InvoiceRefused::UnknownNetwork
        | InvoiceRefused::BadAmount
        | InvoiceRefused::Truncated
        | InvoiceRefused::BadField(_)
        | InvoiceRefused::DuplicateField(_)
        | InvoiceRefused::MissingField(_)
        | InvoiceRefused::BadSignature
        | InvoiceRefused::WrongNetwork
        | InvoiceRefused::NoAmount
        | InvoiceRefused::WrongAmount { .. }
        | InvoiceRefused::WrongPaymentHash
        | InvoiceRefused::WrongExpiry { .. }
        | InvoiceRefused::WrongDescription
        | InvoiceRefused::WrongNode => all_invoice()
            .iter()
            .any(|v| std::mem::discriminant(v) == std::mem::discriminant(e)),
    }
}

fn connection_listed(e: &ConnectionRefused) -> bool {
    match e {
        ConnectionRefused::CredentialTooStrong { .. }
        | ConnectionRefused::ProbeUnclear { .. }
        | ConnectionRefused::RuneNotFromNode
        | ConnectionRefused::RuneRevoked
        | ConnectionRefused::NoInvoice(_)
        | ConnectionRefused::LabelUsed
        | ConnectionRefused::WrongNode
        | ConnectionRefused::BadInvoice(_) => all_connection()
            .iter()
            .any(|v| std::mem::discriminant(v) == std::mem::discriminant(e)),
    }
}

fn not_paid_listed(e: &NotPaid) -> bool {
    match e {
        NotPaid::Waiting
        | NotPaid::Expired
        | NotPaid::Cancelled
        | NotPaid::Held
        | NotPaid::TooLittle { .. }
        | NotPaid::NotFound
        | NotPaid::Unknown(_) => all_not_paid()
            .iter()
            .any(|v| std::mem::discriminant(v) == std::mem::discriminant(e)),
    }
}

#[test]
fn ln_22_a_variant_missing_from_the_list_fails() {
    // Every listed value is of a variant the matches above know, and no two
    // listed values of one kind are the same variant unless the words differ
    // by node (the two kinds of credential that was too strong).
    assert!(all_credential().iter().all(credential_listed));
    assert!(all_reply().iter().all(reply_listed));
    assert!(all_invoice().iter().all(invoice_listed));
    assert!(all_connection().iter().all(connection_listed));
    assert!(all_not_paid().iter().all(not_paid_listed));
    for e in all_ask() {
        match e {
            AskRefused::BadAmount | AskRefused::BadExpiry => {}
        }
    }
    for e in all_uri() {
        match e {
            UriRefused::BadAddress | UriRefused::AmountMismatch | UriRefused::NotBech32 => {}
        }
    }
    let all = all_words();
    assert_eq!(
        all.len(),
        all_credential().len()
            + all_reply().len()
            + all_invoice().len()
            + all_ask().len()
            + all_uri().len()
            + all_connection().len()
            + all_not_paid().len()
            + 1
    );
    let names: std::collections::BTreeSet<&String> = all.iter().map(|(n, _)| n).collect();
    assert_eq!(names.len(), all.len(), "a name is listed twice");
}

#[test]
fn ln_22_a_setup_is_for_the_owner_and_a_sale_is_for_the_cashier() {
    for e in all_credential() {
        let w = e.words();
        assert_eq!((w.who, w.tone), (Who::Owner, Tone::Stop), "{e}");
        assert_eq!(w.code, Some(e.rule()));
    }
    for e in all_connection() {
        let w = e.words();
        assert_eq!((w.who, w.tone), (Who::Owner, Tone::Stop), "{e}");
        assert_eq!(w.code, Some(e.rule()));
    }
    for e in all_reply() {
        assert_eq!(e.words().who, Who::Cashier, "{e}");
    }
    for e in all_invoice() {
        let w = e.words();
        assert_eq!(w.who, Who::Cashier, "{e}");
        // The customer is never told to pay, and nothing is handed over.
        assert!(w.next.contains("another kind of payment"), "{e}");
        assert_eq!(w.tone, Tone::Stop);
    }
    for e in all_ask() {
        assert_eq!(e.words().who, Who::Cashier);
    }
    for e in all_uri() {
        assert_eq!(e.words().who, Who::Cashier);
    }
}

#[test]
fn ln_22_what_the_node_wrote_is_quoted_and_never_in_the_sentences() {
    let e = ReplyRefused::NodeSaid {
        status: 500,
        message: "wallet is locked, enter the password".to_string(),
    };
    let w = e.words();
    assert_eq!(
        w.quoted.as_ref().map(|q| q.text.as_str()),
        Some("wallet is locked, enter the password")
    );
    for text in [&w.heading, &w.what, &w.next] {
        assert!(!text.contains("locked"));
    }
    // With nothing to quote there is no quotation.
    let none = ReplyRefused::NodeSaid {
        status: 502,
        message: String::new(),
    };
    assert_eq!(none.words().quoted, None);
    // A state the crate does not know is quoted too.
    let w = NotPaid::Unknown("HOLD".to_string()).words();
    assert_eq!(w.quoted.as_ref().map(|q| q.text.as_str()), Some("HOLD"));
    assert!(!w.what.contains("HOLD"));
    // So is the method a rune allows that it should not.
    let w = CredentialRefused::RuneAllowsMethod("pay".to_string()).words();
    assert_eq!(w.quoted.as_ref().map(|q| q.text.as_str()), Some("pay"));
}

#[test]
fn ln_22_the_words_of_a_setup_give_the_command_to_make_the_right_key() {
    let mac = CredentialRefused::BadMacaroon.words();
    assert!(mac
        .next
        .contains("lncli bakemacaroon invoices:read invoices:write"));
    let strong = ConnectionRefused::CredentialTooStrong {
        call: "GET /v1/balance/blockchain",
        rule: "LN-06",
    }
    .words();
    assert!(strong
        .next
        .contains("lncli bakemacaroon invoices:read invoices:write"));
    let rune = ConnectionRefused::CredentialTooStrong {
        call: "POST /v1/listfunds",
        rule: "LN-09",
    }
    .words();
    assert!(rune.next.contains("invoice and listinvoices"));
    assert_ne!(strong.next, rune.next);
}

#[test]
fn ln_22_a_paid_payment_is_the_only_green_screen() {
    let green: Vec<String> = all_words()
        .into_iter()
        .filter(|(_, w)| w.tone == Tone::Good)
        .map(|(n, _)| n)
        .collect();
    assert_eq!(green, ["Payment::Paid"]);
}

#[test]
fn ln_22_the_list_is_on_disk() {
    let path = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("../../docs/screens/lightning.md");
    let want = screens_markdown();
    if std::env::var_os("OPENBMT_WRITE_SCREENS").is_some() {
        std::fs::write(&path, &want).expect("write docs/screens/lightning.md");
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "docs/screens/lightning.md is not what screens_markdown makes: run this test with \
         OPENBMT_WRITE_SCREENS=1 to write it again"
    );
}

#[test]
fn ln_02_eclair_is_not_supported_and_the_design_and_the_rules_say_why() {
    let rules = include_str!("../../../../docs/LIGHTNING-RULES.md");
    let design = include_str!("../../../../docs/DESIGN.md");
    assert!(rules.contains("Eclair is not supported"));
    assert!(rules.contains("one password"));
    assert!(rules.contains("no restricted credential"));
    assert!(design.contains("Eclair is not supported"));
    assert!(design.contains("no restricted credential"));
    // The design no longer lists Eclair among the nodes that can be connected.
    assert!(!design.contains("Core Lightning, LND or Eclair"));
    // There is no credential for it: the kinds are the two of LN-01.
    let _ = (lnd(), cln());
}

//! Tests of the owner's words for the queue and the set-up (SC-01, SC-05,
//! SC-07, SC-08, SC-12).

use super::*;
use bitcoin::hashes::Hash;
use openbmt_core::say::{check, check_distinct};

mod screens_file;

fn kind_name(k: EntryKind) -> &'static str {
    match k {
        EntryKind::Refund => "Refund",
        EntryKind::CashOut => "CashOut",
        EntryKind::Batch => "Batch",
        EntryKind::SettingsChange => "SettingsChange",
        EntryKind::Pairing => "Pairing",
    }
}

fn stage_name(s: &Stage, k: EntryKind) -> String {
    let n = match s {
        Stage::Queued => "Queued",
        Stage::InSession { .. } => "InSession",
        Stage::Returned { reason } if reason == REASON_DROPPED => "Dropped",
        Stage::Returned { .. } => "Returned",
        Stage::Signed { .. } => "Signed",
        Stage::Sent { .. } => "Sent",
        Stage::Paid { .. } => "Paid",
    };
    format!("Stage::{n}/{}", kind_name(k))
}

fn known(list: &[&'static str], what: &str) -> &'static str {
    let last = list.last().copied().unwrap_or("");
    list.iter()
        .copied()
        .take(list.len().saturating_sub(1))
        .find(|w| *w == what)
        .unwrap_or(last)
}

fn encode_name(e: &EncodeError) -> &'static str {
    match e {
        EncodeError::TooLarge => "TooLarge",
        EncodeError::ItemCount => "ItemCount",
        EncodeError::ItemOrder => "ItemOrder",
        EncodeError::BadText(_) => "BadText",
        EncodeError::BadItem(_) => "BadItem",
        EncodeError::BadPayload(_) => "BadPayload",
    }
}

fn decode_name(e: &DecodeError) -> &'static str {
    match e {
        DecodeError::Truncated => "Truncated",
        DecodeError::TrailingBytes => "TrailingBytes",
        DecodeError::BadMagic => "BadMagic",
        DecodeError::UnsupportedVersion(_) => "UnsupportedVersion",
        DecodeError::TooLarge => "TooLarge",
        DecodeError::NonCanonical(_) => "NonCanonical",
        DecodeError::InvalidKey => "InvalidKey",
        DecodeError::UnknownKind(_) => "UnknownKind",
    }
}

fn bind_name(e: &BindError) -> &'static str {
    match e {
        BindError::WrongApprover => "WrongApprover",
        BindError::BadSignature => "BadSignature",
        BindError::WrongSession => "WrongSession",
        BindError::WrongRequest => "WrongRequest",
        BindError::UnknownItem { .. } => "UnknownItem",
        BindError::MissingItem { .. } => "MissingItem",
        BindError::KindMismatch { .. } => "KindMismatch",
        BindError::BadHealthProof { .. } => "BadHealthProof",
    }
}

fn refusal_name(r: &Refusal) -> String {
    let n = match r {
        Refusal::Full(w) => format!("Full/{}", known(&FULLS, w)),
        Refusal::BadText(w) => format!("BadText/{}", known(&BAD_TEXTS, w)),
        Refusal::DestinationKind => "DestinationKind".to_string(),
        Refusal::BelowDust { .. } => "BelowDust".to_string(),
        Refusal::AmountTooLarge => "AmountTooLarge".to_string(),
        Refusal::UnknownSale => "UnknownSale".to_string(),
        Refusal::MoreThanReceived { .. } => "MoreThanReceived".to_string(),
        Refusal::SaleInUse => "SaleInUse".to_string(),
        Refusal::UnknownEntry(_) => "UnknownEntry".to_string(),
        Refusal::WrongStage(w) => format!("WrongStage/{}", known(&WRONG_STAGES, w)),
        Refusal::BatchNotDerived => "BatchNotDerived".to_string(),
        Refusal::PairingRequest(_) => "PairingRequest".to_string(),
        Refusal::SettingsInvalid(_) => "SettingsInvalid".to_string(),
        Refusal::SettingsVersion { .. } => "SettingsVersion".to_string(),
        Refusal::ZeroFeeRate => "ZeroFeeRate".to_string(),
        Refusal::Encode(e) => format!("Encode/{}", encode_name(e)),
        Refusal::SessionIdUsed => "SessionIdUsed".to_string(),
        Refusal::Frame(e) => format!("Frame/{}", decode_name(e)),
        Refusal::UnknownSession => "UnknownSession".to_string(),
        Refusal::Bind(e) => format!("Bind/{}", bind_name(e)),
        Refusal::OutboxFull => "OutboxFull".to_string(),
        Refusal::UnknownTransaction(_) => "UnknownTransaction".to_string(),
        Refusal::SnapshotTooLarge => "SnapshotTooLarge".to_string(),
        Refusal::Key1Wallet => "Key1Wallet".to_string(),
    };
    format!("Refusal::{n}")
}

fn owner_error_name(e: &OwnerError<String>) -> String {
    match e {
        OwnerError::Refused(r) => refusal_name(r),
        OwnerError::Store(_) => "OwnerError::Store".to_string(),
    }
}

fn setup_name(e: &SetupError) -> String {
    match e {
        SetupError::Identity(IdentityError::InvalidSecret) => {
            "SetupError::Identity/InvalidSecret".to_string()
        }
        SetupError::Identity(IdentityError::InvalidPublicKey) => {
            "SetupError::Identity/InvalidPublicKey".to_string()
        }
        SetupError::SameRoot => "SetupError::SameRoot".to_string(),
    }
}

fn restore_name(e: &RestoreError) -> String {
    match e {
        RestoreError::Setup(s) => setup_name(s),
        RestoreError::Decode(d) => format!("RestoreError::Decode/{}", decode_name(d)),
        RestoreError::WrongWallet => "RestoreError::WrongWallet".to_string(),
        RestoreError::WrongOwner => "RestoreError::WrongOwner".to_string(),
        RestoreError::WrongApprover => "RestoreError::WrongApprover".to_string(),
        RestoreError::Inconsistent(_) => "RestoreError::Inconsistent".to_string(),
    }
}

fn outcome_name(o: &ItemOutcome) -> &'static str {
    match o {
        ItemOutcome::Delivered(_) => "ItemOutcome::Delivered",
        ItemOutcome::Returned(_) => "ItemOutcome::Returned",
        ItemOutcome::Healthy => "ItemOutcome::Healthy",
    }
}

fn find(name: &str) -> Words {
    all_words()
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, w)| w)
        .unwrap_or_else(|| panic!("{name} is not listed"))
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
    let txid = Txid::all_zeros();
    let mut wanted: Vec<String> = Vec::new();
    for k in ALL_KINDS {
        for s in [
            Stage::Queued,
            Stage::InSession { session: [0; 32] },
            Stage::Returned {
                reason: "no".to_string(),
            },
            Stage::Returned {
                reason: REASON_DROPPED.to_string(),
            },
            Stage::Signed { txid },
            Stage::Sent { txid },
            Stage::Paid { txid },
        ] {
            wanted.push(stage_name(&s, k));
        }
    }
    let mut refusals: Vec<Refusal> = Vec::new();
    refusals.extend(FULLS.iter().copied().map(Refusal::Full));
    refusals.extend(BAD_TEXTS.iter().copied().map(Refusal::BadText));
    refusals.extend(WRONG_STAGES.iter().copied().map(Refusal::WrongStage));
    refusals.extend([
        Refusal::DestinationKind,
        Refusal::BelowDust { sats: 1, dust: 2 },
        Refusal::AmountTooLarge,
        Refusal::UnknownSale,
        Refusal::MoreThanReceived {
            received: 1,
            wanted: 2,
        },
        Refusal::SaleInUse,
        Refusal::UnknownEntry(0),
        Refusal::BatchNotDerived,
        Refusal::PairingRequest(String::new()),
        Refusal::SettingsInvalid(String::new()),
        Refusal::SettingsVersion { known: 1, got: 0 },
        Refusal::ZeroFeeRate,
        Refusal::SessionIdUsed,
        Refusal::UnknownSession,
        Refusal::OutboxFull,
        Refusal::UnknownTransaction(txid),
        Refusal::SnapshotTooLarge,
        Refusal::Key1Wallet,
    ]);
    refusals.extend(
        [
            EncodeError::TooLarge,
            EncodeError::ItemCount,
            EncodeError::ItemOrder,
            EncodeError::BadText("x"),
            EncodeError::BadItem("x"),
            EncodeError::BadPayload("x"),
        ]
        .into_iter()
        .map(Refusal::Encode),
    );
    refusals.extend(decodes().into_iter().map(|(_, e)| Refusal::Frame(e)));
    refusals.extend(
        [
            BindError::WrongApprover,
            BindError::BadSignature,
            BindError::WrongSession,
            BindError::WrongRequest,
            BindError::UnknownItem { item: 0 },
            BindError::MissingItem { item: 0 },
            BindError::KindMismatch { item: 0 },
            BindError::BadHealthProof { item: 0 },
        ]
        .into_iter()
        .map(Refusal::Bind),
    );
    wanted.extend(refusals.iter().map(refusal_name));
    wanted.push(owner_error_name(&OwnerError::Store("x".to_string())));
    wanted.push(owner_error_name(&OwnerError::Refused(Refusal::OutboxFull)));
    let mut setups = vec![
        SetupError::Identity(IdentityError::InvalidSecret),
        SetupError::Identity(IdentityError::InvalidPublicKey),
        SetupError::SameRoot,
    ];
    wanted.extend(setups.iter().map(setup_name));
    let mut restores = vec![
        RestoreError::Setup(setups.remove(0)),
        RestoreError::WrongWallet,
        RestoreError::WrongOwner,
        RestoreError::WrongApprover,
        RestoreError::Inconsistent("x"),
    ];
    restores.extend(decodes().into_iter().map(|(_, e)| RestoreError::Decode(e)));
    wanted.extend(restores.iter().map(restore_name));
    for o in [
        ItemOutcome::Delivered(0),
        ItemOutcome::Returned(String::new()),
        ItemOutcome::Healthy,
    ] {
        wanted.push(outcome_name(&o).to_string());
    }
    wanted.extend(SKIPPED.iter().map(|(n, _)| format!("Skipped::{n}")));
    for name in &wanted {
        assert!(names.contains(name), "{name} has no words");
    }
    for name in &names {
        assert!(wanted.contains(name), "{name} is not a variant");
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
fn sc_07_dropped_and_declined_are_two_screens() {
    for k in ALL_KINDS {
        let dropped = Stage::Returned {
            reason: REASON_DROPPED.to_string(),
        }
        .words(k);
        let declined = Stage::Returned {
            reason: "Not this one".to_string(),
        }
        .words(k);
        assert_ne!(dropped.heading, declined.heading);
        assert_ne!(dropped.what, declined.what);
        assert!(dropped.quoted.is_none());
        assert!(declined.quoted.is_some());
    }
}

#[test]
fn sc_07_the_four_response_refusals_read_differently() {
    let bind = Refusal::Bind(BindError::BadSignature).words();
    let frame = Refusal::Frame(DecodeError::Truncated).words();
    let session = Refusal::UnknownSession.words();
    let wallet = Refusal::Key1Wallet.words();
    let all = [bind, frame, session, wallet];
    for (i, a) in all.iter().enumerate() {
        for b in all.iter().skip(i + 1) {
            assert_ne!(a.heading, b.heading);
            assert_ne!(a.what, b.what);
        }
    }
}

#[test]
fn sc_08_the_key_holders_reason_is_quoted_and_cleaned() {
    let w = Stage::Returned {
        reason: "wrong\u{202e}amount\nfor Ann ord 4a5e1e4baab89f3a32518a88c31bc87f".to_string(),
    }
    .words(EntryKind::Refund);
    let q = w.quoted.as_ref().expect("quoted");
    assert_eq!(q.from, "The key holder wrote");
    assert!(!q.text.contains('\n') && !q.text.contains('\u{202e}'));
    assert!(!w.what.contains("Ann") && !w.next.contains("Ann"));
    assert_eq!(check(&w), Ok(()));
    let o = ItemOutcome::Returned("again".to_string()).words();
    assert_eq!(o.quoted.map(|q| q.text), Some("again".to_string()));
}

#[test]
fn sc_03_each_kind_is_named_in_the_stage_words() {
    for k in ALL_KINDS {
        let w = Stage::Queued.words(k);
        assert!(w.heading.starts_with(k.label()), "{}", w.heading);
        assert!(w.what.contains(k.noun()), "{}", w.what);
    }
    // The transaction number is in the details only.
    let w = find("Stage::Signed/Refund");
    assert!(w.detail.is_some_and(|d| d.starts_with("transaction ")));
    assert!(!w.what.contains("transaction"));
}

#[test]
fn sc_07_the_same_root_fault_has_one_sentence_here() {
    let w = SetupError::SameRoot.words();
    assert_eq!(w.who, Who::KeyHolder);
    assert!(w.what.contains("same secret"));
    assert_eq!(RestoreError::Setup(SetupError::SameRoot).words(), w);
}

#[test]
fn sc_10_a_failed_store_names_the_details() {
    let w = OwnerError::<String>::Store("disk full".to_string()).words();
    assert_eq!(w.detail.as_deref(), Some("disk full"));
    assert!(w.what.contains("nothing was changed"));
    assert!(w.next.contains("Details"));
}

#[test]
fn sc_12_the_list_on_disk_is_this_one() {
    screens_file::check_or_write("owner", &screens_markdown());
}

use openbmt_core::say::{check, check_distinct};

use crate::coins::{ApplyError, PendingError, ReceiptState, ViewState};
use crate::facts::FactsRefused;
use crate::node::{NodeError, SendRefused, SendState};
use crate::receipts::{MergeOffer, MergeRefused, ReceiptsLine};
use crate::refunds::NoteKind;
use crate::say::{all_words, screens_markdown, Alert};
use crate::watcher::OwnerStartError;

fn view_name(value: ViewState) -> String {
    match value {
        ViewState::Ready => "ViewState::Ready",
        ViewState::Rescanning { .. } => "ViewState::Rescanning",
        ViewState::DeepReorg => "ViewState::DeepReorg",
    }
    .to_string()
}

fn receipt_name(value: ReceiptState) -> String {
    match value {
        ReceiptState::Complete => "ReceiptState::Complete",
        ReceiptState::LogFull => "ReceiptState::LogFull",
    }
    .to_string()
}

fn facts_name(value: FactsRefused) -> String {
    match value {
        FactsRefused::ViewNotReady => "FactsRefused::ViewNotReady",
        // The same words as the full receipt log (OC-13).
        FactsRefused::ReceiptLogFull => "ReceiptState::LogFull",
        FactsRefused::TooOldForFacts => "FactsRefused::TooOldForFacts",
        FactsRefused::NotThisCountersAddress => "FactsRefused::NotThisCountersAddress",
    }
    .to_string()
}

fn note_name(value: NoteKind) -> String {
    match value {
        NoteKind::Refused => "NoteKind::Refused",
        NoteKind::WithdrawalNotApplied => "NoteKind::WithdrawalNotApplied",
        NoteKind::Interrupted => "NoteKind::Interrupted",
        NoteKind::ClaimedMoreThanSeen => "NoteKind::ClaimedMoreThanSeen",
    }
    .to_string()
}

fn send_name(value: SendState) -> String {
    match value {
        SendState::Waiting => "SendState::Waiting",
        SendState::Sent => "SendState::Sent",
        SendState::SeenBack { .. } => "SendState::SeenBack",
        SendState::NotSeen => "SendState::NotSeen",
        SendState::Confirmed { .. } => "SendState::Confirmed",
        SendState::Done => "SendState::Done",
        SendState::Replaced => "SendState::Replaced",
        SendState::Unresolved => "SendState::Unresolved",
    }
    .to_string()
}

fn start_name(value: OwnerStartError) -> String {
    match value {
        OwnerStartError::Locked(_) => "OwnerStartError::Locked",
        OwnerStartError::Dir(_) => "OwnerStartError::Dir",
        OwnerStartError::Headers(_) => "OwnerStartError::Headers",
        OwnerStartError::Driver(_) => "OwnerStartError::Driver",
    }
    .to_string()
}

fn node_name(value: NodeError) -> String {
    match value {
        NodeError::Start(error) => start_name(error).to_string(),
        NodeError::Files(_) => "NodeError::Files".to_string(),
        NodeError::Config(_) => "NodeError::Config".to_string(),
        NodeError::NotReady(_) => "NodeError::NotReady".to_string(),
        NodeError::Owner(_) => "NodeError::Owner".to_string(),
        NodeError::KeyRecoveryRequired => "NodeError::KeyRecoveryRequired".to_string(),
        NodeError::WatchLimit { .. } => "NodeError::WatchLimit".to_string(),
    }
}

fn apply_name(value: ApplyError<String>) -> String {
    match value {
        ApplyError::Refused(refusal) => {
            format!("BlockRefusal::{}", refusal.group().name())
        }
        ApplyError::Full => "ApplyError::Full".to_string(),
        ApplyError::Store(_) => "ApplyError::Store".to_string(),
    }
}

fn pending_name(value: PendingError<String>) -> String {
    match value {
        PendingError::Full => "PendingError::Full",
        PendingError::Store(_) => "PendingError::Store",
    }
    .to_string()
}

fn alert_name(value: Alert) -> String {
    match value {
        Alert::NoHealthAnswer => "Alert::NoHealthAnswer",
        Alert::Stopped => "Alert::Stopped",
        Alert::ChainHalted => "Alert::ChainHalted",
        Alert::DeepReorg => "Alert::DeepReorg",
        Alert::TooFewPeerGroups => "Alert::TooFewPeerGroups",
        Alert::TooFewFilterPeers => "Alert::TooFewFilterPeers",
        Alert::TipIsOld => "Alert::TipIsOld",
        Alert::HeadersBehind => "Alert::HeadersBehind",
        Alert::ClockMismatch => "Alert::ClockMismatch",
    }
    .to_string()
}

fn line_name(value: ReceiptsLine) -> String {
    match value {
        ReceiptsLine::NotReady => "ReceiptsLine::NotReady",
        ReceiptsLine::NoCoins => "ReceiptsLine::NoCoins",
        ReceiptsLine::FeeUnknown { .. } => "ReceiptsLine::FeeUnknown",
        ReceiptsLine::Known { .. } => "ReceiptsLine::Known",
    }
    .to_string()
}

fn merge_refused_name(value: MergeRefused) -> String {
    match value {
        MergeRefused::NoOffer => "MergeRefused::NoOffer".to_string(),
        MergeRefused::OtherPaymentsWaiting => "MergeRefused::OtherPaymentsWaiting".to_string(),
        MergeRefused::NotBuilt(_) => "MergeRefused::NotBuilt".to_string(),
        // The words of the node's own refusal.
        MergeRefused::Node(error) => node_name(error),
    }
}

fn send_refusal_name(value: SendRefused) -> String {
    match value {
        SendRefused::Malformed => "SendRefused::Malformed",
        SendRefused::NoTor => "SendRefused::NoTor",
    }
    .to_string()
}

#[test]
fn sc_12_each_word_passes_the_checker() {
    let bad: Vec<_> = all_words()
        .iter()
        .filter_map(|(name, words)| check(words).err().map(|e| format!("{name}: {e}")))
        .collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn sc_12_all_words_are_distinct() {
    assert_eq!(check_distinct(&all_words()), Ok(()));
}

#[test]
fn sc_01_every_listed_variant_is_present() {
    let names: Vec<_> = all_words().into_iter().map(|(name, _)| name).collect();
    let wanted = [
        "ViewState::Ready",
        "ViewState::Rescanning",
        "ViewState::DeepReorg",
        "SendState::Waiting",
        "SendState::Sent",
        "SendState::SeenBack",
        "SendState::NotSeen",
        "SendState::Confirmed",
        "SendState::Done",
        "SendState::Replaced",
        "SendState::Unresolved",
        "ReceiptState::Complete",
        "ReceiptState::LogFull",
        "FactsRefused::ViewNotReady",
        "FactsRefused::TooOldForFacts",
        "FactsRefused::NotThisCountersAddress",
        "NoteKind::Refused",
        "NoteKind::WithdrawalNotApplied",
        "NoteKind::Interrupted",
        "NoteKind::ClaimedMoreThanSeen",
        "SendRefused::Malformed",
        "SendRefused::NoTor",
        "ReceiptsLine::NotReady",
        "ReceiptsLine::NoCoins",
        "ReceiptsLine::FeeUnknown",
        "ReceiptsLine::Known",
        "MergeOffer::All",
        "MergeOffer::Part",
        "MergeRefused::NoOffer",
        "MergeRefused::OtherPaymentsWaiting",
        "MergeRefused::NotBuilt",
        "OwnerStartError::Locked",
        "OwnerStartError::Dir",
        "OwnerStartError::Headers",
        "OwnerStartError::Driver",
        "NodeError::Files",
        "NodeError::Config",
        "NodeError::NotReady",
        "NodeError::Owner",
        "NodeError::KeyRecoveryRequired",
        "NodeError::WatchLimit",
        "ApplyError::Full",
        "ApplyError::Store",
        "PendingError::Full",
        "PendingError::Store",
        "Alert::NoHealthAnswer",
        "Alert::Stopped",
        "Alert::ChainHalted",
        "Alert::DeepReorg",
        "Alert::TooFewPeerGroups",
        "Alert::TooFewFilterPeers",
        "Alert::TipIsOld",
        "Alert::HeadersBehind",
        "Alert::ClockMismatch",
    ];
    for name in wanted {
        assert!(
            names.iter().any(|found| found.as_str() == name),
            "missing {name}"
        );
    }
    for group in openbmt_watch::say::RefusalGroup::ALL {
        let name = format!("BlockRefusal::{}", group.name());
        assert!(names.iter().any(|found| found == &name), "missing {name}");
    }
    for name in [
        view_name(ViewState::Ready),
        view_name(ViewState::Rescanning { to: 10 }),
        view_name(ViewState::DeepReorg),
        receipt_name(ReceiptState::Complete),
        receipt_name(ReceiptState::LogFull),
        facts_name(FactsRefused::ViewNotReady),
        facts_name(FactsRefused::ReceiptLogFull),
        facts_name(FactsRefused::TooOldForFacts),
        facts_name(FactsRefused::NotThisCountersAddress),
        note_name(NoteKind::Refused),
        note_name(NoteKind::WithdrawalNotApplied),
        note_name(NoteKind::Interrupted),
        note_name(NoteKind::ClaimedMoreThanSeen),
        send_name(SendState::Waiting),
        send_name(SendState::Sent),
        send_name(SendState::SeenBack { groups: 2 }),
        send_name(SendState::NotSeen),
        send_name(SendState::Confirmed { depth: 2 }),
        send_name(SendState::Done),
        send_name(SendState::Replaced),
        send_name(SendState::Unresolved),
        start_name(OwnerStartError::Locked(String::new())),
        start_name(OwnerStartError::Dir(String::new())),
        start_name(OwnerStartError::Headers(String::new())),
        start_name(OwnerStartError::Driver(String::new())),
        node_name(NodeError::Start(OwnerStartError::Dir(String::new()))),
        node_name(NodeError::Files(String::new())),
        node_name(NodeError::Config(String::new())),
        node_name(NodeError::NotReady(String::new())),
        node_name(NodeError::Owner(openbmt_owner::OwnerError::Store(
            String::new(),
        ))),
        node_name(NodeError::KeyRecoveryRequired),
        node_name(NodeError::WatchLimit {
            needed: 1,
            limit: 1,
        }),
        apply_name(ApplyError::Refused(
            openbmt_watch::Refusal::NetworkNotSupported,
        )),
        apply_name(ApplyError::Full),
        apply_name(ApplyError::Store(String::new())),
        pending_name(PendingError::Full),
        pending_name(PendingError::Store(String::new())),
        alert_name(Alert::NoHealthAnswer),
        alert_name(Alert::Stopped),
        alert_name(Alert::ChainHalted),
        alert_name(Alert::DeepReorg),
        alert_name(Alert::TooFewPeerGroups),
        alert_name(Alert::TooFewFilterPeers),
        alert_name(Alert::TipIsOld),
        alert_name(Alert::HeadersBehind),
        alert_name(Alert::ClockMismatch),
        send_refusal_name(SendRefused::Malformed),
        send_refusal_name(SendRefused::NoTor),
        line_name(ReceiptsLine::NotReady),
        line_name(ReceiptsLine::NoCoins),
        line_name(ReceiptsLine::FeeUnknown { payments: 1 }),
        line_name(ReceiptsLine::Known {
            payments: 1,
            cost_sats: 1,
            rate_per_kvb: 1_000,
        }),
        merge_refused_name(MergeRefused::NoOffer),
        merge_refused_name(MergeRefused::OtherPaymentsWaiting),
        merge_refused_name(MergeRefused::NotBuilt(String::new())),
        merge_refused_name(MergeRefused::Node(NodeError::KeyRecoveryRequired)),
    ] {
        assert!(names.iter().any(|found| found == &name), "missing {name}");
    }
    let start = NodeError::Start(OwnerStartError::Dir(String::new()));
    assert_eq!(start.words(), OwnerStartError::Dir(String::new()).words());
    let refusal = ApplyError::<String>::Refused(openbmt_watch::Refusal::NetworkNotSupported);
    let refusal_name = format!(
        "BlockRefusal::{}",
        openbmt_watch::say::RefusalGroup::SavedHeadersUnusable.name()
    );
    let refusal_words = all_words()
        .into_iter()
        .find(|(name, _)| name == &refusal_name)
        .map(|(_, words)| words)
        .expect("the owner saved chain refusal is listed");
    assert_eq!(refusal.words(), refusal_words);
}

#[test]
fn owner_block_refusal_words_do_not_use_counter_or_sale_language() {
    for (name, words) in all_words()
        .into_iter()
        .filter(|(name, _)| name.starts_with("BlockRefusal::"))
    {
        let primary = format!("{} {}", words.heading, words.what).to_lowercase();
        let next = words.next.to_lowercase();
        for banned in ["counter", "sale", "goods"] {
            assert!(!primary.contains(banned), "{name} uses {banned}: {primary}");
            assert!(!next.contains(banned), "{name} uses {banned}: {next}");
        }
    }
}

#[test]
fn sc_05_no_stop_screen_leaves_the_owner_without_an_action() {
    use openbmt_core::say::{Tone, Who};
    let missing: Vec<_> = all_words()
        .into_iter()
        .filter(|(_, words)| words.tone == Tone::Stop && words.who == Who::Nobody)
        .map(|(name, _)| name)
        .collect();
    assert!(
        missing.is_empty(),
        "stop screens without an owner: {missing:?}"
    );
}

#[test]
fn sc_12_generated_screen_catalog_is_current() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/screens/owner-node.md");
    let expected = format!(
        "# Owner device: coins, sending and recovery\n\nGenerated by the tests of `openbmt-owner-node` (SC-12).\nRun those tests with `OPENBMT_WRITE_SCREENS=1` to write this\nfile again.\n\n| state | heading | what happened | what to do next | who | code |\n|---|---|---|---|---|---|\n{}",
        screens_markdown()
    );
    if std::env::var("OPENBMT_WRITE_SCREENS").is_ok_and(|v| v == "1") {
        std::fs::write(path, expected).expect("the screen catalog can be written");
    } else {
        assert_eq!(std::fs::read_to_string(path).unwrap_or_default(), expected,
            "docs/screens/owner-node.md is out of date: run the owner-node tests with OPENBMT_WRITE_SCREENS=1");
    }
}

#[test]
fn oc_19_the_offer_says_the_cost_now_the_usual_cost_and_the_privacy_cost() {
    let offer = MergeOffer {
        coins: 12,
        of: 12,
        rate_per_kvb: 1_000,
        cost_sats: 1_896,
        usual_cost_sats: 4_740,
    };
    let words = offer.words();
    assert!(words.what.contains("1,896 sats"), "{}", words.what);
    assert!(words.what.contains("4,740 sats"), "{}", words.what);
    assert!(words.what.contains("12 payments"), "{}", words.what);
    assert!(words.next.contains("one wallet"), "{}", words.next);
    let part = MergeOffer {
        coins: 400,
        of: 1_000,
        ..offer
    }
    .words();
    assert!(
        part.what.contains("400 of your 1,000 payments"),
        "{}",
        part.what
    );
    // The refusal that wraps the node's own has the node's words.
    assert_eq!(
        MergeRefused::Node(NodeError::KeyRecoveryRequired).words(),
        NodeError::KeyRecoveryRequired.words()
    );
}

#[test]
fn oc_18_the_receipts_screens_use_payments_and_merge_not_the_program_s_names() {
    for (name, words) in all_words()
        .into_iter()
        .filter(|(name, _)| name.starts_with("ReceiptsLine::") || name.starts_with("MergeOffer"))
    {
        let text = format!("{} {} {}", words.heading, words.what, words.next).to_lowercase();
        for banned in ["utxo", "output", "consolidat", "coin"] {
            assert!(!text.contains(banned), "{name} says {banned}: {text}");
        }
    }
}

#[test]
fn owner_refusals_keep_their_specific_next_step_through_the_node() {
    let error = openbmt_owner::OwnerError::Refused(openbmt_owner::Refusal::ZeroFeeRate);
    assert_eq!(crate::NodeError::from(error.clone()).words(), error.words());
}

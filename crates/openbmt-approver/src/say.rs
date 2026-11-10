//! What the key holder is told when the Key app does not open a request,
//! does not take a decision, cannot finish, cannot be set up or cannot take
//! its stored state back (`docs/SCREEN-RULES.md`, SC-01 to SC-12).
//!
//! The `Display` texts of the errors are for logs and tests. A person is
//! shown the [`Words`] these functions return.

use std::fmt;

use openbmt_core::say::{Tone, Who, Words};
use openbmt_session::EncodeError;

use crate::{DecideError, FinishError, RefuseSession, RestoreError, SetupError, StateError};

/// What the key holder does when a request does not come from the owner's
/// phone as it should.
const TAMPERED: &str =
    "Do not approve it, and tell the owner in person, not through the phone that sent it.";

/// What the key holder does when what the Key app saved cannot be used.
const SAVED_STATE: &str =
    "Do not use this saved state, and tell the owner in person; the key can be set up again from its words.";

/// The text the Key app gives when the receive mark is refused where no
/// signer is on the device (`Approver::raise_receive_mark`).
pub const RECEIVE_MARK_REFUSED: &str = "SR-70: the mark only goes up, to at most 2^31";

fn tampered(heading: &str, what: &str) -> Words {
    Words::new(Tone::Stop, heading, what, Who::KeyHolder, TAMPERED)
}

fn saved(heading: &str, what: &str) -> Words {
    Words::new(Tone::Stop, heading, what, Who::KeyHolder, SAVED_STATE)
}

impl RefuseSession {
    /// What the key holder is shown when a request is not opened. Nothing
    /// was changed.
    pub fn words(&self) -> Words {
        match self {
            Self::TooLarge => Words::new(
                Tone::Stop,
                "Request too big",
                "The request file is bigger than this app will read.",
                Who::KeyHolder,
                "Tell the owner in person; the request has to be made smaller.",
            )
            .code("AS-02"),
            Self::Frame(e) => Words::new(
                Tone::Stop,
                "Request not readable",
                "The request is not in the exact form this app reads.",
                Who::KeyHolder,
                "Ask the owner to send it again, and if it fails again, tell the owner in person.",
            )
            .detail(e.to_string())
            .code("AS-03"),
            Self::WrongOwner => tampered(
                "Not from the owner's phone",
                "This request is not from the owner this app was set up with.",
            )
            .code("AS-20"),
            Self::BadSignature => tampered(
                "Signature on request fails",
                "The owner's signature on this request does not check out.",
            )
            .code("AS-20"),
            Self::WrongWallet => tampered(
                "Request for another wallet",
                "This request is for a different wallet than the one this app guards.",
            )
            .code("AS-20"),
            Self::SeenBefore => Words::new(
                Tone::Ended,
                "Request already answered",
                "This request was already answered on this app.",
                Who::Nobody,
                "Nothing to do. The owner can send a new request if it is still needed.",
            )
            .code("AS-22"),
        }
    }
}

impl DecideError {
    /// What the key holder is shown when a decision is not taken. Nothing
    /// was changed.
    pub fn words(&self) -> Words {
        match self {
            Self::Finished => Words::new(
                Tone::Ended,
                "Cannot decide, already done",
                "This request was already finished, so a decision can no longer be taken.",
                Who::Nobody,
                "Nothing to do. Ask the owner for a new request if you need to decide again.",
            )
            .code("AS-34"),
            Self::UnknownItem(n) => Words::new(
                Tone::Stop,
                "No such line",
                "Nothing was changed, because the request has no line with that number.",
                Who::KeyHolder,
                "Choose one of the lines shown on screen; if none works, tell the owner in person.",
            )
            .detail(format!("line {n}"))
            .code("AS-30"),
            Self::CannotApprove(why) => Words::new(
                Tone::Stop,
                "This line cannot be approved",
                "This line failed one of this app's own checks, so it cannot be approved.",
                Who::KeyHolder,
                "Decline it, and tell the owner in person what the line says is wrong.",
            )
            .detail(why.clone())
            .code("AS-23"),
            Self::BadReason => Words::new(
                Tone::Stop,
                "Reason not accepted",
                "The reason is empty, too long or holds characters that cannot be shown.",
                Who::KeyHolder,
                "Type a short plain reason and decline again.",
            )
            .code("AS-30"),
        }
    }
}

/// What the key holder is shown when an answer cannot be built; the cause
/// is the nested error.
fn response_words(e: &EncodeError) -> Words {
    let what = match e {
        EncodeError::TooLarge => "Nothing was sent or saved, because the answer would be too big.",
        EncodeError::ItemCount
        | EncodeError::ItemOrder
        | EncodeError::BadText(_)
        | EncodeError::BadItem(_)
        | EncodeError::BadPayload(_) => {
            "Nothing was sent or saved, because the answer is not in a form that can be sent."
        }
    };
    Words::new(
        Tone::Stop,
        "Answer could not be built",
        what,
        Who::KeyHolder,
        "Approve fewer lines at once and finish again; if it still fails, tell the owner in person.",
    )
    .detail(e.to_string())
    .code("AS-37")
}

impl<E: fmt::Display> FinishError<E> {
    /// What the key holder is shown when the answer is not returned. Nothing
    /// was sent, and nothing this app keeps was changed.
    pub fn words(&self) -> Words {
        match self {
            Self::Finished => Words::new(
                Tone::Ended,
                "Already finished",
                "This request was already finished, so it cannot be finished again.",
                Who::Nobody,
                "Nothing to do. The answer was given the first time.",
            )
            .code("AS-34"),
            Self::Response(e) => response_words(e),
            Self::Store(e) => Words::new(
                Tone::Stop,
                "Answer not saved",
                "Nothing was sent and nothing was changed, because this phone could not save.",
                Who::KeyHolder,
                "Finish again; if it keeps failing, stop and tell the owner in person.",
            )
            .detail(e.to_string())
            .code("AS-34"),
        }
    }
}

impl SetupError {
    /// What the key holder is shown when the Key app cannot be set up.
    /// Nothing was changed.
    pub fn words(&self) -> Words {
        match self {
            Self::Identity(e) => Words::new(
                Tone::Stop,
                "Key not valid",
                "The key this app was given is not a valid key.",
                Who::KeyHolder,
                "Make the key again, or type its words again.",
            )
            .detail(e.to_string())
            .code("AS-38"),
            Self::SameRoot => Words::new(
                Tone::Stop,
                "Same identity as the owner",
                "The owner's identity given to this app is this app's own.",
                Who::KeyHolder,
                "Scan the owner's phone, not this app's own code.",
            )
            .code("AS-38"),
            Self::SignerWallet => tampered_setup(
                "Key 2 is for another wallet",
                "The key 2 held here belongs to a different wallet than this one.",
            ),
            Self::SignerKey => tampered_setup(
                "Key 2 does not fit wallet",
                "The key 2 held here is not the key the wallet lists for it.",
            ),
            Self::SignerAttached => Words::new(
                Tone::Stop,
                "Key 2 is already in use",
                "Nothing was changed, because this app already has key 2 attached to a wallet.",
                Who::KeyHolder,
                "Close this app and open it again; if it happens again, tell the owner in person.",
            )
            .code("AS-36"),
        }
    }
}

/// A setup refusal that shows the wallet is not the one this phone holds
/// key 2 for.
fn tampered_setup(heading: &str, what: &str) -> Words {
    Words::new(
        Tone::Stop,
        heading,
        what,
        Who::KeyHolder,
        "Do not use this wallet, and tell the owner in person.",
    )
    .code("AS-36")
}

impl RestoreError {
    /// What the key holder is shown when what the Key app saved is not
    /// taken back. Nothing was changed.
    pub fn words(&self) -> Words {
        match self {
            Self::Decode(e) => saved(
                "Saved state not readable",
                "Nothing was changed, but what this app saved cannot be read.",
            )
            .detail(e.to_string())
            .code("AS-38"),
            Self::WrongWallet => saved(
                "Saved state for other wallet",
                "What this app saved belongs to a different wallet.",
            )
            .code("AS-38"),
            Self::WrongOwner => saved(
                "Saved state for other owner",
                "What this app saved belongs to a different owner.",
            )
            .code("AS-38"),
            Self::SignerMismatch => saved(
                "Saved state and key 2 differ",
                "What this app saved does not fit whether key 2 is on this phone.",
            )
            .code("AS-38"),
            Self::BadSettings => saved(
                "Saved settings not accepted",
                "The settings this app saved were not agreed to by this app itself.",
            )
            .code("AS-38"),
            Self::Setup(e) => {
                let what = match e {
                    SetupError::Identity(_) => "The saved key is not a valid key.",
                    SetupError::SameRoot => {
                        "The saved owner is the same as this app's own identity."
                    }
                    SetupError::SignerWallet => "The saved key 2 belongs to a different wallet.",
                    SetupError::SignerKey => {
                        "The saved key 2 is not the key the wallet lists for it."
                    }
                    SetupError::SignerAttached => "The saved state attaches key 2 twice.",
                };
                saved("Saved state does not fit", what)
                    .detail(e.to_string())
                    .code("AS-38")
            }
        }
    }
}

impl<E: fmt::Display> StateError<E> {
    /// What the key holder is shown when a change made on this app's own
    /// screen is not taken. Nothing was changed.
    pub fn words(&self) -> Words {
        match self {
            Self::Refused(why) => refused_words(why),
            Self::NoSigner => no_key2_words(),
            Self::Store(e) => Words::new(
                Tone::Stop,
                "Change not saved",
                "Nothing was changed, because this phone could not save.",
                Who::KeyHolder,
                "Try again; if it keeps failing, stop and tell the owner in person.",
            )
            .detail(e.to_string())
            .code("AS-34"),
        }
    }
}

/// The words of the error whose `Display` text is `text`, for a caller that
/// has only the text (the Key app's calls carry it as a string). The errors
/// of the signer are looked up in the signer's own list. `None` for a text
/// of no known kind.
///
/// A saved state that fails its setup reads like the setup error, since
/// the text is the same.
pub fn words_for_text(text: &str) -> Option<Words> {
    let mut fixed: Vec<(String, Words)> = Vec::new();
    for e in refuse_sessions()
        .into_iter()
        .filter(|e| !matches!(e, RefuseSession::Frame(_)))
    {
        fixed.push((e.to_string(), e.words()));
    }
    for e in [DecideError::Finished, DecideError::BadReason] {
        fixed.push((e.to_string(), e.words()));
    }
    let finished = FinishError::<String>::Finished;
    fixed.push((finished.to_string(), finished.words()));
    for e in setup_errors()
        .into_iter()
        .filter(|e| !matches!(e, SetupError::Identity(_)))
    {
        fixed.push((e.to_string(), e.words()));
    }
    for e in restore_errors()
        .into_iter()
        .filter(|e| !matches!(e, RestoreError::Decode(_)))
    {
        fixed.push((e.to_string(), e.words()));
    }
    let no_signer = StateError::<String>::NoSigner;
    fixed.push((no_signer.to_string(), no_signer.words()));
    if let Some((_, words)) = fixed.into_iter().find(|(t, _)| t == text) {
        return Some(words);
    }
    if let Some(rest) = text.strip_prefix("AS-03: the request is not in canonical form: ") {
        let e = RefuseSession::Frame(openbmt_core::wire::DecodeError::Truncated);
        return Some(e.words().detail(rest));
    }
    if let Some(rest) = text.strip_prefix("AS-30: the request has no item ") {
        let n = rest.parse::<u32>().ok()?;
        return Some(DecideError::UnknownItem(n).words());
    }
    if let Some(rest) = text.strip_prefix("AS-23: the line cannot be approved: ") {
        return Some(DecideError::CannotApprove(rest.to_string()).words());
    }
    if let Some(rest) = text.strip_prefix("AS-37: the response cannot be built: ") {
        let cause = if rest.starts_with("AS-02:") {
            EncodeError::TooLarge
        } else {
            EncodeError::ItemCount
        };
        return Some(response_words(&cause).detail(rest));
    }
    if let Some(rest) = text.strip_prefix("AS-38: identity: ") {
        let e = SetupError::Identity(openbmt_core::identity::IdentityError::InvalidSecret);
        return Some(e.words().detail(rest));
    }
    if let Some(rest) = text.strip_prefix("AS-38: the snapshot is not in canonical form: ") {
        let e = RestoreError::Decode(openbmt_core::wire::DecodeError::Truncated);
        return Some(e.words().detail(rest));
    }
    if text == RECEIVE_MARK_REFUSED {
        return Some(refused_words(text));
    }
    openbmt_signer::words_for_text(text)
}

/// The words of the refusal whose text is `why`: a refusal of the signer, the
/// receive mark, or any other text.
fn refused_words(why: &str) -> Words {
    if let Some(words) = openbmt_signer::words_for_text(why) {
        return words;
    }
    if why == RECEIVE_MARK_REFUSED {
        return Words::new(
            Tone::Stop,
            "Address number not accepted",
            "The number must be higher than the one this app has, and not absurdly high.",
            Who::KeyHolder,
            "Check the number with the owner and try again.",
        )
        .code("SR-70");
    }
    Words::new(
        Tone::Stop,
        "Change not accepted",
        "Nothing was changed, because this app refused the change.",
        Who::KeyHolder,
        "Check what was typed and try again; if it is refused again, tell the owner in person.",
    )
    .detail(why)
}

/// Key 2 is not on this phone (AS-36). The one sentence for every way the
/// Key app finds this out.
pub fn no_key2_words() -> Words {
    Words::new(
        Tone::Stop,
        "No key 2 on this phone",
        "This phone does not hold key 2, so it cannot do this.",
        Who::KeyHolder,
        "Use the phone that holds key 2.",
    )
    .code("AS-36")
}

/// A batch of addresses that does not start where the Key app expects the
/// shop's next address to be (AS-26, SR-70).
pub fn receive_range_words() -> Words {
    Words::new(
        Tone::Stop,
        "Address batch out of place",
        "The new addresses do not start where this app expects the shop's next address to be.",
        Who::KeyHolder,
        "If this app is new, open \"Wallet\" and say how far the shop's addresses have got; if not, tell the owner in person.",
    )
    .code("AS-26")
}

/// The words of every error and every state of this crate, one or more
/// for each variant and each nested variant, named after them (SC-12).
pub fn all_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = Vec::new();
    for e in refuse_sessions() {
        all.push((format!("RefuseSession::{}", refuse_name(&e)), e.words()));
    }
    for e in decide_errors() {
        all.push((format!("DecideError::{}", decide_name(&e)), e.words()));
    }
    for e in finish_errors() {
        all.push((format!("FinishError::{}", finish_name(&e)), e.words()));
    }
    for e in setup_errors() {
        all.push((format!("SetupError::{}", setup_name(&e)), e.words()));
    }
    for e in restore_errors() {
        all.push((format!("RestoreError::{}", restore_name(&e)), e.words()));
    }
    for e in setup_errors() {
        all.push((
            format!("RestoreError::Setup({})", setup_name(&e)),
            RestoreError::Setup(e).words(),
        ));
    }
    for e in state_errors() {
        all.push((format!("StateError::{}", state_name(&e)), e.words()));
    }
    all.push((
        "StateError::Refused(ReceiveMark)".to_string(),
        refused_words(RECEIVE_MARK_REFUSED),
    ));
    all.push(("BatchStep::RangeStart".to_string(), receive_range_words()));
    all
}

fn refuse_sessions() -> Vec<RefuseSession> {
    vec![
        RefuseSession::TooLarge,
        RefuseSession::Frame(openbmt_core::wire::DecodeError::Truncated),
        RefuseSession::WrongOwner,
        RefuseSession::BadSignature,
        RefuseSession::WrongWallet,
        RefuseSession::SeenBefore,
    ]
}

fn refuse_name(e: &RefuseSession) -> &'static str {
    match e {
        RefuseSession::TooLarge => "TooLarge",
        RefuseSession::Frame(_) => "Frame",
        RefuseSession::WrongOwner => "WrongOwner",
        RefuseSession::BadSignature => "BadSignature",
        RefuseSession::WrongWallet => "WrongWallet",
        RefuseSession::SeenBefore => "SeenBefore",
    }
}

fn decide_errors() -> Vec<DecideError> {
    vec![
        DecideError::Finished,
        DecideError::UnknownItem(9),
        DecideError::CannotApprove("AS-27: the values do not pass the settings rules".to_string()),
        DecideError::BadReason,
    ]
}

fn decide_name(e: &DecideError) -> &'static str {
    match e {
        DecideError::Finished => "Finished",
        DecideError::UnknownItem(_) => "UnknownItem",
        DecideError::CannotApprove(_) => "CannotApprove",
        DecideError::BadReason => "BadReason",
    }
}

fn finish_errors() -> Vec<FinishError<String>> {
    vec![
        FinishError::Finished,
        FinishError::Response(EncodeError::TooLarge),
        FinishError::Response(EncodeError::ItemCount),
        FinishError::Store("the disk is full".to_string()),
    ]
}

/// The name of a variant; the two answers that cannot be built are told
/// apart by their cause.
fn finish_name(e: &FinishError<String>) -> &'static str {
    match e {
        FinishError::Finished => "Finished",
        FinishError::Response(EncodeError::TooLarge) => "Response(TooLarge)",
        FinishError::Response(
            EncodeError::ItemCount
            | EncodeError::ItemOrder
            | EncodeError::BadText(_)
            | EncodeError::BadItem(_)
            | EncodeError::BadPayload(_),
        ) => "Response(NotSendable)",
        FinishError::Store(_) => "Store",
    }
}

fn setup_errors() -> Vec<SetupError> {
    vec![
        SetupError::Identity(openbmt_core::identity::IdentityError::InvalidSecret),
        SetupError::SameRoot,
        SetupError::SignerWallet,
        SetupError::SignerKey,
        SetupError::SignerAttached,
    ]
}

fn setup_name(e: &SetupError) -> &'static str {
    match e {
        SetupError::Identity(_) => "Identity",
        SetupError::SameRoot => "SameRoot",
        SetupError::SignerWallet => "SignerWallet",
        SetupError::SignerKey => "SignerKey",
        SetupError::SignerAttached => "SignerAttached",
    }
}

fn restore_errors() -> Vec<RestoreError> {
    vec![
        RestoreError::Decode(openbmt_core::wire::DecodeError::Truncated),
        RestoreError::WrongWallet,
        RestoreError::WrongOwner,
        RestoreError::SignerMismatch,
        RestoreError::BadSettings,
    ]
}

fn restore_name(e: &RestoreError) -> &'static str {
    match e {
        RestoreError::Decode(_) => "Decode",
        RestoreError::WrongWallet => "WrongWallet",
        RestoreError::WrongOwner => "WrongOwner",
        RestoreError::SignerMismatch => "SignerMismatch",
        RestoreError::BadSettings => "BadSettings",
        RestoreError::Setup(_) => "Setup",
    }
}

fn state_errors() -> Vec<StateError<String>> {
    vec![
        StateError::Refused("a refusal of some other kind".to_string()),
        StateError::NoSigner,
        StateError::Store("the disk is full".to_string()),
    ]
}

fn state_name(e: &StateError<String>) -> &'static str {
    match e {
        StateError::Refused(_) => "Refused",
        StateError::NoSigner => "NoSigner",
        StateError::Store(_) => "Store",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use openbmt_core::say::check_distinct;
    use openbmt_signer::ChangeRefusal;

    use super::*;

    /// The variant names of `enum_name` in `source`, read from the text.
    fn variants(source: &str, enum_name: &str) -> Vec<String> {
        let head = format!("pub enum {enum_name}");
        let mut inside = false;
        let mut out = Vec::new();
        for line in source.lines() {
            if line.starts_with(&head) {
                inside = true;
            } else if inside && line == "}" {
                break;
            } else if inside && line.starts_with("    ") && !line.starts_with("     ") {
                let rest = line.trim_start();
                if rest.starts_with(|c: char| c.is_ascii_uppercase()) {
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric())
                        .collect();
                    out.push(name);
                }
            }
        }
        out
    }

    fn covered(enum_name: &str, source: &str) {
        let names: BTreeSet<String> = all_words().into_iter().map(|(n, _)| n).collect();
        let found = variants(source, enum_name);
        assert!(!found.is_empty(), "no variants read for {enum_name}");
        for v in found {
            let own = format!("{enum_name}::{v}");
            let nested = format!("{own}(");
            assert!(
                names.iter().any(|n| *n == own || n.starts_with(&nested)),
                "{own} has no words in all_words"
            );
        }
    }

    #[test]
    fn sc_12_every_word_keeps_the_rules() {
        assert_eq!(check_distinct(&all_words()), Ok(()));
    }

    #[test]
    fn sc_01_no_variant_is_missing() {
        let source = include_str!("error.rs");
        for e in [
            "RefuseSession",
            "DecideError",
            "FinishError",
            "SetupError",
            "RestoreError",
            "StateError",
        ] {
            covered(e, source);
        }
        // The nested setup errors are each told on their own too.
        let names: BTreeSet<String> = all_words().into_iter().map(|(n, _)| n).collect();
        for v in variants(source, "SetupError") {
            assert!(names.contains(&format!("RestoreError::Setup({v})")));
        }
        // And the two kinds of answer that cannot be built.
        assert!(names.contains("FinishError::Response(TooLarge)"));
        assert!(names.contains("FinishError::Response(NotSendable)"));
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
    fn a_request_not_from_the_owner_is_stopped_and_told_in_person() {
        for e in [
            RefuseSession::WrongOwner,
            RefuseSession::BadSignature,
            RefuseSession::WrongWallet,
        ] {
            let w = e.words();
            assert_eq!(w.tone, Tone::Stop);
            assert_eq!(w.next, TAMPERED);
            assert_eq!(w.code, Some("AS-20"));
        }
        let a = RefuseSession::WrongOwner.words();
        let b = RefuseSession::WrongWallet.words();
        assert_ne!(a.what, b.what);
    }

    #[test]
    fn the_code_is_the_rule_the_text_starts_with() {
        for e in refuse_sessions() {
            assert!(e.to_string().starts_with(e.words().code.unwrap_or("")));
        }
        for e in decide_errors() {
            assert!(e.to_string().starts_with(e.words().code.unwrap_or("")));
        }
        for e in finish_errors() {
            assert!(e.to_string().starts_with(e.words().code.unwrap_or("")));
        }
        for e in setup_errors() {
            assert!(e.to_string().starts_with(e.words().code.unwrap_or("")));
        }
        for e in restore_errors() {
            assert!(e.to_string().starts_with(e.words().code.unwrap_or("")));
        }
    }

    #[test]
    fn a_store_that_fails_says_nothing_was_sent_or_changed_and_where_the_detail_is() {
        let f = FinishError::Store("the disk is full".to_string()).words();
        assert!(f.what.starts_with("Nothing was sent"));
        assert_eq!(f.detail.as_deref(), Some("the disk is full"));
        let s = StateError::Store("the disk is full".to_string()).words();
        assert!(s.what.starts_with("Nothing was changed"));
        assert_eq!(s.detail.as_deref(), Some("the disk is full"));
    }

    #[test]
    fn a_refusal_of_the_own_screen_is_told_in_the_signers_words() {
        for r in [
            ChangeRefusal::AllowlistFull,
            ChangeRefusal::MarkNotHigher,
            ChangeRefusal::ReceiveMarkTooHigh,
        ] {
            let text = r.to_string();
            assert_eq!(StateError::<String>::Refused(text).words(), r.words());
        }
    }

    #[test]
    fn the_text_the_state_gives_for_the_receive_mark_has_words_of_its_own() {
        let w = StateError::<String>::Refused(RECEIVE_MARK_REFUSED.to_string()).words();
        assert_eq!(w.code, Some("SR-70"));
        assert_eq!(w.heading, "Address number not accepted");
    }

    #[test]
    fn the_receive_range_points_to_the_wallet_screen() {
        let w = receive_range_words();
        assert!(w.next.contains("\"Wallet\""));
        assert!(!w.what.contains("mark"));
    }

    #[test]
    fn the_receive_mark_text_is_the_one_the_state_gives() {
        assert!(include_str!("state.rs").contains(RECEIVE_MARK_REFUSED));
    }

    #[test]
    fn the_text_of_an_error_finds_its_words() {
        let same = |text: String, words: Words| {
            let found = words_for_text(&text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(found.heading, words.heading, "{text}");
            assert_eq!(found.what, words.what, "{text}");
            assert_eq!(found.code, words.code, "{text}");
        };
        for e in refuse_sessions() {
            same(e.to_string(), e.words());
        }
        for e in decide_errors() {
            same(e.to_string(), e.words());
        }
        for e in finish_errors() {
            if !matches!(e, FinishError::Store(_)) {
                same(e.to_string(), e.words());
            }
        }
        for e in setup_errors() {
            same(e.to_string(), e.words());
        }
        for e in restore_errors() {
            same(e.to_string(), e.words());
        }
        // A saved state that fails its setup reads like the setup error.
        for e in setup_errors() {
            same(RestoreError::Setup(e.clone()).to_string(), e.words());
        }
        same(StateError::<String>::NoSigner.to_string(), no_key2_words());
        // The signer's own texts are found through this one.
        for r in [ChangeRefusal::AllowlistFull, ChangeRefusal::MarkTooHigh] {
            same(r.to_string(), r.words());
        }
        assert_eq!(words_for_text("something else"), None);
    }

    #[test]
    fn key_2_missing_is_one_sentence() {
        assert_eq!(StateError::<String>::NoSigner.words(), no_key2_words());
    }
}

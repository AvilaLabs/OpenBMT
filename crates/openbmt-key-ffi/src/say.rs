//! What the key holder is told by the Key app (`docs/SCREEN-RULES.md`,
//! SC-01 to SC-12).
//!
//! The words of the setup state, of a check, of the kit check and of every
//! error that crosses the call protocol. The words of the signer, the
//! session and the approver are in their own crates; [`screens_markdown`]
//! puts all of them in one list, `docs/screens/key.md`.
//!
//! An error of the call protocol has a kind, a rule string and a text.
//! [`words_for`] turns those into [`Words`]. A rule string that has no
//! words of its own here gets the words of its kind, which say that the
//! app does not know the cause, say what is known, and send the person to
//! the owner.

use openbmt_approver::{no_key2_words, words_for_text as approver_words_for_text};
use openbmt_core::say::{Tone, Who, Words};
use openbmt_signer::{wallet_words_for_text, ChangeRefusal};

use crate::app::SetupState;
use crate::error::KeyError;
use crate::kitcheck::KitCheck;
use crate::views::CheckStatus;

/// Which of the four kinds of [`KeyError`] an error is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A rule refused (`KeyError::Refused`).
    Refused,
    /// The storage failed or the stored bytes cannot be read
    /// (`KeyError::Store`).
    Store,
    /// The call is not possible in the state the app is in
    /// (`KeyError::State`).
    State,
    /// A fault of the app itself (`KeyError::Internal`).
    Internal,
}

impl KeyError {
    /// The kind of this error.
    pub fn kind(&self) -> Kind {
        match self {
            Self::Refused { .. } => Kind::Refused,
            Self::Store { .. } => Kind::Store,
            Self::State { .. } => Kind::State,
            Self::Internal { .. } => Kind::Internal,
        }
    }

    /// What the key holder is shown for this error.
    pub fn words(&self) -> Words {
        words_for(self.kind(), self.rule(), self.text())
    }
}

/// The number of a tone in the response frame.
pub(crate) fn tone_byte(tone: Tone) -> u8 {
    match tone {
        Tone::Info => 0,
        Tone::Good => 1,
        Tone::Stop => 2,
        Tone::Ended => 3,
    }
}

/// The number of a person in the response frame.
pub(crate) fn who_byte(who: Who) -> u8 {
    match who {
        Who::Nobody => 0,
        Who::Customer => 1,
        Who::Cashier => 2,
        Who::Manager => 3,
        Who::Owner => 4,
        Who::KeyHolder => 5,
        Who::BoxKeeper => 6,
    }
}

/// Every tone with its name and number, for the protocol description and
/// for the test that compares the Kotlin constants with it.
pub(crate) const TONES: [(Tone, &str, u8); 4] = [
    (Tone::Info, "Info", 0),
    (Tone::Good, "Good", 1),
    (Tone::Stop, "Stop", 2),
    (Tone::Ended, "Ended", 3),
];

/// Every person with its name and number.
pub(crate) const WHOS: [(Who, &str, u8); 7] = [
    (Who::Nobody, "Nobody", 0),
    (Who::Customer, "Customer", 1),
    (Who::Cashier, "Cashier", 2),
    (Who::Manager, "Manager", 3),
    (Who::Owner, "Owner", 4),
    (Who::KeyHolder, "KeyHolder", 5),
    (Who::BoxKeeper, "BoxKeeper", 6),
];

/// What the key holder does when the app's own work failed.
const FAULT: &str =
    "Try again, and if it happens again, stop and tell the owner in person; see Details.";

/// What the key holder does when what the app saved cannot be used.
const SAVED_STATE: &str =
    "Do not use this saved state, and tell the owner in person; the key can be set up again from its words.";

/// Words for a refused call that is the fault of the call, not of the
/// person: nothing was signed and nothing was changed.
fn call_fault(heading: &str, what: &str, text: &str) -> Words {
    Words::new(Tone::Stop, heading, what, Who::KeyHolder, FAULT).detail(text)
}

/// A scan or a typed value the app did not take; the person types or scans
/// again.
fn retry(heading: &str, what: &str, next: &str, text: &str) -> Words {
    Words::new(Tone::Stop, heading, what, Who::KeyHolder, next).detail(text)
}

/// A refusal involving data supplied by another device. The diagnostic is
/// kept in `KeyError`, while the screen avoids echoing that data as detail.
fn retry_input(heading: &str, what: &str, next: &str) -> Words {
    Words::new(Tone::Stop, heading, what, Who::KeyHolder, next)
}

fn without_detail(mut words: Words) -> Words {
    words.detail = None;
    words
}

/// The words of a call that the app refused for a scanned payment code.
fn code_words(rule: &str, text: &str) -> Option<Words> {
    let again = "Scan the code again, or ask the owner for another one.";
    Some(match rule {
        "deposit-too-long" => retry(
            "Payment code too long",
            "The scanned code is too long to be a payment code.",
            again,
            text,
        ),
        "deposit-chars" => retry(
            "Odd characters in the code",
            "The code holds spaces, hidden or unusual characters that a payment code does not carry.",
            again,
            text,
        ),
        "deposit-syntax" => retry(
            "Payment code not understood",
            "The part of the code after the address is written wrongly.",
            again,
            text,
        ),
        "deposit-duplicate" => retry(
            "Code repeats a part",
            "A part of the code is given more than once.",
            "Ask the owner for a new code.",
            text,
        ),
        "deposit-required" => retry(
            "Code needs something else",
            "The code requires something this app does not understand, so it is refused.",
            "Use the plain address, or ask the owner for another code.",
            text,
        ),
        "deposit-label" => retry(
            "Label has hidden characters",
            "The label in the code holds characters that cannot be shown.",
            "Type a name yourself, or ask the owner for another code.",
            text,
        ),
        "deposit-amount" => retry(
            "Amount in code not usable",
            "The amount in the code is not a number of bitcoin this app can read.",
            "Use the plain address, or ask the owner for another code.",
            text,
        ),
        "deposit-escape" => retry(
            "Code has a broken part",
            "A part of the code is cut off or written wrongly.",
            again,
            text,
        ),
        _ => return None,
    })
}

/// The words of a call that the app refused for typed words or throws.
fn typed_words(rule: &str, text: &str) -> Option<Words> {
    Some(match rule {
        "words-count" => retry(
            "Wrong number of words",
            "A key has 12 or 24 words, and the number typed is neither.",
            "Type all 12 words or all 24 words.",
            "",
        ),
        "words-unknown" => unknown_word_words(text),
        "words-checksum" => retry(
            "Words do not fit together",
            "The typed words do not fit together: one is wrong or out of order.",
            "Check each word against the paper, in order, and type them again.",
            "",
        ),
        "entropy" => entropy_words(text),
        _ => return None,
    })
}

/// The parser reports only a word position. Retain that safe number for the
/// promised Details line, but accept no other text from this error field.
fn unknown_word_words(text: &str) -> Words {
    const PREFIX: &str = "word ";
    const SUFFIX: &str = " is not on the word list; check how it is spelled";
    let position = text
        .strip_prefix(PREFIX)
        .and_then(|rest| rest.strip_suffix(SUFFIX))
        .and_then(|digits| digits.parse::<u8>().ok())
        .filter(|n| (1..=24).contains(n))
        .filter(|n| text == format!("word {n}{SUFFIX}"));
    let next = if position.is_some() {
        "Check how it is spelled; its place in the list of words is under Details."
    } else {
        "Check how each word is spelled, then type the words again."
    };
    let mut words = retry_input(
        "Word not on the list",
        "One of the typed words is not on the word list.",
        next,
    );
    if let Some(position) = position {
        words.detail = Some(format!("word position {position}"));
    }
    words
}

/// The words of a refusal of dice or coin throws, from the text of the
/// refusal.
fn entropy_words(text: &str) -> Words {
    if text.contains("is not a die face") {
        retry(
            "Not a die face",
            "A character typed is not a die face; only 1 to 6 are used.",
            "Remove it, and type only the numbers 1 to 6.",
            "",
        )
    } else if text.contains("rolls is too few") {
        retry(
            "Too few rolls",
            "Not enough rolls were typed to make a key.",
            "Roll the die more times, and type all the rolls.",
            "",
        )
    } else if text.contains("throws is too many") {
        retry(
            "Too many throws",
            "More throws were typed than this app takes in one go.",
            "Type fewer throws and try again.",
            "",
        )
    } else if text.contains("bits is too few") {
        retry(
            "Too few throws",
            "Not enough throws were typed to make this many words.",
            "Keep throwing until the app shows enough, then go on.",
            "",
        )
    } else if text.contains("could not be made") {
        call_fault(
            "No words were made",
            "Nothing was made, because this app could not turn the throws into words.",
            "",
        )
    } else {
        retry(
            "Throws not accepted",
            "This app did not accept the throws it was given.",
            "Check what was typed and try again.",
            "",
        )
    }
}

/// The words of a call that the program's own plumbing refused. Nothing was
/// signed and nothing was changed.
fn call_words(rule: &str, text: &str) -> Option<Words> {
    Some(match rule {
        "request-too-long" => call_fault(
            "Call too big",
            "Nothing was signed or changed, because the app sent a call that is too big.",
            text,
        ),
        "request-magic" => call_fault(
            "Call not recognised",
            "Nothing was signed or changed, because the app sent a call this part does not know.",
            text,
        ),
        "request-version" => call_fault(
            "Call of an unknown version",
            "Nothing was signed or changed, because the app sent a call of a version not read.",
            text,
        ),
        "op-unknown" => call_fault(
            "Unknown call",
            "Nothing was signed or changed, because the app asked for something this part cannot do.",
            text,
        ),
        "request-malformed" => call_fault(
            "Call not readable",
            "Nothing was signed or changed, because the app sent a call that cannot be read.",
            text,
        ),
        "request-null" => call_fault(
            "Call was empty",
            "Nothing was signed or changed, because the app sent no call at all.",
            text,
        ),
        "request-length" => call_fault(
            "Call has no length",
            "Nothing was signed or changed, because the size of the call could not be read.",
            text,
        ),
        "request-read" => call_fault(
            "Call could not be read",
            "Nothing was signed or changed, because the call could not be read from the app.",
            text,
        ),
        "handle-unknown" => call_fault(
            "Object not open",
            "Nothing was signed or changed, because the part the call names is not open.",
            text,
        ),
        "handle-unexpected" => call_fault(
            "Call names a part by mistake",
            "Nothing was signed or changed, because the call names a part it does not use.",
            text,
        ),
        "busy" => Words::new(
            Tone::Info,
            "Another call is running",
            "Nothing was signed or changed, because the app was still busy with another call.",
            Who::KeyHolder,
            "Wait a moment and try again.",
        )
        .detail(text),
        "handles-used" => call_fault(
            "Too many parts open",
            "Nothing was signed or changed, because too many parts of the app are open.",
            text,
        ),
        "app-limit" => Words::new(
            Tone::Stop,
            "Too many apps open",
            "Nothing was signed or changed, because four apps are already open.",
            Who::KeyHolder,
            "Close this app and open it again; if it happens again, tell the owner in person.",
        )
        .detail(text),
        _ => return None,
    })
}

/// The words of a call that is not possible in the state the app is in.
fn state_words(rule: &str, text: &str) -> Option<Words> {
    Some(match rule {
        "session-open" => Words::new(
            Tone::Info,
            "A request is open",
            "A request is still open, so this cannot be done now.",
            Who::KeyHolder,
            "Finish or cancel the open request first.",
        )
        .detail(text),
        "session-closed" => Words::new(
            Tone::Ended,
            "Request is closed",
            "This request is no longer open.",
            Who::KeyHolder,
            "Open the owner's request again.",
        )
        .detail(text),
        "not-ready" => Words::new(
            Tone::Info,
            "No wallet yet",
            "This app has no wallet yet, so it cannot do this.",
            Who::KeyHolder,
            "Finish the setup first: accept the wallet from the owner's phone.",
        )
        .detail(text),
        "wallet-pending" => Words::new(
            Tone::Info,
            "Wallet not confirmed yet",
            "A wallet is waiting for the key holder's check, so this cannot be done yet.",
            Who::KeyHolder,
            "Confirm that its words match the owner's phone, or discard it, then try again.",
        )
        .detail(text),
        "nothing-pending" => Words::new(
            Tone::Ended,
            "No wallet to confirm",
            "No wallet is waiting for the key holder's check, so there is nothing to confirm or discard.",
            Who::KeyHolder,
            "Scan the wallet code on the owner's phone first.",
        )
        .detail(text),
        "wallet-set" => Words::new(
            Tone::Ended,
            "Wallet already accepted",
            "The wallet is already accepted, so this cannot be done again.",
            Who::Nobody,
            "Nothing to do. For a different wallet, use \"Move to a new wallet\".",
        )
        .detail(text),
        "empty" => empty_words().detail(text),
        "words-shown-once" => Words::new(
            Tone::Ended,
            "Words already shown",
            "The words were shown once and are not kept, so they cannot be shown again.",
            Who::KeyHolder,
            "Use the paper you wrote them on; if it is lost, tell the owner in person.",
        )
        .detail(text),
        _ => return None,
    })
}

/// The app has no key yet: for the setup state and for a call that needs
/// the key.
fn empty_words() -> Words {
    Words::new(
        Tone::Info,
        "No key yet",
        "This app has no key yet.",
        Who::KeyHolder,
        "Make the key, or type the words of an existing key.",
    )
}

/// The saved state of the app cannot be read or used.
fn saved_state_words(text: &str) -> Words {
    Words::new(
        Tone::Stop,
        "Stored bytes not readable",
        "Nothing was changed, but the bytes this app stored cannot be read.",
        Who::KeyHolder,
        SAVED_STATE,
    )
    .detail(text)
}

/// The words of a refusal that is neither a scan, typed words nor the call
/// itself: a key or wallet the app was given.
fn given_words(rule: &str, text: &str) -> Option<Words> {
    Some(match rule {
        "xpub" => retry_input(
            "Key text not understood",
            "The key text given for key 3 is not a key this app can read.",
            "Scan the key again from the device that made it.",
        ),
        "network" => Words::new(
            Tone::Stop,
            "Key 2 for another network",
            "Key 2 on this phone was made for a different network than the wallet.",
            Who::KeyHolder,
            "Do not use this wallet, and tell the owner in person.",
        )
        .detail(text),
        "owner" => retry_input(
            "Owner key not accepted",
            "The owner's key given to this app is not a key it can use.",
            "Scan the owner's phone again.",
        ),
        "wallet" => without_detail(wallet_words_for_text(text)),
        "kit-too-long" => retry_input(
            "Scanned text too long",
            "The scanned text is too long to be a wallet.",
            "Scan the wallet code on the kit again.",
        ),
        "address" => {
            if text.contains("another network") {
                retry_input(
                    "Address for another network",
                    "This address is for a different network than this wallet.",
                    "Check with the owner that it is the right address.",
                )
            } else {
                retry_input(
                    "Not an address",
                    "What was scanned or typed is not a bitcoin address.",
                    "Scan the address again, or ask the owner for it.",
                )
            }
        }
        "address-kind" => ChangeRefusal::ScriptKind.words(),
        "AS-36" => no_key2_words(),
        "OBM7" => saved_state_words(text),
        _ => return None,
    })
}

/// The words of a store that failed or could not be read.
fn store_words(rule: &str, text: &str) -> Option<Words> {
    Some(match rule {
        "AS-34" => Words::new(
            Tone::Stop,
            "Nothing was saved",
            "Nothing was changed or sent, because this phone's storage did not work.",
            Who::KeyHolder,
            "Try again; if it keeps failing, stop and tell the owner in person; see Details.",
        )
        .detail(
            text.strip_prefix("AS-34: could not store the new state: ")
                .unwrap_or(text),
        )
        .code("AS-34"),
        "OBM7" => saved_state_words(text),
        "random" => Words::new(
            Tone::Stop,
            "Random bytes not usable",
            "Nothing was made, because the phone's random source gave nothing this app can use.",
            Who::KeyHolder,
            FAULT,
        )
        .detail(text),
        "load" => Words::new(
            Tone::Stop,
            "Saved state not read",
            "Nothing was changed, because the phone could not read what this app saved.",
            Who::KeyHolder,
            "Open the app again; if it keeps failing, tell the owner in person; see Details.",
        )
        .detail(text),
        "store" => call_fault(
            "App storage not usable",
            "Nothing was changed, because the app's storage part lacks a method this app needs.",
            text,
        ),
        _ => return None,
    })
}

/// The words of a fault of the app itself, from its text. Only what is
/// known is said: a panic can come after the new state was saved.
fn internal_known(text: &str) -> Option<Words> {
    let known: [(&str, Words); 11] = [
        (
            "caught a panic: ",
            Words::new(
                Tone::Stop,
                "This app stopped itself",
                "This app stopped itself and gave no answer, so nothing was sent to the owner's phone.",
                Who::KeyHolder,
                "Close the app and open it again; if it happens again, stop and tell the owner in person.",
            ),
        ),
        (
            "an earlier panic poisoned the Key app",
            Words::new(
                Tone::Stop,
                "This app must be reopened",
                "Nothing was signed, because an earlier fault stopped this app until it is opened again.",
                Who::KeyHolder,
                "Close the app and open it again; it starts from what it saved.",
            ),
        ),
        (
            "an earlier panic poisoned this session",
            Words::new(
                Tone::Stop,
                "Request must be reopened",
                "Nothing was signed, because an earlier fault stopped this request.",
                Who::KeyHolder,
                "Close the request and open the owner's request again.",
            ),
        ),
        (
            "the phase changed during setup",
            Words::new(
                Tone::Stop,
                "Setup did not finish",
                "The wallet was saved, but this app could not finish its setup.",
                Who::KeyHolder,
                "Close the app and open it again; it starts from what it saved.",
            ),
        ),
        (
            "the session has no wallet",
            Words::new(
                Tone::Stop,
                "Request has no wallet",
                "Nothing was signed, because this request has no wallet to work with.",
                Who::KeyHolder,
                FAULT,
            ),
        ),
        (
            "the op has no app",
            Words::new(
                Tone::Stop,
                "Call has no app",
                "Nothing was signed or changed, because the call has no app to work on.",
                Who::KeyHolder,
                FAULT,
            ),
        ),
        (
            "the op has no session",
            Words::new(
                Tone::Stop,
                "Call has no request",
                "Nothing was signed or changed, because the call has no request to work on.",
                Who::KeyHolder,
                FAULT,
            ),
        ),
        (
            "random: count",
            Words::new(
                Tone::Stop,
                "Random bytes not counted",
                "Nothing was made, because this app could not count the random bytes it asked for.",
                Who::KeyHolder,
                FAULT,
            ),
        ),
        (
            "random: short",
            Words::new(
                Tone::Stop,
                "Random bytes too few",
                "Nothing was made, because the phone gave fewer random bytes than were asked for.",
                Who::KeyHolder,
                FAULT,
            ),
        ),
        (
            "a decision taken before was refused now: ",
            Words::new(
                Tone::Stop,
                "Earlier decision refused",
                "Nothing was signed, because a decision taken before is no longer accepted.",
                Who::KeyHolder,
                "Open the owner's request again and decide again; if it keeps happening, tell the owner.",
            ),
        ),
        (
            "not a valid secret key",
            Words::new(
                Tone::Stop,
                "Saved key not usable",
                "Nothing was changed, but the key this app saved is not a key it can use.",
                Who::KeyHolder,
                SAVED_STATE,
            ),
        ),
    ];
    known
        .into_iter()
        .find(|(prefix, _)| text.starts_with(prefix))
        .map(|(prefix, words)| {
            if prefix == "caught a panic: " {
                // A panic payload is arbitrary program data and can contain
                // secrets. Keep it in KeyError for diagnostics, not on-screen.
                words
            } else {
                words.detail(text.strip_prefix(prefix).unwrap_or(text))
            }
        })
}

/// The words of a refusal that came from a rule crate and carries its rule
/// in its text (`AS-20`, `SR-30`): looked up by the text.
fn typed_text_words(text: &str) -> Option<Words> {
    if text == "owner and Key app roots must differ" {
        return approver_words_for_text("AS-38: the owner root is the Key app's own root");
    }
    approver_words_for_text(text)
}

/// The words the app has of its own for this error, or `None` when the rule
/// string and the text are of no known kind.
pub(crate) fn own_words(kind: Kind, rule: &str, text: &str) -> Option<Words> {
    let words = match kind {
        Kind::Refused => code_words(rule, text)
            .or_else(|| typed_words(rule, text))
            .or_else(|| call_words(rule, text))
            .or_else(|| given_words(rule, text))
            .or_else(|| typed_text_words(text)),
        Kind::Store => store_words(rule, text),
        Kind::State => state_words(rule, text).or_else(|| call_words(rule, text)),
        Kind::Internal => internal_known(text),
    };
    // No text, no detail: the small print is for what the system said.
    words.map(|mut w| {
        if w.detail.as_deref() == Some("") {
            w.detail = None;
        }
        w
    })
}

/// The words for an error of this kind, rule string and text. A rule the
/// app has no words for gets the words of its kind, which are a valid
/// screen: they say what is known, and send the person to the owner.
pub fn words_for(kind: Kind, rule: &str, text: &str) -> Words {
    own_words(kind, rule, text).unwrap_or_else(|| fallback(kind, rule, text))
}

/// The words for an error that has none of its own.
fn fallback(kind: Kind, rule: &str, _text: &str) -> Words {
    // Unknown error text can include bytes originating in a seed, key, scan,
    // or request. Keep it in KeyError for diagnostics, but never put it on a
    // person's screen without a source-specific sanitization rule.
    let detail = rule.to_string();
    let (heading, what) = match kind {
        Kind::Refused => (
            "This app refused the call",
            "This app refused the call for a reason it cannot put in words, and nothing was changed.",
        ),
        Kind::Store => (
            "Saving did not work",
            "Nothing was changed, because the phone's storage did not work as this app needs.",
        ),
        Kind::State => (
            "Not possible right now",
            "The call cannot be done in the state this app is in, and nothing was changed.",
        ),
        Kind::Internal => (
            "This app has a fault",
            "This app stopped itself and gave no answer, so nothing was sent to the owner's phone.",
        ),
    };
    Words::new(Tone::Stop, heading, what, Who::KeyHolder, FAULT).detail(detail)
}

impl SetupState {
    /// What the key holder is shown for this state of the setup.
    pub fn words(&self) -> Words {
        match self {
            Self::Empty => empty_words(),
            Self::HasIdentity {
                holds_key2: _,
                words_waiting: true,
            } => Words::new(
                Tone::Info,
                "Write the words down",
                "The words of this key are ready, and they are shown only once.",
                Who::KeyHolder,
                "Write them on paper now; they cannot be shown again.",
            ),
            Self::HasIdentity {
                holds_key2: true,
                words_waiting: false,
            } => Words::new(
                Tone::Info,
                "Waiting for the wallet",
                "This app holds key 2 and is waiting for the wallet from the owner's phone.",
                Who::KeyHolder,
                "Give the owner's phone this app's key details, then accept the wallet it makes.",
            ),
            Self::HasIdentity {
                holds_key2: false,
                words_waiting: false,
            } => Words::new(
                Tone::Info,
                "Waiting, without key 2",
                "This app has its own identity but not key 2, and is waiting for the wallet.",
                Who::KeyHolder,
                "Give the owner's phone this app's identity, then accept the wallet it makes.",
            ),
            Self::Ready { holds_key2: true } => Words::new(
                Tone::Good,
                "Ready to approve",
                "The wallet is accepted, and this app can answer the owner's requests.",
                Who::Nobody,
                "Nothing to do. It waits for a request from the owner's phone.",
            ),
            Self::Ready { holds_key2: false } => Words::new(
                Tone::Info,
                "Ready, without key 2",
                "The wallet is accepted, but this phone does not hold key 2.",
                Who::Nobody,
                "Nothing to do for payments here. This app answers other requests only.",
            ),
            Self::WalletPending { holds_key2: _ } => Words::new(
                Tone::Info,
                "Wallet waiting for you",
                "The wallet from the owner's phone is held but not registered, so nothing can be signed yet.",
                Who::KeyHolder,
                "Compare its words and fingerprints with the owner's phone, then confirm or discard it.",
            ),
        }
    }
}

impl CheckStatus {
    /// What the key holder is shown for one check of a line.
    pub fn words(&self) -> Words {
        match self {
            Self::Passed => Words::new(
                Tone::Good,
                "Check passed",
                "This check of the line passed.",
                Who::Nobody,
                "Nothing to do for this check.",
            ),
            Self::Failed => Words::new(
                Tone::Stop,
                "Check not passed",
                "This check of the line did not pass, so the line cannot be approved.",
                Who::KeyHolder,
                "Decline the line, and tell the owner in person which check it was.",
            ),
            Self::NotChecked => Words::new(
                Tone::Ended,
                "Check not run",
                "An earlier check did not pass, so this one was not run.",
                Who::KeyHolder,
                "Look at the first check that did not pass.",
            ),
        }
    }
}

impl KitCheck {
    /// What the key holder is shown for the check of the printed kit.
    pub fn words(&self) -> Words {
        match self {
            Self::Same => Words::new(
                Tone::Good,
                "Kit matches this phone",
                "The wallet on the paper is the same as the wallet on this phone.",
                Who::Nobody,
                "Nothing to do. Only the wallet is compared, not the other lines on the paper.",
            ),
            Self::Different {
                first_difference,
                same_checksum_words,
                parses,
            } => {
                let words = match (*same_checksum_words, *parses) {
                    (true, _) => Words::new(
                        Tone::Stop,
                        "Same check words, other keys",
                        "The paper shows the same four words but another wallet, which should not happen by accident.",
                        Who::KeyHolder,
                        "Stop, do not use this kit, and tell the owner in person.",
                    ),
                    (false, false) => Words::new(
                        Tone::Stop,
                        "Paper is not a wallet",
                        "The text scanned from the paper is not a wallet this app can read.",
                        Who::KeyHolder,
                        "Scan the wallet code again; if it still differs, do not use this kit and print a new one.",
                    ),
                    (false, true) => Words::new(
                        Tone::Stop,
                        "Kit is for another wallet",
                        "The paper and this phone do not describe the same wallet.",
                        Who::KeyHolder,
                        "Do not use this kit to recover; print a new kit from the owner's phone and check it.",
                    ),
                };
                words.detail(format!("first difference at position {first_difference}"))
            }
        }
    }
}

/// The words of every state and every error of this crate, one or more
/// for each variant and each nested variant, named after them (SC-12). The
/// words of the other crates of the Key app are in their own lists.
pub fn all_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = Vec::new();
    for (name, state) in setup_states() {
        all.push((format!("SetupState::{name}"), state.words()));
    }
    for s in [
        CheckStatus::Passed,
        CheckStatus::Failed,
        CheckStatus::NotChecked,
    ] {
        all.push((format!("CheckStatus::{}", check_name(s)), s.words()));
    }
    for (name, kit) in kit_checks() {
        all.push((format!("KitCheck::{name}"), kit.words()));
    }
    for (kind, rule, text) in samples() {
        all.push((
            format!("KeyError::{}({rule}, {})", kind_name(kind), short(text)),
            words_for(kind, rule, text),
        ));
    }
    for kind in [Kind::Refused, Kind::Store, Kind::State, Kind::Internal] {
        all.push((
            format!("KeyError::{}(unknown rule)", kind_name(kind)),
            fallback(kind, "a-rule-nobody-wrote", "a text nobody wrote"),
        ));
    }
    all
}

/// The start of a text, to name an entry.
fn short(text: &str) -> String {
    text.chars().take(24).collect()
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Refused => "Refused",
        Kind::Store => "Store",
        Kind::State => "State",
        Kind::Internal => "Internal",
    }
}

fn check_name(s: CheckStatus) -> &'static str {
    match s {
        CheckStatus::Passed => "Passed",
        CheckStatus::Failed => "Failed",
        CheckStatus::NotChecked => "NotChecked",
    }
}

/// The states of the setup, named, with the combination that cannot be
/// reached (words waiting without key 2) left out.
fn setup_states() -> Vec<(&'static str, SetupState)> {
    vec![
        ("Empty", SetupState::Empty),
        (
            "HasIdentity(WordsWaiting)",
            SetupState::HasIdentity {
                holds_key2: true,
                words_waiting: true,
            },
        ),
        (
            "HasIdentity(Key2)",
            SetupState::HasIdentity {
                holds_key2: true,
                words_waiting: false,
            },
        ),
        (
            "HasIdentity(NoKey2)",
            SetupState::HasIdentity {
                holds_key2: false,
                words_waiting: false,
            },
        ),
        ("Ready(Key2)", SetupState::Ready { holds_key2: true }),
        ("Ready(NoKey2)", SetupState::Ready { holds_key2: false }),
        (
            "WalletPending",
            SetupState::WalletPending { holds_key2: true },
        ),
    ]
}

/// The name of a state, by an exhaustive match: a new variant does not
/// compile until it is named here (SC-01).
#[cfg(test)]
fn setup_name(s: &SetupState) -> &'static str {
    match s {
        SetupState::Empty => "Empty",
        SetupState::HasIdentity { .. } => "HasIdentity",
        SetupState::Ready { .. } => "Ready",
        SetupState::WalletPending { .. } => "WalletPending",
    }
}

fn kit_checks() -> Vec<(&'static str, KitCheck)> {
    vec![
        ("Same", KitCheck::Same),
        (
            "Different(SameCheckWords)",
            KitCheck::Different {
                first_difference: 40,
                same_checksum_words: true,
                parses: true,
            },
        ),
        (
            "Different(NotAWallet)",
            KitCheck::Different {
                first_difference: 0,
                same_checksum_words: false,
                parses: false,
            },
        ),
        (
            "Different(OtherWallet)",
            KitCheck::Different {
                first_difference: 40,
                same_checksum_words: false,
                parses: true,
            },
        ),
    ]
}

#[cfg(test)]
fn kit_name(k: &KitCheck) -> &'static str {
    match k {
        KitCheck::Same => "Same",
        KitCheck::Different { .. } => "Different",
    }
}

/// One sample of every error of the call protocol that has words of its
/// own here, as (kind, rule string, text). The rules that are the words of
/// another crate (`AS-36`, `address-kind`, `wallet`, `empty`) are in that
/// crate's list or in the setup states.
pub(crate) fn samples() -> Vec<(Kind, &'static str, &'static str)> {
    use Kind::{Internal, Refused, State, Store};
    vec![
        (Refused, "address", "not an address: bad character"),
        (
            Refused,
            "address",
            "the address is for another network: wrong prefix",
        ),
        (Refused, "deposit-too-long", ""),
        (Refused, "deposit-chars", ""),
        (Refused, "deposit-syntax", "a parameter has no '='"),
        (Refused, "deposit-duplicate", ""),
        (Refused, "deposit-required", ""),
        (Refused, "deposit-label", ""),
        (Refused, "deposit-amount", ""),
        (Refused, "deposit-escape", ""),
        (Refused, "xpub", "not an extended public key: bad"),
        (Refused, "network", ""),
        (Refused, "owner", "the owner root is not 32 bytes"),
        (
            Refused,
            "entropy",
            "character 3 is not a die face (use only 1 to 6)",
        ),
        (
            Refused,
            "entropy",
            "5 rolls is too few, at least 50 are needed",
        ),
        (
            Refused,
            "entropy",
            "5000 throws is too many, the most allowed is 1000",
        ),
        (Refused, "entropy", "100 bits is too few, 128 are needed"),
        (Refused, "entropy", "the words could not be made"),
        (Refused, "entropy", "something else"),
        (
            Refused,
            "words-count",
            "5 words were typed; a key has 12 or 24",
        ),
        (
            Refused,
            "words-unknown",
            "word 5 is not on the word list; check how it is spelled",
        ),
        (Refused, "words-checksum", ""),
        (Refused, "kit-too-long", ""),
        (
            Refused,
            "request-too-long",
            "a request is at most 1048576 bytes",
        ),
        (Refused, "request-magic", ""),
        (Refused, "request-version", ""),
        (Refused, "op-unknown", "99 is not the number of an op"),
        (Refused, "request-null", ""),
        (Refused, "request-length", ""),
        (Refused, "request-read", ""),
        (Refused, "request-malformed", ""),
        (Refused, "handle-unknown", ""),
        (Refused, "handle-unexpected", ""),
        (Refused, "busy", ""),
        (State, "session-open", ""),
        (State, "session-closed", ""),
        (State, "not-ready", ""),
        (State, "wallet-set", ""),
        (State, "wallet-pending", ""),
        (State, "nothing-pending", ""),
        (State, "words-shown-once", ""),
        (State, "handles-used", ""),
        (State, "app-limit", ""),
        (
            Store,
            "AS-34",
            "AS-34: could not store the new state: disk full",
        ),
        (Store, "OBM7", "OBM7: the stored state is not readable: cut"),
        (Store, "random", "the platform gave 3 random bytes, not 32"),
        (Store, "load", "could not read the stored state: locked"),
        (Store, "store", "the app's store object has no usable class"),
        (Internal, "internal", "caught a panic: index out of range"),
        (
            Internal,
            "internal",
            "an earlier panic poisoned the Key app: open it again from its stored bytes",
        ),
        (
            Internal,
            "internal",
            "an earlier panic poisoned this session",
        ),
        (Internal, "internal", "the phase changed during setup"),
        (Internal, "internal", "the session has no wallet"),
        (Internal, "internal", "the op has no app"),
        (Internal, "internal", "the op has no session"),
        (Internal, "internal", "random: count"),
        (Internal, "internal", "random: short"),
        (
            Internal,
            "internal",
            "a decision taken before was refused now: gone",
        ),
        (Internal, "internal", "not a valid secret key"),
    ]
}

/// The list of every screen of the Key app, as a Markdown table: the
/// signer, the session, the approver and this crate (SC-12).
pub fn screens_markdown() -> String {
    let mut all: Vec<(String, Words)> = openbmt_signer::all_words();
    all.extend(openbmt_session::all_words());
    all.extend(openbmt_approver::all_words());
    all.extend(all_words());
    let mut s = String::new();
    s.push_str("# Key app screens\n\n");
    s.push_str("Every state and every refusal of the Key app, with the words the key\n");
    s.push_str("holder is shown (`docs/SCREEN-RULES.md`). Generated by a test of\n");
    s.push_str("`openbmt-key-ffi`; run the test `screens_list` with\n");
    s.push_str("`OPENBMT_WRITE_SCREENS=1` to write it again. Do not edit it by hand.\n\n");
    s.push_str("| state | heading | what happened | what to do next | who | code |\n");
    s.push_str("|---|---|---|---|---|---|\n");
    for (name, w) in &all {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            cell(name),
            cell(&w.heading),
            cell(&w.what),
            cell(&w.next),
            who_name(w.who),
            w.code.unwrap_or("")
        ));
    }
    s
}

/// A table cell: a bar would end it.
fn cell(text: &str) -> String {
    text.replace('|', "/")
}

/// The words for a person in the list.
fn who_name(who: Who) -> &'static str {
    match who {
        Who::Nobody => "nobody",
        Who::Customer => "customer",
        Who::Cashier => "cashier",
        Who::Manager => "manager",
        Who::Owner => "owner",
        Who::KeyHolder => "key holder",
        Who::BoxKeeper => "Box keeper",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use openbmt_approver::{DecideError, FinishError, RefuseSession, RestoreError, StateError};
    use openbmt_core::say::{check, check_distinct};
    use openbmt_signer::RegisterError;

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

    fn names() -> BTreeSet<String> {
        all_words().into_iter().map(|(n, _)| n).collect()
    }

    #[test]
    fn sc_12_every_word_keeps_the_rules() {
        assert_eq!(check_distinct(&all_words()), Ok(()));
    }

    #[test]
    fn sc_12_the_whole_key_app_keeps_the_rules_together() {
        let mut all = openbmt_signer::all_words();
        all.extend(openbmt_session::all_words());
        all.extend(openbmt_approver::all_words());
        all.extend(all_words());
        assert_eq!(check_distinct(&all), Ok(()));
        assert!(all.len() > 200, "{}", all.len());
    }

    #[test]
    fn sc_01_no_variant_is_missing() {
        let names = names();
        let has = |prefix: &str| names.iter().any(|n| n.starts_with(prefix));
        for v in variants(include_str!("app.rs"), "SetupState") {
            assert!(has(&format!("SetupState::{v}")), "SetupState::{v}");
        }
        for v in variants(include_str!("views.rs"), "CheckStatus") {
            assert!(has(&format!("CheckStatus::{v}")), "CheckStatus::{v}");
        }
        for v in variants(include_str!("kitcheck.rs"), "KitCheck") {
            assert!(has(&format!("KitCheck::{v}")), "KitCheck::{v}");
        }
        for v in variants(include_str!("error.rs"), "KeyError") {
            assert!(has(&format!("KeyError::{v}(")), "KeyError::{v}");
        }
        // The exhaustive matches that name every variant compile only while
        // the list is complete.
        for (_, s) in setup_states() {
            assert!(!setup_name(&s).is_empty());
        }
        for (_, k) in kit_checks() {
            assert!(!kit_name(&k).is_empty());
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

    // ---- the strings the crate passes to its three error functions ----------

    /// The part of a source file before its tests.
    fn code_of(source: &str) -> &str {
        source.split("#[cfg(test)]").next().unwrap_or(source)
    }

    /// The string literal that starts at the first quote of `text`, with a
    /// `format!(` or white space in front of it, and the text of the
    /// literal up to its `{` or its end.
    fn literal_after(text: &str) -> Option<String> {
        let mut rest = text.trim_start();
        if let Some(r) = rest.strip_prefix("format!(") {
            rest = r.trim_start();
        }
        let mut chars = rest.strip_prefix('"')?.chars().peekable();
        let mut out = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' | '{' => break,
                '\\' => match chars.next()? {
                    // A line continuation drops the white space after it.
                    '\n' => {
                        while chars.peek().is_some_and(|c| c.is_whitespace()) {
                            chars.next();
                        }
                    }
                    other => out.push(other),
                },
                c => out.push(c),
            }
        }
        Some(out)
    }

    /// Every literal that follows `name(` in `source`, where `name` is not
    /// the tail of a longer name.
    fn literals_after_calls(source: &str, name: &str) -> Vec<String> {
        literals_after(source, &format!("{name}("))
    }

    /// Every literal that follows `needle` in `source`, where `needle` does
    /// not start in the middle of a longer name.
    fn literals_after(source: &str, needle: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut from = 0;
        while let Some(at) = source.get(from..).and_then(|s| s.find(needle)) {
            let start = from + at;
            from = start + needle.len();
            let before = source.get(..start).and_then(|s| s.chars().last());
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            if let Some(lit) = source.get(from..).and_then(literal_after) {
                out.push(lit);
            }
        }
        out
    }

    const SOURCES: [(&str, &str); 11] = [
        ("app.rs", include_str!("app.rs")),
        ("call.rs", include_str!("call.rs")),
        ("codec.rs", include_str!("codec.rs")),
        ("deposit.rs", include_str!("deposit.rs")),
        ("error.rs", include_str!("error.rs")),
        ("jni.rs", include_str!("jni.rs")),
        ("keys.rs", include_str!("keys.rs")),
        ("kitcheck.rs", include_str!("kitcheck.rs")),
        ("screen.rs", include_str!("screen.rs")),
        ("session.rs", include_str!("session.rs")),
        ("words.rs", include_str!("words.rs")),
    ];

    #[test]
    fn every_rule_string_the_crate_uses_has_words_of_its_own() {
        let mut seen = 0usize;
        for (file, source) in SOURCES {
            let code = code_of(source);
            // `bad` is the deposit parser's own name for `refused_by`.
            let refused = literals_after_calls(code, "refused_by").into_iter().chain(
                if file == "deposit.rs" {
                    literals_after_calls(code, "bad")
                } else {
                    Vec::new()
                },
            );
            for rule in refused {
                assert!(
                    own_words(Kind::Refused, &rule, "").is_some(),
                    "{file}: refused_by({rule:?}) has no words of its own"
                );
                seen += 1;
            }
            for rule in literals_after_calls(code, "wrong_state") {
                assert!(
                    own_words(Kind::State, &rule, "").is_some(),
                    "{file}: wrong_state({rule:?}) has no words of its own"
                );
                seen += 1;
            }
            for text in literals_after_calls(code, "internal") {
                assert!(
                    own_words(Kind::Internal, "internal", &text).is_some(),
                    "{file}: internal({text:?}) has no words of its own"
                );
                seen += 1;
            }
            // The rules of the errors made without those functions.
            for rule in literals_after(code, "rule:") {
                if rule == "internal" {
                    continue;
                }
                assert!(
                    own_words(Kind::Store, &rule, "").is_some(),
                    "{file}: a store error with rule {rule:?} has no words of its own"
                );
                seen += 1;
            }
        }
        assert!(seen > 45, "only {seen} literals were found");
    }

    #[test]
    fn the_walk_finds_the_calls_it_is_meant_to_find() {
        let src = "x = refused_by(\n    \"one\",\n    format!(\"two {}\", 3));\n\
                   y = wrong_state(\"three\", \"t\"); z = internal(format!(\"four: {e}\"));\n\
                   w = not_internal(\"five\"); v = bad(\"six\", \"t\");";
        assert_eq!(literals_after_calls(src, "refused_by"), ["one"]);
        assert_eq!(literals_after_calls(src, "wrong_state"), ["three"]);
        assert_eq!(literals_after_calls(src, "internal"), ["four: "]);
        assert_eq!(literals_after_calls(src, "bad"), ["six"]);
    }

    #[test]
    fn an_unknown_rule_gets_a_valid_screen_of_its_kind() {
        for kind in [Kind::Refused, Kind::Store, Kind::State, Kind::Internal] {
            assert!(own_words(kind, "no-such-rule", "no such text").is_none());
            let w = words_for(kind, "no-such-rule", &"x".repeat(70_000));
            assert_eq!(check(&w), Ok(()), "{kind:?}");
            assert_eq!(w.who, Who::KeyHolder);
            assert_eq!(w.detail.as_deref(), Some("no-such-rule"));
            assert!(!w.detail.as_deref().unwrap().contains("x"));
        }
    }

    #[test]
    fn seed_and_foreign_input_diagnostics_are_not_screen_details() {
        for (kind, rule, text) in [
            (Kind::Refused, "words-count", "words contain secret-seed"),
            (Kind::Refused, "words-unknown", "secret-seed is not a word"),
            (Kind::Refused, "words-checksum", "secret-seed does not fit"),
            (Kind::Refused, "entropy", "secret-seed is not a die face"),
            (Kind::Refused, "xpub", "secret-key-material"),
            (Kind::Refused, "owner", "secret-owner-material"),
            (Kind::Refused, "wallet", "secret-wallet-material"),
            (Kind::Refused, "unknown-rule", "secret-wallet-material"),
        ] {
            let words = words_for(kind, rule, text);
            assert!(
                !words.detail.as_deref().unwrap_or("").contains("secret"),
                "{rule}"
            );
        }
    }

    #[test]
    fn an_unknown_word_shows_only_a_checked_position_in_details() {
        let valid = words_for(
            Kind::Refused,
            "words-unknown",
            "word 5 is not on the word list; check how it is spelled",
        );
        assert_eq!(valid.detail.as_deref(), Some("word position 5"));

        let unexpected = words_for(
            Kind::Refused,
            "words-unknown",
            "word 5 is not on the word list; check how it is spelled: secret-seed",
        );
        assert_eq!(unexpected.detail, None);
        assert!(!unexpected.next.contains("Details"));
        assert!(!unexpected.what.contains("secret-seed"));
    }

    #[test]
    fn every_sample_has_words_that_keep_the_rules() {
        for (kind, rule, text) in samples() {
            let w = own_words(kind, rule, text)
                .unwrap_or_else(|| panic!("{kind:?} {rule} has no words of its own"));
            assert_eq!(check(&w), Ok(()), "{kind:?} {rule}");
        }
    }

    #[test]
    fn a_program_fault_says_nothing_was_signed_where_that_is_true() {
        for rule in [
            "request-too-long",
            "request-magic",
            "request-version",
            "op-unknown",
            "request-malformed",
            "request-null",
            "request-length",
            "request-read",
            "handle-unknown",
            "handle-unexpected",
            "busy",
        ] {
            let w = words_for(Kind::Refused, rule, "");
            assert!(w.what.starts_with("Nothing was signed"), "{rule}");
        }
        for rule in ["handles-used", "app-limit"] {
            let w = words_for(Kind::State, rule, "");
            assert!(w.what.starts_with("Nothing was signed"), "{rule}");
        }
        // A caught panic can come after the new state was saved: it says
        // what is known, that the owner's phone was sent nothing.
        let w = words_for(Kind::Internal, "internal", "caught a panic: x");
        assert!(w.what.contains("nothing was sent"), "{}", w.what);
        assert!(!w.what.starts_with("Nothing was signed"));
        assert_eq!(w.detail, None);
    }

    #[test]
    fn the_empty_rule_names_no_function() {
        let w = words_for(Kind::State, "empty", "no key has been made yet");
        assert_eq!(w.what, "This app has no key yet.");
        assert!(w.next.contains("Make the key"));
        assert!(w.next.contains("type the words of an existing key"));
        assert_eq!(w.heading, SetupState::Empty.words().heading);
    }

    #[test]
    fn key_2_missing_is_one_sentence_however_it_is_reached() {
        assert_eq!(
            words_for(Kind::Refused, "AS-36", "AS-36: key 2 is not on this device"),
            no_key2_words()
        );
        assert_eq!(
            StateError::<String>::NoSigner.words(),
            words_for(
                Kind::Refused,
                "AS-36",
                &StateError::<String>::NoSigner.to_string()
            )
        );
    }

    #[test]
    fn one_fault_reached_two_ways_is_one_sentence() {
        let wallet = "AS-36: the wallet is accepted already";
        let a = words_for(Kind::State, "wallet-set", "the wallet is accepted already");
        let b = words_for(
            Kind::State,
            "wallet-set",
            "the wallet is accepted already; key 2 cannot be made again",
        );
        let c = words_for(Kind::State, "wallet-set", wallet);
        assert_eq!((a.heading.clone(), a.what.clone()), (b.heading, b.what));
        assert_eq!((a.heading, a.what), (c.heading, c.what));
        let open1 = words_for(Kind::State, "session-open", "a session is open: finish");
        let open2 = words_for(Kind::State, "session-open", "a session is open already");
        assert_eq!((open1.heading, open1.what), (open2.heading, open2.what));
        let stored = words_for(Kind::Store, "OBM7", "x");
        let read = words_for(Kind::Refused, "OBM7", "x");
        assert_eq!((stored.heading, stored.what), (read.heading, read.what));
        // The same kind of address refusal as the signer's own.
        assert_eq!(
            words_for(
                Kind::Refused,
                "address-kind",
                "SR-30: the allowlist takes only"
            ),
            ChangeRefusal::ScriptKind.words()
        );
    }

    #[test]
    fn a_text_a_rule_crate_gave_is_told_in_that_crates_words() {
        let cases: Vec<(String, Words)> = vec![
            (
                RefuseSession::WrongOwner.to_string(),
                RefuseSession::WrongOwner.words(),
            ),
            (
                RefuseSession::SeenBefore.to_string(),
                RefuseSession::SeenBefore.words(),
            ),
            (
                DecideError::Finished.to_string(),
                DecideError::Finished.words(),
            ),
            (
                FinishError::<String>::Finished.to_string(),
                FinishError::<String>::Finished.words(),
            ),
            (
                RestoreError::WrongWallet.to_string(),
                RestoreError::WrongWallet.words(),
            ),
            (
                ChangeRefusal::AllowlistFull.to_string(),
                ChangeRefusal::AllowlistFull.words(),
            ),
            (
                RegisterError::Key3NotInWallet.to_string(),
                RegisterError::Key3NotInWallet.words(),
            ),
            (
                "owner and Key app roots must differ".to_string(),
                openbmt_approver::SetupError::SameRoot.words(),
            ),
        ];
        for (text, words) in cases {
            let rule = text.split(':').next().unwrap_or("check").to_string();
            let got = words_for(Kind::Refused, &rule, &text);
            assert_eq!(got, words, "{text}");
        }
    }

    #[test]
    fn the_wallet_rule_tells_each_wallet_text_apart() {
        let a = words_for(
            Kind::Refused,
            "wallet",
            "descriptor checksum does not match",
        );
        let b = words_for(Kind::Refused, "wallet", "descriptor has no checksum");
        assert_ne!(a.heading, b.heading);
        assert_eq!(a.detail, None);
        assert_eq!(b.detail, None);
    }

    #[test]
    fn the_kit_that_shares_the_check_words_is_the_most_serious_and_says_so() {
        let kit = |same, parses| KitCheck::Different {
            first_difference: 12,
            same_checksum_words: same,
            parses,
        };
        let serious = kit(true, true).words();
        assert!(serious.what.contains("should not happen by accident"));
        assert!(serious.next.contains("tell the owner in person"));
        assert_eq!(serious.tone, Tone::Stop);
        for (same, parses) in [(true, true), (false, true), (false, false)] {
            let w = kit(same, parses).words();
            assert_eq!(w.tone, Tone::Stop);
            assert_eq!(w.who, Who::KeyHolder);
            assert_eq!(w.detail.as_deref(), Some("first difference at position 12"));
        }
        let other = kit(false, true).words();
        assert!(other.next.contains("print a new kit"));
        assert!(
            other.next.contains("do not use this kit")
                || other.next.contains("Do not use this kit")
        );
        assert_eq!(KitCheck::Same.words().tone, Tone::Good);
    }

    #[test]
    fn the_tone_and_person_numbers_are_unique_and_complete() {
        for (tone, _, byte) in TONES {
            assert_eq!(tone_byte(tone), byte);
        }
        for (who, _, byte) in WHOS {
            assert_eq!(who_byte(who), byte);
        }
        let tones: BTreeSet<u8> = TONES.iter().map(|t| t.2).collect();
        let whos: BTreeSet<u8> = WHOS.iter().map(|t| t.2).collect();
        assert_eq!((tones.len(), whos.len()), (4, 7));
    }

    // ---- the list of screens --------------------------------------------------

    fn screens_path() -> PathBuf {
        std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR")
                .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
        )
        .join("../../docs/screens/key.md")
    }

    #[test]
    fn screens_list() {
        let text = screens_markdown();
        let path = screens_path();
        if std::env::var_os("OPENBMT_WRITE_SCREENS").is_some() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).expect("a directory");
            }
            std::fs::write(&path, &text).expect("written");
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            committed == text,
            "docs/screens/key.md differs from the code: run this test with \
             OPENBMT_WRITE_SCREENS=1 to write it again"
        );
        for line in text.lines().filter(|l| !l.starts_with('|')) {
            assert!(line.chars().count() <= 80, "{line}");
        }
    }
}

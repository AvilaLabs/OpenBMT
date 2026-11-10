//! What the key holder is told about the approval session file (SC-01 to
//! SC-12): why a request or an answer is not accepted, and what an answer
//! said about a line.
//!
//! The `Display` texts of the errors are for logs and tests. A person is
//! shown the [`Words`] these functions return.

use openbmt_core::say::{Tone, Who, Words};

use crate::{BindError, DeclineCode, EncodeError, VerifyError};

/// What the key holder does when a request does not come from the owner's
/// phone as it should.
const TAMPERED: &str =
    "Do not approve it, and tell the owner in person, not through the phone that sent it.";

fn tampered(heading: &str, what: &str) -> Words {
    Words::new(Tone::Stop, heading, what, Who::KeyHolder, TAMPERED)
}

/// The line number a person sees: the item number as the file gives it.
fn line(item: u32) -> String {
    format!("line {item}")
}

impl DeclineCode {
    /// What the key holder is shown about a line that was declined, and
    /// what the owner's phone is told with it.
    pub fn words(&self) -> Words {
        match self {
            Self::ByPerson => Words::new(
                Tone::Ended,
                "You declined this line",
                "The owner's phone is told that this line was declined on this app.",
                Who::Nobody,
                "Nothing to do. The owner can send a new request if it is still needed.",
            ),
            Self::ChecksFailed => Words::new(
                Tone::Stop,
                "A check of this app failed",
                "This line was declined because it failed one of this app's own checks.",
                Who::KeyHolder,
                "Tell the owner in person which check it was; they can send a new request.",
            ),
            Self::NotOnThisDevice => Words::new(
                Tone::Ended,
                "Payment line not answered",
                "The payment line was declined, because this phone does not hold key 2.",
                Who::KeyHolder,
                "Use the phone that holds key 2 to answer payments.",
            ),
            Self::NotDecided => Words::new(
                Tone::Ended,
                "No answer given to a line",
                "No decision was made on this line, so it was declined.",
                Who::Nobody,
                "Nothing to do. The owner can send a new request if it is still needed.",
            ),
        }
        .code("AS-12")
    }
}

impl VerifyError {
    /// What the key holder is shown when a request fails the check of its
    /// signature.
    pub fn words(&self) -> Words {
        match self {
            Self::WrongOwner => tampered(
                "Owner key does not match",
                "The request names an owner key other than the one this app was set up with.",
            ),
            Self::BadSignature => tampered(
                "Owner signature fails",
                "The owner's signature on this request does not check out.",
            ),
        }
        .code("AS-05")
    }
}

impl BindError {
    /// What the key holder is shown when an answer does not fit the request
    /// it is offered for.
    pub fn words(&self) -> Words {
        let code = self.rule();
        let words = match self {
            Self::WrongApprover => tampered(
                "Answer from another app",
                "The answer was not made by the Key app this wallet was set up with.",
            ),
            Self::BadSignature => tampered(
                "Answer signature fails",
                "This app's signature on the answer does not check out.",
            ),
            Self::WrongSession => tampered(
                "Answer for another session",
                "The answer belongs to a different approval than the one it is offered for.",
            ),
            Self::WrongRequest => tampered(
                "Answer for another request",
                "The answer was made for a different request than the one it is offered for.",
            ),
            Self::UnknownItem { item } => tampered(
                "Answer to a line not asked",
                "The answer holds a result for a line the request does not have.",
            )
            .detail(line(*item)),
            Self::MissingItem { item } => tampered(
                "Answer lacks a line",
                "The request has a line that the answer gives no result for.",
            )
            .detail(line(*item)),
            Self::KindMismatch { item } => tampered(
                "Answer does not fit the line",
                "A result in the answer is not of the kind its line asks for.",
            )
            .detail(line(*item)),
            Self::BadHealthProof { item } => tampered(
                "Health check answer fails",
                "The signed answer to the health check does not check out.",
            )
            .detail(line(*item)),
        };
        words.code(code)
    }

    /// The rule behind this error.
    fn rule(&self) -> &'static str {
        match self {
            Self::WrongApprover
            | Self::BadSignature
            | Self::WrongSession
            | Self::WrongRequest
            | Self::UnknownItem { .. }
            | Self::MissingItem { .. }
            | Self::KindMismatch { .. } => "AS-13",
            Self::BadHealthProof { .. } => "AS-14",
        }
    }
}

impl EncodeError {
    /// What the key holder is shown when a request or an answer cannot be
    /// built. Nothing was changed.
    pub fn words(&self) -> Words {
        let fault = |heading: &str, what: &str, next: &str| {
            Words::new(Tone::Stop, heading, what, Who::KeyHolder, next)
        };
        match self {
            Self::TooLarge => fault(
                "File would be too big",
                "Nothing was changed, because the file would be bigger than this app allows.",
                "Approve fewer lines at once and finish again.",
            )
            .code("AS-02"),
            Self::ItemCount => fault(
                "Wrong number of lines",
                "Nothing was changed, because a file holds from 1 to 64 lines.",
                "Tell the owner in person; the request has to be made again.",
            )
            .code("AS-04"),
            Self::ItemOrder => fault(
                "Lines out of order",
                "Nothing was changed, because the lines of the file are not in order.",
                "Tell the owner in person; the request has to be made again.",
            )
            .code("AS-04"),
            Self::BadText(why) => fault(
                "Text not allowed",
                "Nothing was changed, because a text in the file is not allowed.",
                "Tell the owner in person; the request has to be made again.",
            )
            .detail(*why)
            .code("AS-07"),
            Self::BadItem(why) => fault(
                "Line not allowed",
                "Nothing was changed, because a line in the file is not one it can hold.",
                "Tell the owner in person; the request has to be made again.",
            )
            .detail(*why)
            .code("AS-06"),
            Self::BadPayload(why) => fault(
                "Result not allowed",
                "Nothing was changed, because a result in the answer is not one it can hold.",
                "Try again, and if it happens again, stop and tell the owner in person.",
            )
            .detail(*why)
            .code("AS-12"),
        }
    }
}

/// The words of every error and every state of this crate, one or more
/// for each variant, named after them (SC-12).
pub fn all_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = Vec::new();
    for c in declines() {
        all.push((format!("DeclineCode::{}", decline_name(c)), c.words()));
    }
    for e in verify_errors() {
        all.push((format!("VerifyError::{}", verify_name(&e)), e.words()));
    }
    for e in bind_errors() {
        all.push((format!("BindError::{}", bind_name(&e)), e.words()));
    }
    for e in encode_errors() {
        all.push((format!("EncodeError::{}", encode_name(&e)), e.words()));
    }
    all
}

fn declines() -> [DeclineCode; 4] {
    [
        DeclineCode::ByPerson,
        DeclineCode::ChecksFailed,
        DeclineCode::NotOnThisDevice,
        DeclineCode::NotDecided,
    ]
}

fn decline_name(c: DeclineCode) -> &'static str {
    match c {
        DeclineCode::ByPerson => "ByPerson",
        DeclineCode::ChecksFailed => "ChecksFailed",
        DeclineCode::NotOnThisDevice => "NotOnThisDevice",
        DeclineCode::NotDecided => "NotDecided",
    }
}

fn verify_errors() -> [VerifyError; 2] {
    [VerifyError::WrongOwner, VerifyError::BadSignature]
}

fn verify_name(e: &VerifyError) -> &'static str {
    match e {
        VerifyError::WrongOwner => "WrongOwner",
        VerifyError::BadSignature => "BadSignature",
    }
}

fn bind_errors() -> Vec<BindError> {
    vec![
        BindError::WrongApprover,
        BindError::BadSignature,
        BindError::WrongSession,
        BindError::WrongRequest,
        BindError::UnknownItem { item: 3 },
        BindError::MissingItem { item: 3 },
        BindError::KindMismatch { item: 3 },
        BindError::BadHealthProof { item: 3 },
    ]
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

fn encode_errors() -> Vec<EncodeError> {
    vec![
        EncodeError::TooLarge,
        EncodeError::ItemCount,
        EncodeError::ItemOrder,
        EncodeError::BadText("a label holds a control character"),
        EncodeError::BadItem("a spend has no labels"),
        EncodeError::BadPayload("a payload is the wrong size"),
    ]
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use openbmt_core::say::check_distinct;

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
            assert!(names.contains(&own), "{own} has no words in all_words");
        }
    }

    #[test]
    fn sc_12_every_word_keeps_the_rules() {
        assert_eq!(check_distinct(&all_words()), Ok(()));
    }

    #[test]
    fn sc_01_no_variant_is_missing() {
        covered("DeclineCode", include_str!("response.rs"));
        covered("VerifyError", include_str!("error.rs"));
        covered("BindError", include_str!("error.rs"));
        covered("EncodeError", include_str!("error.rs"));
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
    fn an_answer_or_request_that_does_not_fit_is_stopped_and_told_in_person() {
        for e in bind_errors() {
            let w = e.words();
            assert_eq!(w.tone, Tone::Stop);
            assert_eq!(w.next, TAMPERED);
            assert!(w.code.is_some());
        }
        for e in verify_errors() {
            assert_eq!(e.words().next, TAMPERED);
            assert_eq!(e.words().code, Some("AS-05"));
        }
    }

    #[test]
    fn every_code_is_the_rule_the_text_starts_with() {
        for e in bind_errors() {
            assert!(e.to_string().starts_with(e.words().code.unwrap_or("")));
        }
        for e in encode_errors() {
            assert!(e.to_string().starts_with(e.words().code.unwrap_or("")));
        }
        for e in verify_errors() {
            assert!(e.to_string().starts_with(e.words().code.unwrap_or("")));
        }
    }

    #[test]
    fn the_item_number_is_in_the_detail_not_the_sentence() {
        let w = BindError::MissingItem { item: 7 }.words();
        assert_eq!(w.detail.as_deref(), Some("line 7"));
        assert!(!w.what.contains('7'));
    }
}

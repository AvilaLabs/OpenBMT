//! The one error type that crosses the boundary, and the error of the
//! storage callback.

use std::fmt;

/// Why a call did not do what was asked. Every variant carries the ID of
/// the rule that refused (for example `AS-20` or `SR-30`) when the refusal
/// comes from one of the rule crates, and the crate's own text, which says
/// why and what to do next where the underlying error has it.
#[derive(Debug)]
pub enum KeyError {
    /// A rule in one of the existing crates refused. `rule` is its ID, or
    /// the name of the check when the error has no ID.
    Refused {
        /// The rule's ID.
        rule: String,
        /// The refusal, as the rule crate words it.
        text: String,
    },
    /// The storage callback failed, or the stored bytes cannot be read.
    /// Nothing was changed.
    Store {
        /// `AS-34` for a failed store, `OBM7` for unreadable bytes.
        rule: String,
        /// What went wrong.
        text: String,
    },
    /// The call is not possible in the state the Key app is in.
    State {
        /// A short name of the state rule.
        rule: String,
        /// What to do instead.
        text: String,
    },
    /// A panic was caught. The object is poisoned: every later call
    /// returns this error until the app opens the object again.
    Internal {
        /// Always `internal`.
        rule: String,
        /// What was caught.
        text: String,
    },
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused { text, .. }
            | Self::Store { text, .. }
            | Self::State { text, .. }
            | Self::Internal { text, .. } => write!(f, "{text}"),
        }
    }
}

impl std::error::Error for KeyError {}

impl KeyError {
    /// The rule string of any variant.
    pub fn rule(&self) -> &str {
        match self {
            Self::Refused { rule, .. }
            | Self::Store { rule, .. }
            | Self::State { rule, .. }
            | Self::Internal { rule, .. } => rule,
        }
    }

    /// The text of any variant.
    pub fn text(&self) -> &str {
        match self {
            Self::Refused { text, .. }
            | Self::Store { text, .. }
            | Self::State { text, .. }
            | Self::Internal { text, .. } => text,
        }
    }
}

/// `AS-20`, `SR-30` and the like at the start of a rule crate's text.
fn leading_rule(text: &str) -> Option<&str> {
    let (head, _) = text.split_once(':')?;
    let (letters, digits) = head.split_once('-')?;
    let ok = (1..=3).contains(&letters.len())
        && letters.bytes().all(|b| b.is_ascii_uppercase())
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit());
    ok.then_some(head)
}

/// A refusal from a rule crate. The rule ID is the one the text starts
/// with; the text is passed through unchanged.
pub(crate) fn refused(text: impl fmt::Display) -> KeyError {
    let text = text.to_string();
    let rule = leading_rule(&text).unwrap_or("check").to_string();
    KeyError::Refused { rule, text }
}

/// A refusal that names its own check, for errors that carry no rule ID.
pub(crate) fn refused_by(rule: &str, text: impl fmt::Display) -> KeyError {
    KeyError::Refused {
        rule: rule.to_string(),
        text: text.to_string(),
    }
}

pub(crate) fn wrong_state(rule: &str, text: &str) -> KeyError {
    KeyError::State {
        rule: rule.to_string(),
        text: text.to_string(),
    }
}

pub(crate) fn internal(text: impl fmt::Display) -> KeyError {
    KeyError::Internal {
        rule: "internal".to_string(),
        text: text.to_string(),
    }
}

/// A failed or refused store, with the rule of the stored-before-released
/// step.
pub(crate) fn store_failed(why: &StoreError) -> KeyError {
    KeyError::Store {
        rule: "AS-34".to_string(),
        text: format!("AS-34: could not store the new state: {why}"),
    }
}

/// The error of a [`KeyStore`](crate::KeyStore) call, raised by the app's
/// own code.
#[derive(Debug)]
pub enum StoreError {
    /// The platform call failed. Nothing was stored.
    Failed {
        /// What the platform said.
        text: String,
    },
    /// The app's callback threw something it did not declare, or the call
    /// could not be made.
    Unexpected {
        /// What was thrown.
        text: String,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed { text } | Self::Unexpected { text } => write!(f, "{text}"),
        }
    }
}

impl std::error::Error for StoreError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rule_is_read_from_the_front_of_the_text() {
        assert_eq!(
            refused("AS-20: the request is for another wallet").rule(),
            "AS-20"
        );
        assert_eq!(refused("SR-30: the allowlist is full").rule(), "SR-30");
        assert_eq!(refused("no rule here: at all").rule(), "check");
        assert_eq!(refused("internal error: x").rule(), "check");
        assert_eq!(refused("AS-: x").rule(), "check");
    }
}

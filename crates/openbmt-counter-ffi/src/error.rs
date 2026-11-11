//! The one error type that crosses the boundary.

use std::fmt;

use openbmt_core::say::Words;

use crate::say::words_for;

/// Which of the four kinds of [`CounterError`] an error is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A rule in one of the existing crates refused, or the request is not
    /// one the library takes.
    Refused,
    /// The counter's files could not be written or read.
    Store,
    /// The call is not possible in the state the node is in.
    State,
    /// A fault of the library itself (a panic was caught).
    Internal,
}

/// Why a call did not do what was asked. Every variant carries the ID of the
/// rule that refused (for example `CR-12` or `NR-79`) when the refusal comes
/// from one of the rule crates, or the name of the check, the crate's own
/// text, and the words a person is shown (`docs/SCREEN-RULES.md`).
#[derive(Debug, Clone)]
pub enum CounterError {
    /// A rule refused, or the request is not valid. `rule` is the rule's ID
    /// or the name of the check.
    Refused {
        /// The rule's ID or the name of the check.
        rule: String,
        /// The refusal, as the rule crate words it.
        text: String,
        /// What the person is shown.
        words: Box<Words>,
    },
    /// A file of the counter's data folder failed (`CR-40`). Nothing the
    /// text does not say was changed.
    Store {
        /// The rule.
        rule: String,
        /// What went wrong.
        text: String,
        /// What the person is shown.
        words: Box<Words>,
    },
    /// The call is not possible now (the node is stopped, too many nodes
    /// are open, the handle is used up).
    State {
        /// A short name of the state rule.
        rule: String,
        /// What to do instead.
        text: String,
        /// What the person is shown.
        words: Box<Words>,
    },
    /// A panic was caught. The node is poisoned: every later call returns
    /// this error until it is stopped.
    Internal {
        /// Always `internal` or `poisoned`.
        rule: String,
        /// What was caught.
        text: String,
        /// What the person is shown.
        words: Box<Words>,
    },
}

impl fmt::Display for CounterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.text())
    }
}

impl std::error::Error for CounterError {}

impl CounterError {
    /// The kind.
    pub fn kind(&self) -> Kind {
        match self {
            Self::Refused { .. } => Kind::Refused,
            Self::Store { .. } => Kind::Store,
            Self::State { .. } => Kind::State,
            Self::Internal { .. } => Kind::Internal,
        }
    }

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

    /// What the person is shown for this error.
    pub fn words(&self) -> &Words {
        match self {
            Self::Refused { words, .. }
            | Self::Store { words, .. }
            | Self::State { words, .. }
            | Self::Internal { words, .. } => words,
        }
    }
}

/// A refusal from a rule crate, with that crate's own words. A failed write
/// (`CR-40`) is a store error.
pub(crate) fn from_rule(rule: &str, text: impl fmt::Display, words: Words) -> CounterError {
    let (rule, text) = (rule.to_string(), text.to_string());
    if rule == "CR-40" {
        CounterError::Store {
            rule,
            text,
            words: Box::new(words),
        }
    } else {
        CounterError::Refused {
            rule,
            text,
            words: Box::new(words),
        }
    }
}

/// A refusal that names its own check, for errors of this layer.
pub(crate) fn refused_by(rule: &str, text: impl fmt::Display) -> CounterError {
    CounterError::Refused {
        rule: rule.to_string(),
        text: text.to_string(),
        words: Box::new(words_for(Kind::Refused, rule)),
    }
}

pub(crate) fn wrong_state(rule: &str, text: &str) -> CounterError {
    CounterError::State {
        rule: rule.to_string(),
        text: text.to_string(),
        words: Box::new(words_for(Kind::State, rule)),
    }
}

pub(crate) fn internal(text: impl fmt::Display) -> CounterError {
    CounterError::Internal {
        rule: "internal".to_string(),
        text: text.to_string(),
        words: Box::new(words_for(Kind::Internal, "internal")),
    }
}

/// The error of every call on a node that an earlier call poisoned.
pub(crate) fn poisoned() -> CounterError {
    CounterError::Internal {
        rule: "poisoned".to_string(),
        text:
            "an earlier call failed with a fault of the library: stop the node and start it again"
                .to_string(),
        words: Box::new(words_for(Kind::Internal, "poisoned")),
    }
}

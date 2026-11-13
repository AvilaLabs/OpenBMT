//! The starter wallet (`docs/START-SIMPLE-RULES.md`, SS-01 to SS-50): a
//! small wallet the owner phone makes for a shop's first takings, with a
//! limit and a date, until the shop moves to a key made offline.
//!
//! The key is made, kept and used here, in the Rust core, and nothing
//! secret crosses to the app as text:
//!
//! - [`chance`] (SS-01 to SS-05): 32 bytes from the kernel through the
//!   [`KernelRandom`] trait, checked, mixed with die rolls, made into 12
//!   words and the wallet of them.
//! - [`setup`] (SS-10, SS-11): the words drawn four at a time as a grey
//!   [`Image`], and the three check-back positions answered on a keypad
//!   that gives back a count and a yes, never the letters.
//! - [`seal`] (SS-20, SS-23): the seed sealed under a random data key with
//!   ChaCha20-Poly1305, and the restore from typed words.
//! - [`pay`] (SS-30, SS-31, SS-42): a queued refund signed, and the one move
//!   of everything to the new wallet, each with the only-these-outputs
//!   check.
//! - [`limit`] (SS-35 to SS-38, SS-43): the limit and the date as pure
//!   functions.
//!
//! The library keeps nothing between calls but a [`Setup`] or a [`Restore`]
//! that the caller holds. A call that signs takes the sealed seed and the
//! data key, opens the seed for that call only and wipes it. The caller
//! passes in the coins, the fee rate and the time; the library has no
//! network and no clock. The kernel's bytes come through [`KernelRandom`]
//! so that a test can give fixed ones.
//!
//! Every refusal is a [`StarterRefusal`]. The words a person would see are
//! placeholders that name themselves (`WORDS_NEEDED_SS_xx_...`) until they
//! are written (SC-12), listed by [`all_starter_words`].

mod chance;
mod limit;
mod pay;
mod seal;
mod setup;
#[cfg(test)]
mod tests;

use std::fmt;

use openbmt_core::say::{Tone, Who, Words};

use crate::own_say::needed;

pub use chance::{
    checked_read, make_seed, KernelRandom, Seed, StarterWallet, ENTROPY_BYTES, KERNEL_BYTES,
    STARTER_TAG, STARTER_WORDS,
};
pub use limit::{
    change_held, counter_offers_bitcoin, limit_state, not_yet_until, seed_may_go, LimitState,
    SettingsChange, MAX_NOT_YET_SECS,
};
pub use pay::{
    check_only_these, fee_rate, move_all, pay_refund, receive_script_of, StarterPaid,
    MIN_RATE_PER_KVB,
};
pub use seal::{wallet_of_sealed, DataKey, Restore, Sealed, DATA_KEY_BYTES};
pub use setup::{pick_positions, Image, Key, Setup, Typed, ASKED, PAGES, WORDS_PER_PAGE};

pub use openbmt_spend::Coin;

/// SS-01: the kernel gave no random bytes.
pub const WORDS_NEEDED_SS_01_RANDOM: &str = "WORDS_NEEDED_SS_01_RANDOM";
/// SS-02: two reads of the kernel's bytes were the same.
pub const WORDS_NEEDED_SS_02_REPEAT: &str = "WORDS_NEEDED_SS_02_REPEAT";
/// SS-02: a read of the kernel's bytes was all zero or one byte repeated.
pub const WORDS_NEEDED_SS_02_WEAK: &str = "WORDS_NEEDED_SS_02_WEAK";
/// SS-03: a die roll that is not 1 to 6.
pub const WORDS_NEEDED_SS_03_ROLLS: &str = "WORDS_NEEDED_SS_03_ROLLS";
/// SS-04, SS-23: the 12 words typed are not 12 words of the English list
/// with a right check word.
pub const WORDS_NEEDED_SS_04_WORDS: &str = "WORDS_NEEDED_SS_04_WORDS";
/// SS-10: a screen of words that is not one of the three.
pub const WORDS_NEEDED_SS_10_PAGE: &str = "WORDS_NEEDED_SS_10_PAGE";
/// SS-10: a step of the setup out of its order (words shown after the
/// check passed, a check before the words were shown again).
pub const WORDS_NEEDED_SS_10_STAGE: &str = "WORDS_NEEDED_SS_10_STAGE";
/// SS-10: a wrong answer in the check-back; the words are shown again.
pub const WORDS_NEEDED_SS_10_WRONG: &str = "WORDS_NEEDED_SS_10_WRONG";
/// SS-11: a key of the keypad that is not a letter, backspace or enter.
pub const WORDS_NEEDED_SS_11_KEY: &str = "WORDS_NEEDED_SS_11_KEY";
/// SS-20: the data key is not 32 bytes.
pub const WORDS_NEEDED_SS_20_KEY: &str = "WORDS_NEEDED_SS_20_KEY";
/// SS-20, SS-23: the sealed seed cannot be opened with this data key (it
/// is damaged, or the key is not its own): the owner is offered the
/// restore.
pub const WORDS_NEEDED_SS_20_OPEN: &str = "WORDS_NEEDED_SS_20_OPEN";
/// SS-23: the restore has not got 12 accepted words.
pub const WORDS_NEEDED_SS_23_INCOMPLETE: &str = "WORDS_NEEDED_SS_23_INCOMPLETE";
/// SS-23: the words typed are another wallet than the starter wallet.
pub const WORDS_NEEDED_SS_23_OTHER: &str = "WORDS_NEEDED_SS_23_OTHER";
/// SS-30: the payment cannot be planned from the starter wallet's coins
/// (the text is the detail).
pub const WORDS_NEEDED_SS_30_PLAN: &str = "WORDS_NEEDED_SS_30_PLAN";
/// SS-30: the starter wallet's key did not sign, or the finish failed (the
/// text is the detail).
pub const WORDS_NEEDED_SS_30_SIGN: &str = "WORDS_NEEDED_SS_30_SIGN";
/// SS-30: a coin is not one of the starter wallet's.
pub const WORDS_NEEDED_SS_30_COIN: &str = "WORDS_NEEDED_SS_30_COIN";
/// SS-30: the finished transaction pays something other than what it
/// should, or spends a coin that is not the starter wallet's.
pub const WORDS_NEEDED_SS_30_CHECK: &str = "WORDS_NEEDED_SS_30_CHECK";
/// SS-30: there is no fee rate to use.
pub const WORDS_NEEDED_SS_30_NO_RATE: &str = "WORDS_NEEDED_SS_30_NO_RATE";
/// SS-30: the starter wallet cannot cover the payment and its fee.
pub const WORDS_NEEDED_SS_30_SHORT: &str = "WORDS_NEEDED_SS_30_SHORT";
/// SS-30: the starter wallet's description or an address cannot be made.
pub const WORDS_NEEDED_SS_30_WALLET: &str = "WORDS_NEEDED_SS_30_WALLET";
/// SS-42: the starter wallet holds no coin to move.
pub const WORDS_NEEDED_SS_42_NOTHING: &str = "WORDS_NEEDED_SS_42_NOTHING";
/// SS-42: the new wallet's address cannot be derived from its description.
pub const WORDS_NEEDED_SS_42_ADDRESS: &str = "WORDS_NEEDED_SS_42_ADDRESS";
/// SS-42: a coin of the starter wallet is not confirmed yet, so the move
/// waits and would not be one payment.
pub const WORDS_NEEDED_SS_42_UNCONFIRMED: &str = "WORDS_NEEDED_SS_42_UNCONFIRMED";
/// SS-36: the line of the home screen: what the starter wallet holds, the
/// limit and the date.
pub const WORDS_NEEDED_SS_36_LINE: &str = "WORDS_NEEDED_SS_36_LINE";
/// SS-37: the starter wallet is at its limit: the upgrade screen.
pub const WORDS_NEEDED_SS_37_AT_LIMIT: &str = "WORDS_NEEDED_SS_37_AT_LIMIT";
/// SS-37: the date to move has come: the upgrade screen.
pub const WORDS_NEEDED_SS_37_PAST_DATE: &str = "WORDS_NEEDED_SS_37_PAST_DATE";
/// SS-37: "Not yet" is in force, until a time.
pub const WORDS_NEEDED_SS_37_NOT_YET: &str = "WORDS_NEEDED_SS_37_NOT_YET";
/// SS-37: a change of settings waits.
pub const WORDS_NEEDED_SS_37_HELD: &str = "WORDS_NEEDED_SS_37_HELD";
/// SS-38: the counter offers no bitcoin while the wallet is at its limit.
pub const WORDS_NEEDED_SS_38_NO_BITCOIN: &str = "WORDS_NEEDED_SS_38_NO_BITCOIN";
/// SS-43: the key is deleted: the card of starter words can be destroyed.
pub const WORDS_NEEDED_SS_43_DELETED: &str = "WORDS_NEEDED_SS_43_DELETED";

/// Why a step of the starter wallet was refused. Nothing was changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StarterRefusal {
    /// SS-01: the kernel gave no bytes.
    Random,
    /// SS-02: two reads were the same.
    RandomRepeat,
    /// SS-02: a read was all zero or one byte repeated.
    RandomWeak,
    /// SS-03: a roll that is not 1 to 6.
    Rolls,
    /// SS-04, SS-23: not 12 English words with a right check word.
    Words,
    /// SS-10: a screen of words that does not exist.
    Page,
    /// SS-10: a step out of its order.
    Stage,
    /// SS-10: the answer was wrong.
    Wrong,
    /// SS-11: a key that is not a letter, backspace or enter.
    Key,
    /// SS-20: the data key is not 32 bytes.
    DataKey,
    /// SS-20: the sealed seed does not open.
    Open,
    /// SS-23: fewer than 12 words accepted.
    Incomplete,
    /// SS-23: words of another wallet.
    OtherWallet,
    /// SS-30: the spend crate could not plan it (the text says why).
    Plan(String),
    /// SS-30: signing or finishing failed (the text says why).
    Sign(String),
    /// SS-30: a coin handed in is not a coin of the starter wallet.
    NotStarterCoin(String),
    /// SS-30, SS-42: the finished transaction is not only what it should be
    /// (the text says which check).
    NotOnlyThese(String),
    /// SS-30: no recent rate and none typed.
    NoFeeRate,
    /// SS-30: the wallet holds too little for the payment and its fee.
    NotEnough {
        /// What the eligible coins hold.
        available: u64,
        /// What the payment and the fee need.
        needed: u64,
    },
    /// SS-30: the description or an address cannot be made.
    Wallet,
    /// SS-42: no coin to move.
    NothingToMove,
    /// SS-42: the new wallet's address cannot be derived.
    NewAddress,
    /// SS-42: a coin is not confirmed to the depth asked.
    Unconfirmed,
}

impl fmt::Display for StarterRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Random => write!(f, "SS-01: the kernel gave no random bytes"),
            Self::RandomRepeat => write!(f, "SS-02: two reads of the kernel were the same"),
            Self::RandomWeak => write!(f, "SS-02: a read of the kernel was all one byte"),
            Self::Rolls => write!(f, "SS-03: a roll is not 1 to 6"),
            Self::Words => write!(f, "SS-04: not 12 words of the English list"),
            Self::Page => write!(f, "SS-10: there is no such screen of words"),
            Self::Stage => write!(f, "SS-10: this step is out of its order"),
            Self::Wrong => write!(f, "SS-10: the answer was wrong"),
            Self::Key => write!(f, "SS-11: not a letter, backspace or enter"),
            Self::DataKey => write!(f, "SS-20: the data key is not 32 bytes"),
            Self::Open => write!(f, "SS-20: the sealed seed does not open"),
            Self::Incomplete => write!(f, "SS-23: fewer than 12 words were typed"),
            Self::OtherWallet => write!(f, "SS-23: these words are another wallet"),
            Self::Plan(why) => write!(f, "SS-30: the payment cannot be planned: {why}"),
            Self::Sign(why) => write!(f, "SS-30: the payment cannot be signed: {why}"),
            Self::NotStarterCoin(why) => write!(f, "SS-30: a coin is not the wallet's: {why}"),
            Self::NotOnlyThese(why) => write!(f, "SS-30: {why}"),
            Self::NoFeeRate => write!(f, "SS-30: there is no fee rate"),
            Self::NotEnough { available, needed } => write!(
                f,
                "SS-30: the coins hold {available} sat; the payment and its fee need {needed} sat"
            ),
            Self::Wallet => write!(f, "SS-30: the wallet cannot be read"),
            Self::NothingToMove => write!(f, "SS-42: the wallet holds no coin"),
            Self::NewAddress => write!(f, "SS-42: the new wallet's address cannot be derived"),
            Self::Unconfirmed => write!(f, "SS-42: a coin is not confirmed yet"),
        }
    }
}

impl std::error::Error for StarterRefusal {}

impl StarterRefusal {
    /// The rule ID of the refusal.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Random => "SS-01",
            Self::RandomRepeat | Self::RandomWeak => "SS-02",
            Self::Rolls => "SS-03",
            Self::Words => "SS-04",
            Self::Page | Self::Stage | Self::Wrong => "SS-10",
            Self::Key => "SS-11",
            Self::DataKey | Self::Open => "SS-20",
            Self::Incomplete | Self::OtherWallet => "SS-23",
            Self::Plan(_)
            | Self::Sign(_)
            | Self::NotStarterCoin(_)
            | Self::NotOnlyThese(_)
            | Self::NoFeeRate
            | Self::NotEnough { .. }
            | Self::Wallet => "SS-30",
            Self::NothingToMove | Self::NewAddress | Self::Unconfirmed => "SS-42",
        }
    }

    /// The placeholder that is the refusal's words.
    pub fn placeholder(&self) -> &'static str {
        match self {
            Self::Random => WORDS_NEEDED_SS_01_RANDOM,
            Self::RandomRepeat => WORDS_NEEDED_SS_02_REPEAT,
            Self::RandomWeak => WORDS_NEEDED_SS_02_WEAK,
            Self::Rolls => WORDS_NEEDED_SS_03_ROLLS,
            Self::Words => WORDS_NEEDED_SS_04_WORDS,
            Self::Page => WORDS_NEEDED_SS_10_PAGE,
            Self::Stage => WORDS_NEEDED_SS_10_STAGE,
            Self::Wrong => WORDS_NEEDED_SS_10_WRONG,
            Self::Key => WORDS_NEEDED_SS_11_KEY,
            Self::DataKey => WORDS_NEEDED_SS_20_KEY,
            Self::Open => WORDS_NEEDED_SS_20_OPEN,
            Self::Incomplete => WORDS_NEEDED_SS_23_INCOMPLETE,
            Self::OtherWallet => WORDS_NEEDED_SS_23_OTHER,
            Self::Plan(_) => WORDS_NEEDED_SS_30_PLAN,
            Self::Sign(_) => WORDS_NEEDED_SS_30_SIGN,
            Self::NotStarterCoin(_) => WORDS_NEEDED_SS_30_COIN,
            Self::NotOnlyThese(_) => WORDS_NEEDED_SS_30_CHECK,
            Self::NoFeeRate => WORDS_NEEDED_SS_30_NO_RATE,
            Self::NotEnough { .. } => WORDS_NEEDED_SS_30_SHORT,
            Self::Wallet => WORDS_NEEDED_SS_30_WALLET,
            Self::NothingToMove => WORDS_NEEDED_SS_42_NOTHING,
            Self::NewAddress => WORDS_NEEDED_SS_42_ADDRESS,
            Self::Unconfirmed => WORDS_NEEDED_SS_42_UNCONFIRMED,
        }
    }

    /// What the owner is told: the placeholder, with the library's own text
    /// as the detail where it has one.
    pub fn words(&self) -> Words {
        let words = needed(self.placeholder()).code(self.rule());
        match self {
            Self::Plan(why)
            | Self::Sign(why)
            | Self::NotStarterCoin(why)
            | Self::NotOnlyThese(why) => words.detail(why.clone()),
            _ => words,
        }
    }
}

/// A value of each refusal, with the name of its variant, for the words of
/// every state to be listed.
pub fn all_refusals() -> Vec<(&'static str, StarterRefusal)> {
    vec![
        ("Random", StarterRefusal::Random),
        ("RandomRepeat", StarterRefusal::RandomRepeat),
        ("RandomWeak", StarterRefusal::RandomWeak),
        ("Rolls", StarterRefusal::Rolls),
        ("Words", StarterRefusal::Words),
        ("Page", StarterRefusal::Page),
        ("Stage", StarterRefusal::Stage),
        ("Wrong", StarterRefusal::Wrong),
        ("Key", StarterRefusal::Key),
        ("DataKey", StarterRefusal::DataKey),
        ("Open", StarterRefusal::Open),
        ("Incomplete", StarterRefusal::Incomplete),
        ("OtherWallet", StarterRefusal::OtherWallet),
        ("Plan", StarterRefusal::Plan("no coin".to_string())),
        ("Sign", StarterRefusal::Sign("no signature".to_string())),
        (
            "NotStarterCoin",
            StarterRefusal::NotStarterCoin("coin 0".to_string()),
        ),
        (
            "NotOnlyThese",
            StarterRefusal::NotOnlyThese("an output".to_string()),
        ),
        ("NoFeeRate", StarterRefusal::NoFeeRate),
        (
            "NotEnough",
            StarterRefusal::NotEnough {
                available: 1,
                needed: 2,
            },
        ),
        ("Wallet", StarterRefusal::Wallet),
        ("NothingToMove", StarterRefusal::NothingToMove),
        ("NewAddress", StarterRefusal::NewAddress),
        ("Unconfirmed", StarterRefusal::Unconfirmed),
    ]
}

fn info(placeholder: &'static str, code: &'static str) -> Words {
    Words::new(
        Tone::Info,
        placeholder,
        placeholder,
        Who::Owner,
        placeholder,
    )
    .code(code)
}

/// SS-36: the words of the home screen's line.
pub fn line_words() -> Words {
    info(WORDS_NEEDED_SS_36_LINE, "SS-36")
}

/// SS-37, SS-36: the words of a state of the limit and the date; none when
/// the wallet is under its limit and before its date.
pub fn state_words(state: LimitState) -> Option<Words> {
    match state {
        LimitState::Under => None,
        LimitState::AtLimit => Some(info(WORDS_NEEDED_SS_37_AT_LIMIT, "SS-37")),
        LimitState::PastDate => Some(info(WORDS_NEEDED_SS_37_PAST_DATE, "SS-37")),
        LimitState::NotYet { .. } => Some(info(WORDS_NEEDED_SS_37_NOT_YET, "SS-37")),
    }
}

/// SS-37: the words said when a change of settings waits.
pub fn held_words() -> Words {
    info(WORDS_NEEDED_SS_37_HELD, "SS-37")
}

/// SS-38: the words a counter says when it offers no bitcoin.
pub fn no_bitcoin_words() -> Words {
    info(WORDS_NEEDED_SS_38_NO_BITCOIN, "SS-38")
}

/// SS-43: the words said when the key is deleted.
pub fn deleted_words() -> Words {
    info(WORDS_NEEDED_SS_43_DELETED, "SS-43")
}

/// Every state of this module that has words, by name.
pub fn all_starter_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = all_refusals()
        .into_iter()
        .map(|(n, r)| (format!("StarterRefusal/{n}"), r.words()))
        .collect();
    all.push(("StarterLine".to_string(), line_words()));
    for (name, state) in [
        ("AtLimit", LimitState::AtLimit),
        ("PastDate", LimitState::PastDate),
        ("NotYet", LimitState::NotYet { until: 1 }),
    ] {
        if let Some(w) = state_words(state) {
            all.push((format!("StarterState/{name}"), w));
        }
    }
    all.push(("StarterHeld".to_string(), held_words()));
    all.push(("StarterNoBitcoin".to_string(), no_bitcoin_words()));
    all.push(("StarterDeleted".to_string(), deleted_words()));
    all
}

/// Overwrites `bytes` with zeros in a way the compiler keeps.
pub(crate) fn wipe(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        *b = 0;
    }
    // Keep the writes from being dropped as dead stores.
    std::hint::black_box(&bytes);
}

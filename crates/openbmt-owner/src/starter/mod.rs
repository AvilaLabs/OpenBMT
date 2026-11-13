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
//! - [`watch`] (SS-36, SS-42, SS-43): what the owner node's watch says the
//!   wallet holds, and a signed transaction handed to the node's outbox.
//!
//! The library keeps nothing between calls but a [`Setup`] or a [`Restore`]
//! that the caller holds. A call that signs takes the sealed seed and the
//! data key, opens the seed for that call only and wipes it. The caller
//! passes in the coins, the fee rate and the time; the library has no
//! network and no clock. The kernel's bytes come through [`KernelRandom`]
//! so that a test can give fixed ones.
//!
//! Every refusal is a [`StarterRefusal`]. The words a person sees are listed
//! by [`all_starter_words`] and checked with the owner's other words (SC-12).

mod chance;
mod limit;
mod pay;
mod seal;
mod setup;
#[cfg(test)]
mod tests;
mod watch;

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
pub use watch::{held_sats, IncomingCoin};

pub use openbmt_spend::Coin;

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

    /// What the owner is told, with the library's own text as the detail
    /// where it has one.
    pub fn words(&self) -> Words {
        let rule = self.rule();
        let words = match self {
            Self::Random => stop(
                "No random numbers",
                "The phone's system gave no random numbers, so no wallet was made.",
                "Restart the phone and try again.",
                rule,
            ),
            Self::RandomRepeat => stop(
                "Random numbers repeated",
                "The phone gave the same random numbers twice, so no wallet was made.",
                "Make the key on the stick instead. These random numbers cannot be trusted.",
                rule,
            ),
            Self::RandomWeak => stop(
                "Random numbers look wrong",
                "The phone's random numbers were one byte over and over, so no wallet was made.",
                "Make the key on the stick instead. This phone's random numbers cannot be trusted.",
                rule,
            ),
            Self::Rolls => stop(
                "Not a die roll",
                "Each roll is a number from 1 to 6.",
                "Type the roll again.",
                rule,
            ),
            Self::Words => stop(
                "Not 12 good words",
                "These are not 12 words of the list, or one of them is wrong or out of order.",
                "Check each word against the card.",
                rule,
            ),
            Self::Page => stop(
                "No such page of words",
                "The words are on three pages of four.",
                "Go back and show the words again.",
                rule,
            ),
            Self::Stage => stop(
                "Not at this step",
                "The steps go in order: the words, then the check.",
                "Go back to the words.",
                rule,
            ),
            Self::Wrong => stop(
                "That word is not right",
                "The word typed is not the word with that number on the card.",
                "The words are shown again. Check the card, then answer again.",
                rule,
            ),
            Self::Key => stop(
                "Not a key here",
                "Only letters, Backspace and Enter work here.",
                "Type the word's letters.",
                rule,
            ),
            Self::DataKey => stop(
                "Cannot open the key",
                "The phone's key store gave back a key of the wrong size, so nothing was signed.",
                "Try again. If it happens again, restore from the card of starter words.",
                rule,
            ),
            Self::Open => stop(
                "The key will not open",
                "This phone no longer opens the starter wallet's key. Removing the screen lock does this.",
                "Restore it by typing the 12 words from the card.",
                rule,
            ),
            Self::Incomplete => stop(
                "Not all 12 words yet",
                "The restore needs all 12 words from the card.",
                "Type the rest of the words.",
                rule,
            ),
            Self::OtherWallet => stop(
                "Not this wallet's words",
                "These 12 words make a different wallet from the starter wallet.",
                "Check the card, then type the starter wallet's words.",
                rule,
            ),
            Self::Plan(_) => stop(
                "Cannot make this payment",
                "The starter wallet's coins do not make this payment.",
                "Check the amount and try again.",
                rule,
            ),
            Self::Sign(_) => stop(
                "The payment was not signed",
                "The starter wallet's key did not sign this payment, so nothing was sent.",
                "Try again. If it happens again, restore from the card of starter words.",
                rule,
            ),
            Self::NotStarterCoin(_) => stop(
                "Not the starter's money",
                "A coin handed in is not the starter wallet's, so nothing was signed.",
                "Refresh the wallet and try again.",
                rule,
            ),
            Self::NotOnlyThese(_) => stop(
                "The payment failed a check",
                "The finished payment did not pay only what was asked, so it was not sent.",
                "Nothing left the wallet. Try again.",
                rule,
            ),
            Self::NoFeeRate => stop(
                "No fee rate to pay",
                "The phone has no recent fee rate and none was typed.",
                "Wait for the phone to connect, or type a fee rate.",
                rule,
            ),
            Self::NotEnough { .. } => stop(
                "Not enough in the wallet",
                "The starter wallet holds less than this payment and its fee.",
                "It stays in the queue for the shop's other wallet.",
                rule,
            ),
            Self::Wallet => stop(
                "Cannot read the wallet",
                "The starter wallet's description does not read on this phone.",
                "Restore from the card of starter words.",
                rule,
            ),
            Self::NothingToMove => stop(
                "Nothing to move",
                "The starter wallet holds no money, so there is nothing to move.",
                "No payment is needed.",
                rule,
            ),
            Self::NewAddress => stop(
                "No address in new wallet",
                "This phone cannot work out an address from the new wallet's description.",
                "Connect the new wallet again from its own screen.",
                rule,
            ),
            Self::Unconfirmed => stop(
                "Wait for a payment",
                "A payment into the starter wallet is not confirmed yet. The move waits so that it takes everything at once.",
                "Try again after its first confirmation.",
                rule,
            ),
        };
        match self {
            Self::Plan(why)
            | Self::Sign(why)
            | Self::NotStarterCoin(why)
            | Self::NotOnlyThese(why) => words.detail(why.clone()),
            _ => words,
        }
    }
}

fn stop(heading: &str, what: &str, next: &str, code: &'static str) -> Words {
    Words::new(Tone::Stop, heading, what, Who::Owner, next).code(code)
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

/// SS-36: the words of the home screen's line.
pub fn line_words() -> Words {
    Words::new(
        Tone::Info,
        "Starter wallet",
        "What it holds, the limit you set and your date to move to a key made offline.",
        Who::Owner,
        "Move before the limit or the date, from Upgrade now.",
    )
    .code("SS-36")
}

/// SS-37, SS-36: the words of a state of the limit and the date; none when
/// the wallet is under its limit and before its date.
pub fn state_words(state: LimitState) -> Option<Words> {
    let (heading, what, next) = match state {
        LimitState::Under => return None,
        LimitState::AtLimit => (
            "Starter wallet at limit",
            "The starter wallet holds as much as the limit you set. Sales go on.",
            "Upgrade now to a key made offline, or choose Not yet.",
        ),
        LimitState::PastDate => (
            "Time to upgrade",
            "The date you set to move to a key made offline has come. Sales go on.",
            "Upgrade now, or choose Not yet.",
        ),
        LimitState::NotYet { .. } => (
            "Upgrade put off",
            "You chose Not yet. Until it ends, raising the limit, adding counters and other changes wait.",
            "Upgrade when you can. Not yet lasts at most 7 days.",
        ),
    };
    Some(Words::new(Tone::Info, heading, what, Who::Owner, next).code("SS-37"))
}

/// SS-37: the words said when a change of settings waits.
pub fn held_words() -> Words {
    Words::new(
        Tone::Info,
        "This change waits",
        "While the starter wallet is at its limit or past its date, changes that let it hold more wait.",
        Who::Owner,
        "Upgrade to a key made offline, then make the change.",
    )
    .code("SS-37")
}

/// SS-38: the words a counter says when it offers no bitcoin.
pub fn no_bitcoin_words() -> Words {
    Words::new(
        Tone::Info,
        "Bitcoin paused",
        "The shop's starter wallet is full, so this counter takes no bitcoin for now.",
        Who::Cashier,
        "Take another way to pay.",
    )
    .code("SS-38")
}

/// SS-43: the words said when the key is deleted.
pub fn deleted_words() -> Words {
    Words::new(
        Tone::Good,
        "Starter key deleted",
        "The starter wallet's key is gone from this phone. Its money is in the new wallet.",
        Who::Owner,
        "The card of starter words is no longer needed. Destroy it.",
    )
    .code("SS-43")
}

/// SS-10: no starter wallet is being made, for a step of the setup.
pub fn no_setup_words() -> Words {
    stop(
        "No wallet being made",
        "This step belongs to making a starter wallet, and none is being made.",
        "Start again from Start simple.",
        "SS-10",
    )
}

/// SS-30: a coin's transaction, as the app hands it in, does not read.
pub fn coin_unreadable_words() -> Words {
    stop(
        "Cannot read a payment",
        "A payment into the starter wallet does not read on this phone, so nothing was signed.",
        "Refresh the wallet and try again.",
        "SS-30",
    )
}

/// SS-42: the new wallet's description does not read.
pub fn new_wallet_unreadable_words() -> Words {
    stop(
        "Cannot read new wallet",
        "The new wallet's description does not read on this phone, so nothing was moved.",
        "Connect the new wallet again from its own screen.",
        "SS-42",
    )
}

/// SS-36: the owner node's watch of the starter wallet is not ready, so
/// what it holds is not known. The words are not written yet.
pub fn not_watching_words() -> Words {
    needed("WORDS_NEEDED_SS36_NOT_WATCHING")
}

/// SS-30: the refund to pay is not a refund that waits in the queue. The
/// words are not written yet.
pub fn no_such_refund_words() -> Words {
    needed("WORDS_NEEDED_SS30_NO_SUCH_REFUND")
}

/// The words of this module that are not written yet, by name: each is the
/// placeholder constant that names its rule, in the heading and both
/// sentences. They are not in [`all_starter_words`], which holds written
/// words only.
pub fn all_needed_words() -> Vec<(String, Words)> {
    vec![
        ("StarterNotWatching".to_string(), not_watching_words()),
        ("StarterNoSuchRefund".to_string(), no_such_refund_words()),
    ]
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
    all.push(("StarterNoSetup".to_string(), no_setup_words()));
    all.push(("StarterCoinUnreadable".to_string(), coin_unreadable_words()));
    all.push((
        "StarterNewWalletUnreadable".to_string(),
        new_wallet_unreadable_words(),
    ));
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

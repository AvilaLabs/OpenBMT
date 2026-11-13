//! The refund float (`docs/FLOAT-RULES.md`, FL-01 to FL-40): a small wallet
//! on the owner phone for paying refunds at once, refilled from the shop's
//! savings and capped, so that a lost phone loses at most the float.
//!
//! The float is the single-key native SegWit wallet (`m/84'/0'/0'`, no
//! passphrase) of 12 words that are a BIP-85 child of the shop's words
//! (FL-01, FL-02). The child is made offline, on the stick or on a signer;
//! this library never takes the shop's own words, and the BIP-85 derivation
//! is checked in the tests only.
//!
//! The library keeps nothing (FL-03). A call that signs ([`pay_refund`],
//! [`sweep`]) is given the words in a [`FloatWords`], which wipes its buffer
//! when dropped, and returns the finished transaction and nothing secret.
//! The calls that only need the float's public description
//! ([`prepare_refill`], [`FloatWallet::receive_address`]) take that and no
//! words. The caller passes in the coins, the balance and the fee rate; the
//! library has no network and no clock.
//!
//! Every refusal is a [`FloatRefusal`]. The words a person would see are
//! listed by [`all_float_words`] and checked as every other words are
//! (SC-12).
//!
//! The cap of FL-10 is a number the caller gives each call: the owner
//! phone's own record, not a shop setting.

use std::fmt;
use std::str::FromStr;

use bip39::{Language, Mnemonic};
use bitcoin::bip32::{DerivationPath, Xpriv};
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::{Address, Network, OutPoint, ScriptBuf, Transaction, Txid};
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_spend::{
    check_finished, finalize_plan, plan, ChangePosition, Plan, PlanError, SpendRequest,
};

pub use openbmt_spend::Coin;

use crate::error::OwnerError;
use crate::single_key::{check_pays_only, sign_p2wpkh_inputs, Breach};
use crate::starter::{seal_seed, KernelRandom, Sealed, Seed, StarterRefusal, ENTROPY_BYTES};
use crate::state::Owner;
use crate::types::CashAmount;

/// FL-01: the number of words of the float.
pub const FLOAT_WORDS: usize = 12;
/// FL-12: the float is low below this share of the cap, as `1/LOW_DIVISOR`.
pub const LOW_DIVISOR: u64 = 4;
/// FL-22: the least fee rate of a refund, in satoshis per 1000 virtual
/// bytes (1 sat/vB), as `docs/OWNER-COINS-RULES.md` OC-17 sets the least.
pub const MIN_REFUND_RATE_PER_KVB: u64 = 1_000;
/// FL-30: the label of a refill in the queue's cash-outs.
pub const REFILL_LABEL: &str = "Refund float";

/// Why a step of the float was refused. Nothing was changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloatRefusal {
    /// FL-01: not 12 English words with a right check word.
    Words,
    /// FL-10: the cap is 0.
    NoCap,
    /// FL-11: the float holds the cap or more.
    NothingToRefill,
    /// FL-11: the refill would leave the float above the cap.
    AboveCap {
        /// The most a refill may pay now.
        room: u64,
        /// What was asked.
        asked: u64,
    },
    /// FL-11: a refill of 0, or sums that overflow.
    Amount,
    /// FL-20: the spend crate could not plan it (the text says why).
    Plan(String),
    /// FL-20: signing or finishing failed (the text says why).
    Sign(String),
    /// FL-21: a coin handed in is not a coin of the float's wallet.
    NotFloatCoin(String),
    /// FL-21: the finished transaction is not only the float's coins paying
    /// only what it should (the text says which check).
    NotOnlyFloat(String),
    /// FL-22: no recent rate and none typed.
    NoFeeRate,
    /// FL-23: the float holds too little for the refund and its fee.
    NotEnough {
        /// What the eligible coins hold.
        available: u64,
        /// What the refund and the fee need.
        needed: u64,
    },
    /// FL-30: the description or an address of the float cannot be made.
    Wallet,
    /// FL-35: no coin to move.
    NothingToSweep,
    /// FL-35: the shop's address cannot be derived from its description.
    ShopAddress,
}

impl fmt::Display for FloatRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Words => write!(f, "FL-01: not 12 words of the English list"),
            Self::NoCap => write!(f, "FL-10: the cap is 0"),
            Self::NothingToRefill => write!(f, "FL-11: the float holds the cap or more"),
            Self::AboveCap { room, asked } => write!(
                f,
                "FL-11: a refill of {asked} sat would leave the float above the cap; the room is {room} sat"
            ),
            Self::Amount => write!(f, "FL-11: the refill amount is 0 or the sums overflow"),
            Self::Plan(why) => write!(f, "FL-20: the payment cannot be planned: {why}"),
            Self::Sign(why) => write!(f, "FL-20: the payment cannot be signed: {why}"),
            Self::NotFloatCoin(why) => write!(f, "FL-21: a coin is not the float's: {why}"),
            Self::NotOnlyFloat(why) => write!(f, "FL-21: {why}"),
            Self::NoFeeRate => write!(f, "FL-22: there is no fee rate"),
            Self::NotEnough { available, needed } => write!(
                f,
                "FL-23: the float's coins hold {available} sat; the refund and its fee need {needed} sat"
            ),
            Self::Wallet => write!(f, "FL-30: the float's wallet cannot be read"),
            Self::NothingToSweep => write!(f, "FL-35: the float holds no coin"),
            Self::ShopAddress => write!(f, "FL-35: the shop's address cannot be derived"),
        }
    }
}

impl std::error::Error for FloatRefusal {}

impl FloatRefusal {
    /// The rule ID of the refusal.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Words => "FL-01",
            Self::NoCap => "FL-10",
            Self::NothingToRefill | Self::AboveCap { .. } | Self::Amount => "FL-11",
            Self::Plan(_) | Self::Sign(_) => "FL-20",
            Self::NotFloatCoin(_) | Self::NotOnlyFloat(_) => "FL-21",
            Self::NoFeeRate => "FL-22",
            Self::NotEnough { .. } => "FL-23",
            Self::Wallet => "FL-30",
            Self::NothingToSweep | Self::ShopAddress => "FL-35",
        }
    }

    /// What the owner is told, with the library's own text as the detail
    /// where it has one.
    pub fn words(&self) -> Words {
        let code = self.rule();
        let words = match self {
            Self::Words => stop(
                "Not the float's words",
                "The code scanned is not 12 words of the list with a right check word.",
                "Nothing was changed. Scan the refund float code on the stick again.",
                code,
            ),
            Self::NoCap => stop(
                "The float is off",
                "No cap is set for the refund float, so it holds nothing and pays nothing.",
                "Nothing was changed. Set a cap for the refund float to turn it on.",
                code,
            ),
            Self::NothingToRefill => stop(
                "The float is full",
                "The refund float already holds its cap or more.",
                "Nothing was changed. Refill it once refunds have used some of it.",
                code,
            ),
            Self::AboveCap { .. } => stop(
                "That is over the cap",
                "That refill would leave the refund float holding more than its cap.",
                "Nothing was changed. Refill the amount shown, or less.",
                code,
            ),
            Self::Amount => stop(
                "No amount to refill",
                "The refill amount is zero, or too large to add up.",
                "Nothing was changed. Enter an amount above zero.",
                code,
            ),
            Self::Plan(_) => stop(
                "Cannot make the refund",
                "The refund could not be made from the refund float's money.",
                "Nothing was changed. The refund is still waiting in Send from your wallet.",
                code,
            ),
            Self::Sign(_) => stop(
                "The float did not sign",
                "The refund float's key could not sign or finish the payment.",
                "Nothing was changed. Try again, or pay the refund from your wallet.",
                code,
            ),
            Self::NotFloatCoin(_) => stop(
                "Not the float's money",
                "A payment picked for this is not in the refund float.",
                "Nothing was changed. Let the float's balance catch up, then try again.",
                code,
            ),
            Self::NotOnlyFloat(_) => stop(
                "The refund failed a check",
                "The finished payment would pay something other than the refund and the float's own change.",
                "Nothing was sent. The refund is still waiting.",
                code,
            ),
            Self::NoFeeRate => stop(
                "No fee rate yet",
                "The phone has no recent fee rate from the network, and none was typed.",
                "Nothing was changed. Wait for the next block, or type a fee rate.",
                code,
            ),
            Self::NotEnough { .. } => stop(
                "Not enough in the float",
                "The refund and its fee come to more than the refund float holds. A refund is never split.",
                "Nothing was changed. Refill the float first, or pay the refund from your wallet.",
                code,
            ),
            Self::Wallet => stop(
                "Cannot read the float",
                "The refund float's description or address could not be read or made.",
                "Nothing was changed. Make the float's code on the stick again and scan it.",
                code,
            ),
            Self::NothingToSweep => stop(
                "The float is empty",
                "The refund float holds nothing to move.",
                "Nothing was moved. The float can be turned off as it is.",
                code,
            ),
            Self::ShopAddress => stop(
                "No address to move it to",
                "The shop's wallet address for the float's money could not be made.",
                "Nothing was moved. Check the shop's wallet is connected, then try again.",
                code,
            ),
        };
        match self {
            Self::Plan(why)
            | Self::Sign(why)
            | Self::NotFloatCoin(why)
            | Self::NotOnlyFloat(why) => words.detail(why.clone()),
            _ => words,
        }
    }
}

fn stop(heading: &str, what: &str, next: &str, code: &'static str) -> Words {
    Words::new(Tone::Stop, heading, what, Who::Owner, next).code(code)
}

/// A value of each refusal, with the name of its variant, for the words of
/// every state to be listed.
pub fn all_refusals() -> Vec<(&'static str, FloatRefusal)> {
    vec![
        ("Words", FloatRefusal::Words),
        ("NoCap", FloatRefusal::NoCap),
        ("NothingToRefill", FloatRefusal::NothingToRefill),
        ("AboveCap", FloatRefusal::AboveCap { room: 1, asked: 2 }),
        ("Amount", FloatRefusal::Amount),
        ("Plan", FloatRefusal::Plan("no coin".to_string())),
        ("Sign", FloatRefusal::Sign("no signature".to_string())),
        (
            "NotFloatCoin",
            FloatRefusal::NotFloatCoin("coin 0".to_string()),
        ),
        (
            "NotOnlyFloat",
            FloatRefusal::NotOnlyFloat("an output".to_string()),
        ),
        ("NoFeeRate", FloatRefusal::NoFeeRate),
        (
            "NotEnough",
            FloatRefusal::NotEnough {
                available: 1,
                needed: 2,
            },
        ),
        ("Wallet", FloatRefusal::Wallet),
        ("NothingToSweep", FloatRefusal::NothingToSweep),
        ("ShopAddress", FloatRefusal::ShopAddress),
    ]
}

/// FL-20: a coin handed in by the app could not be read.
pub fn coin_unreadable_words() -> Words {
    stop(
        "Cannot read a float payment",
        "A payment into the refund float could not be read on this phone.",
        "Nothing was changed. Let the float's balance catch up, then try once more.",
        "FL-20",
    )
}

/// FL-35: the shop's pinned wallet description could not be read.
pub fn shop_wallet_unreadable_words() -> Words {
    stop(
        "Cannot read the shop wallet",
        "The shop's wallet description on this phone could not be read.",
        "Nothing was moved. Connect the shop's wallet again, then turn the float off.",
        "FL-35",
    )
}

/// FL-12: the words of the home screen's line when the float is low.
pub fn low_words() -> Words {
    Words::new(
        Tone::Info,
        "Refund float low",
        "The refund float holds less than a quarter of its cap.",
        Who::Owner,
        "Refill it from savings in Send from your wallet.",
    )
    .code("FL-12")
}

/// FL-31: the words said before a refill goes.
pub fn refill_linked_words() -> Words {
    Words::new(
        Tone::Info,
        "A refill links your wallets",
        "Anyone who looks at the chain can see that this payment joins your savings to the refund float.",
        Who::Owner,
        "Send it if that is fine. It is the same for every refill.",
    )
    .code("FL-31")
}

/// Every state of this module that has words, by name.
pub fn all_float_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = all_refusals()
        .into_iter()
        .map(|(n, r)| (format!("FloatRefusal/{n}"), r.words()))
        .collect();
    all.push(("FloatLow".to_string(), low_words()));
    all.push(("FloatCoinUnreadable".to_string(), coin_unreadable_words()));
    all.push((
        "FloatShopWalletUnreadable".to_string(),
        shop_wallet_unreadable_words(),
    ));
    all.push(("FloatRefillLinked".to_string(), refill_linked_words()));
    all
}

// ---- the words -----------------------------------------------------------

/// FL-01, FL-03: the 12 words of the float, in memory only while a call
/// uses them. The buffer is overwritten when the value is dropped, and
/// `Debug` hides it.
pub struct FloatWords(String);

impl FloatWords {
    /// FL-01: 12 words of the English list with a right check word. Case
    /// and the white space between words are not kept: the words are stored
    /// lower case, one space apart.
    pub fn new(words: &str) -> Result<Self, FloatRefusal> {
        let tidy = words
            .split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>()
            .join(" ");
        let held = Self(tidy);
        if held.0.split(' ').count() != FLOAT_WORDS {
            return Err(FloatRefusal::Words);
        }
        Mnemonic::parse_in_normalized(Language::English, &held.0)
            .map_err(|_| FloatRefusal::Words)?;
        Ok(held)
    }

    /// FL-01: the words of a Compact SeedQR the owner scanned: the 16 bytes
    /// of entropy of 12 words (`docs/STICK-RULES.md` SK-27).
    pub fn from_compact_seedqr(entropy: &[u8]) -> Result<Self, FloatRefusal> {
        if entropy.len() != 16 {
            return Err(FloatRefusal::Words);
        }
        let m = Mnemonic::from_entropy_in(Language::English, entropy)
            .map_err(|_| FloatRefusal::Words)?;
        let words = m.words().collect::<Vec<_>>().join(" ");
        Self::new(&words)
    }

    /// The words, space-separated, for the caller to show or seal once.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for FloatWords {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FloatWords(<hidden>)")
    }
}

impl Drop for FloatWords {
    fn drop(&mut self) {
        wipe_string(&mut self.0);
    }
}

fn wipe_string(s: &mut String) {
    let mut bytes = std::mem::take(s).into_bytes();
    wipe(&mut bytes);
}

fn wipe(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        *b = 0;
    }
    // Keep the writes from being dropped as dead stores.
    std::hint::black_box(&bytes);
}

/// FL-03: what [`seal_compact_seedqr`] gives: the sealed seed, which the
/// app stores, with the data key that opens it, which the app wraps with its
/// keystore key at once; and the float's public wallet, which holds no
/// secret.
#[derive(Debug)]
pub struct FloatSealed {
    /// The sealed seed and its data key.
    pub sealed: Sealed,
    /// The float's public wallet.
    pub wallet: FloatWallet,
}

/// Why the words of a scan were not sealed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloatSealError {
    /// The scan is not the 16 bytes of a Compact SeedQR of 12 words (FL-01).
    Float(FloatRefusal),
    /// The sealing itself was refused: the kernel gave no usable bytes
    /// (SS-02).
    Seal(StarterRefusal),
}

/// FL-01, FL-03: the 16 bytes of a Compact SeedQR the owner scanned, turned
/// into the float's seed here and sealed as the starter wallet's seed is
/// sealed (SS-20): the words are never text outside this call. The sealed
/// seed opens with `starter::wallet_of_sealed`, the data key is given back
/// once, and the entropy of the scan is the caller's to overwrite.
pub fn seal_compact_seedqr(
    entropy: &[u8],
    rng: &mut dyn KernelRandom,
) -> Result<FloatSealed, FloatSealError> {
    let words = FloatWords::from_compact_seedqr(entropy).map_err(FloatSealError::Float)?;
    let wallet = FloatWallet::from_words(&words).map_err(FloatSealError::Float)?;
    let mut bytes = <[u8; ENTROPY_BYTES]>::try_from(entropy)
        .map_err(|_| FloatSealError::Float(FloatRefusal::Words))?;
    let seed = Seed::from_entropy(bytes);
    wipe(&mut bytes);
    let sealed = seal_seed(&seed, rng).map_err(FloatSealError::Seal)?;
    Ok(FloatSealed { sealed, wallet })
}

/// The float's wallet and its account key, only inside one call.
struct Opened {
    account: Xpriv,
    wallet: SingleKeyWallet,
}

impl Drop for Opened {
    fn drop(&mut self) {
        self.account.private_key.non_secure_erase();
    }
}

fn open(words: &FloatWords) -> Result<Opened, FloatRefusal> {
    let secp = Secp256k1::new();
    let mnemonic = Mnemonic::parse_in_normalized(Language::English, &words.0)
        .map_err(|_| FloatRefusal::Words)?;
    let mut seed = mnemonic.to_seed("");
    let master = Xpriv::new_master(Network::Bitcoin, &seed);
    wipe(&mut seed);
    let mut master = master.map_err(|_| FloatRefusal::Wallet)?;
    let path = DerivationPath::from_str("m/84'/0'/0'").map_err(|_| FloatRefusal::Wallet)?;
    let account = master
        .derive_priv(&secp, &path)
        .map_err(|_| FloatRefusal::Wallet);
    let fingerprint = master.fingerprint(&secp);
    master.private_key.non_secure_erase();
    let account = account?;
    let xpub = bitcoin::bip32::Xpub::from_priv(&secp, &account);
    let wallet = SingleKeyWallet::new(fingerprint, 0, xpub).map_err(|_| FloatRefusal::Wallet)?;
    Ok(Opened { account, wallet })
}

/// FL-02: the float's public wallet, which the app may keep: it holds no
/// secret. Receive and change scripts are P2WPKH of the account `84'/0'/0'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatWallet(SingleKeyWallet);

impl FloatWallet {
    /// FL-02: the wallet of 12 words.
    pub fn from_words(words: &FloatWords) -> Result<Self, FloatRefusal> {
        Ok(Self(open(words)?.wallet.clone()))
    }

    /// FL-30: the wallet from its canonical description, as the app kept it.
    pub fn parse(description: &str) -> Result<Self, FloatRefusal> {
        let w = SingleKeyWallet::parse(description).map_err(|_| FloatRefusal::Wallet)?;
        if w.account() != 0 {
            return Err(FloatRefusal::Wallet);
        }
        Ok(Self(w))
    }

    /// The description, checksum included.
    pub fn description(&self) -> &str {
        self.0.canonical_string()
    }

    /// The wallet as the spend crate takes it.
    pub fn imported(&self) -> ImportedWallet {
        ImportedWallet::Single(self.0.clone())
    }

    /// The script of the float at `chain/index`.
    pub fn script(&self, chain: Keychain, index: u32) -> Result<ScriptBuf, FloatRefusal> {
        self.0
            .script(chain, index)
            .map_err(|_| FloatRefusal::Wallet)
    }

    /// FL-30: the receive address at `index`, the next unused one that the
    /// caller counted.
    pub fn receive_address(&self, index: u32) -> Result<Address, FloatRefusal> {
        self.0
            .address(Keychain::Receive, index)
            .map_err(|_| FloatRefusal::Wallet)
    }
}

// ---- the cap -------------------------------------------------------------

/// FL-11: what the float holds: confirmed plus the payments into it not yet
/// confirmed.
pub fn held_sats(confirmed: u64, unconfirmed_incoming: u64) -> Result<u64, FloatRefusal> {
    confirmed
        .checked_add(unconfirmed_incoming)
        .ok_or(FloatRefusal::Amount)
}

/// FL-11: the amount of a refill, the cap less what the float holds
/// (confirmed and unconfirmed incoming).
pub fn refill_amount(
    cap: u64,
    confirmed: u64,
    unconfirmed_incoming: u64,
) -> Result<u64, FloatRefusal> {
    if cap == 0 {
        return Err(FloatRefusal::NoCap);
    }
    let held = held_sats(confirmed, unconfirmed_incoming)?;
    match cap.checked_sub(held) {
        Some(room) if room > 0 => Ok(room),
        _ => Err(FloatRefusal::NothingToRefill),
    }
}

/// FL-11: a refill of `amount` is refused when it is 0 or would leave the
/// float above the cap.
pub fn check_refill_amount(
    cap: u64,
    confirmed: u64,
    unconfirmed_incoming: u64,
    amount: u64,
) -> Result<(), FloatRefusal> {
    let room = refill_amount(cap, confirmed, unconfirmed_incoming)?;
    if amount == 0 {
        return Err(FloatRefusal::Amount);
    }
    if amount > room {
        return Err(FloatRefusal::AboveCap {
            room,
            asked: amount,
        });
    }
    Ok(())
}

/// FL-12: the float is low when it holds less than a quarter of the cap
/// (confirmed and unconfirmed incoming). A cap of 0 is off, never low.
pub fn is_low(cap: u64, confirmed: u64, unconfirmed_incoming: u64) -> bool {
    if cap == 0 {
        return false;
    }
    let held = u128::from(confirmed) + u128::from(unconfirmed_incoming);
    held * u128::from(LOW_DIVISOR) < u128::from(cap)
}

// ---- the fee -------------------------------------------------------------

/// FL-22: the fee rate of a refund, in satoshis per 1000 virtual bytes: the
/// recent rate of the blocks the device checked (`FeeHistory::recent_rate`,
/// `docs/FEE-RULES.md` FE-13) and at least 1 sat/vB; when there is none, the
/// rate the owner typed (as for every payment of the owner phone), and
/// never 0. With neither, no rate.
pub fn refund_fee_rate(recent: Option<u64>, typed: Option<u64>) -> Result<u64, FloatRefusal> {
    match (recent, typed) {
        (Some(r), _) => Ok(r.max(MIN_REFUND_RATE_PER_KVB)),
        (None, Some(t)) if t > 0 => Ok(t),
        _ => Err(FloatRefusal::NoFeeRate),
    }
}

// ---- paying a refund -----------------------------------------------------

/// What [`pay_refund`] and [`sweep`] hand back: the finished transaction,
/// ready for the outbox, and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatPaid {
    /// The finished, signed transaction.
    pub tx: Transaction,
    /// Its ID.
    pub txid: Txid,
    /// The fee in satoshis.
    pub fee: u64,
    /// The virtual size.
    pub vsize: u64,
    /// The amount paid to the destination.
    pub paid: u64,
    /// The float's change: its index on the change chain and its amount.
    pub change: Option<(u32, u64)>,
    /// The coins spent.
    pub spent: Vec<OutPoint>,
}

/// FL-20 to FL-23: pay `sats` to `pay_to` (a queued refund) from the float's
/// own `coins`, with change to the float's change script at `change_index`.
///
/// `fee_rate` is in satoshis per 1000 virtual bytes ([`refund_fee_rate`]).
/// `min_confirmations` is the fewest a coin needs (0 also spends the float's
/// own change that is not yet confirmed). A refund larger than the coins can
/// cover with its fee is refused with [`FloatRefusal::NotEnough`] and is
/// never split (FL-23). Before it is returned the finished transaction is
/// checked to spend only the float's coins and to pay only `pay_to` and the
/// float's change (FL-21).
#[allow(clippy::too_many_arguments)]
pub fn pay_refund(
    words: &FloatWords,
    coins: &[Coin],
    pay_to: &ScriptBuf,
    sats: u64,
    fee_rate: u64,
    change_index: u32,
    min_confirmations: u32,
    lock_height: u32,
) -> Result<FloatPaid, FloatRefusal> {
    let opened = open(words)?;
    let request = SpendRequest {
        payments: vec![(pay_to.clone(), sats)],
        fee_rate,
        change_index,
        min_confirmations,
        send_all: false,
        must_spend: Vec::new(),
        lock_height,
        change_position: ChangePosition::Last,
    };
    spend(&opened, coins, &request, Some(sats))
}

/// FL-35: move every coin of the float to `shop_to`, a script of the shop's
/// own wallet. `min_confirmations` is as for [`pay_refund`]; the coins that
/// are not eligible are left out and the caller is told by the amount.
pub fn sweep(
    words: &FloatWords,
    coins: &[Coin],
    shop_to: &ScriptBuf,
    fee_rate: u64,
    min_confirmations: u32,
    lock_height: u32,
) -> Result<FloatPaid, FloatRefusal> {
    if coins.is_empty() {
        return Err(FloatRefusal::NothingToSweep);
    }
    let opened = open(words)?;
    let request = SpendRequest {
        payments: vec![(shop_to.clone(), 0)],
        fee_rate,
        change_index: 0,
        min_confirmations,
        send_all: true,
        must_spend: Vec::new(),
        lock_height,
        change_position: ChangePosition::Last,
    };
    spend(&opened, coins, &request, None)
}

/// FL-35: the script of the shop's wallet at `index` on its receive chain,
/// from the pinned description (the next unused index is the caller's, as
/// the library has no chain). The description is the shop's, so a 2-of-3
/// wallet and a single-key one both work.
pub fn shop_receive_script(shop: &ImportedWallet, index: u32) -> Result<ScriptBuf, FloatRefusal> {
    shop.receive_script(index)
        .map_err(|_| FloatRefusal::ShopAddress)
}

fn plan_refusal(e: PlanError, sats: u64) -> FloatRefusal {
    match e {
        PlanError::InsufficientFunds { available, needed } => {
            FloatRefusal::NotEnough { available, needed }
        }
        PlanError::NoEligibleCoins => FloatRefusal::NotEnough {
            available: 0,
            needed: sats,
        },
        PlanError::Coin(c) => FloatRefusal::NotFloatCoin(c.to_string()),
        other => FloatRefusal::Plan(other.to_string()),
    }
}

fn spend(
    opened: &Opened,
    coins: &[Coin],
    request: &SpendRequest,
    sats: Option<u64>,
) -> Result<FloatPaid, FloatRefusal> {
    let wallet = ImportedWallet::Single(opened.wallet.clone());
    let planned = plan(&wallet, coins, request).map_err(|e| plan_refusal(e, sats.unwrap_or(0)))?;
    let mut psbt = planned
        .to_psbt(&wallet)
        .map_err(|e| FloatRefusal::Sign(e.to_string()))?;
    sign_inputs(opened, &planned, &mut psbt)?;
    let tx =
        finalize_plan(&wallet, &psbt, &planned).map_err(|e| FloatRefusal::Sign(e.to_string()))?;
    let fee = check_finished(&wallet, &psbt, &tx).map_err(|e| FloatRefusal::Sign(e.to_string()))?;
    let pay = request
        .payments
        .first()
        .ok_or_else(|| FloatRefusal::Plan("no payment".to_string()))?;
    let change_script = opened
        .wallet
        .change_script(request.change_index)
        .map_err(|_| FloatRefusal::Wallet)?;
    let used: Vec<&Coin> = planned.inputs().iter().map(|i| &i.coin).collect();
    check_float_only(
        &opened.wallet,
        &used,
        &tx,
        &pay.0,
        sats,
        &change_script,
        request.send_all,
    )?;
    let paid: u64 = tx
        .output
        .iter()
        .filter(|o| o.script_pubkey == pay.0)
        .map(|o| o.value.to_sat())
        .sum();
    let change = tx
        .output
        .iter()
        .find(|o| o.script_pubkey == change_script && !request.send_all)
        .map(|o| (request.change_index, o.value.to_sat()));
    Ok(FloatPaid {
        txid: tx.compute_txid(),
        vsize: u64::try_from(tx.vsize()).unwrap_or(u64::MAX),
        fee,
        paid,
        change,
        spent: tx.input.iter().map(|i| i.previous_output).collect(),
        tx,
    })
}

/// FL-20: sign every input with the float's key, `SIGHASH_ALL`, P2WPKH.
fn sign_inputs(opened: &Opened, planned: &Plan, psbt: &mut Psbt) -> Result<(), FloatRefusal> {
    sign_p2wpkh_inputs(&opened.account, planned, psbt).map_err(FloatRefusal::Sign)
}

/// FL-21: the finished transaction `tx` spends only coins of the float
/// (each of `coins`, whose scripts are the float wallet's) and pays only
/// `pay_to` (with `pay_sats` when it is a payment of a set amount) and the
/// float's own change script `change`. A sweep (`sweep` set) pays `pay_to`
/// alone and spends every coin it was given.
pub fn check_float_only(
    wallet: &SingleKeyWallet,
    coins: &[&Coin],
    tx: &Transaction,
    pay_to: &ScriptBuf,
    pay_sats: Option<u64>,
    change: &ScriptBuf,
    sweep: bool,
) -> Result<(), FloatRefusal> {
    let bad = |why: &str| FloatRefusal::NotOnlyFloat(why.to_string());
    check_pays_only(wallet, coins, tx, pay_to, pay_sats, change, sweep).map_err(|b| match b {
        Breach::NoInputs => bad("the transaction spends nothing"),
        Breach::NotACoin => bad("an input is not a coin of the float"),
        Breach::SweepMissesCoins => bad("a sweep must spend every coin"),
        Breach::PaysOwnChange => bad("the payment is to the float's own change"),
        Breach::WrongAmount => bad("the payment is not the amount of the refund"),
        Breach::OtherOutput => bad("an output pays something else"),
        Breach::NotPaidOnce => bad("the destination must be paid exactly once"),
        Breach::ManyChange => bad("more than one change output"),
        Breach::Coin(e) => FloatRefusal::NotFloatCoin(e),
    })
}

// ---- refilling -----------------------------------------------------------

/// FL-30: what [`prepare_refill`] queued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefillQueued {
    /// The cash-out's number in the queue, to pick under "Send from your
    /// wallet".
    pub id: u32,
    /// The amount, in satoshis.
    pub sats: u64,
    /// The float's receive address it pays.
    pub address: Address,
}

/// Why a refill was not queued.
#[derive(Debug)]
pub enum RefillError<E> {
    /// A rule of the float refused.
    Float(FloatRefusal),
    /// The owner queue refused or its store failed.
    Owner(OwnerError<E>),
}

impl<E> From<FloatRefusal> for RefillError<E> {
    fn from(r: FloatRefusal) -> Self {
        Self::Float(r)
    }
}

/// FL-11, FL-30: queue a refill as a cash-out of the shop's own wallet to
/// the float's receive address at `next_index` (the next unused one, as
/// the caller counted), for the owner to send through "Send from your
/// wallet". `amount` is `None` for the cap less what the float holds, or a
/// smaller amount the owner chose; it is refused when it would leave the
/// float above the cap. The queue is the one of `AS-57`; nothing is queued
/// when a check refuses.
#[allow(clippy::too_many_arguments)]
pub fn prepare_refill<E>(
    owner: &mut Owner,
    float: &FloatWallet,
    next_index: u32,
    cap: u64,
    confirmed: u64,
    unconfirmed_incoming: u64,
    amount: Option<u64>,
    persist: impl FnOnce(&[u8]) -> Result<(), E>,
) -> Result<RefillQueued, RefillError<E>> {
    let sats = match amount {
        Some(a) => {
            check_refill_amount(cap, confirmed, unconfirmed_incoming, a)?;
            a
        }
        None => refill_amount(cap, confirmed, unconfirmed_incoming)?,
    };
    let address = float.receive_address(next_index)?;
    let id = owner
        .queue_cash_out(
            address.script_pubkey(),
            CashAmount::Sats(sats),
            REFILL_LABEL,
            persist,
        )
        .map_err(RefillError::Owner)?;
    Ok(RefillQueued { id, sats, address })
}

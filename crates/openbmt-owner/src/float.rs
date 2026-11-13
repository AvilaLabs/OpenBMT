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
//! placeholders that name themselves (`WORDS_NEEDED_FL_xx_...`) until they
//! are written (SC-12), listed by [`all_float_words`].
//!
//! The cap of FL-10 is a number the caller gives each call. Where the signed
//! settings carry it is not decided here: see the report of this build.

use std::fmt;
use std::str::FromStr;

use bip39::{Language, Mnemonic};
use bitcoin::bip32::{ChildNumber, DerivationPath, Xpriv};
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{ecdsa, Address, Network, OutPoint, PublicKey, ScriptBuf, Transaction, Txid};
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_spend::{
    check_finished, finalize_plan, plan, ChangePosition, Plan, PlanError, SpendRequest,
};

pub use openbmt_spend::Coin;

use crate::error::OwnerError;
use crate::own_say::needed;
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

/// FL-01: the 12 words are not 12 words of the English list with a right
/// check word.
pub const WORDS_NEEDED_FL_01_WORDS: &str = "WORDS_NEEDED_FL_01_WORDS";
/// FL-10: the shop has set no cap, so the float is off.
pub const WORDS_NEEDED_FL_10_NO_CAP: &str = "WORDS_NEEDED_FL_10_NO_CAP";
/// FL-11: the float holds the cap or more, so there is nothing to refill.
pub const WORDS_NEEDED_FL_11_NOTHING: &str = "WORDS_NEEDED_FL_11_NOTHING";
/// FL-11: the refill would leave the float above the cap.
pub const WORDS_NEEDED_FL_11_ABOVE_CAP: &str = "WORDS_NEEDED_FL_11_ABOVE_CAP";
/// FL-11: the refill amount is zero or the sums overflow.
pub const WORDS_NEEDED_FL_11_AMOUNT: &str = "WORDS_NEEDED_FL_11_AMOUNT";
/// FL-12: the float is low (the home screen's line).
pub const WORDS_NEEDED_FL_12_LOW: &str = "WORDS_NEEDED_FL_12_LOW";
/// FL-20: the refund cannot be planned from the float's coins (the text is
/// the detail).
pub const WORDS_NEEDED_FL_20_PLAN: &str = "WORDS_NEEDED_FL_20_PLAN";
/// FL-20: the float's key did not sign or the finish failed (the text is the
/// detail).
pub const WORDS_NEEDED_FL_20_SIGN: &str = "WORDS_NEEDED_FL_20_SIGN";
/// FL-21: a coin is not one of the float's wallet.
pub const WORDS_NEEDED_FL_21_COIN: &str = "WORDS_NEEDED_FL_21_COIN";
/// FL-21: the finished transaction pays something other than the refund and
/// the float's change, or spends a coin that is not the float's.
pub const WORDS_NEEDED_FL_21_CHECK: &str = "WORDS_NEEDED_FL_21_CHECK";
/// FL-22: there is no fee rate to use.
pub const WORDS_NEEDED_FL_22_NO_RATE: &str = "WORDS_NEEDED_FL_22_NO_RATE";
/// FL-23: the float cannot cover the refund and its fee; it stays queued.
pub const WORDS_NEEDED_FL_23_SHORT: &str = "WORDS_NEEDED_FL_23_SHORT";
/// FL-30: the float's description or its address cannot be read or made.
pub const WORDS_NEEDED_FL_30_WALLET: &str = "WORDS_NEEDED_FL_30_WALLET";
/// FL-31: said before a refill goes: it links the savings and the float on
/// the chain.
pub const WORDS_NEEDED_FL_31_LINKED: &str = "WORDS_NEEDED_FL_31_LINKED";
/// FL-35: the float holds nothing to move.
pub const WORDS_NEEDED_FL_35_NOTHING: &str = "WORDS_NEEDED_FL_35_NOTHING";
/// FL-35: the shop's address for the sweep cannot be derived.
pub const WORDS_NEEDED_FL_35_ADDRESS: &str = "WORDS_NEEDED_FL_35_ADDRESS";

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

    /// The placeholder that is the refusal's words.
    pub fn placeholder(&self) -> &'static str {
        match self {
            Self::Words => WORDS_NEEDED_FL_01_WORDS,
            Self::NoCap => WORDS_NEEDED_FL_10_NO_CAP,
            Self::NothingToRefill => WORDS_NEEDED_FL_11_NOTHING,
            Self::AboveCap { .. } => WORDS_NEEDED_FL_11_ABOVE_CAP,
            Self::Amount => WORDS_NEEDED_FL_11_AMOUNT,
            Self::Plan(_) => WORDS_NEEDED_FL_20_PLAN,
            Self::Sign(_) => WORDS_NEEDED_FL_20_SIGN,
            Self::NotFloatCoin(_) => WORDS_NEEDED_FL_21_COIN,
            Self::NotOnlyFloat(_) => WORDS_NEEDED_FL_21_CHECK,
            Self::NoFeeRate => WORDS_NEEDED_FL_22_NO_RATE,
            Self::NotEnough { .. } => WORDS_NEEDED_FL_23_SHORT,
            Self::Wallet => WORDS_NEEDED_FL_30_WALLET,
            Self::NothingToSweep => WORDS_NEEDED_FL_35_NOTHING,
            Self::ShopAddress => WORDS_NEEDED_FL_35_ADDRESS,
        }
    }

    /// What the owner is told: the placeholder, with the library's own text
    /// as the detail where it has one.
    pub fn words(&self) -> Words {
        let words = needed(self.placeholder()).code(self.rule());
        match self {
            Self::Plan(why)
            | Self::Sign(why)
            | Self::NotFloatCoin(why)
            | Self::NotOnlyFloat(why) => words.detail(why.clone()),
            _ => words,
        }
    }
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

/// FL-12: the words of the home screen's line when the float is low.
pub fn low_words() -> Words {
    Words::new(
        Tone::Info,
        WORDS_NEEDED_FL_12_LOW,
        WORDS_NEEDED_FL_12_LOW,
        Who::Owner,
        WORDS_NEEDED_FL_12_LOW,
    )
    .code("FL-12")
}

/// FL-31: the words said before a refill goes.
pub fn refill_linked_words() -> Words {
    Words::new(
        Tone::Info,
        WORDS_NEEDED_FL_31_LINKED,
        WORDS_NEEDED_FL_31_LINKED,
        Who::Owner,
        WORDS_NEEDED_FL_31_LINKED,
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
    let secp = Secp256k1::new();
    let unsigned = psbt.unsigned_tx.clone();
    let mut cache = SighashCache::new(&unsigned);
    for (n, (entry, input)) in psbt.inputs.iter_mut().zip(planned.inputs()).enumerate() {
        let coin = &input.coin;
        let output = coin
            .output()
            .ok_or_else(|| FloatRefusal::Sign("a coin has no output".to_string()))?;
        let branch = match coin.chain {
            Keychain::Receive => 0,
            Keychain::Change => 1,
        };
        let mut child = opened
            .account
            .derive_priv(
                &secp,
                &[
                    ChildNumber::Normal { index: branch },
                    ChildNumber::Normal { index: coin.index },
                ],
            )
            .map_err(|_| FloatRefusal::Sign("the key cannot be derived".to_string()))?;
        let hash = cache
            .p2wpkh_signature_hash(
                n,
                &output.script_pubkey,
                output.value,
                EcdsaSighashType::All,
            )
            .map_err(|e| FloatRefusal::Sign(e.to_string()))?;
        let message = Message::from_digest(hash.to_byte_array());
        let signature = secp.sign_ecdsa(&message, &child.private_key);
        let key = PublicKey::new(child.private_key.public_key(&secp));
        child.private_key.non_secure_erase();
        entry.partial_sigs.insert(
            key,
            ecdsa::Signature {
                signature,
                sighash_type: EcdsaSighashType::All,
            },
        );
    }
    Ok(())
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
    if tx.input.is_empty() {
        return Err(bad("the transaction spends nothing"));
    }
    let float_wallet = ImportedWallet::Single(wallet.clone());
    for input in &tx.input {
        let coin = coins
            .iter()
            .find(|c| c.outpoint == input.previous_output)
            .ok_or_else(|| bad("an input is not a coin of the float"))?;
        coin.check(&float_wallet)
            .map_err(|e| FloatRefusal::NotFloatCoin(e.to_string()))?;
    }
    if sweep && coins.len() != tx.input.len() {
        return Err(bad("a sweep must spend every coin"));
    }
    if pay_to == change {
        return Err(bad("the payment is to the float's own change"));
    }
    let mut paid = 0usize;
    let mut changes = 0usize;
    for output in &tx.output {
        if output.script_pubkey == *pay_to {
            paid += 1;
            if let Some(sats) = pay_sats {
                if output.value.to_sat() != sats {
                    return Err(bad("the payment is not the amount of the refund"));
                }
            }
        } else if output.script_pubkey == *change && !sweep {
            changes += 1;
        } else {
            return Err(bad("an output pays something else"));
        }
    }
    if paid != 1 {
        return Err(bad("the destination must be paid exactly once"));
    }
    if changes > 1 {
        return Err(bad("more than one change output"));
    }
    Ok(())
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

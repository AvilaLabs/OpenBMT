//! The ops of the refund float (ops 165 to 170, `docs/FLOAT-RULES.md`,
//! FL-01 to FL-35): open the float from its 12 words, say what the cap
//! asks, give the fee rate of a refund, pay a refund from the float, turn
//! the float off, and queue a refill.
//!
//! The library keeps nothing of the float (FL-03). The ops that sign are
//! given the words in the request and give back the finished transaction and
//! nothing secret: they work on no node, and the app keeps the words sealed
//! and hands them over for each call. The refill is the one op of a node:
//! it queues a cash-out to the float's address in the shop's own queue, and
//! it takes the float's public description, never its words.

use std::fmt;
use std::str::FromStr;

use bitcoin::consensus::encode::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::{Address, Network, OutPoint, ScriptBuf, Transaction, Txid};
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::ImportedWallet;
use openbmt_owner::float::{
    self, Coin, FloatPaid, FloatRefusal, FloatSealError, FloatWallet, FloatWords, RefillError,
    RefillQueued,
};
use openbmt_owner::starter::KernelRandom;

use crate::codec::{wire_record, wire_record_get};
use crate::error::{from_words, refused_by, AppError, Kind};

/// A refusal of this layer, with the library's words for it.
fn refused_with(rule: &str, text: String, words: openbmt_core::say::Words) -> AppError {
    from_words(Kind::Refused, rule, text, words)
}
use crate::views::OwnerNetwork;

/// A refusal of the float as the error that crosses the boundary.
pub(crate) fn refusal(e: &FloatRefusal) -> AppError {
    from_words(Kind::Refused, e.rule(), e, e.words())
}

/// One coin of the float or of the starter wallet, as the app's coin view
/// gives it.
#[derive(Clone, PartialEq, Eq)]
pub struct FloatCoinView {
    /// The transaction ID, 32 bytes in the order of the ID's hash.
    pub txid: Vec<u8>,
    /// The output index.
    pub vout: u32,
    /// The whole previous transaction, consensus bytes.
    pub prev_tx: Vec<u8>,
    /// Whether the coin is on the change chain (else the receive chain).
    pub change: bool,
    /// The index on that chain.
    pub index: u32,
    /// How many blocks deep the coin is; 0 is unconfirmed.
    pub confirmations: u32,
}

impl fmt::Debug for FloatCoinView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FloatCoinView")
            .field("vout", &self.vout)
            .field("change", &self.change)
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

wire_record!(FloatCoinView {
    txid,
    vout,
    prev_tx,
    change,
    index,
    confirmations
});
wire_record_get!(FloatCoinView {
    txid,
    vout,
    prev_tx,
    change,
    index,
    confirmations
});

fn coin_of(v: &FloatCoinView) -> Result<Coin, AppError> {
    coin_with(v, |e| {
        refused_with(
            "float-coin",
            format!("a coin's transaction: {e}"),
            float::coin_unreadable_words(),
        )
    })
}

/// A coin of the app's coin view, read the same way for the float and the
/// starter wallet; `unreadable` makes each one's own refusal when the
/// previous transaction does not read.
pub(crate) fn coin_with(
    v: &FloatCoinView,
    unreadable: impl FnOnce(String) -> AppError,
) -> Result<Coin, AppError> {
    let txid: [u8; 32] = v
        .txid
        .as_slice()
        .try_into()
        .map_err(|_| refused_by("id-length", "a coin's transaction ID is 32 bytes"))?;
    let prev_tx: Transaction = deserialize(&v.prev_tx).map_err(|e| unreadable(e.to_string()))?;
    Ok(Coin {
        outpoint: OutPoint {
            txid: Txid::from_byte_array(txid),
            vout: v.vout,
        },
        prev_tx,
        vout: v.vout,
        chain: if v.change {
            Keychain::Change
        } else {
            Keychain::Receive
        },
        index: v.index,
        confirmations: v.confirmations,
    })
}

fn coins_of(list: &[FloatCoinView]) -> Result<Vec<Coin>, AppError> {
    list.iter().map(coin_of).collect()
}

fn words_of(text: &str) -> Result<FloatWords, AppError> {
    FloatWords::new(text).map_err(|e| refusal(&e))
}

/// The float the words open (op 165, FL-01, FL-02): its public description,
/// which the app keeps, and its first receive address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatWalletView {
    /// The description with its checksum; it holds no secret.
    pub description: String,
    /// The first receive address, mainnet.
    pub first_address: String,
}

wire_record!(FloatWalletView {
    description,
    first_address
});

pub(crate) fn open(words: String) -> Result<FloatWalletView, AppError> {
    let words = words_of(&words)?;
    let wallet = FloatWallet::from_words(&words).map_err(|e| refusal(&e))?;
    let first = wallet.receive_address(0).map_err(|e| refusal(&e))?;
    Ok(FloatWalletView {
        description: wallet.description().to_string(),
        first_address: first.to_string(),
    })
}

/// The float, sealed (op 171, FL-01, FL-03): the sealed seed for the app to
/// store, the data key that opens it for the app to wrap with its keystore
/// key at once and overwrite, and the float's public side.
#[derive(Clone, PartialEq, Eq)]
pub struct FloatSealedView {
    /// The sealed seed (the starter wallet's format, SS-20).
    pub sealed: Vec<u8>,
    /// The data key, 32 bytes.
    pub data_key: Vec<u8>,
    /// The description with its checksum; it holds no secret.
    pub description: String,
    /// The first receive address, mainnet.
    pub first_address: String,
}

impl fmt::Debug for FloatSealedView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FloatSealedView(<hidden>)")
    }
}

wire_record!(FloatSealedView {
    sealed,
    data_key,
    description,
    first_address
});

/// Op 171: the 16 bytes of the Compact SeedQR the owner scanned become the
/// float's seed and are sealed in Rust (FL-01, FL-03). The bytes the app
/// gave are overwritten here.
pub(crate) fn seal(
    mut scan: Vec<u8>,
    rng: &mut dyn KernelRandom,
) -> Result<FloatSealedView, AppError> {
    let made = float::seal_compact_seedqr(&scan, rng);
    for b in scan.iter_mut() {
        *b = 0;
    }
    std::hint::black_box(&scan);
    let made = made.map_err(seal_error)?;
    let first = made.wallet.receive_address(0).map_err(|e| refusal(&e))?;
    Ok(FloatSealedView {
        sealed: made.sealed.sealed.clone(),
        data_key: made.sealed.data_key.as_bytes().to_vec(),
        description: made.wallet.description().to_string(),
        first_address: first.to_string(),
    })
}

/// A refusal of sealing or opening a sealed float, as the error that crosses
/// the boundary.
pub(crate) fn seal_error(e: FloatSealError) -> AppError {
    match e {
        FloatSealError::Float(f) => refusal(&f),
        FloatSealError::Seal(s) => crate::starter::refusal(&s),
    }
}

/// FL-05: the node's watch of the float is not ready, or there is none.
pub(crate) fn not_watching(e: impl std::fmt::Display) -> AppError {
    from_words(
        Kind::State,
        "float-not-watching",
        format!("the node's watch of the float is not ready: {e}"),
        float::not_watching_words(),
    )
}

/// The data key the app handed over, wiped here as the starter's is.
fn key_of(bytes: &mut [u8]) -> Result<openbmt_owner::starter::DataKey, AppError> {
    crate::starter::data_key_of(bytes)
}

/// The float's coins as the node's watch gives them (op 172, FL-05): the
/// checked view that `float_status` and the signing need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatCoinsView {
    /// The coins.
    pub coins: Vec<FloatCoinView>,
    /// What the confirmed coins add up to.
    pub confirmed: u64,
    /// What is on its way into the float, beyond that.
    pub unconfirmed_incoming: u64,
    /// The height of the tip they are counted from.
    pub tip: u32,
    /// One past the highest receive index the watch has seen used.
    pub receive_next: u32,
    /// The next change index.
    pub change_next: u32,
}

wire_record!(FloatCoinsView {
    coins,
    confirmed,
    unconfirmed_incoming,
    tip,
    receive_next,
    change_next
});

pub(crate) fn coins_view(c: &openbmt_owner_node::FloatCoins) -> FloatCoinsView {
    FloatCoinsView {
        coins: c
            .coins
            .iter()
            .map(|k| FloatCoinView {
                txid: k.outpoint.txid.to_byte_array().to_vec(),
                vout: k.vout,
                prev_tx: serialize(&k.prev_tx),
                change: k.chain == Keychain::Change,
                index: k.index,
                confirmations: k.confirmations,
            })
            .collect(),
        confirmed: c.confirmed,
        unconfirmed_incoming: c.unconfirmed_incoming,
        tip: c.tip,
        receive_next: c.receive_next,
        change_next: c.change_next,
    }
}

/// The arguments of `float_pay_node` (FL-20 to FL-22): a queued refund to
/// pay from the node's coins of the float, signed from the sealed seed.
#[derive(Clone, PartialEq, Eq)]
pub struct FloatNodePayArgs {
    /// The sealed seed the app stores.
    pub sealed: Vec<u8>,
    /// The data key, opened from the Keystore for this call only.
    pub data_key: Vec<u8>,
    /// The refund's number in the queue.
    pub refund: u32,
    /// The fee rate in satoshis per 1000 virtual bytes (`float_fee_rate`).
    pub fee_rate: u64,
    /// The fewest confirmations a coin needs.
    pub min_confirmations: u32,
}

impl fmt::Debug for FloatNodePayArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FloatNodePayArgs")
            .field("refund", &self.refund)
            .finish_non_exhaustive()
    }
}

wire_record!(FloatNodePayArgs {
    sealed,
    data_key,
    refund,
    fee_rate,
    min_confirmations
});
wire_record_get!(FloatNodePayArgs {
    sealed,
    data_key,
    refund,
    fee_rate,
    min_confirmations
});

/// The refund, paid from the node's coins of the float, signed from the
/// sealed seed; the address and the amount are the refund's.
pub(crate) fn pay_from_node(
    mut a: FloatNodePayArgs,
    refund: &openbmt_owner::Refund,
    network: Network,
    coins: &[Coin],
    change_index: u32,
    lock_height: u32,
) -> Result<FloatPaidView, AppError> {
    let key = key_of(&mut a.data_key)?;
    let paid = float::pay_refund_sealed(
        &a.sealed,
        &key,
        coins,
        &refund.script,
        refund.sats,
        a.fee_rate,
        change_index,
        a.min_confirmations,
        lock_height,
    )
    .map_err(seal_error)?;
    let destination = Address::from_script(&refund.script, network)
        .map(|x| x.to_string())
        .unwrap_or_default();
    Ok(paid_view(&paid, destination))
}

/// The arguments of `float_sweep_node` (FL-35): everything the node's watch
/// holds to the one address of the shop's wallet that the move was given.
#[derive(Clone, PartialEq, Eq)]
pub struct FloatNodeSweepArgs {
    /// The sealed seed the app stores.
    pub sealed: Vec<u8>,
    /// The data key, opened for this call only.
    pub data_key: Vec<u8>,
    /// The time, to take the address from the book.
    pub now: u64,
    /// The fee rate in satoshis per 1000 virtual bytes.
    pub fee_rate: u64,
    /// The fewest confirmations a coin needs.
    pub min_confirmations: u32,
}

impl fmt::Debug for FloatNodeSweepArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FloatNodeSweepArgs").finish_non_exhaustive()
    }
}

wire_record!(FloatNodeSweepArgs {
    sealed,
    data_key,
    now,
    fee_rate,
    min_confirmations
});
wire_record_get!(FloatNodeSweepArgs {
    sealed,
    data_key,
    now,
    fee_rate,
    min_confirmations
});

/// FL-35: the move of every coin to `to`, signed from the sealed seed.
pub(crate) fn sweep_from_node(
    mut a: FloatNodeSweepArgs,
    to: &ScriptBuf,
    network: Network,
    coins: &[Coin],
    lock_height: u32,
) -> Result<FloatPaidView, AppError> {
    let key = key_of(&mut a.data_key)?;
    let paid = float::sweep_sealed(
        &a.sealed,
        &key,
        coins,
        to,
        a.fee_rate,
        a.min_confirmations,
        lock_height,
    )
    .map_err(seal_error)?;
    let destination = Address::from_script(to, network)
        .map(|x| x.to_string())
        .unwrap_or_default();
    Ok(paid_view(&paid, destination))
}

/// The arguments of `float_send` (FL-20, FL-35): a signed transaction of the
/// float for the node's outbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatSendArgs {
    /// The finished transaction, consensus bytes.
    pub tx: Vec<u8>,
    /// The queued refunds it pays; none for the move.
    pub refunds: Vec<u32>,
    /// Whether it is the move of everything (FL-35).
    pub sweep: bool,
}

wire_record!(FloatSendArgs { tx, refunds, sweep });
wire_record_get!(FloatSendArgs { tx, refunds, sweep });

/// The arguments of `float_status` (FL-10 to FL-12): the cap and what the
/// float holds, in satoshis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatStatusArgs {
    /// The cap the owner set; 0 is off.
    pub cap: u64,
    /// The confirmed balance.
    pub confirmed: u64,
    /// Payments into the float not yet confirmed.
    pub unconfirmed_incoming: u64,
}

wire_record!(FloatStatusArgs {
    cap,
    confirmed,
    unconfirmed_incoming
});
wire_record_get!(FloatStatusArgs {
    cap,
    confirmed,
    unconfirmed_incoming
});

/// What the cap says of the float (op 166).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatStatusView {
    /// What the float holds, counting what is on its way.
    pub held: u64,
    /// FL-12: under a quarter of the cap.
    pub low: bool,
    /// FL-11: what a refill would pay, when there is room.
    pub refill: Option<u64>,
}

wire_record!(FloatStatusView { held, low, refill });

pub(crate) fn status(a: FloatStatusArgs) -> Result<FloatStatusView, AppError> {
    let held = float::held_sats(a.confirmed, a.unconfirmed_incoming).map_err(|e| refusal(&e))?;
    Ok(FloatStatusView {
        held,
        low: float::is_low(a.cap, a.confirmed, a.unconfirmed_incoming),
        refill: float::refill_amount(a.cap, a.confirmed, a.unconfirmed_incoming).ok(),
    })
}

/// The arguments of `float_fee_rate` (FL-22): the recent rate of the blocks
/// the device checked, and the rate the owner typed, each in satoshis per
/// 1000 virtual bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatFeeArgs {
    /// `FeeHistory::recent_rate`, when there is one.
    pub recent: Option<u64>,
    /// What the owner typed, when anything.
    pub typed: Option<u64>,
}

wire_record!(FloatFeeArgs { recent, typed });
wire_record_get!(FloatFeeArgs { recent, typed });

pub(crate) fn fee_rate(a: FloatFeeArgs) -> Result<u64, AppError> {
    float::refund_fee_rate(a.recent, a.typed).map_err(|e| refusal(&e))
}

/// The arguments of `float_pay` (FL-20 to FL-23).
#[derive(Clone, PartialEq, Eq)]
pub struct FloatPayArgs {
    /// The float's 12 words, from the app's keystore, for this call only.
    pub words: String,
    /// The network of the refund's address.
    pub network: OwnerNetwork,
    /// The refund's address.
    pub address: String,
    /// The refund in satoshis.
    pub sats: u64,
    /// The fee rate in satoshis per 1000 virtual bytes (`float_fee_rate`).
    pub fee_rate: u64,
    /// The index on the float's change chain for the change.
    pub change_index: u32,
    /// The fewest confirmations a coin needs.
    pub min_confirmations: u32,
    /// The lock time, a block height.
    pub lock_height: u32,
    /// The float's coins.
    pub coins: Vec<FloatCoinView>,
}

impl fmt::Debug for FloatPayArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FloatPayArgs")
            .field("sats", &self.sats)
            .finish_non_exhaustive()
    }
}

wire_record!(FloatPayArgs {
    words,
    network,
    address,
    sats,
    fee_rate,
    change_index,
    min_confirmations,
    lock_height,
    coins
});
wire_record_get!(FloatPayArgs {
    words,
    network,
    address,
    sats,
    fee_rate,
    change_index,
    min_confirmations,
    lock_height,
    coins
});

/// A finished transaction of the float (ops 168 and 169), ready for the
/// outbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatPaidView {
    /// The transaction, consensus bytes.
    pub tx: Vec<u8>,
    /// Its ID, hex.
    pub txid: String,
    /// The fee in satoshis.
    pub fee: u64,
    /// The virtual size.
    pub vsize: u64,
    /// What the destination is paid.
    pub paid: u64,
    /// The index of the float's change on the change chain, if there is
    /// change.
    pub change_index: Option<u32>,
    /// The change in satoshis, if there is any.
    pub change_sats: Option<u64>,
    /// The destination, as an address.
    pub destination: String,
}

wire_record!(FloatPaidView {
    tx,
    txid,
    fee,
    vsize,
    paid,
    change_index,
    change_sats,
    destination
});

fn paid_view(p: &FloatPaid, destination: String) -> FloatPaidView {
    FloatPaidView {
        tx: serialize(&p.tx),
        txid: p.txid.to_string(),
        fee: p.fee,
        vsize: p.vsize,
        paid: p.paid,
        change_index: p.change.map(|(i, _)| i),
        change_sats: p.change.map(|(_, s)| s),
        destination,
    }
}

pub(crate) fn pay(a: FloatPayArgs) -> Result<FloatPaidView, AppError> {
    let network: Network = a.network.network();
    let to = Address::from_str(&a.address)
        .map_err(|e| refused_by("address-invalid", format!("{:?}: {e}", a.address)))?
        .require_network(network)
        .map_err(|e| refused_by("address-invalid", format!("{:?}: {e}", a.address)))?;
    let words = words_of(&a.words)?;
    let coins = coins_of(&a.coins)?;
    let paid = float::pay_refund(
        &words,
        &coins,
        &to.script_pubkey(),
        a.sats,
        a.fee_rate,
        a.change_index,
        a.min_confirmations,
        a.lock_height,
    )
    .map_err(|e| refusal(&e))?;
    Ok(paid_view(&paid, to.to_string()))
}

/// The arguments of `float_sweep` (FL-35).
#[derive(Clone, PartialEq, Eq)]
pub struct FloatSweepArgs {
    /// The float's 12 words, for this call only.
    pub words: String,
    /// The shop's pinned wallet description.
    pub shop: String,
    /// The index on the shop's receive chain of the address to pay: the
    /// next unused one.
    pub shop_index: u32,
    /// The fee rate in satoshis per 1000 virtual bytes.
    pub fee_rate: u64,
    /// The fewest confirmations a coin needs.
    pub min_confirmations: u32,
    /// The lock time, a block height.
    pub lock_height: u32,
    /// The float's coins.
    pub coins: Vec<FloatCoinView>,
}

impl fmt::Debug for FloatSweepArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FloatSweepArgs")
            .field("shop_index", &self.shop_index)
            .finish_non_exhaustive()
    }
}

wire_record!(FloatSweepArgs {
    words,
    shop,
    shop_index,
    fee_rate,
    min_confirmations,
    lock_height,
    coins
});
wire_record_get!(FloatSweepArgs {
    words,
    shop,
    shop_index,
    fee_rate,
    min_confirmations,
    lock_height,
    coins
});

pub(crate) fn sweep(a: FloatSweepArgs) -> Result<FloatPaidView, AppError> {
    let shop = ImportedWallet::read(&a.shop).map_err(|e| {
        refused_with(
            "float-shop-wallet",
            format!("the shop's wallet: {e:?}"),
            float::shop_wallet_unreadable_words(),
        )
    })?;
    let to: ScriptBuf = float::shop_receive_script(&shop, a.shop_index).map_err(|e| refusal(&e))?;
    let destination = Address::from_script(&to, shop.network())
        .map(|x| x.to_string())
        .unwrap_or_default();
    let words = words_of(&a.words)?;
    let coins = coins_of(&a.coins)?;
    let paid = float::sweep(
        &words,
        &coins,
        &to,
        a.fee_rate,
        a.min_confirmations,
        a.lock_height,
    )
    .map_err(|e| refusal(&e))?;
    Ok(paid_view(&paid, destination))
}

/// The arguments of `float_refill` (FL-11, FL-30).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatRefillArgs {
    /// The float's public description (from `float_open`).
    pub description: String,
    /// The index of the float's next unused receive address.
    pub next_index: u32,
    /// The cap the owner set.
    pub cap: u64,
    /// The float's confirmed balance.
    pub confirmed: u64,
    /// Payments into the float not yet confirmed.
    pub unconfirmed_incoming: u64,
    /// A smaller amount than the room, if the owner chose one.
    pub amount: Option<u64>,
}

wire_record!(FloatRefillArgs {
    description,
    next_index,
    cap,
    confirmed,
    unconfirmed_incoming,
    amount
});
wire_record_get!(FloatRefillArgs {
    description,
    next_index,
    cap,
    confirmed,
    unconfirmed_incoming,
    amount
});

/// What a refill queued (op 170).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatRefillView {
    /// The cash-out's number in the queue.
    pub id: u32,
    /// The amount in satoshis.
    pub sats: u64,
    /// The float's address it pays.
    pub address: String,
}

wire_record!(FloatRefillView { id, sats, address });

pub(crate) fn wallet_of(description: &str) -> Result<FloatWallet, AppError> {
    FloatWallet::parse(description).map_err(|e| refusal(&e))
}

pub(crate) fn refill_view(q: &RefillQueued, network: Network) -> FloatRefillView {
    FloatRefillView {
        id: q.id,
        sats: q.sats,
        address: Address::from_script(&q.address.script_pubkey(), network)
            .map(|a| a.to_string())
            .unwrap_or_else(|_| q.address.to_string()),
    }
}

/// A refusal of a refill: the float's, or the queue's.
pub(crate) fn refill_error(
    e: RefillError<String>,
    queue: impl Fn(&openbmt_owner::OwnerError<String>) -> AppError,
) -> AppError {
    match e {
        RefillError::Float(f) => refusal(&f),
        RefillError::Owner(o) => queue(&o),
    }
}

//! The things the owner phone keeps (AS-50 to AS-65).

use bitcoin::{OutPoint, ScriptBuf, Txid};
use openbmt_core::batch::AddressBatch;
use openbmt_core::boxpair::Claim;
use openbmt_core::identity::{DeviceId, Sig};
use openbmt_core::pairing::PairRequest;
use openbmt_core::settings::SignedSettings;
use openbmt_session::ShopValue;

/// A sale as the counter and its receive index name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SaleKey {
    /// The counter that made the sale.
    pub counter: DeviceId,
    /// The sale's receive index on that counter.
    pub index: u32,
}

/// What a sale received and whether it reached the refund depth (AS-52,
/// AS-53). The owner phone takes these as given, so the caller must fold
/// them from the owner's own checked view of the chain and the ranges the
/// owner issued (OC-14), never from a counter's records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaleFacts {
    /// What was received for the sale, in satoshis.
    pub received_sats: u64,
    /// Whether the payment has reached the refund depth (AS-53).
    pub reached_refund_depth: bool,
}

/// A range of receive indices the owner has signed an address batch for
/// (OC-10). It is kept for good: delivering or archiving the batch does not
/// remove it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IssuedRange {
    /// The first receive index.
    pub start: u32,
    /// How many indices.
    pub count: u32,
    /// The counter the batch was signed for.
    pub counter: DeviceId,
    /// The owner's clock, in seconds, when the batch was signed (OC-22).
    pub signed_at: u64,
}

impl IssuedRange {
    /// The first index past the range, as a wider number so that it cannot
    /// overflow.
    pub fn end(&self) -> u64 {
        u64::from(self.start) + u64::from(self.count)
    }
}

/// Who pays the fee of a refund transaction (AS-56).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeePayer {
    /// The fee comes on top, out of the shop's coins.
    Shop,
    /// Every refund output is its amount less an equal share of the fee.
    Customer,
}

/// How refunds are put into transactions (AS-55).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefundPacking {
    /// One transaction for all ready refunds, at most 99 outputs.
    OneTransaction,
    /// One transaction for each refund, so that the Key app can approve one
    /// and decline another.
    OneEach,
}

/// The owner's choices, stored with the queue (AS-55, AS-56).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Who pays the fee.
    pub fee_payer: FeePayer,
    /// How refunds are put into transactions.
    pub packing: RefundPacking,
    /// The fewest confirmations a coin needs to be spent.
    pub min_confirmations: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            fee_payer: FeePayer::Shop,
            packing: RefundPacking::OneTransaction,
            min_confirmations: 1,
        }
    }
}

/// Where a queue entry is (AS-54, AS-63).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// Waiting for the next session.
    Queued,
    /// Frozen into an open session.
    InSession {
        /// The session's ID.
        session: [u8; 32],
    },
    /// Declined or refused, with the reason. It is not put in a session by
    /// itself.
    Returned {
        /// The reason, as text.
        reason: String,
    },
    /// Signed by both keys; the transaction waits to be sent.
    Signed {
        /// The transaction's ID.
        txid: Txid,
    },
    /// Reported as broadcast.
    Sent {
        /// The transaction's ID.
        txid: Txid,
    },
    /// Reported as confirmed.
    Paid {
        /// The transaction's ID.
        txid: Txid,
    },
}

/// OC-51: a rate in the shop's currency, as the books need it: minor units of
/// the currency per bitcoin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateNote {
    /// Three upper-case ASCII letters (`SAT` for Bitcoin Standard).
    pub currency: [u8; 3],
    /// Minor units of the currency per bitcoin. Never zero.
    pub minor_per_btc: u64,
}

/// OC-51: what the books need of a refund besides its amount, kept with it
/// from the counter's request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefundBooks {
    /// The request's number on the counter.
    pub request: u64,
    /// The sale's last quote, when the counter's records have one.
    pub last_quote: Option<RateNote>,
}

/// A refund in the queue (AS-51 to AS-56).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refund {
    /// The entry's number.
    pub id: u32,
    /// The sale it refunds.
    pub sale: SaleKey,
    /// The cashier's name, as the label will say.
    pub cashier: String,
    /// Where the money goes.
    pub script: ScriptBuf,
    /// The amount in satoshis.
    pub sats: u64,
    /// The refund's value in the shop's currency, if one is stated.
    pub shop_value: Option<ShopValue>,
    /// OC-51: the request it came from and the sale's last quote, when it
    /// came from a counter's request.
    pub books: Option<RefundBooks>,
    /// Where it is.
    pub stage: Stage,
    /// AS-68: the outpoints that the transactions this refund was in spent,
    /// from every one that was reported dropped. A new transaction spends
    /// them, so that the two cannot both confirm.
    pub conflict: Vec<OutPoint>,
}

/// What a cash-out moves (AS-57).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CashAmount {
    /// An amount in satoshis.
    Sats(u64),
    /// Every coin left after the refunds.
    All,
}

/// A cash-out in the queue (AS-57).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashOut {
    /// The entry's number.
    pub id: u32,
    /// Where the money goes.
    pub script: ScriptBuf,
    /// How much.
    pub amount: CashAmount,
    /// The destination's label, as the owner phone says it.
    pub label: String,
    /// OC-51: the price the owner confirmed for it, for the books.
    pub price: Option<RateNote>,
    /// Where it is.
    pub stage: Stage,
    /// AS-68: the outpoints that the transactions this one paid spent, from
    /// every one that was reported dropped. A new transaction spends them.
    pub conflict: Vec<OutPoint>,
}

/// A batch in the queue, signed by the owner (AS-58).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedBatch {
    /// The entry's number.
    pub id: u32,
    /// The batch.
    pub batch: AddressBatch,
    /// The owner's signature over it.
    pub owner_sig: Sig,
    /// Where it is.
    pub stage: Stage,
}

/// Settings in the queue, signed by the owner (AS-59).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedSettings {
    /// The entry's number.
    pub id: u32,
    /// The settings with the owner's signature and no Key app signature.
    pub signed: SignedSettings,
    /// Where it is.
    pub stage: Stage,
}

/// A pairing in the queue (AS-58).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedPairing {
    /// The entry's number.
    pub id: u32,
    /// The counter's request.
    pub request: PairRequest,
    /// Where it is.
    pub stage: Stage,
}

/// A Box's claim in the queue (AS-83).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedBoxClaim {
    /// The entry's number.
    pub id: u32,
    /// The claim the Box made. The owner phone cannot check its MAC.
    pub claim: Claim,
    /// Where it is.
    pub stage: Stage,
}

/// What `queue_settings` did (AS-59).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsQueued {
    /// A loosening change, queued for a session under this number.
    Queued(u32),
    /// A tightening change, signed by the owner alone and handed back. It
    /// also waits among the finished objects under this number.
    Tightening {
        /// The number among the finished objects.
        id: u32,
        /// The settings with the owner's signature.
        signed: Box<SignedSettings>,
    },
}

/// A finished object that waits to be delivered (AS-64).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deliverable {
    /// The object's number.
    pub id: u32,
    /// What it is.
    pub what: Delivery,
}

/// The kinds of finished object (AS-64).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// A signed transaction, as consensus bytes, with the refunds and the
    /// cash-out it pays. It waits until it is reported confirmed or
    /// dropped.
    Transaction {
        /// The transaction's ID.
        txid: Txid,
        /// The transaction, consensus-encoded.
        tx: Vec<u8>,
        /// The refunds it pays, in output order.
        refunds: Vec<u32>,
        /// The cash-out it pays, if it is one.
        cash_out: Option<u32>,
    },
    /// A batch with both signatures (`OBMB`).
    Batch(Vec<u8>),
    /// Settings with the owner's signature, and the Key app's when they
    /// loosen (`OBMS`).
    Settings(Vec<u8>),
    /// A pairing with both signatures (`OBMD`).
    Pairing(Vec<u8>),
    /// A Box's claim (`OBM5`) and the welcome the Key app made for it
    /// (`OBMM`), one after the other, 202 bytes (AS-85).
    BoxWelcome(Vec<u8>),
}

/// AS-89: a transaction that was given to a hardware signer and whose open
/// approval was then abandoned. Key 1's signature is on it already, so it
/// may still be finished and sent, and it is watched until it confirms or
/// can no longer confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedElsewhere {
    /// The transaction's ID. Signatures added later do not change it.
    pub txid: Txid,
    /// The refunds it pays.
    pub refunds: Vec<u32>,
    /// The cash-out it pays, if it is one.
    pub cash_out: Option<u32>,
    /// What it spends, ascending.
    pub inputs: Vec<OutPoint>,
}

/// What happened to one item of a session when its response was taken
/// (AS-62, AS-63).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemOutcome {
    /// The item came back signed and checked; the object waits under this
    /// number.
    Delivered(u32),
    /// The item went back to the queue as returned, with the reason.
    Returned(String),
    /// The health check was answered by the pinned Key app (AS-14).
    Healthy,
}

/// The result of taking a response (AS-62).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taken {
    /// The session's ID.
    pub session_id: [u8; 32],
    /// One outcome for each item, in item order, with the item's number.
    pub outcomes: Vec<(u32, ItemOutcome)>,
}

/// Why a queue entry was not put in the session (AS-61).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The entry's number.
    pub id: u32,
    /// Why, in words.
    pub reason: String,
}

/// A session that was built (AS-61).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    /// The session's ID.
    pub session_id: [u8; 32],
    /// The request, as bytes to carry to the Key app.
    pub request: Vec<u8>,
    /// Entries that stayed queued, with the reason.
    pub skipped: Vec<Skipped>,
}

//! Room bought ahead from a provider that offers LSPS1 (SL-28, SL-35, SL-29).
//!
//! The decisions are pure functions here, as in `provider.rs`, so that they
//! are tested without a provider: when to order, what to ask for, whether to
//! pay what the provider asks, and what the order's state is after the
//! provider's answer. The open order is kept in the two-copy store
//! ([`Orders`]) so that a restart does not order twice and the fees paid are
//! not lost.
//!
//! The node pays only the Lightning (bolt11) payment option of an order, from
//! its own channels, to the chosen provider, and never more than the owner
//! allows for one opening (SL-29).

use crate::store::DualStore;
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::PublicKey;
use lightning::io;
use lightning::util::persist::KVStore;
use lightning_liquidity::lsps1::msgs::{
    LSPS1ChannelInfo, LSPS1Options, LSPS1OrderParams, LSPS1PaymentInfo, LSPS1PaymentState,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Where the book is kept.
pub const PRIMARY: &str = "lsps1_orders";
/// The key of the one blob.
pub const KEY: &str = "book";
const VERSION: u8 = 1;

/// The provider's answer to its options is waited for this long; no answer
/// means it has no LSPS1.
pub const OPTIONS_ANSWER: Duration = Duration::from_secs(30);
/// How long the node waits for the provider to answer one order request.
pub const ORDER_ANSWER: Duration = Duration::from_secs(30);
/// How often an open order is asked about.
pub const POLL_EVERY_SECS: u64 = 10;
/// After a refusal to pay, a failed order or a finished one, no new order is
/// made sooner than this: an hour.
pub const RETRY_AFTER_SECS: u64 = 3600;
/// An order not paid by this time is given up as expired: an hour.
pub const OPEN_UNPAID_SECS: u64 = 3600;
/// An order paid and not opened by this time is given up as failed: two days.
pub const GIVE_UP_AFTER_SECS: u64 = 2 * 24 * 3600;
/// The channel is asked to last at least this many blocks if the provider
/// allows it: about two weeks, as the window of SL-30.
pub const WANTED_EXPIRY_BLOCKS: u32 = 2016;
/// The longest the node waits, in blocks, for the funding to confirm that the
/// provider may take.
pub const WANTED_FUNDING_WITHIN_BLOCKS: u16 = 6;
/// The longest order id kept.
pub const MAX_ID_LEN: usize = 64;

/// Whether the provider offers LSPS1, as far as the node knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// Not asked yet, or the provider is away.
    Unknown,
    /// It answered with its options.
    Offered,
    /// It gave an error or no answer in time.
    NotOffered,
}

/// Where an order is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderState {
    /// Made by the provider, not paid yet.
    Open,
    /// The provider's invoice is paid; the channel is not opened yet.
    Paid,
    /// The provider has opened the channel.
    Done,
    /// Not paid, not opened, or given up (see [`OrderWhy`]).
    Failed,
}

impl OrderState {
    fn code(self) -> u8 {
        match self {
            Self::Open => 1,
            Self::Paid => 2,
            Self::Done => 3,
            Self::Failed => 4,
        }
    }

    fn from_code(c: u8) -> Option<Self> {
        [Self::Open, Self::Paid, Self::Done, Self::Failed]
            .into_iter()
            .find(|s| s.code() == c)
    }

    /// An order the node is waiting on: no second one is made.
    pub fn is_open(self) -> bool {
        matches!(self, Self::Open | Self::Paid)
    }
}

/// Why an order failed or was not paid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderWhy {
    /// Nothing is wrong.
    None,
    /// The fee is above what the owner allows for one opening (SL-29).
    FeeAboveMax,
    /// The node cannot pay it from its channels.
    CannotPay,
    /// The provider refused the order, or gave an answer that cannot be used.
    Refused,
    /// The provider's invoice ran out before it was paid.
    Expired,
    /// The payment failed.
    PaymentFailed,
    /// The provider refunded the payment.
    Refunded,
    /// The provider's options do not allow an order the node can make, or the
    /// order has no Lightning payment option, or the invoice is not the
    /// provider's.
    NotPossible,
    /// Paid, and no channel came in two days.
    NoChannel,
}

impl OrderWhy {
    fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::FeeAboveMax => 1,
            Self::CannotPay => 2,
            Self::Refused => 3,
            Self::Expired => 4,
            Self::PaymentFailed => 5,
            Self::Refunded => 6,
            Self::NotPossible => 7,
            Self::NoChannel => 8,
        }
    }

    fn from_code(c: u8) -> Option<Self> {
        [
            Self::None,
            Self::FeeAboveMax,
            Self::CannotPay,
            Self::Refused,
            Self::Expired,
            Self::PaymentFailed,
            Self::Refunded,
            Self::NotPossible,
            Self::NoChannel,
        ]
        .into_iter()
        .find(|w| w.code() == c)
    }
}

/// The last order, kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderRecord {
    /// The provider's id for it.
    pub id: String,
    /// Where it is.
    pub state: OrderState,
    /// Why it failed; [`OrderWhy::None`] unless it did.
    pub why: OrderWhy,
    /// What the provider asks for it, in satoshis.
    pub fee_sat: u64,
    /// The payment hash of the provider's invoice; zeros until it is known.
    pub payment_hash: [u8; 32],
    /// When it was made, in seconds since the epoch.
    pub created_unix: u64,
    /// When its state last changed.
    pub updated_unix: u64,
}

/// What is kept: the last order, the fees paid for orders so far, and the
/// time before which no new order is made.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OrderBook {
    /// What orders paid so far, in satoshis (SL-29, SL-45).
    pub fees_paid_sat: u64,
    /// No new order before this, in seconds since the epoch.
    pub next_try_unix: u64,
    /// The last order.
    pub order: Option<OrderRecord>,
}

impl OrderBook {
    /// Whether an order is open: one the node waits on.
    pub fn has_open_order(&self) -> bool {
        self.order.as_ref().is_some_and(|o| o.state.is_open())
    }

    /// A new order is made and kept as open.
    pub fn begin(&mut self, record: OrderRecord) {
        self.order = Some(record);
    }

    /// The provider's invoice was paid: the order is Paid, and its fee is
    /// counted once, whatever the number of times this is told. `false` if
    /// the payment is not the open order's.
    pub fn paid(&mut self, payment_hash: &[u8; 32], now: u64) -> bool {
        let Some(o) = self.order.as_mut() else {
            return false;
        };
        if o.payment_hash != *payment_hash {
            return false;
        }
        if o.state == OrderState::Open {
            o.state = OrderState::Paid;
            o.updated_unix = now;
            self.fees_paid_sat = self.fees_paid_sat.saturating_add(o.fee_sat);
        }
        true
    }

    /// The provider has opened the channel. An order whose payment the node
    /// has not been told of yet is counted as paid now: the provider does not
    /// open a channel before it is.
    pub fn done(&mut self, now: u64) {
        if let Some(o) = self.order.as_mut() {
            if o.state.is_open() {
                if o.state == OrderState::Open {
                    self.fees_paid_sat = self.fees_paid_sat.saturating_add(o.fee_sat);
                }
                o.state = OrderState::Done;
                o.updated_unix = now;
                // The room comes when the channel is ready; the same wait as
                // after a failure keeps a second order from being made while
                // the first channel is not yet usable.
                self.next_try_unix = now.saturating_add(RETRY_AFTER_SECS);
            }
        }
    }

    /// The payment of the open order failed. `false` if the payment is not the
    /// open order's, or the order is not waiting for its payment.
    pub fn payment_failed(&mut self, payment_hash: &[u8; 32], now: u64) -> bool {
        let ours = self
            .order
            .as_ref()
            .is_some_and(|o| o.payment_hash == *payment_hash && o.state == OrderState::Open);
        if ours {
            self.fail(OrderWhy::PaymentFailed, now);
        }
        ours
    }

    /// The order failed, or was not paid; a new one is made no sooner than an
    /// hour from now. A fee counted for a payment the provider gave back is
    /// taken off again, since the fees paid count what was taken (SL-29).
    pub fn fail(&mut self, why: OrderWhy, now: u64) {
        if let Some(o) = self.order.as_mut() {
            if o.state != OrderState::Done {
                if o.state == OrderState::Paid && why == OrderWhy::Refunded {
                    self.fees_paid_sat = self.fees_paid_sat.saturating_sub(o.fee_sat);
                }
                o.state = OrderState::Failed;
                o.why = why;
                o.updated_unix = now;
            }
        }
        self.next_try_unix = now.saturating_add(RETRY_AFTER_SECS);
    }
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

fn encode(book: &OrderBook) -> Vec<u8> {
    let mut out = vec![VERSION];
    out.extend_from_slice(&book.fees_paid_sat.to_be_bytes());
    out.extend_from_slice(&book.next_try_unix.to_be_bytes());
    match &book.order {
        None => out.push(0),
        Some(o) => {
            out.push(1);
            out.push(o.id.len() as u8);
            out.extend_from_slice(o.id.as_bytes());
            out.push(o.state.code());
            out.push(o.why.code());
            out.extend_from_slice(&o.fee_sat.to_be_bytes());
            out.extend_from_slice(&o.payment_hash);
            out.extend_from_slice(&o.created_unix.to_be_bytes());
            out.extend_from_slice(&o.updated_unix.to_be_bytes());
        }
    }
    out
}

/// A strict reader: bytes decode only if they are what [`encode`] writes.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], io::Error> {
        if self.0.len() < n {
            return Err(invalid("orders: short"));
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, io::Error> {
        Ok(self.take(1)?[0])
    }

    fn u64(&mut self) -> Result<u64, io::Error> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
}

fn decode(bytes: &[u8]) -> Result<OrderBook, io::Error> {
    let mut r = Cursor(bytes);
    if r.u8()? != VERSION {
        return Err(invalid("orders: version"));
    }
    let fees_paid_sat = r.u64()?;
    let next_try_unix = r.u64()?;
    let order = match r.u8()? {
        0 => None,
        1 => {
            let len = r.u8()? as usize;
            if len == 0 || len > MAX_ID_LEN {
                return Err(invalid("orders: id length"));
            }
            let id = std::str::from_utf8(r.take(len)?)
                .map_err(|_| invalid("orders: id"))?
                .to_string();
            if !id.chars().all(|c| c.is_ascii_graphic()) {
                return Err(invalid("orders: id characters"));
            }
            let state = OrderState::from_code(r.u8()?).ok_or_else(|| invalid("orders: state"))?;
            let why = OrderWhy::from_code(r.u8()?).ok_or_else(|| invalid("orders: why"))?;
            // A failure says why, and nothing else has a reason.
            if (state == OrderState::Failed) == (why == OrderWhy::None) {
                return Err(invalid("orders: state and reason"));
            }
            let fee_sat = r.u64()?;
            let payment_hash: [u8; 32] = r.take(32)?.try_into().unwrap();
            let created_unix = r.u64()?;
            let updated_unix = r.u64()?;
            Some(OrderRecord {
                id,
                state,
                why,
                fee_sat,
                payment_hash,
                created_unix,
                updated_unix,
            })
        }
        _ => return Err(invalid("orders: flag")),
    };
    if !r.0.is_empty() {
        return Err(invalid("orders: trailing bytes"));
    }
    Ok(OrderBook {
        fees_paid_sat,
        next_try_unix,
        order,
    })
}

/// The book read from `bytes` and written again, for the fuzz target: the
/// result must be the input.
#[cfg(feature = "testing")]
pub fn reencode_for_test(bytes: &[u8]) -> Option<Vec<u8>> {
    decode(bytes).ok().map(|book| encode(&book))
}

/// The book, kept on both devices.
pub struct Orders {
    store: Arc<DualStore>,
    book: Mutex<OrderBook>,
}

impl Orders {
    /// Reads the book from the counter's copy (the newer one by the time the
    /// node reads it, SL-25). A blob that does not read stops the start.
    pub async fn load(store: Arc<DualStore>) -> Result<Self, io::Error> {
        let book = match KVStore::read(&*store, PRIMARY, "", KEY).await {
            Ok(bytes) => decode(&bytes)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => OrderBook::default(),
            Err(e) => return Err(e),
        };
        Ok(Orders {
            store,
            book: Mutex::new(book),
        })
    }

    /// What is kept now.
    pub fn snapshot(&self) -> OrderBook {
        self.book.lock().unwrap().clone()
    }

    /// Changes the book and returns when both copies hold the change. The
    /// numbering of the store's writes is taken while the book is locked, so
    /// the blobs land in the order the book changed. A change that leaves the
    /// book as it was is not written.
    pub async fn change(&self, f: impl FnOnce(&mut OrderBook)) -> Result<(), io::Error> {
        let write = {
            let mut book = self.book.lock().unwrap();
            let before = book.clone();
            f(&mut book);
            if *book == before {
                return Ok(());
            }
            KVStore::write(&*self.store, PRIMARY, "", KEY, encode(&book))
        };
        write.await
    }
}

/// Whether to make an order now (SL-28): the room to receive is under half
/// the cap, no order is open, and the time before the next try has come.
pub fn order_wanted(room_msat: u64, cap_sat: u64, book: &OrderBook, now: u64) -> bool {
    cap_sat > 0
        && !book.has_open_order()
        && now >= book.next_try_unix
        && (room_msat as u128) * 2 < (cap_sat as u128) * 1000
}

/// Why no order can be made from the provider's options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoOrder {
    /// The provider needs the client to put in coins; the shop puts in none.
    ClientBalanceRequired,
    /// No size is within the provider's limits and the cap.
    OutsideLimits,
    /// The provider allows no lease of the channel.
    NoExpiry,
}

/// The order that brings the room up to the cap (SL-28): the provider puts in
/// what the room lacks, at least its smallest and at most its largest, the
/// shop puts in nothing, the channel is used after one confirmation, kept
/// private, and leased for the most the provider allows. `room_msat` is the
/// room to receive now.
pub fn order_params(
    options: &LSPS1Options,
    room_msat: u64,
    cap_sat: u64,
    token: Option<&str>,
) -> Result<LSPS1OrderParams, NoOrder> {
    if options.min_initial_client_balance_sat > 0 {
        return Err(NoOrder::ClientBalanceRequired);
    }
    if options.max_channel_expiry_blocks == 0 {
        return Err(NoOrder::NoExpiry);
    }
    let lacking = cap_sat.saturating_sub(room_msat / 1000);
    let lo = options
        .min_initial_lsp_balance_sat
        .max(options.min_channel_balance_sat);
    let hi = options
        .max_initial_lsp_balance_sat
        .min(options.max_channel_balance_sat);
    if lo > hi || hi == 0 {
        return Err(NoOrder::OutsideLimits);
    }
    Ok(LSPS1OrderParams {
        lsp_balance_sat: lacking.clamp(lo, hi),
        client_balance_sat: 0,
        required_channel_confirmations: options.min_required_channel_confirmations.max(1),
        funding_confirms_within_blocks: options
            .min_funding_confirms_within_blocks
            .max(WANTED_FUNDING_WITHIN_BLOCKS),
        channel_expiry_blocks: options.max_channel_expiry_blocks.min(u32::MAX).max(1),
        token: token.map(str::to_string),
        announce_channel: false,
    })
}

/// Why the node does not pay an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayRefusal {
    /// The order has no Lightning (bolt11) payment option.
    NoBolt11,
    /// The invoice is not the chosen provider's, so it is never paid.
    WrongPayee,
    /// The invoice has run out.
    Expired,
    /// The invoice has no amount.
    NoAmount,
    /// The fee is above what the owner allows for one opening (SL-29).
    FeeAboveMax {
        /// What the provider asks, in satoshis.
        fee_sat: u64,
    },
    /// No channel to the provider can send that much now.
    CannotPay,
}

impl PayRefusal {
    /// The reason an order not paid is recorded with.
    pub fn why(self) -> OrderWhy {
        match self {
            Self::NoBolt11 | Self::WrongPayee | Self::NoAmount => OrderWhy::NotPossible,
            Self::Expired => OrderWhy::Expired,
            Self::FeeAboveMax { .. } => OrderWhy::FeeAboveMax,
            Self::CannotPay => OrderWhy::CannotPay,
        }
    }
}

/// A payment the node will make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayPlan {
    /// What is paid, in millisatoshis: the invoice's amount.
    pub amount_msat: u64,
    /// What the order costs, in satoshis: the most of what the provider calls
    /// its fee, its total and the invoice's amount.
    pub fee_sat: u64,
    /// The invoice's payment hash.
    pub payment_hash: [u8; 32],
    /// The invoice's payment secret.
    pub payment_secret: [u8; 32],
    /// The final CLTV delta the invoice asks.
    pub min_final_cltv: u32,
}

/// What the order costs, in satoshis, as the provider's Lightning option
/// says it: the most of its fee, its total and the invoice's amount (the shop
/// puts in no coins, so the whole order is the fee); 0 with no such option.
pub fn order_fee_sat(payment: &LSPS1PaymentInfo) -> u64 {
    let Some(b) = payment.bolt11.as_ref() else {
        return 0;
    };
    let invoice_msat = b.invoice.amount_milli_satoshis().unwrap_or(0);
    invoice_msat
        .max(b.fee_total_sat.saturating_mul(1000))
        .max(b.order_total_sat.saturating_mul(1000))
        .div_ceil(1000)
}

/// The payment hash of the order's Lightning invoice; zeros with none.
pub fn payment_hash_of(payment: &LSPS1PaymentInfo) -> [u8; 32] {
    payment
        .bolt11
        .as_ref()
        .map_or([0; 32], |b| b.invoice.payment_hash().to_byte_array())
}

/// Whether to pay the order's Lightning payment option, and what (SL-28,
/// SL-29). `can_pay_msat` is the most one channel to the provider can send
/// now; `now_unix` is the time.
pub fn judge_payment(
    payment: &LSPS1PaymentInfo,
    provider: &PublicKey,
    max_fee_msat: u64,
    can_pay_msat: u64,
    now_unix: u64,
) -> Result<PayPlan, PayRefusal> {
    let bolt11 = payment.bolt11.as_ref().ok_or(PayRefusal::NoBolt11)?;
    let invoice = &bolt11.invoice;
    if invoice.recover_payee_pub_key() != *provider {
        return Err(PayRefusal::WrongPayee);
    }
    if bolt11.expires_at.0.timestamp() < now_unix as i64
        || invoice.would_expire(Duration::from_secs(now_unix))
    {
        return Err(PayRefusal::Expired);
    }
    let amount_msat = invoice
        .amount_milli_satoshis()
        .filter(|a| *a > 0)
        .ok_or(PayRefusal::NoAmount)?;
    // The shop puts in no coins, so the whole order is the fee. Whichever of
    // the numbers the provider gives is the largest counts.
    let fee_sat = order_fee_sat(payment);
    let fee_msat = fee_sat.saturating_mul(1000);
    if fee_msat > max_fee_msat {
        return Err(PayRefusal::FeeAboveMax { fee_sat });
    }
    if amount_msat > can_pay_msat {
        return Err(PayRefusal::CannotPay);
    }
    Ok(PayPlan {
        amount_msat,
        fee_sat,
        payment_hash: invoice.payment_hash().to_byte_array(),
        payment_secret: invoice.payment_secret().0,
        min_final_cltv: invoice.min_final_cltv_expiry_delta().min(u32::MAX as u64) as u32,
    })
}

/// What the provider says of an order, in the node's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// The provider waits for the payment.
    Expecting,
    /// The provider has the payment and has not opened the channel.
    PaidNotOpened,
    /// The channel is opened.
    Opened,
    /// The provider gave the payment back.
    Refunded,
    /// The order has no Lightning payment option.
    NoBolt11,
}

/// Reads the provider's answer about an order.
pub fn read_status(payment: &LSPS1PaymentInfo, channel: &Option<LSPS1ChannelInfo>) -> Seen {
    if channel.is_some() {
        return Seen::Opened;
    }
    match payment.bolt11.as_ref().map(|b| &b.state) {
        None => Seen::NoBolt11,
        Some(LSPS1PaymentState::ExpectPayment) => Seen::Expecting,
        Some(LSPS1PaymentState::Paid) => Seen::PaidNotOpened,
        Some(LSPS1PaymentState::Refunded) => Seen::Refunded,
    }
}

/// What to do about an open order after the provider's answer: the change of
/// the book, or to try to pay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Nothing yet; ask again.
    Wait,
    /// Pay the invoice (the library refuses a second payment of the same
    /// invoice, so a repeat is harmless).
    Pay,
    /// The provider has the payment, and the node had not seen it sent.
    MarkPaid,
    /// The channel is opened.
    Done,
    /// The order is over.
    Fail(OrderWhy),
}

/// The step for an open order, from what the provider says (SL-28).
pub fn next_step(record: &OrderRecord, seen: Seen, now: u64) -> Next {
    let unpaid_too_long = now.saturating_sub(record.created_unix) > OPEN_UNPAID_SECS;
    match (seen, record.state) {
        (Seen::Opened, _) => Next::Done,
        (Seen::Refunded, _) => Next::Fail(OrderWhy::Refunded),
        (Seen::NoBolt11, _) => Next::Fail(OrderWhy::NotPossible),
        (Seen::Expecting, OrderState::Open) if unpaid_too_long => Next::Fail(OrderWhy::Expired),
        (Seen::Expecting, OrderState::Open) => Next::Pay,
        // Paid, and the provider has not got it yet: the payment is on its way.
        (Seen::Expecting, _) => Next::Wait,
        (Seen::PaidNotOpened, OrderState::Open) => Next::MarkPaid,
        (Seen::PaidNotOpened, _)
            if now.saturating_sub(record.updated_unix) > GIVE_UP_AFTER_SECS =>
        {
            Next::Fail(OrderWhy::NoChannel)
        }
        (Seen::PaidNotOpened, _) => Next::Wait,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bip85::NodeSeed;
    use crate::device::{DeviceStatus, DirDevice, GatedDevice, SecondDevice};
    use crate::logger::Logger;
    use crate::seal::SealKey;
    use crate::testutil::temp_dir;
    use bitcoin::hashes::{sha256, Hash};
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use lightning::sign::KeysManager;
    use lightning::types::payment::PaymentSecret;
    use lightning::util::logger::Level;
    use lightning_invoice::{Currency, InvoiceBuilder};
    use lightning_liquidity::lsps0::ser::LSPSDateTime;
    use lightning_liquidity::lsps1::msgs::LSPS1Bolt11PaymentInfo;
    use lightning_persister::fs_store::FilesystemStore;

    const NOW: u64 = 2_000_000_000;

    fn key(n: u8) -> SecretKey {
        SecretKey::from_slice(&[n; 32]).unwrap()
    }

    fn provider() -> PublicKey {
        key(7).public_key(&Secp256k1::new())
    }

    fn options() -> LSPS1Options {
        LSPS1Options {
            min_required_channel_confirmations: 0,
            min_funding_confirms_within_blocks: 1,
            supports_zero_channel_reserve: false,
            max_channel_expiry_blocks: 4032,
            min_initial_client_balance_sat: 0,
            max_initial_client_balance_sat: 0,
            min_initial_lsp_balance_sat: 100_000,
            max_initial_lsp_balance_sat: 5_000_000,
            min_channel_balance_sat: 100_000,
            max_channel_balance_sat: 5_000_000,
        }
    }

    fn invoice_by(
        signer: u8,
        amount_msat: u64,
        hash: u8,
        expiry: u64,
    ) -> lightning_invoice::Bolt11Invoice {
        InvoiceBuilder::new(Currency::Regtest)
            .description("order".to_string())
            .payment_hash(sha256::Hash::from_slice(&[hash; 32]).unwrap())
            .payment_secret(PaymentSecret([9; 32]))
            .duration_since_epoch(Duration::from_secs(NOW))
            .min_final_cltv_expiry_delta(144)
            .expiry_time(Duration::from_secs(expiry))
            .amount_milli_satoshis(amount_msat)
            .build_signed(|h| Secp256k1::new().sign_ecdsa_recoverable(h, &key(signer)))
            .unwrap()
    }

    fn payment(
        signer: u8,
        amount_msat: u64,
        fee_sat: u64,
        state: LSPS1PaymentState,
    ) -> LSPS1PaymentInfo {
        LSPS1PaymentInfo {
            bolt11: Some(LSPS1Bolt11PaymentInfo {
                state,
                expires_at: LSPSDateTime::new_from_duration_since_epoch(Duration::from_secs(
                    NOW + 3600,
                )),
                fee_total_sat: fee_sat,
                order_total_sat: fee_sat,
                invoice: invoice_by(signer, amount_msat, 5, 3600),
            }),
            bolt12: None,
            onchain: None,
        }
    }

    fn record(state: OrderState) -> OrderRecord {
        OrderRecord {
            id: "order-1".into(),
            state,
            why: if state == OrderState::Failed {
                OrderWhy::Refused
            } else {
                OrderWhy::None
            },
            fee_sat: 2_000,
            payment_hash: [5; 32],
            created_unix: NOW,
            updated_unix: NOW,
        }
    }

    // ---- when to order ----

    #[test]
    fn sl_28_an_order_is_made_only_under_half_the_cap() {
        let book = OrderBook::default();
        // Cap 1,000,000 sat: half is 500,000 sat = 500,000,000 msat.
        assert!(order_wanted(499_999_999, 1_000_000, &book, NOW));
        assert!(!order_wanted(500_000_000, 1_000_000, &book, NOW));
        assert!(!order_wanted(900_000_000, 1_000_000, &book, NOW));
        assert!(order_wanted(0, 1_000_000, &book, NOW));
        // No cap, no order.
        assert!(!order_wanted(0, 0, &book, NOW));
    }

    #[test]
    fn sl_28_no_second_order_while_one_is_open() {
        for state in [OrderState::Open, OrderState::Paid] {
            let book = OrderBook {
                order: Some(record(state)),
                ..Default::default()
            };
            assert!(!order_wanted(0, 1_000_000, &book, NOW), "{state:?}");
        }
        // A finished or failed one does not stop a new order, once the wait is over.
        for state in [OrderState::Done, OrderState::Failed] {
            let book = OrderBook {
                order: Some(record(state)),
                ..Default::default()
            };
            assert!(order_wanted(0, 1_000_000, &book, NOW), "{state:?}");
        }
    }

    #[test]
    fn sl_28_after_a_refusal_no_order_for_an_hour() {
        let mut book = OrderBook {
            order: Some(record(OrderState::Open)),
            ..Default::default()
        };
        book.fail(OrderWhy::FeeAboveMax, NOW);
        assert!(!book.has_open_order());
        assert!(!order_wanted(0, 1_000_000, &book, NOW + 3599));
        assert!(order_wanted(0, 1_000_000, &book, NOW + 3600));
    }

    // ---- what to ask for ----

    #[test]
    fn sl_28_the_order_brings_the_room_up_to_the_cap() {
        // 300,000 sat of room, a cap of 1,000,000: the provider puts in 700,000.
        let p = order_params(&options(), 300_000_000, 1_000_000, None).unwrap();
        assert_eq!(p.lsp_balance_sat, 700_000);
        assert_eq!(p.client_balance_sat, 0);
        assert_eq!(p.required_channel_confirmations, 1);
        assert!(!p.announce_channel);
        assert_eq!(p.channel_expiry_blocks, 4032);
        assert_eq!(p.token, None);
    }

    #[test]
    fn sl_28_the_size_is_held_within_the_providers_limits() {
        // Little lacking: raised to the provider's smallest.
        let p = order_params(&options(), 950_000_000, 1_000_000, None).unwrap();
        assert_eq!(p.lsp_balance_sat, 100_000);
        // A lot lacking: cut to the provider's largest.
        let p = order_params(&options(), 0, 50_000_000, None).unwrap();
        assert_eq!(p.lsp_balance_sat, 5_000_000);
        // The channel's own limits count too.
        let mut o = options();
        o.min_channel_balance_sat = 200_000;
        o.max_channel_balance_sat = 600_000;
        assert_eq!(
            order_params(&o, 0, 1_000_000, None)
                .unwrap()
                .lsp_balance_sat,
            600_000
        );
        o.min_channel_balance_sat = 6_000_000;
        assert_eq!(
            order_params(&o, 0, 1_000_000, None),
            Err(NoOrder::OutsideLimits)
        );
    }

    #[test]
    fn sl_28_the_lease_is_the_most_allowed_and_the_provider_s_confirmations_are_met() {
        let mut o = options();
        o.max_channel_expiry_blocks = 1000;
        assert_eq!(
            order_params(&o, 0, 1_000_000, None)
                .unwrap()
                .channel_expiry_blocks,
            1000
        );
        o.max_channel_expiry_blocks = 0;
        assert_eq!(order_params(&o, 0, 1_000_000, None), Err(NoOrder::NoExpiry));
        let mut o = options();
        o.min_required_channel_confirmations = 3;
        o.min_funding_confirms_within_blocks = 12;
        let p = order_params(&o, 0, 1_000_000, Some("tok")).unwrap();
        assert_eq!(p.required_channel_confirmations, 3);
        assert_eq!(p.funding_confirms_within_blocks, 12);
        assert_eq!(p.token.as_deref(), Some("tok"));
    }

    #[test]
    fn sl_28_a_provider_that_needs_the_shops_coins_is_not_ordered_from() {
        let mut o = options();
        o.min_initial_client_balance_sat = 1;
        assert_eq!(
            order_params(&o, 0, 1_000_000, None),
            Err(NoOrder::ClientBalanceRequired)
        );
    }

    // ---- whether to pay ----

    #[test]
    fn sl_29_a_fee_within_the_most_is_paid() {
        let p = payment(7, 2_000_000, 2_000, LSPS1PaymentState::ExpectPayment);
        let plan = judge_payment(&p, &provider(), 3_000_000, 10_000_000, NOW).unwrap();
        assert_eq!(plan.amount_msat, 2_000_000);
        assert_eq!(plan.fee_sat, 2_000);
        assert_eq!(plan.payment_hash, [5; 32]);
        assert_eq!(plan.payment_secret, [9; 32]);
        assert_eq!(plan.min_final_cltv, 144);
        // The most itself is allowed.
        assert!(judge_payment(&p, &provider(), 2_000_000, 10_000_000, NOW).is_ok());
    }

    #[test]
    fn sl_29_a_fee_above_the_most_is_not_paid() {
        let p = payment(7, 2_000_000, 2_000, LSPS1PaymentState::ExpectPayment);
        assert_eq!(
            judge_payment(&p, &provider(), 1_999_999, 10_000_000, NOW),
            Err(PayRefusal::FeeAboveMax { fee_sat: 2_000 })
        );
    }

    #[test]
    fn sl_29_the_largest_of_the_providers_numbers_counts() {
        // The invoice asks 2,000 sat; the provider says its fee is 500: the
        // invoice is what leaves the node.
        let mut p = payment(7, 2_000_000, 500, LSPS1PaymentState::ExpectPayment);
        assert_eq!(
            judge_payment(&p, &provider(), 1_000_000, 10_000_000, NOW),
            Err(PayRefusal::FeeAboveMax { fee_sat: 2_000 })
        );
        // The invoice asks 500 sat and the provider's total is 4,000.
        p.bolt11.as_mut().unwrap().invoice = invoice_by(7, 500_000, 5, 3600);
        p.bolt11.as_mut().unwrap().order_total_sat = 4_000;
        assert_eq!(
            judge_payment(&p, &provider(), 3_000_000, 10_000_000, NOW),
            Err(PayRefusal::FeeAboveMax { fee_sat: 4_000 })
        );
    }

    #[test]
    fn sl_28_an_order_the_node_cannot_pay_is_not_paid() {
        let p = payment(7, 2_000_000, 2_000, LSPS1PaymentState::ExpectPayment);
        assert_eq!(
            judge_payment(&p, &provider(), 3_000_000, 1_999_999, NOW),
            Err(PayRefusal::CannotPay)
        );
    }

    #[test]
    fn sl_28_an_invoice_that_is_not_the_providers_is_never_paid() {
        let p = payment(8, 2_000_000, 2_000, LSPS1PaymentState::ExpectPayment);
        assert_eq!(
            judge_payment(&p, &provider(), 3_000_000, 10_000_000, NOW),
            Err(PayRefusal::WrongPayee)
        );
    }

    #[test]
    fn sl_28_only_the_lightning_option_is_paid_and_not_an_expired_one() {
        let mut p = payment(7, 2_000_000, 2_000, LSPS1PaymentState::ExpectPayment);
        assert_eq!(
            judge_payment(&p, &provider(), 3_000_000, 10_000_000, NOW + 3601),
            Err(PayRefusal::Expired)
        );
        p.bolt11 = None;
        assert_eq!(
            judge_payment(&p, &provider(), 3_000_000, 10_000_000, NOW),
            Err(PayRefusal::NoBolt11)
        );
    }

    // ---- what the provider says ----

    #[test]
    fn sl_28_the_providers_answer_is_read_by_its_payment_and_channel() {
        let expect = payment(7, 1_000, 1, LSPS1PaymentState::ExpectPayment);
        let paid = payment(7, 1_000, 1, LSPS1PaymentState::Paid);
        let refunded = payment(7, 1_000, 1, LSPS1PaymentState::Refunded);
        let channel = Some(LSPS1ChannelInfo {
            funded_at: LSPSDateTime::new_from_duration_since_epoch(Duration::from_secs(NOW)),
            funding_outpoint: bitcoin::OutPoint::null(),
            expires_at: LSPSDateTime::new_from_duration_since_epoch(Duration::from_secs(NOW)),
        });
        assert_eq!(read_status(&expect, &None), Seen::Expecting);
        assert_eq!(read_status(&paid, &None), Seen::PaidNotOpened);
        assert_eq!(read_status(&refunded, &None), Seen::Refunded);
        assert_eq!(read_status(&paid, &channel), Seen::Opened);
        let none = LSPS1PaymentInfo {
            bolt11: None,
            bolt12: None,
            onchain: None,
        };
        assert_eq!(read_status(&none, &None), Seen::NoBolt11);
    }

    #[test]
    fn sl_28_the_next_step_follows_the_state_and_the_providers_answer() {
        let open = record(OrderState::Open);
        let paid = record(OrderState::Paid);
        assert_eq!(next_step(&open, Seen::Expecting, NOW), Next::Pay);
        assert_eq!(next_step(&paid, Seen::Expecting, NOW), Next::Wait);
        assert_eq!(next_step(&open, Seen::PaidNotOpened, NOW), Next::MarkPaid);
        assert_eq!(next_step(&paid, Seen::PaidNotOpened, NOW + 60), Next::Wait);
        assert_eq!(
            next_step(&paid, Seen::PaidNotOpened, NOW + GIVE_UP_AFTER_SECS + 1),
            Next::Fail(OrderWhy::NoChannel)
        );
        assert_eq!(next_step(&paid, Seen::Opened, NOW), Next::Done);
        assert_eq!(
            next_step(&open, Seen::Refunded, NOW),
            Next::Fail(OrderWhy::Refunded)
        );
    }

    // ---- the book ----

    #[test]
    fn sl_45_the_fee_is_counted_once_when_the_payment_is_told_twice() {
        let mut book = OrderBook::default();
        book.begin(record(OrderState::Open));
        assert!(
            !book.paid(&[6; 32], NOW),
            "another payment is not this order's"
        );
        assert_eq!(book.fees_paid_sat, 0);
        assert!(book.paid(&[5; 32], NOW + 1));
        assert!(book.paid(&[5; 32], NOW + 2));
        assert_eq!(book.fees_paid_sat, 2_000);
        assert_eq!(book.order.as_ref().unwrap().state, OrderState::Paid);
        // The fees add up over orders.
        book.done(NOW + 10);
        let mut next = record(OrderState::Open);
        next.payment_hash = [6; 32];
        next.fee_sat = 500;
        book.begin(next);
        assert!(book.paid(&[6; 32], NOW + 20));
        assert_eq!(book.fees_paid_sat, 2_500);
    }

    #[test]
    fn sl_45_a_refund_takes_the_fee_off_again_and_an_order_opened_unseen_counts_once() {
        let mut book = OrderBook::default();
        book.begin(record(OrderState::Open));
        book.paid(&[5; 32], NOW);
        assert_eq!(book.fees_paid_sat, 2_000);
        book.fail(OrderWhy::Refunded, NOW + 5);
        assert_eq!(book.fees_paid_sat, 0);
        assert_eq!(book.order.as_ref().unwrap().why, OrderWhy::Refunded);
        // The provider opened the channel before the node saw its payment sent.
        let mut book = OrderBook::default();
        book.begin(record(OrderState::Open));
        book.done(NOW + 5);
        assert_eq!(book.fees_paid_sat, 2_000);
        assert!(book.paid(&[5; 32], NOW + 6));
        assert_eq!(book.fees_paid_sat, 2_000);
    }

    #[test]
    fn sl_28_a_failed_payment_fails_the_open_order_only() {
        let mut book = OrderBook::default();
        book.begin(record(OrderState::Open));
        assert!(!book.payment_failed(&[6; 32], NOW));
        assert!(book.has_open_order());
        assert!(book.payment_failed(&[5; 32], NOW + 1));
        assert_eq!(book.order.as_ref().unwrap().why, OrderWhy::PaymentFailed);
        assert_eq!(book.fees_paid_sat, 0);
        assert!(!order_wanted(0, 1_000_000, &book, NOW + 100));
        // An order already paid is not failed by a late failure event.
        let mut book = OrderBook::default();
        book.begin(record(OrderState::Open));
        book.paid(&[5; 32], NOW);
        assert!(!book.payment_failed(&[5; 32], NOW + 1));
        assert_eq!(book.order.as_ref().unwrap().state, OrderState::Paid);
    }

    #[test]
    fn sl_28_an_order_not_paid_within_the_hour_is_given_up() {
        let open = record(OrderState::Open);
        assert_eq!(next_step(&open, Seen::Expecting, NOW + 3600), Next::Pay);
        assert_eq!(
            next_step(&open, Seen::Expecting, NOW + 3601),
            Next::Fail(OrderWhy::Expired)
        );
    }

    #[test]
    fn sl_28_a_done_order_waits_an_hour_before_the_next() {
        let mut book = OrderBook::default();
        book.begin(record(OrderState::Open));
        book.paid(&[5; 32], NOW);
        book.done(NOW + 5);
        assert_eq!(book.order.as_ref().unwrap().state, OrderState::Done);
        assert!(!order_wanted(0, 1_000_000, &book, NOW + 100));
        assert!(order_wanted(0, 1_000_000, &book, NOW + 5 + 3600));
    }

    #[test]
    fn sl_28_the_book_reads_back_what_was_written_and_nothing_else() {
        let mut book = OrderBook {
            fees_paid_sat: 77,
            next_try_unix: NOW + 5,
            order: Some(record(OrderState::Paid)),
        };
        assert_eq!(decode(&encode(&book)).unwrap(), book);
        book.order = Some(record(OrderState::Failed));
        assert_eq!(decode(&encode(&book)).unwrap(), book);
        book.order = None;
        assert_eq!(decode(&encode(&book)).unwrap(), book);
        assert_eq!(
            decode(&encode(&OrderBook::default())).unwrap(),
            OrderBook::default()
        );

        // Strict: a trailing byte, a short blob, a wrong version, a state
        // that is not one, a reason on an order that did not fail, and an
        // id that is not printable are all refused.
        let good = encode(&OrderBook {
            order: Some(record(OrderState::Paid)),
            ..Default::default()
        });
        let mut bad = good.clone();
        bad.push(0);
        assert!(decode(&bad).is_err());
        assert!(decode(&good[..good.len() - 1]).is_err());
        let mut bad = good.clone();
        bad[0] = 2;
        assert!(decode(&bad).is_err());
        let id_end = 1 + 8 + 8 + 1 + 1 + "order-1".len();
        let mut bad = good.clone();
        bad[id_end] = 9;
        assert!(decode(&bad).is_err());
        let mut bad = good.clone();
        bad[id_end + 1] = 1;
        assert!(decode(&bad).is_err());
        let mut bad = good.clone();
        bad[1 + 8 + 8 + 2] = b' ';
        assert!(decode(&bad).is_err());
        assert!(decode(&[]).is_err());
    }

    // ---- the store: a restart keeps the order ----

    struct Disks {
        dir: std::path::PathBuf,
        device: GatedDevice<DirDevice>,
        key: SealKey,
    }

    fn disks(name: &str) -> Disks {
        let dir = temp_dir(name);
        Disks {
            device: GatedDevice::new(DirDevice::new(dir.join("device")).unwrap()),
            key: SealKey::from_seed(&NodeSeed([5; 64])),
            dir,
        }
    }

    /// A fresh store over the same two disks: what a start makes.
    fn start(d: &Disks) -> Arc<DualStore> {
        Arc::new(DualStore::new(
            Arc::new(FilesystemStore::new(d.dir.join("local"))),
            Arc::new(d.device.clone()),
            d.key.clone(),
            Arc::new(KeysManager::new(&[1; 32], 1, 1, true)),
            Arc::new(DeviceStatus::new()),
            Arc::new(Logger::open(&d.dir.join("log"), Level::Debug).unwrap()),
        ))
    }

    #[tokio::test]
    async fn sl_28_a_restart_keeps_the_open_order_and_makes_no_second() {
        let d = disks("lsps1-restart");
        let orders = Orders::load(start(&d)).await.unwrap();
        assert_eq!(orders.snapshot(), OrderBook::default());
        orders
            .change(|b| b.begin(record(OrderState::Open)))
            .await
            .unwrap();
        orders
            .change(|b| {
                b.paid(&[5; 32], NOW + 1);
            })
            .await
            .unwrap();
        drop(orders);
        // The node is stopped and started: a new store over the same disks.
        let again = Orders::load(start(&d)).await.unwrap();
        let book = again.snapshot();
        assert_eq!(book.order.as_ref().unwrap().state, OrderState::Paid);
        assert_eq!(book.fees_paid_sat, 2_000);
        assert!(!order_wanted(0, 1_000_000, &book, NOW + 100));
    }

    #[tokio::test]
    async fn sl_28_the_order_goes_to_the_second_device_first_and_is_made_only_when_both_hold_it() {
        let d = disks("lsps1-both");
        let orders = Arc::new(Orders::load(start(&d)).await.unwrap());
        d.device.set_away(true);
        let o = Arc::clone(&orders);
        let write =
            tokio::spawn(async move { o.change(|b| b.begin(record(OrderState::Open))).await });
        tokio::time::sleep(Duration::from_millis(700)).await;
        // Not returned, and the counter's own copy is not ahead of the device's.
        assert!(!write.is_finished());
        assert!(!d.dir.join("local").join(PRIMARY).join(KEY).exists());
        d.device.set_away(false);
        tokio::time::timeout(Duration::from_secs(10), write)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(d.dir.join("local").join(PRIMARY).join(KEY).exists());
        let name = crate::store::device_name(PRIMARY, "", KEY);
        assert!(d.device.read(&name).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn sl_28_a_blob_that_does_not_read_stops_the_load() {
        let d = disks("lsps1-bad");
        let store = start(&d);
        KVStore::write(&*store, PRIMARY, "", KEY, vec![9, 9, 9])
            .await
            .unwrap();
        assert!(Orders::load(store).await.is_err());
    }
}

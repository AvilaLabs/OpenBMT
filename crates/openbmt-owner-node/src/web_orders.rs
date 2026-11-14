//! What the owner phone keeps of its web orders (`docs/WEB-ORDER-RULES.md`,
//! WO-05 to WO-32): the pairing with a store, the ranges of addresses it
//! gave the store, the orders the store reported and what the store was told
//! of each. The visits that use it are in [`crate::web_visit`].
//!
//! - **The file** is `web-orders` in the node's folder, framed with the magic
//!   `OBM"`. It is written whole (a temporary file, a flush, a rename, a flush
//!   of the directory) before any change takes effect, and read back
//!   strictly: a file that does not read, that is not in the order it is
//!   written in or that does not encode to the same bytes is refused whole,
//!   left as it is, and web orders are off until the owner acts (as RQ-31).
//! - **It holds the pairing secret, not the web order key.** The key's seed
//!   is made again each time from the owner secret the node holds and the
//!   pairing's id ([`web_key_seed`]), so the file carries nothing that signs.
//! - **The addresses** a range holds are desk indexes of the request chain
//!   (RQ-03), kept in the live watch from the moment the range is written,
//!   whether an order uses them or not (WO-12, RQ-04).
//! - **The sequence** a range is sent with (`start`, WO-10) counts the
//!   addresses given to the store so far, from 0, with no gap, which is what
//!   the store's pool asks of its next range. The desk's own indexes are
//!   not always next to each other, so the chain index of each address is
//!   kept here and never sent.
//!
//! Layout: the magic, the version byte 1, the pairing (a byte 0 for none, 1
//! for one the owner has not confirmed, 2 for a confirmed one, then the
//! secret, the shop's name and the store's address, and when it was
//! made), the number of the last order taken, the sequence of the next range,
//! when the store was last visited, the addresses the store said it had left,
//! the store's last refusal of a range, the ranges, the orders and the
//! orders that were refused. Numbers are little endian.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use openbmt_core::say::Words;
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_owner::web_order::say as words;
use openbmt_owner::web_order::say::StoreTrouble;
use openbmt_owner::web_order::{
    OrderRefusal, Range, RangeResult, SentRanges, StoreOrder, WebOrderKey, WebOrderRefusal,
    MAX_SHOP_NAME_BYTES, RANGE_SIZE, SECRET_BYTES,
};
use openbmt_requests::BookError;

use crate::requests::RequestsOff;
use crate::watcher::write_file;

/// The file in the node's folder.
pub const WEB_ORDERS_FILE: &str = "web-orders";

/// Magic of the file: the last printable byte after `OBM` that no other frame
/// of this workspace has.
pub const MAGIC_WEB_ORDERS: [u8; 4] = *b"OBM\"";

/// Most ranges kept: 20,000 addresses. (A limit of this module, for the size
/// of the file: the rules name none.)
pub const MAX_RANGES: usize = 200;

/// Most orders kept. (A limit of this module, as above.)
pub const MAX_ORDERS: usize = 20_000;

/// Most refused orders kept; the oldest goes first.
pub const MAX_REFUSALS: usize = 100;

/// WO-11: the store is given the next range at the visit after it has fewer
/// than this many addresses left.
pub const LOW_ADDRESSES: u64 = 20;

/// The longest address of a store that is taken, in bytes.
pub const MAX_STORE_URL_BYTES: usize = 200;

/// The most transaction IDs one order's notice key counts.
const MAX_KEY_TXIDS: u8 = 16;

/// The bytes of an order's record in the file, at most (for the count's
/// check): an order number, a short address and a request number.
const ORDER_MIN_LEN: usize = 8 + 1 + 14 + 1 + 1 + 1;

/// The bytes of a range's record in the file at least.
const RANGE_MIN_LEN: usize = 4 + 32 + 1 + 1;

/// Why web orders could not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebError {
    /// RQ-01, RQ-31: requests are off, so web orders are.
    Off(RequestsOff),
    /// The file of web orders cannot be read or stored.
    File(String),
    /// WO-06: the store's address is not an address with HTTPS.
    Url,
    /// WO-05: a store is paired already.
    AlreadyPaired,
    /// WO-05: no store is paired.
    NotPaired,
    /// WO-05, WO-43: the library refused.
    Library(WebOrderRefusal),
    /// WO-10: the indexes given for a range are not what a range takes.
    Indexes,
    /// WO-10: no more ranges can be kept.
    TooMany,
    /// The call does not follow the one before it.
    Sequence,
}

impl fmt::Display for WebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off(why) => write!(f, "{why}"),
            Self::File(why) => write!(f, "WO-12: the file of web orders: {why}"),
            Self::Url => write!(f, "WO-06: the store's address is not an https address"),
            Self::AlreadyPaired => write!(f, "WO-05: a store is paired already"),
            Self::NotPaired => write!(f, "WO-05: no store is paired"),
            Self::Library(r) => write!(f, "{}: a web order frame or key was refused", r.rule()),
            Self::Indexes => write!(f, "WO-10: the indexes are not what a range takes"),
            Self::TooMany => write!(f, "WO-10: the file holds as many addresses as it may"),
            Self::Sequence => write!(f, "WO-20: the call does not follow the one before it"),
        }
    }
}

impl std::error::Error for WebError {}

impl WebError {
    /// The words for the refusal: the requests' own where requests are off,
    /// and the placeholder of the rule where no words are written yet.
    pub fn words(&self) -> Words {
        match self {
            Self::Off(why) => crate::requests::RequestError::Off(why.clone()).words(),
            Self::File(_) => words::store_words(StoreTrouble::File),
            Self::TooMany => words::store_words(StoreTrouble::TooMany),
            Self::Url => words::store_words(StoreTrouble::Url),
            Self::AlreadyPaired => words::store_words(StoreTrouble::AlreadyPaired),
            Self::NotPaired => words::store_words(StoreTrouble::NotPaired),
            Self::Library(r) => words::refusal_words(r),
            Self::Indexes => words::store_words(StoreTrouble::Indexes),
            Self::Sequence => words::visit_words(words::VisitTrouble::Sequence),
        }
    }
}

/// The store's address, checked (WO-06): `https://`, a name or a number with
/// no user, an optional port, and an optional path of a store kept in a
/// folder. No query, no fragment, no space. Returns the address in the form
/// kept: the name in lower case, no slash at the end.
pub fn check_store_url(text: &str) -> Result<String, WebError> {
    let text = text.trim();
    let rest = text
        .get(..8)
        .filter(|s| s.eq_ignore_ascii_case("https://"))
        .and_then(|_| text.get(8..))
        .ok_or(WebError::Url)?;
    if text.len() > MAX_STORE_URL_BYTES || !text.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(WebError::Url);
    }
    let (authority, path) = match rest.find('/') {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    if path.contains(['?', '#', '\\', '%']) || authority.contains(['@', '[', ']']) {
        return Err(WebError::Url);
    }
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    if let Some(p) = port {
        let digits = !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit());
        if !digits || p.parse::<u16>().map_or(true, |n| n == 0) {
            return Err(WebError::Url);
        }
    }
    let label_ok = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    if host.is_empty() || !host.split('.').all(label_ok) {
        return Err(WebError::Url);
    }
    let path = path.trim_end_matches('/');
    let port = port.map(|p| format!(":{p}")).unwrap_or_default();
    Ok(format!("https://{}{port}{path}", host.to_ascii_lowercase()))
}

/// The tag of the web order key's seed.
const TAG_WEB_KEY: &str = "OpenBMT/web-order-key/v1";

/// The Ed25519 seed of the web order key of the pairing with `secret`: the
/// tagged hash of the owner secret and the pairing's id (WO-10). The same
/// owner secret gives the same key after a restart; another gives another.
pub(crate) fn web_key_seed(owner_secret: &[u8; 32], secret: &[u8; SECRET_BYTES]) -> [u8; 32] {
    let id = openbmt_owner::web_order::pairing_id_of(secret);
    let mut msg = Vec::with_capacity(32 + id.len());
    msg.extend_from_slice(owner_secret);
    msg.extend_from_slice(&id);
    openbmt_core::identity::tagged_hash(TAG_WEB_KEY, &msg)
}

/// How far a pairing has come (WO-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairState {
    /// The code is shown and the owner has not said the fingerprints match.
    Waiting,
    /// The owner confirmed the fingerprints; the store is visited.
    Paired,
}

/// The pairing with a store (WO-05).
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Pairing {
    pub(crate) secret: [u8; SECRET_BYTES],
    pub(crate) shop_name: String,
    pub(crate) url: String,
    pub(crate) confirmed: bool,
    pub(crate) made_at: u64,
}

impl fmt::Debug for Pairing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pairing")
            .field("shop_name", &self.shop_name)
            .field("url", &self.url)
            .field("confirmed", &self.confirmed)
            .finish_non_exhaustive()
    }
}

impl Pairing {
    /// The web order key of this pairing, made from the owner secret.
    pub(crate) fn key(&self, owner_secret: &[u8; 32]) -> WebOrderKey {
        WebOrderKey::from_seed(&web_key_seed(owner_secret, &self.secret))
    }

    pub(crate) fn state(&self) -> PairState {
        if self.confirmed {
            PairState::Paired
        } else {
            PairState::Waiting
        }
    }
}

/// A range given to the store (WO-10, WO-12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SentBlock {
    /// The sequence of its first address.
    pub(crate) seq: u32,
    /// The checksum of the wallet its addresses were made from (RQ-04).
    pub(crate) wallet: [u8; 32],
    /// Whether the store said it holds the range.
    pub(crate) delivered: bool,
    /// The desk index and the text of each address, in order.
    pub(crate) entries: Vec<(u32, String)>,
}

/// What the store was last told of an order, as far as the notice's
/// content goes (WO-30): the state, what was counted and how many
/// transactions it came in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NoticeKey {
    pub(crate) state: u8,
    pub(crate) counted: u64,
    pub(crate) txids: u8,
}

/// An order taken (WO-20) and what the store was told of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrderRec {
    pub(crate) order: u64,
    pub(crate) address: String,
    /// The request made for it; none for an order the request book refused.
    pub(crate) request: Option<u64>,
    /// The notice the store took (WO-32).
    pub(crate) acked: Option<NoticeKey>,
    /// The notice the store refused: not sent again until it changes.
    pub(crate) refused: Option<NoticeKey>,
}

/// Why an order was refused and shown (WO-22).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusedWhy {
    /// The address was not in a range the owner phone sent.
    AddressNotSent,
    /// The address belongs to another order already.
    AddressUsedBefore,
    /// The order was reported before with another address.
    OrderChanged,
    /// The amount is zero.
    NoAmount,
    /// The request book would not take the order: the smallest sale, too
    /// many open requests, an amount too large, or a wallet since changed.
    NoRequest,
}

impl RefusedWhy {
    /// The byte kept in the file.
    pub fn byte(self) -> u8 {
        match self {
            Self::AddressNotSent => 1,
            Self::AddressUsedBefore => 2,
            Self::OrderChanged => 3,
            Self::NoAmount => 4,
            Self::NoRequest => 5,
        }
    }

    /// The reason of a byte.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::AddressNotSent),
            2 => Some(Self::AddressUsedBefore),
            3 => Some(Self::OrderChanged),
            4 => Some(Self::NoAmount),
            5 => Some(Self::NoRequest),
            _ => None,
        }
    }

    /// WO-22: the words for the refusal.
    pub fn words(self) -> Words {
        match self {
            Self::AddressNotSent => words::order_refusal_words(&OrderRefusal::AddressNotSent),
            Self::AddressUsedBefore => words::order_refusal_words(&OrderRefusal::AddressUsedBefore),
            Self::OrderChanged => words::order_refusal_words(&OrderRefusal::OrderChanged),
            Self::NoAmount => words::order_refusal_words(&OrderRefusal::NoAmount),
            Self::NoRequest => words::no_request_words(),
        }
    }

    pub(crate) fn of(refusal: OrderRefusal) -> Self {
        match refusal {
            OrderRefusal::AddressNotSent => Self::AddressNotSent,
            OrderRefusal::AddressUsedBefore => Self::AddressUsedBefore,
            OrderRefusal::OrderChanged => Self::OrderChanged,
            OrderRefusal::NoAmount => Self::NoAmount,
        }
    }
}

/// An order the owner phone refused (WO-22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedOrder {
    /// The order number the store gave.
    pub order: u64,
    /// The address the store gave the customer.
    pub address: String,
    /// The amount the store asked for, in satoshis.
    pub sats: u64,
    /// Why.
    pub why: RefusedWhy,
    /// When it was refused.
    pub at: u64,
}

/// What the file holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct WebState {
    pub(crate) pairing: Option<Pairing>,
    /// The number of the last order taken (WO-20).
    pub(crate) since: u64,
    /// The sequence of the next range's first address.
    pub(crate) next_seq: u32,
    pub(crate) last_visit: Option<u64>,
    /// The addresses the store said it had left at the last answer.
    pub(crate) left: Option<u64>,
    /// What the store said of a range it did not take.
    pub(crate) trouble: Option<RangeResult>,
    pub(crate) blocks: Vec<SentBlock>,
    pub(crate) orders: Vec<OrderRec>,
    pub(crate) refusals: Vec<RefusedOrder>,
}

/// What the file holds and what follows from it: the check of WO-22 and the
/// chain index of each address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Loaded {
    pub(crate) state: WebState,
    pub(crate) sent: SentRanges,
    pub(crate) index_of: BTreeMap<String, (u32, [u8; 32])>,
}

impl Loaded {
    pub(crate) fn new(state: WebState) -> Result<Self, &'static str> {
        let mut sent = SentRanges::new();
        let mut index_of = BTreeMap::new();
        for b in &state.blocks {
            let addresses: Vec<String> = b.entries.iter().map(|(_, a)| a.clone()).collect();
            let range = Range::new(b.seq, addresses).map_err(|_| "a range is not one")?;
            sent.add_range(&range)
                .map_err(|_| "an address was sent twice")?;
            for (index, address) in &b.entries {
                index_of.insert(address.clone(), (*index, b.wallet));
            }
        }
        for o in &state.orders {
            let order = StoreOrder {
                order: o.order,
                address: o.address.clone(),
                sats: 1,
                quoted_at: 0,
                total_minor: 0,
                currency: String::new(),
            };
            if !matches!(
                sent.check(&order),
                Ok(openbmt_owner::web_order::OrderCheck::New)
            ) {
                return Err("an order has an address that is not its own");
            }
        }
        Ok(Self {
            state,
            sent,
            index_of,
        })
    }

    /// The desk indexes of every address given to a store (WO-12).
    pub(crate) fn indexes(&self) -> BTreeSet<u32> {
        self.index_of.values().map(|(i, _)| *i).collect()
    }

    pub(crate) fn order(&self, number: u64) -> Option<&OrderRec> {
        self.state
            .orders
            .binary_search_by_key(&number, |o| o.order)
            .ok()
            .and_then(|at| self.state.orders.get(at))
    }
}

fn put_text(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&wire::len_u32(text.len()).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

fn put_key(out: &mut Vec<u8>, key: Option<NoticeKey>) {
    match key {
        Some(k) => {
            out.push(1);
            out.push(k.state);
            out.extend_from_slice(&k.counted.to_le_bytes());
            out.push(k.txids);
        }
        None => out.push(0),
    }
}

/// The bytes of the file for a state.
pub(crate) fn encode(state: &WebState) -> Vec<u8> {
    let mut out = wire::start(
        &MAGIC_WEB_ORDERS,
        256 + state.blocks.len() * 4_000 + state.orders.len() * 64,
    );
    match &state.pairing {
        None => out.push(0),
        Some(p) => {
            out.push(if p.confirmed { 2 } else { 1 });
            out.extend_from_slice(&p.secret);
            put_text(&mut out, &p.shop_name);
            put_text(&mut out, &p.url);
            out.extend_from_slice(&p.made_at.to_le_bytes());
        }
    }
    out.extend_from_slice(&state.since.to_le_bytes());
    out.extend_from_slice(&state.next_seq.to_le_bytes());
    wire::put_opt_u64(&mut out, state.last_visit);
    wire::put_opt_u64(&mut out, state.left);
    out.push(state.trouble.map_or(0, RangeResult::byte));
    out.extend_from_slice(&wire::len_u32(state.blocks.len()).to_le_bytes());
    for b in &state.blocks {
        out.extend_from_slice(&b.seq.to_le_bytes());
        out.extend_from_slice(&b.wallet);
        out.push(u8::from(b.delivered));
        out.push(u8::try_from(b.entries.len()).unwrap_or(0));
        for (index, address) in &b.entries {
            out.extend_from_slice(&index.to_le_bytes());
            out.push(u8::try_from(address.len()).unwrap_or(0));
            out.extend_from_slice(address.as_bytes());
        }
    }
    out.extend_from_slice(&wire::len_u32(state.orders.len()).to_le_bytes());
    for o in &state.orders {
        out.extend_from_slice(&o.order.to_le_bytes());
        out.push(u8::try_from(o.address.len()).unwrap_or(0));
        out.extend_from_slice(o.address.as_bytes());
        wire::put_opt_u64(&mut out, o.request);
        put_key(&mut out, o.acked);
        put_key(&mut out, o.refused);
    }
    out.extend_from_slice(&wire::len_u32(state.refusals.len()).to_le_bytes());
    for r in &state.refusals {
        out.extend_from_slice(&r.order.to_le_bytes());
        out.push(u8::try_from(r.address.len()).unwrap_or(0));
        out.extend_from_slice(r.address.as_bytes());
        out.extend_from_slice(&r.sats.to_le_bytes());
        out.push(r.why.byte());
        out.extend_from_slice(&r.at.to_le_bytes());
    }
    out
}

fn corrupt(e: DecodeError) -> &'static str {
    match e {
        DecodeError::NonCanonical(why) => why,
        DecodeError::TooLarge => "the file holds more than it may",
        DecodeError::TrailingBytes => "bytes follow the last entry",
        _ => "the file is cut short or not framed as it is written",
    }
}

fn get_text(r: &mut Reader<'_>, max: usize) -> Result<String, &'static str> {
    let len = usize::try_from(r.u32_le().map_err(corrupt)?).map_err(|_| "a text is too long")?;
    if len > max {
        return Err("a text is too long");
    }
    let raw = r.take(len).map_err(corrupt)?;
    std::str::from_utf8(raw)
        .map(str::to_string)
        .map_err(|_| "a text is not UTF-8")
}

fn get_address(r: &mut Reader<'_>) -> Result<String, &'static str> {
    let len = usize::from(r.u8().map_err(corrupt)?);
    let raw = r.take(len).map_err(corrupt)?;
    std::str::from_utf8(raw)
        .map(str::to_string)
        .map_err(|_| "an address is not text")
}

fn get_key(r: &mut Reader<'_>) -> Result<Option<NoticeKey>, &'static str> {
    if !r.flag().map_err(corrupt)? {
        return Ok(None);
    }
    let key = NoticeKey {
        state: r.u8().map_err(corrupt)?,
        counted: r.u64_le().map_err(corrupt)?,
        txids: r.u8().map_err(corrupt)?,
    };
    if !(1..=4).contains(&key.state) || key.txids == 0 || key.txids > MAX_KEY_TXIDS {
        return Err("a notice is not one");
    }
    Ok(Some(key))
}

/// Reads the file strictly. A file that is not exactly what [`encode`] makes
/// of the state it holds is refused whole.
pub(crate) fn decode(bytes: &[u8]) -> Result<WebState, &'static str> {
    let mut r = Reader::framed(bytes, &MAGIC_WEB_ORDERS).map_err(corrupt)?;
    let pairing = match r.u8().map_err(corrupt)? {
        0 => None,
        kind @ (1 | 2) => {
            let secret = r.array::<SECRET_BYTES>().map_err(corrupt)?;
            let shop_name = get_text(&mut r, MAX_SHOP_NAME_BYTES)?;
            let url = get_text(&mut r, MAX_STORE_URL_BYTES)?;
            if shop_name.is_empty()
                || check_store_url(&url).as_deref() != Ok(url.as_str())
                || shop_name.bytes().any(|b| b < 0x20 || b == 0x7f)
            {
                return Err("the pairing is not one");
            }
            Some(Pairing {
                secret,
                shop_name,
                url,
                confirmed: kind == 2,
                made_at: r.u64_le().map_err(corrupt)?,
            })
        }
        _ => return Err("the pairing is not one"),
    };
    let since = r.u64_le().map_err(corrupt)?;
    let next_seq = r.u32_le().map_err(corrupt)?;
    let last_visit = r.opt_u64().map_err(corrupt)?;
    let left = r.opt_u64().map_err(corrupt)?;
    let trouble = match r.u8().map_err(corrupt)? {
        0 => None,
        b => Some(RangeResult::from_byte(b).ok_or("a refusal of a range is not one")?),
    };
    let n = r.count(MAX_RANGES, RANGE_MIN_LEN).map_err(corrupt)?;
    let mut blocks: Vec<SentBlock> = Vec::with_capacity(n);
    let mut expect = 0u32;
    for _ in 0..n {
        let seq = r.u32_le().map_err(corrupt)?;
        let wallet = r.array::<32>().map_err(corrupt)?;
        let delivered = r.flag().map_err(corrupt)?;
        let count = usize::from(r.u8().map_err(corrupt)?);
        if seq != expect || count == 0 || count > RANGE_SIZE {
            return Err("the ranges do not follow each other from 0");
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let index = r.u32_le().map_err(corrupt)?;
            entries.push((index, get_address(&mut r)?));
        }
        expect = expect
            .checked_add(u32::try_from(count).map_err(|_| "too many")?)
            .ok_or("the sequence is too long")?;
        blocks.push(SentBlock {
            seq,
            wallet,
            delivered,
            entries,
        });
    }
    if next_seq != expect {
        return Err("the sequence of the next range is not where the last ended");
    }
    let n = r.count(MAX_ORDERS, ORDER_MIN_LEN).map_err(corrupt)?;
    let mut orders: Vec<OrderRec> = Vec::with_capacity(n);
    for _ in 0..n {
        let order = r.u64_le().map_err(corrupt)?;
        let address = get_address(&mut r)?;
        let request = r.opt_u64().map_err(corrupt)?;
        let acked = get_key(&mut r)?;
        let refused = get_key(&mut r)?;
        if orders.last().is_some_and(|last| last.order >= order) {
            return Err("the orders are not in order, or one is there twice");
        }
        orders.push(OrderRec {
            order,
            address,
            request,
            acked,
            refused,
        });
    }
    let n = r.count(MAX_REFUSALS, 8 + 1 + 8 + 1 + 8).map_err(corrupt)?;
    let mut refusals = Vec::with_capacity(n);
    for _ in 0..n {
        refusals.push(RefusedOrder {
            order: r.u64_le().map_err(corrupt)?,
            address: get_address(&mut r)?,
            sats: r.u64_le().map_err(corrupt)?,
            why: RefusedWhy::from_byte(r.u8().map_err(corrupt)?)
                .ok_or("a refusal of an order is not one")?,
            at: r.u64_le().map_err(corrupt)?,
        });
    }
    r.finish().map_err(corrupt)?;
    let state = WebState {
        pairing,
        since,
        next_seq,
        last_visit,
        left,
        trouble,
        blocks,
        orders,
        refusals,
    };
    if encode(&state) != bytes {
        return Err("it does not encode to the same bytes");
    }
    Ok(state)
}

/// Reads the file of a folder: none is an empty state, one that does not
/// read is an error and is not touched.
pub(crate) fn open(dir: &Path) -> Result<Loaded, WebError> {
    let state = match openbmt_requests::fsio::read_file(dir, WEB_ORDERS_FILE) {
        Ok(Some(bytes)) => decode(&bytes).map_err(|why| WebError::File(why.to_string()))?,
        Ok(None) => WebState::default(),
        Err(e) => return Err(WebError::File(book_text(&e))),
    };
    Loaded::new(state).map_err(|why| WebError::File(why.to_string()))
}

fn book_text(e: &BookError) -> String {
    e.to_string()
}

/// Stores the state whole.
pub(crate) fn store(dir: &Path, state: &WebState) -> Result<(), WebError> {
    write_file(dir, WEB_ORDERS_FILE, &encode(state)).map_err(WebError::File)
}

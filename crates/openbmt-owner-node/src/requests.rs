//! The owner phone's payment requests on the node (`docs/REQUEST-RULES.md`,
//! RQ-01 to RQ-37).
//!
//! The library `openbmt-requests` holds the judgment and keeps no clock, no
//! network and no screen. This module is what joins it to the node:
//!
//! - **The book** ([`RequestBook`]) is kept in the node's folder
//!   (`request-book`, `OBM=`) and is written whole before any change takes
//!   effect: a change is made on a copy, the copy is stored, and only then
//!   does it replace the book in memory (RQ-03, RQ-12, RQ-30). A book that
//!   does not read back is never replaced: requests are off, with the
//!   library's words, until the owner acts (RQ-31).
//! - **The addresses** are receive indexes of the desk, the owner root's own
//!   device in the allocation book (RQ-02). The caller takes the next unused
//!   index from [`OwnNode::desk_index`](crate::OwnNode::desk_index), which
//!   keeps the desk's range in the book before its first index is used; the
//!   node keeps every index a request ever had in the live watch, and never
//!   sets one aside (RQ-04, OC-22).
//! - **The payments** are found in the blocks the coin view applies: an
//!   output to a request's address in a checked block is kept as an arrival
//!   with its block's time (`request-arrivals`, `OBM\`), and is credited to
//!   the request once the block has the confirmation depth of the shop's
//!   settings, and no fewer than one (RQ-10, RQ-15). Nothing is credited from
//!   an unconfirmed transaction (OC-01, OC-02).
//! - **A block that leaves the chain** takes back the arrivals and the
//!   credits of its blocks from the same place the coin view undoes them,
//!   while the node runs and at its start (RQ-37, OC-53).
//! - **A return** goes onto the owner's refund queue and is never paid at
//!   once (RQ-29). Its refund is a refund of the desk's sale: the owner root
//!   as the counter and the request's address index as the sale.
//! - **Time** is passed in by the app on every call that needs it; there is
//!   no timer and no job (RQ-22, RQ-23).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use bitcoin::hashes::Hash;
use bitcoin::{Address, BlockHash, ScriptBuf};
use openbmt_core::currency::ShopCurrency;
use openbmt_core::say::Words;
use openbmt_core::wallet::Keychain;
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_owner::own_say::needed;
use openbmt_owner::{SaleFacts, SaleKey, Stage};
use openbmt_requests::say;
use openbmt_requests::{
    check_return_address, share_link, AddressRefused, Arrival, BookError, CheckedRate, CreditRefused,
    Day, DueRepeat, EndRefused, MakeRefused, NewRequest, Quote, RepeatRefused, RequestBook,
    ReturnAmount, ReturnRefused, ShareRefused, Shop,
};
use openbmt_watch::HeaderChain;

use crate::node::{OwnerNode, OwnerPort};
use crate::watcher::write_file;

pub use openbmt_requests::REQUEST_BOOK_FILE;

/// The arrivals that wait for their block to reach the confirmation depth,
/// in the node's folder.
pub const REQUEST_ARRIVALS_FILE: &str = "request-arrivals";

/// Magic of the arrivals file: the free frame magic `OBM\`.
const MAGIC_REQUEST_ARRIVALS: [u8; 4] = *b"OBM\\";

/// Most arrivals kept at once. (A limit of this module, for the size of the
/// file: the rules name none.)
pub const MAX_ARRIVALS: usize = 10_000;

/// The bytes of one arrival in the file.
const ARRIVAL_LEN: usize = 4 + 32 + 4 + 4 + 32 + 8 + 8;

/// The label a return's refund carries on the owner's queue, so that the
/// node finds the return again from the refund and the refund from the
/// return: "RQ" and the request's number, then "R" and the return's id.
pub fn return_label(request: u64, id: u64) -> String {
    format!("RQ{request}R{id}")
}

/// What the node needs from the shop's settings, and the shop's time zone,
/// before it can make a request or count a payment. The app passes it on
/// every call and the node keeps the newest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestContext {
    /// The shop's currency (CR-193).
    pub currency: ShopCurrency,
    /// The smallest bitcoin sale in satoshis; 0 for no floor (CR-75).
    pub min_sale_sats: u64,
    /// The confirmations the settings ask for; a request is paid at no fewer
    /// than one (RQ-10).
    pub min_confirmations: u8,
    /// The shop's time zone, in seconds east of UTC.
    pub tz_offset_secs: i32,
}

impl RequestContext {
    fn shop(&self) -> Shop {
        Shop {
            currency: self.currency,
            min_sale_sats: self.min_sale_sats,
            tz_offset_secs: self.tz_offset_secs,
        }
    }

    fn depth(&self) -> u32 {
        u32::from(self.min_confirmations.max(1))
    }
}

/// Why requests are off (RQ-01, RQ-31).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestsOff {
    /// RQ-01: the shop is paired through the Key app and has the counter's
    /// requests only.
    KeyApp,
    /// RQ-31: the book (or the file of arrivals) cannot be read or stored.
    /// It is left as it is, and no new request is made.
    Unreadable(BookError),
}

impl fmt::Display for RequestsOff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyApp => write!(f, "RQ-01: a shop with a Key app has no requests of the owner"),
            Self::Unreadable(e) => write!(f, "{e}"),
        }
    }
}

/// Why a request call was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {
    /// Requests are off.
    Off(RequestsOff),
    /// The shop's settings were not given yet.
    NoContext,
    /// RQ-05 to RQ-11.
    Make(MakeRefused),
    /// RQ-12.
    Share(ShareRefused),
    /// RQ-16, RQ-19, RQ-20.
    End(EndRefused),
    /// RQ-25.
    Repeat(RepeatRefused),
    /// RQ-26, RQ-29.
    Return(ReturnRefused),
    /// RQ-28.
    Address(AddressRefused),
    /// RQ-29: the owner's queue did not take the return.
    Queue(String),
    /// RQ-30: the book could not be stored; nothing changed.
    Store(BookError),
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off(why) => write!(f, "{why}"),
            Self::NoContext => write!(
                f,
                "RQ-35: the shop's settings and time zone were not given yet"
            ),
            Self::Make(e) => write!(f, "{e}"),
            Self::Share(e) => write!(f, "{e}"),
            Self::End(e) => write!(f, "{e}"),
            Self::Repeat(e) => write!(f, "{e}"),
            Self::Return(e) => write!(f, "{e}"),
            Self::Address(e) => write!(f, "{e}"),
            Self::Queue(e) => write!(f, "RQ-29: the owner's queue did not take the return: {e}"),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RequestError {}

impl RequestError {
    /// RQ-36: the words for the refusal. The library's own where it has
    /// them, and a placeholder that names the rule where it has none.
    pub fn words(&self) -> Words {
        match self {
            Self::Off(RequestsOff::KeyApp) => needed("WORDS_NEEDED_RQ_01_KEY_APP_SHOP"),
            Self::Off(RequestsOff::Unreadable(e)) | Self::Store(e) => say::book_unreadable(e),
            Self::NoContext => needed("WORDS_NEEDED_RQ_35_SETTINGS_NOT_GIVEN"),
            Self::Make(e) => say::make_refused(e),
            Self::Share(e) => say::share_refused(e),
            Self::End(e) => say::end_refused(e),
            Self::Repeat(e) => say::repeat_refused(e),
            Self::Return(e) => say::return_refused(e),
            Self::Address(e) => say::address_refused(e),
            Self::Queue(_) => needed("WORDS_NEEDED_RQ_29_RETURN_NOT_QUEUED"),
        }
    }

    /// Whether the refusal is the store's (a file), not a rule's.
    pub fn is_store(&self) -> bool {
        matches!(
            self,
            Self::Store(_) | Self::Off(RequestsOff::Unreadable(_))
        )
    }
}

/// An output paid to a request's address in a checked block that has not
/// reached the confirmation depth yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingArrival {
    /// The desk index of the address paid.
    pub index: u32,
    /// The transaction.
    pub txid: [u8; 32],
    /// Which output of it.
    pub vout: u32,
    /// The height of the block.
    pub height: u32,
    /// The block.
    pub block: [u8; 32],
    /// The block's time, in Unix seconds.
    pub block_time: u64,
    /// The satoshis, more than zero.
    pub sats: u64,
}

impl PendingArrival {
    fn key(&self) -> (u32, [u8; 32], u32) {
        (self.height, self.txid, self.vout)
    }
}

pub(crate) fn encode_arrivals(list: &[PendingArrival]) -> Vec<u8> {
    let mut out = wire::start(&MAGIC_REQUEST_ARRIVALS, list.len() * ARRIVAL_LEN + 4);
    out.extend_from_slice(&wire::len_u32(list.len()).to_le_bytes());
    for a in list {
        out.extend_from_slice(&a.index.to_le_bytes());
        out.extend_from_slice(&a.txid);
        out.extend_from_slice(&a.vout.to_le_bytes());
        out.extend_from_slice(&a.height.to_le_bytes());
        out.extend_from_slice(&a.block);
        out.extend_from_slice(&a.block_time.to_le_bytes());
        out.extend_from_slice(&a.sats.to_le_bytes());
    }
    out
}

pub(crate) fn decode_arrivals(bytes: &[u8]) -> Result<Vec<PendingArrival>, DecodeError> {
    let mut r = Reader::framed(bytes, &MAGIC_REQUEST_ARRIVALS)?;
    let n = r.count(MAX_ARRIVALS, ARRIVAL_LEN)?;
    let mut out: Vec<PendingArrival> = Vec::with_capacity(n);
    let mut seen: BTreeSet<([u8; 32], u32)> = BTreeSet::new();
    for _ in 0..n {
        let a = PendingArrival {
            index: r.u32_le()?,
            txid: r.array()?,
            vout: r.u32_le()?,
            height: r.u32_le()?,
            block: r.array()?,
            block_time: r.u64_le()?,
            sats: r.u64_le()?,
        };
        if a.sats == 0 {
            return Err(DecodeError::NonCanonical("an arrival of zero satoshis"));
        }
        if out.last().is_some_and(|last| last.key() >= a.key()) {
            return Err(DecodeError::NonCanonical(
                "the arrivals are not in the order of their blocks",
            ));
        }
        if !seen.insert((a.txid, a.vout)) {
            return Err(DecodeError::NonCanonical("an output is kept twice"));
        }
        out.push(a);
    }
    r.finish()?;
    Ok(out)
}

fn corrupt(e: DecodeError) -> BookError {
    BookError::Corrupt(match e {
        DecodeError::NonCanonical(why) => why,
        DecodeError::TooLarge => "the file of arrivals holds more than it may",
        _ => "the file of arrivals is cut short or not framed as it is written",
    })
}

/// The book and the arrivals that wait, as the node holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Loaded {
    pub(crate) book: RequestBook,
    pub(crate) pending: Vec<PendingArrival>,
}

/// What the node keeps of the requests.
pub(crate) struct Requests {
    pub(crate) state: Result<Loaded, RequestsOff>,
    pub(crate) context: Option<RequestContext>,
    pub(crate) rate: Option<CheckedRate>,
}

impl Requests {
    /// RQ-01, RQ-30, RQ-31: read the book and the arrivals of the folder.
    /// A shop with a Key app has none and reads nothing.
    pub(crate) fn open(dir: &Path, own_wallet: bool) -> Self {
        let state = if own_wallet {
            Self::read(dir).map_err(RequestsOff::Unreadable)
        } else {
            Err(RequestsOff::KeyApp)
        };
        Self {
            state,
            context: None,
            rate: None,
        }
    }

    fn read(dir: &Path) -> Result<Loaded, BookError> {
        let book = RequestBook::open(dir)?;
        let pending = match openbmt_requests::fsio::read_file(dir, REQUEST_ARRIVALS_FILE)? {
            Some(bytes) => decode_arrivals(&bytes).map_err(corrupt)?,
            None => Vec::new(),
        };
        Ok(Loaded { book, pending })
    }

    /// RQ-04: the desk index of every request, which the node keeps watching
    /// for good.
    pub(crate) fn indexes(&self) -> BTreeSet<u32> {
        match &self.state {
            Ok(loaded) => loaded.book.address_indexes().into_iter().collect(),
            Err(_) => BTreeSet::new(),
        }
    }
}

/// A request that was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MadeRequest {
    /// Its number (RQ-05).
    pub number: u64,
    /// The desk index of its address (RQ-03).
    pub index: u32,
    /// Its address, on the node's network.
    pub address: String,
}

/// A quote and what is shared with it (RQ-12, RQ-13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shared {
    /// The quote written to the request.
    pub quote: Quote,
    /// The BIP 321 link: the address, the amount and the shop's name.
    pub link: String,
    /// The request's address, on the node's network.
    pub address: String,
}

/// A return that was queued (RQ-26 to RQ-29).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnAdded {
    /// The return's id in the book.
    pub id: u64,
    /// The refund's id on the owner's queue.
    pub refund: u32,
    /// The satoshis.
    pub sats: u64,
    /// The address, in groups of four, for the owner to compare (RQ-28).
    pub groups: String,
}

impl<P: OwnerPort> OwnerNode<P> {
    // What the node knows.

    /// The request book, or why requests are off (RQ-01, RQ-31).
    pub fn requests(&self) -> Result<&RequestBook, &RequestsOff> {
        self.requests.state.as_ref().map(|l| &l.book)
    }

    /// The arrivals that wait for their block to reach the confirmation
    /// depth.
    pub fn request_arrivals(&self) -> &[PendingArrival] {
        match &self.requests.state {
            Ok(loaded) => &loaded.pending,
            Err(_) => &[],
        }
    }

    /// The shop's context the app gave last, if it gave one.
    pub fn request_context(&self) -> Option<RequestContext> {
        self.requests.context
    }

    /// The desk index of every request, for the allocation of the next
    /// (RQ-03).
    pub fn request_indexes(&self) -> BTreeSet<u32> {
        self.requests.indexes()
    }

    /// The app gives the shop's settings, its time zone and the newest rate
    /// it has checked, if any. Payments are not credited before the first
    /// call, because the due day and the lateness are the shop's (RQ-17).
    pub fn set_request_context(&mut self, context: RequestContext, rate: Option<CheckedRate>) {
        self.requests.context = Some(context);
        if rate.is_some() {
            self.requests.rate = rate;
        }
    }

    fn address_of(&self, script: &ScriptBuf) -> String {
        Address::from_script(script, self.network()).map_or_else(|_| String::new(), |a| a.to_string())
    }

    fn desk_script(&self, index: u32) -> Option<ScriptBuf> {
        self.wallet.receive_script(index).ok()
    }

    fn request_ready(&self) -> Result<(&Loaded, RequestContext), RequestError> {
        let loaded = self
            .requests
            .state
            .as_ref()
            .map_err(|e| RequestError::Off(e.clone()))?;
        let context = self.requests.context.ok_or(RequestError::NoContext)?;
        Ok((loaded, context))
    }

    /// Make a change on a copy of the book, store the copy whole, and only
    /// then take it into memory (RQ-03, RQ-12, RQ-30). `change` may also
    /// change the arrivals. Nothing changes on a refusal or a store that
    /// fails.
    fn edit<T>(
        &mut self,
        change: impl FnOnce(&mut RequestBook, &mut Vec<PendingArrival>) -> Result<T, RequestError>,
    ) -> Result<T, RequestError> {
        let loaded = self
            .requests
            .state
            .as_ref()
            .map_err(|e| RequestError::Off(e.clone()))?;
        let mut next = loaded.clone();
        let done = change(&mut next.book, &mut next.pending)?;
        let arrivals_changed = loaded.pending != next.pending;
        self.store_requests(&next, arrivals_changed)
            .map_err(RequestError::Store)?;
        self.requests.state = Ok(next);
        Ok(done)
    }

    /// Store the book, and the arrivals when they changed.
    fn store_requests(&self, next: &Loaded, arrivals_changed: bool) -> Result<(), BookError> {
        next.book.save(&self.dir)?;
        if arrivals_changed {
            write_file(
                &self.dir,
                REQUEST_ARRIVALS_FILE,
                &encode_arrivals(&next.pending),
            )
            .map_err(BookError::Io)?;
        }
        Ok(())
    }

    /// RQ-31: a change that must reach memory failed to store: requests are
    /// off until the node starts again and reads the file as it is.
    fn requests_off(&mut self, why: BookError) {
        self.note(format!("{why}; requests are off"));
        self.requests.state = Err(RequestsOff::Unreadable(why));
    }

    // Making and sharing.

    /// RQ-03, RQ-05 to RQ-11: make a request with the desk's next index
    /// `index` (taken with [`OwnNode::desk_index`](crate::OwnNode::desk_index)).
    /// The book is stored before the address is given.
    pub fn make_request(
        &mut self,
        new: &NewRequest,
        index: u32,
        now: u64,
    ) -> Result<MadeRequest, RequestError> {
        let (_, context) = self.request_ready()?;
        let rate = self.requests.rate.clone();
        let number = self.edit(|book, _| {
            book.make(new, index, now, &context.shop(), rate.as_ref())
                .map_err(RequestError::Make)
        })?;
        let script = self
            .desk_script(index)
            .ok_or(RequestError::Make(MakeRefused::IndexUsed))?;
        Ok(MadeRequest {
            number,
            index,
            address: self.address_of(&script),
        })
    }

    /// RQ-25: make the requests of the repeats that are due on `today`, one
    /// for each of `indexes`, taken for them in order. The book is stored
    /// once, after all of them.
    pub fn make_repeats(
        &mut self,
        due: &[DueRepeat],
        indexes: &[u32],
        now: u64,
    ) -> Result<Vec<MadeRequest>, RequestError> {
        let (_, context) = self.request_ready()?;
        let tz = context.tz_offset_secs;
        let made = self.edit(|book, _| {
            let mut made = Vec::new();
            for (d, index) in due.iter().zip(indexes) {
                let number = book
                    .make_repeat(d, *index, now, tz)
                    .map_err(RequestError::Repeat)?;
                made.push((number, *index));
            }
            Ok(made)
        })?;
        Ok(made
            .into_iter()
            .map(|(number, index)| MadeRequest {
                number,
                index,
                address: self
                    .desk_script(index)
                    .map_or_else(String::new, |s| self.address_of(&s)),
            })
            .collect())
    }

    /// RQ-25: the repeats due on `today`, oldest first, at most 12.
    pub fn repeats_due(&self, today: Day) -> Result<Vec<DueRepeat>, RequestError> {
        let (loaded, _) = self.request_ready()?;
        Ok(loaded.book.repeats_due(today))
    }

    /// RQ-12, RQ-13: make a quote of what is still owed, write it to the
    /// request (stored before anything is shared) and give the BIP 321 link.
    pub fn share_request(
        &mut self,
        number: u64,
        now: u64,
        shop_name: &str,
    ) -> Result<Shared, RequestError> {
        self.request_ready()?;
        let rate = self.requests.rate.clone();
        let (quote, index) = self.edit(|book, _| {
            let quote = book
                .share(number, now, rate.as_ref())
                .map_err(RequestError::Share)?;
            let index = book
                .request(number)
                .map_or(0, |request| request.address_index);
            Ok((quote, index))
        })?;
        let address = self
            .desk_script(index)
            .map_or_else(String::new, |s| self.address_of(&s));
        let link = share_link(&address, quote.sats, shop_name);
        Ok(Shared {
            quote,
            link,
            address,
        })
    }

    // Ending.

    /// RQ-19: cancel a request that has received nothing.
    pub fn cancel_request(&mut self, number: u64, now: u64) -> Result<(), RequestError> {
        self.request_ready()?;
        self.edit(|book, _| book.cancel(number, now).map_err(RequestError::End))
    }

    /// RQ-20: close a part paid request as it stands.
    pub fn close_request(&mut self, number: u64, now: u64) -> Result<(), RequestError> {
        let (_, context) = self.request_ready()?;
        self.edit(|book, _| {
            book.close(number, now, &context.shop())
                .map_err(RequestError::End)
        })
    }

    /// RQ-16: count an almost paid request as paid. It cannot be undone.
    pub fn count_request_as_paid(&mut self, number: u64, now: u64) -> Result<(), RequestError> {
        let (_, context) = self.request_ready()?;
        self.edit(|book, _| {
            book.count_as_paid(number, now, &context.shop())
                .map_err(RequestError::End)
        })
    }

    /// RQ-25: stop the series a request belongs to.
    pub fn stop_request_repeating(&mut self, number: u64) -> Result<(), RequestError> {
        self.request_ready()?;
        self.edit(|book, _| book.stop_repeating(number).map_err(RequestError::End))
    }

    // Returns.

    /// RQ-28: whether `script` is an address of the shop's own wallet: a
    /// script the live watch holds, one set aside from it, or one of a
    /// former wallet.
    fn is_own_script(&self, script: &bitcoin::Script) -> bool {
        let owned = ScriptBuf::from(script);
        if self.scripts.contains_key(&owned) || self.former.iter().any(|f| f.has_script(&owned)) {
            return true;
        }
        (0..self.view.receive_watch()).any(|i| {
            self.view.released().contains(i)
                && self
                    .wallet
                    .receive_script(i)
                    .is_ok_and(|made| made == owned)
        })
    }

    /// RQ-28, RQ-33: the script behind a pasted or scanned return address,
    /// or why it was refused, and the address in groups of four.
    pub fn check_return(&self, text: &str) -> Result<(ScriptBuf, String), RequestError> {
        let script = check_return_address(text, |s| self.is_own_script(s))
            .map_err(RequestError::Address)?;
        let groups = openbmt_requests::address_in_groups(&script).unwrap_or_default();
        Ok((script, groups))
    }

    /// RQ-26 to RQ-29: queue a return of `amount` from request `number` to
    /// the address the customer sent. The return is written to the book, and
    /// its refund is put on the owner's queue; nothing is paid. If the queue
    /// does not take the refund the book is put back as it was.
    pub fn add_return(
        &mut self,
        number: u64,
        amount: ReturnAmount,
        text: &str,
        now: u64,
    ) -> Result<ReturnAdded, RequestError> {
        self.request_ready()?;
        let (script, groups) = self.check_return(text)?;
        let rate = self.requests.rate.clone();
        let before = self.requests.state.clone();
        let (id, sats, index, received) = self.edit(|book, _| {
            let id = book
                .queue_return(number, amount, script.clone(), now, rate.as_ref())
                .map_err(RequestError::Return)?;
            let sats = book
                .returns()
                .iter()
                .find(|r| r.id == id)
                .map_or(0, |r| r.sats);
            let request = book.request(number);
            Ok((
                id,
                sats,
                request.map_or(0, |r| r.address_index),
                request.map_or(0, |r| r.received_sats()),
            ))
        })?;
        match self.queue_return_refund(number, id, index, received, script, sats) {
            Ok(refund) => Ok(ReturnAdded {
                id,
                refund,
                sats,
                groups,
            }),
            Err(why) => {
                // The book goes back to what it was.
                if let Ok(old) = &before {
                    match self.store_requests(old, true) {
                        Ok(()) => self.requests.state = before,
                        Err(e) => self.requests_off(e),
                    }
                }
                Err(why)
            }
        }
    }

    /// RQ-26: the facts of the desk's sale of a request, if `sale` is one:
    /// what it received, which the book credited at the confirmation depth.
    /// Nothing for a sale of a counter.
    pub(crate) fn desk_facts(&self, sale: &SaleKey) -> Option<SaleFacts> {
        if sale.counter != self.owner.owner_id() {
            return None;
        }
        let loaded = self.requests.state.as_ref().ok()?;
        let received = loaded
            .book
            .requests()
            .iter()
            .find(|r| r.address_index == sale.index)
            .map_or(0, |r| r.received_sats());
        Some(SaleFacts {
            received_sats: received,
            reached_refund_depth: received > 0,
        })
    }

    /// The desk's sale of a request.
    pub(crate) fn desk_sale(&self, index: u32) -> SaleKey {
        SaleKey {
            counter: self.owner.owner_id(),
            index,
        }
    }

    fn queue_return_refund(
        &mut self,
        number: u64,
        id: u64,
        index: u32,
        received: u64,
        script: ScriptBuf,
        sats: u64,
    ) -> Result<u32, RequestError> {
        let queue_error = |e: String| RequestError::Queue(e);
        self.ready_for_queue_write()
            .map_err(|e| queue_error(e.to_string()))?;
        let sale = self.desk_sale(index);
        let write = self.queue_writer();
        let owner = self
            .owner_mut()
            .map_err(|e| queue_error(e.to_string()))?;
        owner
            .set_sale_facts(
                sale,
                SaleFacts {
                    received_sats: received,
                    reached_refund_depth: received > 0,
                },
                &write,
            )
            .map_err(|e| queue_error(e.to_string()))?;
        owner
            .queue_refund(
                sale,
                &return_label(number, id),
                script,
                sats,
                None,
                &write,
            )
            .map_err(|e| queue_error(e.to_string()))
    }

    // What follows the chain.

    /// RQ-04: the receive indexes the live watch never sets aside.
    pub(crate) fn desk_kept(&self) -> BTreeSet<u32> {
        self.requests.indexes()
    }

    /// RQ-15, RQ-10: a block the coin view applied. Every output to a
    /// request's address is kept as an arrival with the block's time, to be
    /// credited at the confirmation depth. Called from the place the block
    /// is applied to the view, with the bytes the block came as.
    pub(crate) fn requests_arrivals(&mut self, hash: &BlockHash, bytes: &[u8]) {
        let Ok(loaded) = &self.requests.state else {
            return;
        };
        let by_index: BTreeSet<u32> = loaded.book.address_indexes().into_iter().collect();
        if by_index.is_empty() {
            return;
        }
        let Some(height) = self.watcher.chain().height_of(hash) else {
            return;
        };
        let Ok(block) = bitcoin::consensus::deserialize::<bitcoin::Block>(bytes) else {
            return;
        };
        let time = u64::from(block.header.time);
        let mut found: Vec<PendingArrival> = Vec::new();
        for tx in block.txdata.iter().skip(1) {
            let txid = tx.compute_txid();
            for (vout, out) in tx.output.iter().enumerate() {
                let sats = out.value.to_sat();
                let Some((Keychain::Receive, index)) = self.scripts.get(&out.script_pubkey) else {
                    continue;
                };
                let (Some(vout), true) = (u32::try_from(vout).ok(), by_index.contains(index)) else {
                    continue;
                };
                if sats == 0 {
                    continue;
                }
                found.push(PendingArrival {
                    index: *index,
                    txid: txid.to_byte_array(),
                    vout,
                    height,
                    block: hash.to_byte_array(),
                    block_time: time,
                    sats,
                });
            }
        }
        if found.is_empty() {
            return;
        }
        let stored = self.edit(|_book, pending| {
            for a in found {
                if pending.len() >= MAX_ARRIVALS {
                    break;
                }
                if !pending
                    .iter()
                    .any(|p| p.txid == a.txid && p.vout == a.vout)
                {
                    pending.push(a);
                }
            }
            pending.sort_by_key(PendingArrival::key);
            Ok(())
        });
        if let Err(e) = stored {
            self.note(format!("an arrival for a request could not be stored: {e}"));
        }
    }

    /// RQ-37, OC-53: the checked chain dropped every block above `fork`. The
    /// arrivals of those blocks are dropped and each payment recorded from
    /// them is taken back, the request's state is worked out again, the
    /// queued returns that no longer fit are taken off the queue, and the
    /// book is stored again. Called from the place the coin view undoes the
    /// same blocks, while the node runs and at its start.
    pub(crate) fn requests_reorg(&mut self, fork: u32) {
        let Ok(loaded) = &self.requests.state else {
            return;
        };
        let mut next = loaded.clone();
        next.pending.retain(|a| a.height <= fork);
        let taken = next.book.take_back_above(fork);
        if next.book == loaded.book && next.pending == loaded.pending {
            return;
        }
        let arrivals_changed = next.pending != loaded.pending;
        if let Err(e) = self.store_requests(&next, arrivals_changed) {
            // Memory and file would differ: the file is what a start reads,
            // and it is read again at a start (OC-53), so requests go off.
            return self.requests_off(e);
        }
        self.requests.state = Ok(next);
        self.note(format!(
            "{} payment(s) to requests were taken back: their blocks left the chain",
            taken.credits.len()
        ));
        for gone in &taken.removed_returns {
            self.cancel_return_refund(gone.request, gone.id);
        }
        for (number, over) in &taken.over_returned {
            self.note(format!(
                "request No. {number} shows {over} sats returned more than it now received"
            ));
        }
    }

    /// RQ-37: a queued return was taken off the queue: its refund goes too,
    /// if it is still waiting on the owner's queue.
    fn cancel_return_refund(&mut self, request: u64, id: u64) {
        let label = return_label(request, id);
        let Some(refund) = self
            .owner
            .refunds()
            .iter()
            .find(|r| r.cashier == label && matches!(r.stage, Stage::Queued | Stage::Returned { .. }))
            .map(|r| r.id)
        else {
            return;
        };
        if self.ready_for_queue_write().is_err() {
            self.note(format!("the refund of {label} waits to be taken off the queue"));
            return;
        }
        let write = self.queue_writer();
        if let Err(e) = self.owner.cancel(refund, &write) {
            self.note(format!("the refund of {label} was not taken off the queue: {e}"));
        }
    }

    /// RQ-10, RQ-15: credit the arrivals whose block has the confirmation
    /// depth, and follow the refunds of the returns. Called after the
    /// events of a pump are applied, so the chain is the node's newest.
    pub(crate) fn requests_tick(&mut self) {
        let (Ok(loaded), Some(context)) = (&self.requests.state, self.requests.context) else {
            return;
        };
        if !loaded.pending.is_empty() && self.view.ready() {
            self.credit_arrivals(context);
        }
        self.follow_returns();
    }

    fn credit_arrivals(&mut self, context: RequestContext) {
        let Ok(loaded) = &self.requests.state else {
            return;
        };
        let tip = self.watcher.chain().tip_height();
        let depth = context.depth();
        let on_chain = |a: &PendingArrival| {
            self.watcher.chain().hash_at(a.height) == Some(BlockHash::from_byte_array(a.block))
        };
        // Arrivals whose block is no longer on the chain are dropped: the
        // reorg that took the block already did the same (RQ-37).
        let (due, rest): (Vec<PendingArrival>, Vec<PendingArrival>) = loaded
            .pending
            .iter()
            .copied()
            .filter(|a| a.height > tip || on_chain(a))
            .partition(|a| a.height <= tip && tip - a.height + 1 >= depth);
        let dropped = loaded.pending.len() - due.len() - rest.len();
        if due.is_empty() && dropped == 0 {
            return;
        }
        let by_index: BTreeMap<u32, u64> = loaded
            .book
            .requests()
            .iter()
            .map(|r| (r.address_index, r.number))
            .collect();
        let mut by_request: BTreeMap<u64, Vec<Arrival>> = BTreeMap::new();
        for a in &due {
            if let Some(number) = by_index.get(&a.index) {
                by_request.entry(*number).or_default().push(Arrival {
                    txid: a.txid,
                    vout: a.vout,
                    height: a.height,
                    block_time: a.block_time,
                    sats: a.sats,
                });
            }
        }
        let now = self.watcher.now();
        let rate = self.requests.rate.clone();
        let shop = context.shop();
        let mut next = loaded.clone();
        let mut waiting: Vec<PendingArrival> = rest;
        for (number, arrivals) in &by_request {
            match next
                .book
                .credit(*number, arrivals, now, &shop, rate.as_ref())
            {
                Ok(_) => {}
                Err(CreditRefused::NoRate | CreditRefused::TooManyPayments) => {
                    // Kept, to be tried again with a rate; the arrival is not
                    // lost.
                    waiting.extend(
                        due.iter()
                            .filter(|a| by_index.get(&a.index) == Some(number))
                            .copied(),
                    );
                }
                Err(_) => {}
            }
        }
        waiting.sort_by_key(PendingArrival::key);
        next.pending = waiting;
        if next == *loaded {
            return;
        }
        let arrivals_changed = next.pending != loaded.pending;
        match self.store_requests(&next, arrivals_changed) {
            Ok(()) => self.requests.state = Ok(next),
            Err(e) => self.note(format!("payments to requests could not be stored: {e}")),
        }
    }

    /// RQ-29: the returns follow their refunds on the owner's queue. A
    /// refund in a transaction that was sent makes its return Sent; one that
    /// is paid makes it done; one that goes back to the queue (the spend was
    /// replaced) puts the return back (OC-03); one that was archived after it
    /// was paid is done.
    fn follow_returns(&mut self) {
        let stages: BTreeMap<String, Stage> = self
            .owner
            .refunds()
            .iter()
            .map(|f| (f.cashier.clone(), f.stage.clone()))
            .collect();
        self.follow_returns_from(&stages);
    }

    /// [`follow_returns`](Self::follow_returns) with the stage of each refund
    /// by its label.
    pub(crate) fn follow_returns_from(&mut self, stages: &BTreeMap<String, Stage>) {
        use openbmt_requests::ReturnState;
        let Ok(loaded) = &self.requests.state else {
            return;
        };
        let now = self.watcher.now();
        let mut steps: Vec<ReturnStep> = Vec::new();
        for r in loaded.book.returns() {
            let stage = stages.get(&return_label(r.request, r.id));
            let step = match (r.state, stage) {
                (ReturnState::Done { .. }, _) => None,
                (
                    ReturnState::Queued,
                    Some(Stage::Signed { txid } | Stage::Sent { txid } | Stage::Paid { txid }),
                ) => Some(ReturnStep::Sent(r.id, txid.to_byte_array())),
                (ReturnState::Sent { txid }, Some(Stage::Paid { txid: paid }))
                    if txid == paid.to_byte_array() =>
                {
                    Some(ReturnStep::Done(txid))
                }
                (
                    ReturnState::Sent { txid },
                    Some(Stage::Signed { txid: in_spend } | Stage::Sent { txid: in_spend }),
                ) if txid != in_spend.to_byte_array() => Some(ReturnStep::Replaced(txid)),
                (
                    ReturnState::Sent { txid },
                    Some(Stage::Queued | Stage::InSession { .. } | Stage::Returned { .. }),
                ) => Some(ReturnStep::Replaced(txid)),
                (ReturnState::Sent { txid }, None) => Some(ReturnStep::Done(txid)),
                _ => None,
            };
            steps.extend(step);
        }
        if steps.is_empty() {
            return;
        }
        let result = self.edit(|book, _| {
            for step in &steps {
                match step {
                    ReturnStep::Sent(id, txid) => {
                        book.mark_returns_sent(&[*id], *txid);
                    }
                    ReturnStep::Done(txid) => {
                        book.mark_returns_done(*txid, now);
                    }
                    ReturnStep::Replaced(txid) => {
                        book.requeue_returns(*txid);
                    }
                }
            }
            Ok(())
        });
        if let Err(e) = result {
            self.note(format!("the returns of requests could not be followed: {e}"));
        }
    }
}

/// One change to a return that its refund's stage calls for.
enum ReturnStep {
    Sent(u64, [u8; 32]),
    Done([u8; 32]),
    Replaced([u8; 32]),
}

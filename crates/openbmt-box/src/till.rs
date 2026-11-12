//! What the Box keeps for sales a till asks for (BX-70 to BX-79): the till
//! key, the sales in memory, and the way a counter's signed notices move
//! them.
//!
//! Nothing here touches a socket or the clock: the time is passed in. The
//! listener is [`crate::till_listener`]; the runner joins the two to the
//! Box's state and its courier.
//!
//! A till sale is a request the Box passes to one counter. The Box never
//! decides that a sale is paid: it takes the word of a counter-signed notice
//! that names the sale (BX-78), and the till closes its own order when it
//! reads `paid` (BX-79).

use std::collections::BTreeMap;

use openbmt_core::identity::{tagged_hash, DeviceId};
use openbmt_core::settings::DeviceRole;
use openbmt_counter::Currency;
use openbmt_link::os_random;
use openbmt_pos::{PaidNotice, SaleCancel, SaleRequest, TillMessage, MAX_TILL_REFERENCE};

use crate::files::BoxFiles;
use crate::state::BoxError;

/// The file that holds the hash of the till key (BX-71).
pub const TILL_KEY_FILE: &str = "till-key";
/// The tag of the hash the till key is stored under.
pub const TAG_TILL_KEY: &str = "OpenBMT/till-key/v1";
/// Seconds a request waits for the cashier to press Start (BX-76).
pub const REQUEST_SECS: u64 = 300;
/// Seconds an ended sale is kept before it is dropped (BX-76).
pub const KEEP_SECS: u64 = 3_600;
/// Seconds an opened sale is kept open before it counts as unpaid (BX-78).
pub const OPEN_SECS: u64 = 3_600;
/// Most sales waiting for a counter at once (BX-76).
pub const MAX_WAITING: usize = 32;
/// Most sales kept at all, whatever state they are in.
pub const MAX_KEPT: usize = 4_096;
/// Most messages waiting to be sent to counters.
pub const MAX_OUTBOX: usize = 256;
/// Most characters of a till's own request id (BX-72).
pub const MAX_REQUEST_ID: usize = 64;

/// What a till sale is now (BX-73).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaleState {
    /// Asked of the counter; nobody has pressed Start.
    Waiting,
    /// The counter opened it; the customer can pay.
    Opened,
    /// A counter-signed notice said it is paid.
    Paid,
    /// The counter's sale ended without being paid.
    Unpaid,
    /// Nobody pressed Start in time.
    Expired,
    /// The till cancelled it.
    Cancelled,
}

impl SaleState {
    /// The word the till reads.
    pub fn word(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Opened => "opened",
            Self::Paid => "paid",
            Self::Unpaid => "unpaid",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether the sale has ended and will be dropped after [`KEEP_SECS`].
    pub fn ended(self) -> bool {
        !matches!(self, Self::Waiting | Self::Opened)
    }
}

/// Why the Box refused the till (BX-72 to BX-75).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TillRefusal {
    /// 401: no till key, or not the Box's.
    KeyWrong,
    /// 503: the Box has not joined the shop.
    NotPaired,
    /// 404: no counter has this code; the code.
    UnknownCounter(String),
    /// 409: the sale is in another money than the shop's.
    WrongMoney {
        /// The shop's currency.
        shop: String,
        /// The currency the till asked in.
        asked: String,
    },
    /// 400: the amount cannot be taken.
    BadAmount,
    /// 429: too many sales are waiting.
    Busy,
    /// 404: the Box does not know the sale.
    Lost,
    /// 400: the request does not read. No words: the till's own fault.
    BadRequest,
    /// 500: the Box could not make an id for the sale. No words.
    Fault,
}

impl TillRefusal {
    /// The status code of the answer.
    pub fn status(&self) -> u16 {
        match self {
            Self::KeyWrong => 401,
            Self::NotPaired => 503,
            Self::UnknownCounter(_) | Self::Lost => 404,
            Self::WrongMoney { .. } => 409,
            Self::BadAmount | Self::BadRequest => 400,
            Self::Busy => 429,
            Self::Fault => 500,
        }
    }
}

// ---- the till key ----------------------------------------------------

/// What the Box stores of a till key: the tagged hash of its 32 bytes.
pub fn till_key_hash(key: &[u8; 32]) -> [u8; 32] {
    tagged_hash(TAG_TILL_KEY, key)
}

/// A key as 64 lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Lowercase hex of exactly `2 * N` characters as `N` bytes.
pub fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let bytes = text.as_bytes();
    if bytes.len() != N * 2 {
        return None;
    }
    let nibble = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    let mut out = [0u8; N];
    let (pairs, _) = bytes.as_chunks::<2>();
    for (to, [hi, lo]) in out.iter_mut().zip(pairs) {
        *to = (nibble(*hi)? << 4) | nibble(*lo)?;
    }
    Some(out)
}

/// Whether two byte strings are equal, taking the same time for any two of
/// the same length.
fn same_bytes(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// BX-71: whether `presented` (64 lowercase hex) is the key whose hash is
/// `stored`. The comparison is of the hashes, in constant time.
pub fn till_key_matches(stored: &[u8], presented: &str) -> bool {
    match unhex::<32>(presented) {
        Some(key) => {
            let hash = till_key_hash(&key);
            same_bytes(stored, &hash)
        }
        None => false,
    }
}

/// BX-71: make a till key from the system's randomness and store its hash in
/// `files`, replacing any earlier one. Returns the key as 64 lowercase hex,
/// which is the only place it ever exists.
pub fn make_till_key(files: &mut impl BoxFiles) -> Result<String, BoxError> {
    let mut key = [0u8; 32];
    os_random(&mut key).map_err(|_| BoxError::NoRandomness)?;
    files
        .write(TILL_KEY_FILE, &till_key_hash(&key))
        .map_err(|e| BoxError::Io {
            file: TILL_KEY_FILE,
            op: "write",
            kind: e.kind(),
        })?;
    Ok(hex(&key))
}

// ---- what the till sends ----------------------------------------------

/// A decimal string as minor units (BX-72): digits, then if there is a
/// point at least one digit and at most `digits` of them. No sign, no
/// exponent, no space. `None` for anything else and for zero.
pub fn parse_amount(text: &str, digits: u32) -> Option<u64> {
    let (whole, frac) = match text.split_once('.') {
        Some((w, f)) => (w, f),
        None => (text, ""),
    };
    let has_point = text.contains('.');
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if has_point && frac.is_empty() {
        return None;
    }
    if !frac.bytes().all(|b| b.is_ascii_digit()) || frac.len() > usize::try_from(digits).ok()? {
        return None;
    }
    let unit = 10u64.checked_pow(digits)?;
    let whole: u64 = whole.parse().ok()?;
    let mut minor: u64 = if frac.is_empty() {
        0
    } else {
        frac.parse().ok()?
    };
    let missing = usize::try_from(digits).ok()?.checked_sub(frac.len())?;
    for _ in 0..missing {
        minor = minor.checked_mul(10)?;
    }
    let total = whole.checked_mul(unit)?.checked_add(minor)?;
    (total > 0).then_some(total)
}

/// BX-72: the reference as the counter gets it: printable ASCII only, cut
/// to [`MAX_TILL_REFERENCE`] characters.
pub fn clean_reference(text: &str) -> String {
    text.chars()
        .filter(|c| (' '..='~').contains(c))
        .take(MAX_TILL_REFERENCE)
        .collect()
}

/// BX-72: whether `id` is a request id: 1 to 64 of `[A-Za-z0-9_-]`.
pub fn request_id_ok(id: &str) -> bool {
    (1..=MAX_REQUEST_ID).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The code a counter goes by in the owner app: the first 4 bytes of its
/// key as 8 lowercase hex (BX-72).
pub fn counter_code(counter: &DeviceId) -> String {
    hex(counter.to_bytes().get(..4).unwrap_or(&[]))
}

/// The one counter of `counters` whose code is `code`, if exactly one is
/// (BX-72).
pub fn find_counter(counters: impl IntoIterator<Item = DeviceId>, code: &str) -> Option<DeviceId> {
    let mut found = None;
    for counter in counters {
        if counter_code(&counter) == code {
            if found.is_some() {
                return None;
            }
            found = Some(counter);
        }
    }
    found
}

// ---- the sales ---------------------------------------------------------

#[derive(Debug, Clone)]
struct Sale {
    request: String,
    counter: DeviceId,
    amount: u64,
    currency: Currency,
    expires_at: u64,
    opened_at: Option<u64>,
    state: SaleState,
    ended_at: Option<u64>,
}

/// A message for a counter that the worker has still to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// Its place in the order the messages were made; what
    /// [`TillTable::sent`] takes off the list.
    pub seq: u64,
    /// The counter it is for.
    pub counter: DeviceId,
    /// What it says.
    pub message: TillMessage,
}

/// The till's sales, in memory only (BX-76). A restart forgets them.
#[derive(Debug, Default)]
pub struct TillTable {
    sales: BTreeMap<[u8; 16], Sale>,
    requests: BTreeMap<String, [u8; 16]>,
    outbox: Vec<Outgoing>,
    next_seq: u64,
    last: BTreeMap<DeviceId, u64>,
}

/// What a till asked for, after the checks that need the settings (BX-72).
#[derive(Debug, Clone)]
pub struct NewSale {
    /// The counter, found from its code.
    pub counter: DeviceId,
    /// The amount in minor units; above zero.
    pub amount: u64,
    /// The shop's currency.
    pub currency: Currency,
    /// The reference, cleaned.
    pub reference: String,
    /// The till's own id for the request.
    pub request: String,
}

/// Why a notice for a till sale was set aside; the note says it.
fn ignore(why: &str) -> String {
    format!("BX-78: a counter's notice for a till sale was ignored: {why}")
}

impl TillTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many sales are kept.
    pub fn len(&self) -> usize {
        self.sales.len()
    }

    /// Whether no sale is kept.
    pub fn is_empty(&self) -> bool {
        self.sales.is_empty()
    }

    /// How many sales wait for a counter.
    pub fn waiting(&self) -> usize {
        self.sales
            .values()
            .filter(|s| s.state == SaleState::Waiting)
            .count()
    }

    /// BX-76, BX-78: let time pass. A request nobody started in 5 minutes
    /// expires, an opened sale that nothing has come of in an hour counts as
    /// unpaid, and sales ended for an hour are dropped. Requests that can no
    /// longer be started are taken off the list of messages to send.
    pub fn sweep(&mut self, now: u64) {
        for sale in self.sales.values_mut() {
            match sale.state {
                SaleState::Waiting if now >= sale.expires_at => {
                    sale.state = SaleState::Expired;
                    sale.ended_at = Some(now);
                }
                SaleState::Opened
                    if sale
                        .opened_at
                        .is_some_and(|at| now >= at.saturating_add(OPEN_SECS)) =>
                {
                    sale.state = SaleState::Unpaid;
                    sale.ended_at = Some(now);
                }
                _ => {}
            }
        }
        let gone: Vec<[u8; 16]> = self
            .sales
            .iter()
            .filter(|(_, s)| {
                s.ended_at
                    .is_some_and(|at| now >= at.saturating_add(KEEP_SECS))
            })
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            if let Some(sale) = self.sales.remove(&id) {
                self.requests.remove(&sale.request);
            }
        }
        let sales = &self.sales;
        self.outbox.retain(|o| match &o.message {
            TillMessage::Request(r) => sales
                .get(&r.sale)
                .is_some_and(|s| s.state == SaleState::Waiting),
            TillMessage::Cancel(c) => sales.contains_key(&c.sale),
        });
    }

    /// BX-72, BX-76: take a till's request. The same request id again
    /// answers the same sale and sends nothing new. `id` is the id for a new
    /// sale.
    pub fn create(
        &mut self,
        now: u64,
        id: [u8; 16],
        new: NewSale,
    ) -> Result<([u8; 16], SaleState), TillRefusal> {
        self.sweep(now);
        if let Some(sale_id) = self.requests.get(&new.request) {
            if let Some(sale) = self.sales.get(sale_id) {
                return Ok((*sale_id, sale.state));
            }
        }
        if self.waiting() >= MAX_WAITING || self.sales.len() >= MAX_KEPT {
            return Err(TillRefusal::Busy);
        }
        if self.sales.contains_key(&id) || self.outbox.len() >= MAX_OUTBOX {
            return Err(TillRefusal::Busy);
        }
        let expires_at = now.saturating_add(REQUEST_SECS);
        let seq = self.take_seq();
        self.outbox.push(Outgoing {
            seq,
            counter: new.counter,
            message: TillMessage::Request(SaleRequest {
                sale: id,
                amount: openbmt_counter::Amount(new.amount),
                currency: new.currency,
                reference: new.reference,
                expires_at,
            }),
        });
        self.requests.insert(new.request.clone(), id);
        self.sales.insert(
            id,
            Sale {
                request: new.request,
                counter: new.counter,
                amount: new.amount,
                currency: new.currency,
                expires_at,
                opened_at: None,
                state: SaleState::Waiting,
                ended_at: None,
            },
        );
        Ok((id, SaleState::Waiting))
    }

    /// BX-73: the state of a sale, or `None` when the Box does not know it.
    pub fn state(&mut self, now: u64, id: &[u8; 16]) -> Option<SaleState> {
        self.sweep(now);
        self.sales.get(id).map(|s| s.state)
    }

    /// BX-74: cancel a sale unless it is paid, and tell its counter. A sale
    /// that is already cancelled is not told again. `None` when the Box does
    /// not know the sale.
    pub fn cancel(&mut self, now: u64, id: &[u8; 16]) -> Option<SaleState> {
        self.sweep(now);
        let sale = self.sales.get_mut(id)?;
        match sale.state {
            SaleState::Paid | SaleState::Cancelled => {}
            _ => {
                sale.state = SaleState::Cancelled;
                sale.ended_at = Some(now);
                if self.outbox.len() < MAX_OUTBOX {
                    let seq = self.next_seq;
                    self.next_seq = seq.wrapping_add(1);
                    self.outbox.push(Outgoing {
                        seq,
                        counter: sale.counter,
                        message: TillMessage::Cancel(SaleCancel { sale: *id }),
                    });
                }
            }
        }
        self.sales.get(id).map(|s| s.state)
    }

    /// BX-78: take a paid notice the counter sent. A notice with no till
    /// sale is not the till's and is passed over. Otherwise it must read,
    /// name the key that sent it, come from a counter in `roles`, carry a
    /// good signature and a number above the last till notice from that
    /// counter, and name a sale this Box asked that counter for, with the
    /// same amount and currency. A notice that fails any of that changes
    /// nothing and its number is not used. Returns whether the sale moved.
    pub fn take_notice(
        &mut self,
        now: u64,
        sender: &DeviceId,
        bytes: &[u8],
        roles: &BTreeMap<DeviceId, DeviceRole>,
    ) -> Result<bool, String> {
        let Ok(notice) = PaidNotice::from_bytes(bytes) else {
            // The POS side says whether a notice reads; the till has no
            // business with one that does not.
            return Ok(false);
        };
        let Some(sale_id) = notice.till_sale else {
            return Ok(false);
        };
        if notice.counter != *sender {
            return Err(ignore("it names another counter than the one that sent it"));
        }
        if roles.get(sender) != Some(&DeviceRole::Counter) {
            return Err(ignore("its key is not a counter in the settings in force"));
        }
        if !notice.verify() {
            return Err(ignore("its signature does not check"));
        }
        if notice.number <= self.last.get(sender).copied().unwrap_or(0) {
            return Err(ignore("its number is not above the last one taken"));
        }
        self.sweep(now);
        let Some(sale) = self.sales.get_mut(&sale_id) else {
            return Err(ignore("this Box holds no such sale"));
        };
        if sale.counter != notice.counter {
            return Err(ignore("this Box asked another counter for the sale"));
        }
        if sale.amount != notice.amount.0 || sale.currency != notice.currency {
            return Err(ignore("its amount or currency is not the sale's"));
        }
        self.last.insert(*sender, notice.number);
        Ok(sale.apply(notice.state, now))
    }

    /// The messages to send to counters, oldest first. They stay in the list
    /// until [`TillTable::sent`] says they went.
    pub fn outgoing(&self) -> Vec<Outgoing> {
        self.outbox.clone()
    }

    /// The messages with these numbers were sent, or can never be.
    pub fn sent(&mut self, seqs: &[u64]) {
        self.outbox.retain(|o| !seqs.contains(&o.seq));
    }

    fn take_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq = seq.wrapping_add(1);
        seq
    }
}

impl Sale {
    /// The counter's word on the sale. Returns whether the state moved.
    fn apply(&mut self, said: openbmt_pos::NoticeState, now: u64) -> bool {
        use openbmt_pos::NoticeState as N;
        let next = match (said, self.state) {
            // Paid is the counter's word and wins over any state but
            // itself, a late payment after the till gave up included.
            (N::Paid | N::Final, SaleState::Paid) => return false,
            (N::Paid | N::Final, _) => SaleState::Paid,
            (N::Opened, SaleState::Waiting) => SaleState::Opened,
            (N::Unpaid, SaleState::Waiting | SaleState::Opened) => SaleState::Unpaid,
            // Opened again after the sale ended, unpaid again after it was
            // paid, and anything repeated, change nothing the till was told.
            _ => return false,
        };
        self.state = next;
        match next {
            SaleState::Opened => self.opened_at = Some(now),
            _ => self.ended_at = Some(now),
        }
        true
    }
}

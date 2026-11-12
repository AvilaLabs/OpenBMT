//! Lightning at the counter through a swap provider: the driver that asks
//! the provider for an invoice for each sale and follows the swap until the
//! claim is in a block (CR-219 to CR-223).
//!
//! The driver has no network, no clock and no randomness. It hands out the
//! next request as the bytes of an HTTP/1.1 request for the provider's onion
//! name ([`SwapDriver::next`]) with a number, and takes the bytes that came
//! back for that number ([`SwapDriver::reply`], [`SwapDriver::failed`]); an
//! answer for any other number is ignored. The app sends the bytes through
//! its Tor, one request at a time (CR-225). What is asked and how each reply
//! is read and checked is `openbmt-swap` (SW-01 to SW-91); this file only
//! decides when, keeps what must survive a restart and tells the node what
//! the chain showed.
//!
//! ## What it asks, and when
//!
//! - For each open sale whose quote is within the shop's maximum, one swap
//!   for the quote's satoshis: the provider's terms first, if the ones held
//!   are older than SW-63 allows, then the request to make the swap with a
//!   fresh preimage and claim key (CR-219). A new quote asks again and the
//!   earlier swaps stay followed, at most [`MAX_SWAPS`] for a sale.
//! - If no reply that passed SW-20 to SW-28 has come [`SWAP_WINDOW_SECS`]
//!   seconds after the quote, the sale has no Lightning code and says so. A
//!   reply that comes later is dropped: nobody saw its invoice.
//! - Each swap is asked for its status as the machine of SW-71 says, until
//!   the claim is sent.
//!
//! ## What it keeps
//!
//! Each swap is a [`SwapRecord`] the node writes before the invoice is shown
//! and again at every change (CR-220): the machine of the swap library, the
//! lockup, the claims that were built, the fee of the last one. After a
//! restart the claim is built again at the same fee, which is the same
//! transaction, and sent again.
//!
//! ## What it asks of the chain
//!
//! The lockup address of every swap that has not ended is watched, by the
//! counter's own light client. A sighting of a peer and the proof of a block
//! feed one watcher of the lockup (`SaleWatch`), so the lockup is judged as an
//! on-chain payment is (WR-31 to WR-46), and a second watcher of the sale's
//! own address finds the claim in a block. What the provider says of the
//! lockup is used only to know when to look (SW-72).

use std::collections::BTreeMap;

use bitcoin::consensus::serialize;
use bitcoin::hashes::Hash;
use bitcoin::{Address, Network, OutPoint, ScriptBuf, Txid};
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::settings::SwapProvider;
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_lightning::bolt11::{self, Checked};
use openbmt_swap::follow::{Ctx, Failure, Next, ReverseSwap, Stage};
use openbmt_swap::{
    check, parse_response, Expect, LockupSeen, Pairs, Provider, Quote, Refused, Reply, Request,
    Secrets, Seen, Status, SwapState, MIN_CLAIM_BLOCKS,
};
use openbmt_watch::{HeaderChain, SaleStatus, SaleWatch, Source, WellFormedBlock};

/// The magic of the saved swap (version 1).
pub const MAGIC_SWAP_RECORD: [u8; 4] = *b"OBM(";
/// CR-219: seconds after the quote in which the invoice must have come.
pub const SWAP_WINDOW_SECS: u64 = 5;
/// CR-219: most swaps the driver makes for one sale.
pub const MAX_SWAPS: usize = 8;
/// CR-222: the first fee rate of a claim, in satoshis per 1000 virtual
/// bytes (2 sat/vB). **[to tune]**
pub const CLAIM_FEE_PER_KVB: u64 = 2_000;
/// CR-222: the least a fee rate goes up by when a claim is built again, in
/// satoshis per 1000 virtual bytes. **[to tune]**
pub const BUMP_MIN_PER_KVB: u64 = 2_000;
/// CR-225: seconds before a request that has no answer is let go.
pub const REQUEST_GIVE_UP_SECS: u64 = 60;
/// Seconds before a request that failed is made again, for the terms.
pub const RETRY_AFTER_SECS: u64 = 30;
/// The description sent with a request (SW-03). It names neither the shop
/// nor the sale.
pub const DESCRIPTION: &str = "Lightning payment";
/// The most claims a swap record keeps the ids of.
const MAX_CLAIMS: usize = 32;
/// The most bytes of a record.
const MAX_RECORD_BYTES: usize = 4096;
/// CR-219: the small line under the code of a sale that has no Lightning.
pub const NOT_FOR_THIS_SALE: &str = "Lightning is not available for this sale.";

/// One request for the app to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapRequest {
    /// The number the answer comes back with.
    pub id: u64,
    /// `host:port` of the provider's onion service: the port is 80.
    pub host_port: String,
    /// The bytes of the HTTP/1.1 request.
    pub bytes: Vec<u8>,
}

/// What the screen of a sale shows of the swap (CR-219, CR-221, CR-223).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaleSwap {
    /// No provider is in use, or the sale is not one the driver follows.
    Off,
    /// The swap has been asked for and has not come yet.
    Waiting,
    /// The invoice passed SW-20 to SW-28 and was saved: the code carries it.
    Invoice(Checked),
    /// There is none for this sale: the small line of CR-219.
    NotForThisSale,
    /// The provider could not be reached, or refused this amount.
    Unavailable(Words),
    /// The Lightning payment arrived and the sale waits for a block.
    Confirming(Words),
    /// The swap failed before the claim; the sale is unpaid.
    Failed(Words),
}

/// Why a quote has no swap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    /// The quote is above what the shop takes over Lightning (CR-218).
    OverShopLimit,
    /// The provider could not be reached.
    Down,
    /// The provider refused the amount, or the request.
    Refused,
    /// The window passed with nothing.
    Late,
}

/// What the driver tells the node to do after a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Write this record (CR-220). When the write is done the node calls
    /// [`SwapDriver::saved`] and sends `send` (the bytes of a claim) to the
    /// peers. A write that failed is asked for again by the next
    /// [`SwapDriver::flush`].
    Save {
        /// The sale.
        index: u32,
        /// The swap's number within the sale.
        n: u32,
        /// The record.
        bytes: Vec<u8>,
        /// A claim to send once the record is written.
        send: Option<Vec<u8>>,
    },
}

/// What the chain and the provider led to that the node must act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// The claim of this swap is built and saved: the sale is paid over
    /// Lightning (CR-221). `lockup_confirmed` says whether the lockup was in
    /// a block; if not, `status` is what the lockup's watch shows, for the
    /// decision of CR-221.
    Claimed {
        /// The sale.
        index: u32,
        /// The swap.
        n: u32,
        /// The payment hash of the invoice.
        payment_hash: [u8; 32],
        /// What the customer paid.
        invoice_sats: u64,
        /// What the claim pays the sale's address.
        received_sats: u64,
        /// The lockup is in a block.
        lockup_confirmed: bool,
        /// The lockup's watch, now.
        status: SaleStatus,
    },
    /// The lockup of a swap whose sale was paid before it was in a block is
    /// now in one (CR-15).
    LockupConfirmed {
        /// The sale.
        index: u32,
    },
    /// A sale that was paid before its lockup was in a block lost its
    /// lockup or its swap (CR-221).
    Cancel {
        /// The sale.
        index: u32,
    },
    /// The lockup of a swap of this sale is on the way or the claim is not
    /// yet in a block (`true`), or neither (`false`): the sale does not
    /// expire while it is.
    Pending {
        /// The sale.
        index: u32,
        /// Whether a swap of it is on the way.
        pending: bool,
    },
    /// A swap failed before its claim (CR-223).
    Failed {
        /// The sale.
        index: u32,
        /// The swap.
        n: u32,
        /// Why.
        failure: Failure,
    },
    /// The claim of this sale is still not in a block and the provider's
    /// deadline is near (CR-222).
    Stuck {
        /// The sale.
        index: u32,
    },
}

/// What the node keeps of one swap (CR-220).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapRecord {
    /// The machine of the swap library.
    pub machine: ReverseSwap,
    /// The lockup that passed SW-30, once there is one.
    pub lockup: Option<LockupSeen>,
    /// The ids of the claims that were built, the last one the current.
    pub claims: Vec<Txid>,
    /// The fee rate of the last claim, in satoshis per 1000 virtual bytes.
    pub fee_per_kvb: u64,
    /// The height of the chain when the last claim was built, 0 for none.
    pub claim_tip: u32,
}

impl SwapRecord {
    /// The bytes the node writes: the magic, version 1, the machine (SW-73)
    /// with its length, the lockup (flag, id, index, value, flags), the
    /// claims, the fee rate and the height.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_SWAP_RECORD, 1024);
        let machine = self.machine.encode();
        out.extend_from_slice(
            &u16::try_from(machine.len())
                .unwrap_or(u16::MAX)
                .to_le_bytes(),
        );
        out.extend_from_slice(&machine);
        match &self.lockup {
            Some(l) => {
                out.push(1);
                out.extend_from_slice(l.outpoint.txid.as_ref());
                out.extend_from_slice(&l.outpoint.vout.to_le_bytes());
                out.extend_from_slice(&l.value_sats.to_le_bytes());
                out.push(u8::from(l.confirmed) | (u8::from(l.signals_rbf) << 1));
            }
            None => out.push(0),
        }
        out.push(u8::try_from(self.claims.len()).unwrap_or(u8::MAX));
        for id in &self.claims {
            out.extend_from_slice(id.as_ref());
        }
        out.extend_from_slice(&self.fee_per_kvb.to_le_bytes());
        out.extend_from_slice(&self.claim_tip.to_le_bytes());
        out
    }

    /// Read the bytes strictly: the machine must read (SW-73), every length
    /// within its limit, the claims must agree with the machine (a swap that
    /// was claimed names its claim last, and one that was not has none), and
    /// the bytes written back are the bytes read.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let bad = |why: &'static str| DecodeError::NonCanonical(why);
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let mut r = Reader::framed(bytes, &MAGIC_SWAP_RECORD)?;
        let n = usize::from(r.u16_le()?);
        let machine =
            ReverseSwap::decode(r.take(n)?).map_err(|_| bad("the swap does not read back"))?;
        let lockup = if r.flag()? {
            let txid = Txid::from_byte_array(r.array()?);
            let vout = r.u32_le()?;
            let value_sats = r.u64_le()?;
            let flags = r.u8()?;
            if flags > 3 {
                return Err(bad("the lockup flags are not known"));
            }
            Some(LockupSeen {
                outpoint: OutPoint::new(txid, vout),
                value_sats,
                confirmed: flags & 1 != 0,
                signals_rbf: flags & 2 != 0,
            })
        } else {
            None
        };
        let count = usize::from(r.u8()?);
        if count > MAX_CLAIMS {
            return Err(DecodeError::TooLarge);
        }
        let mut claims = Vec::with_capacity(count);
        for _ in 0..count {
            claims.push(Txid::from_byte_array(r.array()?));
        }
        let fee_per_kvb = r.u64_le()?;
        let claim_tip = r.u32_le()?;
        r.finish()?;
        let claimed = matches!(machine.stage(), Stage::ClaimSent | Stage::Paid { .. });
        let current = match machine.state().phase() {
            openbmt_swap::Phase::Claimed(txid) => Some(txid),
            openbmt_swap::Phase::Waiting => None,
        };
        if claimed != (lockup.is_some() && !claims.is_empty()) || claims.last() != current.as_ref()
        {
            return Err(bad("the claims do not agree with the swap"));
        }
        let record = Self {
            machine,
            lockup,
            claims,
            fee_per_kvb,
            claim_tip,
        };
        if record.to_bytes() != bytes {
            return Err(bad("the record is not canonical"));
        }
        Ok(record)
    }
}

/// What a request that is out is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    Pairs,
    Create(u32, u32),
    Status(u32, u32),
}

#[derive(Debug, Clone)]
struct Out {
    id: u64,
    what: Asked,
    since: u64,
}

/// What a swap is at.
#[derive(Debug)]
enum Phase {
    /// The swap must be asked for.
    Want,
    /// The request for it is out.
    Creating { secrets: Secrets, sats: u64 },
    /// Followed.
    Have(Box<Follow>),
    /// There is none for this quote.
    None(Why),
}

#[derive(Debug)]
struct Swap {
    n: u32,
    sats: u64,
    /// When the quote was made: the window runs from here.
    quoted_at: u64,
    phase: Phase,
}

/// One swap that has a reply that passed the checks.
#[derive(Debug)]
struct Follow {
    record: SwapRecord,
    /// The record was written and the invoice may be shown.
    saved: bool,
    /// The record changed since it was written.
    dirty: bool,
    /// A claim to send once the record is written.
    pending_claim: Option<Vec<u8>>,
    /// The sale was told that it is paid.
    told: bool,
    /// The sale was told that the lockup is in a block.
    lockup_told: bool,
    stuck_told: bool,
    failure_told: bool,
    /// What the last claim pays the sale's address.
    claim_sats: u64,
    last_status: Option<u64>,
    poll_after: u32,
    /// The lockup, as the light client sees it.
    watch: SaleWatch,
    /// The sale's own address, for the claim.
    claim_watch: SaleWatch,
    lockup_script: ScriptBuf,
    sale_script: ScriptBuf,
    invoice: Checked,
}

#[derive(Debug)]
struct SaleSwaps {
    open: bool,
    /// The node was told whether a swap of the sale is on the way.
    pending_told: bool,
    address: Address,
    swaps: Vec<Swap>,
}

/// The driver. See the module docs.
pub struct SwapDriver {
    network: Network,
    provider: Option<Provider>,
    max_sale_sats: u64,
    pairs: Option<Pairs>,
    /// When the terms may be asked for again, after a failure.
    pairs_retry_at: u64,
    sales: BTreeMap<u32, SaleSwaps>,
    round: u64,
    out: Option<Out>,
    notes: Vec<String>,
}

impl std::fmt::Debug for SwapDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwapDriver")
            .field("on", &self.provider.is_some())
            .field("sales", &self.sales.len())
            .finish()
    }
}

fn claim_ctx(now: u64, tip: u32, accept_unconfirmed: bool) -> Ctx {
    Ctx {
        now_secs: now,
        tip_height: tip,
        accept_unconfirmed,
    }
}

fn checked_of(invoice: &str) -> Option<Checked> {
    let d = bolt11::decode(invoice).ok()?;
    Some(Checked {
        bolt11: invoice.to_string(),
        amount_msat: d.amount_msat?,
        payment_hash: d.payment_hash,
        timestamp: d.timestamp,
    })
}

impl SwapDriver {
    /// The driver for the signed settings' provider (none: off) on this
    /// network, with the records that were saved: each is a sale's index, the
    /// swap's number and the bytes. `sale_scripts` gives the script of a sale
    /// the counter holds. A record that does not read, or whose sale is not
    /// held, is left alone on the disk and noted; it is not followed.
    pub fn new(
        network: Network,
        setting: Option<&SwapProvider>,
        saved: &[(u32, u32, Vec<u8>)],
        sale_script: impl Fn(u32) -> Option<ScriptBuf>,
    ) -> Self {
        let mut notes = Vec::new();
        let (provider, max_sale_sats) = match setting {
            Some(s) => match Provider::new(&s.name, &s.onion, &s.path_prefix) {
                Ok(p) => (Some(p), s.max_sale_sats),
                Err(e) => {
                    notes.push(format!("CR-218: the swap provider is not used: {e}"));
                    (None, 0)
                }
            },
            None => (None, 0),
        };
        let mut me = Self {
            network,
            provider,
            max_sale_sats,
            pairs: None,
            pairs_retry_at: 0,
            sales: BTreeMap::new(),
            round: 0,
            out: None,
            notes,
        };
        // A swap that was saved is followed whether or not the setting is
        // still on: its lockup could still be claimed.
        for (index, n, bytes) in saved {
            let record = match SwapRecord::from_bytes(bytes) {
                Ok(r) => r,
                Err(e) => {
                    me.notes.push(format!(
                        "CR-220: swap {index}-{n} was not read back and is not followed: {e}"
                    ));
                    continue;
                }
            };
            let Some(script) = sale_script(*index) else {
                me.notes.push(format!(
                    "CR-220: swap {index}-{n} is for a sale the counter does not hold"
                ));
                continue;
            };
            let state = record.machine.state();
            if state.destination().script_pubkey() != script {
                me.notes.push(format!(
                    "CR-220: swap {index}-{n} claims to another address than its sale's"
                ));
                continue;
            }
            let Some(invoice) = checked_of(state.invoice()) else {
                continue;
            };
            let lockup_script = match state.tree() {
                Ok(t) => t.script_pubkey(),
                Err(_) => continue,
            };
            let sats = state.onchain_sats();
            let address = state.destination().clone();
            let mut follow = Follow {
                watch: SaleWatch::new(lockup_script.clone(), sats, 0),
                claim_watch: SaleWatch::new(script.clone(), 1, 0),
                lockup_script,
                sale_script: script,
                invoice,
                record,
                saved: true,
                dirty: false,
                pending_claim: None,
                told: false,
                lockup_told: false,
                stuck_told: false,
                failure_told: false,
                claim_sats: 0,
                last_status: None,
                poll_after: 0,
            };
            // A claim that was sent is sent again after a start, at the same
            // fee: the signature is the same, and so is the transaction.
            if matches!(follow.record.machine.stage(), Stage::ClaimSent) {
                follow.resend(claim_ctx(0, 0, false));
            }
            let sale = me.sales.entry(*index).or_insert_with(|| SaleSwaps {
                open: false,
                pending_told: false,
                address,
                swaps: Vec::new(),
            });
            sale.swaps.push(Swap {
                n: *n,
                sats: state_invoice_sats(&follow),
                quoted_at: 0,
                phase: Phase::Have(Box::new(follow)),
            });
        }
        me
    }

    /// The lines the driver has logged since the last call.
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// Whether a provider is set.
    pub fn is_on(&self) -> bool {
        self.provider.is_some()
    }

    /// The provider, when set.
    pub fn provider(&self) -> Option<&Provider> {
        self.provider.as_ref()
    }

    /// The scripts to watch for the lockups of swaps that have not ended.
    pub fn watched_scripts(&self) -> Vec<ScriptBuf> {
        let mut out = Vec::new();
        for sale in self.sales.values() {
            for swap in &sale.swaps {
                if let Phase::Have(f) = &swap.phase {
                    if !f.ended() {
                        out.push(f.lockup_script.clone());
                    }
                }
            }
        }
        out
    }

    /// CR-219: a sale opened, with the address of its quote and the quote's
    /// satoshis.
    pub fn sale_opened(&mut self, index: u32, address: Address, sats: u64, now: u64) {
        if self.provider.is_none() {
            return;
        }
        let sale = self.sales.entry(index).or_insert_with(|| SaleSwaps {
            open: true,
            pending_told: false,
            address,
            swaps: Vec::new(),
        });
        sale.open = true;
        self.want(index, sats, now);
    }

    /// CR-219: the sale after a restart: it asks again for its current
    /// quote when it is open and has no swap that is alive.
    pub fn sale_restored(&mut self, index: u32, address: Address, sats: u64, open: bool, now: u64) {
        if self.provider.is_none() {
            return;
        }
        let sale = self.sales.entry(index).or_insert_with(|| SaleSwaps {
            open,
            pending_told: false,
            address,
            swaps: Vec::new(),
        });
        sale.open = open;
        if open
            && !sale
                .swaps
                .iter()
                .any(|s| matches!(&s.phase, Phase::Have(f) if !f.ended()))
        {
            self.want(index, sats, now);
        }
    }

    /// CR-219: a new quote of a sale asks again and drops nothing.
    pub fn sale_requoted(&mut self, index: u32, sats: u64, now: u64) {
        if self.provider.is_some() && self.sales.get(&index).is_some_and(|s| s.open) {
            self.want(index, sats, now);
        }
    }

    /// The sale is over (paid, cancelled, expired): no new swap, and the
    /// ones made go on.
    pub fn sale_closed(&mut self, index: u32) {
        if let Some(sale) = self.sales.get_mut(&index) {
            sale.open = false;
            // A swap not yet asked for is not made.
            sale.swaps
                .retain(|s| !matches!(s.phase, Phase::Want | Phase::None(_)));
        }
    }

    fn want(&mut self, index: u32, sats: u64, now: u64) {
        let max = self.max_sale_sats;
        let Some(sale) = self.sales.get_mut(&index) else {
            return;
        };
        if sale.swaps.len() >= MAX_SWAPS {
            return;
        }
        let n = sale.swaps.iter().map(|s| s.n + 1).max().unwrap_or(0);
        let phase = if sats > max {
            Phase::None(Why::OverShopLimit)
        } else {
            Phase::Want
        };
        sale.swaps.push(Swap {
            n,
            sats,
            quoted_at: now,
            phase,
        });
    }

    /// What the screen of a sale shows of the swap (CR-219).
    pub fn sale_view(&self, index: u32, now: u64) -> SaleSwap {
        let Some(sale) = self.sales.get(&index) else {
            return SaleSwap::Off;
        };
        // A lockup that waits for a block, and a swap that failed, first.
        for swap in &sale.swaps {
            if let Phase::Have(f) = &swap.phase {
                if f.waiting_for_block() {
                    return SaleSwap::Confirming(confirming_words());
                }
            }
        }
        let Some(current) = sale.swaps.last() else {
            return SaleSwap::Off;
        };
        for swap in &sale.swaps {
            if let Phase::Have(f) = &swap.phase {
                if f.failed_unpaid() && !current_alive(sale) {
                    return SaleSwap::Failed(failed_words());
                }
            }
        }
        match &current.phase {
            Phase::Have(f) if f.shows_invoice(now) => SaleSwap::Invoice(f.invoice.clone()),
            Phase::Want | Phase::Creating { .. } | Phase::Have(_) => {
                if now > current.quoted_at.saturating_add(SWAP_WINDOW_SECS) {
                    SaleSwap::NotForThisSale
                } else {
                    SaleSwap::Waiting
                }
            }
            Phase::None(Why::OverShopLimit | Why::Late) => SaleSwap::NotForThisSale,
            Phase::None(Why::Down | Why::Refused) => SaleSwap::Unavailable(unavailable_words()),
        }
    }

    /// Whether a claim is not in a block while the provider's deadline is
    /// near (CR-222).
    pub fn any_stuck(&self) -> bool {
        self.sales.values().any(|s| {
            s.swaps
                .iter()
                .any(|w| matches!(&w.phase, Phase::Have(f) if f.stuck_told && f.on_the_way()))
        })
    }

    /// The sale's swaps that are on the way (CR-221): whether the sale must
    /// not expire. For the node's first start.
    pub fn sale_pending(&self, index: u32) -> bool {
        self.sales.get(&index).is_some_and(|s| {
            s.swaps
                .iter()
                .any(|w| matches!(&w.phase, Phase::Have(f) if f.on_the_way()))
        })
    }

    // The requests.

    /// The next request, or none. At most one is out at a time: while one
    /// is, none is handed out until it is answered, failed or given up.
    /// `random` gives 32 bytes of the operating system's randomness, or none
    /// when it has none, in which case nothing is asked for.
    pub fn next(
        &mut self,
        now: u64,
        tip: u32,
        random: &mut dyn FnMut() -> Option<[u8; 32]>,
    ) -> Option<SwapRequest> {
        let provider = self.provider.clone()?;
        if let Some(out) = &self.out {
            if now < out.since.saturating_add(REQUEST_GIVE_UP_SECS) {
                return None;
            }
            let given_up = out.what;
            self.out = None;
            self.fail_asked(given_up, now);
        }
        // The window of a quote that has nothing runs out.
        for sale in self.sales.values_mut() {
            for swap in &mut sale.swaps {
                if matches!(swap.phase, Phase::Want)
                    && now > swap.quoted_at.saturating_add(SWAP_WINDOW_SECS)
                {
                    swap.phase = Phase::None(Why::Late);
                }
            }
        }
        // The status of a swap that is followed.
        let due: Option<(u32, u32)> = self.sales.iter().find_map(|(index, sale)| {
            sale.swaps.iter().find_map(|s| match &s.phase {
                Phase::Have(f) if f.status_due(now) => Some((*index, s.n)),
                _ => None,
            })
        });
        if let Some((index, n)) = due {
            let id = self
                .follow(index, n)
                .map(|f| f.record.machine.state().id().to_string())?;
            let bytes = provider.status_request(&id).ok()?;
            if let Some(f) = self.follow_mut(index, n) {
                f.last_status = Some(now);
            }
            return Some(self.start(Asked::Status(index, n), &provider, bytes, now));
        }
        // A swap that is wanted.
        let want: Option<(u32, u32, u64)> = self.sales.iter().find_map(|(index, sale)| {
            sale.swaps.iter().find_map(|s| match &s.phase {
                Phase::Want => Some((*index, s.n, s.sats)),
                _ => None,
            })
        });
        let (index, n, sats) = want?;
        let Some(pairs) = self.pairs.filter(|p| p.fresh(now).is_ok()) else {
            if now < self.pairs_retry_at {
                return None;
            }
            let bytes = provider.pairs_request().ok()?;
            return Some(self.start(Asked::Pairs, &provider, bytes, now));
        };
        let (preimage, claim_key) = (random()?, random()?);
        let secrets = Secrets::from_random(preimage, claim_key).ok()?;
        let request = match Request::new_live(&secrets, sats, DESCRIPTION, &pairs, now) {
            Ok(r) => r,
            Err(_) => {
                // Outside the provider's limits, or the shop's own band.
                self.set_phase(index, n, Phase::None(Why::Refused));
                return None;
            }
        };
        let bytes = provider.create_request(&request).ok()?;
        self.set_phase(index, n, Phase::Creating { secrets, sats });
        let _ = tip;
        Some(self.start(Asked::Create(index, n), &provider, bytes, now))
    }

    fn start(&mut self, what: Asked, provider: &Provider, bytes: Vec<u8>, now: u64) -> SwapRequest {
        self.round += 1;
        self.out = Some(Out {
            id: self.round,
            what,
            since: now,
        });
        SwapRequest {
            id: self.round,
            host_port: format!("{}:80", provider.onion()),
            bytes,
        }
    }

    fn set_phase(&mut self, index: u32, n: u32, phase: Phase) {
        if let Some(swap) = self
            .sales
            .get_mut(&index)
            .and_then(|s| s.swaps.iter_mut().find(|w| w.n == n))
        {
            swap.phase = phase;
        }
    }

    fn follow(&self, index: u32, n: u32) -> Option<&Follow> {
        self.sales
            .get(&index)?
            .swaps
            .iter()
            .find(|w| w.n == n)
            .and_then(|w| match &w.phase {
                Phase::Have(f) => Some(f.as_ref()),
                _ => None,
            })
    }

    fn follow_mut(&mut self, index: u32, n: u32) -> Option<&mut Follow> {
        self.sales
            .get_mut(&index)?
            .swaps
            .iter_mut()
            .find(|w| w.n == n)
            .and_then(|w| match &mut w.phase {
                Phase::Have(f) => Some(f.as_mut()),
                _ => None,
            })
    }

    /// A request that got no answer or was given up.
    fn fail_asked(&mut self, what: Asked, now: u64) {
        match what {
            Asked::Pairs => {
                self.pairs_retry_at = now.saturating_add(RETRY_AFTER_SECS);
                // Every swap that waits for the terms has none.
                for sale in self.sales.values_mut() {
                    for swap in &mut sale.swaps {
                        if matches!(swap.phase, Phase::Want) {
                            swap.phase = Phase::None(Why::Down);
                        }
                    }
                }
            }
            Asked::Create(index, n) => self.set_phase(index, n, Phase::None(Why::Down)),
            Asked::Status(..) => {}
        }
    }

    /// CR-225: the request with this number got no answer: the app could
    /// not reach the provider.
    pub fn failed(&mut self, id: u64, now: u64) {
        let Some(out) = self.out.take_if(|o| o.id == id) else {
            return;
        };
        self.fail_asked(out.what, now);
    }

    /// The bytes that came back for the request with this number. `tip` is
    /// the height of the chain now.
    pub fn reply(&mut self, id: u64, bytes: &[u8], now: u64, tip: u32) {
        let Some(out) = self.out.take_if(|o| o.id == id) else {
            return;
        };
        let body = match parse_response(bytes).and_then(|r| r.ok_body().map(<[u8]>::to_vec)) {
            Ok(b) => b,
            Err(e) => {
                self.notes.push(format!(
                    "{}: the provider's reply was refused: {e}",
                    e.rule()
                ));
                self.fail_reply(out.what, now);
                return;
            }
        };
        match out.what {
            Asked::Pairs => match Pairs::parse(&body, now) {
                Ok(p) => self.pairs = Some(p),
                Err(e) => {
                    self.notes.push(format!(
                        "{}: the provider's terms were refused: {e}",
                        e.rule()
                    ));
                    self.fail_reply(Asked::Pairs, now);
                }
            },
            Asked::Create(index, n) => self.created(index, n, &body, now, tip),
            Asked::Status(index, n) => self.statused(index, n, &body, now, tip),
        }
    }

    fn fail_reply(&mut self, what: Asked, now: u64) {
        match what {
            Asked::Pairs => self.fail_asked(Asked::Pairs, now),
            Asked::Create(index, n) => self.set_phase(index, n, Phase::None(Why::Refused)),
            Asked::Status(..) => {}
        }
    }

    fn created(&mut self, index: u32, n: u32, body: &[u8], now: u64, tip: u32) {
        let network = self.network;
        let Some(pairs) = self.pairs else {
            self.set_phase(index, n, Phase::None(Why::Down));
            return;
        };
        let Some(sale) = self.sales.get_mut(&index) else {
            return;
        };
        let address = sale.address.clone();
        let Some(swap) = sale.swaps.iter_mut().find(|w| w.n == n) else {
            return;
        };
        let Phase::Creating { secrets, sats } = std::mem::replace(&mut swap.phase, Phase::Want)
        else {
            return;
        };
        let late = now > swap.quoted_at.saturating_add(SWAP_WINDOW_SECS);
        let made = (|| -> Result<Follow, Refused> {
            let reply = Reply::parse(body)?;
            let quote = Quote::from_pairs(&pairs, now)?;
            let checked = check(
                &reply,
                &Expect {
                    secrets: &secrets,
                    invoice_sats: sats,
                    network,
                    quote,
                    tip_height: tip,
                    now_secs: now,
                },
            )?;
            let state = SwapState::new(&checked, &secrets, network, &address)?;
            let lockup_script = checked.tree.script_pubkey();
            let invoice = checked_of(&checked.invoice).ok_or(Refused::BadState("invoice"))?;
            Ok(Follow {
                watch: SaleWatch::new(lockup_script.clone(), checked.onchain_sats, 0),
                claim_watch: SaleWatch::new(address.script_pubkey(), 1, 0),
                lockup_script,
                sale_script: address.script_pubkey(),
                invoice,
                record: SwapRecord {
                    machine: ReverseSwap::new(state),
                    lockup: None,
                    claims: Vec::new(),
                    fee_per_kvb: CLAIM_FEE_PER_KVB,
                    claim_tip: 0,
                },
                saved: false,
                dirty: true,
                pending_claim: None,
                told: false,
                lockup_told: false,
                stuck_told: false,
                failure_told: false,
                claim_sats: 0,
                last_status: Some(now),
                poll_after: openbmt_swap::POLL_WAITING_SECS,
            })
        })();
        match made {
            // CR-219: nobody saw the invoice of a reply that came late, so
            // nobody can pay it, and it is not followed.
            Ok(_) if late => swap.phase = Phase::None(Why::Late),
            Ok(follow) => swap.phase = Phase::Have(Box::new(follow)),
            Err(e) => {
                self.notes
                    .push(format!("{}: the swap was refused: {e}", e.rule()));
                swap.phase = Phase::None(Why::Refused);
            }
        }
    }

    fn statused(&mut self, index: u32, n: u32, body: &[u8], now: u64, tip: u32) {
        let Some(f) = self.follow_mut(index, n) else {
            return;
        };
        match Status::parse(body) {
            Ok(status) => {
                let ctx = claim_ctx(now, tip, false);
                let (machine, next) = f.record.machine.on_status(&status, &ctx);
                f.adopt(machine, &next);
            }
            Err(e) => {
                let line = format!("{}: the swap's status was refused: {e}", e.rule());
                self.notes.push(line);
            }
        }
    }

    // What the chain shows.

    /// A transaction the light client saw, from a peer: for the watch of a
    /// swap's lockup. True if a swap took it.
    pub fn sighting(&mut self, tx: &[u8], from: Source, now: u64) -> bool {
        let mut taken = false;
        for sale in self.sales.values_mut() {
            for swap in &mut sale.swaps {
                if let Phase::Have(f) = &mut swap.phase {
                    if f.ended() {
                        continue;
                    }
                    if f.watch
                        .sighting(tx, from, now)
                        .is_ok_and(|s| !matches!(s, openbmt_watch::Sighted::Ignored))
                    {
                        taken = true;
                    }
                }
            }
        }
        taken
    }

    /// A block the light client downloaded: the lockups and the claims in
    /// it are proven and given to the watchers.
    pub fn block<C: HeaderChain + ?Sized>(
        &mut self,
        block: &WellFormedBlock,
        headers: &C,
        now: u64,
    ) {
        for sale in self.sales.values_mut() {
            for swap in &mut sale.swaps {
                let Phase::Have(f) = &mut swap.phase else {
                    continue;
                };
                if let Ok(list) = block.payments(headers, &f.lockup_script) {
                    for proven in list {
                        let _ = f.watch.confirmed_at(proven, now);
                    }
                }
                if f.record.claims.is_empty() {
                    continue;
                }
                if let Ok(list) = block.payments(headers, &f.sale_script) {
                    for proven in list {
                        if f.record.claims.contains(&proven.tx().compute_txid()) {
                            let _ = f.claim_watch.confirmed_at(proven, now);
                        }
                    }
                }
            }
        }
    }

    /// The records that must be written (CR-220), and, with the last one of
    /// a swap that was just claimed, the claim to send once it is written.
    /// Asking again before [`SwapDriver::saved`] gives them again.
    pub fn flush(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        for (index, sale) in &self.sales {
            for swap in &sale.swaps {
                if let Phase::Have(f) = &swap.phase {
                    if f.dirty || f.pending_claim.is_some() {
                        out.push(Effect::Save {
                            index: *index,
                            n: swap.n,
                            bytes: f.record.to_bytes(),
                            send: f.pending_claim.clone(),
                        });
                    }
                }
            }
        }
        out
    }

    /// The record of a swap was written, as [`Effect::Save`] gave it.
    pub fn saved(&mut self, index: u32, n: u32, bytes: &[u8]) {
        if let Some(f) = self.follow_mut(index, n) {
            if f.record.to_bytes() == bytes {
                f.saved = true;
                f.dirty = false;
                f.pending_claim = None;
            }
        }
    }

    /// The sale of a swap was told that it is paid, with its lockup in a
    /// block or not: if not, [`Update::LockupConfirmed`] follows.
    pub fn told(&mut self, index: u32, n: u32, lockup_confirmed: bool) {
        if let Some(f) = self.follow_mut(index, n) {
            f.told = true;
            f.lockup_told = lockup_confirmed;
        }
    }

    /// Go through every swap with what the chain and the provider have
    /// shown: the lockup is read from the watch, the claim built when its
    /// time has come, built again when it did not confirm, and what the node
    /// must tell the counter is returned.
    ///
    /// `decide` says, for a sale and the quote's satoshis and the lockup's
    /// status, whether the counter takes the payment at once (CR-221).
    pub fn step<C: HeaderChain + ?Sized>(
        &mut self,
        now: u64,
        chain: &C,
        decide: &mut dyn FnMut(u32, u64, &SaleStatus) -> bool,
    ) -> Vec<Update> {
        let tip = chain.tip_height();
        let mut updates = Vec::new();
        let mut notes = Vec::new();
        for (index, sale) in &mut self.sales {
            let index = *index;
            let mut pending = false;
            for swap in &mut sale.swaps {
                let n = swap.n;
                let sats = swap.sats;
                let Phase::Have(f) = &mut swap.phase else {
                    continue;
                };
                f.step(
                    index,
                    n,
                    sats,
                    now,
                    tip,
                    chain,
                    decide,
                    &mut updates,
                    &mut notes,
                );
                pending |= f.on_the_way();
            }
            // CR-221: told when it changes.
            if sale.pending_told != pending {
                sale.pending_told = pending;
                updates.push(Update::Pending { index, pending });
            }
        }
        self.notes.append(&mut notes);
        updates
    }
}

fn state_invoice_sats(f: &Follow) -> u64 {
    f.invoice.amount_msat / 1_000
}

fn current_alive(sale: &SaleSwaps) -> bool {
    sale.swaps
        .last()
        .is_some_and(|w| matches!(&w.phase, Phase::Have(f) if !f.ended()))
}

impl Follow {
    fn ended(&self) -> bool {
        matches!(
            self.record.machine.stage(),
            Stage::Paid { .. } | Stage::Failed(_)
        )
    }

    /// The claim is not built yet and the lockup is in the mempool: the sale
    /// is waiting for a block.
    fn waiting_for_block(&self) -> bool {
        matches!(self.record.machine.stage(), Stage::Locked(l) if !l.confirmed)
            && !self.told
            && self.record.claims.is_empty()
    }

    fn failed_unpaid(&self) -> bool {
        matches!(
            self.record.machine.stage(),
            Stage::Failed(f) if f != Failure::InvoiceExpired
        ) && !self.told
    }

    fn on_the_way(&self) -> bool {
        matches!(
            self.record.machine.stage(),
            Stage::Locked(_) | Stage::ClaimSent
        )
    }

    fn shows_invoice(&self, now: u64) -> bool {
        self.saved
            && matches!(
                self.record.machine.stage(),
                Stage::Created | Stage::Announced
            )
            && now < self.record.machine.state().invoice_expires_at()
    }

    fn status_due(&self, now: u64) -> bool {
        matches!(
            self.record.machine.stage(),
            Stage::Created | Stage::Announced | Stage::Locked(_)
        ) && self.saved
            && self
                .last_status
                .is_none_or(|at| now >= at.saturating_add(u64::from(self.poll_after)))
    }

    fn adopt(&mut self, machine: ReverseSwap, next: &Next) {
        if let Next::Wait { poll_after_secs } = next {
            self.poll_after = *poll_after_secs;
        }
        if machine != self.record.machine {
            self.record.machine = machine;
            self.dirty = true;
        }
    }

    /// The same claim again, after a start: its signature is the same, so
    /// is its transaction.
    fn resend(&mut self, ctx: Ctx) {
        let Some(lockup) = self.record.lockup else {
            return;
        };
        let Ok((machine, tx)) = self.record.machine.reclaim(
            &lockup,
            self.record.fee_per_kvb,
            &Ctx {
                tip_height: 0,
                ..ctx
            },
        ) else {
            return;
        };
        if machine.state().phase() == self.record.machine.state().phase() {
            self.claim_sats = tx.output.first().map_or(0, |o| o.value.to_sat());
            self.pending_claim = Some(serialize(&tx));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn step<C: HeaderChain + ?Sized>(
        &mut self,
        index: u32,
        n: u32,
        sats: u64,
        now: u64,
        tip: u32,
        chain: &C,
        decide: &mut dyn FnMut(u32, u64, &SaleStatus) -> bool,
        updates: &mut Vec<Update>,
        notes: &mut Vec<String>,
    ) {
        let stage = self.record.machine.stage();
        let status = self.watch.status(chain, now);
        let confirmed = status.min_confirmations >= 1;
        // 1. The lockup the light client holds is checked by SW-30.
        if matches!(stage, Stage::Created | Stage::Announced | Stage::Locked(_)) {
            let held = self.watch.transactions().next().cloned();
            if let Some(tx) = held {
                let better = match stage {
                    Stage::Locked(l) => confirmed && !l.confirmed,
                    _ => true,
                };
                if better {
                    let seen = if confirmed {
                        Seen::Block
                    } else {
                        Seen::Mempool
                    };
                    match self
                        .record
                        .machine
                        .on_lockup(&tx, seen, &claim_ctx(now, tip, false))
                    {
                        Ok((machine, _)) => {
                            if let Stage::Locked(l) = machine.stage() {
                                self.record.lockup = Some(l);
                            }
                            self.adopt(machine, &Next::CheckLockup);
                        }
                        Err(e) => notes.push(format!("{}: a lockup was refused: {e}", e.rule())),
                    }
                }
            }
        }
        // A swap past the timeout or the invoice's end has failed.
        if matches!(
            self.record.machine.stage(),
            Stage::Created | Stage::Announced
        ) {
            let (machine, next) = self.record.machine.settle(&claim_ctx(now, tip, false));
            if matches!(next, Next::Failed(_)) {
                self.adopt(machine, &next);
            }
        }
        let stage = self.record.machine.stage();
        // 2. The claim, when its time has come.
        if let Stage::Locked(l) = stage {
            let accept = l.confirmed || decide(index, sats, &status);
            let ctx = claim_ctx(now, tip, accept);
            let (machine, next) = self.record.machine.settle(&ctx);
            match next {
                Next::BuildClaim { lockup } => match machine.claim(self.record.fee_per_kvb, &ctx) {
                    Ok((claimed, tx)) => {
                        self.record.machine = claimed;
                        self.record.lockup = Some(lockup);
                        self.record.claims.push(tx.compute_txid());
                        self.record.claim_tip = tip;
                        self.claim_sats = tx.output.first().map_or(0, |o| o.value.to_sat());
                        self.pending_claim = Some(serialize(&tx));
                        self.dirty = true;
                    }
                    Err(e) => notes.push(format!("{}: the claim was not built: {e}", e.rule())),
                },
                Next::Failed(_) => self.adopt(machine, &next),
                _ => {}
            }
        }
        let stage = self.record.machine.stage();
        // 3. The claim, sent: told to the sale, built again, or confirmed.
        if matches!(stage, Stage::ClaimSent) {
            if let Some(lockup) = self.record.lockup {
                if !self.told {
                    updates.push(Update::Claimed {
                        index,
                        n,
                        payment_hash: self.invoice.payment_hash,
                        invoice_sats: self.invoice.amount_msat / 1_000,
                        received_sats: self.claim_sats,
                        lockup_confirmed: lockup.confirmed || confirmed,
                        status,
                    });
                }
                let cst = self.claim_watch.status(chain, now);
                if cst.min_confirmations >= 1 {
                    let height = tip.saturating_sub(cst.min_confirmations).saturating_add(1);
                    let seen = self
                        .claim_watch
                        .standing()
                        .first()
                        .map(|(txid, _)| *txid)
                        .or(self.record.claims.last().copied());
                    if let Some(txid) = seen {
                        let earlier = self.record.claims.clone();
                        if let Ok(done) = self
                            .record
                            .machine
                            .claim_confirmed_among(txid, &earlier, height)
                        {
                            // The claim that was mined is the last one.
                            self.record.claims.retain(|c| *c != txid);
                            self.record.claims.push(txid);
                            self.record.machine = done;
                            self.dirty = true;
                        }
                    }
                } else if tip > self.record.claim_tip {
                    // CR-222: a block came and the claim is not in it.
                    self.bump(lockup, tip, now, notes);
                }
                let timeout = u64::from(self.record.machine.state().timeout());
                if !self.stuck_told
                    && cst.min_confirmations == 0
                    && u64::from(tip) + MIN_CLAIM_BLOCKS / 2 >= timeout
                {
                    self.stuck_told = true;
                    updates.push(Update::Stuck { index });
                }
            }
        }
        let stage = self.record.machine.stage();
        // 4. The lockup, now in a block, for the sale paid before.
        if self.told && !self.lockup_told && confirmed {
            self.lockup_told = true;
            if let Some(l) = &mut self.record.lockup {
                l.confirmed = true;
                self.dirty = true;
            }
            updates.push(Update::LockupConfirmed { index });
        }
        // 5. A swap that failed.
        if let Stage::Failed(failure) = stage {
            if !self.failure_told {
                self.failure_told = true;
                updates.push(Update::Failed { index, n, failure });
                if self.told && !self.lockup_told {
                    updates.push(Update::Cancel { index });
                }
            }
        } else if self.told && !self.lockup_told && status.conflict_seen {
            // The lockup the sale was paid on was replaced.
            self.lockup_told = true;
            updates.push(Update::Cancel { index });
        }
    }

    fn bump(&mut self, lockup: LockupSeen, tip: u32, now: u64, notes: &mut Vec<String>) {
        let old = self.record.fee_per_kvb;
        let fee = old.saturating_add((old / 2).max(BUMP_MIN_PER_KVB));
        let ctx = claim_ctx(now, tip, true);
        match self.record.machine.reclaim(&lockup, fee, &ctx) {
            Ok((machine, tx)) => {
                self.record.machine = machine;
                self.record.fee_per_kvb = fee;
                self.record.claim_tip = tip;
                self.record.claims.push(tx.compute_txid());
                if self.record.claims.len() > MAX_CLAIMS {
                    self.record.claims.remove(0);
                }
                self.claim_sats = tx.output.first().map_or(0, |o| o.value.to_sat());
                self.pending_claim = Some(serialize(&tx));
                self.dirty = true;
            }
            Err(e) => {
                // Too late, or too dear: the claim as it is stays out.
                self.record.claim_tip = tip;
                notes.push(format!("{}: the claim was not built again: {e}", e.rule()));
            }
        }
    }
}

// The words.

/// CR-221: the sale waits for the lockup's first block.
pub fn confirming_words() -> Words {
    Words::new(
        Tone::Info,
        "Waiting for one block",
        "The Lightning payment arrived. This sale is above the instant limit, so it waits for one block.",
        Who::Cashier,
        "Ask the customer to wait a few minutes.",
    )
    .code("CR-221")
}

/// CR-223: the swap failed before the claim.
pub fn failed_words() -> Words {
    Words::new(
        Tone::Stop,
        "Lightning payment not finished",
        "The swap provider did not lock the bitcoin in time, so the customer's payment was cancelled and goes back to them.",
        Who::Cashier,
        "Ask the customer to pay again, or take another payment.",
    )
    .code("CR-223")
}

/// CR-219: the provider could not be reached, or refused the amount.
pub fn unavailable_words() -> Words {
    Words::new(
        Tone::Info,
        "Lightning off for now",
        "The swap provider could not be reached, or refused this amount.",
        Who::Cashier,
        "The same code still takes bitcoin on-chain.",
    )
    .code("CR-219")
}

/// CR-222: the claim is not yet in a block and the deadline is near.
pub fn claim_stuck_words() -> Words {
    Words::new(
        Tone::Info,
        "Lightning claim waiting",
        "A Lightning sale's claim has not confirmed and the provider's deadline is near.",
        Who::Owner,
        "Nothing to do. The counter keeps raising the fee. If the deadline passes, the sale is shown unpaid.",
    )
    .code("CR-222")
}

/// Every words of this driver, for the list of SC-12. The heading of
/// [`failed_words`] is the brief's and is 30 characters, two over the
/// limit of SC-12, so it is not in the list until that is decided.
pub fn all_words() -> Vec<(String, Words)> {
    vec![
        ("SwapConfirming".to_string(), confirming_words()),
        ("SwapUnavailable".to_string(), unavailable_words()),
        ("SwapClaimStuck".to_string(), claim_stuck_words()),
    ]
}

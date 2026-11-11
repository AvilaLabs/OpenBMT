//! One sale: what it holds, how its satoshis are valued (CR-05, CR-06,
//! CR-08, CR-16, CR-17) and its stored form (`OBML`, CR-42).
//!
//! A sale belongs to one receive index. It keeps the watcher's evidence
//! ([`SaleWatch`]) and everything the counter must remember about the sale
//! that the watcher does not know: the quotes, the valued satoshis, what was
//! accepted at once or overridden, what was reported outward.
//!
//! ```text
//! OBML | 1 | index u32 | opened_at u64 | script_len u32 | script
//!   | amount u64 | currency (3) | tolerance_bps u32
//!   | n_quotes u32 | n_quotes * (minor_per_btc u64 | from u64 | until u64)
//!   | n_segments u32 | n_segments * (sats u64 | quote u32)
//!   | frozen (flag, u64) | accepted (flag, label u8, mode u8)
//!   | overridden (flag, role u8, name_len u8, name, at u64, conflict flag, reopened flag)
//!   | reopened | cancel_recorded | exposure_counted | exposure_released (flags)
//!   | ended (0 none, 1 expired, 2 cancelled; then at u64 unless none)
//!   | dust_logged flag | pos u8 | confirmed_logged flag | books_final flag
//!   | surplus_reported u64
//!   | late_unlisted flag | n_late u32 | n_late * (txid (32) | sats u64)
//!   | watch_len u32 | the sale watch (OBMV)
//!
//! OBML | 2 | the fields of version 1
//!   | n_refunds u32 | n_refunds * refund entry   (see `refund::RefundEntry`)
//! ```
//!
//! OBML | 3 | the fields of version 1
//!   | payment_hash (32) | amount_msat u64 | settled_at (flag, u64)
//!   | n_refunds u32 | n_refunds * refund entry
//!
//! Version 2 is written only for a sale that has a refund entry (CR-61,
//! CR-73); a sale without one is written as version 1, which is what every
//! earlier build wrote. A version 2 form with no entry does not read back.
//! Version 3 is written only for a sale that was paid over Lightning
//! (CR-109); its refund list may be empty. A sale that was not is written as
//! before, so every earlier file reads back and every earlier build's file
//! is unchanged.

use bitcoin::hashes::Hash;
use bitcoin::{ScriptBuf, Txid};
use openbmt_core::policy::{CheckMode, InstantLabel};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_watch::{
    HeaderChain, OwnNodeHealth, SaleStatus, SaleWatch, MAX_SCRIPT_BYTES, MAX_TOLERANCE_BPS,
};

use crate::desk::{len32, valid_name, Role};
use crate::events::{label_from_u8, label_to_u8, mode_from_u8, mode_to_u8};
use crate::money::{is_enough, sats_for, value_of, Amount, Currency, Quote, Rate, QUOTE_SECS};
use crate::refund::{RefundEntry, MAX_REFUNDS_PER_SALE};
use crate::{
    DUST_SATS, MAX_LATE_PAYMENTS, MAX_NAME_BYTES, MAX_QUOTES, MAX_SEGMENTS, MAX_WATCH_BYTES,
};

/// The version byte of a sale that holds refund entries (CR-73).
const SALE_VERSION_WITH_REFUNDS: u8 = 2;
/// The version byte of a sale that was paid over Lightning (CR-109).
const SALE_VERSION_LIGHTNING: u8 = 3;

/// What the sale keeps of a Lightning payment (CR-109, CR-110): the facts
/// the node reported when the invoice was settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightningPaid {
    /// The payment hash of the invoice.
    pub payment_hash: [u8; 32],
    /// What the node received, in millisatoshis.
    pub amount_msat: u64,
    /// When the node says it settled, in seconds since 1970, if it says.
    pub settled_at: Option<u64>,
}

impl LightningPaid {
    /// The whole satoshis received (the millisatoshis rounded down).
    pub fn sats(&self) -> u64 {
        self.amount_msat / 1_000
    }
}

/// How a sale ended (CR-07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// Nothing arrived inside the time.
    Expired,
    /// The cashier cancelled it.
    Cancelled,
}

/// What the POS and the books were last told about a sale (CR-30).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosState {
    /// Nothing yet.
    Nothing,
    /// Closed as paid.
    Paid,
    /// Reported unpaid again after it had been reported paid.
    Unpaid,
}

/// Satoshis valued once, at one quote (CR-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// The satoshis.
    pub sats: u64,
    /// The index of the quote they were valued at.
    pub quote: u32,
}

/// Who overrode a sale, and what was true then (CR-22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideMark {
    /// Cashier or manager.
    pub role: Role,
    /// Who.
    pub name: String,
    /// When.
    pub at: u64,
    /// A conflict had already been seen. Rule 3 of CR-12 applies again only
    /// to a conflict that is new.
    pub conflict: bool,
    /// The sale was already reopened by a reorg. Rule 2 of CR-12 applies
    /// again only to a reorg that is new.
    pub reopened: bool,
}

/// One sale.
#[derive(Debug, Clone)]
pub struct Sale {
    pub(crate) index: u32,
    pub(crate) script: ScriptBuf,
    pub(crate) opened_at: u64,
    pub(crate) amount: Amount,
    pub(crate) currency: Currency,
    pub(crate) tolerance_bps: u32,
    pub(crate) quotes: Vec<Quote>,
    pub(crate) segments: Vec<Segment>,
    pub(crate) frozen: Option<u64>,
    pub(crate) watch: SaleWatch,
    pub(crate) accepted: Option<(InstantLabel, CheckMode)>,
    pub(crate) overridden: Option<OverrideMark>,
    pub(crate) reopened: bool,
    pub(crate) cancel_recorded: bool,
    pub(crate) exposure_counted: bool,
    pub(crate) exposure_released: bool,
    pub(crate) ended: Option<(End, u64)>,
    pub(crate) dust_logged: bool,
    pub(crate) pos: PosState,
    pub(crate) confirmed_logged: bool,
    pub(crate) books_final: bool,
    pub(crate) surplus_reported: u64,
    pub(crate) late: Vec<(Txid, u64)>,
    pub(crate) late_unlisted: bool,
    pub(crate) refunds: Vec<RefundEntry>,
    pub(crate) lightning: Option<LightningPaid>,
}

impl Sale {
    /// A new sale for `index`, paid to `script`, with its first quote and
    /// the satoshis that quote asks for.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        index: u32,
        script: ScriptBuf,
        opened_at: u64,
        amount: Amount,
        currency: Currency,
        quote: Quote,
        needs_sats: u64,
        tolerance_bps: u32,
    ) -> Self {
        let tolerance_bps = tolerance_bps.min(MAX_TOLERANCE_BPS);
        Self {
            index,
            watch: SaleWatch::new(script.clone(), needs_sats, tolerance_bps),
            script,
            opened_at,
            amount,
            currency,
            tolerance_bps,
            quotes: vec![quote],
            segments: Vec::new(),
            frozen: None,
            accepted: None,
            overridden: None,
            reopened: false,
            cancel_recorded: false,
            exposure_counted: false,
            exposure_released: false,
            ended: None,
            dust_logged: false,
            pos: PosState::Nothing,
            confirmed_logged: false,
            books_final: false,
            surplus_reported: 0,
            late: Vec::new(),
            late_unlisted: false,
            refunds: Vec::new(),
            lightning: None,
        }
    }

    /// The receive index.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// The address script.
    pub fn script(&self) -> &ScriptBuf {
        &self.script
    }

    /// When the sale opened.
    pub fn opened_at(&self) -> u64 {
        self.opened_at
    }

    /// The amount in the shop's currency.
    pub fn amount(&self) -> Amount {
        self.amount
    }

    /// The currency.
    pub fn currency(&self) -> Currency {
        self.currency
    }

    /// All quotes of the sale, oldest first (CR-05).
    pub fn quotes(&self) -> &[Quote] {
        &self.quotes
    }

    /// The valued segments (CR-06).
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// The need frozen once enough had arrived (CR-08).
    pub fn frozen_need(&self) -> Option<u64> {
        self.frozen
    }

    /// What the sale needs right now, in satoshis.
    pub fn needs_sats(&self) -> u64 {
        self.watch.sale_sats()
    }

    /// The evidence.
    pub fn watch(&self) -> &SaleWatch {
        &self.watch
    }

    /// How the sale ended, and when.
    pub fn ended(&self) -> Option<(End, u64)> {
        self.ended
    }

    /// The check label it was accepted at once with, and the mode.
    pub fn accepted(&self) -> Option<(InstantLabel, CheckMode)> {
        self.accepted
    }

    /// Who overrode it.
    pub fn overridden(&self) -> Option<&OverrideMark> {
        self.overridden.as_ref()
    }

    /// What the POS was last told.
    pub fn pos(&self) -> PosState {
        self.pos
    }

    /// The books were told it is final (CR-31).
    pub fn books_final(&self) -> bool {
        self.books_final
    }

    /// The Lightning payment that settled the sale, if one did (CR-109).
    pub fn lightning(&self) -> Option<&LightningPaid> {
        self.lightning.as_ref()
    }

    /// The exposure for this sale is counted in the ledger.
    pub fn exposure_counted(&self) -> bool {
        self.exposure_counted
    }

    /// The exposure for this sale has left the ledger (CR-15).
    pub fn exposure_released(&self) -> bool {
        self.exposure_released
    }

    /// The cancel of this sale was recorded (CR-14).
    pub fn cancel_recorded(&self) -> bool {
        self.cancel_recorded
    }

    /// Payments that arrived after the sale ended, with what each paid
    /// (CR-19).
    pub fn late_payments(&self) -> &[(Txid, u64)] {
        &self.late
    }

    /// CR-61 to CR-72: whether the sale was at the refund depth the last
    /// time it was judged and has not been reopened since. A refund request
    /// is "waiting for depth" until then (CR-66).
    pub fn at_refund_depth(&self) -> bool {
        self.books_final && !self.reopened
    }

    /// CR-19: a payment arrived after the sale ended or was final.
    pub fn is_late(&self) -> bool {
        !self.late.is_empty() || self.late_unlisted
    }

    /// Whether the sale still takes evidence: it has not ended and the books
    /// have not been told it is final. A script that is not live is watched
    /// for late payments only (CR-19).
    pub fn is_live(&self) -> bool {
        self.ended.is_none() && !self.books_final
    }

    fn segment_sum(&self) -> u64 {
        self.segments
            .iter()
            .fold(0u64, |sum, s| sum.saturating_add(s.sats))
    }

    pub(crate) fn rate_of(&self, quote: u32) -> Option<Rate> {
        let i = usize::try_from(quote).ok()?;
        self.quotes.get(i).map(|q| q.rate)
    }

    /// CR-06: the quote in force at `now`: the newest whose window holds
    /// it, or the last quote if none does (the one the customer was shown).
    pub(crate) fn quote_index_at(&self, now: u64) -> u32 {
        let found = self.quotes.iter().rposition(|q| q.holds(now));
        let i = found.unwrap_or(self.quotes.len().saturating_sub(1));
        u32::try_from(i).unwrap_or(u32::MAX)
    }

    /// CR-06: what the valued segments are worth, each at the rate of its
    /// own quote. `None` when a segment names a quote the sale does not hold.
    fn segments_value(&self) -> Option<u64> {
        let mut valued = 0u64;
        for s in &self.segments {
            let rate = self.rate_of(s.quote)?;
            valued = valued.saturating_add(value_of(s.sats, rate).0);
        }
        Some(valued)
    }

    /// CR-32: what `received` satoshis are worth in the shop's currency: the
    /// valued segments at their own quotes (CR-06) and the satoshis not
    /// valued yet at the quote in force at `now`, as [`Sale::value`] would
    /// value them.
    pub(crate) fn valued_amount(&self, received: u64, now: u64) -> Amount {
        let Some(valued) = self.segments_value() else {
            return Amount(0);
        };
        let unvalued = received.saturating_sub(self.segment_sum());
        let rest = match self.rate_of(self.quote_index_at(now)) {
            Some(rate) => value_of(unvalued, rate).0,
            None => 0,
        };
        Amount(valued.saturating_add(rest))
    }

    /// CR-08: the satoshis already valued plus the rest of the amount at the
    /// rate of quote `q`, rounded up. A rate that cannot be found gives the
    /// largest number, so nothing is ever enough.
    fn needs_at(&self, q: u32) -> u64 {
        let Some(valued) = self.segments_value() else {
            return u64::MAX;
        };
        let rest = Amount(self.amount.0.saturating_sub(valued));
        let Some(rate) = self.rate_of(q) else {
            return u64::MAX;
        };
        let rest_sats = sats_for(rest, rate).unwrap_or(u64::MAX);
        self.segment_sum().saturating_add(rest_sats)
    }

    /// CR-06, CR-08: value what has arrived and set what the sale needs.
    /// `received` is what the watcher says has arrived, with dust counted as
    /// nothing (CR-17).
    ///
    /// 1. A frozen need stays while `received` plus the tolerance still
    ///    reaches it; otherwise it is cleared.
    /// 2. While the valued satoshis exceed `received`, the newest valuation
    ///    shrinks or goes.
    /// 3. The need is worked out at the quote in force.
    /// 4. If that is enough, it is frozen. Otherwise satoshis that arrived
    ///    and are not valued yet are valued at the quote in force, and the
    ///    need is worked out again.
    pub(crate) fn value(&mut self, received: u64, now: u64) {
        let bps = self.tolerance_bps;
        if let Some(frozen) = self.frozen {
            if is_enough(received, frozen, bps) {
                self.watch.set_sale_sats(frozen);
                return;
            }
            self.frozen = None;
        }
        while self.segment_sum() > received {
            let excess = self.segment_sum().saturating_sub(received);
            match self.segments.last_mut() {
                Some(last) if last.sats > excess => {
                    last.sats = last.sats.saturating_sub(excess);
                }
                Some(_) => {
                    self.segments.pop();
                }
                None => break,
            }
        }
        let q = self.quote_index_at(now);
        let mut needs = self.needs_at(q);
        if is_enough(received, needs, bps) {
            self.frozen = Some(needs);
            self.watch.set_sale_sats(needs);
            return;
        }
        let unvalued = received.saturating_sub(self.segment_sum());
        if unvalued > 0 && self.segments.len() < MAX_SEGMENTS {
            self.segments.push(Segment {
                sats: unvalued,
                quote: q,
            });
            needs = self.needs_at(q);
        }
        self.watch.set_sale_sats(needs);
    }

    /// CR-11: ask the watcher for its status, keep the "reopened" latch,
    /// value what has arrived, and ask again if the need moved (the
    /// confirmation depth is judged against the need). `depth` is the
    /// shop's confirmation depth.
    ///
    /// Returns the status and what has arrived, with dust counted as nothing.
    pub(crate) fn refresh<C: HeaderChain + ?Sized>(
        &mut self,
        headers: &C,
        now: u64,
        depth: u32,
        own: &OwnNodeHealth,
    ) -> (SaleStatus, u64) {
        let first = self.watch.status_with_own_node(headers, now, own);
        let before = self.watch.sale_sats();
        let received = if first.received_sats < DUST_SATS {
            0
        } else {
            first.received_sats
        };
        self.value(received, now);
        let status = if self.watch.sale_sats() == before {
            first
        } else {
            self.watch.status_with_own_node(headers, now, own)
        };
        if first.unconfirmed_again {
            self.reopened = true;
        }
        if status.min_confirmations >= depth {
            self.reopened = false;
            if let Some(mark) = self.overridden.as_mut() {
                mark.reopened = false;
            }
        }
        (status, received)
    }

    /// The stored form (see the module docs). Lengths are bounded by the
    /// caps, so the length fields cannot overflow.
    pub fn to_bytes(&self) -> Vec<u8> {
        let watch = self.watch.to_bytes();
        let mut out = wire::start(&wire::MAGIC_COUNTER_SALE, 256 + watch.len());
        out.extend_from_slice(&self.index.to_le_bytes());
        out.extend_from_slice(&self.opened_at.to_le_bytes());
        out.extend_from_slice(&len32(self.script.len()).to_le_bytes());
        out.extend_from_slice(self.script.as_bytes());
        out.extend_from_slice(&self.amount.0.to_le_bytes());
        out.extend_from_slice(&self.currency.code());
        out.extend_from_slice(&self.tolerance_bps.to_le_bytes());
        out.extend_from_slice(&len32(self.quotes.len()).to_le_bytes());
        for q in &self.quotes {
            out.extend_from_slice(&q.rate.minor_per_btc().to_le_bytes());
            out.extend_from_slice(&q.from.to_le_bytes());
            out.extend_from_slice(&q.until.to_le_bytes());
        }
        out.extend_from_slice(&len32(self.segments.len()).to_le_bytes());
        for s in &self.segments {
            out.extend_from_slice(&s.sats.to_le_bytes());
            out.extend_from_slice(&s.quote.to_le_bytes());
        }
        wire::put_opt_u64(&mut out, self.frozen);
        match self.accepted {
            Some((label, mode)) => {
                out.push(1);
                out.push(label_to_u8(label));
                out.push(mode_to_u8(mode));
            }
            None => out.push(0),
        }
        match &self.overridden {
            Some(mark) => {
                out.push(1);
                out.push(mark.role as u8);
                out.push(u8::try_from(mark.name.len()).unwrap_or(u8::MAX));
                out.extend_from_slice(mark.name.as_bytes());
                out.extend_from_slice(&mark.at.to_le_bytes());
                out.push(u8::from(mark.conflict));
                out.push(u8::from(mark.reopened));
            }
            None => out.push(0),
        }
        out.push(u8::from(self.reopened));
        out.push(u8::from(self.cancel_recorded));
        out.push(u8::from(self.exposure_counted));
        out.push(u8::from(self.exposure_released));
        match self.ended {
            Some((End::Expired, at)) => {
                out.push(1);
                out.extend_from_slice(&at.to_le_bytes());
            }
            Some((End::Cancelled, at)) => {
                out.push(2);
                out.extend_from_slice(&at.to_le_bytes());
            }
            None => out.push(0),
        }
        out.push(u8::from(self.dust_logged));
        out.push(match self.pos {
            PosState::Nothing => 0,
            PosState::Paid => 1,
            PosState::Unpaid => 2,
        });
        out.push(u8::from(self.confirmed_logged));
        out.push(u8::from(self.books_final));
        out.extend_from_slice(&self.surplus_reported.to_le_bytes());
        out.push(u8::from(self.late_unlisted));
        out.extend_from_slice(&len32(self.late.len()).to_le_bytes());
        for (txid, sats) in &self.late {
            out.extend_from_slice(&txid.to_byte_array());
            out.extend_from_slice(&sats.to_le_bytes());
        }
        out.extend_from_slice(&len32(watch.len()).to_le_bytes());
        out.extend_from_slice(&watch);
        if let Some(paid) = &self.lightning {
            if let Some(byte) = out.get_mut(4) {
                *byte = SALE_VERSION_LIGHTNING;
            }
            out.extend_from_slice(&paid.payment_hash);
            out.extend_from_slice(&paid.amount_msat.to_le_bytes());
            wire::put_opt_u64(&mut out, paid.settled_at);
            out.extend_from_slice(&len32(self.refunds.len()).to_le_bytes());
            for entry in &self.refunds {
                entry.write(&mut out);
            }
        } else if !self.refunds.is_empty() {
            if let Some(byte) = out.get_mut(4) {
                *byte = SALE_VERSION_WITH_REFUNDS;
            }
            out.extend_from_slice(&len32(self.refunds.len()).to_le_bytes());
            for entry in &self.refunds {
                entry.write(&mut out);
            }
        }
        out
    }

    /// Read the stored form strictly (see `openbmt_core::wire`). Besides the
    /// framing it checks every invariant the live code keeps: the currency
    /// code, a non-zero amount, a tolerance within the cap, 1 to
    /// [`MAX_QUOTES`] quotes each ending [`QUOTE_SECS`] after it starts and
    /// each able to price the amount, segments that name an existing quote
    /// and whose satoshis do not add past the largest number, a frozen need
    /// above zero, known labels, modes and roles, a name of valid text, a
    /// cancel only on a sale that was accepted or overridden, no end on a
    /// sale that was accepted or overridden, late payments without repeats,
    /// and a watch that parses, pays the same script and has the same
    /// tolerance.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let bad = DecodeError::NonCanonical;
        // `Reader::framed` reads only version 1, and this form has two.
        let mut r = Reader::new(bytes);
        if r.take(4)? != wire::MAGIC_COUNTER_SALE {
            return Err(DecodeError::BadMagic);
        }
        let version = r.u8()?;
        if version != wire::VERSION
            && version != SALE_VERSION_WITH_REFUNDS
            && version != SALE_VERSION_LIGHTNING
        {
            return Err(DecodeError::UnsupportedVersion(version));
        }
        let index = r.u32_le()?;
        let opened_at = r.u64_le()?;
        let script_len = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
        if script_len == 0 || script_len > MAX_SCRIPT_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let script = ScriptBuf::from_bytes(r.take(script_len)?.to_vec());
        let amount = Amount(r.u64_le()?);
        if amount.0 == 0 {
            return Err(bad("a sale has no amount"));
        }
        let currency = Currency::new(r.array::<3>()?).ok_or(bad("a currency code is not valid"))?;
        let tolerance_bps = r.u32_le()?;
        if tolerance_bps > MAX_TOLERANCE_BPS {
            return Err(bad("tolerance is above 10000"));
        }

        let n = r.count(MAX_QUOTES, 24)?;
        if n == 0 {
            return Err(bad("a sale has no quote"));
        }
        let mut quotes = Vec::with_capacity(n);
        for _ in 0..n {
            let rate = Rate::new(r.u64_le()?).ok_or(bad("a rate is zero"))?;
            let from = r.u64_le()?;
            let until = r.u64_le()?;
            if until != from.saturating_add(QUOTE_SECS) {
                return Err(bad("a quote does not last the quote time"));
            }
            if sats_for(amount, rate).is_none() {
                return Err(bad("a quote cannot price the amount"));
            }
            quotes.push(Quote { rate, from, until });
        }

        let n = r.count(MAX_SEGMENTS, 12)?;
        let mut segments = Vec::with_capacity(n);
        let mut sum = 0u64;
        for _ in 0..n {
            let sats = r.u64_le()?;
            let quote = r.u32_le()?;
            if sats == 0 {
                return Err(bad("a valued segment is empty"));
            }
            if usize::try_from(quote).map_or(true, |q| q >= quotes.len()) {
                return Err(bad("a segment names a quote that does not exist"));
            }
            sum = sum
                .checked_add(sats)
                .ok_or(bad("valued satoshis overflow"))?;
            segments.push(Segment { sats, quote });
        }

        let frozen = r.opt_u64()?;
        if frozen == Some(0) {
            return Err(bad("a frozen need is zero"));
        }
        let accepted = if r.flag()? {
            let label = label_from_u8(r.u8()?).ok_or(bad("a label is not known"))?;
            let mode = mode_from_u8(r.u8()?).ok_or(bad("a check mode is not known"))?;
            Some((label, mode))
        } else {
            None
        };
        let overridden = if r.flag()? {
            let role = Role::from_u8(r.u8()?).ok_or(bad("a role is not known"))?;
            let len = usize::from(r.u8()?);
            if len == 0 || len > MAX_NAME_BYTES {
                return Err(bad("a name has a bad length"));
            }
            let name = std::str::from_utf8(r.take(len)?)
                .map_err(|_| bad("a name is not text"))?
                .to_string();
            if !valid_name(&name) {
                return Err(bad("a name is not valid"));
            }
            Some(OverrideMark {
                role,
                name,
                at: r.u64_le()?,
                conflict: r.flag()?,
                reopened: r.flag()?,
            })
        } else {
            None
        };
        let reopened = r.flag()?;
        let cancel_recorded = r.flag()?;
        let exposure_counted = r.flag()?;
        let exposure_released = r.flag()?;
        let ended = match r.u8()? {
            0 => None,
            1 => Some((End::Expired, r.u64_le()?)),
            2 => Some((End::Cancelled, r.u64_le()?)),
            _ => return Err(bad("an end is not known")),
        };
        let dust_logged = r.flag()?;
        let pos = match r.u8()? {
            0 => PosState::Nothing,
            1 => PosState::Paid,
            2 => PosState::Unpaid,
            _ => return Err(bad("a reported state is not known")),
        };
        let confirmed_logged = r.flag()?;
        let books_final = r.flag()?;
        let surplus_reported = r.u64_le()?;
        let late_unlisted = r.flag()?;
        let n = r.count(MAX_LATE_PAYMENTS, 40)?;
        let mut late: Vec<(Txid, u64)> = Vec::with_capacity(n);
        for _ in 0..n {
            let txid = Txid::from_byte_array(r.array::<32>()?);
            let sats = r.u64_le()?;
            if sats == 0 || late.iter().any(|(t, _)| *t == txid) {
                return Err(bad("late payments repeat or pay nothing"));
            }
            late.push((txid, sats));
        }
        let watch_len = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
        if watch_len > MAX_WATCH_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let watch = SaleWatch::from_bytes(r.take(watch_len)?)?;
        let mut refunds: Vec<RefundEntry> = Vec::new();
        let mut lightning = None;
        if version == SALE_VERSION_LIGHTNING {
            let payment_hash = r.array::<32>()?;
            let amount_msat = r.u64_le()?;
            if amount_msat == 0 {
                return Err(bad("a Lightning payment is for nothing"));
            }
            lightning = Some(LightningPaid {
                payment_hash,
                amount_msat,
                settled_at: r.opt_u64()?,
            });
        }
        if version == SALE_VERSION_WITH_REFUNDS || version == SALE_VERSION_LIGHTNING {
            let n = r.count(MAX_REFUNDS_PER_SALE, 28)?;
            if n == 0 && version == SALE_VERSION_WITH_REFUNDS {
                return Err(bad("version 2 holds no refund entry"));
            }
            for _ in 0..n {
                let entry = RefundEntry::read(&mut r)?;
                if entry.kind == crate::refund::EntryKind::Request
                    && refunds
                        .iter()
                        .any(|e| e.kind == entry.kind && e.number >= entry.number)
                {
                    return Err(bad("request numbers are not rising"));
                }
                refunds.push(entry);
            }
        }
        r.finish()?;

        if watch.script() != script.as_script() {
            return Err(bad("the watch is for another script"));
        }
        if watch.tolerance_bps() != tolerance_bps {
            return Err(bad("the watch has another tolerance"));
        }
        if cancel_recorded && accepted.is_none() && overridden.is_none() {
            return Err(bad(
                "a cancel is recorded on a sale that was never accepted",
            ));
        }
        if ended.is_some() && (accepted.is_some() || overridden.is_some()) {
            return Err(bad("an accepted sale has ended"));
        }
        if lightning.is_some() && (ended.is_some() || !books_final) {
            return Err(bad("a sale paid over Lightning has ended or is not final"));
        }
        Ok(Self {
            index,
            script,
            opened_at,
            amount,
            currency,
            tolerance_bps,
            quotes,
            segments,
            frozen,
            watch,
            accepted,
            overridden,
            reopened,
            cancel_recorded,
            exposure_counted,
            exposure_released,
            ended,
            dust_logged,
            pos,
            confirmed_logged,
            books_final,
            surplus_reported,
            late,
            late_unlisted,
            refunds,
            lightning,
        })
    }
}

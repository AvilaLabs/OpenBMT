//! The request book as bytes (RQ-30).
//!
//! Layout, all numbers little-endian: the magic `OBM=`, the version byte 1,
//! then
//!
//! ```text
//! next number u64 | next return id u64
//! stopped series: count u32, then each series u64, rising
//! requests: count u32, then each
//!   number u64 | note (len u16, UTF-8) | amount u64
//!   | pricing u8 (0 bitcoin, 1 currency) + currency byte for 1
//!   | made at u64 | made day u32 | due day u32 | address index u32
//!   | repeat flag, then every u8 (1 week, 2 month, 3 year), series u64,
//!     index u32, first due u32, lead u8
//!   | end flag, then kind u8 (1 cancelled, 2 closed, 3 counted paid), at u64
//!   | quotes: count u32, then made at u64, owed u64, sats u64,
//!     rate u64 (0 for none), until u64 (0 for none)
//!   | credits: count u32, then txid 32, vout u32, height u32,
//!     block time u64, sats u64, value u64, rate u64 (0 for none),
//!     flags u8 (1 late, 2 old price, 4 after a cancel, 8 after a close)
//! returns: count u32, then each
//!   id u64 | request u64 | script (len u8, bytes) | sats u64 | queued at u64
//!   | state u8 (0 queued, 1 sent + txid 32, 2 done + txid 32 + at u64)
//! ```
//!
//! The requests are numbered 1, 2, 3 and so on with none missing. The
//! returns' ids count up, with a gap where a return left the queue
//! (RQ-37). The read is strict: another magic or version, a count
//! over its limit, a field out of range or that does not agree with
//! another, and bytes left over are refused, and what decodes encodes to
//! the same bytes.

use std::collections::HashSet;

use bitcoin::ScriptBuf;
use openbmt_core::currency::ShopCurrency;
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_counter::money::{sats_for, value_of, Amount, Rate, MAX_SATS};
use openbmt_counter::refund::{dust_limit, is_payment_script, MAX_SCRIPT_BYTES};

use crate::book::{RequestBook, MAX_REQUESTS, MAX_RETURNS};
use crate::date::{Day, MAX_DAY};
use crate::error::BookError;
use crate::model::{
    note_ok, After, Credit, End, Every, Pricing, Quote, Repeat, Request, Return, ReturnState,
    MAX_CREDITS, MAX_CURRENCY_UNITS, MAX_DUE_DAYS, MAX_LEAD_DAYS, MAX_OPEN, MAX_QUOTES,
    QUOTE_HOLD_SECS,
};
use crate::returns::MAX_QUEUED_RETURNS;

/// The magic of the request book (RQ-30).
pub const MAGIC_REQUEST_BOOK: [u8; 4] = *b"OBM=";

/// The fewest bytes a request takes: its fixed fields, an empty note, no
/// repeat, no end, no quotes and no credits.
const MIN_REQUEST_BYTES: usize = 50;
/// The fewest bytes a return takes.
const MIN_RETURN_BYTES: usize = 35;

fn corrupt(e: DecodeError) -> BookError {
    match e {
        DecodeError::NonCanonical(why) => BookError::Corrupt(why),
        DecodeError::TrailingBytes => BookError::Corrupt("bytes follow the last return"),
        DecodeError::TooLarge => BookError::Corrupt("a count is over its limit"),
        DecodeError::BadMagic => BookError::Corrupt("it is not a request book"),
        DecodeError::UnsupportedVersion(_) => BookError::Corrupt("its version is not read"),
        _ => BookError::Corrupt("it is cut short"),
    }
}

fn bad<T>(why: &'static str) -> Result<T, BookError> {
    Err(BookError::Corrupt(why))
}

fn rate_or_zero(rate: Option<Rate>) -> u64 {
    rate.map_or(0, |r| r.minor_per_btc())
}

fn rate_of(raw: u64) -> Option<Rate> {
    Rate::new(raw)
}

impl RequestBook {
    /// The book as bytes (RQ-30).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_REQUEST_BOOK, 64 + self.requests.len() * 120);
        out.extend_from_slice(&self.next_number.to_le_bytes());
        out.extend_from_slice(&self.next_return.to_le_bytes());
        out.extend_from_slice(&wire::len_u32(self.stopped.len()).to_le_bytes());
        for s in &self.stopped {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out.extend_from_slice(&wire::len_u32(self.requests.len()).to_le_bytes());
        for r in &self.requests {
            encode_request(&mut out, r);
        }
        out.extend_from_slice(&wire::len_u32(self.returns.len()).to_le_bytes());
        for r in &self.returns {
            encode_return(&mut out, r);
        }
        out
    }

    /// Read the book strictly (RQ-30, RQ-34): any bytes at all give a book
    /// or a refusal, never a panic.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BookError> {
        let mut r = Reader::framed(bytes, &MAGIC_REQUEST_BOOK).map_err(corrupt)?;
        let next_number = r.u64_le().map_err(corrupt)?;
        let next_return = r.u64_le().map_err(corrupt)?;
        let n = r.count(MAX_REQUESTS, 8).map_err(corrupt)?;
        let mut stopped = Vec::with_capacity(n);
        for _ in 0..n {
            let s = r.u64_le().map_err(corrupt)?;
            if stopped.last().is_some_and(|&l| l >= s) {
                return bad("the stopped series are not in order");
            }
            stopped.push(s);
        }
        let n = r.count(MAX_REQUESTS, MIN_REQUEST_BYTES).map_err(corrupt)?;
        let mut requests: Vec<Request> = Vec::with_capacity(n);
        let mut indexes: HashSet<u32> = HashSet::with_capacity(n);
        for at in 0..n {
            let request = decode_request(&mut r)?;
            if request.number != at as u64 + 1 {
                return bad("the requests are not numbered 1, 2, 3 and so on");
            }
            if !indexes.insert(request.address_index) {
                return bad("an address index is used by two requests");
            }
            requests.push(request);
        }
        if next_number != n as u64 + 1 {
            return bad("the next number is not the one after the last request");
        }
        if requests.iter().filter(|r| r.is_open()).count() > MAX_OPEN {
            return bad("more requests are open than may be");
        }
        for request in &requests {
            if let Some(rep) = request.repeat {
                if rep.index > 0 && rep.series >= request.number {
                    return bad("a repeat names a series that has not begun");
                }
                if rep.index > 0 {
                    // The series starts with a first request of the same
                    // cadence, first due day and lead.
                    let root = usize::try_from(rep.series)
                        .ok()
                        .and_then(|s| s.checked_sub(1))
                        .and_then(|s| requests.get(s))
                        .and_then(|r| r.repeat);
                    let same = root.is_some_and(|root| {
                        root.index == 0
                            && root.every == rep.every
                            && root.first_due == rep.first_due
                            && root.lead == rep.lead
                    });
                    if !same {
                        return bad("a repeat is not of the series it names");
                    }
                }
            }
        }
        for s in &stopped {
            let begun = usize::try_from(*s)
                .ok()
                .and_then(|s| s.checked_sub(1))
                .and_then(|s| requests.get(s))
                .and_then(|r| r.repeat)
                .is_some_and(|rep| rep.index == 0 && rep.series == *s);
            if !begun {
                return bad("a stopped series is not one that repeats");
            }
        }
        let n = r.count(MAX_RETURNS, MIN_RETURN_BYTES).map_err(corrupt)?;
        let mut returns: Vec<Return> = Vec::with_capacity(n);
        // Per request, by number less one: the satoshis returned in every
        // state, and whether any return is still queued. One pass over the
        // returns, whatever the number of requests.
        let mut returned: Vec<(u64, bool)> = vec![(0, false); requests.len()];
        for _ in 0..n {
            let ret = decode_return(&mut r)?;
            // Ids only count up: a return taken off the queue (RQ-37) leaves
            // a gap.
            if returns.last().is_some_and(|l| l.id >= ret.id) || ret.id == 0 {
                return bad("the returns are not in the order of their numbers");
            }
            let Some(sum) = usize::try_from(ret.request)
                .ok()
                .and_then(|q| q.checked_sub(1))
                .and_then(|q| returned.get_mut(q))
            else {
                return bad("a return is from a request that is not in the book");
            };
            sum.0 = sum.0.saturating_add(ret.sats);
            sum.1 |= ret.state == ReturnState::Queued;
            returns.push(ret);
        }
        if returns.last().is_some_and(|l| next_return <= l.id) || next_return == u64::MAX {
            return bad("the next return is not after the last");
        }
        r.finish().map_err(corrupt)?;
        let in_flight = returns
            .iter()
            .filter(|r| matches!(r.state, ReturnState::Queued | ReturnState::Sent { .. }))
            .count();
        if in_flight > MAX_QUEUED_RETURNS {
            return bad("more returns are queued or in a spend than one payment takes");
        }
        // More returned than received is only a book after a reorg (RQ-37),
        // and then none of that request's returns wait in the queue.
        for (request, (total, queued)) in requests.iter().zip(&returned) {
            if *total > request.received_sats() && *queued {
                return bad("more was returned from a request than it received");
            }
        }
        let book = Self {
            next_number,
            requests,
            next_return,
            returns,
            stopped,
        };
        if book.to_bytes() != bytes {
            return bad("it does not encode to the same bytes");
        }
        Ok(book)
    }
}

fn encode_request(out: &mut Vec<u8>, r: &Request) {
    out.extend_from_slice(&r.number.to_le_bytes());
    out.extend_from_slice(
        &u16::try_from(r.note.len())
            .unwrap_or(u16::MAX)
            .to_le_bytes(),
    );
    out.extend_from_slice(r.note.as_bytes());
    out.extend_from_slice(&r.amount.to_le_bytes());
    match r.pricing {
        Pricing::Bitcoin => out.push(0),
        Pricing::Currency(c) => out.extend_from_slice(&[1, c.to_byte()]),
    }
    out.extend_from_slice(&r.made_at.to_le_bytes());
    out.extend_from_slice(&r.made.0.to_le_bytes());
    out.extend_from_slice(&r.due.0.to_le_bytes());
    out.extend_from_slice(&r.address_index.to_le_bytes());
    match r.repeat {
        None => out.push(0),
        Some(rep) => {
            out.push(1);
            out.push(match rep.every {
                Every::Week => 1,
                Every::Month => 2,
                Every::Year => 3,
            });
            out.extend_from_slice(&rep.series.to_le_bytes());
            out.extend_from_slice(&rep.index.to_le_bytes());
            out.extend_from_slice(&rep.first_due.0.to_le_bytes());
            out.push(rep.lead);
        }
    }
    match r.end {
        None => out.push(0),
        Some((end, at)) => {
            out.push(1);
            out.push(match end {
                End::Cancelled => 1,
                End::Closed => 2,
                End::CountedPaid => 3,
            });
            out.extend_from_slice(&at.to_le_bytes());
        }
    }
    out.extend_from_slice(&wire::len_u32(r.quotes.len()).to_le_bytes());
    for q in &r.quotes {
        out.extend_from_slice(&q.made_at.to_le_bytes());
        out.extend_from_slice(&q.owed.to_le_bytes());
        out.extend_from_slice(&q.sats.to_le_bytes());
        out.extend_from_slice(&rate_or_zero(q.rate).to_le_bytes());
        out.extend_from_slice(&q.until.unwrap_or(0).to_le_bytes());
    }
    out.extend_from_slice(&wire::len_u32(r.credits.len()).to_le_bytes());
    for c in &r.credits {
        out.extend_from_slice(&c.txid);
        out.extend_from_slice(&c.vout.to_le_bytes());
        out.extend_from_slice(&c.height.to_le_bytes());
        out.extend_from_slice(&c.block_time.to_le_bytes());
        out.extend_from_slice(&c.sats.to_le_bytes());
        out.extend_from_slice(&c.value.to_le_bytes());
        out.extend_from_slice(&rate_or_zero(c.rate).to_le_bytes());
        let flags = u8::from(c.late)
            | u8::from(c.old_price) << 1
            | match c.after {
                None => 0,
                Some(After::Cancel) => 4,
                Some(After::Close) => 8,
            };
        out.push(flags);
    }
}

fn encode_return(out: &mut Vec<u8>, r: &Return) {
    out.extend_from_slice(&r.id.to_le_bytes());
    out.extend_from_slice(&r.request.to_le_bytes());
    let script = r.script.as_bytes();
    out.push(u8::try_from(script.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(script);
    out.extend_from_slice(&r.sats.to_le_bytes());
    out.extend_from_slice(&r.queued_at.to_le_bytes());
    match r.state {
        ReturnState::Queued => out.push(0),
        ReturnState::Sent { txid } => {
            out.push(1);
            out.extend_from_slice(&txid);
        }
        ReturnState::Done { txid, at } => {
            out.push(2);
            out.extend_from_slice(&txid);
            out.extend_from_slice(&at.to_le_bytes());
        }
    }
}

fn day(r: &mut Reader<'_>) -> Result<Day, BookError> {
    let d = r.u32_le().map_err(corrupt)?;
    if d > MAX_DAY.0 {
        return bad("a day is out of range");
    }
    Ok(Day(d))
}

fn decode_request(r: &mut Reader<'_>) -> Result<Request, BookError> {
    let number = r.u64_le().map_err(corrupt)?;
    let len = usize::from(r.u16_le().map_err(corrupt)?);
    let note = std::str::from_utf8(r.take(len).map_err(corrupt)?)
        .map_err(|_| BookError::Corrupt("a note is not text"))?
        .to_string();
    if !note_ok(&note) {
        return bad("a note is not 1 to 64 characters of plain text");
    }
    let amount = r.u64_le().map_err(corrupt)?;
    let pricing = match r.u8().map_err(corrupt)? {
        0 => Pricing::Bitcoin,
        1 => match ShopCurrency::from_byte(r.u8().map_err(corrupt)?) {
            Some(c) if !c.is_sats() => Pricing::Currency(c),
            _ => return bad("a request is priced in a currency that is not one"),
        },
        _ => return bad("a request is priced in no known way"),
    };
    let limit = match pricing {
        Pricing::Bitcoin => MAX_SATS,
        Pricing::Currency(c) => MAX_CURRENCY_UNITS * 10u64.pow(c.minor_digits()),
    };
    if amount == 0 || amount > limit {
        return bad("an amount is out of range");
    }
    let made_at = r.u64_le().map_err(corrupt)?;
    let made = day(r)?;
    let due = day(r)?;
    let address_index = r.u32_le().map_err(corrupt)?;
    let repeat = if r.flag().map_err(corrupt)? {
        let every = match r.u8().map_err(corrupt)? {
            1 => Every::Week,
            2 => Every::Month,
            3 => Every::Year,
            _ => return bad("a repeat is every something that is not known"),
        };
        let series = r.u64_le().map_err(corrupt)?;
        let index = r.u32_le().map_err(corrupt)?;
        let first_due = day(r)?;
        let lead = r.u8().map_err(corrupt)?;
        if u32::from(lead) > MAX_LEAD_DAYS {
            return bad("a repeat's lead is over 31 days");
        }
        if every.due(first_due, index) != Some(due) {
            return bad("a repeat's due date is not the one its series gives");
        }
        if index == 0 && (series != number || first_due != due) {
            return bad("the first request of a series is not its own series");
        }
        if index == 0 && due.0.saturating_sub(made.0).min(MAX_LEAD_DAYS) != u32::from(lead) {
            return bad("a repeat's lead is not its due date less the day it was made");
        }
        Some(Repeat {
            every,
            series,
            index,
            first_due,
            lead,
        })
    } else {
        None
    };
    // A request made by hand is due from the day it is made to 366 days on.
    if repeat.is_none_or(|rep| rep.index == 0) && (due < made || due.0 - made.0 > MAX_DUE_DAYS) {
        return bad("a due date is not within 366 days of the day it was made");
    }
    let end = if r.flag().map_err(corrupt)? {
        let kind = match r.u8().map_err(corrupt)? {
            1 => End::Cancelled,
            2 => End::Closed,
            3 => End::CountedPaid,
            _ => return bad("a request ended in no known way"),
        };
        if kind == End::CountedPaid && pricing == Pricing::Bitcoin {
            return bad("a request in bitcoin was counted as paid");
        }
        Some((kind, r.u64_le().map_err(corrupt)?))
    } else {
        None
    };
    let n = r.count(MAX_QUOTES, 40).map_err(corrupt)?;
    let mut quotes: Vec<Quote> = Vec::with_capacity(n);
    for _ in 0..n {
        let made_at = r.u64_le().map_err(corrupt)?;
        let owed = r.u64_le().map_err(corrupt)?;
        let sats = r.u64_le().map_err(corrupt)?;
        let rate = r.u64_le().map_err(corrupt)?;
        let until = r.u64_le().map_err(corrupt)?;
        let held = match pricing {
            Pricing::Bitcoin => rate == 0 && until == 0 && sats == owed,
            Pricing::Currency(_) => rate != 0 && until == made_at.saturating_add(QUOTE_HOLD_SECS),
        };
        if !held || owed == 0 || owed > amount || sats > MAX_SATS {
            return bad("a quote does not fit its request");
        }
        // A quote in the currency asks what its amount and rate give.
        if rate != 0 && rate_of(rate).and_then(|rt| sats_for(Amount(owed), rt)) != Some(sats) {
            return bad("a quote's satoshis are not what its amount and rate give");
        }
        if quotes.last().is_some_and(|l| l.made_at > made_at) {
            return bad("the quotes are not in the order they were made");
        }
        quotes.push(Quote {
            made_at,
            owed,
            sats,
            rate: rate_of(rate),
            until: (until != 0).then_some(until),
        });
    }
    let n = r.count(MAX_CREDITS, 73).map_err(corrupt)?;
    let mut credits: Vec<Credit> = Vec::with_capacity(n);
    let mut seen: HashSet<([u8; 32], u32)> = HashSet::with_capacity(n);
    for _ in 0..n {
        let txid = r.array::<32>().map_err(corrupt)?;
        let vout = r.u32_le().map_err(corrupt)?;
        let height = r.u32_le().map_err(corrupt)?;
        let block_time = r.u64_le().map_err(corrupt)?;
        let sats = r.u64_le().map_err(corrupt)?;
        let value = r.u64_le().map_err(corrupt)?;
        let rate = r.u64_le().map_err(corrupt)?;
        let flags = r.u8().map_err(corrupt)?;
        if flags > 15 || flags & 12 == 12 {
            return bad("a payment's flags are not any that are written");
        }
        let after = match flags & 12 {
            4 => Some(After::Cancel),
            8 => Some(After::Close),
            _ => None,
        };
        let old_price = flags & 2 != 0;
        if sats == 0 || sats > MAX_SATS || !seen.insert((txid, vout)) {
            return bad("a payment is empty, or is there twice");
        }
        let fits = match (after, pricing) {
            (Some(_), _) => value == 0 && rate == 0 && !old_price,
            (None, Pricing::Bitcoin) => value == sats && rate == 0 && !old_price,
            (None, Pricing::Currency(_)) => {
                rate_of(rate).is_some_and(|rt| value_of(sats, rt).0 == value)
            }
        };
        if !fits {
            return bad("a payment's credit is not what its request credits");
        }
        let ended = matches!(
            (after, end),
            (None, _)
                | (Some(After::Cancel), Some((End::Cancelled, _)))
                | (
                    Some(After::Close),
                    Some((End::Closed | End::CountedPaid, _))
                )
        );
        if !ended
            || credits
                .last()
                .is_some_and(|l| l.after.is_some() && after.is_none())
        {
            return bad("a payment is marked as after an end the request did not have");
        }
        credits.push(Credit {
            txid,
            vout,
            height,
            block_time,
            sats,
            value,
            rate: rate_of(rate),
            late: flags & 1 != 0,
            old_price,
            after,
        });
    }
    if matches!(end, Some((End::Cancelled, _))) && credits.iter().any(|c| c.after.is_none()) {
        return bad("a request that was cancelled has a payment from before");
    }
    Ok(Request {
        number,
        note,
        amount,
        pricing,
        made_at,
        made,
        due,
        address_index,
        repeat,
        end,
        quotes,
        credits,
    })
}

fn decode_return(r: &mut Reader<'_>) -> Result<Return, BookError> {
    let id = r.u64_le().map_err(corrupt)?;
    let request = r.u64_le().map_err(corrupt)?;
    let len = usize::from(r.u8().map_err(corrupt)?);
    if len > MAX_SCRIPT_BYTES {
        return bad("a return's script is too long");
    }
    let script = ScriptBuf::from_bytes(r.take(len).map_err(corrupt)?.to_vec());
    let min = match dust_limit(&script) {
        Some(min) if is_payment_script(&script) => min,
        _ => return bad("a return is to a script of no standard kind"),
    };
    let sats = r.u64_le().map_err(corrupt)?;
    if sats < min || sats > MAX_SATS {
        return bad("a return's amount is out of range");
    }
    let queued_at = r.u64_le().map_err(corrupt)?;
    let state = match r.u8().map_err(corrupt)? {
        0 => ReturnState::Queued,
        1 => ReturnState::Sent {
            txid: r.array::<32>().map_err(corrupt)?,
        },
        2 => ReturnState::Done {
            txid: r.array::<32>().map_err(corrupt)?,
            at: r.u64_le().map_err(corrupt)?,
        },
        _ => return bad("a return is in no known state"),
    };
    Ok(Return {
        id,
        request,
        script,
        sats,
        queued_at,
        state,
    })
}

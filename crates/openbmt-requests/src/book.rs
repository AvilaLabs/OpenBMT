//! The request book: every request, its quotes and payments, the returns
//! and the series that repeat (RQ-30). This file makes, shares, credits,
//! cancels and closes; the list, the repeats and the returns are in their
//! own files, on the same type.
//!
//! The book keeps no clock, no rate and no address of its own: the caller
//! passes the time, a checked rate and the index of the desk's next address
//! (RQ-03). The desk's allocation (RQ-02, RQ-04) is the owner state's.

use openbmt_core::currency::ShopCurrency;
use openbmt_counter::money::{sats_for, value_of, Amount, Rate, MAX_SATS};
use openbmt_counter::refund::refund_text_ok;

use crate::date::{day_of, end_of_day, Day, MAX_DAY};
use crate::error::{CreditRefused, EndRefused, Limit, MakeRefused, ShareRefused};
use crate::model::Arrival;
use crate::model::{
    After, CheckedRate, Credit, End, Every, Pricing, Quote, Repeat, Request, Return, Status,
    MAX_CREDITS, MAX_CURRENCY_UNITS, MAX_DUE_DAYS, MAX_LEAD_DAYS, MAX_OPEN, MAX_QUOTES,
    QUOTE_HOLD_SECS,
};

/// Most requests the book holds, ended or not. (A limit of this library,
/// for the size of the file: the rules name none.)
pub const MAX_REQUESTS: usize = 100_000;
/// Most returns the book holds.
pub const MAX_RETURNS: usize = 100_000;

/// What the shop's settings say that a request depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shop {
    /// The shop's currency (CR-193); Bitcoin Standard has requests in
    /// bitcoin only.
    pub currency: ShopCurrency,
    /// The smallest bitcoin sale in satoshis; 0 for no floor (CR-75).
    pub min_sale_sats: u64,
    /// The shop's time zone, in seconds east of UTC.
    pub tz_offset_secs: i32,
}

/// How the owner chose to price a request (RQ-07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceIn {
    /// In satoshis.
    Bitcoin,
    /// In the currency of the shop's settings.
    ShopCurrency,
}

/// What the owner gives when making a request (RQ-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRequest {
    /// The note (RQ-06).
    pub note: String,
    /// The amount: satoshis, or minor units of the shop's currency.
    pub amount: u64,
    /// How it is priced.
    pub price_in: PriceIn,
    /// The due date (RQ-09).
    pub due: Day,
    /// How often it repeats, if it does (RQ-24).
    pub repeat: Option<Every>,
}

/// The request book (RQ-30). The fields are read through methods; only the
/// calls below change them, so the book stays one the decoder would accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestBook {
    pub(crate) next_number: u64,
    pub(crate) requests: Vec<Request>,
    pub(crate) next_return: u64,
    pub(crate) returns: Vec<Return>,
    pub(crate) stopped: Vec<u64>,
}

impl Default for RequestBook {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestBook {
    /// A book with nothing in it: the first request is No. 1.
    pub fn new() -> Self {
        Self {
            next_number: 1,
            requests: Vec::new(),
            next_return: 1,
            returns: Vec::new(),
            stopped: Vec::new(),
        }
    }

    /// The number the next request gets (RQ-05).
    pub fn next_number(&self) -> u64 {
        self.next_number
    }

    /// Every request, by number.
    pub fn requests(&self) -> &[Request] {
        &self.requests
    }

    /// One request by its number.
    pub fn request(&self, number: u64) -> Option<&Request> {
        self.requests
            .binary_search_by_key(&number, |r| r.number)
            .ok()
            .map(|at| &self.requests[at])
    }

    pub(crate) fn request_mut(&mut self, number: u64) -> Option<&mut Request> {
        let at = self
            .requests
            .binary_search_by_key(&number, |r| r.number)
            .ok()?;
        Some(&mut self.requests[at])
    }

    /// Every return, by id.
    pub fn returns(&self) -> &[Return] {
        &self.returns
    }

    /// The series the owner stopped (RQ-25), by number.
    pub fn stopped_series(&self) -> &[u64] {
        &self.stopped
    }

    /// How many requests are open (RQ-11).
    pub fn open_count(&self) -> usize {
        self.requests.iter().filter(|r| r.is_open()).count()
    }

    /// Whether some request has this address index (RQ-03).
    pub fn index_used(&self, index: u32) -> bool {
        self.requests.iter().any(|r| r.address_index == index)
    }

    /// RQ-05 to RQ-11, RQ-03: make a request with the desk's next address
    /// index, which the caller took from the allocation and saved first.
    /// Returns the number. Nothing changes on a refusal.
    ///
    /// `rate` is needed for a request in the currency, to work out the
    /// satoshis for the smallest-sale check; a rate that is missing or not
    /// fresh at `now` is a refusal (RQ-12).
    pub fn make(
        &mut self,
        new: &NewRequest,
        address_index: u32,
        now: u64,
        shop: &Shop,
        rate: Option<&CheckedRate>,
    ) -> Result<u64, MakeRefused> {
        if !refund_text_ok(&new.note) {
            return Err(MakeRefused::NoteNeeded);
        }
        let pricing = match new.price_in {
            PriceIn::Bitcoin => Pricing::Bitcoin,
            PriceIn::ShopCurrency if shop.currency.is_sats() => {
                return Err(MakeRefused::BitcoinOnly)
            }
            PriceIn::ShopCurrency => Pricing::Currency(shop.currency),
        };
        let made = day_of(now, shop.tz_offset_secs);
        if new.due < made || new.due.0 - made.0 > MAX_DUE_DAYS || new.due > MAX_DAY {
            return Err(MakeRefused::DueOutOfRange);
        }
        let floor = Request::floor_sats(shop.min_sale_sats);
        let sats = match pricing {
            Pricing::Bitcoin => new.amount,
            Pricing::Currency(currency) => {
                let units = 10u64.pow(currency.minor_digits());
                if new.amount > MAX_CURRENCY_UNITS * units {
                    return Err(MakeRefused::TooLarge {
                        max: Limit::Units(currency, MAX_CURRENCY_UNITS),
                    });
                }
                if new.amount == 0 {
                    return Err(MakeRefused::TooSmall { min_sats: floor });
                }
                let rate = rate
                    .and_then(|r| r.fresh_at(now))
                    .ok_or(MakeRefused::StaleRate)?;
                sats_for(Amount(new.amount), rate).ok_or(MakeRefused::TooLarge {
                    max: Limit::Units(currency, MAX_CURRENCY_UNITS),
                })?
            }
        };
        if sats > MAX_SATS {
            return Err(MakeRefused::TooLarge {
                max: Limit::Sats(MAX_SATS),
            });
        }
        if sats < floor {
            return Err(MakeRefused::TooSmall { min_sats: floor });
        }
        if self.open_count() >= MAX_OPEN {
            return Err(MakeRefused::TooManyOpen);
        }
        if self.index_used(address_index) {
            return Err(MakeRefused::IndexUsed);
        }
        if self.requests.len() >= MAX_REQUESTS {
            return Err(MakeRefused::BookFull);
        }
        let number = self.next_number;
        let repeat = new.repeat.map(|every| Repeat {
            every,
            series: number,
            index: 0,
            first_due: new.due,
            lead: u8::try_from((new.due.0 - made.0).min(MAX_LEAD_DAYS)).unwrap_or(31),
        });
        self.requests.push(Request {
            number,
            note: new.note.clone(),
            amount: new.amount,
            pricing,
            made_at: now,
            made,
            due: new.due,
            address_index,
            repeat,
            end: None,
            quotes: Vec::new(),
            credits: Vec::new(),
        });
        self.next_number += 1;
        Ok(number)
    }

    /// RQ-12, RQ-14: make a quote of what is still owed and write it to the
    /// request. The caller saves the book before anything is shared. A
    /// quote for a request in the currency needs a rate fresh at `now`; a
    /// stale or missing one is a refusal and nothing is written.
    pub fn share(
        &mut self,
        number: u64,
        now: u64,
        rate: Option<&CheckedRate>,
    ) -> Result<Quote, ShareRefused> {
        let request = self.request(number).ok_or(ShareRefused::NoSuchRequest)?;
        if !request.is_open() {
            return Err(ShareRefused::NotOpen);
        }
        let owed = request.owed();
        let quote = match request.pricing {
            Pricing::Bitcoin => Quote {
                made_at: now,
                owed,
                sats: owed,
                rate: None,
                until: None,
            },
            Pricing::Currency(_) => {
                let rate = rate
                    .and_then(|r| r.fresh_at(now))
                    .ok_or(ShareRefused::StaleRate)?;
                Quote {
                    made_at: now,
                    owed,
                    sats: sats_for(Amount(owed), rate).ok_or(ShareRefused::StaleRate)?,
                    rate: Some(rate),
                    until: Some(now.saturating_add(QUOTE_HOLD_SECS)),
                }
            }
        };
        let request = self
            .request_mut(number)
            .ok_or(ShareRefused::NoSuchRequest)?;
        // Quotes stay in the order they were made, whatever the clock does.
        let mut quote = quote;
        if let Some(last) = request.quotes.last() {
            if quote.made_at < last.made_at {
                let shift = last.made_at - quote.made_at;
                quote.made_at = last.made_at;
                quote.until = quote.until.map(|u| u.saturating_add(shift));
            }
        }
        request.quotes.push(quote);
        if request.quotes.len() > MAX_QUOTES {
            request.quotes.remove(0);
        }
        Ok(quote)
    }

    /// RQ-15 to RQ-18: count the outputs paid to a request's address, in the
    /// order of the blocks. An output already counted (the same transaction
    /// and output number) is skipped, so the owner app may hand over the
    /// whole of what it has seen each time. Returns how many were counted.
    /// Nothing changes on a refusal.
    ///
    /// `rate` values a payment in the currency when the request has no quote
    /// at all; it is not used otherwise.
    pub fn credit(
        &mut self,
        number: u64,
        arrivals: &[Arrival],
        now: u64,
        shop: &Shop,
        rate: Option<&CheckedRate>,
    ) -> Result<usize, CreditRefused> {
        let request = self.request(number).ok_or(CreditRefused::NoSuchRequest)?;
        if arrivals.windows(2).any(|w| w[1].height < w[0].height) {
            return Err(CreditRefused::NotInBlockOrder);
        }
        let now_rate = rate.and_then(|r| r.fresh_at(now));
        let after = match request.end {
            None => None,
            Some((End::Cancelled, _)) => Some(After::Cancel),
            Some((End::Closed | End::CountedPaid, _)) => Some(After::Close),
        };
        let mut credits = request.credits.clone();
        let mut counted = 0usize;
        for a in arrivals {
            if credits.iter().any(|c| c.txid == a.txid && c.vout == a.vout) {
                continue;
            }
            if credits.len() >= MAX_CREDITS {
                return Err(CreditRefused::TooManyPayments);
            }
            let (value, rate_used, old_price) = match (after, request.pricing) {
                (Some(_), _) => (0, None, false),
                (None, Pricing::Bitcoin) => (a.sats, None, false),
                (None, Pricing::Currency(_)) => {
                    // The newest quote shared before the block; if it had run
                    // out, or there was none, the newest of all, or the rate
                    // of now, and the owner is shown it (RQ-15, RQ-18).
                    let before = request
                        .quotes
                        .iter()
                        .rev()
                        .find(|q| q.made_at <= a.block_time);
                    let (rate, old) = match before {
                        Some(q) => (q.rate, !q.holds_at(a.block_time)),
                        None => (
                            request.quotes.last().and_then(|q| q.rate).or(now_rate),
                            true,
                        ),
                    };
                    let rate: Rate = rate.ok_or(CreditRefused::NoRate)?;
                    (value_of(a.sats, rate).0, Some(rate), old)
                }
            };
            credits.push(Credit {
                txid: a.txid,
                vout: a.vout,
                height: a.height,
                block_time: a.block_time,
                sats: a.sats,
                value,
                rate: rate_used,
                late: a.block_time >= end_of_day(request.due, shop.tz_offset_secs),
                old_price,
                after,
            });
            counted += 1;
        }
        if counted > 0 {
            self.request_mut(number)
                .ok_or(CreditRefused::NoSuchRequest)?
                .credits = credits;
        }
        Ok(counted)
    }

    /// RQ-19: cancel a request that has received nothing. Its address stays
    /// watched and a payment after it is offered for return.
    pub fn cancel(&mut self, number: u64, now: u64) -> Result<(), EndRefused> {
        let request = self.request_mut(number).ok_or(EndRefused::NoSuchRequest)?;
        if request.end.is_some() {
            return Err(EndRefused::AlreadyEnded);
        }
        if !request.credits.is_empty() {
            return Err(EndRefused::HasPayments);
        }
        request.end = Some((End::Cancelled, now));
        Ok(())
    }

    /// RQ-20: close a part paid request as it stands.
    pub fn close(&mut self, number: u64, now: u64, shop: &Shop) -> Result<(), EndRefused> {
        let request = self.request_mut(number).ok_or(EndRefused::NoSuchRequest)?;
        if request.end.is_some() {
            return Err(EndRefused::AlreadyEnded);
        }
        if !matches!(
            request.status(shop.min_sale_sats),
            Status::PartPaid { .. } | Status::Almost { .. }
        ) {
            return Err(EndRefused::NotPartPaid);
        }
        request.end = Some((End::Closed, now));
        Ok(())
    }

    /// RQ-16: count an almost paid request as paid. It is recorded and
    /// cannot be undone.
    pub fn count_as_paid(&mut self, number: u64, now: u64, shop: &Shop) -> Result<(), EndRefused> {
        let request = self.request_mut(number).ok_or(EndRefused::NoSuchRequest)?;
        if request.end.is_some() {
            return Err(EndRefused::AlreadyEnded);
        }
        if !matches!(request.status(shop.min_sale_sats), Status::Almost { .. }) {
            return Err(EndRefused::NotAlmost);
        }
        request.end = Some((End::CountedPaid, now));
        Ok(())
    }
}

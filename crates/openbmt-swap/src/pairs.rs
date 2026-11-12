//! The service's live terms, read from `GET /v2/swap/reverse` (SW-60 to
//! SW-63). Only the BTC to BTC pair is read; every other pair is ignored.

use openbmt_json::{Limits, Value};

use crate::error::Refused;
use crate::{MAX_HTTP_BYTES, MAX_INVOICE_SATS, MIN_INVOICE_SATS, PAIRS_FRESH_SECS};

/// The deepest nesting of a pairs reply that is read: the BTC to BTC pair
/// sits at depth 5, and the other pairs have room to grow.
const PAIRS_DEPTH: usize = 8;
/// The most the percentage fee can be, in parts per million: 10 percent.
const MAX_FEE_PPM: u32 = 100_000;

/// The service's terms for a reverse swap from BTC to BTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Terms {
    /// The least invoice amount the service takes, in satoshis.
    pub min_sats: u64,
    /// The most invoice amount the service takes, in satoshis.
    pub max_sats: u64,
    /// The percentage fee, in parts per million of the invoice amount.
    pub fee_ppm: u32,
    /// The miner fee the service counts for the claim, in satoshis.
    pub claim_fee_sats: u64,
    /// The miner fee the service counts for its lockup, in satoshis.
    pub lockup_fee_sats: u64,
}

/// The terms and the time they were read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pairs {
    terms: Terms,
    read_at: u64,
}

fn get<'a>(v: &'a Value, name: &'static str) -> Result<&'a Value, Refused> {
    v.get(name).ok_or(Refused::BadPairs(name))
}

fn whole(v: &Value, name: &'static str) -> Result<u64, Refused> {
    get(v, name)?.as_u64().ok_or(Refused::BadPairs(name))
}

/// SW-61: a percentage as parts per million, from the text of the number
/// and from nothing else. At most 4 decimal places, no exponent, not
/// negative, at most 10 percent.
pub(crate) fn percentage_ppm(v: &Value) -> Result<u32, Refused> {
    let ppm = match v {
        Value::Int(i) => u32::try_from(*i)
            .ok()
            .and_then(|p| p.checked_mul(10_000))
            .ok_or(Refused::BadPercentage)?,
        Value::Number(text) => decimal_ppm(text)?,
        _ => return Err(Refused::BadPercentage),
    };
    if ppm > MAX_FEE_PPM {
        return Err(Refused::BadPercentage);
    }
    Ok(ppm)
}

/// The text of a JSON decimal (`12`, `0.6`, `10.0`) times 10,000, which is
/// the percentage in parts per million.
fn decimal_ppm(text: &str) -> Result<u32, Refused> {
    let (whole_part, fraction) = text.split_once('.').unwrap_or((text, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole_part.is_empty()
        || !digits(whole_part)
        || !digits(fraction)
        || fraction.len() > 4
        || whole_part.len() > 3
    {
        return Err(Refused::BadPercentage);
    }
    let int: u32 = whole_part.parse().map_err(|_| Refused::BadPercentage)?;
    let mut ppm = int.checked_mul(10_000).ok_or(Refused::BadPercentage)?;
    let mut scale = 1_000;
    for d in fraction.bytes() {
        ppm = ppm
            .checked_add(u32::from(d - b'0') * scale)
            .ok_or(Refused::BadPercentage)?;
        scale /= 10;
    }
    Ok(ppm)
}

impl Pairs {
    /// SW-60: read the body of the reply to `GET /v2/swap/reverse`, taken
    /// at `read_at` (seconds since 1970).
    pub fn parse(body: &[u8], read_at: u64) -> Result<Self, Refused> {
        let limits = Limits {
            max_bytes: MAX_HTTP_BYTES,
            max_depth: PAIRS_DEPTH,
            ..Limits::DEFAULT
        };
        let doc = openbmt_json::parse(body, &limits).map_err(|_| Refused::BadPairs("document"))?;
        let pair = doc
            .get("BTC")
            .and_then(|p| p.get("BTC"))
            .ok_or(Refused::BadPairs("BTC/BTC"))?;
        let limits = get(pair, "limits")?;
        let fees = get(pair, "fees")?;
        let miner = get(fees, "minerFees")?;
        let terms = Terms {
            min_sats: whole(limits, "minimal")?,
            max_sats: whole(limits, "maximal")?,
            fee_ppm: percentage_ppm(get(fees, "percentage")?)?,
            claim_fee_sats: whole(miner, "claim")?,
            lockup_fee_sats: whole(miner, "lockup")?,
        };
        if terms.min_sats == 0 || terms.min_sats > terms.max_sats {
            return Err(Refused::BadPairs("limits"));
        }
        Ok(Self { terms, read_at })
    }

    /// The terms, whatever their age. Use [`Pairs::fresh`] to act on them.
    pub fn terms(&self) -> &Terms {
        &self.terms
    }

    /// The time the reply was read, in seconds since 1970.
    pub fn read_at(&self) -> u64 {
        self.read_at
    }

    /// SW-63: the terms, if they were read at most `PAIRS_FRESH_SECS` before
    /// `now_secs`. Terms read at a later time than now are refused too:
    /// the clock has gone back.
    pub fn fresh(&self, now_secs: u64) -> Result<&Terms, Refused> {
        match now_secs.checked_sub(self.read_at) {
            Some(age) if age <= PAIRS_FRESH_SECS => Ok(&self.terms),
            _ => Err(Refused::PairsStale),
        }
    }

    /// SW-02, SW-62: the band of invoice amounts, the shop's own and the
    /// service's live one together: at least the larger minimum, at most
    /// the smaller maximum. Refused when the terms are stale or the two do
    /// not meet.
    pub fn band(&self, now_secs: u64) -> Result<(u64, u64), Refused> {
        let t = self.fresh(now_secs)?;
        let lo = MIN_INVOICE_SATS.max(t.min_sats);
        let hi = MAX_INVOICE_SATS.min(t.max_sats);
        if lo > hi {
            return Err(Refused::ServiceLimit {
                min: t.min_sats,
                max: t.max_sats,
            });
        }
        Ok((lo, hi))
    }
}

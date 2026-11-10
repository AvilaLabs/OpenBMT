//! The bundle: the books as a framed, strictly read byte string (BK-28).

use std::collections::BTreeMap;

use openbmt_core::identity::DeviceId;
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_counter::{Currency, Role, MAX_NAME_BYTES};

use crate::books::{Books, MAX_COUNTERS, MAX_LINES, MAX_PER_SALE, MAX_SALES};
use crate::date::MAX_UNIX;
use crate::model::{
    Basis, CashReturn, Disposal, DisposalKind, LatePayment, OverrideNote, PosMark, Sale, SaleId,
    SaleKey, SaleState,
};
use crate::value::{UnknownField, Value};

/// BK-28: magic of the bundle.
pub const MAGIC_BUNDLE: [u8; 4] = *b"OBMF";

/// Fewest bytes a sale takes (it is 111), for the count check.
const MIN_SALE: usize = 100;
const MIN_DISPOSAL: usize = 8 + 1 + 3 + 8 + 2;
const MIN_RETURN: usize = 8 + 36 + 8;

fn put_u32(out: &mut Vec<u8>, n: usize) {
    // Every list is bounded far below the u32 limit when it is built.
    out.extend_from_slice(&u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes());
}

fn put_value<T>(out: &mut Vec<u8>, v: &Value<T>, put: impl Fn(&mut Vec<u8>, &T)) {
    match v {
        Value::Known(x) => {
            out.push(1);
            put(out, x);
        }
        Value::Unknown(f) => {
            out.push(0);
            out.push(*f as u8);
        }
    }
}

fn get_value<'a, T>(
    r: &mut Reader<'a>,
    get: impl Fn(&mut Reader<'a>) -> Result<T, DecodeError>,
) -> Result<Value<T>, DecodeError> {
    if r.flag()? {
        Ok(Value::Known(get(r)?))
    } else {
        UnknownField::from_u8(r.u8()?)
            .map(Value::Unknown)
            .ok_or(DecodeError::NonCanonical(
                "an unknown field code is not known",
            ))
    }
}

fn put_text(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let bytes = bytes.get(..255).unwrap_or(bytes);
    out.push(u8::try_from(bytes.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(bytes);
}

fn get_text(r: &mut Reader<'_>) -> Result<String, DecodeError> {
    let len = usize::from(r.u8()?);
    std::str::from_utf8(r.take(len)?)
        .map(str::to_string)
        .map_err(|_| DecodeError::NonCanonical("a text is not UTF-8"))
}

fn get_time(r: &mut Reader<'_>) -> Result<u64, DecodeError> {
    let at = r.u64_le()?;
    if at > MAX_UNIX {
        return Err(DecodeError::NonCanonical("a time is not a date"));
    }
    Ok(at)
}

fn get_opt_time(r: &mut Reader<'_>) -> Result<Option<u64>, DecodeError> {
    let at = r.opt_u64()?;
    if at.is_some_and(|t| t > MAX_UNIX) {
        return Err(DecodeError::NonCanonical("a time is not a date"));
    }
    Ok(at)
}

fn get_currency(r: &mut Reader<'_>) -> Result<Currency, DecodeError> {
    Currency::new(r.array::<3>()?).ok_or(DecodeError::NonCanonical("a currency code is not valid"))
}

fn put_key(out: &mut Vec<u8>, key: &SaleKey) {
    out.extend_from_slice(&key.counter.to_bytes());
    out.extend_from_slice(&key.index.to_le_bytes());
}

fn get_key(r: &mut Reader<'_>) -> Result<SaleKey, DecodeError> {
    Ok(SaleKey {
        counter: r.device_id()?,
        index: r.u32_le()?,
    })
}

/// BK-23, BK-28: the counters and their high-water marks, in key order.
pub(crate) fn put_marks(out: &mut Vec<u8>, marks: &BTreeMap<DeviceId, Option<u32>>) {
    put_u32(out, marks.len());
    for (id, hw) in marks {
        out.extend_from_slice(&id.to_bytes());
        match hw {
            Some(i) => {
                out.push(1);
                out.extend_from_slice(&i.to_le_bytes());
            }
            None => out.push(0),
        }
    }
}

/// BK-23, BK-28: read the marks; the keys must be strictly ascending.
pub(crate) fn get_marks(
    r: &mut Reader<'_>,
) -> Result<BTreeMap<DeviceId, Option<u32>>, DecodeError> {
    let n = r.count(MAX_COUNTERS, 33)?;
    let mut out = BTreeMap::new();
    let mut last: Option<DeviceId> = None;
    for _ in 0..n {
        let id = r.device_id()?;
        if last.is_some_and(|l| l >= id) {
            return Err(DecodeError::NonCanonical("counters are not in order"));
        }
        last = Some(id);
        let hw = if r.flag()? { Some(r.u32_le()?) } else { None };
        out.insert(id, hw);
    }
    Ok(out)
}

fn put_sale(out: &mut Vec<u8>, s: &Sale) {
    put_key(out, &s.key);
    out.extend_from_slice(&s.opened_at.to_le_bytes());
    out.extend_from_slice(&s.amount_minor.to_le_bytes());
    out.extend_from_slice(&s.currency.code());
    out.extend_from_slice(&s.first_quote_sats.to_le_bytes());
    out.extend_from_slice(&s.quote_rate_minor_per_btc.to_le_bytes());
    put_value(out, &s.rate_source, |o, t| put_text(o, t));
    out.push(s.state as u8);
    out.push(u8::from(s.funds_seen));
    out.push(u8::from(s.reached_final));
    wire::put_opt_u64(out, s.first_funds_at);
    let put_u64 = |o: &mut Vec<u8>, v: &u64| o.extend_from_slice(&v.to_le_bytes());
    put_value(out, &s.sats_received, put_u64);
    put_value(out, &s.sales_tax_minor, put_u64);
    put_value(out, &s.value_at_first_seen_minor, put_u64);
    put_value(out, &s.address, |o, t| put_text(o, t));
    put_u32(out, s.late_payments.len());
    for p in &s.late_payments {
        out.extend_from_slice(&p.txid);
        out.extend_from_slice(&p.sats.to_le_bytes());
    }
    wire::put_opt_u64(out, s.at_risk_sats);
    out.extend_from_slice(&s.surplus_owed_sats.to_le_bytes());
    out.extend_from_slice(&s.dust_sats.to_le_bytes());
    put_u32(out, s.overrides.len());
    for o in &s.overrides {
        out.push(o.role as u8);
        put_text(out, &o.name);
        out.extend_from_slice(&o.sats.to_le_bytes());
    }
    out.push(s.pos as u8);
}

fn get_sale(r: &mut Reader<'_>) -> Result<Sale, DecodeError> {
    let key = get_key(r)?;
    let opened_at = get_time(r)?;
    let amount_minor = r.u64_le()?;
    let currency = get_currency(r)?;
    let first_quote_sats = r.u64_le()?;
    let quote_rate_minor_per_btc = r.u64_le()?;
    let rate_source = get_value(r, get_text)?;
    let state = SaleState::from_u8(r.u8()?)
        .ok_or(DecodeError::NonCanonical("a sale state is not known"))?;
    let funds_seen = r.flag()?;
    let reached_final = r.flag()?;
    let first_funds_at = get_opt_time(r)?;
    if funds_seen != first_funds_at.is_some() || (reached_final && !funds_seen) {
        return Err(DecodeError::NonCanonical("a sale's funds flags disagree"));
    }
    let sats_received = get_value(r, |r| r.u64_le())?;
    let sales_tax_minor = get_value(r, |r| r.u64_le())?;
    let value_at_first_seen_minor = get_value(r, |r| r.u64_le())?;
    let address = get_value(r, get_text)?;
    let n = r.count(MAX_PER_SALE, 40)?;
    let mut late_payments = Vec::with_capacity(n);
    for _ in 0..n {
        late_payments.push(LatePayment {
            txid: r.array::<32>()?,
            sats: r.u64_le()?,
        });
    }
    let at_risk_sats = r.opt_u64()?;
    let surplus_owed_sats = r.u64_le()?;
    let dust_sats = r.u64_le()?;
    let n = r.count(MAX_PER_SALE, 10)?;
    let mut overrides = Vec::with_capacity(n);
    for _ in 0..n {
        let role =
            Role::from_u8(r.u8()?).ok_or(DecodeError::NonCanonical("a role is not known"))?;
        let name = get_text(r)?;
        if name.is_empty() || name.len() > MAX_NAME_BYTES {
            return Err(DecodeError::NonCanonical("a name has a bad length"));
        }
        overrides.push(OverrideNote {
            role,
            name,
            sats: r.u64_le()?,
        });
    }
    let pos = match r.u8()? {
        0 => PosMark::NotRecorded,
        1 => PosMark::Paid,
        2 => PosMark::Unpaid,
        _ => return Err(DecodeError::NonCanonical("a POS mark is not known")),
    };
    Ok(Sale {
        key,
        id: SaleId::for_sale(&key),
        opened_at,
        amount_minor,
        currency,
        first_quote_sats,
        quote_rate_minor_per_btc,
        rate_source,
        state,
        funds_seen,
        reached_final,
        first_funds_at,
        sats_received,
        sales_tax_minor,
        value_at_first_seen_minor,
        address,
        late_payments,
        at_risk_sats,
        surplus_owed_sats,
        dust_sats,
        overrides,
        pos,
    })
}

fn put_disposal(out: &mut Vec<u8>, d: &Disposal) {
    out.extend_from_slice(&d.at.to_le_bytes());
    match d.kind {
        DisposalKind::Refund { sale } => {
            out.push(1);
            put_key(out, &sale);
        }
        DisposalKind::CashOut => out.push(2),
        DisposalKind::Fee => out.push(3),
    }
    out.extend_from_slice(&d.currency.code());
    out.extend_from_slice(&d.sats.to_le_bytes());
    wire::put_opt_u64(out, d.minor_value);
    wire::put_opt_u64(out, d.basis.map(|b| b.minor));
}

fn get_disposal(r: &mut Reader<'_>) -> Result<Disposal, DecodeError> {
    let at = get_time(r)?;
    let kind = match r.u8()? {
        1 => DisposalKind::Refund { sale: get_key(r)? },
        2 => DisposalKind::CashOut,
        3 => DisposalKind::Fee,
        _ => return Err(DecodeError::NonCanonical("a disposal kind is not known")),
    };
    Ok(Disposal {
        at,
        kind,
        currency: get_currency(r)?,
        sats: r.u64_le()?,
        minor_value: r.opt_u64()?,
        basis: r.opt_u64()?.map(|minor| Basis { minor }),
    })
}

impl Books {
    /// BK-28: the books as a framed bundle that [`Books::from_bundle`] reads
    /// back.
    pub fn to_bundle(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_BUNDLE, 1024);
        put_marks(&mut out, &self.high_water);
        put_u32(&mut out, self.sales.len());
        for s in &self.sales {
            put_sale(&mut out, s);
        }
        put_u32(&mut out, self.disposals.len());
        for d in &self.disposals {
            put_disposal(&mut out, d);
        }
        put_u32(&mut out, self.returns.len());
        for c in &self.returns {
            out.extend_from_slice(&c.at.to_le_bytes());
            put_key(&mut out, &c.sale);
            out.extend_from_slice(&c.minor.to_le_bytes());
        }
        out
    }

    /// BK-28, BK-30: read a bundle strictly. What it gives encodes back to
    /// the input.
    pub fn from_bundle(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_BUNDLE)?;
        let high_water = get_marks(&mut r)?;
        let n = r.count(MAX_SALES, MIN_SALE)?;
        let mut sales: Vec<Sale> = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            let sale = get_sale(&mut r)?;
            if sales.last().is_some_and(|p| p.key >= sale.key) {
                return Err(DecodeError::NonCanonical("sales are not in order"));
            }
            sales.push(sale);
        }
        let n = r.count(MAX_LINES, MIN_DISPOSAL)?;
        let mut disposals = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            let d = get_disposal(&mut r)?;
            if disposals.last().is_some_and(|p: &Disposal| p.at > d.at) {
                return Err(DecodeError::NonCanonical("disposals are not in order"));
            }
            disposals.push(d);
        }
        let n = r.count(MAX_LINES, MIN_RETURN)?;
        let mut returns = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            let c = CashReturn {
                at: get_time(&mut r)?,
                sale: get_key(&mut r)?,
                minor: r.u64_le()?,
            };
            if returns.last().is_some_and(|p: &CashReturn| p.at > c.at) {
                return Err(DecodeError::NonCanonical("returns are not in order"));
            }
            returns.push(c);
        }
        r.finish()?;
        // The marks must be the ones the sales give.
        let mut derived: BTreeMap<DeviceId, Option<u32>> =
            high_water.keys().map(|k| (*k, None)).collect();
        for s in &sales {
            let slot = derived
                .get_mut(&s.key.counter)
                .ok_or(DecodeError::NonCanonical("a sale's counter is not listed"))?;
            *slot = Some(slot.map_or(s.key.index, |h| h.max(s.key.index)));
        }
        if derived != high_water {
            return Err(DecodeError::NonCanonical(
                "a high-water mark is not the sales' highest index",
            ));
        }
        let books = Self {
            sales,
            disposals: Vec::new(),
            returns: Vec::new(),
            high_water,
        };
        books
            .with_disposals(disposals)
            .and_then(|b| b.with_cash_returns(returns))
            .map_err(|_| DecodeError::NonCanonical("a disposal or a return is not valid"))
    }
}
